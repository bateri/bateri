//! What recording a key says and writes: the words of the note under a row, the verb of its
//! button, and the edits to the file that accepting the key comes to.
//!
//! The decision is [`Resolved::judge`](super::Resolved::judge) (the nine outcomes); this is the
//! page's side of it. A key that is refused has a note and no button (the field keeps
//! listening); a key with a cost has a note and a verb (Use Here, Assign Anyway); a free key has
//! neither and is written at once.
//!
//! Writing is one edit for the command and, when the key was another command's, a second one
//! that takes it from that command (Use Here writes both, in this order). A command whose
//! shortcuts come to its own defaults has its line removed, not written out: a default that
//! later changes would otherwise stay as the file wrote it.

use bt_core::{Chord, Key, Modifier, Mods, NamedKey, SettingsEdit, Shortcut};

use super::mapping::glyphs;
use super::page::{Page, family_glyphs};
use super::resolve::Resolved;
use super::table::{Command, Family, by_name};
use super::tap::Tapped;
use super::verdict::{Clash, Ruling, Verdict};

/// The button that says "yes, I know what it costs".
pub const USE_HERE: &str = "Use Here";
pub const ASSIGN_ANYWAY: &str = "Assign Anyway";

/// What a pressed key comes to.
#[derive(Clone, Debug, PartialEq)]
pub struct Outcome {
    pub verdict: Verdict,
    /// What to tell the user; none for a free key.
    pub note: Option<String>,
    /// The verb of the button that accepts the key anyway; none for a refused key and for a free
    /// one.
    pub button: Option<&'static str>,
    /// What accepting writes, in order. Empty for a refused key.
    pub edits: Vec<SettingsEdit>,
}

impl Outcome {
    /// The key is written without asking.
    pub fn is_accepted(&self) -> bool {
        self.verdict == Verdict::Free
    }

    /// The key cannot be given; the field keeps listening.
    pub fn is_refused(&self) -> bool {
        self.verdict.is_refused()
    }
}

/// The shortcuts a command would have with `stored` in its `slot` (the first, the second; past
/// the last is a new one): the others stay, the same key twice is once.
fn replaced(current: &[Shortcut], slot: usize, stored: Shortcut) -> Vec<Shortcut> {
    let mut out: Vec<Shortcut> = current.to_vec();
    if slot < out.len() {
        out[slot] = stored;
    } else {
        out.push(stored);
    }
    let mut kept: Vec<Shortcut> = Vec::new();
    for shortcut in out {
        if !kept.contains(&shortcut) {
            kept.push(shortcut);
        }
    }
    kept.truncate(bt_core::MAX_SHORTCUTS);
    kept
}

/// The edit that leaves `command` with `shortcuts`: the line is removed when they are its own.
pub fn edit_of(command: &Command, shortcuts: Vec<Shortcut>) -> SettingsEdit {
    let own = command.default_shortcuts();
    SettingsEdit::Shortcut {
        name: command.name.to_owned(),
        shortcuts: (shortcuts != own).then_some(shortcuts),
    }
}

/// The edit that puts `command` back to its own shortcuts (Reset).
pub fn reset_edit(command: &Command) -> SettingsEdit {
    SettingsEdit::Shortcut {
        name: command.name.to_owned(),
        shortcuts: None,
    }
}

/// The edit that clears the file of every shortcut it gives (Restore Defaults…).
pub fn restore_edit() -> SettingsEdit {
    SettingsEdit::DefaultShortcuts {
        names: super::page::command_names(),
    }
}

/// The edit that takes the shortcut in `slot` away from `command` (⌫, Remove Shortcut): the
/// other one stays, and a command left with none has `""`.
pub fn remove_edit(resolved: &Resolved, command: &Command, slot: usize) -> Option<SettingsEdit> {
    let mut left = resolved.binding(command.name)?.shortcuts.clone();
    if slot >= left.len() {
        return None;
    }
    left.remove(slot);
    Some(edit_of(command, left))
}

