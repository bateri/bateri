//! A pane's look under `[appearance] split_style = "cards"`: the **card**.
//! Every pane of every tab is one, a tab's only pane too — a gap between and
//! around ([`GAP_PT`], the frame computation's: `split::Spacing`), corners
//! rounded and clipped ([`corner_pt`], concentric with the window's corner),
//! standing on the window's **ground** ([`Ground`], `Theme::ground_srgb`, one
//! flat tone).
//! No line draws a card: it reads as a card by **depth**, light from above.
//! Its top edge catches the light, a dark theme's card has a faint sheen
//! under it, and its shadow is tucked under its foot instead of spreading
//! over the ground ([`Shade`]) — a shadow that spread muddied the ground on
//! black, and a frame line round every card was what read as plain. The
//! focused card is nearer: a deeper shadow, a warm light on a dark theme.
//! Nothing is veiled: all panes read at once (the veil is
//! `[appearance] dim_unfocused_splits`, the pane's own `DimOverlay`). With
//! `[appearance] focus_frame` the focused pane of a split tab also has a line
//! in the accent round its card ([`FocusRing`]), under either style, which
//! slides to the next card when the focus moves.
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
use std::ptr::{NonNull, null_mut};
use std::sync::OnceLock;

use block2::RcBlock;
use bt_core::Theme;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool};
use objc2::{
    AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send,
};
use objc2_app_kit::{
    NSAnimatablePropertyContainer, NSAnimationContext, NSBezierPath, NSBitmapImageRep, NSColor,
    NSCompositingOperation, NSDeviceRGBColorSpace, NSGradient, NSGraphicsContext, NSImage, NSView,
    NSWindingRule, NSWindow, NSWindowStyleMask,
};
use objc2_core_foundation::CGRect;
use objc2_core_graphics::{CGContext, CGImage};
use objc2_foundation::{
    NSNumber, NSObjectProtocol, NSOperatingSystemVersion, NSPoint, NSProcessInfo, NSRect, NSSize,
    NSString, NSValue, ns_string,
};
use objc2_quartz_core::{
    CABasicAnimation, CACornerMask, CACurrentMediaTime, CALayer, CAMediaTiming,
    CAMediaTimingFunction, CATransaction, CATransform3D, NSValueCATransform3DAdditions,
    kCAFillModeBackwards,
};

use crate::split::{self, Rect, Slide};

/// The gap between cards and around them, in points. A design constant: wide
/// enough that the ground reads between two cards, narrow enough that a split
/// keeps most of the room a divider would have taken.
pub(crate) const GAP_PT: f64 = 8.0;

/// The radius, in points, of the window's bottom corners **now**: the
/// windowed corner ([`windowed_corner_pt`]), or square in full screen. The one
/// owner of the window's corner — the card's corner ([`corner_pt`]) and the
/// text's distance from the curve (`CellMetrics::with_corners`, told through
/// `TerminalPane::set_corners`) both come from here. `None`, no window yet,
/// is the windowed corner: the window a tab is about to stand in.
pub(crate) fn window_corner_pt(window: Option<&NSWindow>) -> f64 {
    if window.is_some_and(|window| window.styleMask().contains(NSWindowStyleMask::FullScreen)) {
        0.0
    } else {
        windowed_corner_pt()
    }
}

