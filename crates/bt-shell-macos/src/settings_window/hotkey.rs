//! Settings ▸ Hotkey Window: the row that records the shortcut.
//!
//! The rest of the page is the window's ordinary rows (two switches, three choices, two numbers:
//! `pages::HOTKEY`); the shortcut is the one row that listens to the keyboard, so it is a
//! [`ShortcutRecorder`] — the field the Keyboard page uses — and the rule of what it may record
//! is not drawn here but asked of `bt_core`: **the one rule** ([`Shortcut::for_global`]) the file's
//! reading asks too, so what the page accepts is what the file would.
//!
//! What the row holds is only the note of a refusal (a key that would be taken from every
//! application): the shortcut itself is the file's and comes back on screen from there
//! (`SettingsWindow::refresh`), like every other row's value. The field tells its news inside the
//! event it came from, so the page works on it a turn later ([`DispatchQueue`]).
//!
//! What was recorded is written alone (`SettingsEdit::HotkeyShortcut`): the page writes the
//! shortcut and nothing else. **Remove** takes it away by the same path as ⌫ — also while the
//! field listens, and for a double tap too, whose tap comes down with the key — and is dimmed,
//! never hidden, while there is none. A write that did not happen is said in the row.
//!
//! **The right of the row is the same in every state**: the field and Remove, each of a width
//! that is its own and is not given up to anything beside them. What a key waits for (the
//! permission a double tap needs) is said **under** the row's top line, as the Keyboard page says
//! a pending key: a note as wide as the row and the button that answers it on a line of its own.
//! The row grows by that and only while the key waits.

use std::cell::{Cell, RefCell};

use bt_core::{Modifier, Settings, SettingsEdit, Shortcut};
use bt_shell_common::shortcuts::{Tapped, glyphs};
use dispatch2::DispatchQueue;
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::{
    NSApplication, NSButton, NSFont, NSLayoutAttribute, NSLayoutConstraintOrientation,
    NSLayoutPriorityRequired, NSStackView, NSTextField, NSUserInterfaceLayoutOrientation, NSView,
};
use objc2_foundation::{NSEdgeInsets, NSString, ns_string};

use super::look::{self, Ink};

use super::recorder::{Recorded, ShortcutRecorder};
use super::{ROW_PAD_Y, ROW_WIDTH};
use crate::app;

/// What the field says while it holds no shortcut.
const NONE: &str = "Not set";

thread_local! {
    /// Why the last key pressed was refused, until the next thing the field hears (the page is
    /// on the main thread only).
    static REFUSED: RefCell<Option<String>> = const { RefCell::new(None) };
    /// Why the last shortcut could not be written (the file is locked, unreadable or not ours to
    /// write): said in the row until the person does something else ([`unsaved_after`]).
    static UNSAVED: RefCell<Option<String>> = const { RefCell::new(None) };
    /// The file holds a shortcut line nobody can read (the last `show` said so): Remove takes it
    /// away although the settings read none.
    static UNREADABLE: Cell<bool> = const { Cell::new(false) };
}

/// What the page does with what the field heard.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Answer {
    /// Writes the shortcut (`None` takes it away) and lets the keyboard go.
    Write(Option<Shortcut>),
    /// Refuses with a note, and the field keeps listening for another key.
    Refuse(String),
    /// Lets the keyboard go and writes nothing.
    Leave,
}

/// What each thing the field hears comes to. Pure: the rule is `Shortcut::for_global`'s.
pub(super) fn answer(heard: Recorded) -> Answer {
    match heard {
        Recorded::Pressed(chord) => match Shortcut::Chord(chord).for_global() {
            Ok(shortcut) => Answer::Write(Some(shortcut)),
            Err(refusal) => Answer::Refuse(refusal.to_string()),
        },
        Recorded::Twice(Tapped::Modifier(modifier)) => twice(modifier),
        Recorded::Twice(Tapped::Function) => {
            Answer::Refuse("fn is not a key a shortcut can use".to_owned())
        }
        Recorded::Delete => Answer::Write(None),
        Recorded::Escape | Recorded::Lost => Answer::Leave,
    }
}

