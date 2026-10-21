//! The shortcuts in effect: the table's defaults, then what `[keys]` says.
//!
//! [`resolve`] reads the settings once and answers, for every command, which shortcuts it has —
//! and, as data, what is wrong with the file: a name no command has, a shortcut that cannot
//! mean what it says, and the keys that two commands share. The menu, the palette, the tab
//! strip's hints and the settings page ask this one answer.
//!
//! A shortcut that reads but that its command cannot take is judged once, by [`check`], in two
//! places that differ in what the command has then. [`admit`] is the read of a file
//! ([`crate::settings::load_keeping`]): the line goes, and the command keeps the shortcut it
//! was running with — the one of the settings the read replaces, the table's own when it had
//! none — exactly as `bt-core` treats a line it cannot read. [`resolve`] has no such memory,
//! so for settings that did not come through that read it can only give the command its own.

use std::collections::HashMap;

use bt_core::{Chord, Diagnostic, KeyEntry, Modifier, Mods, Settings, Shortcut};

use super::table::{COMMANDS, Command, by_name};
use super::verdict::{MACOS_KEYS, Refusal, refusal};
use crate::keys::{LINE_EDITING, LineKey};

/// Where a command's shortcuts come from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// The table's own.
    Default,
    /// A line of `[keys]` (the 1-based line number, when the parser gave one).
    File(Option<usize>),
}

/// One command and the shortcuts it has.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Binding {
    pub command: &'static Command,
    /// None, one or two. For a family, the modifiers.
    pub shortcuts: Vec<Shortcut>,
    pub source: Source,
}

impl Binding {
    /// Whether the shortcuts differ from the command's own. A line of the file that says what
    /// the default says is no change.
    pub fn changed(&self) -> bool {
        self.shortcuts != self.command.default_shortcuts()
    }
}

/// A shortcut two or more commands have.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Conflict {
    /// The shared key. For a family, the one item's chord that is shared.
    pub shortcut: Shortcut,
    /// In the order of the menu: the first is the one that runs.
    pub commands: Vec<&'static Command>,
}

/// A set of commands that share keys, with the first key they share and how many. A family that
/// shares nine keys with another command is one set, not nine.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SharedKeys {
    /// In the order of the menu: the first is the one that runs.
    pub commands: Vec<&'static Command>,
    /// The first key they share.
    pub shortcut: Shortcut,
    pub count: usize,
}

/// One menu item and the key it is given.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Assignment {
    pub command: &'static Command,
    /// The menu item's id (`selectTab:3`).
    pub id: String,
    /// 0 for the command's first shortcut, 1 for its second.
    pub slot: usize,
    pub chord: Chord,
}

/// The settings read against the table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resolved {
    bindings: Vec<Binding>,
    /// What is wrong with the file, to be reported like any other setting's problem.
    pub diagnostics: Vec<Diagnostic>,
    /// The keys two commands share, in the order they are first met.
    pub conflicts: Vec<Conflict>,
}

/// What a family takes: only modifiers, and none of the family's items on a key that cannot
/// be.
fn check(command: &Command, shortcut: &Shortcut) -> Result<(), String> {
    match (shortcut, command.family) {
        (Shortcut::Chord(chord), None) => match refusal(chord) {
            Some(Refusal::MacOs(key)) => Err(format!(
                "`{chord}` is macOS's {}, bateri never gets it",
                key.reason
            )),
            Some(Refusal::NeedsModifier) => Err(format!(
                "`{chord}` alone is typing; a shortcut needs cmd, ctrl or option"
            )),
            None => Ok(()),
        },
        (Shortcut::Chord(chord), Some(_)) => Err(format!(
            "`{chord}` is a key and {} is a family: it takes only modifiers, as in `{0} = \"option\"`",
            command.name
        )),
        (Shortcut::Mods(mods), None) => Err(format!(
            "`{mods}` is only modifiers, and {} takes a key (only a family such as select_tab takes only modifiers)",
            command.name
        )),
        (Shortcut::Mods(mods), Some(family)) => {
            if !(mods.cmd || mods.ctrl || mods.option) {
                return Err(format!(
                    "`{mods}` alone is typing; the modifiers of {} need cmd, ctrl or option",
                    command.name
                ));
            }
            if mods.shift && family.is_digits() {
                return Err(format!(
                    "`{mods}` has shift, which makes a digit type another character (shift+1 is `!`) \
                     that the menu cannot tell from the digit; {} takes other modifiers",
                    command.name
                ));
            }
            for item in command.items(shortcut) {
                if let Some(key) = MACOS_KEYS.iter().find(|key| key.chord == item.chord) {
                    return Err(format!(
                        "`{mods}` would give {} the key `{}` that macOS uses {}",
                        command.name, item.chord, key.reason
                    ));
                }
            }
            Ok(())
        }
        (Shortcut::Twice(_), None) => Ok(()),
        (Shortcut::Twice(_), Some(_)) => Err(format!(
            "a double press is one shortcut and {} needs a key for each of its items",
            command.name
        )),
    }
}

