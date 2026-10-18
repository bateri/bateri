//! The field that listens for a shortcut: it takes the next key before the menu can, shows the
//! modifiers while they are held, and tells what was pressed.
//!
//! Recording must not run the menu: with ⌘W held down the window would close and ⌘Q would quit,
//! so the field answers `performKeyEquivalent:` — AppKit asks the key window's views before the
//! main menu — and `keyDown:` for the keys nobody asks about. What is recorded is the key as the
//! menu reads it (`charactersIgnoringModifiers`, the same mapping as the menu's items:
//! `shortcuts::chord_of`), so the key that works today is the key that is written. esc gives up
//! and ⌫ removes; with a modifier they are keys like any other.
//!
//! The field never acts on what it hears: the news goes to the closure it was given, which is
//! called while the event is still being handled — and the page that listens rebuilds the table
//! the field sits in, so the closure only queues the news and the page works on it a turn later.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use bt_core::{Chord, Mods, Shortcut};
use bt_shell_common::palette::Modifiers;
use bt_shell_common::shortcuts::{chord_of, glyphs};
use objc2::rc::Retained;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{
    NSBezierPath, NSColor, NSEvent, NSEventModifierFlags, NSEventType, NSFont, NSTextField, NSView,
};
use objc2_foundation::{NSObjectProtocol, NSPoint, NSRect, NSSize, NSString};

/// What the field heard.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Recorded {
    /// A key, as the menu reads it.
    Pressed(Chord),
    /// esc: give up.
    Escape,
    /// ⌫ alone: take the shortcut away.
    Delete,
    /// The keyboard went elsewhere (a click, another field): stop listening.
    Lost,
}

/// The key codes of the two keys with a meaning of their own.
const ESCAPE: u16 = 53;
const BACKSPACE: u16 = 51;

/// The field's width and height, in points.
const WIDTH: f64 = 150.0;
const HEIGHT: f64 = 22.0;

/// What the field tells, to whoever listens.
type News = Rc<dyn Fn(Recorded)>;

pub(super) struct RecorderIvars {
    /// The modifiers held now, shown as they are pressed.
    held: Cell<Mods>,
    listening: Cell<bool>,
    label: Retained<NSTextField>,
    prompt: String,
    news: RefCell<Option<News>>,
}

define_class!(
    // SAFETY: NSView is designed for subclassing; the field implements no `Drop` and is born
    // with `initWithFrame:`.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriShortcutRecorder"]
    #[ivars = RecorderIvars]
    pub(super) struct ShortcutRecorder;

    unsafe impl NSObjectProtocol for ShortcutRecorder {}

    impl ShortcutRecorder {
        #[unsafe(method(acceptsFirstResponder))]
        fn accepts_first_responder(&self) -> bool {
            true
        }

        #[unsafe(method(becomeFirstResponder))]
        fn become_first_responder(&self) -> bool {
            self.ivars().listening.set(true);
            self.show();
            true
        }

        #[unsafe(method(resignFirstResponder))]
        fn resign_first_responder(&self) -> bool {
            if self.ivars().listening.replace(false) {
                self.ivars().held.set(Mods::default());
                self.show();
                self.tell(Recorded::Lost);
            }
            true
        }

        /// Before the menu: ⌘W while listening is the key ⌘W.
        #[unsafe(method(performKeyEquivalent:))]
        fn perform_key_equivalent(&self, event: &NSEvent) -> bool {
            self.hear(event)
        }

        #[unsafe(method(keyDown:))]
        fn key_down(&self, event: &NSEvent) {
            self.hear(event);
        }

        #[unsafe(method(flagsChanged:))]
        fn flags_changed(&self, event: &NSEvent) {
            if self.ivars().listening.get() {
                self.ivars().held.set(mods_of(event.modifierFlags()));
                self.show();
            }
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, _event: &NSEvent) {
            if let Some(window) = self.window() {
                window.makeFirstResponder(Some(self));
            }
        }

        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            let bounds = self.bounds();
            let inner = NSRect::new(
                NSPoint::new(1.0, 1.0),
                NSSize::new(bounds.size.width - 2.0, bounds.size.height - 2.0),
            );
            let path = NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(inner, 5.0, 5.0);
            NSColor::controlBackgroundColor().setFill();
            path.fill();
            if self.ivars().listening.get() {
                NSColor::controlAccentColor().setStroke();
                path.setLineWidth(2.0);
            } else {
                NSColor::separatorColor().setStroke();
                path.setLineWidth(1.0);
            }
            path.stroke();
        }
    }
);