fn twice(modifier: Modifier) -> Answer {
    Answer::Write(Some(Shortcut::Twice(modifier)))
}

/// The note of a write that did not happen, after the field heard `heard` and (when it wrote)
/// `saved`: the reason of a write that failed, none after one that went through, and none when
/// the person has done something new (a key, esc, ⌫, a double tap). Only the keyboard leaving the
/// field keeps it: that is what the failed write itself does, one turn later.
pub(super) fn unsaved_after(
    before: Option<String>,
    heard: Recorded,
    saved: Option<Result<(), String>>,
) -> Option<String> {
    match (heard, saved) {
        (_, Some(Err(why))) => Some(why),
        (_, Some(Ok(()))) => None,
        (Recorded::Lost, None) => before,
        (_, None) => None,
    }
}

/// What the field says for `shortcut`: its glyphs, or that there is none.
pub(super) fn prompt(shortcut: Option<Shortcut>) -> String {
    shortcut
        .map(|shortcut| glyphs(&shortcut).unwrap_or_else(|| shortcut.to_string()))
        .unwrap_or_else(|| NONE.to_owned())
}

/// The least width of Remove: the title's own is what it takes, this is the floor under a title
/// that is short.
const REMOVE_MIN_WIDTH: f64 = 64.0;

/// The shortcut's row: on the right the field and the Remove button (always there, dimmed while
/// there is no shortcut), and under the row's top line what the key waits for — a note and the
/// button that sends the person to the permission a double tap needs — while it waits.
pub(super) struct ShortcutRow {
    recorder: Retained<ShortcutRecorder>,
    remove: Retained<NSButton>,
    allow: Retained<NSButton>,
    /// What the key waits for, in the warning's colour.
    note: Retained<NSTextField>,
    /// The note and the button, as wide as the row; takes no room while the key waits for nothing.
    under: Retained<NSStackView>,
    view: Retained<NSView>,
}

impl ShortcutRow {
    /// `remove` and `allow` are the window's buttons (they hold the actions): Remove, and the
    /// permission's.
    pub(super) fn new(
        mtm: MainThreadMarker,
        remove: Retained<NSButton>,
        allow: Retained<NSButton>,
    ) -> Self {
        let recorder = ShortcutRecorder::new(mtm, NONE, |heard| {
            // Told inside the event that made it: the work is done a turn later.
            DispatchQueue::main().exec_async(move || {
                // audit: a block running on the main queue is on the main thread by definition.
                let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
                work(mtm, heard);
            });
        });
        // Remove is as wide as its title asks, whatever the row has beside it: a button that the
        // row's text may squeeze draws a sliver.
        let width = remove.fittingSize().width.ceil().max(REMOVE_MIN_WIDTH);
        remove
            .widthAnchor()
            .constraintEqualToConstant(width)
            .setActive(true);
        remove.setContentCompressionResistancePriority_forOrientation(
            NSLayoutPriorityRequired,
            NSLayoutConstraintOrientation::Horizontal,
        );
        remove.setContentHuggingPriority_forOrientation(
            NSLayoutPriorityRequired,
            NSLayoutConstraintOrientation::Horizontal,
        );
        let view = super::hstack(mtm, &[&recorder, &remove], 8.0);
        // The Keyboard page's pending key: a note, then the button, one under the other.
        let note = super::wrapping_text_label(ns_string!(""), mtm);
        note.setFont(Some(&NSFont::systemFontOfSize(
            NSFont::smallSystemFontSize(),
        )));
        note.setPreferredMaxLayoutWidth(ROW_WIDTH);
        let under = super::stack(
            mtm,
            &[&note, &allow],
            NSUserInterfaceLayoutOrientation::Vertical,
            6.0,
            NSLayoutAttribute::Leading,
        );
        note.setHidden(true);
        allow.setHidden(true);
        let row = Self {
            recorder,
            remove,
            allow,
            note,
            under,
            view,
        };
        row.leave_room(false);
        row
    }

    /// The field and its button, to put in the row.
    pub(super) fn view(&self) -> Retained<NSView> {
        self.view.clone()
    }

