//! A pane's look in a split tab: the **card**. With one pane the tab shows it
//! edge to edge; with two or more every pane is a card — a gap between and
//! around ([`GAP_PT`], the frame computation's: `split::Spacing`), corners
//! rounded and clipped ([`corner_pt`], concentric with the window's corner),
//! standing on the window's **ground** ([`Ground`], `Theme::ground_srgb`).
//! No line draws a card: it reads as a card by **depth**, light from above.
//! Its top edge catches the light, a dark theme's card has a faint sheen
//! under it, and its shadow is tucked under its foot instead of spreading
//! over the ground ([`Shade`]) — a shadow that spread muddied the ground on
//! black, and a frame line round every card was what read as plain. The
//! focused card is nearer: a deeper shadow, a warm light on a dark theme.
//! Nothing is veiled: all panes read at once (the veil is
//! `[appearance] dim_unfocused_splits`, the pane's own `DimOverlay`).
//!
//! **Three views, three jobs.** The corners are the pane's *own* backing
//! layer (`cornerRadius` + `masksToBounds`: the Metal child and every overlay
//! are clipped by one mask). The light on the card is a separate topmost
//! child, [`FrameBox`]: on the pane's layer it would sit *under* the Metal
//! view. The shadow and the dark cut round the card are outside it, so they
//! cannot be the pane's (its clip would cut them): one [`Shade`] for the
//! window, between the ground and the containers. None of them takes part
//! in hit testing, like the veil.
//!
//! **The slide** (one pane ↔ two, [`slide`]) plays on the layers and never on
//! the frames. The pane is laid out once, at its final frame, so the program
//! in it is resized once; what moves is a Core Animation transform from the
//! old rectangle's look to none. Animating the frame instead would resize the
//! pty on every tick (a `SIGWINCH` storm and a flickering `vim`) and would
//! pull the animation through the main thread. It asks for no GPU frame:
//! the compositor stretches the drawable that is already there, and the next
//! content frame replaces it at the final size.

use std::cell::{Cell, RefCell};
use std::ptr::NonNull;
use std::sync::OnceLock;

use block2::RcBlock;
use bt_core::Theme;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{
    AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send,
};
use objc2_app_kit::{
    NSAnimatablePropertyContainer, NSAnimationContext, NSBezierPath, NSColor, NSGradient,
    NSGraphicsContext, NSShadow, NSView, NSWindingRule,
};
use objc2_core_foundation::CGRect;
use objc2_foundation::{
    NSNumber, NSObjectProtocol, NSOperatingSystemVersion, NSPoint, NSProcessInfo, NSRect, NSSize,
    NSString, NSValue, ns_string,
};
use objc2_quartz_core::{
    CABasicAnimation, CACurrentMediaTime, CALayer, CAMediaTiming, CAMediaTimingFunction,
    CATransaction, CATransform3D, NSValueCATransform3DAdditions, kCAFillModeBackwards,
};

use crate::split::{self, Rect, Slide};

/// The gap between cards and around them, in points. A design constant: wide
/// enough that the ground reads between two cards, narrow enough that a split
/// keeps most of the room a divider would have taken.
pub(crate) const GAP_PT: f64 = 8.0;

/// The window's own corner radius, in points. AppKit has no public reading
/// of it, so it is measured: macOS 26 draws this app's window (a compact
/// toolbar over a full-size content view) with a continuous curve of
/// ~20.5 pt — fitted to the window's picture at 2× —; the releases before it
/// round windows at 10 pt (the known value, not measured here).
fn window_corner_pt() -> f64 {
    static RADIUS: OnceLock<f64> = OnceLock::new();
    *RADIUS.get_or_init(|| {
        let tahoe = NSOperatingSystemVersion {
            majorVersion: 26,
            minorVersion: 0,
            patchVersion: 0,
        };
        if NSProcessInfo::processInfo().isOperatingSystemAtLeastVersion(tahoe) {
            20.5
        } else {
            10.0
        }
    })
}

/// A card's corner radius, in points: concentric with the window's corner —
/// the window's radius less the gap around the cards — so the gap is as wide
/// at the window's corners as along its edges. A fixed 10 pt was a tighter
/// corner than macOS 26's window: the gap narrowed to ~4 pt there.
pub(crate) fn corner_pt() -> f64 {
    (window_corner_pt() - GAP_PT).max(0.0)
}

