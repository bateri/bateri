//! The commands that can have a shortcut: one table, in the menu's order.
//!
//! A command **is** a menu item: the table lists the items of the menu bar the shell builds, by
//! the id the command palette already gives a menu command — the item's selector, and its tag
//! when it has one (`splitRight`, `selectTab:3`) — so the palette's row, the menu item and the
//! settings page speak about the same command without a second naming. Each row adds the
//! command's **fixed name** (the key in `settings.toml`'s `[keys]`; the title of an item changes
//! with the language and with the host a tab is on, the name does not) and the shortcuts the
//! command has until the file says otherwise.
//!
//! Not in the table: the items whose list changes as you work — the themes, the open ports, the
//! tabs a split can move to, the groups a tab can move to. A key bound to "the third theme" would
//! go to another theme the day one is added; the palette finds all of them.
//!
//! A **family** is one row for several items that differ only by a fixed key: the digits of the
//! tabs, the digits of the groups, the arrows of three split commands. The file gives the family
//! the modifiers (`select_tab = "option"`) and the items take their keys from the family.

use bt_core::{Chord, Key, Mods, NamedKey, Shortcut};

/// The items of a family and the key each one has.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Family {
    /// Select Tab ▸: the tag is the digit, 1–9 (9 is the last tab).
    Tabs,
    /// Select Group ▸: the tag is the digit, 0–9 (0 is the ungrouped tabs, 9 the last group).
    Groups,
    /// Select, Resize and Swap Split ▸: the tags 0–3 are left, right, up and down.
    Arrows,
}

impl Family {
    /// The keys of the items, with the tag of each (the menu item's tag).
    pub fn keys(self) -> Vec<(u8, Key)> {
        match self {
            Self::Tabs => (1..=9).map(|n| (n, digit(n))).collect(),
            Self::Groups => (0..=9).map(|n| (n, digit(n))).collect(),
            Self::Arrows => [
                NamedKey::Left,
                NamedKey::Right,
                NamedKey::Up,
                NamedKey::Down,
            ]
            .into_iter()
            .zip(0u8..)
            .map(|(key, tag)| (tag, Key::Named(key)))
            .collect(),
        }
    }

    /// Whether `key` is the key of one of the items.
    pub fn has(self, key: Key) -> bool {
        self.keys().iter().any(|(_, own)| *own == key)
    }

    /// Whether the keys of the items are digits, which Shift turns into other characters.
    pub fn is_digits(self) -> bool {
        matches!(self, Self::Tabs | Self::Groups)
    }

    /// What the user presses beside the modifiers: the word for a note ("Press it with a
    /// digit").
    pub fn presses(self) -> &'static str {
        match self {
            Self::Tabs | Self::Groups => "digit",
            Self::Arrows => "arrow",
        }
    }
}

fn digit(n: u8) -> Key {
    Key::Char(char::from(b'0' + n))
}

/// The menu command a row of the table names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Command {
    /// The key in `[keys]`: lowercase and underscores, fixed once released.
    pub name: &'static str,
    /// The menu item's id: its selector without the colon, and `:tag` when the tag is not 0
    /// (the command palette's own `command_key`). For a family, the id of the menu holds the
    /// root: the items add their tags to it.
    pub id: &'static str,
    /// The shortcuts the command has until the file says otherwise, as the file writes them:
    /// none, one, or two. A family has the one of only modifiers.
    pub defaults: &'static [&'static str],
    pub family: Option<Family>,
    /// Whether the command works on the **content** of the pane that has the keyboard — the text
    /// it shows, the way it is scrolled or sized — and not on the pane, the tab or the window.
    /// While a program's view covers the pane the content is the view's, so its keys are the
    /// view's and its menu items are not the pane's to run ([`super::Resolved::kept_from_view`]); every
    /// other command stays the application's whatever has the keyboard. The one place that says which
    /// commands are which: a second list of "the view's keys" would drift from the table.
    pub content: bool,
}

/// One item of a command: the id of the menu item and the chord it is bound to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub id: String,
    pub chord: Chord,
}

impl Command {
    /// The defaults, parsed. A row whose default cannot be read would be a defect of the table
    /// (a test reads every one); it has none here rather than failing.
    pub fn default_shortcuts(&self) -> Vec<Shortcut> {
        self.defaults
            .iter()
            .filter_map(|text| Shortcut::parse(text).ok())
            .collect()
    }

