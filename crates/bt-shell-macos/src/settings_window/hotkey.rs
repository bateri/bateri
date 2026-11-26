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
//! shortcut and nothing else.

use std::cell::RefCell;

use bt_core::{Modifier, Settings, SettingsEdit, Shortcut};
use bt_shell_common::shortcuts::{Tapped, glyphs};
use dispatch2::DispatchQueue;
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::{NSApplication, NSView};

use super::recorder::{Recorded, ShortcutRecorder};
use crate::app;

/// What the field says while it holds no shortcut.
const NONE: &str = "Not set";

thread_local! {
    /// Why the last key pressed was refused, until the next thing the field hears (the page is
    /// on the main thread only).
    static REFUSED: RefCell<Option<String>> = const { RefCell::new(None) };
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

/// What the field says for `shortcut`: its glyphs, or that there is none.
pub(super) fn prompt(shortcut: Option<Shortcut>) -> String {
    shortcut
        .map(|shortcut| glyphs(&shortcut).unwrap_or_else(|| shortcut.to_string()))
        .unwrap_or_else(|| NONE.to_owned())
}

/// The shortcut's row: the field, and what it was told to say.
pub(super) struct ShortcutRow {
    recorder: Retained<ShortcutRecorder>,
}

impl ShortcutRow {
    pub(super) fn new(mtm: MainThreadMarker) -> Self {
        let recorder = ShortcutRecorder::new(mtm, NONE, |heard| {
            // Told inside the event that made it: the work is done a turn later.
            DispatchQueue::main().exec_async(move || {
                // audit: a block running on the main queue is on the main thread by definition.
                let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
                work(mtm, heard);
            });
        });
        Self { recorder }
    }

    /// The field, to put in the row.
    pub(super) fn view(&self) -> Retained<NSView> {
        Retained::into_super(self.recorder.clone())
    }

    /// Says what the file holds, and whether the field may be used (the file is not locked).
    pub(super) fn show(&self, settings: &Settings, enabled: bool) {
        self.recorder
            .set_prompt(&prompt(settings.hotkey_window.shortcut));
        self.recorder.set_enabled(enabled);
    }

    /// The note of the last key refused, if the field is still waiting for another.
    pub(super) fn refused(&self) -> Option<String> {
        REFUSED.with_borrow(Clone::clone)
    }
}

/// Does what [`answer`] says to what the field heard.
fn work(mtm: MainThreadMarker, heard: Recorded) {
    let Some(delegate) = app::delegate(mtm) else {
        return;
    };
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
            if delegate.file_settings().hotkey_window.shortcut != shortcut {
                delegate
                    .save_edits(&[SettingsEdit::HotkeyShortcut(shortcut)])
                    .ok();
            }
        }
    }
    delegate.refresh_settings_window();
}

/// The keyboard goes back to the window: the field stops listening.
fn stop_listening(mtm: MainThreadMarker) {
    if let Some(window) = NSApplication::sharedApplication(mtm).keyWindow() {
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
