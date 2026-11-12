//! The motion of Zoom Pane: a pane grows to fill the room its kind has and pushes the others of
//! its layer out, and gives the room back the same way.
//!
//! **Everything that moves is a picture.** The layout changes once, before anything plays — a
//! program is resized once — and what the eye follows is pictures on a [`Stage`]: the panes that
//! leave as they stood, the ones that come back as they stand, the pane that grows as it will be
//! (and, coming back, as it was). The live views wait unseen under them and show when the
//! pictures go. Three things ask for it: what is pushed out must be cut where the splits end,
//! which takes a parent that clips; a live pane cannot be moved to such a parent, since taking
//! it out of its container puts the keyboard down; and what a pane showed while it filled the
//! tab is gone the moment it is laid out small again.
//!
//! **The growing pane's edges move and what is in it stays.** Its picture is uncovered by a
//! mask that goes from the place it had to the place it has ([`uncover`]); under the mask its
//! body holds to the bottom left corner, where a terminal's input is, its header to the top
//! edge, and the header's tools to the top right corner ([`Hold`]). Nothing is stretched: the
//! text is its own size from the first frame.
//!
//! The motion is Core Animation's: it asks no pane for a frame and lays nothing out on the way.

use objc2::rc::Retained;
use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{NSColor, NSImage, NSView};
use objc2_foundation::{NSNumber, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString, ns_string};
use objc2_quartz_core::{
    CABasicAnimation, CALayer, CAMediaTiming, CAMediaTimingFunction, CATransaction,
    NSValueCATransform3DAdditions, kCAFillModeBoth,
};

use crate::card;
use crate::split::Rect;

/// How long a zoom's motion takes, in seconds. A design constant, the canvas's 240 ms: the base
/// a new motion starts from.
pub(crate) const ZOOM_SECS: f64 = 0.24;

/// How far past the tab's edge a floating card that is pushed out goes, in points: clear of the
/// edge with its picture's corners.
pub(crate) const CLEAR_PT: f64 = 24.0;

define_class!(
    /// What a zoom's pictures move on: a view over the container that cuts them at its own edges
    /// and takes no press — a press during the motion is the pane's under it.
    // SAFETY: NSView is designed for subclassing; Stage implements no `Drop` and is born with
    // `initWithFrame:`.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriZoomStage"]
    pub(crate) struct Stage;

    unsafe impl NSObjectProtocol for Stage {}

    impl Stage {
        /// Top-down, as the container is: a picture's frame here is its frame there.
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method_id(hitTest:))]
        fn hit_test(&self, _point: NSPoint) -> Option<Retained<NSView>> {
            None
        }
    }
);

impl Stage {
    /// A stage that shows `seen` of the container (the container's space) and cuts the rest.
    /// Its own space is the container's: a picture is placed by the frame it had there.
    pub(crate) fn new(mtm: MainThreadMarker, seen: Rect) -> Retained<Self> {
        let frame = ns_rect(seen);
        let this = Self::alloc(mtm).set_ivars(());
        // SAFETY: `initWithFrame:` is NSView's designated initializer; there are no ivars.
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: frame] };
        this.setWantsLayer(true);
        this.setBoundsOrigin(frame.origin);
        if let Some(layer) = this.layer() {
            layer.setMasksToBounds(true);
        }
        this
    }

    /// Puts `still` on the stage where it stood, its corners rounded by `corner`, and gives
    /// back its view.
    pub(crate) fn add(&self, still: &Still, corner: f64) -> Retained<NSView> {
        let mtm = self.mtm();
        let scale = self
            .window()
            .map_or(2.0, |window| window.backingScaleFactor());
        let ghost = NSView::initWithFrame(NSView::alloc(mtm), ns_rect(still.frame));
        ghost.setWantsLayer(true);
        self.addSubview(&ghost);
        if let Some(layer) = ghost.layer() {
            let contents = still.image.layerContentsForContentsScale(scale);
            CATransaction::begin();
            CATransaction::setDisableActions(true);
            layer.setContentsScale(scale);
            // SAFETY: what `NSImage::layerContentsForContentsScale` returns is what `contents`
            // takes.
            unsafe { layer.setContents(Some(&contents)) };
            layer.setCornerRadius(corner);
            layer.setMasksToBounds(corner > 0.0);
            CATransaction::commit();
        }
        ghost
    }
}