/// The slide's length, in seconds. A design constant, the canvas's 220 ms.
pub(crate) const SLIDE_SECS: f64 = 0.22;

/// The width of the light on a card's top edge and of the focused card's
/// warm ring, in points.
pub(crate) const FRAME_PT: f64 = 1.0;

/// How far down a dark card's sheen reaches, as a share of its height.
const SHEEN_SHARE: f64 = 0.26;

/// White, and the warm white of the focused card's light on a dark theme.
const WHITE: [u8; 3] = [255, 255, 255];
const WARM: [u8; 3] = [255, 246, 232];

fn srgb([r, g, b]: [u8; 3], alpha: f64) -> Retained<NSColor> {
    NSColor::colorWithSRGBRed_green_blue_alpha(
        f64::from(r) / 255.0,
        f64::from(g) / 255.0,
        f64::from(b) / 255.0,
        alpha,
    )
}

fn rounded(rect: NSRect, radius: f64) -> Retained<NSBezierPath> {
    let radius = radius.max(0.0);
    NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(rect, radius, radius)
}

fn grown(rect: NSRect, by: f64) -> NSRect {
    NSRect::new(
        NSPoint::new(rect.origin.x - by, rect.origin.y - by),
        NSSize::new(
            (rect.size.width + 2.0 * by).max(0.0),
            (rect.size.height + 2.0 * by).max(0.0),
        ),
    )
}

/// A move's slide: panes that change places settle in 200 ms, a touch quicker
/// than a pane count changing.
pub(crate) const MOVE_SECS: f64 = 0.20;

pub(crate) struct FrameIvars {
    /// Whether the theme is dark (`Theme::is_dark`): the sheen and the warm
    /// light are a dark theme's; a light card's top edge is plain white.
    dark: Cell<bool>,
    /// Whether the pane holds the focus: its light is warm.
    focused: Cell<bool>,
    /// Whether the pane is a card now.
    carded: Cell<bool>,
    /// Whether the pane is lifted for arranging ([`crate::arrange`]): every
    /// card reads as the focused one.
    raised: Cell<bool>,
}

define_class!(
    // SAFETY: NSView is designed for subclassing; FrameBox implements no
    // `Drop` and is born with NSView's `init`.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriFrameBox"]
    #[ivars = FrameIvars]
    pub(crate) struct FrameBox;

    unsafe impl NSObjectProtocol for FrameBox {}

    impl FrameBox {
        /// Never takes part in hit testing (the veil's rule): a click, a drag
        /// or a mouse report on the frame's pixel belongs to the pane under it.
        #[unsafe(method_id(hitTest:))]
        fn hit_test(&self, _point: NSPoint) -> Option<Retained<NSView>> {
            None
        }

        /// The light on the card, in bottom-up coordinates: the sheen under
        /// its top (dark), the light on its top edge, the focused card's warm
        /// ring (dark).
        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            let iv = self.ivars();
            let bounds = self.bounds();
            let radius = corner_pt();
            let (dark, lit) = (iv.dark.get(), iv.focused.get() || iv.raised.get());
            NSGraphicsContext::saveGraphicsState_class();
            rounded(bounds, radius).addClip();
            if dark {
                let (tone, alpha) = if lit { (WARM, 0.07) } else { (WHITE, 0.05) };
                let height = bounds.size.height * SHEEN_SHARE;
                let band = NSRect::new(
                    NSPoint::new(bounds.origin.x, bounds.origin.y + bounds.size.height - height),
                    NSSize::new(bounds.size.width, height),
                );
                let sheen = NSGradient::initWithStartingColor_endingColor(
                    NSGradient::alloc(),
                    &srgb(tone, alpha),
                    &srgb(tone, 0.0),
                );
                if let Some(sheen) = sheen {
                    sheen.drawInRect_angle(band, -90.0);
                }
            }
            // The top edge's light: the card less itself moved one line down,
            // so it thins out round the top corners the way light does.
            let edge = rounded(bounds, radius);
            let below = NSRect::new(
                NSPoint::new(bounds.origin.x, bounds.origin.y - FRAME_PT),
                bounds.size,
            );
            edge.appendBezierPath(&rounded(below, radius));
            edge.setWindingRule(NSWindingRule::EvenOdd);
            let (tone, alpha) = match (dark, lit) {
                (true, true) => (WARM, 0.32),
                (true, false) => (WHITE, 0.13),
                (false, _) => (WHITE, 1.0),
            };
            srgb(tone, alpha).setFill();
            edge.fill();
            if dark && lit {
                let half = FRAME_PT / 2.0;
                let ring = rounded(grown(bounds, -half), radius - half);
                ring.setLineWidth(FRAME_PT);
                srgb(WARM, 0.08).setStroke();
                ring.stroke();
            }
            NSGraphicsContext::restoreGraphicsState_class();
        }
    }
);