/// Why `entry`'s line cannot be what its command (`command`, found by the entry's name) has:
/// one of its shortcuts is one the command cannot take. The text of both places that judge it.
fn refused(command: &Command, entry: &KeyEntry) -> Option<Diagnostic> {
    let reason = entry
        .shortcuts
        .iter()
        .find_map(|shortcut| check(command, shortcut).err())?;
    Some(Diagnostic {
        key: Some("keys"),
        line: entry.line,
        message: format!("`keys.{}`: {reason}; the line is ignored", entry.name),
    })
}

/// Takes the lines of `settings.keys` that their commands cannot have out of it, and gives
/// each of those commands what it had: the entry of the same name in `previous` (the settings
/// this read replaces), at the place the name stands now — the shortcuts stay, the position is
/// the line's, as `bt-core` keeps a line it cannot read — or, with no such entry, nothing, which
/// is the table's own. Returns what was refused, to be reported with the file's other problems.
///
/// The file of the last read is what `previous` holds, never a preview: a save with a typo must
/// not give a command another key than the one it was running with, which may be the very key
/// the user has since moved to a different command. A name no command has stays (it is
/// [`resolve`]'s to report) and so does a line the commands can take.
pub fn admit(settings: &mut Settings, previous: &Settings) -> Vec<Diagnostic> {
    let mut refusals = Vec::new();
    let mut entries = Vec::with_capacity(settings.keys.len());
    for entry in std::mem::take(&mut settings.keys) {
        let Some(command) = by_name(&entry.name) else {
            entries.push(entry);
            continue;
        };
        let Some(refusal) = refused(command, &entry) else {
            entries.push(entry);
            continue;
        };
        refusals.push(refusal);
        // What it had must itself be something a command can have (the settings that were
        // handed in may not have come through this read).
        if let Some(held) = previous
            .keys
            .iter()
            .find(|held| held.name == entry.name && refused(command, held).is_none())
        {
            entries.push(KeyEntry {
                shortcuts: held.shortcuts.clone(),
                ..entry
            });
        }
    }
    settings.keys = entries;
    refusals
}

/// Reads the settings against the table. A line its command cannot take gives the command its
/// own shortcut here: what it had before is the read's to keep ([`admit`]).
pub fn resolve(settings: &Settings) -> Resolved {
    let mut bindings: Vec<Binding> = COMMANDS
        .iter()
        .map(|command| Binding {
            command,
            shortcuts: command.default_shortcuts(),
            source: Source::Default,
        })
        .collect();
    let mut diagnostics = Vec::new();
    for entry in &settings.keys {
        let Some(command) = by_name(&entry.name) else {
            diagnostics.push(Diagnostic {
                key: Some("keys"),
                line: entry.line,
                message: format!(
                    "`keys.{}` is not the name of a command; the line is ignored",
                    entry.name
                ),
            });
            continue;
        };
        if let Some(refusal) = refused(command, entry) {
            diagnostics.push(refusal);
            continue;
        }
        if let Some(binding) = bindings
            .iter_mut()
            .find(|binding| binding.command.name == command.name)
        {
            binding.shortcuts.clone_from(&entry.shortcuts);
            binding.source = Source::File(entry.line);
        }
    }
    let conflicts = conflicts(&bindings);
    Resolved {
        bindings,
        diagnostics,
        conflicts,
    }
}

/// The keys more than one command has.
fn conflicts(bindings: &[Binding]) -> Vec<Conflict> {
    let mut order: Vec<Shortcut> = Vec::new();
    let mut holders: HashMap<Shortcut, Vec<&'static Command>> = HashMap::new();
    for binding in bindings {
        for shortcut in &binding.shortcuts {
            let keys: Vec<Shortcut> = match shortcut {
                Shortcut::Twice(_) => vec![*shortcut],
                _ => binding
                    .command
                    .items(shortcut)
                    .into_iter()
                    .map(|item| Shortcut::Chord(item.chord))
                    .collect(),
            };
            for key in keys {
                let held = holders.entry(key).or_insert_with(|| {
                    order.push(key);
                    Vec::new()
                });
                if !held.contains(&binding.command) {
                    held.push(binding.command);
                }
            }
        }
    }
    order
        .into_iter()
        .filter_map(|shortcut| {
            let commands = holders.remove(&shortcut)?;
            (commands.len() > 1).then_some(Conflict { shortcut, commands })
        })
        .collect()
}

