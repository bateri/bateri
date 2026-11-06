//! A capsule: what a minimized floating pane leaves in its tab — a small pill with the pane's
//! mark and title (the side column's row says the same), standing in a row under the splits or
//! at the tab's top right corner (`[appearance] minimized_panes`). A press on it opens the card
//! where it was and gives it the keyboard (`showMinimizedPane:`, the window's action, which
//! reads the pane off the sender). The programs in the pane run on, and the mark's dot says its
//! signals as the header's does.
//!
//! **The measures are `floating::capsule`'s** (26 pt tall, the mark 14 pt, the title 12.5 pt in
//! medium weight, 200 pt at the widest — a longer title fades at its end as a tab's does,
//! [`Title`]); the container asks [`Capsule::width`] and puts the view where the row says.
//!
//! **The view is the pill grown by [`REACH`]** so its shadow has room, and it takes a press only
//! on the pill itself (`hitTest:`), so the room round it stays the panes' and the strip's.
//!
//! A child of `tab_bar` for the bar's mark drawing and colours, which a sibling could not reach.

use std::cell::{Cell, RefCell};

use bt_shell_common::floating::{CapsulePlace, capsule};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAccessibility, NSApplication, NSBezierPath, NSEvent, NSFont, NSFontWeightMedium,
    NSGraphicsContext, NSShadow, NSTextAlignment, NSView,
};
use objc2_foundation::{NSAttributedString, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString};

use super::pane_head::{DOT, DOT_RING, HeadLook, MARK, TITLE_TEXT};
use super::tab_title::Title;
use super::{Palette, Tint, draw_mark, rounded, track_hover};
use crate::palette::attributes;
use crate::tabs;

/// The room round the pill for its shadow, points.
pub(crate) const REACH: f64 = 10.0;

/// How much of the foreground the face under the pointer is.
const HOT_FACE: f64 = 0.07;

/// The shadow under a capsule: `(drop, blur, opacity)` in the strip, where it rests on the
/// ground, and at the corner, where it stands on the content like a card.
fn shade(place: CapsulePlace) -> (f64, f64, f64) {
    match place {
        CapsulePlace::Strip => (2.0, 6.0, 0.18),
        CapsulePlace::TopRight => (4.0, 12.0, 0.30),
    }
}

pub(crate) struct CapsuleIvars {
    pane: Cell<u64>,
    look: RefCell<Option<HeadLook>>,
    palette: Cell<Option<Palette>>,
    place: Cell<CapsulePlace>,
    /// The newest capsule is the one ⌃⌘M opens: its tip names the key.
    newest: Cell<bool>,
    /// The pointer is on the pill.
    hot: Cell<bool>,
    pressed: Cell<bool>,
    /// The title's width as the type sets it, points.
    natural: Cell<f64>,
    title: Title,
}

define_class!(
    // SAFETY: NSView is designed for subclassing; Capsule implements no `Drop` and is born with
    // `initWithFrame:`.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriCapsule"]
    #[ivars = CapsuleIvars]
    pub(crate) struct Capsule;

    unsafe impl NSObjectProtocol for Capsule {}

    impl Capsule {
        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool {
            true
        }

        #[unsafe(method(mouseDownCanMoveWindow))]
        fn can_move_window(&self) -> bool {
            false
        }

        /// Only the pill takes a press: the room round it for the shadow is the view under's.
        #[unsafe(method_id(hitTest:))]
        fn hit_test(&self, point: NSPoint) -> Option<Retained<NSView>> {
            // SAFETY: reading the superview; we are on the main thread.
            let parent = unsafe { self.superview() };
            let at = self.convertPoint_fromView(point, parent.as_deref());
            let view: &NSView = self;
            contains(self.pill(), at).then(|| view.retain())
        }

        #[unsafe(method(mouseEntered:))]
        fn mouse_entered(&self, _event: &NSEvent) {
            self.warm(true);
        }

        #[unsafe(method(mouseExited:))]
        fn mouse_exited(&self, _event: &NSEvent) {
            self.warm(false);
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, _event: &NSEvent) {
            self.ivars().pressed.set(true);
        }

        /// A press that ends on the pill opens the card.
        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, event: &NSEvent) {
            if !self.ivars().pressed.replace(false) {
                return;
            }
            let at = self.convertPoint_fromView(event.locationInWindow(), None);
            if contains(self.pill(), at) {
                self.open();
            }
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            self.draw();
        }
    }
);

