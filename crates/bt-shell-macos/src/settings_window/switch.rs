//! The window's on/off control, drawn in the theme ([`super::look`]): the system's switch takes
//! the system's accent and has no other, so a row that is on would be the one thing on the page
//! not in the theme's colours.
//!
//! It is the system switch's size and does what it does: a click or the space bar turns it, the
//! action goes to its target with its tag, it takes the keyboard focus and shows the focus ring,
//! and VoiceOver reads it as a switch with its state. Like the pictured choice it keeps nothing of
//! its own: the window sets it from the file and writes the file from its action.

use std::cell::Cell;

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{
    ClassType, DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send,
};
use objc2_app_kit::{
    NSAccessibilityCheckBoxRole, NSAccessibilitySwitchSubrole, NSBezierPath, NSControl, NSEvent,
    NSView,
};
use objc2_foundation::{NSInteger, NSNumber, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString};

use super::look;
use crate::tab_bar::{Tint, rounded};

/// The system switch's regular size, in points.
const WIDTH: f64 = 38.0;
const HEIGHT: f64 = 22.0;
/// The room between the track's edge and the knob.
const INSET: f64 = 2.0;

pub(super) struct SwitchIvars {
    on: Cell<bool>,
    enabled: Cell<bool>,
    tag: Cell<NSInteger>,
}

define_class!(
    /// An on/off switch in the theme's colours.
    // SAFETY: NSControl has no subclassing requirement beyond what `tag` and `isEnabled` say (the
    // base class keeps neither for a control without a cell — both are answered here); Drop is not
    // implemented.
    #[unsafe(super(NSControl))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriSettingsSwitch"]
    #[ivars = SwitchIvars]
    pub(super) struct ThemeSwitch;

    unsafe impl NSObjectProtocol for ThemeSwitch {}

    impl ThemeSwitch {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(tag))]
        fn tag(&self) -> NSInteger {
            self.ivars().tag.get()
        }

        #[unsafe(method(setTag:))]
        fn set_tag(&self, tag: NSInteger) {
            self.ivars().tag.set(tag);
        }

        #[unsafe(method(isEnabled))]
        fn is_enabled(&self) -> bool {
            self.ivars().enabled.get()
        }

        #[unsafe(method(setEnabled:))]
        fn set_enabled(&self, enabled: bool) {
            if self.ivars().enabled.replace(enabled) != enabled {
                self.redraw();
            }
        }

        #[unsafe(method(intrinsicContentSize))]
        fn intrinsic_content_size(&self) -> NSSize {
            NSSize::new(WIDTH, HEIGHT)
        }

        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool {
            true
        }

        #[unsafe(method(acceptsFirstResponder))]
        fn accepts_first_responder(&self) -> bool {
            self.ivars().enabled.get()
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, _event: &NSEvent) {
            self.flip();
        }

        #[unsafe(method(keyDown:))]
        fn key_down(&self, event: &NSEvent) {
            let space = event
                .charactersIgnoringModifiers()
                .is_some_and(|keys| keys.to_string() == " ");
            if space {
                self.flip();
            } else {
                // SAFETY: the superclass's own `keyDown:` with the event it was given: the key
                // goes on along the responder chain.
                unsafe {
                    let _: () = msg_send![super(self), keyDown: event];
                }
            }
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            self.paint();
        }

        #[unsafe(method(focusRingMaskBounds))]
        fn focus_ring_mask_bounds(&self) -> NSRect {
            self.bounds()
        }

        #[unsafe(method(drawFocusRingMask))]
        fn draw_focus_ring_mask(&self) {
            track_path(self.bounds()).fill();
        }

        #[unsafe(method(isAccessibilityElement))]
        fn is_accessibility_element(&self) -> bool {
            true
        }

        #[unsafe(method_id(accessibilityRole))]
        fn accessibility_role(&self) -> Option<Retained<NSString>> {
            // SAFETY: AppKit's constant role name, alive for the process.
            Some(unsafe { NSAccessibilityCheckBoxRole }.retain())
        }

        #[unsafe(method_id(accessibilitySubrole))]
        fn accessibility_subrole(&self) -> Option<Retained<NSString>> {
            // SAFETY: AppKit's constant subrole name, alive for the process.
            Some(unsafe { NSAccessibilitySwitchSubrole }.retain())
        }

        #[unsafe(method_id(accessibilityValue))]
        fn accessibility_value(&self) -> Option<Retained<NSNumber>> {
            Some(NSNumber::new_bool(self.ivars().on.get()))
        }

        #[unsafe(method(isAccessibilityEnabled))]
        fn is_accessibility_enabled(&self) -> bool {
            self.ivars().enabled.get()
        }

        #[unsafe(method(accessibilityPerformPress))]
        fn accessibility_press(&self) -> bool {
            self.flip();
            true
        }
    }
);