impl Resolved {
    /// The keys two or more commands have, grouped by the commands that share them: one set for
    /// each distinct list of commands, in the order they are first met. The subtitle's warning
    /// ([`Resolved::problems`]) and the note under a row are worded from the same sets.
    pub fn shared_sets(&self) -> Vec<SharedKeys> {
        let mut sets: Vec<SharedKeys> = Vec::new();
        for conflict in &self.conflicts {
            match sets
                .iter_mut()
                .find(|set| set.commands == conflict.commands)
            {
                Some(set) => set.count += 1,
                None => sets.push(SharedKeys {
                    commands: conflict.commands.clone(),
                    shortcut: conflict.shortcut,
                    count: 1,
                }),
            }
        }
        sets
    }

    /// What is wrong with the file, as the settings report it (the subtitle, the notice and the
    /// settings window's banner): the diagnostics of [`resolve`], and one line for each set of
    /// commands that share a key — a family that shares nine keys is one line — at the line of
    /// the one the file gave it to last.
    pub fn problems(&self) -> Vec<Diagnostic> {
        let mut out = self.diagnostics.clone();
        for SharedKeys {
            commands,
            shortcut,
            count,
        } in self.shared_sets()
        {
            let names: Vec<String> = commands
                .iter()
                .map(|command| format!("`{}`", command.name))
                .collect();
            let line = commands
                .iter()
                .filter_map(|command| match self.binding(command.name)?.source {
                    Source::File(line) => line,
                    Source::Default => None,
                })
                .max();
            let message = if count == 1 {
                format!(
                    "`{shortcut}` is the shortcut of {}; the menu runs {}'s",
                    names.join(" and "),
                    names[0]
                )
            } else {
                format!(
                    "{} share {count} shortcuts, as `{shortcut}`; the menu runs {}'s",
                    names.join(" and "),
                    names[0]
                )
            };
            out.push(Diagnostic {
                key: Some("keys"),
                line,
                message,
            });
        }
        out
    }

    /// Every command with the shortcuts it has, in the menu's order.
    pub fn bindings(&self) -> &[Binding] {
        &self.bindings
    }

    /// The command named `name`.
    pub fn binding(&self, name: &str) -> Option<&Binding> {
        self.bindings
            .iter()
            .find(|binding| binding.command.name == name)
    }

    /// Every menu item that has a key, in the menu's order — the families expanded, a double
    /// press (which no menu item can show) left out.
    pub fn assignments(&self) -> Vec<Assignment> {
        let mut out = Vec::new();
        for binding in &self.bindings {
            for (slot, shortcut) in binding.shortcuts.iter().enumerate() {
                for item in binding.command.items(shortcut) {
                    out.push(Assignment {
                        command: binding.command,
                        id: item.id,
                        slot,
                        chord: item.chord,
                    });
                }
            }
        }
        out
    }

    /// The chords of each menu item, by the item's id, in the order the file gave them: the first
    /// is the key the item shows, the second the key of its hidden copy. A double press binds no
    /// item, so it takes no place in the order. The menu is written from this.
    pub fn item_keys(&self) -> HashMap<String, Vec<Chord>> {
        let mut keys: HashMap<String, Vec<Chord>> = HashMap::new();
        for assignment in self.assignments() {
            keys.entry(assignment.id)
                .or_default()
                .push(assignment.chord);
        }
        keys
    }