    /// What the row says under its top line, to put in the row.
    pub(super) fn under(&self) -> Retained<NSView> {
        Retained::into_super(self.under.clone())
    }

    /// The field, Remove, the permission's button and the note, to measure.
    pub(super) fn parts(&self) -> (&NSView, &NSButton, &NSButton, &NSTextField) {
        (&self.recorder, &self.remove, &self.allow, &self.note)
    }

    /// The part under the top line: the room under the button while there is something in it,
    /// none while there is not.
    fn leave_room(&self, waiting: bool) {
        self.under.setEdgeInsets(NSEdgeInsets {
            top: 0.0,
            left: 0.0,
            bottom: if waiting { ROW_PAD_Y } else { 0.0 },
            right: 0.0,
        });
    }

    /// Says what the file holds, and whether the field may be used (the file is not locked);
    /// `waiting` is what the key waits for the permission to hear a double tap with — the note
    /// the row says under its top line — and the button that answers it is there while it is
    /// `Some`; `unreadable` is a line of the file that holds a shortcut that cannot be read
    /// (Remove takes that away too).
    pub(super) fn show(
        &self,
        settings: &Settings,
        enabled: bool,
        waiting: Option<&str>,
        unreadable: bool,
    ) {
        self.recorder
            .set_prompt(&prompt(settings.hotkey_window.shortcut));
        self.recorder.set_enabled(enabled);
        // There is something to take away only while there is a shortcut; the button stays where
        // it is and says so by its colour.
        UNREADABLE.set(unreadable);
        let removable = enabled && (settings.hotkey_window.shortcut.is_some() || unreadable);
        self.remove.setEnabled(removable);
        super::ink_button(&self.remove, removable);
        if let Some(words) = waiting {
            self.note.setStringValue(&NSString::from_str(words));
            self.note.setTextColor(Some(&look::ink(Ink::Warning)));
        }
        self.note.setHidden(waiting.is_none());
        self.allow.setHidden(waiting.is_none());
        self.allow.setEnabled(enabled);
        super::ink_button(&self.allow, enabled);
        self.leave_room(waiting.is_some());
    }

    /// The note of the last key refused, if the field is still waiting for another, or of the
    /// write that did not happen.
    pub(super) fn refused(&self) -> Option<String> {
        REFUSED
            .with_borrow(Clone::clone)
            .or_else(|| UNSAVED.with_borrow(Clone::clone))
    }
}

/// Remove: takes the shortcut away as ⌫ does, whether or not the field is listening. A turn
/// later, like what the field tells: the page rebuilds the row the button sits in.
pub(super) fn remove() {
    DispatchQueue::main().exec_async(|| {
        // audit: a block running on the main queue is on the main thread by definition.
        let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
        work(mtm, Recorded::Delete);
    });
}

/// Does what [`answer`] says to what the field heard.
fn work(mtm: MainThreadMarker, heard: Recorded) {
    let Some(delegate) = app::delegate(mtm) else {
        return;
    };
    // What was written, when something was: its error is the row's note.
    let mut saved = None;
    match answer(heard) {
        Answer::Refuse(note) => {
            REFUSED.set(Some(note));
        }
        Answer::Leave => {
            REFUSED.set(None);
            stop_listening(mtm);
        }
        Answer::Write(shortcut) => {
            REFUSED.set(None);
            stop_listening(mtm);
            // Written when it changes the file: a different shortcut, or one taken away from a
            // line the file's reading could not make a shortcut of (it reads as none and is still
            // there to go). ⌫ with nothing set writes nothing: no section is added to the file.
            let unreadable = shortcut.is_none() && UNREADABLE.get();
            if unreadable || delegate.file_settings().hotkey_window.shortcut != shortcut {
                saved = Some(delegate.save_edits(&[SettingsEdit::HotkeyShortcut(shortcut)]));
            }
            // A double tap is heard by a listening tap, which needs the person's permission: asked
            // here, where the person just chose it, and nowhere at launch.
            if matches!(shortcut, Some(Shortcut::Twice(_))) {
                delegate.allow_listening(false);
            }
        }
    }
    UNSAVED.with_borrow_mut(|note| *note = unsaved_after(note.take(), heard, saved));
    delegate.refresh_settings_window();
}