    /// The id of the menu item with `tag`.
    fn item_id(&self, tag: u8) -> String {
        if tag == 0 {
            self.id.to_owned()
        } else {
            format!("{}:{tag}", self.id)
        }
    }

    /// The menu items `shortcut` binds, each with its chord. A chord binds the one item; the
    /// modifiers of a family bind all of its items, each with its own key; a double press binds
    /// no menu item (the menu has no way to show one).
    pub fn items(&self, shortcut: &Shortcut) -> Vec<Item> {
        match (shortcut, self.family) {
            (Shortcut::Chord(chord), None) => vec![Item {
                id: self.id.to_owned(),
                chord: *chord,
            }],
            (Shortcut::Mods(mods), Some(family)) => family
                .keys()
                .into_iter()
                .map(|(tag, key)| Item {
                    id: self.item_id(tag),
                    chord: Chord::new(*mods, key),
                })
                .collect(),
            _ => Vec::new(),
        }
    }

    /// Whether the menu item with `id` is this command's.
    fn owns(&self, id: &str) -> bool {
        match self.family {
            None => self.id == id,
            Some(family) => family
                .keys()
                .iter()
                .any(|(tag, _)| self.item_id(*tag) == id),
        }
    }
}

const fn command(
    name: &'static str,
    id: &'static str,
    defaults: &'static [&'static str],
) -> Command {
    Command {
        name,
        id,
        defaults,
        family: None,
        content: false,
    }
}

const fn family(
    name: &'static str,
    id: &'static str,
    modifiers: &'static [&'static str],
    family: Family,
) -> Command {
    Command {
        name,
        id,
        defaults: modifiers,
        family: Some(family),
        content: false,
    }
}

/// `command` marked as working on the pane's content ([`Command::content`]).
const fn content(command: Command) -> Command {
    Command {
        content: true,
        ..command
    }
}