/// The four modifiers a held-key mask holds.
fn mods_of(flags: NSEventModifierFlags) -> Mods {
    Mods::new(
        flags.contains(NSEventModifierFlags::Control),
        flags.contains(NSEventModifierFlags::Option),
        flags.contains(NSEventModifierFlags::Shift),
        flags.contains(NSEventModifierFlags::Command),
    )
}

impl ShortcutRecorder {
    /// A field that says `prompt` until a modifier is held, and tells `news` what it hears.
    pub(super) fn new(
        mtm: MainThreadMarker,
        prompt: &str,
        news: impl Fn(Recorded) + 'static,
    ) -> Retained<Self> {
        let label = NSTextField::labelWithString(&NSString::from_str(prompt), mtm);
        label.setFont(Some(&NSFont::systemFontOfSize(
            NSFont::smallSystemFontSize(),
        )));
        label.setTextColor(Some(&NSColor::secondaryLabelColor()));
        let this = Self::alloc(mtm).set_ivars(RecorderIvars {
            held: Cell::new(Mods::default()),
            listening: Cell::new(false),
            label: label.clone(),
            prompt: prompt.to_owned(),
            news: RefCell::new(Some(Rc::new(news))),
        });
        // SAFETY: `initWithFrame:` is NSView's designated initializer and the ivars are set.
        let this: Retained<Self> = unsafe {
            msg_send![super(this), initWithFrame: NSRect::new(NSPoint::ZERO, NSSize::new(WIDTH, HEIGHT))]
        };
        this.show();
        this.setTranslatesAutoresizingMaskIntoConstraints(false);
        label.setTranslatesAutoresizingMaskIntoConstraints(false);
        this.addSubview(&label);
        for constraint in [
            this.widthAnchor().constraintEqualToConstant(WIDTH),
            this.heightAnchor().constraintEqualToConstant(HEIGHT),
            label
                .centerXAnchor()
                .constraintEqualToAnchor(&this.centerXAnchor()),
            label
                .centerYAnchor()
                .constraintEqualToAnchor(&this.centerYAnchor()),
        ] {
            constraint.setActive(true);
        }
        this
    }

    /// Takes the keyboard.
    pub(super) fn listen(&self) {
        if let Some(window) = self.window() {
            window.makeFirstResponder(Some(self));
        }
    }

    /// What the field says: the modifiers held with an ellipsis, or what it was told to say.
    fn show(&self) {
        let iv = self.ivars();
        let held = iv.held.get();
        let (text, quiet) = if !iv.listening.get() {
            (iv.prompt.clone(), true)
        } else if held.is_empty() {
            ("Type shortcut".to_owned(), true)
        } else {
            (
                format!("{}…", glyphs(&Shortcut::Mods(held)).unwrap_or_default()),
                false,
            )
        };
        iv.label.setStringValue(&NSString::from_str(&text));
        let color = if quiet {
            NSColor::secondaryLabelColor()
        } else {
            NSColor::labelColor()
        };
        iv.label.setTextColor(Some(&color));
        self.setNeedsDisplay(true);
    }

    fn tell(&self, heard: Recorded) {
        // The closure is called with no borrow held: it may drop this very field.
        let news = self.ivars().news.borrow().clone();
        if let Some(news) = news {
            news(heard);
        }
    }

    /// A key down: what it is as a shortcut, or nothing for a key that is none. `true` when the
    /// field took it (while listening it takes every key, so the menu never sees one).
    fn hear(&self, event: &NSEvent) -> bool {
        if !self.ivars().listening.get() || event.r#type() != NSEventType::KeyDown {
            return false;
        }
        if event.isARepeat() {
            return true;
        }
        let mods = mods_of(event.modifierFlags());
        let code = event.keyCode();
        let heard = if code == ESCAPE && mods.is_empty() {
            Some(Recorded::Escape)
        } else if code == BACKSPACE && mods.is_empty() {
            Some(Recorded::Delete)
        } else {
            let characters = event
                .charactersIgnoringModifiers()
                .map(|text| text.to_string())
                .unwrap_or_default();
            chord_of(&characters, Modifiers::from(mods)).map(Recorded::Pressed)
        };
        // A key that is no key (a dead key, a media key) was not pressed: nothing to tell.
        if let Some(heard) = heard {
            self.ivars().held.set(Mods::default());
            self.show();
            self.tell(heard);
        }
        true
    }
}