/// What `pressed` comes to as the shortcut in `slot` of the command `name` (a name from the
/// table); `None` for a name that is not one.
pub fn record(
    page: &Page,
    resolved: &Resolved,
    name: &str,
    slot: usize,
    pressed: &Chord,
) -> Option<Outcome> {
    let command = by_name(name)?;
    let ruling = resolved.judge(name, pressed)?;
    let key = shown(pressed);
    let (note, button) = words(page, command, &ruling.verdict, pressed, &key);
    // What a key takes from its owner: the owner's shortcuts that bind the chord.
    let held = |owner: &Command, shortcut: &Shortcut| {
        owner
            .items(shortcut)
            .iter()
            .any(|item| item.chord == *pressed)
    };
    Some(outcome(
        resolved,
        command,
        slot,
        ruling,
        (note, button),
        &held,
    ))
}

/// What a double press of `tapped` comes to as the shortcut in `slot` of the command `name`;
/// `None` for a name that is not one. fn is refused without being judged.
pub fn record_twice(
    page: &Page,
    resolved: &Resolved,
    name: &str,
    slot: usize,
    tapped: Tapped,
) -> Option<Outcome> {
    let command = by_name(name)?;
    let Tapped::Modifier(modifier) = tapped else {
        return Some(Outcome {
            verdict: Verdict::FunctionTwice,
            note: Some(
                "fn fn is macOS’s: it starts Dictation or the emoji picker, so bateri never gets it."
                    .to_owned(),
            ),
            button: None,
            edits: Vec::new(),
        });
    };
    let ruling = resolved.judge_twice(name, modifier)?;
    let (note, button) = words_twice(page, command, &ruling.verdict, modifier);
    let held = |_: &Command, shortcut: &Shortcut| *shortcut == Shortcut::Twice(modifier);
    Some(outcome(
        resolved,
        command,
        slot,
        ruling,
        (note, button),
        &held,
    ))
}

/// The outcome of a ruling: its words and, for a key that can be given, what accepting writes —
/// the command's line, and when the key was another command's, that command's line without it
/// (`held` says which of its shortcuts are the key).
fn outcome(
    resolved: &Resolved,
    command: &Command,
    slot: usize,
    ruling: Ruling,
    (note, button): (Option<String>, Option<&'static str>),
    held: &dyn Fn(&Command, &Shortcut) -> bool,
) -> Outcome {
    let verdict = ruling.verdict;
    let mut edits = Vec::new();
    if !verdict.is_refused() {
        let current = resolved
            .binding(command.name)
            .map(|binding| binding.shortcuts.clone())
            .unwrap_or_default();
        edits.push(edit_of(command, replaced(&current, slot, ruling.stored)));
        if let Some(owner) = verdict.takes_from() {
            let left: Vec<Shortcut> = resolved
                .binding(owner.name)
                .map(|binding| binding.shortcuts.clone())
                .unwrap_or_default()
                .into_iter()
                .filter(|shortcut| !held(owner, shortcut))
                .collect();
            edits.push(edit_of(owner, left));
        }
    }
    Outcome {
        verdict,
        note,
        button,
        edits,
    }
}

/// How a pressed key is shown: its glyphs, or the bare key for one that is typing.
fn shown(chord: &Chord) -> String {
    glyphs(&Shortcut::Chord(*chord)).unwrap_or_else(|| match chord.key {
        Key::Char(c) => c.to_uppercase().collect(),
        Key::Named(named) => named.to_string(),
    })
}

/// What the terminal's program does with a ⌃ key, in a shell: the cost of taking it.
fn control_effect(key: Key) -> Option<&'static str> {
    let Key::Char(c) = key else {
        return None;
    };
    Some(match c {
        'a' => "moves to the start of the line",
        'b' => "moves back a character",
        'c' => "interrupts the program",
        'd' => "ends the input",
        'e' => "moves to the end of the line",
        'f' => "moves forward a character",
        'k' => "deletes to the end of the line",
        'l' => "clears the screen",
        'n' => "recalls the next command",
        'p' => "recalls the previous command",
        'r' => "searches the history",
        'u' => "deletes the line",
        'w' => "deletes a word",
        'z' => "suspends the program",
        _ => return None,
    })
}