impl FrameBox {
    /// Born invisible: a pane is no card until its container says so.
    pub(crate) fn new(mtm: MainThreadMarker, theme: &Theme) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(FrameIvars {
            dark: Cell::new(theme.is_dark()),
            focused: Cell::new(false),
            carded: Cell::new(false),
            raised: Cell::new(false),
        });
        // SAFETY: `NSView`'s `init`; the ivars are set.
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        this.setAlphaValue(0.0);
        this
    }

    /// The theme's lightness: a dark card's light is warm and has a sheen.
    pub(crate) fn paint(&self, theme: &Theme) {
        if self.ivars().dark.replace(theme.is_dark()) != theme.is_dark() {
            self.setNeedsDisplay(true);
        }
    }

    /// Whether the pane holds the focus: its light is warm.
    pub(crate) fn set_focused(&self, focused: bool) {
        if self.ivars().focused.replace(focused) != focused {
            self.setNeedsDisplay(true);
        }
    }

    pub(crate) fn focused(&self) -> bool {
        self.ivars().focused.get()
    }

    pub(crate) fn carded(&self) -> bool {
        self.ivars().carded.get()
    }

    /// The pane is lifted for arranging, or set down: while lifted every
    /// card's light reads as the focused one's.
    pub(crate) fn set_raised(&self, raised: bool) {
        if self.ivars().raised.replace(raised) != raised {
            self.setNeedsDisplay(true);
        }
    }

    /// Makes the pane a card, or not. `true` if the answer changed.
    ///
    /// Leaving the card only turns the light **invisible**; what it drew stays
    /// drawn, so a slide can fade it out instead of cutting it.
    pub(crate) fn set_carded(&self, carded: bool) -> bool {
        self.setAlphaValue(if carded { 1.0 } else { 0.0 });
        self.ivars().carded.replace(carded) != carded
    }
}

/// Rounds and clips the pane's own layer, or gives the corners back.
pub(crate) fn round(layer: &CALayer, carded: bool) {
    layer.setCornerRadius(if carded { corner_pt() } else { 0.0 });
    layer.setMasksToBounds(carded);
}

/// The matrix of a [`Slide`], about the layer's anchor: scale, then shift.
/// `mirrored` reads the vertical axis the other way up.
fn matrix(slide: Slide, layer: &CALayer, to: Rect, mirrored: bool) -> CATransform3D {
    // A layer scales about its anchor point, which is not always the centre
    // (`Slide` is about the centre): the shift makes up for the difference.
    // The centre of the pane stands `(0.5 - anchor)` of its size from the
    // pivot, and a scale by `s` moves it by `(1 - s)` of that.
    let anchor = layer.anchorPoint();
    let (anchor_y, direction) = if mirrored {
        (1.0 - anchor.y, -1.0)
    } else {
        (anchor.y, 1.0)
    };
    let dx = slide.dx + (1.0 - slide.scale_x) * (0.5 - anchor.x) * to.width;
    let dy = direction * slide.dy + (1.0 - slide.scale_y) * (0.5 - anchor_y) * to.height;
    CATransform3D {
        m11: slide.scale_x,
        m12: 0.0,
        m13: 0.0,
        m14: 0.0,
        m21: 0.0,
        m22: slide.scale_y,
        m23: 0.0,
        m24: 0.0,
        m31: 0.0,
        m32: 0.0,
        m33: 1.0,
        m34: 0.0,
        m41: dx,
        m42: dy,
        m43: 0.0,
        m44: 1.0,
    }
}