/// The window's own corner radius, in points, in its windowed state. AppKit
/// has no public reading of it, so it is measured: macOS 26 draws this app's
/// window (a compact toolbar over a full-size content view) with a
/// continuous curve of ~20.5 pt — fitted to the window's picture at 2× —; the
/// releases before it round windows at 10 pt (the known value, not measured
/// here).
fn windowed_corner_pt() -> f64 {
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
/// corner than macOS 26's window: the gap narrowed to ~4 pt there. The
/// windowed corner on purpose: full screen has no curve to be concentric
/// with, and cards that changed their corner would change shape under a
/// layout that did not.
pub(crate) fn corner_pt() -> f64 {
    (windowed_corner_pt() - GAP_PT).max(0.0)
}

/// The height of a pane's header, points — every pane in view has one: the terminal's gutter
/// above and below a row's 18 pt line (`pane_head`). A design constant, the design's 34.
pub(crate) const HEAD_PT: f64 = 34.0;

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

pub(crate) fn grown(rect: NSRect, by: f64) -> NSRect {
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
            let (dark, lit) = (iv.dark.get(), iv.focused.get());
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

    /// Makes the pane a card, or not. `true` if the answer changed.
    ///
    /// Leaving the card only turns the light **invisible**; what it drew stays
    /// drawn, so a slide can fade it out instead of cutting it.
    pub(crate) fn set_carded(&self, carded: bool) -> bool {
        self.setAlphaValue(if carded { 1.0 } else { 0.0 });
        self.ivars().carded.replace(carded) != carded
    }
}

// ─── The focus frame ─────────────────────────────────────────────────────

/// The focus frame's width, in points (`[appearance] focus_frame`).
const FOCUS_PT: f64 = 2.0;

pub(crate) struct RingIvars {
    /// The pane whose card the line stands round; `None` while it is hidden.
    on: Cell<Option<u64>>,
}

define_class!(
    /// The focus frame (`[appearance] focus_frame`): one line in the accent round the focused
    /// card of a split tab, its header and all, under either style. It is the container's and not
    /// a pane's, so moving the focus **slides** it from one card to the next ([`MOVE_SECS`])
    /// instead of putting one out and lighting another. It is its layer's border: Core Animation
    /// draws it at every size on the way, so its corners stay round through a slide and from the
    /// first frame — a drawn line would be a picture stretched from the size it was drawn at.
    // SAFETY: NSView is designed for subclassing; FocusRing implements no `Drop` and is born
    // with NSView's `init`.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriFocusRing"]
    #[ivars = RingIvars]
    pub(crate) struct FocusRing;

    unsafe impl NSObjectProtocol for FocusRing {}

    impl FocusRing {
        /// Never takes part in hit testing (the veil's rule): a click on the
        /// line belongs to the pane under it.
        #[unsafe(method_id(hitTest:))]
        fn hit_test(&self, _point: NSPoint) -> Option<Retained<NSView>> {
            None
        }
    }
);

impl FocusRing {
    /// Born hidden: the container stands it round the focused card of a split tab.
    pub(crate) fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(RingIvars {
            on: Cell::new(None),
        });
        // SAFETY: `NSView`'s `init`; the ivars are set.
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        this.setWantsLayer(true);
        if let Some(layer) = this.layer() {
            layer.setBorderWidth(FOCUS_PT);
        }
        this.setHidden(true);
        this
    }

    /// Stands the line round `frame` (the container's space) of pane `pane`'s card, with the
    /// card's corners `(top, bottom left, bottom right)`, in `theme`'s frame colour; `None` hides
    /// it. A move to another pane's card slides there unless `still` (Reduce Motion); the same
    /// card laid out again is followed at once.
    pub(crate) fn place(
        &self,
        at: Option<(u64, NSRect, (f64, f64, f64))>,
        theme: &Theme,
        still: bool,
    ) {
        let Some(layer) = self.layer() else {
            return;
        };
        let Some((pane, frame, (top, left, right))) = at else {
            self.ivars().on.set(None);
            self.setHidden(true);
            return;
        };
        layer.setBorderColor(Some(&srgb(theme.focus_frame_srgb(), 1.0).CGColor()));
        // One radius for the corners that have one: a card's are all alike, a lined pane's are
        // the window's curve where it meets the window's bottom and square elsewhere. The layer
        // is not flipped: its minimum y is the bottom.
        let radius = top.max(left).max(right);
        let mut corners = CACornerMask::empty();
        for (round, corner) in [
            (
                top,
                CACornerMask::LayerMinXMaxYCorner | CACornerMask::LayerMaxXMaxYCorner,
            ),
            (left, CACornerMask::LayerMinXMinYCorner),
            (right, CACornerMask::LayerMaxXMinYCorner),
        ] {
            if round > 0.0 {
                corners |= corner;
            }
        }
        layer.setCornerRadius(radius);
        layer.setMaskedCorners(corners);
        let shown = !self.isHidden();
        let moved = self.ivars().on.replace(Some(pane)) != Some(pane);
        self.setHidden(false);
        if self.frame() == frame {
            return;
        }
        if shown && moved && !still {
            let this = self.retain();
            animate(MOVE_SECS, move || this.animator().setFrame(frame), || {});
        } else {
            // A slide on its way would carry on to its old place.
            layer.removeAllAnimations();
            self.setFrame(frame);
        }
    }
}

/// Rounds and clips the pane's own layer, or gives the corners back. Under a header (a split
/// tab's pane) only the bottom corners are the pane's: the header takes the top ones.
pub(crate) fn round(layer: &CALayer, carded: bool, headed: bool) {
    layer.setCornerRadius(if carded { corner_pt() } else { 0.0 });
    layer.setMasksToBounds(carded);
    // The pane's layer is not flipped: its minimum y is the bottom.
    let bottom = CACornerMask::LayerMinXMinYCorner | CACornerMask::LayerMaxXMinYCorner;
    let all = bottom | CACornerMask::LayerMinXMaxYCorner | CACornerMask::LayerMaxXMaxYCorner;
    layer.setMaskedCorners(if headed { bottom } else { all });
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
pub(crate) fn fitted(layer: &CALayer, from: Rect, to: Rect) -> Option<CATransform3D> {
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

pub(crate) const IDENTITY: CATransform3D = CATransform3D {
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
    /// `Theme::ground_srgb`; `None` until painted.
    tone: Cell<Option<[u8; 3]>>,
    /// Whether it is meant to show: the tab on screen is laid out as cards.
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
            let Some(tone) = self.ivars().tone.get() else {
                return;
            };
            srgb(tone, 1.0).setFill();
            NSBezierPath::fillRect(self.bounds());
        }
    }
);