impl ThemeSwitch {
    pub(super) fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(SwitchIvars {
            on: Cell::new(false),
            enabled: Cell::new(true),
            tag: Cell::new(0),
        });
        // SAFETY: `init` is NSControl's designated initializer (zero frame); the size is the
        // intrinsic one.
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        this.setTranslatesAutoresizingMaskIntoConstraints(false);
        this
    }

    pub(super) fn is_on(&self) -> bool {
        self.ivars().on.get()
    }

    /// Shows `on` (the file's value); sends no action.
    pub(super) fn set_on(&self, on: bool) {
        if self.ivars().on.replace(on) != on {
            self.redraw();
        }
    }

    /// The user turned it: the other state, and the action to the target.
    fn flip(&self) {
        if !self.ivars().enabled.get() {
            return;
        }
        self.ivars().on.set(!self.ivars().on.get());
        self.redraw();
        // SAFETY: the action and target are the ones the window gave this control; the target is
        // a weak reference to an object that lives for the whole process.
        unsafe {
            let _ = self.sendAction_to(self.action(), self.target().as_deref());
        }
    }

    fn redraw(&self) {
        let view: &NSView = self.as_super().as_super();
        view.setNeedsDisplay(true);
    }

    /// The track in the accent when on, a step off the card when off, and the knob at its end: the
    /// accent's mark when on, the theme's light end when off (the foreground in a dark theme, the
    /// background, edged, in a light one).
    fn paint(&self) {
        let palette = look::palette();
        let on = self.ivars().on.get();
        let strength = if self.ivars().enabled.get() {
            1.0
        } else {
            0.45
        };
        let bounds = self.bounds();
        let track = Tint::of(
            if on {
                palette.accent
            } else {
                palette.foreground
            },
            if on { strength } else { 0.16 * strength },
        );
        track.color().setFill();
        track_path(bounds).fill();
        let side = bounds.size.height - 2.0 * INSET;
        let x = if on {
            bounds.size.width - INSET - side
        } else {
            INSET
        };
        let knob = NSRect::new(NSPoint::new(x, INSET), NSSize::new(side, side));
        let (fill, edge) = if on {
            (look::on_accent(&palette), None)
        } else if palette.dark {
            (palette.foreground, None)
        } else {
            (
                palette.background,
                Some(Tint::of(palette.separator, strength)),
            )
        };
        rounded(knob, side / 2.0, Tint::of(fill, strength), edge);
    }
}

/// The track: the whole control, round at both ends.
fn track_path(bounds: NSRect) -> Retained<NSBezierPath> {
    let radius = bounds.size.height / 2.0;
    NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(bounds, radius, radius)
}

/// The window's switches are this control: the action's sender is downcast from `sender`.
pub(super) fn of(sender: Option<&AnyObject>) -> Option<&ThemeSwitch> {
    sender.and_then(|sender| sender.downcast_ref::<ThemeSwitch>())
}