/// The matrix that makes `layer` look as if it stood at `from` while its model
/// frame is `to`, **checked against the layer itself**: a layer-backed view's
/// layer has its own anchor point, its own flip, and which way is "down" for
/// its transform is the layer's answer, not an assumption. The candidate is
/// set as the model transform for a moment (actions disabled, so nothing
/// animates and nothing is drawn: it is undone before the transaction
/// commits) and the frame Core Animation derives from it is compared with
/// `from`. `None` if no reading of the axes lands on `from` — then there is
/// no slide, which beats a slide that jumps.
fn fitted(layer: &CALayer, from: Rect, to: Rect) -> Option<CATransform3D> {
    let slide = split::slide(from, to);
    let lands = |frame: CGRect| {
        (frame.origin.x - from.x).abs() < 0.5
            && (frame.origin.y - from.y).abs() < 0.5
            && (frame.size.width - from.width).abs() < 0.5
            && (frame.size.height - from.height).abs() < 0.5
    };
    CATransaction::begin();
    CATransaction::setDisableActions(true);
    let found = [false, true].into_iter().find_map(|mirrored| {
        let candidate = matrix(slide, layer, to, mirrored);
        layer.setTransform(candidate);
        lands(layer.frame()).then_some(candidate)
    });
    layer.setTransform(IDENTITY);
    CATransaction::commit();
    found
}

const IDENTITY: CATransform3D = CATransform3D {
    m11: 1.0,
    m12: 0.0,
    m13: 0.0,
    m14: 0.0,
    m21: 0.0,
    m22: 1.0,
    m23: 0.0,
    m24: 0.0,
    m31: 0.0,
    m32: 0.0,
    m33: 1.0,
    m34: 0.0,
    m41: 0.0,
    m42: 0.0,
    m43: 0.0,
    m44: 1.0,
};

/// One explicit animation of `key_path` on `layer`, from `from` to `to`, over
/// `secs`. The layer's model value is already the end of it, and the animation
/// removes itself on completion: there is nothing to clean up.
fn play_for(
    layer: &CALayer,
    key_path: &NSString,
    from: &AnyObject,
    to: &AnyObject,
    key: &NSString,
    secs: f64,
) {
    let animation = CABasicAnimation::animationWithKeyPath(Some(key_path));
    // SAFETY: both values are the objects the key path's property takes
    // (an `NSValue` of a `CATransform3D`, an `NSNumber`).
    unsafe {
        animation.setFromValue(Some(from));
        animation.setToValue(Some(to));
    }
    animation.setDuration(secs);
    let curve = CAMediaTimingFunction::functionWithControlPoints(0.2, 0.8, 0.2, 1.0);
    animation.setTimingFunction(Some(&curve));
    layer.addAnimation_forKey(&animation, Some(key));
}

/// Sets `layer` — a view's backing layer, the view standing at `frame` — to
/// look shrunk by `scale` about its centre (`1.0` is rest), over `secs` (at
/// once for zero). The model value is the end at once; a running lift is
/// continued from where it is on screen. The matrix is [`fitted`]'s, checked
/// against the layer: no reading of its axes that lands on the shrunk
/// rectangle, no lift.
pub(crate) fn lift(layer: &CALayer, frame: Rect, scale: f64, secs: f64) {
    // SAFETY: reading the presentation copy of a layer we hold, on the main thread.
    let shown = unsafe { layer.presentationLayer() };
    let from = shown.map_or_else(|| layer.transform(), |shown| shown.transform());
    let target = if scale == 1.0 {
        Some(IDENTITY)
    } else {
        let (width, height) = (frame.width * scale, frame.height * scale);
        let look = Rect::new(
            frame.x + (frame.width - width) / 2.0,
            frame.y + (frame.height - height) / 2.0,
            width,
            height,
        );
        fitted(layer, look, frame)
    };
    let Some(target) = target else {
        return;
    };
    CATransaction::begin();
    CATransaction::setDisableActions(true);
    layer.setTransform(target);
    CATransaction::commit();
    if secs <= 0.0 {
        layer.removeAnimationForKey(ns_string!("bateri.lift"));
        return;
    }
    // SAFETY: plain `NSValue`s around `CATransform3D` values.
    let (from, to) = unsafe {
        (
            NSValue::valueWithCATransform3D(from),
            NSValue::valueWithCATransform3D(target),
        )
    };
    play_for(
        layer,
        ns_string!("transform"),
        &from,
        &to,
        ns_string!("bateri.lift"),
        secs,
    );
}