/// A picture of something of the container and the frame it has there.
pub(crate) struct Still {
    pub(crate) frame: Rect,
    pub(crate) image: Retained<NSImage>,
}

/// Which way a zoom's motion runs: to the zoom, or back from it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Way {
    /// A pane grows to fill the tab and the others go.
    In,
    /// It gives the room back and the others return.
    Out,
}

/// Which way a picture travels ([`travel`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Run {
    /// From its place to a point away from it, where it stays until its stage goes.
    Leaves,
    /// From a point away to its place.
    Arrives,
}

/// The corner of the growing pane a part of its picture holds to while the pane's edges move.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Hold {
    /// The body: a terminal's input and its last lines are at its foot.
    BottomLeft,
    /// The header: its mark and title.
    TopLeft,
    /// The header's tools, which end at the pane's right edge.
    TopRight,
}

impl Hold {
    /// How far a part that holds to this corner stands from its place while the pane is at
    /// `small` and its place is in the pane at `big`, in points (x, y; top-down).
    pub(crate) fn shift(self, small: Rect, big: Rect) -> (f64, f64) {
        let left = small.x - big.x;
        let right = (small.x + small.width) - (big.x + big.width);
        let top = small.y - big.y;
        let bottom = (small.y + small.height) - (big.y + big.height);
        match self {
            Hold::BottomLeft => (left, bottom),
            Hold::TopLeft => (left, top),
            Hold::TopRight => (right, top),
        }
    }
}

/// Plays `view` between its place — its frame — and the point `(dx, dy)` points from it over
/// [`ZOOM_SECS`], on the slides' curve. Without a transform that lands there
/// ([`card::fitted`]) the view stays where it is, which beats a picture that jumps.
pub(crate) fn travel(view: &NSView, (dx, dy): (f64, f64), run: Run, begin: f64) {
    let Some(layer) = view.layer() else {
        return;
    };
    let frame = view.frame();
    let at = Rect::new(
        frame.origin.x,
        frame.origin.y,
        frame.size.width,
        frame.size.height,
    );
    let away = Rect::new(at.x + dx, at.y + dy, at.width, at.height);
    let Some(moved) = card::fitted(&layer, away, at) else {
        return;
    };
    let (from, to) = match run {
        Run::Leaves => (card::IDENTITY, moved),
        Run::Arrives => (moved, card::IDENTITY),
    };
    // SAFETY: plain `NSValue`s around `CATransform3D` values, nothing retained or borrowed.
    let (from_value, to_value) = unsafe {
        (
            objc2_foundation::NSValue::valueWithCATransform3D(from),
            objc2_foundation::NSValue::valueWithCATransform3D(to),
        )
    };
    // The animation holds both its ends ([`timed`]) and the layer's own transform is left
    // alone: AppKit keeps a view's layer transform as it likes.
    let animation = CABasicAnimation::animationWithKeyPath(Some(ns_string!("transform")));
    // SAFETY: both values are `NSValue`s of a `CATransform3D`, what `transform` takes.
    unsafe {
        animation.setFromValue(Some(&from_value));
        animation.setToValue(Some(&to_value));
    }
    timed(&animation, &layer, begin);
    layer.addAnimation_forKey(&animation, Some(ns_string!("bateri.zoom")));
}

