//! A title that does not fit its place: it fades out at its end instead of ending in "…". The
//! chip's title, the side row's and the group button's name are one [`Title`]; the group headers
//! of the column and the lists keep the "…" of an ordinary label.
//!
//! **The arithmetic is `tabs::title_overflows` and `tabs::title_fade`**; this is how it is drawn.
//! The title is a label as wide as its text, in a host view that is as wide as its place and
//! clips: where the text does not fit, the host's layer carries a mask that runs out over the
//! last [`tabs::TITLE_FADE`] points, and the label stands at the left end of it. A text that fits
//! stands where its place says, at the left or in the middle. The title never moves to show the
//! rest: the owner puts the whole of it in the tooltip.
//!
//! **The label stays left-aligned, and centring is its frame's.** AppKit measures a centred label
//! 4 pt wider than a left-aligned one and draws its text 2 pt further in. A label whose alignment
//! followed the verdict changed the measure the next verdict reads: a text within those 4 pt of
//! its room faded and stopped fading at every layout, and hopped 2 pt each time.
//!
//! The host never takes a press: the view under it decides what a press means.

use std::cell::{Cell, RefCell};

use objc2::rc::Retained;
use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{NSBezierPath, NSColor, NSLineBreakMode, NSTextAlignment, NSTextField, NSView};
use objc2_foundation::{NSObjectProtocol, NSPoint, NSRect, NSSize};
use objc2_quartz_core::CALayer;

use super::{label, paint_in, still};
use crate::tabs;

define_class!(
    // SAFETY: NSView is designed for subclassing; TitleHost has no ivars and implements no `Drop`
    // and is born with `initWithFrame:`.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriTitleHost"]
    struct TitleHost;

    unsafe impl NSObjectProtocol for TitleHost {}

    impl TitleHost {
        /// A title never takes a press, wherever it stands: what is under it does.
        #[unsafe(method_id(hitTest:))]
        fn hit_test(&self, _point: NSPoint) -> Option<Retained<NSView>> {
            // No `return`: `define_class!` converts the body's last expression.
            None
        }
    }
);

impl TitleHost {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        // SAFETY: `initWithFrame:` is NSView's designated initializer and there are no ivars.
        unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] }
    }
}

/// What the mask's picture was painted for: the host's size and the display's scale.
type Painted = (f64, f64, f64);

/// A title: a host view of the place's size, the label in it, and what the fade has to remember.
pub(super) struct Title {
    host: Retained<TitleHost>,
    text: Retained<NSTextField>,
    /// How the text stands in its place while it fits: centred in the lone tab's wide place, at the
    /// left in a button. A chip's and a row's place is as wide as the text, where the two agree.
    rest: NSTextAlignment,
    /// The host's mask while the text does not fit, and what its picture was painted for.
    mask: RefCell<Option<Retained<CALayer>>>,
    painted: Cell<Option<Painted>>,
    /// The text does not fit its place now (it fades at its end).
    over: Cell<bool>,
}

impl Title {
    /// A title whose text stands as `rest` while it fits.
    pub(super) fn new(mtm: MainThreadMarker, rest: NSTextAlignment) -> Self {
        let text = label(mtm);
        // The text is set as wide as it is: nothing truncates it, the host's edge and the mask
        // are what cut it.
        text.setLineBreakMode(NSLineBreakMode::ByClipping);
        // One alignment for good: `rest` is the label's frame (the module header).
        text.setAlignment(NSTextAlignment::Left);
        let host = TitleHost::new(mtm);
        host.setWantsLayer(true);
        host.setClipsToBounds(true);
        host.addSubview(&text);
        Self {
            host,
            text,
            rest,
            mask: RefCell::new(None),
            painted: Cell::new(None),
            over: Cell::new(false),
        }
    }

    /// The view that goes into the chip or the button, where the label went.
    pub(super) fn view(&self) -> &NSView {
        &self.host
    }