/// Settles whatever slide is still running on the pane's layers: the model
/// values are the end of every slide, so dropping the animations is the end.
pub(crate) fn settle(pane: &NSView, frame: &NSView) {
    for view in [pane, frame] {
        if let Some(layer) = view.layer() {
            layer.removeAllAnimations();
        }
    }
}

/// What changed for a pane across one layout: where it stood and where it
/// stands, and whether it was a card.
pub(crate) struct Change {
    /// The pane's rectangle before, in the container's coordinates.
    pub from: Rect,
    /// The pane's final frame.
    pub to: Rect,
    pub was_card: bool,
    pub is_card: bool,
}

/// Plays the slide of one pane over `secs`: the pane's layer from its old look to none,
/// and — if the pane gained or lost its card — the corners and the frame with
/// it. The model values are already final ([`round`], [`FrameBox::set_carded`]).
pub(crate) fn slide(pane: &NSView, frame: &NSView, change: &Change, secs: f64) {
    let Some(layer) = pane.layer() else {
        return;
    };
    if let Some(start) = fitted(&layer, change.from, change.to) {
        // SAFETY: a plain `NSValue` around a `CATransform3D` value, nothing
        // retained or borrowed.
        let (from, to) = unsafe {
            (
                NSValue::valueWithCATransform3D(start),
                NSValue::valueWithCATransform3D(IDENTITY),
            )
        };
        play_for(
            &layer,
            ns_string!("transform"),
            &from,
            &to,
            ns_string!("bateri.slide"),
            secs,
        );
    }
    if change.was_card == change.is_card {
        return;
    }
    // A card that stops being one keeps its clip until the next layout, so its
    // corners are still round while they open out; the radius is already 0.
    layer.setMasksToBounds(true);
    let corners = |card: bool| NSNumber::numberWithDouble(if card { corner_pt() } else { 0.0 });
    play_for(
        &layer,
        ns_string!("cornerRadius"),
        corners(change.was_card).as_ref(),
        corners(change.is_card).as_ref(),
        ns_string!("bateri.slide.corners"),
        secs,
    );
    if let Some(frame_layer) = frame.layer() {
        let opacity = |card: bool| NSNumber::numberWithDouble(if card { 1.0 } else { 0.0 });
        play_for(
            &frame_layer,
            ns_string!("opacity"),
            opacity(change.was_card).as_ref(),
            opacity(change.is_card).as_ref(),
            ns_string!("bateri.slide.frame"),
            secs,
        );
    }
}

// ─── The ground under the cards ──────────────────────────────────────────

pub(crate) struct GroundIvars {
    /// `Theme::ground_srgb`'s two stops; `None` until painted.
    stops: Cell<Option<[[u8; 3]; 2]>>,
    /// Whether it is meant to show: the tab on screen is split into cards.
    shown: Cell<bool>,
}

define_class!(
    // SAFETY: NSView is designed for subclassing; Ground implements no `Drop`
    // and is born with `initWithFrame:`.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriGround"]
    #[ivars = GroundIvars]
    pub(crate) struct Ground;

    unsafe impl NSObjectProtocol for Ground {}

    impl Ground {
        /// Never takes a press: the title row's drag belongs to the bar's
        /// claim above it, the gaps' to the split's handles.
        #[unsafe(method_id(hitTest:))]
        fn hit_test(&self, _point: NSPoint) -> Option<Retained<NSView>> {
            None
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            let Some([start, end]) = self.ivars().stops.get() else {
                return;
            };
            let color = |[r, g, b]: [u8; 3]| {
                NSColor::colorWithSRGBRed_green_blue_alpha(
                    f64::from(r) / 255.0,
                    f64::from(g) / 255.0,
                    f64::from(b) / 255.0,
                    1.0,
                )
            };
            let gradient = NSGradient::initWithStartingColor_endingColor(
                NSGradient::alloc(),
                &color(start),
                &color(end),
            );
            if let Some(gradient) = gradient {
                // Bottom-up coordinates: -45° runs from the top left to the
                // bottom right.
                gradient.drawInRect_angle(self.bounds(), -45.0);
            }
        }
    }
);