impl Ground {
    /// The window's ground: the whole root view, under the title row and the
    /// containers; born hidden, a window opens on a tab of one pane.
    pub(crate) fn new(mtm: MainThreadMarker, frame: NSRect) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(GroundIvars {
            tone: Cell::new(None),
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
        let tone = Some(theme.ground_srgb());
        if self.ivars().tone.replace(tone) != tone {
            self.setNeedsDisplay(true);
        }
    }

    /// Shows the ground (the tab on screen is laid out as cards) or gives the
    /// title row back to the background (panes divided by a line). A change
    /// fades over the slide's length, the same 220 ms the panes take to become
    /// cards, so the title row does not flash; `at_once` under Reduce Motion
    /// or off screen.
    pub(crate) fn show(&self, shown: bool, at_once: bool) {
        if self.ivars().shown.replace(shown) == shown {
            return;
        }
        fade_to(self, shown, at_once);
    }
}

/// Fades `view` in (`shown`) or out over the slide's length — the 220 ms the panes take to
/// become cards — or at once. The ground and the line the side column leaves when the ground
/// comes ([`crate::window::RootView::sync_ground`]) take it together, so the one that goes and
/// the one that comes cross at the same pace.
pub(crate) fn fade_to(view: &NSView, shown: bool, at_once: bool) {
    let alpha = if shown { 1.0 } else { 0.0 };
    if at_once {
        view.setAlphaValue(alpha);
        return;
    }
    let view = view.retain();
    animate(
        SLIDE_SECS,
        move || view.animator().setAlphaValue(alpha),
        || {},
    );
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
/// down, `blur`, cast by a fill `spread` points inside the card, so it falls
/// most under the foot and only a little round the sides) and the cut round
/// it. All of them come out in points: the picture's context takes the shadow's
/// numbers so at any scale (`draw_shadow`; read off a 2x window).
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

impl Footing {
    /// The stretch of the shadow's reach that runs out to nothing, as `(from, to)` points off the
    /// caster's edge: the last quarter of the blur, ending a little short of it. `None` for a blur
    /// too small to have one: there is no cut worth hiding, and fading would take the shadow with
    /// it.
    ///
    /// Core Graphics' blur is a Gaussian cut off at (just under) its radius, so a shadow ends on
    /// a faint step instead of dying away, and a step is what the eye finds along a long edge. It
    /// never showed while every card stood in a gap that ended under a neighbour or the window's
    /// edge, short of that cut; the side column is bare ground beside the first card. So the
    /// shadow's own tail is faded by hand before the cut can show.
    fn tail(&self) -> Option<(f64, f64)> {
        let to = self.blur - BLUR_SHORT_PT;
        let from = (to - self.blur / TAIL_SHARE).max(0.0);
        (to > from).then_some((from, to))
    }
}

/// How far short of its radius Core Graphics' blur stops painting, with a margin: the fade ends
/// here, so the cut falls in the part already cleared.
const BLUR_SHORT_PT: f64 = 1.5;

/// The tail is this share of the blur (a quarter): long enough to take the step out, short enough
/// to leave the shadow's body as cast.
const TAIL_SHARE: f64 = 4.0;

/// The spacing of the rings that clear a tail, in points: finer than a pixel at 2x.
const RING_PT: f64 = 0.25;

/// The clearing that runs `footing`'s shadow out: ring by ring, `(distance, alpha)` clears `alpha`
/// of whatever is left beyond `distance` off the caster's edge. What stays at a distance falls from
/// all to nothing along a smooth step across [`Footing::tail`]; the last ring clears the rest. No
/// rings where there is no tail.
fn tail_rings(footing: &Footing) -> Vec<(f64, f64)> {
    let Some((from, to)) = footing.tail() else {
        return Vec::new();
    };
    let count = (((to - from) / RING_PT).ceil() as usize).max(1);
    let mut rings = Vec::with_capacity(count);
    let mut before = 1.0;
    for ring in 1..=count {
        let t = ring as f64 / count as f64;
        let after = 1.0 - t * t * (3.0 - 2.0 * t);
        let alpha = if after > 0.0 {
            1.0 - after / before
        } else {
            1.0
        };
        rings.push((from + t * (to - from), alpha));
        before = after;
    }
    rings
}

/// Draws `footing` for a card at `rect` (bottom-up coordinates): its shadow
/// ([`draw_shadow`]), then the cut just outside the card.
pub(crate) fn draw_footing(rect: NSRect, footing: Footing) {
    draw_shadow(rect, footing);
    let radius = corner_pt();
    let (width, tone, alpha) = footing.cut;
    srgb(tone, alpha).setFill();
    rounded(grown(rect, width), radius + width).fill();
}

/// The corner radius of the fill that casts a card's shadow: the card's, less the spread it is
/// inset by. Never negative — the card's corner is smaller than the spread on the releases before
/// macOS 26 — because the rings round their corners by this radius plus their distance, and a
/// square-cornered caster casts a shadow as round as its distance, not less.
fn caster_radius(corner: f64, footing: &Footing) -> f64 {
    (corner - footing.spread).max(0.0)
}

/// The shadow of `footing` for a card at `rect`: cast by a fill the card hides and run out to
/// nothing at its tail ([`Footing::tail`]).
///
/// It is made in a picture of its own: clearing the tail takes a compositing operation that would
/// reach into whatever was drawn under it, if it were done there. A drawing-
/// handler image is made when it is drawn, at the destination's resolution, so the picture is as
/// sharp as the view it lands in; `the_shadows_picture_is_made_at_the_destinations_scale` holds
/// it to that, and to being a picture of its own.
fn draw_shadow(rect: NSRect, footing: Footing) {
    let caster_radius = caster_radius(corner_pt(), &footing);
    let caster = grown(rect, -footing.spread);
    // The picture holds the caster and all of the blur's reach, on whole points so its pixels
    // fall on the screen's.
    let reach = grown(caster, footing.offset + footing.blur + 1.0);
    let (left, bottom) = (reach.origin.x.floor(), reach.origin.y.floor());
    let area = NSRect::new(
        NSPoint::new(left, bottom),
        NSSize::new(
            (reach.origin.x + reach.size.width).ceil() - left,
            (reach.origin.y + reach.size.height).ceil() - bottom,
        ),
    );
    let paint = RcBlock::new(move |bounds: NSRect| -> Bool {
        let at = |rect: NSRect| {
            NSRect::new(
                NSPoint::new(rect.origin.x - area.origin.x, rect.origin.y - area.origin.y),
                rect.size,
            )
        };
        let caster = at(caster);
        NSGraphicsContext::saveGraphicsState_class();
        // The shadow is cast with the footing's own numbers and down the picture: `NSShadow`
        // itself casts the same offset down a view's backing store and up a bitmap, so only the
        // direction is taken from this drawing.
        if let Some(context) = NSGraphicsContext::currentContext() {
            let context = context.CGContext();
            let down =
                CGContext::convert_size_to_device_space(Some(&context), NSSize::new(0.0, -1.0))
                    .height
                    .signum();
            let tone = srgb(footing.shadow.0, footing.shadow.1).CGColor();
            CGContext::set_shadow_with_color(
                Some(&context),
                NSSize::new(0.0, down * footing.offset),
                footing.blur,
                Some(&tone),
            );
        }
        srgb([0, 0, 0], 1.0).setFill();
        rounded(caster, caster_radius).fill();
        NSGraphicsContext::restoreGraphicsState_class();
        // The blur is centred on the caster as the offset moved it.
        let centre = NSRect::new(
            NSPoint::new(caster.origin.x, caster.origin.y - footing.offset),
            caster.size,
        );
        NSGraphicsContext::saveGraphicsState_class();
        if let Some(context) = NSGraphicsContext::currentContext() {
            context.setCompositingOperation(NSCompositingOperation::DestinationOut);
        }
        for (distance, alpha) in tail_rings(&footing) {
            // Everything beyond this distance, the frame the picture's edge makes with it.
            let beyond = NSBezierPath::bezierPathWithRect(bounds);
            beyond.appendBezierPath(&rounded(grown(centre, distance), caster_radius + distance));
            beyond.setWindingRule(NSWindingRule::EvenOdd);
            srgb([0, 0, 0], alpha).setFill();
            beyond.fill();
        }
        NSGraphicsContext::restoreGraphicsState_class();
        Bool::YES
    });
    let picture = NSImage::imageWithSize_flipped_drawingHandler(area.size, false, &paint);
    picture.drawInRect_fromRect_operation_fraction(
        area,
        NSRect::ZERO,
        NSCompositingOperation::SourceOver,
        1.0,
    );
}

/// One card's footing as a picture Core Animation stretches over the card: [`draw_footing`]
/// drawn once round a card just big enough for every curve in it, and laid on a layer whose
/// `contentsCenter` stretches the middle, where nothing changes along the edges. A straight
/// edge's shadow, tail and cut are measured off that edge alone, so the stretched picture is the
/// one the card's own size would paint (`a_stretched_footing_is_what_the_cards_own_size_paints`)
/// — and a card that changes size moves a layer instead of drawing its shadow again.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Slices {
    /// How far the picture reaches past the card on every side, in whole points.
    margin: f64,
    /// The card the picture is drawn round, in points: on an axis where the card is too small for
    /// its curves, the card's own length, and nothing stretches along it.
    card: NSSize,
    /// What stretches, in the picture's unit square, bottom-up.
    center: CGRect,
}

/// The breadth of the stretching middle, in whole points so it stays on the screen's pixels.
const STRETCH_PT: f64 = 2.0;

impl Slices {
    fn of(footing: &Footing, corner: f64, card: NSSize) -> Self {
        // The shadow's picture reaches this far past its caster, the cut lies inside it. Its
        // offset and blur are pixels: read as points, as here, they only reach further.
        let margin = (footing.offset + footing.blur + 1.0 - footing.spread)
            .max(footing.cut.0)
            .ceil();
        let caster = caster_radius(corner, footing);
        // How far in from the card's edge the picture still changes along it: the caster's
        // corner (its edge `edge` in from the card's) and the blur round it — the tail is
        // inside the blur — and the cut's corner, the card's own. Whole points, with one to
        // spare.
        let reach = |edge: f64| ((edge + caster + footing.blur).max(corner) + 1.0).ceil();
        let side = reach(footing.spread);
        // The shadow is cast down by its offset: its curves reach further into the card from
        // the top than from the bottom.
        let below = reach(footing.spread - footing.offset);
        let above = reach(footing.spread + footing.offset);
        // A length, where along it the middle starts and how long the middle is.
        let axis = |length: f64, low: f64, high: f64| {
            let least = low + high + STRETCH_PT;
            if length > least {
                (least, low, STRETCH_PT)
            } else {
                (length, length / 2.0, 0.0)
            }
        };
        let (width, left, across) = axis(card.width, side, side);
        let (height, bottom, up) = axis(card.height, below, above);
        let (wide, high) = (width + 2.0 * margin, height + 2.0 * margin);
        Self {
            margin,
            card: NSSize::new(width, height),
            center: CGRect::new(
                NSPoint::new((margin + left) / wide, (margin + bottom) / high),
                NSSize::new(across / wide, up / high),
            ),
        }
    }