/// The window forgets the note of a write that did not happen: it was closed.
pub(super) fn forget_unsaved() {
    UNSAVED.set(None);
}

/// The keyboard goes back to the window: the field stops listening. A field that is not
/// listening has nothing to let go of — and a click on another field must not lose the keyboard
/// the field it went to has just taken.
fn stop_listening(mtm: MainThreadMarker) {
    if super::recorder::is_listening(mtm)
        && let Some(window) = NSApplication::sharedApplication(mtm).keyWindow()
    {
        window.makeFirstResponder(None);
    }
}

#[cfg(test)]
mod tests {
    use bt_core::{Chord, Key, Mods, NamedKey};

    use super::*;

    fn chord(mods: Mods, key: Key) -> Recorded {
        Recorded::Pressed(Chord::new(mods, key))
    }

    /// A key is recorded by the rule the file's reading has: a modifier and a key, or a
    /// function key; a bare letter or shift's letter would be taken from every application.
    #[test]
    fn a_key_is_recorded_by_the_rule_the_file_has() {
        let option = Mods::NONE.with(Modifier::Option);
        let space = chord(option, Key::Named(NamedKey::Space));
        assert_eq!(
            answer(space),
            Answer::Write(Shortcut::parse_global("option+space").ok())
        );
        // A function key is a key of its own, with no modifier.
        assert!(matches!(
            answer(chord(Mods::NONE, Key::Named(NamedKey::F(12)))),
            Answer::Write(Some(_))
        ));
        // A bare letter, and one with shift alone, are refused with the file's own sentence.
        for refused in [
            chord(Mods::NONE, Key::Char('a')),
            chord(Mods::NONE.with(Modifier::Shift), Key::Char('a')),
        ] {
            let Answer::Refuse(note) = answer(refused) else {
                panic!("{refused:?} was accepted");
            };
            assert!(note.contains("every application"), "{note}");
        }
    }

    /// One modifier tapped twice is a shortcut; fn is not one.
    #[test]
    fn a_double_tap_is_recorded_for_one_modifier_and_never_for_fn() {
        assert_eq!(
            answer(Recorded::Twice(Tapped::Modifier(Modifier::Option))),
            Answer::Write(Some(Shortcut::Twice(Modifier::Option)))
        );
        assert!(matches!(
            answer(Recorded::Twice(Tapped::Function)),
            Answer::Refuse(_)
        ));
    }

    /// ⌫ takes the shortcut away; esc and leaving the field write nothing.
    #[test]
    fn delete_takes_the_shortcut_away_and_escape_and_leaving_write_nothing() {
        assert_eq!(answer(Recorded::Delete), Answer::Write(None));
        assert_eq!(answer(Recorded::Escape), Answer::Leave);
        assert_eq!(answer(Recorded::Lost), Answer::Leave);
    }

    /// A write that did not happen is said in the row until the person does something else; the
    /// keyboard leaving the field (what the write itself does a turn later) keeps the note.
    #[test]
    fn a_write_that_failed_is_said_until_the_person_does_something_else() {
        let why =
            || Some("settings.toml could not be written; the setting was not saved".to_owned());
        let failed = unsaved_after(None, Recorded::Delete, Some(Err(why().unwrap())));
        assert_eq!(failed, why());
        // The field letting go of the keyboard changes nothing about it.
        assert_eq!(unsaved_after(failed.clone(), Recorded::Lost, None), failed);
        // A write that goes through, or any key, or esc, takes it away.
        assert_eq!(
            unsaved_after(failed.clone(), Recorded::Delete, Some(Ok(()))),
            None
        );
        assert_eq!(unsaved_after(failed.clone(), Recorded::Escape, None), None);
        assert_eq!(unsaved_after(failed.clone(), Recorded::Delete, None), None);
        // And there is nothing to say where nothing failed.
        assert_eq!(unsaved_after(None, Recorded::Lost, None), None);
    }