impl Ground {
    /// The window's ground: the whole root view, under the title row and the
    /// containers; born hidden, a window opens on a tab of one pane.
    pub(crate) fn new(mtm: MainThreadMarker, frame: NSRect) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(GroundIvars {
            stops: Cell::new(None),
            shown: Cell::new(false),
        });
        // SAFETY: `initWithFrame:` is NSView's designated initializer and the
        // ivars are set.
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: frame] };
        this.setWantsLayer(true);
        this.setAlphaValue(0.0);
        this
    }

    /// The theme's ground (`Theme::ground_srgb`); redrawn only when it changed.
    pub(crate) fn paint(&self, theme: &Theme) {
        let stops = Some(theme.ground_srgb());
        if self.ivars().stops.replace(stops) != stops {
            self.setNeedsDisplay(true);
        }
    }

    /// Shows the ground (the tab on screen is split) or gives the title row
    /// back to the background (one pane, or a zoomed one). A change fades over
    /// the slide's length, the same 220 ms the panes take to become cards, so
    /// switching between a split tab and a single one does not flash the
    /// title row; `at_once` under Reduce Motion or off screen.
    pub(crate) fn show(&self, shown: bool, at_once: bool) {
        if self.ivars().shown.replace(shown) == shown {
            return;
        }
        let alpha = if shown { 1.0 } else { 0.0 };
        if at_once {
            self.setAlphaValue(alpha);
            return;
        }
        let this = self.retain();
        animate(
            SLIDE_SECS,
            move || this.animator().setAlphaValue(alpha),
            || {},
        );
    }
}

/// Runs `change` as an AppKit animation of `secs` on the app's curve
/// (`cubic-bezier(0.2, 0.8, 0.2, 1)`), then `done`.
pub(crate) fn animate(secs: f64, change: impl Fn() + 'static, done: impl Fn() + 'static) {
    let changes = RcBlock::new(move |context: NonNull<NSAnimationContext>| {
        // SAFETY: AppKit gives the block a live context, for the block's duration.
        let context = unsafe { context.as_ref() };
        context.setDuration(secs);
        let curve = CAMediaTimingFunction::functionWithControlPoints(0.2, 0.8, 0.2, 1.0);
        context.setTimingFunction(Some(&curve));
        change();
    });
    let finished = RcBlock::new(done);
    NSAnimationContext::runAnimationGroup_completionHandler(&changes, Some(&finished));
}

// ─── The shade under the cards ───────────────────────────────────────────

/// How a card stands off the ground: the shadow under its foot (`offset`
/// down, `blur`, cast by a fill `spread` points inside the card, so it shows
/// under the foot and not round the sides) and the cut round it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Footing {
    pub offset: f64,
    pub blur: f64,
    pub spread: f64,
    /// The shadow's tone and opacity.
    pub shadow: ([u8; 3], f64),
    /// The cut's width in points, its tone and opacity.
    pub cut: (f64, [u8; 3], f64),
}

/// A card's footing: nearer when it holds the focus. On a dark theme the cut
/// is black, a clean edge on the lighter ground; on a light one a faint line,
/// the shadow doing most of the work.
pub(crate) fn footing(dark: bool, focused: bool) -> Footing {
    const BLACK: [u8; 3] = [0, 0, 0];
    const INK: [u8; 3] = [30, 32, 40];
    match (dark, focused) {
        (true, false) => Footing {
            offset: 9.0,
            blur: 16.0,
            spread: 8.0,
            shadow: (BLACK, 0.75),
            cut: (1.0, BLACK, 0.55),
        },
        (true, true) => Footing {
            offset: 14.0,
            blur: 22.0,
            spread: 10.0,
            shadow: (BLACK, 0.9),
            cut: (1.0, BLACK, 0.6),
        },
        (false, false) => Footing {
            offset: 6.0,
            blur: 12.0,
            spread: 6.0,
            shadow: (INK, 0.18),
            cut: (0.5, INK, 0.10),
        },
        (false, true) => Footing {
            offset: 14.0,
            blur: 24.0,
            spread: 10.0,
            shadow: (INK, 0.30),
            cut: (0.5, INK, 0.12),
        },
    }
}