    /// The picture's size, in points.
    fn size(&self) -> NSSize {
        NSSize::new(
            self.card.width + 2.0 * self.margin,
            self.card.height + 2.0 * self.margin,
        )
    }
}

/// A picture of [`Slices`] at a scale, and what it was drawn for.
struct Picture {
    footing: Footing,
    corner: f64,
    card: NSSize,
    scale: f64,
    contents: Retained<CGImage>,
}

thread_local! {
    /// The pictures drawn last, newest first: the four footings at the screen's scale stay, the
    /// odd small card's own drawings fall off the end.
    static PICTURES: RefCell<Vec<Picture>> = const { RefCell::new(Vec::new()) };
}

/// How many pictures [`PICTURES`] keeps.
const PICTURES_KEPT: usize = 12;

/// The picture `slices` asks for, drawn by [`draw_footing`] at `scale`, from [`PICTURES`] when it
/// was drawn already.
fn footing_picture(
    footing: Footing,
    corner: f64,
    slices: &Slices,
    scale: f64,
) -> Option<Retained<CGImage>> {
    PICTURES.with(|kept| {
        let mut kept = kept.borrow_mut();
        let same = |picture: &Picture| {
            picture.footing == footing
                && picture.corner == corner
                && picture.card == slices.card
                && picture.scale == scale
        };
        if let Some(at) = kept.iter().position(same) {
            let picture = kept.remove(at);
            let contents = picture.contents.clone();
            kept.insert(0, picture);
            return Some(contents);
        }
        let card = NSRect::new(NSPoint::new(slices.margin, slices.margin), slices.card);
        let contents = draw_picture(slices.size(), scale, || draw_footing(card, footing))?;
        kept.insert(
            0,
            Picture {
                footing,
                corner,
                card: slices.card,
                scale,
                contents: contents.clone(),
            },
        );
        kept.truncate(PICTURES_KEPT);
        Some(contents)
    })
}

/// What `draw` paints in a transparent picture `size` points big, in pixels at `scale`, in
/// bottom-up coordinates: the bitmap the tests read back (`on_retina`), so the pixels a layer
/// shows are the ones they hold to the card's own drawing.
fn draw_picture(size: NSSize, scale: f64, draw: impl FnOnce()) -> Option<Retained<CGImage>> {
    let (wide, high) = ((size.width * scale).round(), (size.height * scale).round());
    // SAFETY: no planes of our own (the bitmap allocates them), 8-bit RGBA at the size given, and
    // a constant colour space name AppKit exposes.
    let rep = unsafe {
        NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bytesPerRow_bitsPerPixel(
            NSBitmapImageRep::alloc(),
            null_mut(),
            wide as isize,
            high as isize,
            8,
            4,
            true,
            false,
            NSDeviceRGBColorSpace,
            0,
            0,
        )
    }?;
    rep.setSize(size);
    let context = NSGraphicsContext::graphicsContextWithBitmapImageRep(&rep)?;
    NSGraphicsContext::saveGraphicsState_class();
    NSGraphicsContext::setCurrentContext(Some(&context));
    // Transparent to begin with: a new bitmap holds whatever was in the memory.
    context.setCompositingOperation(NSCompositingOperation::Clear);
    NSBezierPath::fillRect(NSRect::new(NSPoint::ZERO, size));
    context.setCompositingOperation(NSCompositingOperation::SourceOver);
    draw();
    context.flushGraphics();
    NSGraphicsContext::restoreGraphicsState_class();
    rep.CGImage()
}

pub(crate) struct ShadeIvars {
    /// The cards on screen, in this view's coordinates, and whether each
    /// holds the focus.
    cards: RefCell<Vec<(NSRect, bool)>>,
    dark: Cell<bool>,
    /// One layer per card, in the cards' order ([`Slices`]).
    layers: RefCell<Vec<Retained<CALayer>>>,
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

