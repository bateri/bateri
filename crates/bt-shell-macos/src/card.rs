//! A pane's look in a split tab: the **card**. With one pane the tab shows it
//! edge to edge; with two or more every pane is a card — a gap between and
//! around ([`GAP_PT`], the frame computation's: `split::Spacing`), corners
//! rounded and clipped ([`corner_pt`], concentric with the window's corner), a frame [`FRAME_PT`] wide in the
//! dividers' tone at full strength — as plain as the divider a split had before the
//! cards, and no fainter: two quiet frames side by side are what tells the
//! panes apart — and one step stronger on the focused pane (the quiet text's
//! tone, `Theme::quiet_srgb`). Nothing is veiled: all panes read at once (the veil is `[appearance] dim_unfocused_splits`, the pane's own
//! `DimOverlay`).
//!
//! **Two layers, two jobs.** The corners are the pane's *own* backing layer
//! (`cornerRadius` + `masksToBounds`: the Metal child and every overlay are
//! clipped by one mask). The frame is a separate topmost child, [`FrameBox`]:
//! a border on the pane's layer would sit *under* its sublayers — the Metal
//! view fills the pane — and the frame must lie over the grid. It takes no
//! part in hit testing, like the veil.
//!
//! **The slide** (one pane ↔ two, [`slide`]) plays on the layers and never on
//! the frames. The pane is laid out once, at its final frame, so the program
//! in it is resized once; what moves is a Core Animation transform from the
//! old rectangle's look to none. Animating the frame instead would resize the
//! pty on every tick (a `SIGWINCH` storm and a flickering `vim`) and would
//! pull the animation through the main thread. It asks for no GPU frame:
//! the compositor stretches the drawable that is already there, and the next
//! content frame replaces it at the final size.

use std::cell::Cell;
use std::sync::OnceLock;

use bt_core::Theme;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{NSBox, NSBoxType, NSColor, NSTitlePosition, NSView};
use objc2_core_foundation::CGRect;
use objc2_foundation::{
    NSNumber, NSObjectProtocol, NSOperatingSystemVersion, NSPoint, NSProcessInfo, NSString,
    NSValue, ns_string,
};
use objc2_quartz_core::{
    CABasicAnimation, CALayer, CAMediaTiming, CAMediaTimingFunction, CATransaction, CATransform3D,
    NSValueCATransform3DAdditions,
};

use crate::split::{self, Rect, Slide};

/// The gap between cards and around them, in points. The canvas's number, a
/// design constant: wide enough that two cards read as two, narrow enough
/// that a split keeps the room a divider would have taken.
pub(crate) const GAP_PT: f64 = 6.0;

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

/// The frame's width, in points, snapped to whole device pixels: two on a
/// Retina screen. One device pixel was a line the eye lost on a light theme,
/// and the frame is all that tells two cards apart.
pub(crate) const FRAME_PT: f64 = 1.0;

/// A move's slide: panes that change places settle in 200 ms, a touch quicker
/// than a pane count changing.
pub(crate) const MOVE_SECS: f64 = 0.20;

pub(crate) struct FrameIvars {
    /// The dividers' tone (`Theme::separator_srgb`): the frame of a pane
    /// without the focus.
    ink: Cell<[u8; 3]>,
    /// One step stronger (`Theme::quiet_srgb`): the focused pane's frame, and
    /// every frame while lifted.
    focus_ink: Cell<[u8; 3]>,
    /// Whether the pane holds the focus: the frame is a step stronger.
    focused: Cell<bool>,
    /// Whether the pane is a card now.
    carded: Cell<bool>,
    /// Whether the pane is lifted for arranging ([`crate::arrange`]): every
    /// frame reads as the focused one.
    raised: Cell<bool>,
    /// The frame's width, in points: [`FRAME_PT`] snapped to the window's
    /// device pixels.
    width: Cell<f64>,
}

define_class!(
    // SAFETY: NSBox is designed for subclassing; FrameBox implements no
    // `Drop` and is born with NSBox's constructor (`init`).
    #[unsafe(super(NSBox))]
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
    }
);

impl FrameBox {
    /// Born invisible: a pane is no card until its container says so.
    pub(crate) fn new(mtm: MainThreadMarker, theme: &Theme) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(FrameIvars {
            ink: Cell::new(theme.separator_srgb()),
            focus_ink: Cell::new(theme.quiet_srgb()),
            focused: Cell::new(false),
            carded: Cell::new(false),
            raised: Cell::new(false),
            width: Cell::new(0.0),
        });
        // SAFETY: `NSBox`'s `init`; the ivars are set.
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        this.setBoxType(NSBoxType::Custom);
        this.setTitlePosition(NSTitlePosition::NoTitle);
        this.setFillColor(&NSColor::clearColor());
        this.setCornerRadius(corner_pt());
        this.setBorderWidth(0.0);
        this.setAlphaValue(0.0);
        this.repaint();
        this
    }

    /// The theme's dividers' tone.
    pub(crate) fn paint(&self, theme: &Theme) {
        self.ivars().ink.set(theme.separator_srgb());
        self.ivars().focus_ink.set(theme.quiet_srgb());
        self.repaint();
    }

    /// Whether the pane holds the focus: the frame is a step stronger.
    pub(crate) fn set_focused(&self, focused: bool) {
        if self.ivars().focused.replace(focused) != focused {
            self.repaint();
        }
    }

    pub(crate) fn carded(&self) -> bool {
        self.ivars().carded.get()
    }

    /// The pane is lifted for arranging, or set down: while lifted the frame
    /// reads at the focused strength on every pane.
    pub(crate) fn set_raised(&self, raised: bool) {
        if self.ivars().raised.replace(raised) != raised {
            self.repaint();
        }
    }

    /// Makes the pane a card, or not, at the window's `scale` (the frame is
    /// [`FRAME_PT`] in whole device pixels). `true` if the answer changed.
    ///
    /// Leaving the card only turns the frame **invisible**; what it drew stays
    /// drawn, so a slide can fade it out instead of cutting it.
    pub(crate) fn set_carded(&self, carded: bool, scale: f64) -> bool {
        let iv = self.ivars();
        if carded {
            let width = (FRAME_PT * scale).round().max(1.0) / scale;
            if iv.width.replace(width) != width {
                self.setBorderWidth(width);
            }
        }
        self.setAlphaValue(if carded { 1.0 } else { 0.0 });
        iv.carded.replace(carded) != carded
    }

    fn repaint(&self) {
        let iv = self.ivars();
        let ink = if iv.focused.get() || iv.raised.get() {
            iv.focus_ink.get()
        } else {
            iv.ink.get()
        };
        let [r, g, b] = ink.map(|byte| f64::from(byte) / 255.0);
        self.setBorderColor(&NSColor::colorWithSRGBRed_green_blue_alpha(r, g, b, 1.0));
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