/// Draws `footing` for a card at `rect` (bottom-up coordinates): its shadow,
/// cast by a fill the card hides, then the cut just outside it.
pub(crate) fn draw_footing(rect: NSRect, footing: Footing) {
    let radius = corner_pt();
    NSGraphicsContext::saveGraphicsState_class();
    let shadow = NSShadow::new();
    shadow.setShadowOffset(NSSize::new(0.0, -footing.offset));
    shadow.setShadowBlurRadius(footing.blur);
    shadow.setShadowColor(Some(&srgb(footing.shadow.0, footing.shadow.1)));
    shadow.set();
    srgb([0, 0, 0], 1.0).setFill();
    rounded(grown(rect, -footing.spread), radius - footing.spread).fill();
    NSGraphicsContext::restoreGraphicsState_class();
    let (width, tone, alpha) = footing.cut;
    srgb(tone, alpha).setFill();
    rounded(grown(rect, width), radius + width).fill();
}

pub(crate) struct ShadeIvars {
    /// The cards on screen, in this view's coordinates, and whether each
    /// holds the focus.
    cards: RefCell<Vec<(NSRect, bool)>>,
    dark: Cell<bool>,
    /// The tab on screen is lifted for arranging: the lift's own plates
    /// stand under the shrunk panes and this shade would show round them.
    lifted: Cell<bool>,
}

define_class!(
    // SAFETY: NSView is designed for subclassing; Shade implements no `Drop`
    // and is born with `initWithFrame:`.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriShade"]
    #[ivars = ShadeIvars]
    pub(crate) struct Shade;

    unsafe impl NSObjectProtocol for Shade {}

    impl Shade {
        #[unsafe(method_id(hitTest:))]
        fn hit_test(&self, _point: NSPoint) -> Option<Retained<NSView>> {
            None
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            let iv = self.ivars();
            let dark = iv.dark.get();
            for (rect, focused) in iv.cards.borrow().iter() {
                draw_footing(*rect, footing(dark, *focused));
            }
        }
    }
);

impl Shade {
    pub(crate) fn new(mtm: MainThreadMarker, frame: NSRect) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(ShadeIvars {
            cards: RefCell::new(Vec::new()),
            dark: Cell::new(true),
            lifted: Cell::new(false),
        });
        // SAFETY: `initWithFrame:` is NSView's designated initializer and the
        // ivars are set.
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: frame] };
        this.setWantsLayer(true);
        this
    }

    /// The theme's lightness: which footing the cards stand on.
    pub(crate) fn paint(&self, theme: &Theme) {
        if self.ivars().dark.replace(theme.is_dark()) != theme.is_dark() {
            self.setNeedsDisplay(true);
        }
    }

    /// The cards on screen now; redrawn only when they changed.
    pub(crate) fn set_cards(&self, cards: Vec<(NSRect, bool)>) {
        if *self.ivars().cards.borrow() != cards {
            *self.ivars().cards.borrow_mut() = cards;
            self.setNeedsDisplay(true);
        }
    }

    /// The tab on screen is lifted, or set down.
    pub(crate) fn set_lifted(&self, lifted: bool) {
        if self.ivars().lifted.replace(lifted) == lifted {
            return;
        }
        if let Some(layer) = self.layer() {
            layer.removeAnimationForKey(ns_string!("bateri.hold"));
        }
        self.setAlphaValue(if lifted { 0.0 } else { 1.0 });
    }

    /// The cards are sliding for `secs`: the shade already stands at their
    /// final places, so it waits out the slide unseen and then fades in.
    /// Core Animation's own delay — a timer would have to find this view
    /// again.
    pub(crate) fn hold(&self, secs: f64) {
        if self.ivars().lifted.get() {
            return;
        }
        let Some(layer) = self.layer() else {
            return;
        };
        self.setAlphaValue(1.0);
        let fade = CABasicAnimation::animationWithKeyPath(Some(ns_string!("opacity")));
        // SAFETY: plain `NSNumber`s, what `opacity` takes.
        unsafe {
            fade.setFromValue(Some(&NSNumber::numberWithDouble(0.0)));
            fade.setToValue(Some(&NSNumber::numberWithDouble(1.0)));
        }
        fade.setBeginTime(layer.convertTime_fromLayer(CACurrentMediaTime(), None) + secs);
        fade.setDuration(HOLD_FADE_SECS);
        // SAFETY: a constant `NSString` Core Animation exposes, only read.
        fade.setFillMode(unsafe { kCAFillModeBackwards });
        layer.addAnimation_forKey(&fade, Some(ns_string!("bateri.hold")));
    }
}

/// How long the shade takes to come back after a slide.
const HOLD_FADE_SECS: f64 = 0.15;