    /// The command that has `chord`, other than `except` (a name). The first in the menu's
    /// order: the one whose item the menu would run.
    pub fn owner_of(&self, chord: &Chord, except: &str) -> Option<&'static Command> {
        self.bindings
            .iter()
            .filter(|binding| binding.command.name != except)
            .find(|binding| {
                binding.shortcuts.iter().any(|shortcut| {
                    binding
                        .command
                        .items(shortcut)
                        .iter()
                        .any(|item| item.chord == *chord)
                })
            })
            .map(|binding| binding.command)
    }

    /// Whether `chord`, pressed while a program's view has the keyboard, is the application's
    /// and not the view's: the command that has it — the first in the menu's order, the one the
    /// menu runs — is not a content command ([`Command::content`]). A key no command has is the
    /// view's, and so is one that a content command has. Read from the shortcuts as they stand
    /// (the file's `[keys]` over the table), so a key the user moved is the application's at its
    /// new place and the view's at the old one.
    pub fn kept_from_view(&self, chord: &Chord) -> bool {
        self.owner_of(chord, "")
            .is_some_and(|command| !command.content)
    }

    /// The command that has `modifier` pressed twice, other than `except` (a name; `""` for
    /// none). The first in the menu's order: the one a double press runs, as the menu runs the
    /// first of two commands that share a chord.
    pub fn owner_of_twice(&self, modifier: Modifier, except: &str) -> Option<&'static Command> {
        self.bindings
            .iter()
            .filter(|binding| binding.command.name != except)
            .find(|binding| binding.shortcuts.contains(&Shortcut::Twice(modifier)))
            .map(|binding| binding.command)
    }

    /// The glyphs of the first double press the command `command` has (`⇧ ⇧`), for the rows that
    /// the menu bar cannot show it on.
    pub fn double_glyphs(&self, command: &Command) -> Option<String> {
        self.binding(command.name)?
            .shortcuts
            .iter()
            .find(|shortcut| matches!(shortcut, Shortcut::Twice(_)))
            .and_then(super::mapping::glyphs)
    }

    /// The command that has taken a line-editing key from the line, if one has.
    pub fn taker_of(&self, key: &LineKey) -> Option<&'static Command> {
        self.owner_of(&key.chord, "")
    }

    /// The line-editing keys some command has taken, with the command.
    pub fn taken_line_keys(&self) -> Vec<(&'static LineKey, &'static Command)> {
        LINE_EDITING
            .iter()
            .filter_map(|key| self.taker_of(key).map(|command| (key, command)))
            .collect()
    }

    /// The modifiers the family `name` (`select_tab`) gives its items: the first of only
    /// modifiers its shortcuts hold, none when it has none. The tab strip's hint, the numbers of
    /// ⌘P and the keys of the menu's items read this one.
    pub fn family_mods(&self, name: &str) -> Option<Mods> {
        let binding = self.binding(name)?;
        binding.command.family?;
        binding
            .shortcuts
            .iter()
            .find_map(|shortcut| match shortcut {
                Shortcut::Mods(mods) => Some(*mods),
                _ => None,
            })
    }
}

#[cfg(test)]
mod tests {
    use bt_core::Settings;

    use super::*;

    fn read(keys: &str) -> Resolved {
        let text = format!("[keys]\n{keys}");
        let parsed = Settings::parse(&text).expect("TOML");
        assert_eq!(parsed.diagnostics, Vec::new(), "{text}");
        resolve(&parsed.settings)
    }

    /// The settings of `keys` as a file's `[keys]` says them, without the judging.
    fn settings_of(keys: &str) -> Settings {
        let text = format!("[keys]\n{keys}");
        let parsed = Settings::parse(&text).expect("TOML");
        assert_eq!(parsed.diagnostics, Vec::new(), "{text}");
        parsed.settings
    }

    /// `keys` read over the settings in use, as a file saved while they were in effect: the
    /// settings it gives, and what the read refused.
    fn admitted(keys: &str, previous: &Settings) -> (Settings, Vec<Diagnostic>) {
        let mut settings = settings_of(keys);
        let refused = admit(&mut settings, previous);
        (settings, refused)
    }