/// The commands, in the order of the menu bar (bateri, Shell, Edit, View, Window): the order the
/// menu runs a key that two commands share is the order here.
pub const COMMANDS: &[Command] = &[
    // bateri
    command("about", "orderFrontStandardAboutPanel", &[]),
    command("check_for_updates", "checkForUpdates", &[]),
    command("settings", "openSettings", &["cmd+,"]),
    command("open_settings_file", "openSettingsFile", &[]),
    command("hide", "hide", &["cmd+h"]),
    command("hide_others", "hideOtherApplications", &["option+cmd+h"]),
    command("show_all", "unhideAllApplications", &[]),
    command("quit", "terminate", &["cmd+q"]),
    // The alternate of Quit, shown while keep_running is "quit"; the id's tag is the menu
    // item's marker.
    command(
        "quit_and_end_programs",
        "quitAndEndPrograms:56",
        &["option+cmd+q"],
    ),
    // Shell
    command("new_window", "newWindow", &["cmd+n"]),
    command("new_tab", "newTab", &["cmd+t"]),
    command("new_local_tab", "newLocalTab", &["option+cmd+t"]),
    command("new_group", "newGroup", &["shift+cmd+n"]),
    command("new_ungrouped_tab", "newUngroupedTab", &[]),
    command("mark_production", "markHost", &[]),
    command("mark_staging", "markHost:1", &[]),
    command("mark_development", "markHost:2", &[]),
    command("mark_none", "markHost:3", &[]),
    command("shell_integration", "toggleHostIntegration", &[]),
    // The terminals the focused pane's server keeps that no pane shows, together in a new tab.
    command("open_all_background", "openAllBackground", &[]),
    // The id's tag is the menu item's marker (its title changes with the host).
    command("forget_password", "forgetPassword:47", &[]),
    command("cancel_upload", "cancelUpload", &["cmd+."]),
    command("go_to_folder", "goToFolder", &["cmd+o"]),
    command("copy_folder_path", "copyFolderPath", &[]),
    command("show_folder_in_finder", "showFolderInFinder", &[]),
    command("split_right", "splitRight", &["cmd+d"]),
    command("split_down", "splitDown", &["shift+cmd+d"]),
    command("close_tab", "closeTab", &["cmd+w"]),
    command("close_window", "closeWindow", &["shift+cmd+w"]),
    // Edit
    content(command("undo", "undoMove", &["cmd+z"])),
    content(command("cut", "cut", &["cmd+x"])),
    content(command("copy", "copy", &["cmd+c"])),
    content(command("paste", "paste", &["cmd+v"])),
    content(command("paste_escaped", "pasteEscaped", &["ctrl+cmd+v"])),
    content(command("select_all", "selectAll", &["cmd+a"])),
    content(command("copy_last_command", "copyLastCommand", &[])),
    content(command("copy_last_output", "copyLastOutput", &[])),
    content(command("command_history", "showCommandHistory", &[])),
    content(command("clear_to_start", "clearToStart", &["cmd+k"])),
    content(command(
        "clear_scrollback",
        "clearScrollback",
        &["option+cmd+k"],
    )),
    content(command("find", "findInScrollback", &["cmd+f"])),
    content(command("find_next", "findNextMatch", &["cmd+g"])),
    content(command(
        "find_previous",
        "findPreviousMatch",
        &["shift+cmd+g"],
    )),
    content(command(
        "use_selection_for_find",
        "useSelectionForFind",
        &["cmd+e"],
    )),
    // View
    command("command_palette", "toggleCommandPalette", &["shift+cmd+p"]),
    command("themes", "showThemes", &[]),
    command("open_themes_folder", "openThemesFolder", &[]),
    command("tabs_top", "tabsAtTop", &[]),
    command("tabs_side", "tabsAtSide", &[]),
    content(command("bigger", "makeFontBigger", &["cmd+plus"])),
    content(command("smaller", "makeFontSmaller", &["cmd+-"])),
    content(command("actual_size", "resetFontSize", &["cmd+0"])),
    content(command("scroll_to_top", "scrollToTop", &["cmd+home"])),
    content(command("scroll_to_bottom", "scrollToBottom", &["cmd+end"])),
    content(command("page_up", "scrollPageUp", &["cmd+pageup"])),
    content(command("page_down", "scrollPageDown", &["cmd+pagedown"])),
    // Window
    command("minimize", "performMiniaturize", &["cmd+m"]),
    command("zoom", "performZoom", &[]),
    // Two shortcuts each: the menu shows the first and keeps the second as a hidden copy.
    command(
        "show_previous_tab",
        "showPreviousTab",
        &["shift+cmd+[", "ctrl+shift+tab"],
    ),
    command("show_next_tab", "showNextTab", &["shift+cmd+]", "ctrl+tab"]),
    command("go_to_tab", "goToTab", &["cmd+p"]),
    command("jump_to_command", "jumpToCommand", &[]),
    family("select_tab", "selectTab", &["cmd"], Family::Tabs),
    family("select_group", "selectGroup", &["ctrl+cmd"], Family::Groups),
    command("move_tab_to_new_group", "moveTabToNewGroup", &[]),
    command("rename_group", "renameGroup", &[]),
    command("move_group_to_new_window", "moveGroupToNewWindow", &[]),
    command("show_all_tabs", "showTabList", &["shift+cmd+\\"]),
    command("rename_tab", "renameTab", &[]),
    command("select_previous_split", "selectPreviousSplit", &["cmd+["]),
    command("select_next_split", "selectNextSplit", &["cmd+]"]),
    family(
        "select_split",
        "selectSplit",
        &["option+cmd"],
        Family::Arrows,
    ),
    family("resize_split", "resizeSplit", &["ctrl+cmd"], Family::Arrows),
    family(
        "swap_split",
        "swapSplit",
        &["option+shift+cmd"],
        Family::Arrows,
    ),
    command("equalize_splits", "equalizeSplits", &["ctrl+cmd+="]),
    command("zoom_split", "toggleSplitZoom", &["shift+cmd+return"]),
    command("move_split_to_new_tab", "movePaneToNewTab", &[]),
    command("move_split_to_new_window", "movePaneToNewWindow", &[]),
    command(
        "move_split_to_previous_tab",
        "movePaneToPreviousTab",
        &["option+shift+cmd+["],
    ),
    command(
        "move_split_to_next_tab",
        "movePaneToNextTab",
        &["option+shift+cmd+]"],
    ),
    command("move_tab_to_new_window", "detachTab", &[]),
    command("merge_all_windows", "mergeWindows", &[]),
    command("bring_all_to_front", "arrangeInFront", &[]),
];