fn article(word: &str) -> &'static str {
    if word.starts_with(['a', 'e', 'i', 'o', 'u']) {
        "an"
    } else {
        "a"
    }
}

/// The first key a family's example shows: `⌥1` for the digits, `⌥←` for the arrows.
fn example(family: Family, mods: Mods) -> String {
    let key = match family {
        Family::Tabs | Family::Groups => Key::Char('1'),
        Family::Arrows => Key::Named(NamedKey::Left),
    };
    glyphs(&Shortcut::Chord(Chord::new(mods, key))).unwrap_or_default()
}

/// The note of a key that is `owner`'s: whose it is and what is left to it. `binds` says which
/// of the owner's shortcuts are the key.
fn taken_by_note(
    page: &Page,
    owner: &Command,
    key: &str,
    binds: &dyn Fn(&Shortcut) -> bool,
) -> String {
    let owner_title = page.title_of(owner.name);
    let others = page
        .row(owner.name)
        .map(|row| {
            row.chips
                .iter()
                .filter(|chip| !binds(&chip.shortcut))
                .map(|chip| chip.text.clone())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    match others.first() {
        Some(left) => format!("{key} is {owner_title}’s shortcut. {owner_title} will keep {left}."),
        None => format!("{key} is {owner_title}’s shortcut. {owner_title} will have none."),
    }
}

/// The words of a double press: the note under the row and the button's verb.
fn words_twice(
    page: &Page,
    command: &Command,
    verdict: &Verdict,
    modifier: Modifier,
) -> (Option<String>, Option<&'static str>) {
    let key = glyphs(&Shortcut::Twice(modifier)).unwrap_or_default();
    match verdict {
        Verdict::TakenBy(owner) => (
            Some(taken_by_note(page, owner, &key, &|shortcut| {
                *shortcut == Shortcut::Twice(modifier)
            })),
            Some(USE_HERE),
        ),
        Verdict::Dictation => (
            Some(format!(
                "{key} starts Dictation if macOS is set to it (System Settings ▸ Keyboard). \
                 As a shortcut, both would run."
            )),
            Some(ASSIGN_ANYWAY),
        ),
        Verdict::FamilyTwice(family) => {
            let word = family.presses();
            (
                Some(format!(
                    "A double press is one shortcut — {} needs {} {word} for each of its items.",
                    page.title_of(command.name),
                    article(word)
                )),
                None,
            )
        }
        // The rest is a chord's.
        _ => (None, None),
    }
}

fn words(
    page: &Page,
    command: &Command,
    verdict: &Verdict,
    pressed: &Chord,
    key: &str,
) -> (Option<String>, Option<&'static str>) {
    let title = |name: &str| page.title_of(name);
    match verdict {
        Verdict::Free => (None, None),
        Verdict::TakenBy(owner) => {
            let owner_title = title(owner.name);
            let ending = match owner.family {
                // A family cannot give up one key: its modifiers go, and all its items with them.
                Some(family) => format!(
                    "{key} is one of {owner_title}’s shortcuts ({}). {owner_title} will have none.",
                    family_glyphs(pressed.mods, family)
                ),
                None => taken_by_note(page, owner, key, &|shortcut| {
                    owner.items(shortcut).iter().any(|i| i.chord == *pressed)
                }),
            };
            (Some(ending), Some(USE_HERE))
        }
        Verdict::LineEditing(line) => (
            Some(format!(
                "{key} {} in the terminal. As a shortcut it no longer does.",
                line.effect
            )),
            Some(USE_HERE),
        ),
        Verdict::ProgramKey => {
            let cost = match control_effect(pressed.key) {
                Some(effect) => format!("In a shell {key} {effect}."),
                None => format!("{key} goes to the program in the terminal."),
            };
            (
                Some(format!(
                    "{cost} As a shortcut the program no longer gets it."
                )),
                Some(ASSIGN_ANYWAY),
            )
        }
        Verdict::TypesCharacter => (
            Some(format!(
                "{key} types a character on most keyboards. As a shortcut you can no longer type it."
            )),
            Some(ASSIGN_ANYWAY),
        ),
        Verdict::MacOs(mac) => (
            Some(format!(
                "macOS keeps {key} {}, so bateri never gets it.",
                mac.reason
            )),
            None,
        ),
        Verdict::NeedsModifier => {
            let alone = if pressed.mods.shift {
                format!("{key} is a capital letter")
            } else {
                format!("{key} alone is typing")
            };
            (
                Some(format!("{alone}. A shortcut needs ⌘, ⌃ or ⌥ with it.")),
                None,
            )
        }
        Verdict::FamilyNeedsKey(family) => {
            let shows = example(*family, pressed.mods);
            let range = family_glyphs(pressed.mods, *family);
            let word = family.presses();
            (
                Some(format!(
                    "Press it with {} {word} — {shows} sets {range}.",
                    article(word)
                )),
                None,
            )
        }
        // A double press is worded apart ([`words_twice`]).
        Verdict::Dictation | Verdict::FamilyTwice(_) | Verdict::FunctionTwice => (None, None),
        Verdict::FamilyShift(_) => (
            Some(format!(
                "⇧ makes a digit type another character (⇧1 types !), and the menu cannot tell it \
                 from the digit, so {} takes no ⇧. Try other modifiers.",
                title(command.name)
            )),
            None,
        ),
        Verdict::FamilyClash { chord, with } => {
            let item = glyphs(&Shortcut::Chord(*chord)).unwrap_or_default();
            let reason = match with {
                Clash::Command(other) => format!("{}’s shortcut", title(other.name)),
                Clash::MacOs(mac) => format!("kept by macOS {}", mac.reason),
                Clash::LineEditing(line) => format!("the key that {}", line.effect),
            };
            (
                Some(format!(
                    "{} would take {item}, which is {reason}. Try other modifiers.",
                    title(command.name)
                )),
                None,
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use bt_core::Settings;

    use super::super::page::Page;
    use super::super::resolve::resolve;
    use super::*;
    use crate::palette::MenuNode;

    fn chord(text: &str) -> Chord {
        match Shortcut::parse(text) {
            Ok(Shortcut::Chord(chord)) => chord,
            other => panic!("`{text}` is not a chord: {other:?}"),
        }
    }

    fn menu() -> Vec<MenuNode> {
        let digits = (1..=9)
            .map(|n| MenuNode::item(100 + n, &format!("Tab {n}"), "selectTab:").tag(n as isize))
            .collect();
        let groups = (0..=9)
            .map(|n| MenuNode::item(200 + n, &format!("Group {n}"), "selectGroup:").tag(n as isize))
            .collect();
        vec![
            MenuNode::holder(
                0,
                "Shell",
                vec![
                    MenuNode::item(1, "Split Right", "splitRight:"),
                    MenuNode::item(2, "Split Down", "splitDown:"),
                    MenuNode::item(3, "Close Tab", "closeTab:"),
                ],
            ),
            MenuNode::holder(
                10,
                "Edit",
                vec![MenuNode::item(11, "Copy Last Output", "copyLastOutput:")],
            ),
            MenuNode::holder(
                20,
                "Window",
                vec![
                    MenuNode::item(21, "Show Next Tab", "showNextTab:"),
                    MenuNode::holder(22, "Select Tab", digits),
                    MenuNode::holder(23, "Select Group", groups),
                ],
            ),
        ]
    }

    fn outcome(keys: &str, name: &str, slot: usize, pressed: &str) -> Outcome {
        let text = format!("[keys]\n{keys}");
        let settings = Settings::parse(&text).expect("TOML").settings;
        let resolved = resolve(&settings);
        let page = Page::new(&menu(), &resolved);
        record(&page, &resolved, name, slot, &chord(pressed)).expect("a command")
    }

    fn edit(name: &str, keys: Option<&[&str]>) -> SettingsEdit {
        SettingsEdit::Shortcut {
            name: name.to_owned(),
            shortcuts: keys.map(|keys| {
                keys.iter()
                    .map(|key| Shortcut::parse(key).expect("a shortcut"))
                    .collect()
            }),
        }
    }

    // The nine outcomes of a pressed key, each with its words.

    #[test]
    fn a_free_key_is_written_at_once_with_no_note() {
        let done = outcome("", "copy_last_output", 0, "shift+cmd+o");
        assert!(done.is_accepted());
        assert_eq!((done.note.as_deref(), done.button), (None, None));
        assert_eq!(
            done.edits,
            [edit("copy_last_output", Some(&["shift+cmd+o"]))]
        );
    }

    #[test]
    fn the_commands_own_key_removes_its_line_instead_of_writing_it_out() {
        // Split Right was emptied; ⌘D is its own again.
        let done = outcome("split_right = \"\"\n", "split_right", 0, "cmd+d");
        assert!(done.is_accepted());
        assert_eq!(done.edits, [edit("split_right", None)]);
    }

    #[test]
    fn another_commands_key_asks_and_use_here_writes_both_lines() {
        let done = outcome("", "copy_last_output", 0, "shift+cmd+d");
        assert_eq!(
            done.note.as_deref(),
            Some("⇧⌘D is Split Down’s shortcut. Split Down will have none.")
        );
        assert_eq!(done.button, Some(USE_HERE));
        assert_eq!(
            done.edits,
            [
                edit("copy_last_output", Some(&["shift+cmd+d"])),
                edit("split_down", Some(&[])),
            ]
        );
    }

    #[test]
    fn taking_one_of_two_keys_leaves_the_other() {
        let done = outcome("", "copy_last_output", 0, "ctrl+tab");
        assert_eq!(
            done.note.as_deref(),
            Some("⌃⇥ is Show Next Tab’s shortcut. Show Next Tab will keep ⇧⌘].")
        );
        assert_eq!(
            done.edits,
            [
                edit("copy_last_output", Some(&["ctrl+tab"])),
                edit("show_next_tab", Some(&["shift+cmd+]"])),
            ]
        );
    }

    #[test]
    fn taking_a_family_item_takes_the_whole_family() {
        let done = outcome("", "copy_last_output", 0, "cmd+3");
        assert_eq!(
            done.note.as_deref(),
            Some("⌘3 is one of Select Tab’s shortcuts (⌘1…9). Select Tab will have none.")
        );
        assert_eq!(done.edits[1], edit("select_tab", Some(&[])));
    }

    #[test]
    fn a_line_editing_key_asks_and_writes_only_the_command() {
        let done = outcome("", "copy_last_output", 0, "cmd+backspace");
        assert_eq!(done.button, Some(USE_HERE));
        assert!(done.note.as_deref().is_some_and(|note| {
            note.starts_with("⌘⌫ deletes the line you type") && note.ends_with("no longer does.")
        }));
        assert_eq!(done.edits.len(), 1);
    }

    #[test]
    fn a_control_key_says_what_the_shell_would_lose() {
        let done = outcome("", "copy_last_output", 0, "ctrl+r");
        assert_eq!(
            done.note.as_deref(),
            Some(
                "In a shell ⌃R searches the history. As a shortcut the program no longer gets it."
            )
        );
        assert_eq!(done.button, Some(ASSIGN_ANYWAY));
        assert_eq!(done.edits.len(), 1);
        // A control key with no word of its own still says whose it is.
        let other = outcome("", "copy_last_output", 0, "ctrl+j");
        assert_eq!(
            other.note.as_deref(),
            Some(
                "⌃J goes to the program in the terminal. As a shortcut the program no longer gets it."
            )
        );
    }

    #[test]
    fn an_option_letter_says_it_would_no_longer_type() {
        let done = outcome("", "copy_last_output", 0, "option+e");
        assert_eq!(
            done.note.as_deref(),
            Some(
                "⌥E types a character on most keyboards. As a shortcut you can no longer type it."
            )
        );
        assert_eq!(done.button, Some(ASSIGN_ANYWAY));
    }

    #[test]
    fn macos_keys_and_plain_letters_are_refused_with_no_button() {
        let mac = outcome("", "copy_last_output", 0, "cmd+space");
        assert!(mac.is_refused() && mac.button.is_none() && mac.edits.is_empty());
        assert!(
            mac.note
                .as_deref()
                .is_some_and(|note| note.starts_with("macOS keeps ⌘Space for ")),
            "{:?}",
            mac.note
        );
        let typing = outcome("", "copy_last_output", 0, "d");
        assert!(typing.is_refused() && typing.edits.is_empty());
        assert_eq!(
            typing.note.as_deref(),
            Some("D alone is typing. A shortcut needs ⌘, ⌃ or ⌥ with it.")
        );
        let capital = outcome("", "copy_last_output", 0, "shift+d");
        assert_eq!(
            capital.note.as_deref(),
            Some("⇧D is a capital letter. A shortcut needs ⌘, ⌃ or ⌥ with it.")
        );
    }

    #[test]
    fn a_family_wants_a_key_of_its_own() {
        let done = outcome("", "select_tab", 0, "option+d");
        assert!(done.is_refused());
        assert_eq!(
            done.note.as_deref(),
            Some("Press it with a digit — ⌥1 sets ⌥1…9.")
        );
        // A digit with the modifiers writes the modifiers; ⌥ alone would type the digit's
        // character, so it asks.
        let typing = outcome("", "select_tab", 0, "option+1");
        assert_eq!(typing.verdict, Verdict::TypesCharacter);
        assert_eq!(typing.button, Some(ASSIGN_ANYWAY));
        assert_eq!(typing.edits, [edit("select_tab", Some(&["option"]))]);
        let ok = outcome("", "select_tab", 0, "option+cmd+1");
        assert!(ok.is_accepted(), "{:?}", ok.note);
        assert_eq!(ok.edits, [edit("select_tab", Some(&["option+cmd"]))]);
    }

    #[test]
    fn shift_on_the_digits_says_why_and_is_refused() {
        let done = outcome("", "select_tab", 0, "shift+cmd+!");
        assert!(done.is_refused() && done.button.is_none() && done.edits.is_empty());
        assert_eq!(
            done.note.as_deref(),
            Some(
                "⇧ makes a digit type another character (⇧1 types !), and the menu cannot tell it from the digit, so Select Tab takes no ⇧. Try other modifiers."
            )
        );
    }

    #[test]
    fn a_family_whose_item_is_someone_elses_is_refused() {
        // ⌘3 is Select Tab's, but Select Tab's own modifiers on Select Group would take them.
        let done = outcome("", "select_group", 0, "cmd+1");
        assert!(done.is_refused() && done.edits.is_empty());
        let note = done.note.unwrap_or_default();
        assert!(
            note.starts_with("Select Group would take")
                && note.contains("’s shortcut")
                && note.ends_with("Try other modifiers."),
            "{note}"
        );
    }

    #[test]
    fn an_added_shortcut_is_the_second_and_a_replaced_one_keeps_the_other() {
        let added = outcome("", "copy_last_output", 1, "shift+cmd+o");
        assert_eq!(
            added.edits,
            [edit("copy_last_output", Some(&["shift+cmd+o"]))]
        );
        let second = outcome(
            "copy_last_output = \"cmd+j\"\n",
            "copy_last_output",
            1,
            "shift+cmd+o",
        );
        assert_eq!(
            second.edits,
            [edit("copy_last_output", Some(&["cmd+j", "shift+cmd+o"]))]
        );
        let replaced = outcome("", "show_next_tab", 1, "ctrl+j");
        assert_eq!(
            replaced.edits,
            [edit("show_next_tab", Some(&["shift+cmd+]", "ctrl+j"]))]
        );
        // The same key in the other slot is once.
        let same = outcome("", "show_next_tab", 1, "shift+cmd+]");
        assert_eq!(same.edits, [edit("show_next_tab", Some(&["shift+cmd+]"]))]);
    }

    #[test]
    fn remove_reset_and_restore_are_edits_of_the_same_kind() {
        let settings = Settings::default();
        let resolved = resolve(&settings);
        let next = by_name("show_next_tab").expect("a command");
        assert_eq!(
            remove_edit(&resolved, next, 1),
            Some(edit("show_next_tab", Some(&["shift+cmd+]"])))
        );
        let split = by_name("split_right").expect("a command");
        assert_eq!(
            remove_edit(&resolved, split, 0),
            Some(edit("split_right", Some(&[])))
        );
        assert_eq!(remove_edit(&resolved, split, 1), None);
        // A command with none by default and none left has no line.
        let last = by_name("copy_last_output").expect("a command");
        assert_eq!(reset_edit(last), edit("copy_last_output", None));
        assert!(matches!(
            restore_edit(),
            SettingsEdit::DefaultShortcuts { names } if names.len() == super::super::table::COMMANDS.len()
        ));
    }

    fn twice_outcome(keys: &str, name: &str, slot: usize, tapped: Tapped) -> Outcome {
        let text = format!("[keys]\n{keys}");
        let settings = Settings::parse(&text).expect("TOML").settings;
        let resolved = resolve(&settings);
        let page = Page::new(&menu(), &resolved);
        record_twice(&page, &resolved, name, slot, tapped).expect("a command")
    }

    fn shift_twice() -> Tapped {
        Tapped::Modifier(Modifier::Shift)
    }

    // A double press.

    #[test]
    fn a_free_double_press_is_written_at_once() {
        let done = twice_outcome("", "copy_last_output", 0, shift_twice());
        assert!(done.is_accepted());
        assert_eq!(
            done.edits,
            [edit("copy_last_output", Some(&["shift+shift"]))]
        );
        // As the second shortcut it keeps the first.
        let second = twice_outcome("", "show_next_tab", 1, Tapped::Modifier(Modifier::Option));
        assert_eq!(
            second.edits,
            [edit(
                "show_next_tab",
                Some(&["shift+cmd+]", "option+option"])
            )]
        );
    }

    #[test]
    fn control_and_command_twice_ask_about_dictation() {
        let done = twice_outcome("", "copy_last_output", 0, Tapped::Modifier(Modifier::Ctrl));
        assert_eq!(done.button, Some(ASSIGN_ANYWAY));
        assert!(
            done.note
                .as_deref()
                .is_some_and(|note| note.starts_with("⌃ ⌃ starts Dictation")),
            "{:?}",
            done.note
        );
        assert_eq!(done.edits, [edit("copy_last_output", Some(&["ctrl+ctrl"]))]);
    }

    #[test]
    fn another_commands_double_press_is_taken_with_use_here() {
        let done = twice_outcome(
            "split_down = \"shift+shift\"\n",
            "copy_last_output",
            0,
            shift_twice(),
        );
        assert_eq!(
            done.note.as_deref(),
            Some("⇧ ⇧ is Split Down’s shortcut. Split Down will have none.")
        );
        assert_eq!(done.button, Some(USE_HERE));
        assert_eq!(
            done.edits,
            [
                edit("copy_last_output", Some(&["shift+shift"])),
                edit("split_down", Some(&[])),
            ]
        );
        // Of two shortcuts, the other stays.
        let two = twice_outcome(
            "split_down = [\"cmd+j\", \"shift+shift\"]\n",
            "copy_last_output",
            0,
            shift_twice(),
        );
        assert_eq!(
            two.note.as_deref(),
            Some("⇧ ⇧ is Split Down’s shortcut. Split Down will keep ⌘J.")
        );
        assert_eq!(two.edits[1], edit("split_down", Some(&["cmd+j"])));
    }

    #[test]
    fn fn_twice_and_a_family_are_refused_with_no_button() {
        let function = twice_outcome("", "copy_last_output", 0, Tapped::Function);
        assert!(function.is_refused() && function.button.is_none() && function.edits.is_empty());
        assert!(
            function
                .note
                .as_deref()
                .is_some_and(|note| note.starts_with("fn fn is macOS’s")),
            "{:?}",
            function.note
        );
        let family = twice_outcome("", "select_tab", 0, shift_twice());
        assert!(family.is_refused() && family.edits.is_empty());
        assert_eq!(
            family.note.as_deref(),
            Some(
                "A double press is one shortcut — Select Tab needs a digit for each of its items."
            )
        );
    }
}