        /// A window of another scale, or a first window: the pictures are drawn for it.
        #[unsafe(method(viewDidChangeBackingProperties))]
        fn did_change_backing(&self) {
            self.lay();
        }

        #[unsafe(method(viewDidMoveToWindow))]
        fn did_move_to_window(&self) {
            self.lay();
        }
    }
);

impl Shade {
    pub(crate) fn new(mtm: MainThreadMarker, frame: NSRect) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(ShadeIvars {
            cards: RefCell::new(Vec::new()),
            dark: Cell::new(true),
            layers: RefCell::new(Vec::new()),
        });
        // SAFETY: `initWithFrame:` is NSView's designated initializer and the
        // ivars are set.
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: frame] };
        this.setWantsLayer(true);
        // The cards' layers are clipped to the view, as a drawing in it would be.
        if let Some(layer) = this.layer() {
            layer.setMasksToBounds(true);
        }
        this
    }

    /// The theme's lightness: which footing the cards stand on.
    pub(crate) fn paint(&self, theme: &Theme) {
        if self.ivars().dark.replace(theme.is_dark()) != theme.is_dark() {
            self.lay();
        }
    }

    /// The cards on screen now; laid again only when they changed.
    pub(crate) fn set_cards(&self, cards: Vec<(NSRect, bool)>) {
        if *self.ivars().cards.borrow() != cards {
            *self.ivars().cards.borrow_mut() = cards;
            self.lay();
        }
    }

    /// Lays a layer under every card with its footing's picture ([`Slices`]), at once: Core
    /// Animation's implicit move would trail the cards a window resize has already moved.
    fn lay(&self) {
        let (Some(host), Some(window)) = (self.layer(), self.window()) else {
            return;
        };
        let scale = window.backingScaleFactor();
        let corner = corner_pt();
        let iv = self.ivars();
        let dark = iv.dark.get();
        let cards = iv.cards.borrow();
        let mut layers = iv.layers.borrow_mut();
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        let kept = cards.len().min(layers.len());
        for gone in layers.drain(kept..) {
            gone.removeFromSuperlayer();
        }
        for (index, (rect, focused)) in cards.iter().enumerate() {
            if index == layers.len() {
                let layer = CALayer::new();
                host.addSublayer(&layer);
                layers.push(layer);
            }
            let layer = &layers[index];
            let footing = footing(dark, *focused);
            let slices = Slices::of(&footing, corner, rect.size);
            let picture = footing_picture(footing, corner, &slices, scale);
            layer.setContentsScale(scale);
            // SAFETY: a `CGImage` is what `contents` takes.
            unsafe { layer.setContents(picture.as_deref().map(|image| image.as_ref())) };
            layer.setContentsCenter(slices.center);
            // AppKit keeps the view's own layer flipped or not as it likes (it flips it once the
            // view stands in the flipped root): the place goes through its conversion.
            layer.setFrame(self.convertRectToLayer(grown(*rect, slices.margin)));
        }
        CATransaction::commit();
    }

    /// The cards are sliding for `secs`: the shade already stands at their
    /// final places, so it waits out the slide unseen and then fades in.
    /// Core Animation's own delay — a timer would have to find this view
    /// again.
    pub(crate) fn hold(&self, secs: f64) {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// What is left of a shadow at `distance` once every ring up to it has cleared its share.
    fn left_at(rings: &[(f64, f64)], distance: f64) -> f64 {
        rings
            .iter()
            .filter(|(at, _)| *at <= distance)
            .fold(1.0, |left, (_, alpha)| left * (1.0 - alpha))
    }

    fn every_footing() -> impl Iterator<Item = Footing> {
        [(true, true), (true, false), (false, true), (false, false)]
            .into_iter()
            .map(|(dark, focused)| footing(dark, focused))
    }

    #[test]
    fn a_shadow_runs_out_to_nothing_before_the_blur_stops() {
        for footing in every_footing() {
            let (from, to) = footing.tail().expect("a real footing has a tail");
            assert!(0.0 < from && from < to && to < footing.blur);
            let rings = tail_rings(&footing);
            assert!(rings.iter().all(|(at, _)| from < *at && *at <= to));
            assert_eq!(
                left_at(&rings, to),
                0.0,
                "the last ring leaves nothing at {to}"
            );
        }
    }

    #[test]
    fn the_tail_is_a_smooth_step_that_leaves_the_body_alone() {
        for footing in every_footing() {
            let (from, to) = footing.tail().expect("a real footing has a tail");
            let rings = tail_rings(&footing);
            // Nothing is cleared where the tail has not begun.
            assert_eq!(left_at(&rings, from), 1.0);
            // The middle of the tail has lost half: a smooth step, not a ramp or a cliff.
            let middle = left_at(&rings, (from + to) / 2.0);
            assert!((middle - 0.5).abs() < 0.1, "{middle} left at the middle");
            // It only ever falls, and no ring takes more than a smooth step's steepest share.
            let steepest = 1.5 * RING_PT / (to - from);
            let mut before = 1.0;
            for (at, _) in &rings {
                let now = left_at(&rings, *at);
                assert!(now <= before && before - now <= steepest + 0.02);
                before = now;
            }
        }
    }

    /// A blur too small to have a tail keeps its shadow as cast: nothing fades, nothing clears.
    #[test]
    fn a_blur_too_small_for_a_tail_is_left_as_cast() {
        for blur in [0.0, 0.5, BLUR_SHORT_PT] {
            let small = Footing {
                blur,
                ..footing(true, false)
            };
            assert_eq!(small.tail(), None, "blur {blur}");
            assert!(tail_rings(&small).is_empty(), "blur {blur}");
        }
        // Just over the line there is a tail, and it clears everything by its end.
        let over = Footing {
            blur: BLUR_SHORT_PT + 0.1,
            ..footing(true, false)
        };
        let rings = tail_rings(&over);
        assert_eq!(left_at(&rings, over.tail().expect("a tail").1), 0.0);
    }

    #[test]
    fn a_caster_smaller_in_the_corner_than_its_spread_is_square() {
        for footing in every_footing() {
            // The corner before macOS 26 (10 pt, less the 8 pt gap) is under every spread.
            assert_eq!(caster_radius(2.0, &footing), 0.0);
            assert_eq!(caster_radius(footing.spread + 2.5, &footing), 2.5);
        }
    }

    // ─── the picture, drawn ──────────────────────────────────────────────

    /// The window `draw_shadow` is drawn in, in points, and its pixels per point: a retina
    /// window's backing store.
    const WINDOW: (f64, f64) = (200.0, 120.0);
    const SCALE: usize = 2;

    /// What `draw` leaves in a transparent `WINDOW` at [`SCALE`]x, as a retina window's backing
    /// store would hold it: `(width, height, rows from the top, premultiplied RGBA)`.
    fn on_retina(draw: impl FnOnce()) -> (usize, usize, Vec<[u8; 4]>) {
        on_retina_of(WINDOW, draw)
    }

    /// [`on_retina`] in a window `window` points big.
    fn on_retina_of(window: (f64, f64), draw: impl FnOnce()) -> (usize, usize, Vec<[u8; 4]>) {
        let (wide, high) = (window.0 as usize * SCALE, window.1 as usize * SCALE);
        // SAFETY: no planes of our own (the bitmap allocates them), 8-bit RGBA at the size given,
        // and a constant colour space name AppKit exposes.
        let rep = unsafe {
            NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bytesPerRow_bitsPerPixel(
            NSBitmapImageRep::alloc(),
            null_mut(),
            wide as isize,
            high as isize,
            8,
            4,
            true,
            false,
            NSDeviceRGBColorSpace,
            0,
            0,
        )
        }
        .expect("a bitmap");
        rep.setSize(NSSize::new(window.0, window.1));
        let context = NSGraphicsContext::graphicsContextWithBitmapImageRep(&rep)
            .expect("a context on the bitmap");
        NSGraphicsContext::saveGraphicsState_class();
        NSGraphicsContext::setCurrentContext(Some(&context));
        // Transparent to begin with: a new bitmap holds whatever was in the memory.
        context.setCompositingOperation(NSCompositingOperation::Clear);
        NSBezierPath::fillRect(NSRect::new(NSPoint::ZERO, NSSize::new(window.0, window.1)));
        context.setCompositingOperation(NSCompositingOperation::SourceOver);
        draw();
        NSGraphicsContext::restoreGraphicsState_class();
        let row = rep.bytesPerRow() as usize;
        // SAFETY: the bitmap owns `row * high` bytes at `bitmapData`, which `rep` keeps alive.
        let bytes = unsafe { std::slice::from_raw_parts(rep.bitmapData(), row * high) };
        let pixels = (0..high)
            .flat_map(|y| (0..wide).map(move |x| (x, y)))
            .map(|(x, y)| {
                let at = y * row + x * 4;
                [bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]
            })
            .collect();
        (wide, high, pixels)
    }

    /// The pixel at `(x, y)` points, `y` up from the bottom, as `[r, g, b, a]`: the middle of its
    /// first pixel row, left of that pixel.
    fn pixel_at(picture: &(usize, usize, Vec<[u8; 4]>), x: f64, y: f64) -> [u8; 4] {
        let (wide, high, pixels) = picture;
        let column = (x * SCALE as f64) as usize;
        let row = high - 1 - (y * SCALE as f64) as usize;
        pixels[row * wide + column]
    }

    /// A focused card on a window `WINDOW` big, its caster's left edge on a pixel boundary at 2x
    /// that is half a point from the points' own.
    fn card() -> NSRect {
        NSRect::new(NSPoint::new(60.5, 40.0), NSSize::new(80.0, 50.0))
    }

    #[test]
    fn the_shadows_picture_is_made_at_the_destinations_scale() {
        let footing = footing(true, true);
        let picture = on_retina(|| draw_shadow(card(), footing));
        // The caster's left edge is at 70.5 pt: between pixels 140 and 141 at 2x, and in the
        // middle of pixel 70 at 1x. Made at 1x and stretched, the edge would be a blur of both
        // sides; made at 2x, the pixel inside is solid and the one outside is the shadow's.
        let (inside, outside) = (
            pixel_at(&picture, 70.75, 55.0),
            pixel_at(&picture, 70.25, 55.0),
        );
        assert!(
            inside[3] >= 250,
            "the caster is solid at its edge: {inside:?}"
        );
        assert!(
            outside[3] <= 150,
            "the pixel beside it is the shadow's alone: {outside:?}"
        );
    }

    #[test]
    fn the_shadows_picture_clears_only_its_own_tail() {
        let footing = footing(true, true);
        // The window already holds something opaque all over, as the shade view does where
        // another card's shadow was drawn: far from this caster, inside the picture's reach and
        // beyond where its tail ends, that must still be there.
        let held = on_retina(|| {
            NSColor::colorWithSRGBRed_green_blue_alpha(1.0, 0.0, 0.0, 1.0).setFill();
            NSBezierPath::fillRect(NSRect::new(NSPoint::ZERO, NSSize::new(WINDOW.0, WINDOW.1)));
            draw_shadow(card(), footing);
        });
        let far = pixel_at(&held, 36.0, 60.0);
        assert_eq!(far[3], 255, "what the view held was cleared: {far:?}");
        assert_eq!(far[1], 0, "what the view held was painted over: {far:?}");
    }

    #[test]
    fn a_cards_shadow_falls_below_its_foot() {
        for footing in every_footing() {
            let picture = on_retina(|| draw_shadow(card(), footing));
            // The caster stands `spread` inside the card (40..90 pt high): as far over its top as
            // under its foot, only the foot has the shadow.
            let caster = (40.0 + footing.spread, 90.0 - footing.spread);
            let under = pixel_at(&picture, 100.0, caster.0 - 2.0)[3];
            let over = pixel_at(&picture, 100.0, caster.1 + 2.0)[3];
            // The faintest footing (a light theme's unfocused card) is under a fifth opaque.
            assert!(
                under >= 20 && under > 4 * over,
                "{footing:?}: {under} under the foot, {over} over the top"
            );
        }
    }

    /// The card a picture of `slices` is drawn round, and the picture as a window would hold it.
    fn footing_on_retina(
        footing: Footing,
        card: NSSize,
        margin: f64,
    ) -> (usize, usize, Vec<[u8; 4]>) {
        let window = (card.width + 2.0 * margin, card.height + 2.0 * margin);
        on_retina_of(window, || {
            draw_footing(NSRect::new(NSPoint::new(margin, margin), card), footing);
        })
    }

    #[test]
    fn a_stretched_footing_is_what_the_cards_own_size_paints() {
        let corner = corner_pt();
        for footing in every_footing() {
            let slices = Slices::of(&footing, corner, NSSize::new(1000.0, 1000.0));
            assert!(slices.center.size.width > 0.0 && slices.center.size.height > 0.0);
            let picture = footing_on_retina(footing, slices.card, slices.margin);
            // A card longer by an odd number of points both ways, drawn at its own size.
            let (longer, taller) = (37.0, 23.0);
            let card = NSSize::new(slices.card.width + longer, slices.card.height + taller);
            let drawn = footing_on_retina(footing, card, slices.margin);
            let (wide, high, pixels) = &picture;
            let (drawn_wide, drawn_high, drawn_pixels) = &drawn;
            // The picture's parts in its pixels: left of the middle, the middle, right of it; and
            // the same from the top (the unit square is bottom-up).
            let px = |unit: f64, of: usize| (unit * of as f64).round() as usize;
            let left = px(slices.center.origin.x, *wide);
            let right = *wide - px(slices.center.origin.x + slices.center.size.width, *wide);
            let top = *high - px(slices.center.origin.y + slices.center.size.height, *high);
            let bottom = px(slices.center.origin.y, *high);
            let from = |at: usize, near: usize, far: usize, length: usize, of: usize| {
                if at < near {
                    at
                } else if at >= length - far {
                    at - (length - of)
                } else {
                    near
                }
            };
            for y in 0..*drawn_high {
                for x in 0..*drawn_wide {
                    let (sx, sy) = (
                        from(x, left, right, *drawn_wide, *wide),
                        from(y, top, bottom, *drawn_high, *high),
                    );
                    let got = drawn_pixels[y * drawn_wide + x];
                    let stretched = pixels[sy * wide + sx];
                    let apart = got
                        .iter()
                        .zip(stretched)
                        .map(|(a, b)| a.abs_diff(b))
                        .max()
                        .unwrap_or(0);
                    assert!(
                        apart <= 2,
                        "{footing:?}: pixel ({x}, {y}) is {got:?} drawn, {stretched:?} stretched"
                    );
                }
            }
        }
    }

    #[test]
    fn a_card_too_small_to_stretch_is_drawn_at_its_own_size() {
        let footing = footing(true, true);
        let corner = corner_pt();
        let roomy = Slices::of(&footing, corner, NSSize::new(1000.0, 1000.0));
        let small = NSSize::new(roomy.card.width - 1.0, roomy.card.height + 50.0);
        let slices = Slices::of(&footing, corner, small);
        assert_eq!(slices.card.width, small.width, "nothing to stretch across");
        assert_eq!(slices.center.size.width, 0.0);
        assert_eq!(
            slices.card.height, roomy.card.height,
            "the height still stretches"
        );
        assert!(slices.center.size.height > 0.0);
    }

    #[test]
    fn a_footings_picture_is_on_whole_points() {
        let corner = corner_pt();
        for footing in every_footing() {
            let slices = Slices::of(&footing, corner, NSSize::new(1000.0, 1000.0));
            let size = slices.size();
            let whole = |value: f64| value.fract() == 0.0;
            assert!(whole(slices.margin) && whole(size.width) && whole(size.height));
            for edge in [
                slices.center.origin.x * size.width,
                slices.center.origin.y * size.height,
                slices.center.size.width * size.width,
                slices.center.size.height * size.height,
            ] {
                assert!((edge - edge.round()).abs() < 1e-9, "{footing:?}: {edge}");
            }
        }
    }
}