impl Capsule {
    pub(crate) fn new(mtm: MainThreadMarker, pane: u64) -> Retained<Self> {
        let title = Title::new(mtm, NSTextAlignment::Left);
        let this = Self::alloc(mtm).set_ivars(CapsuleIvars {
            pane: Cell::new(pane),
            look: RefCell::new(None),
            palette: Cell::new(None),
            place: Cell::new(CapsulePlace::Strip),
            newest: Cell::new(false),
            hot: Cell::new(false),
            pressed: Cell::new(false),
            natural: Cell::new(0.0),
            title,
        });
        // SAFETY: `initWithFrame:` is NSView's designated initializer and the ivars are set.
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };
        this.addSubview(this.ivars().title.view());
        this.ivars().title.field().setAccessibilityElement(false);
        this.setAccessibilityElement(true);
        track_hover(&this);
        this
    }

    /// The pane the capsule stands for.
    pub(crate) fn pane(&self) -> u64 {
        self.ivars().pane.get()
    }

    /// What it shows, in the theme's colours: the pane's look (its mark, title and dot), where
    /// capsules stand, and whether this is the newest. Redrawn only when something changed.
    pub(crate) fn set(
        &self,
        look: &HeadLook,
        theme: &bt_core::Theme,
        place: CapsulePlace,
        newest: bool,
    ) {
        let iv = self.ivars();
        let palette = Palette::of(theme);
        let shown = (iv.look.borrow().as_ref() != Some(look))
            | (iv.palette.replace(Some(palette)) != Some(palette))
            | (iv.place.replace(place) != place);
        let tip = newest != iv.newest.replace(newest);
        if shown {
            iv.look.replace(Some(look.clone()));
            let text = iv.title.field();
            // SAFETY: AppKit's constant font weight.
            let weight = unsafe { NSFontWeightMedium };
            let font = NSFont::systemFontOfSize_weight(TITLE_TEXT, weight);
            // SAFETY: the attributes are `NSColor`/`NSFont` under AppKit's two keys.
            let piece = unsafe {
                NSAttributedString::new_with_attributes(
                    &NSString::from_str(&look.title),
                    &attributes(&font, Tint::of(palette.title, 1.0)),
                )
            };
            text.setAttributedStringValue(&piece);
            iv.natural.set(text.fittingSize().width);
        }
        if shown || tip {
            let words = format!("Show {}", look.title);
            self.setAccessibilityLabel(Some(&NSString::from_str(&words)));
            let key = newest
                .then(|| crate::menu::keys::glyphs_of(self.mtm(), "minimize_floating_pane"))
                .flatten();
            let words = match key {
                Some(key) => format!("{words}  {key}"),
                None => words,
            };
            self.setToolTip(Some(&NSString::from_str(&words)));
        }
        if shown {
            self.lay_out_parts();
            self.setNeedsDisplay(true);
        }
    }

    /// The pill's width for the title it holds, points ([`capsule::width`]); the least before it
    /// has a look.
    pub(crate) fn width(&self) -> f64 {
        capsule::width(self.ivars().natural.get())
    }

    /// Puts the pill at `frame` (in the container's space): the view is that grown by [`REACH`].
    pub(crate) fn place(&self, frame: NSRect) {
        let at = NSRect::new(
            NSPoint::new(frame.origin.x - REACH, frame.origin.y - REACH),
            NSSize::new(
                frame.size.width + 2.0 * REACH,
                frame.size.height + 2.0 * REACH,
            ),
        );
        if self.frame() != at {
            self.setFrame(at);
            self.lay_out_parts();
            self.setNeedsDisplay(true);
        }
    }

    /// The pill, in the view's space.
    fn pill(&self) -> NSRect {
        let size = self.bounds().size;
        NSRect::new(
            NSPoint::new(REACH, REACH),
            NSSize::new((size.width - 2.0 * REACH).max(0.0), capsule::HEIGHT),
        )
    }

    /// The mark's box: on the pill's left padding, in the middle of its height.
    fn mark_rect(&self) -> NSRect {
        let pill = self.pill();
        NSRect::new(
            NSPoint::new(
                pill.origin.x + capsule::PAD_LEFT,
                pill.origin.y + (capsule::HEIGHT - MARK) / 2.0,
            ),
            NSSize::new(MARK, MARK),
        )
    }

    /// The title after the mark, to the right padding; a title that does not fit fades.
    fn lay_out_parts(&self) {
        let iv = self.ivars();
        let pill = self.pill();
        let x = pill.origin.x + capsule::PAD_LEFT + capsule::MARK + capsule::MARK_GAP;
        let room = capsule::text_room(pill.size.width);
        let natural = iv.title.field().fittingSize();
        iv.title.place(
            NSRect::new(
                NSPoint::new(
                    x,
                    pill.origin.y + ((capsule::HEIGHT - natural.height) / 2.0).max(0.0),
                ),
                NSSize::new(room, natural.height),
            ),
            natural.width,
        );
    }

    fn warm(&self, hot: bool) {
        if self.ivars().hot.replace(hot) != hot {
            self.setNeedsDisplay(true);
        }
    }

    /// The window opens the card: its action reads the pane off the sender.
    fn open(&self) {
        let app = NSApplication::sharedApplication(self.mtm());
        let sender: &AnyObject = self;
        // SAFETY: the action takes one optional sender argument.
        unsafe { app.sendAction_to_from(sel!(showMinimizedPane:), None, Some(sender)) };
    }

    /// The pill: a shadow, the theme's ground, a thin line, the face under the pointer, the mark
    /// and its dot. The title is a view of its own over it.
    fn draw(&self) {
        let iv = self.ivars();
        let look = iv.look.borrow();
        let (Some(look), Some(palette)) = (look.as_ref(), iv.palette.get()) else {
            return;
        };
        let pill = self.pill();
        let radius = capsule::HEIGHT / 2.0;
        let (drop, blur, opacity) = shade(iv.place.get());
        NSGraphicsContext::saveGraphicsState_class();
        let shadow = NSShadow::new();
        shadow.setShadowOffset(NSSize::new(0.0, -drop));
        shadow.setShadowBlurRadius(blur);
        shadow.setShadowColor(Some(&Tint::of(0x000000, opacity).color()));
        shadow.set();
        Tint::of(palette.ground, 1.0).color().setFill();
        NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(pill, radius, radius).fill();
        NSGraphicsContext::restoreGraphicsState_class();
        rounded(
            pill,
            radius,
            Tint::of(palette.ground, 1.0),
            Some(Tint::of(palette.title, 0.16)),
        );
        if iv.hot.get() {
            rounded(pill, radius, Tint::of(palette.title, HOT_FACE), None);
        }
        let mark = self.mark_rect();
        draw_mark(look.kind, mark, &Tint::of(palette.dim, 1.0).color());
        if let Some(indicator) = look.indicator {
            let tone = palette.tone(tabs::list_dot(indicator), 0);
            let centre = NSPoint::new(mark.origin.x + MARK - DOT / 2.0, mark.origin.y + DOT / 2.0);
            for (side, color) in [(DOT + 2.0 * DOT_RING, palette.ground), (DOT, tone)] {
                Tint::of(color, 1.0).color().setFill();
                NSBezierPath::bezierPathWithOvalInRect(NSRect::new(
                    NSPoint::new(centre.x - side / 2.0, centre.y - side / 2.0),
                    NSSize::new(side, side),
                ))
                .fill();
            }
        }
    }
}

fn contains(rect: NSRect, at: NSPoint) -> bool {
    at.x >= rect.origin.x
        && at.x <= rect.origin.x + rect.size.width
        && at.y >= rect.origin.y
        && at.y <= rect.origin.y + rect.size.height
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_press_opens_through_the_windows_action_and_only_on_the_pill() {
        let source = include_str!("tab_capsule.rs");
        assert!(source.contains("sel!(showMinimizedPane:)"));
        // The room round the pill is not the capsule's to take.
        let hit = source.find("fn hit_test").expect("a hit test");
        assert!(source[hit..].contains("contains(self.pill()"));
    }

    #[test]
    fn the_shadow_stands_off_further_over_the_content() {
        let (rest, _, _) = shade(CapsulePlace::Strip);
        let (lifted, _, _) = shade(CapsulePlace::TopRight);
        assert!(lifted > rest);
    }
}