    /// The label: its font, ink and text are set on it, and its fitting size is the text's width.
    pub(super) fn field(&self) -> &NSTextField {
        &self.text
    }

    /// Whether the text does not fit its place (it fades at its end).
    pub(super) fn overflows(&self) -> bool {
        self.over.get()
    }

    /// Puts the title in `frame` (the host's, in its parent's space) with its text `natural`
    /// wide as the type sets it.
    pub(super) fn place(&self, frame: NSRect, natural: f64) {
        let over = tabs::title_overflows(natural, frame.size.width);
        self.over.set(over);
        still(|| {
            self.host.setFrame(frame);
            let size = frame.size;
            let width = natural.ceil();
            let x = if !over && self.rest == NSTextAlignment::Center {
                ((size.width - width) / 2.0).max(0.0).round()
            } else {
                0.0
            };
            self.text.setFrame(NSRect::new(
                NSPoint::new(x, 0.0),
                NSSize::new(width, size.height),
            ));
            if over {
                self.fade(size);
            } else {
                self.unfade();
            }
        });
    }

    /// The host's mask for a place of `size` that its text does not fit: opaque, running out over
    /// the last [`tabs::title_fade`] points at the right end. The picture is painted when the
    /// size or the scale changes, not at every placing.
    fn fade(&self, size: NSSize) {
        let Some(host) = self.host.layer() else {
            return;
        };
        let scale = self
            .host
            .window()
            .map_or(2.0, |window| window.backingScaleFactor());
        let mut slot = self.mask.borrow_mut();
        let fresh = slot.is_none();
        let layer = slot.get_or_insert_with(CALayer::new);
        layer.setFrame(NSRect::new(NSPoint::ZERO, size));
        let painted = (size.width, size.height, scale);
        if self.painted.replace(Some(painted)) != Some(painted) {
            let fade = tabs::title_fade(size.width);
            paint_in(layer, size, scale, move |rect| fade_picture(rect, fade));
        }
        if fresh {
            // SAFETY: a mask is any layer; this one is the host's alone and lives as long as the
            // slot holds it.
            unsafe { host.setMask(Some(layer)) };
        }
    }

    /// Takes the mask off: the text fits, and nothing needs hiding.
    fn unfade(&self) {
        if self.mask.take().is_none() {
            return;
        }
        self.painted.set(None);
        if let Some(host) = self.host.layer() {
            // SAFETY: taking a mask off, which any layer allows.
            unsafe { host.setMask(None) };
        }
    }
}

/// The mask's picture in `rect` (top-down): black — its alpha is what shows — from the left edge,
/// running out to nothing over the last `fade` points along [`tabs::title_faded`].
fn fade_picture(rect: NSRect, fade: f64) {
    let (width, height) = (rect.size.width, rect.size.height);
    let black = |alpha: f64| NSColor::colorWithSRGBRed_green_blue_alpha(0.0, 0.0, 0.0, alpha);
    let solid = (width - fade).max(0.0);
    black(1.0).setFill();
    NSBezierPath::bezierPathWithRect(NSRect::new(NSPoint::ZERO, NSSize::new(solid, height))).fill();
    // A column each: the picture is painted when the place's size changes, not per frame.
    for column in 0..fade.ceil() as usize {
        let x = column as f64;
        black(1.0 - tabs::title_faded(x + 0.5, fade)).setFill();
        NSBezierPath::bezierPathWithRect(NSRect::new(
            NSPoint::new(solid + x, 0.0),
            NSSize::new(1.0, height),
        ))
        .fill();
    }
}

#[cfg(test)]
mod tests {
    /// The label's alignment is written once: AppKit measures it by its alignment, so a title that
    /// set it by whether the text fits would change the measure that answers the question.
    #[test]
    fn a_title_never_changes_its_labels_alignment() {
        let source = include_str!("tab_title.rs");
        let code = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert_eq!(code.matches(".setAlignment(").count(), 1);
    }
}