/// Shows `stage` — one that covers its container, so its space is the container's — through a
/// rounded rectangle that goes from `from` to `to` over [`ZOOM_SECS`]: the edges of the pane
/// that grows or comes back. The mask stays at `to`.
pub(crate) fn uncover(stage: &Stage, from: Rect, to: Rect, corner: f64, begin: f64) {
    let Some(layer) = stage.layer() else {
        return;
    };
    let bounds = stage.bounds();
    // The mask is in the layer's own space, which runs the way the layer says: top-down like
    // the view when its contents are flipped, bottom-up otherwise.
    let flipped = layer.contentsAreFlipped();
    let local = |rect: Rect| {
        let x = rect.x - bounds.origin.x;
        let top = rect.y - bounds.origin.y;
        let y = if flipped {
            top
        } else {
            bounds.size.height - top - rect.height
        };
        Rect::new(x, y, rect.width, rect.height)
    };
    let (from, to) = (local(from), local(to));
    let mask = CALayer::new();
    CATransaction::begin();
    CATransaction::setDisableActions(true);
    mask.setBackgroundColor(Some(&NSColor::blackColor().CGColor()));
    mask.setCornerRadius(corner);
    mask.setFrame(ns_rect(to));
    // SAFETY: a mask is any layer; this one is the stage's alone and lives as long as it.
    unsafe { layer.setMask(Some(&mask)) };
    CATransaction::commit();
    let (from_x, from_y) = from.center();
    let (to_x, to_y) = to.center();
    for (path, was, is, key) in [
        (
            ns_string!("position.x"),
            from_x,
            to_x,
            ns_string!("bateri.zoom.x"),
        ),
        (
            ns_string!("position.y"),
            from_y,
            to_y,
            ns_string!("bateri.zoom.y"),
        ),
        (
            ns_string!("bounds.size.width"),
            from.width,
            to.width,
            ns_string!("bateri.zoom.width"),
        ),
        (
            ns_string!("bounds.size.height"),
            from.height,
            to.height,
            ns_string!("bateri.zoom.height"),
        ),
    ] {
        number(&mask, path, was, is, key, &layer, begin);
    }
}

/// One number of `layer` from `was` to `is` over [`ZOOM_SECS`], timed by `clock`'s time.
fn number(
    layer: &CALayer,
    path: &NSString,
    was: f64,
    is: f64,
    key: &NSString,
    clock: &CALayer,
    begin: f64,
) {
    let animation = CABasicAnimation::animationWithKeyPath(Some(path));
    // SAFETY: plain `NSNumber`s, what a layer's coordinates take.
    unsafe {
        animation.setFromValue(Some(&NSNumber::numberWithDouble(was)));
        animation.setToValue(Some(&NSNumber::numberWithDouble(is)));
    }
    timed(&animation, clock, begin);
    layer.addAnimation_forKey(&animation, Some(key));
}

/// The timing every part of a zoom's motion has: [`ZOOM_SECS`] on the slides' curve from
/// `begin` (`CACurrentMediaTime`), holding its ends, so the parts keep step and none shows a
/// frame of its resting place before it starts or after it ends.
fn timed(animation: &CABasicAnimation, clock: &CALayer, begin: f64) {
    animation.setDuration(ZOOM_SECS);
    animation.setBeginTime(clock.convertTime_fromLayer(begin, None));
    let curve = CAMediaTimingFunction::functionWithControlPoints(0.2, 0.8, 0.2, 1.0);
    animation.setTimingFunction(Some(&curve));
    // SAFETY: a constant `NSString` Core Animation exposes, only read.
    animation.setFillMode(unsafe { kCAFillModeBoth });
    animation.setRemovedOnCompletion(false);
}

fn ns_rect(rect: Rect) -> NSRect {
    NSRect::new(
        NSPoint::new(rect.x, rect.y),
        NSSize::new(rect.width, rect.height),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_part_of_the_growing_pane_holds_to_its_own_corner() {
        // The top right pane of three grows to the whole area.
        let small = Rect::new(504.0, 8.0, 488.0, 288.0);
        let big = Rect::new(8.0, 8.0, 984.0, 584.0);
        // The body's foot is the pane's foot and its left edge the pane's left edge.
        assert_eq!(Hold::BottomLeft.shift(small, big), (496.0, -296.0));
        // The header goes with the top left corner: sideways only, the tops are one.
        assert_eq!(Hold::TopLeft.shift(small, big), (496.0, 0.0));
        // The tools end where the pane ends: this pane's right edge is the area's already.
        assert_eq!(Hold::TopRight.shift(small, big), (0.0, 0.0));
        // At its full size nothing is shifted.
        for hold in [Hold::BottomLeft, Hold::TopLeft, Hold::TopRight] {
            assert_eq!(hold.shift(big, big), (0.0, 0.0));
        }
    }
}