    fn names(conflict: &Conflict) -> Vec<&'static str> {
        conflict
            .commands
            .iter()
            .map(|command| command.name)
            .collect()
    }

    /// The chord a shortcut's text is.
    fn chord(text: &str) -> Chord {
        match Shortcut::parse(text) {
            Ok(Shortcut::Chord(chord)) => chord,
            other => panic!("`{text}` is no chord: {other:?}"),
        }
    }

    #[test]
    fn the_keys_of_the_application_stay_with_it_while_a_view_has_the_keyboard() {
        let resolved = resolve(&Settings::default());
        // The keys of tabs, windows, splits, the palette, settings and quitting.
        let mut kept = vec![
            "cmd+t",
            "cmd+w",
            "cmd+n",
            "cmd+p",
            "shift+cmd+p",
            "cmd+d",
            "shift+cmd+d",
            "cmd+[",
            "cmd+]",
            "option+cmd+left",
            "option+cmd+right",
            "option+cmd+up",
            "option+cmd+down",
            "cmd+,",
            "cmd+q",
        ];
        let digits: Vec<String> = (1..=9).map(|n| format!("cmd+{n}")).collect();
        kept.extend(digits.iter().map(String::as_str));
        for key in kept {
            assert!(resolved.kept_from_view(&chord(key)), "{key}");
        }
        // The content's: copy, paste, find, clearing, size and scrolling are the view's.
        for key in [
            "cmd+z",
            "cmd+x",
            "cmd+c",
            "cmd+v",
            "ctrl+cmd+v",
            "cmd+a",
            "cmd+f",
            "cmd+g",
            "shift+cmd+g",
            "cmd+e",
            "cmd+k",
            "option+cmd+k",
            "cmd+plus",
            "cmd+-",
            "cmd+0",
            "cmd+home",
            "cmd+end",
            "cmd+pageup",
            "cmd+pagedown",
        ] {
            assert!(!resolved.kept_from_view(&chord(key)), "{key}");
        }
        // And a key no command has.
        for key in ["cmd+j", "ctrl+c", "cmd+left", "option+cmd+j"] {
            assert!(!resolved.kept_from_view(&chord(key)), "{key}");
        }
    }

    #[test]
    fn a_key_that_moved_is_read_from_its_new_place() {
        // New Tab gives ⌘T away and takes ⌘J; Find (content) takes ⌘Y.
        let resolved = read("new_tab = \"cmd+j\"\nfind = \"cmd+y\"\n");
        assert!(resolved.kept_from_view(&chord("cmd+j")));
        assert!(!resolved.kept_from_view(&chord("cmd+t")));
        assert!(!resolved.kept_from_view(&chord("cmd+y")));
        assert!(!resolved.kept_from_view(&chord("cmd+f")));
        // A family has the modifiers the file gives it.
        let resolved = read("select_tab = \"option\"\n");
        assert!(resolved.kept_from_view(&chord("option+3")));
        assert!(!resolved.kept_from_view(&chord("cmd+3")));
    }

    #[test]
    fn a_key_two_commands_share_is_the_one_the_menu_runs() {
        // Shell's New Tab is before Edit's Find in the menu: ⌘F is the application's.
        let resolved = read("new_tab = \"cmd+f\"\n");
        assert!(resolved.kept_from_view(&chord("cmd+f")));
        // And Find's is the view's when it comes first: Edit is before Window's Go to Tab.
        let resolved = read("go_to_tab = \"cmd+f\"\n");
        assert!(!resolved.kept_from_view(&chord("cmd+f")));
    }

    #[test]
    fn a_file_without_keys_is_the_table() {
        let resolved = resolve(&Settings::default());
        assert_eq!(resolved.diagnostics, Vec::new());
        assert_eq!(resolved.conflicts, Vec::new());
        assert_eq!(resolved.bindings().len(), COMMANDS.len());
        for binding in resolved.bindings() {
            assert_eq!(binding.source, Source::Default);
            assert!(!binding.changed());
            assert_eq!(binding.shortcuts, binding.command.default_shortcuts());
        }
        // The two commands that have two.
        let next = resolved.binding("show_next_tab").expect("a command");
        assert_eq!(next.shortcuts.len(), 2);
        // Nothing is taken from the line.
        assert_eq!(resolved.taken_line_keys(), Vec::new());
    }

    #[test]
    fn a_line_replaces_the_defaults_of_its_command_only() {
        let resolved = read(
            "copy_last_output = \"shift+cmd+o\"\nshow_all_tabs = \"\"\n\
             show_next_tab = [\"shift+cmd+k\"]\n",
        );
        assert_eq!(resolved.diagnostics, Vec::new());
        let copy = resolved.binding("copy_last_output").expect("a command");
        assert_eq!(copy.shortcuts.len(), 1);
        assert_eq!(copy.source, Source::File(Some(2)));
        assert!(copy.changed());
        assert_eq!(
            resolved
                .binding("show_all_tabs")
                .expect("a command")
                .shortcuts,
            Vec::new()
        );
        // One shortcut where there were two: ⌃⇥ is gone.
        assert_eq!(
            resolved
                .binding("show_next_tab")
                .expect("a command")
                .shortcuts
                .len(),
            1
        );
        // The rest is as it was.
        assert!(
            !resolved
                .binding("split_right")
                .expect("a command")
                .changed()
        );
        // A line that says what the default says is no change.
        let same = read("split_right = \"cmd+d\"\n");
        let binding = same.binding("split_right").expect("a command");
        assert_eq!(binding.source, Source::File(Some(2)));
        assert!(!binding.changed());
    }

    #[test]
    fn an_unknown_name_is_reported_with_its_line_and_changes_nothing() {
        let resolved = read("nothing_like_it = \"cmd+j\"\nsplit_right = \"cmd+e\"\n");
        assert_eq!(resolved.diagnostics.len(), 1);
        let diagnostic = &resolved.diagnostics[0];
        assert_eq!((diagnostic.key, diagnostic.line), (Some("keys"), Some(2)));
        assert!(
            diagnostic.message.contains("`keys.nothing_like_it`"),
            "{diagnostic}"
        );
        assert!(
            diagnostic.message.contains("not the name of a command"),
            "{diagnostic}"
        );
        // The line below is read.
        assert!(
            resolved
                .binding("split_right")
                .expect("a command")
                .changed()
        );
    }

    #[test]
    fn a_shortcut_that_cannot_work_is_reported_and_resolved_alone_it_gives_the_command_its_own() {
        for (line, needle) in [
            ("split_right = \"cmd+space\"", "Spotlight"),
            ("split_right = \"d\"", "alone is typing"),
            ("split_right = \"shift+d\"", "alone is typing"),
            ("split_right = \"option\"", "takes a key"),
            ("split_right = [\"cmd+e\", \"cmd+space\"]", "Spotlight"),
            ("select_tab = \"cmd+1\"", "takes only modifiers"),
            ("select_tab = \"shift\"", "alone is typing"),
            ("select_tab = \"shift+cmd\"", "has shift"),
            ("select_group = \"ctrl+shift\"", "has shift"),
            ("select_tab = \"shift+shift\"", "double press"),
            ("resize_split = \"ctrl\"", "macOS uses"),
        ] {
            let resolved = read(&format!("{line}\n"));
            assert_eq!(
                resolved.diagnostics.len(),
                1,
                "{line}: {:?}",
                resolved.diagnostics
            );
            let diagnostic = &resolved.diagnostics[0];
            assert!(diagnostic.message.contains(needle), "{line}: {diagnostic}");
            assert_eq!(diagnostic.line, Some(2), "{line}");
            let name = line.split(" =").next().expect("a name");
            assert!(
                !resolved.binding(name).expect("a command").changed(),
                "{line}"
            );
            assert_eq!(
                resolved.binding(name).expect("a command").source,
                Source::Default
            );
        }
    }

    #[test]
    fn a_shortcut_the_command_cannot_take_keeps_what_the_command_had() {
        // What cannot be taken, for a command of a key and for a family.
        let cases = [
            ("split_right", "d", "cmd+e", "alone is typing"),
            ("split_right", "cmd+space", "cmd+e", "Spotlight"),
            ("split_right", "option", "cmd+e", "takes a key"),
            ("select_tab", "shift+cmd", "option", "has shift"),
            ("select_tab", "cmd+1", "option", "takes only modifiers"),
            ("select_group", "shift", "ctrl", "alone is typing"),
        ];
        for (name, bad, held, needle) in cases {
            let line = format!("{name} = \"{bad}\"\n");
            // The first read has nothing the command was running with: its own.
            let (first, refused) = admitted(&line, &Settings::default());
            assert_eq!(first.keys, Vec::new(), "{name} = {bad}");
            assert_eq!(refused.len(), 1, "{name} = {bad}: {refused:?}");
            assert_eq!(refused[0].line, Some(2));
            assert!(refused[0].message.contains(needle), "{}", refused[0]);
            assert!(
                refused[0].message.ends_with("the line is ignored"),
                "{}",
                refused[0]
            );
            let resolved = resolve(&first);
            assert_eq!(resolved.diagnostics, Vec::new());
            let binding = resolved.binding(name).expect("a command");
            assert_eq!(binding.source, Source::Default, "{name} = {bad}");
            assert!(!binding.changed());
            // The command had a shortcut of the file: it goes on with it, at the line's place.
            let previous = settings_of(&format!("\n{name} = \"{held}\"\n"));
            let (again, refused) = admitted(&line, &previous);
            assert_eq!(refused.len(), 1, "{name} = {bad}");
            assert_eq!(again.keys, previous.keys, "{name} = {bad}");
            assert_eq!(again.keys[0].line, Some(2));
            let resolved = resolve(&again);
            assert_eq!(resolved.diagnostics, Vec::new());
            let binding = resolved.binding(name).expect("a command");
            assert_eq!(binding.source, Source::File(Some(2)));
            assert!(binding.changed(), "{name} = {bad}");
            // The same text again changes nothing: the next save of the typo is the same.
            let (twice, _) = admitted(&line, &again);
            assert_eq!(twice.keys, again.keys, "{name} = {bad}");
            assert_eq!(twice.keys[0].line, Some(2));
        }
        // The command was at its own (an entry that says what the table says is still one):
        // it stays there, and a command that had `""` stays without.
        let own = settings_of("split_right = \"cmd+d\"\n");
        let (kept, _) = admitted("split_right = \"d\"\n", &own);
        assert_eq!(kept.keys, own.keys);
        assert!(
            !resolve(&kept)
                .binding("split_right")
                .expect("a command")
                .changed()
        );
        let none = settings_of("show_all_tabs = \"\"\n");
        let (kept, _) = admitted("show_all_tabs = \"d\"\n", &none);
        assert_eq!(kept.keys, none.keys);
        assert!(
            resolve(&kept)
                .binding("show_all_tabs")
                .expect("a command")
                .shortcuts
                .is_empty()
        );
    }

    #[test]
    fn the_other_lines_of_the_file_are_read_as_usual_beside_a_refused_one() {
        let previous = settings_of("split_right = \"cmd+e\"\ncopy_last_output = \"shift+cmd+o\"\n");
        let (settings, refused) = admitted(
            "split_down = \"cmd+j\"\nsplit_right = \"d\"\nnothing_like_it = \"cmd+k\"\n\
             copy_last_output = \"shift+cmd+u\"\n",
            &previous,
        );
        // Only the refused line is reported; the name is `resolve`'s to report.
        assert_eq!(refused.len(), 1);
        assert_eq!((refused[0].line, refused[0].key), (Some(3), Some("keys")));
        let shown: Vec<(&str, String, Option<usize>)> = settings
            .keys
            .iter()
            .map(|entry| {
                (
                    entry.name.as_str(),
                    entry.shortcuts[0].to_string(),
                    entry.line,
                )
            })
            .collect();
        assert_eq!(
            shown,
            [
                ("split_down", "cmd+j".to_owned(), Some(2)),
                ("split_right", "cmd+e".to_owned(), Some(3)),
                ("nothing_like_it", "cmd+k".to_owned(), Some(4)),
                ("copy_last_output", "shift+cmd+u".to_owned(), Some(5)),
            ]
        );
        let resolved = resolve(&settings);
        assert_eq!(resolved.diagnostics.len(), 1);
        assert!(resolved.diagnostics[0].message.contains("not the name"));
        // A previous entry that is itself one the command cannot take is not "what it had".
        let mut unjudged = settings_of("split_right = \"d\"\n");
        let refused = admit(&mut unjudged, &settings_of("split_right = \"d\"\n"));
        assert_eq!((unjudged.keys.len(), refused.len()), (0, 1));
    }

    #[test]
    fn a_menu_item_has_its_chords_in_the_order_the_file_gave_them() {
        let keys = |text: &str| read(text).item_keys();
        let chord = |text: &str| match Shortcut::parse(text) {
            Ok(Shortcut::Chord(chord)) => chord,
            other => panic!("{text}: {other:?}"),
        };
        // The two keys of Show Next Tab: the item's and its hidden copy's.
        let table = keys("");
        assert_eq!(
            table["showNextTab"],
            [chord("shift+cmd+]"), chord("ctrl+tab")]
        );
        assert_eq!(table["showPreviousTab"].len(), 2);
        // A command with no key has no entry.
        assert!(!table.contains_key("copyLastOutput"));
        // A double press binds no item and takes no place: the chord after it is the first.
        let twice = keys("command_palette = [\"shift+shift\", \"cmd+k\"]\n");
        assert_eq!(twice["toggleCommandPalette"], [chord("cmd+k")]);
        // A family gives each of its items its own key, and a second shortcut a second one.
        let option = keys("select_tab = \"option\"\n");
        assert_eq!(option["selectTab:3"], [chord("option+3")]);
        assert_eq!(option["selectTab:9"], [chord("option+9")]);
        assert!(!option.contains_key("selectTab"), "its tags are 1 to 9");
        // Select Group's 0 is the item with no tag.
        assert_eq!(table["selectGroup"], [chord("ctrl+cmd+0")]);
        let two = keys("select_tab = [\"option\", \"cmd\"]\n");
        assert_eq!(two["selectTab:1"], [chord("option+1"), chord("cmd+1")]);
    }

    #[test]
    fn a_double_press_is_kept_for_a_command_that_is_not_a_family() {
        let resolved = read("command_palette = [\"shift+cmd+p\", \"shift+shift\"]\n");
        assert_eq!(resolved.diagnostics, Vec::new());
        assert_eq!(
            resolved
                .binding("command_palette")
                .expect("a command")
                .shortcuts
                .len(),
            2
        );
        // The menu has no way to show it: it holds no item.
        let items: Vec<_> = resolved
            .assignments()
            .into_iter()
            .filter(|assignment| assignment.command.name == "command_palette")
            .collect();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].slot, 0);
    }

    #[test]
    fn a_key_two_commands_share_is_a_conflict_and_the_menu_runs_the_first() {
        let resolved = read("copy_last_output = \"shift+cmd+d\"\n");
        assert_eq!(resolved.diagnostics, Vec::new());
        assert_eq!(resolved.conflicts.len(), 1);
        let conflict = &resolved.conflicts[0];
        assert_eq!(names(conflict), ["split_down", "copy_last_output"]);
        assert_eq!(
            resolved
                .owner_of(
                    &match conflict.shortcut {
                        Shortcut::Chord(chord) => chord,
                        other => panic!("{other:?}"),
                    },
                    ""
                )
                .map(|command| command.name),
            Some("split_down")
        );
        // Giving a key away ends the conflict: the other command is told it has none.
        let resolved = read("copy_last_output = \"shift+cmd+d\"\nsplit_down = \"\"\n");
        assert_eq!(resolved.conflicts, Vec::new());
    }

    #[test]
    fn a_family_item_can_be_in_a_conflict_too() {
        let resolved = read("select_tab = \"ctrl+cmd\"\n");
        assert_eq!(resolved.diagnostics, Vec::new());
        // Nine digits against Select Group's ten: 1 to 9 are shared.
        assert_eq!(resolved.conflicts.len(), 9);
        assert!(
            resolved
                .conflicts
                .iter()
                .all(|c| names(c) == ["select_tab", "select_group"])
        );
    }

    #[test]
    fn the_problems_are_the_diagnostics_and_one_line_for_each_set_of_commands_that_share_a_key() {
        let resolved = read(
            "copy_last_output = \"shift+cmd+d\"\nnot_a_command = \"cmd+j\"\n\
             select_tab = \"ctrl+cmd\"\n",
        );
        let problems = resolved.problems();
        let texts: Vec<_> = problems
            .iter()
            .map(|problem| (problem.line, problem.message.as_str()))
            .collect();
        assert_eq!(
            texts,
            [
                (
                    Some(3),
                    "`keys.not_a_command` is not the name of a command; the line is ignored"
                ),
                (
                    Some(2),
                    "`shift+cmd+d` is the shortcut of `split_down` and `copy_last_output`; the menu runs `split_down`'s"
                ),
                // Nine digits are shared, one line says so.
                (
                    Some(4),
                    "`select_tab` and `select_group` share 9 shortcuts, as `ctrl+cmd+1`; the menu runs `select_tab`'s"
                ),
            ]
        );
        assert!(problems.iter().all(|problem| problem.key == Some("keys")));
        // A file that says nothing has nothing to report.
        assert_eq!(resolve(&Settings::default()).problems(), Vec::new());
    }

    #[test]
    fn a_line_key_given_to_a_command_is_taken_from_the_line() {
        let resolved = read("jump_to_command = \"cmd+left\"\n");
        assert_eq!(resolved.diagnostics, Vec::new());
        let taken = resolved.taken_line_keys();
        assert_eq!(taken.len(), 1);
        assert_eq!(taken[0].0.title, "Move to Start of Line");
        assert_eq!(taken[0].1.name, "jump_to_command");
    }

    #[test]
    fn the_assignments_are_the_menus_items_with_the_family_expanded() {
        let resolved = resolve(&Settings::default());
        let all = resolved.assignments();
        let of = |name: &str| all.iter().filter(|a| a.command.name == name).count();
        assert_eq!(of("select_tab"), 9);
        assert_eq!(of("select_group"), 10);
        assert_eq!(of("swap_split"), 4);
        assert_eq!(of("show_next_tab"), 2);
        assert_eq!(of("split_right"), 1);
        assert_eq!(of("about"), 0);
        let second = all
            .iter()
            .find(|a| a.command.name == "show_next_tab" && a.slot == 1)
            .expect("the hidden copy");
        assert_eq!(second.id, "showNextTab");
        assert_eq!(second.chord.to_string(), "ctrl+tab");
        let tab3 = all.iter().find(|a| a.id == "selectTab:3").expect("an item");
        assert_eq!(tab3.chord.to_string(), "cmd+3");
    }

    #[test]
    fn the_modifiers_of_a_family_are_what_the_file_says() {
        let resolved = resolve(&Settings::default());
        assert_eq!(resolved.family_mods("select_tab"), Some(Mods::CMD));
        assert_eq!(
            resolved.family_mods("resize_split"),
            Some(Mods::CMD.with(bt_core::Modifier::Ctrl))
        );
        let resolved = read("select_tab = \"option\"\n");
        assert_eq!(
            resolved.family_mods("select_tab"),
            Some(Mods::NONE.with(bt_core::Modifier::Option))
        );
        // A family with no shortcut has no modifiers: the numbers and the hints go away.
        let resolved = read("select_tab = \"\"\n");
        assert_eq!(resolved.family_mods("select_tab"), None);
        // A command that is not a family has none to give.
        assert_eq!(resolved.family_mods("split_right"), None);
        assert_eq!(resolved.family_mods("nothing"), None);
    }
}
