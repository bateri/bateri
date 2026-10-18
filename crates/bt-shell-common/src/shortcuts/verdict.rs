//! What recording a shortcut comes to: the verdict on a key the user pressed for a command.
//!
//! Nine outcomes, in the order they are asked:
//!
//! 1. [`Verdict::MacOs`] — macOS keeps the key (⌘Space, ⌃←): bateri never gets it. **Rejected.**
//! 2. [`Verdict::NeedsModifier`] — a letter with no ⌘, ⌃ or ⌥ is typing. **Rejected.**
//! 3. [`Verdict::FamilyNeedsKey`] — a family is given its modifiers with one of its own keys
//!    (a digit, an arrow); anything else is no way to say them. **Rejected.**
//! 4. [`Verdict::FamilyClash`] — one of the family's items would take a key that is something
//!    else's. **Rejected.**
//! 5. [`Verdict::TakenBy`] — another command has the key. **Asks** (Use Here).
//! 6. [`Verdict::LineEditing`] — the key edits the line you type. **Asks** (Use Here).
//! 7. [`Verdict::ProgramKey`] — ⌃ without ⌘ goes to the program in the terminal. **Asks**
//!    (Assign Anyway).
//! 8. [`Verdict::TypesCharacter`] — ⌥ without ⌘ types a character on most keyboards. **Asks**
//!    (Assign Anyway).
//! 9. [`Verdict::Free`] — the key is the command's. **Accepted.**
//!
//! The texts of the notes and the buttons are the page's; this is the decision. A family is
//! judged on the item the user pressed, and what is written is the modifiers.

use bt_core::{Chord, Key, Mods, NamedKey, Shortcut};

use super::resolve::Resolved;
use super::table::{Command, Family, by_name};
use crate::keys::{LineKey, line_editing};

/// A key macOS keeps for itself, and what it uses it for ("for Spotlight").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MacKey {
    pub chord: Chord,
    pub reason: &'static str,
}

const fn mac(
    ctrl: bool,
    option: bool,
    shift: bool,
    cmd: bool,
    key: Key,
    reason: &'static str,
) -> MacKey {
    MacKey {
        chord: Chord::plain(Mods::new(ctrl, option, shift, cmd), key),
        reason,
    }
}

const fn named(key: NamedKey) -> Key {
    Key::Named(key)
}

/// The keys that never reach an application: the system takes them before bateri's menu is
/// asked. Binding one would be a shortcut that does nothing.
pub const MACOS_KEYS: &[MacKey] = &[
    mac(
        false,
        false,
        false,
        true,
        named(NamedKey::Space),
        "for Spotlight",
    ),
    mac(
        false,
        false,
        false,
        true,
        named(NamedKey::Tab),
        "to switch apps",
    ),
    mac(
        false,
        false,
        true,
        true,
        named(NamedKey::Tab),
        "to switch apps",
    ),
    mac(
        false,
        false,
        false,
        true,
        Key::Char('`'),
        "to go to an app’s next window",
    ),
    mac(false, false, true, true, Key::Char('3'), "for screenshots"),
    mac(false, false, true, true, Key::Char('4'), "for screenshots"),
    mac(false, false, true, true, Key::Char('5'), "for screenshots"),
    mac(
        false,
        true,
        false,
        true,
        Key::Char('d'),
        "to show and hide the Dock",
    ),
    mac(
        true,
        false,
        false,
        true,
        Key::Char('q'),
        "to lock the screen",
    ),
    mac(
        false,
        true,
        false,
        true,
        named(NamedKey::Escape),
        "for Force Quit",
    ),
    mac(
        true,
        false,
        false,
        false,
        named(NamedKey::Up),
        "for Mission Control",
    ),
    mac(
        true,
        false,
        false,
        false,
        named(NamedKey::Down),
        "for App Exposé",
    ),
    mac(
        true,
        false,
        false,
        false,
        named(NamedKey::Left),
        "to move between spaces",
    ),
    mac(
        true,
        false,
        false,
        false,
        named(NamedKey::Right),
        "to move between spaces",
    ),
    mac(
        true,
        false,
        false,
        true,
        Key::Char('f'),
        "for Enter Full Screen in View",
    ),
    mac(
        true,
        false,
        false,
        true,
        named(NamedKey::Space),
        "for Emoji & Symbols in Edit",
    ),
];