/// The command named `name` in `[keys]`.
pub fn by_name(name: &str) -> Option<&'static Command> {
    COMMANDS.iter().find(|command| command.name == name)
}

/// The command a menu item with this id belongs to. The items of a family (`selectTab:3`) are
/// their family's; a command with two menu items (the hidden copies of Show Next Tab) is one
/// row.
pub fn by_id(id: &str) -> Option<&'static Command> {
    COMMANDS.iter().find(|command| command.owns(id))
}

/// The modifiers a family's defaults are.
pub fn family_default(command: &Command) -> Option<Mods> {
    command.family?;
    match command.default_shortcuts().first() {
        Some(Shortcut::Mods(mods)) => Some(*mods),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;
    use crate::palette::Modifiers;
    use crate::shortcuts::mapping::{chord_of, glyphs, menu_key};

    #[test]
    fn the_names_and_the_ids_are_each_one() {
        let mut names = HashSet::new();
        let mut ids = HashSet::new();
        for command in COMMANDS {
            assert!(
                names.insert(command.name),
                "{} is named twice",
                command.name
            );
            assert!(
                ids.insert(command.id),
                "{} is in the table twice",
                command.id
            );
            assert!(
                command
                    .name
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c == '_'),
                "{} is not lowercase and underscores",
                command.name
            );
        }
    }

    #[test]
    fn every_menu_item_is_owned_by_one_command() {
        // The items of the families are the family's, the others their own; no id is two
        // commands'.
        for command in COMMANDS {
            let shortcut = command
                .default_shortcuts()
                .into_iter()
                .next()
                .unwrap_or_else(|| match command.family {
                    Some(_) => Shortcut::Mods(Mods::CMD),
                    None => Shortcut::Chord(Chord::plain(Mods::CMD, Key::Char('x'))),
                });
            for item in command.items(&shortcut) {
                assert_eq!(by_id(&item.id).map(|owner| owner.name), Some(command.name));
            }
        }
        assert_eq!(by_id("selectTab:3").map(|c| c.name), Some("select_tab"));
        assert_eq!(by_id("selectTab:9").map(|c| c.name), Some("select_tab"));
        assert_eq!(by_id("selectTab:0"), None);
        assert_eq!(by_id("selectGroup").map(|c| c.name), Some("select_group"));
        assert_eq!(by_id("selectGroup:0"), None);
        assert_eq!(by_id("resizeSplit:3").map(|c| c.name), Some("resize_split"));
        assert_eq!(by_id("resizeSplit:4"), None);
        assert_eq!(
            by_id("markHost:2").map(|c| c.name),
            Some("mark_development")
        );
        assert_eq!(by_id("nothing"), None);
    }

    #[test]
    fn a_default_is_the_shortcut_its_text_says_and_comes_back_unchanged() {
        for command in COMMANDS {
            assert!(
                command.defaults.len() <= bt_core::MAX_SHORTCUTS,
                "{}",
                command.name
            );
            for text in command.defaults {
                let shortcut = Shortcut::parse(text)
                    .unwrap_or_else(|error| panic!("{}: `{text}`: {error}", command.name));
                // The file's text is the canonical one.
                assert_eq!(shortcut.to_string(), *text, "{}", command.name);
                match (shortcut, command.family) {
                    (Shortcut::Mods(_), Some(_)) => {}
                    (Shortcut::Chord(chord), None) => {
                        // Through the menu's form and back.
                        let held = menu_key(&chord);
                        assert_eq!(
                            chord_of(&held.key, held.modifiers),
                            Some(chord),
                            "{}",
                            command.name
                        );
                        // And it is a shortcut the user can see.
                        assert!(glyphs(&shortcut).is_some(), "{}", command.name);
                    }
                    (other, family) => {
                        panic!("{}: {other:?} is not a default of {family:?}", command.name)
                    }
                }
            }
        }
    }

    /// What the menu bar holds for each of today's keyed items, written out by hand from the
    /// menu's construction: the selector, the key equivalent and whether ⌘, ⌃, ⌥, ⇧ are in the
    /// mask. A key typed into the table that the menu does not have today shows here.
    #[test]
    fn the_defaults_are_the_keys_the_menu_has_today() {
        const TODAY: &[(&str, &str, &str)] = &[
            ("openSettings", ",", "c"),
            ("hide", "h", "c"),
            ("hideOtherApplications", "h", "co"),
            ("terminate", "q", "c"),
            ("quitAndEndPrograms:56", "q", "co"),
            ("newWindow", "n", "c"),
            ("newTab", "t", "c"),
            ("newLocalTab", "t", "co"),
            ("newGroup", "n", "cs"),
            ("cancelUpload", ".", "c"),
            ("goToFolder", "o", "c"),
            ("splitRight", "d", "c"),
            ("splitDown", "d", "cs"),
            ("closeTab", "w", "c"),
            ("closeWindow", "w", "cs"),
            ("undoMove", "z", "c"),
            ("cut", "x", "c"),
            ("copy", "c", "c"),
            ("paste", "v", "c"),
            ("pasteEscaped", "v", "ck"),
            ("selectAll", "a", "c"),
            ("clearToStart", "k", "c"),
            ("clearScrollback", "k", "co"),
            ("findInScrollback", "f", "c"),
            ("findNextMatch", "g", "c"),
            ("findPreviousMatch", "g", "cs"),
            ("useSelectionForFind", "e", "c"),
            ("toggleCommandPalette", "p", "cs"),
            ("makeFontBigger", "+", "c"),
            ("makeFontSmaller", "-", "c"),
            ("resetFontSize", "0", "c"),
            ("scrollToTop", "\u{F729}", "c"),
            ("scrollToBottom", "\u{F72B}", "c"),
            ("scrollPageUp", "\u{F72C}", "c"),
            ("scrollPageDown", "\u{F72D}", "c"),
            ("performMiniaturize", "m", "c"),
            ("showPreviousTab", "{", "c"),
            ("showPreviousTab", "\t", "ks"),
            ("showNextTab", "}", "c"),
            ("showNextTab", "\t", "k"),
            ("goToTab", "p", "c"),
            ("showTabList", "|", "c"),
            ("selectPreviousSplit", "[", "c"),
            ("selectNextSplit", "]", "c"),
            ("equalizeSplits", "=", "ck"),
            ("toggleSplitZoom", "\r", "cs"),
            ("movePaneToPreviousTab", "{", "co"),
            ("movePaneToNextTab", "}", "co"),
        ];
        // `c` ⌘, `k` ⌃ (control), `o` ⌥, `s` ⇧.
        let mask = |letters: &str| Modifiers {
            control: letters.contains('k'),
            option: letters.contains('o'),
            shift: letters.contains('s'),
            command: letters.contains('c'),
        };
        let mut seen: Vec<(String, String, Modifiers)> = Vec::new();
        for command in COMMANDS.iter().filter(|command| command.family.is_none()) {
            for shortcut in command.default_shortcuts() {
                for item in command.items(&shortcut) {
                    let held = menu_key(&item.chord);
                    seen.push((item.id, held.key, held.modifiers));
                }
            }
        }
        let mut today: Vec<(String, String, Modifiers)> = TODAY
            .iter()
            .map(|(id, key, letters)| ((*id).to_owned(), (*key).to_owned(), mask(letters)))
            .collect();
        seen.sort_by_key(|entry| format!("{entry:?}"));
        today.sort_by_key(|entry| format!("{entry:?}"));
        assert_eq!(seen, today);

        // The families, key by key: Select Tab ⌘1…9, Select Group ⌃⌘0…9, the splits' arrows.
        let family_keys = |name: &str| {
            let command = by_name(name).expect("a family");
            let mods = family_default(command).expect("its modifiers");
            command
                .items(&Shortcut::Mods(mods))
                .into_iter()
                .map(|item| {
                    let held = menu_key(&item.chord);
                    (item.id, held.key, held.modifiers)
                })
                .collect::<Vec<_>>()
        };
        let tabs = family_keys("select_tab");
        assert_eq!(tabs.len(), 9);
        assert_eq!(
            tabs[0],
            ("selectTab:1".to_owned(), "1".to_owned(), mask("c"))
        );
        assert_eq!(
            tabs[8],
            ("selectTab:9".to_owned(), "9".to_owned(), mask("c"))
        );
        let groups = family_keys("select_group");
        assert_eq!(groups.len(), 10);
        assert_eq!(
            groups[0],
            ("selectGroup".to_owned(), "0".to_owned(), mask("ck"))
        );
        assert_eq!(
            groups[9],
            ("selectGroup:9".to_owned(), "9".to_owned(), mask("ck"))
        );
        let swap = family_keys("swap_split");
        assert_eq!(
            swap,
            vec![
                ("swapSplit".to_owned(), "\u{F702}".to_owned(), mask("cos")),
                ("swapSplit:1".to_owned(), "\u{F703}".to_owned(), mask("cos")),
                ("swapSplit:2".to_owned(), "\u{F700}".to_owned(), mask("cos")),
                ("swapSplit:3".to_owned(), "\u{F701}".to_owned(), mask("cos")),
            ]
        );
        assert_eq!(family_keys("select_split")[0].2, mask("co"));
        assert_eq!(family_keys("resize_split")[0].2, mask("ck"));
    }

    #[test]
    fn the_commands_with_no_shortcut_are_the_menus_twenty_eight_and_four_new() {
        let none: Vec<&str> = COMMANDS
            .iter()
            .filter(|command| command.defaults.is_empty())
            .map(|command| command.name)
            .collect();
        // The twenty-eight items of the menu that have no key today, and the four that
        // join the menu: Command History…, Themes… and Open Themes Folder (which the theme
        // list builds every time it opens), and Open All in New Tab.
        assert_eq!(none.len(), 28 + 4, "{none:?}");
        for new in [
            "command_history",
            "themes",
            "open_themes_folder",
            "open_all_background",
        ] {
            assert!(none.contains(&new), "{new}");
        }
    }

    /// The commands that work on a pane's content are the Edit menu — it is one run of the
    /// table — and the View items that scroll the content or size it. A command added to either
    /// without the mark would give a program's view a key the application keeps, and the view
    /// would never see it.
    #[test]
    fn the_content_commands_are_the_edit_menu_and_the_views_scrolling_and_size() {
        const SCROLLING_AND_SIZE: &[&str] = &[
            "bigger",
            "smaller",
            "actual_size",
            "scroll_to_top",
            "scroll_to_bottom",
            "page_up",
            "page_down",
        ];
        let at = |name: &str| {
            COMMANDS
                .iter()
                .position(|command| command.name == name)
                .expect("a command")
        };
        let (first, last) = (at("undo"), at("use_selection_for_find"));
        // Undo to Use Selection for Find: the fifteen rows of the Edit menu.
        assert_eq!(last - first + 1, 15);
        for (place, command) in COMMANDS.iter().enumerate() {
            let content =
                (first..=last).contains(&place) || SCROLLING_AND_SIZE.contains(&command.name);
            assert_eq!(command.content, content, "{}", command.name);
        }
        // The palette and the menu name them by these ids.
        assert!(by_id("undoMove").expect("a command").content);
        assert!(by_id("findInScrollback").expect("a command").content);
        assert!(!by_id("newTab").expect("a command").content);
        assert!(!by_id("selectTab:3").expect("a family").content);
    }

    #[test]
    fn a_family_has_the_keys_it_is_named_for() {
        assert_eq!(Family::Tabs.keys().len(), 9);
        assert_eq!(Family::Groups.keys().len(), 10);
        assert_eq!(Family::Arrows.keys().len(), 4);
        assert!(Family::Tabs.has(Key::Char('9')));
        assert!(!Family::Tabs.has(Key::Char('0')));
        assert!(Family::Groups.has(Key::Char('0')));
        assert!(Family::Arrows.has(Key::Named(NamedKey::Up)));
        assert!(!Family::Arrows.has(Key::Char('1')));
        // A chord on a family, or modifiers on a command, bind nothing.
        let tabs = by_name("select_tab").expect("a family");
        let chord = Shortcut::Chord(Chord::plain(Mods::CMD, Key::Char('1')));
        assert_eq!(tabs.items(&chord), Vec::new());
        let split = by_name("split_right").expect("a command");
        assert_eq!(split.items(&Shortcut::Mods(Mods::CMD)), Vec::new());
    }
}