    /// The row's wiring, as a source scan (the page needs a window server): Remove goes through
    /// the same `work` as ⌫, and a failed write is kept and not dropped; the button is dimmed
    /// and never hidden; the right of the row is the field and Remove, each of a width of its own,
    /// and the permission's button is not in it but under the row's top line.
    #[test]
    fn remove_is_the_same_path_as_delete_and_the_row_keeps_it_in_its_place() {
        let code = include_str!("hotkey.rs");
        let code = &code[..code.find("#[cfg(test)]").expect("the tests")];
        assert!(code.contains("work(mtm, Recorded::Delete)"));
        assert!(code.contains("super::hstack(mtm, &[&recorder, &remove], 8.0)"));
        assert!(
            code.contains(".constraintEqualToConstant(width)")
                && code.contains("NSLayoutPriorityRequired"),
            "Remove is not squeezed"
        );
        assert!(
            code.contains("&[&note, &allow]"),
            "the permission's button is under the note, out of the right group"
        );
        assert!(
            !code.contains("remove.setHidden"),
            "Remove is dimmed, never hidden"
        );
        assert!(code.contains("self.remove.setEnabled(removable)"));
        assert!(
            code.contains("|| unreadable"),
            "a line nobody can read can be removed"
        );
        assert!(
            code.contains("let unreadable = shortcut.is_none() && UNREADABLE.get();")
                && !code.contains("shortcut.is_none() || delegate"),
            "⌫ with nothing set writes nothing"
        );
        assert!(code.contains("if super::recorder::is_listening(mtm)"));
        assert!(
            code.contains("settings.hotkey_window.shortcut.is_some()"),
            "dim while there is no shortcut"
        );
        // The write's answer is read: nothing swallows it.
        let work = &code[code.find("fn work(").expect("work")..];
        let work = &work[..work.find("\n}\n").expect("its end")];
        assert!(work.contains("saved = Some(delegate.save_edits("));
        assert!(
            !work.contains(".ok()"),
            "a failed write is said, not dropped"
        );
        // The window's button reaches it.
        let window = include_str!("../settings_window.rs");
        assert!(window.contains("sel!(removeShortcut:)"));
        assert!(window.contains("hotkey::remove()"));
    }

    /// The field shows the shortcut the file holds, or that it holds none.
    #[test]
    fn the_field_shows_what_the_file_holds() {
        assert_eq!(prompt(None), NONE);
        let held = Shortcut::parse_global("option+space").ok();
        assert_eq!(prompt(held), "\u{2325}Space");
        assert_eq!(
            prompt(Some(Shortcut::Twice(Modifier::Option))),
            "\u{2325} \u{2325}"
        );
    }

    /// What the page writes is the shortcut alone: the edit's place is `[hotkey_window] shortcut`
    /// and its value reads back as the same shortcut.
    #[test]
    fn the_edit_writes_the_shortcut_alone_and_reads_back() {
        let shortcut = Shortcut::parse_global("option+space").expect("a shortcut");
        let edit = SettingsEdit::HotkeyShortcut(Some(shortcut));
        assert_eq!(edit.path(), "hotkey_window.shortcut");
        let text = Settings::with_edit("", &edit).expect("it writes");
        let read = |text: &str| {
            let parsed = Settings::parse_keeping(text, &Settings::default()).expect("it parses");
            assert!(parsed.diagnostics.is_empty(), "{text}");
            parsed.settings
        };
        assert_eq!(read(&text).hotkey_window.shortcut, Some(shortcut));
        // And taking it away writes the empty text, which reads as none.
        let cleared =
            Settings::with_edit(&text, &SettingsEdit::HotkeyShortcut(None)).expect("it writes");
        assert_eq!(read(&cleared).hotkey_window.shortcut, None);
        // Only its own line changed.
        assert_eq!(
            read(&text),
            Settings {
                hotkey_window: bt_core::HotkeyWindowSettings {
                    shortcut: Some(shortcut),
                    ..Settings::default().hotkey_window
                },
                ..Settings::default()
            }
        );
    }
}