/// What a key can never be: the macOS's, or typing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    MacOs(&'static MacKey),
    NeedsModifier,
}

/// Whether the chord has a modifier that makes a key a shortcut: ⌘, ⌃ or ⌥ — Shift alone is
/// typing a capital — or is a function key, which is a shortcut by itself.
fn is_shortcut(chord: &Chord) -> bool {
    chord.mods.cmd
        || chord.mods.ctrl
        || chord.mods.option
        || matches!(chord.key, Key::Named(NamedKey::F(_)))
}

/// Why a chord can be no command's, if it cannot. The one rule behind the recorder and the
/// file's reading: a shortcut that macOS keeps, or that is typing, does nothing good anywhere.
pub fn refusal(chord: &Chord) -> Option<Refusal> {
    if let Some(key) = MACOS_KEYS.iter().find(|key| key.chord == *chord) {
        return Some(Refusal::MacOs(key));
    }
    (!is_shortcut(chord)).then_some(Refusal::NeedsModifier)
}

/// What an item of a family would take its key from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Clash {
    Command(&'static Command),
    MacOs(&'static MacKey),
    LineEditing(&'static LineKey),
}

/// The verdict on a pressed key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// Accepted.
    Free,
    /// Another command has the key.
    TakenBy(&'static Command),
    /// The key edits the line you type.
    LineEditing(&'static LineKey),
    /// ⌃ without ⌘: the key goes to the program in the terminal.
    ProgramKey,
    /// ⌥ without ⌘: the key types a character.
    TypesCharacter,
    /// macOS keeps the key.
    MacOs(&'static MacKey),
    /// A letter with no ⌘, ⌃ or ⌥.
    NeedsModifier,
    /// The command is a family and the key is not one of its own (a digit, an arrow).
    FamilyNeedsKey(Family),
    /// One of the family's items would take this chord from someone else.
    FamilyClash { chord: Chord, with: Clash },
}

impl Verdict {
    /// No button: the field keeps listening.
    pub fn is_refused(&self) -> bool {
        matches!(
            self,
            Self::MacOs(_)
                | Self::NeedsModifier
                | Self::FamilyNeedsKey(_)
                | Self::FamilyClash { .. }
        )
    }

    /// A note with a button: the key can still be given, knowing what it costs.
    pub fn needs_confirmation(&self) -> bool {
        matches!(
            self,
            Self::TakenBy(_) | Self::LineEditing(_) | Self::ProgramKey | Self::TypesCharacter
        )
    }

    /// The verdict takes a key from another command, so the other command is left with none
    /// (Use Here writes both lines).
    pub fn takes_from(&self) -> Option<&'static Command> {
        match self {
            Self::TakenBy(command) => Some(command),
            _ => None,
        }
    }
}

/// A verdict, with what would be written if it is accepted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ruling {
    /// The shortcut the command would have: the pressed chord, or for a family its modifiers.
    pub stored: Shortcut,
    pub verdict: Verdict,
}

/// The two warnings about the cost of a key the terminal or the keyboard also uses.
fn costly(chord: &Chord) -> Verdict {
    let mods = chord.mods;
    if mods.ctrl && !mods.cmd && !matches!(chord.key, Key::Named(NamedKey::F(_))) {
        return Verdict::ProgramKey;
    }
    // ⌥ with a character types another character; with an arrow, a function key or Tab it
    // types nothing.
    if mods.option && !mods.cmd && matches!(chord.key, Key::Char(_) | Key::Named(NamedKey::Space)) {
        return Verdict::TypesCharacter;
    }
    Verdict::Free
}

impl Resolved {
    /// The verdict on `pressed` for the command `command` (a name from the table) — `None` for a
    /// name that is not one.
    pub fn judge(&self, command: &str, pressed: &Chord) -> Option<Ruling> {
        let target = by_name(command)?;
        Some(match target.family {
            None => Ruling {
                stored: Shortcut::Chord(*pressed),
                verdict: self.judge_chord(target, pressed),
            },
            Some(family) => Ruling {
                stored: Shortcut::Mods(pressed.mods),
                verdict: self.judge_family(target, family, pressed),
            },
        })
    }

    fn judge_chord(&self, target: &'static Command, pressed: &Chord) -> Verdict {
        match refusal(pressed) {
            Some(Refusal::MacOs(key)) => return Verdict::MacOs(key),
            Some(Refusal::NeedsModifier) => return Verdict::NeedsModifier,
            None => {}
        }
        if let Some(owner) = self.owner_of(pressed, target.name) {
            return Verdict::TakenBy(owner);
        }
        if let Some(line) = line_editing(pressed) {
            return Verdict::LineEditing(line);
        }
        costly(pressed)
    }

    fn judge_family(&self, target: &'static Command, family: Family, pressed: &Chord) -> Verdict {
        // The family's modifiers: what is held when one of its keys is pressed.
        if !(pressed.mods.cmd || pressed.mods.ctrl || pressed.mods.option) {
            return Verdict::NeedsModifier;
        }
        if !family.has(pressed.key) {
            return Verdict::FamilyNeedsKey(family);
        }
        let stored = Shortcut::Mods(pressed.mods);
        for item in target.items(&stored) {
            let chord = item.chord;
            if let Some(key) = MACOS_KEYS.iter().find(|key| key.chord == chord) {
                return Verdict::FamilyClash {
                    chord,
                    with: Clash::MacOs(key),
                };
            }
            if let Some(owner) = self.owner_of(&chord, target.name) {
                return Verdict::FamilyClash {
                    chord,
                    with: Clash::Command(owner),
                };
            }
            if let Some(line) = line_editing(&chord) {
                return Verdict::FamilyClash {
                    chord,
                    with: Clash::LineEditing(line),
                };
            }
        }
        costly(pressed)
    }
}

#[cfg(test)]
mod tests {
    use bt_core::{Modifier, Settings};

    use super::*;
    use crate::shortcuts::resolve::resolve;

    fn chord(text: &str) -> Chord {
        match Shortcut::parse(text) {
            Ok(Shortcut::Chord(chord)) => chord,
            other => panic!("`{text}` is not a chord: {other:?}"),
        }
    }

    fn verdict(command: &str, pressed: &str) -> Verdict {
        resolve(&Settings::default())
            .judge(command, &chord(pressed))
            .expect("a command")
            .verdict
    }

    fn name_of(verdict: &Verdict) -> Option<&'static str> {
        match verdict {
            Verdict::TakenBy(command) => Some(command.name),
            _ => None,
        }
    }

    // The nine outcomes, one test each.

    #[test]
    fn a_free_key_is_accepted() {
        let ruling = resolve(&Settings::default())
            .judge("copy_last_output", &chord("shift+cmd+o"))
            .expect("a command");
        assert_eq!(ruling.verdict, Verdict::Free);
        assert_eq!(ruling.stored, Shortcut::Chord(chord("shift+cmd+o")));
        // A function key stands alone.
        assert_eq!(verdict("copy_last_output", "f5"), Verdict::Free);
        assert_eq!(verdict("copy_last_output", "shift+f5"), Verdict::Free);
        // The command's own shortcut is not taken from anyone.
        assert_eq!(verdict("split_right", "cmd+d"), Verdict::Free);
    }

    #[test]
    fn another_commands_key_is_asked_about() {
        let found = verdict("copy_last_output", "shift+cmd+d");
        assert_eq!(name_of(&found), Some("split_down"));
        assert!(found.needs_confirmation() && !found.is_refused());
        assert_eq!(found.takes_from().map(|c| c.name), Some("split_down"));
        // The second shortcut of a command is its own: ⌃⇥ is Show Next Tab's.
        assert_eq!(
            name_of(&verdict("copy_last_output", "ctrl+tab")),
            Some("show_next_tab")
        );
        // A key of a family's item.
        assert_eq!(
            name_of(&verdict("copy_last_output", "cmd+3")),
            Some("select_tab")
        );
        assert_eq!(
            name_of(&verdict("copy_last_output", "option+cmd+left")),
            Some("select_split")
        );
    }

    #[test]
    fn a_key_that_edits_the_line_is_asked_about() {
        for text in [
            "cmd+left",
            "cmd+right",
            "cmd+backspace",
            "option+left",
            "option+right",
            "option+backspace",
        ] {
            let found = verdict("jump_to_command", text);
            assert!(
                matches!(found, Verdict::LineEditing(_)),
                "{text}: {found:?}"
            );
            assert!(found.needs_confirmation());
        }
        let Verdict::LineEditing(key) = verdict("jump_to_command", "cmd+left") else {
            panic!("a line key");
        };
        assert_eq!(key.title, "Move to Start of Line");
    }

    #[test]
    fn a_key_that_goes_to_the_program_is_asked_about() {
        for text in ["ctrl+r", "ctrl+c", "ctrl+shift+r", "ctrl+option+r"] {
            assert_eq!(
                verdict("jump_to_command", text),
                Verdict::ProgramKey,
                "{text}"
            );
        }
        // With ⌘ it is the shortcut's, and a function key never reaches the program as ⌃.
        assert_eq!(verdict("jump_to_command", "ctrl+cmd+r"), Verdict::Free);
        assert_eq!(verdict("jump_to_command", "ctrl+f5"), Verdict::Free);
    }

    #[test]
    fn an_option_key_that_types_is_asked_about() {
        for text in ["option+e", "option+1", "option+shift+e", "option+space"] {
            assert_eq!(
                verdict("jump_to_command", text),
                Verdict::TypesCharacter,
                "{text}"
            );
        }
        // Nothing is typed with an arrow, Tab or a function key under ⌥, or with ⌘ held.
        assert_eq!(verdict("jump_to_command", "option+tab"), Verdict::Free);
        assert_eq!(verdict("jump_to_command", "option+f5"), Verdict::Free);
        assert_eq!(verdict("jump_to_command", "option+cmd+e"), Verdict::Free);
    }

    #[test]
    fn a_key_macos_keeps_is_refused() {
        for (text, reason) in [
            ("cmd+space", "for Spotlight"),
            ("cmd+tab", "to switch apps"),
            ("shift+cmd+4", "for screenshots"),
            ("ctrl+left", "to move between spaces"),
            ("ctrl+cmd+f", "for Enter Full Screen in View"),
        ] {
            let found = verdict("jump_to_command", text);
            assert!(found.is_refused() && !found.needs_confirmation(), "{text}");
            let Verdict::MacOs(key) = found else {
                panic!("{text}: {found:?}");
            };
            assert_eq!(key.reason, reason);
        }
    }

    #[test]
    fn a_letter_without_a_modifier_is_refused() {
        for text in ["r", "shift+r", "1", "left", "tab", "return", "escape"] {
            assert_eq!(
                verdict("jump_to_command", text),
                Verdict::NeedsModifier,
                "{text}"
            );
        }
        // A macOS key is refused as macOS's, before it is asked what it lacks.
        assert!(matches!(
            verdict("jump_to_command", "cmd+space"),
            Verdict::MacOs(_)
        ));
    }

    #[test]
    fn a_family_wants_one_of_its_own_keys() {
        let tabs = verdict("select_tab", "option+a");
        assert_eq!(tabs, Verdict::FamilyNeedsKey(Family::Tabs));
        assert!(tabs.is_refused());
        // The tabs have no 0; the groups do.
        assert_eq!(
            verdict("select_tab", "option+0"),
            Verdict::FamilyNeedsKey(Family::Tabs)
        );
        assert_eq!(verdict("select_group", "option+0"), Verdict::TypesCharacter);
        assert_eq!(
            verdict("select_split", "option+cmd+1"),
            Verdict::FamilyNeedsKey(Family::Arrows)
        );
        // No modifier is no modifier, whatever the key.
        assert_eq!(verdict("select_tab", "1"), Verdict::NeedsModifier);
        assert_eq!(verdict("select_tab", "shift+1"), Verdict::NeedsModifier);
    }

    #[test]
    fn a_family_whose_item_is_someone_elses_is_refused() {
        // ⌃⌘1 is Select Group's.
        let found = verdict("select_tab", "ctrl+cmd+1");
        let Verdict::FamilyClash { chord: at, with } = found else {
            panic!("{found:?}");
        };
        assert_eq!(at, chord("ctrl+cmd+1"));
        assert!(matches!(with, Clash::Command(command) if command.name == "select_group"));
        assert!(found.is_refused());
        // Pressed on any digit, the first item that clashes is named.
        assert!(matches!(
            verdict("select_tab", "ctrl+cmd+5"),
            Verdict::FamilyClash { chord: at, .. } if at == chord("ctrl+cmd+1")
        ));
        // The system's: ⌃ with the arrows is Mission Control and the spaces.
        let found = verdict("resize_split", "ctrl+up");
        assert!(matches!(
            found,
            Verdict::FamilyClash { with: Clash::MacOs(key), .. } if key.reason == "for Mission Control" || key.reason == "to move between spaces"
        ));
        // The line's: ⌥ with the arrows moves a word.
        let found = verdict("select_split", "option+left");
        assert!(matches!(
            found,
            Verdict::FamilyClash { with: Clash::LineEditing(key), .. } if key.title == "Move One Word Left"
        ));
    }

    #[test]
    fn a_family_is_given_its_modifiers() {
        let resolved = resolve(&Settings::default());
        let ruling = resolved
            .judge("select_tab", &chord("option+cmd+1"))
            .expect("a family");
        assert_eq!(ruling.verdict, Verdict::Free);
        assert_eq!(
            ruling.stored,
            Shortcut::Mods(Mods::CMD.with(Modifier::Option))
        );
        // ⌥1…9 types characters; the cost is the key's, so it is asked about.
        let ruling = resolved
            .judge("select_tab", &chord("option+1"))
            .expect("a family");
        assert_eq!(ruling.verdict, Verdict::TypesCharacter);
        assert_eq!(
            ruling.stored,
            Shortcut::Mods(Mods::NONE.with(Modifier::Option))
        );
        // Its own current modifiers: nothing to take from anyone.
        assert_eq!(verdict("select_tab", "cmd+4"), Verdict::Free);
    }

    #[test]
    fn a_name_that_is_no_command_has_no_verdict() {
        assert!(
            resolve(&Settings::default())
                .judge("nothing", &chord("cmd+d"))
                .is_none()
        );
    }

    #[test]
    fn what_the_table_ships_with_is_never_refused() {
        // Every default is a key the recorder would accept (some after a question: ⌃⇥ goes to
        // the program) — a refusal here would mean a shortcut the menu cannot keep.
        let resolved = resolve(&Settings::default());
        for assignment in resolved.assignments() {
            assert_eq!(
                refusal(&assignment.chord),
                None,
                "{}",
                assignment.command.name
            );
        }
        for key in MACOS_KEYS {
            assert!(
                resolved.assignments().iter().all(|a| a.chord != key.chord),
                "{}",
                key.reason
            );
        }
    }
}
