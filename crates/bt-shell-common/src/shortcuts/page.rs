//! The rows of Settings ▸ Keyboard: every command of the table with its title and the menu it is
//! in, the shortcuts it has, and what the page shows of them (a filter, a search, a key to find).
//!
//! The page does not list the palette's rows: those leave out the items the menu greys, hides or
//! makes an alternate, and a command that cannot be run right now is still one that can be given
//! a key. It is made from the **menu as it stands** (the nodes the palette reads, [`MenuNode`])
//! and the table: an item that is a command of the table is a row, found by the palette's own
//! id of it; the title and the menu it sits in are the item's, so they follow the language and
//! the host a tab is on, as the menu does. The second copy of a command (the hidden item that
//! carries its second shortcut) is the same row, and the items of a family are one row, titled
//! after the submenu that holds them.
//!
//! A search is the palette's: [`crate::palette::score`] over a row made of the title, the menu
//! as its second line, and the words the palette finds the command by (its synonyms, its name).

use bt_core::{Chord, Mods, Shortcut};

use super::mapping::glyphs;
use super::resolve::{Resolved, SharedKeys};
use super::table::{COMMANDS, Command, Family, by_id};
use crate::keys::{LINE_EDITING, line_editing};
use crate::palette::{Kind, MenuNode, Row, command_words, menu_item_id, score};

/// What the page lists: all of the commands, the ones whose shortcuts the file changed, or the
/// ones that have none.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Filter {
    #[default]
    All,
    Changed,
    NoShortcut,
}

impl Filter {
    pub const ALL: [Filter; 3] = [Filter::All, Filter::Changed, Filter::NoShortcut];

    pub fn title(self) -> &'static str {
        match self {
            Filter::All => "All",
            Filter::Changed => "Changed",
            Filter::NoShortcut => "No Shortcut",
        }
    }
}

/// A shortcut a command has, as the page shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chip {
    pub shortcut: Shortcut,
    /// `⇧⌘D`; for a family the modifiers and its keys (`⌘1…9`).
    pub text: String,
}

/// One command's line.
#[derive(Clone, Debug, PartialEq)]
pub struct PageRow {
    pub command: &'static Command,
    /// The menu item's title (the submenu's, for a family).
    pub title: String,
    /// The menus the item is in, below the bar's own: `Shell › Mark “web” as`.
    pub path: String,
    pub chips: Vec<Chip>,
    /// The shortcuts differ from the command's own.
    pub changed: bool,
    /// What the row says on its own, under it: a key two commands share, what a family's
    /// modifiers do to the hints.
    pub note: Option<String>,
    /// Every key the row's items have, for finding a command by its key.
    chords: Vec<Chord>,
    /// The palette's row of it, for the matcher.
    search: Row,
}

impl PageRow {
    /// The second line: the submenu the item is in, when it is in one — the section heading
    /// already says the menu of the bar.
    pub fn submenu(&self) -> Option<&str> {
        self.path.split_once(" › ").map(|(_, rest)| rest)
    }

    /// Whether the command has a key of its own that can be shown in a menu (a double press
    /// cannot).
    pub fn in_menu_bar(&self) -> bool {
        self.chips
            .iter()
            .any(|chip| !matches!(chip.shortcut, Shortcut::Twice(_)))
    }
}

/// The commands of one menu of the bar.
#[derive(Clone, Debug, PartialEq)]
pub struct Section {
    pub title: String,
    pub rows: Vec<PageRow>,
}

/// A key that edits the line you type, listed at the end so that nobody looks for it in the
/// menu — or finds a command that took it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuiltIn {
    pub glyph: String,
    pub title: &'static str,
    pub effect: &'static str,
    /// The command that has the key now (its title); the line no longer has it.
    pub taken_by: Option<String>,
    chord: Chord,
}

/// The heading of the built-in keys.
pub const BUILT_IN_HEADING: &str = "Line editing — built in";

/// What the page shows for a filter, a search and a key.
#[derive(Debug, Default, PartialEq)]
pub struct Listing<'a> {
    pub sections: Vec<(&'a str, Vec<&'a PageRow>)>,
    pub built_in: Vec<&'a BuiltIn>,
}

impl Listing<'_> {
    pub fn is_empty(&self) -> bool {
        self.sections.iter().all(|(_, rows)| rows.is_empty()) && self.built_in.is_empty()
    }
}

/// The page.
#[derive(Clone, Debug, PartialEq)]
pub struct Page {
    sections: Vec<Section>,
    built_in: Vec<BuiltIn>,
}

/// A command found in the menu, before the same command's second item is folded into it.
struct Found {
    command: &'static Command,
    selector: String,
    title: String,
    path: String,
    hidden: bool,
}

/// The first of the family's items a submenu holds, if it holds a family.
fn family_of(children: &[MenuNode]) -> Option<&'static Command> {
    children
        .iter()
        .filter(|node| !node.separator)
        .find_map(|node| {
            let selector = node.selector.as_deref()?;
            by_id(&menu_item_id(selector, node.tag)).filter(|command| command.family.is_some())
        })
}

fn collect(items: &[MenuNode], path: &mut Vec<String>, hidden: bool, out: &mut Vec<Found>) {
    for node in items {
        if node.separator {
            continue;
        }
        if let Some(children) = &node.children {
            if let Some(command) = family_of(children) {
                // The family is the submenu: one row, however many items it has.
                out.push(Found {
                    command,
                    selector: command.id.to_owned(),
                    title: node.title.clone(),
                    path: path.join(" › "),
                    hidden: hidden || node.hidden,
                });
                continue;
            }
            path.push(node.title.clone());
            collect(children, path, hidden || node.hidden, out);
            path.pop();
            continue;
        }
        let Some(selector) = &node.selector else {
            continue;
        };
        let Some(command) = by_id(&menu_item_id(selector, node.tag)) else {
            continue;
        };
        if command.family.is_some() || node.title.trim().is_empty() {
            continue;
        }
        out.push(Found {
            command,
            selector: selector.clone(),
            title: node.title.clone(),
            path: path.join(" › "),
            hidden: hidden || node.hidden,
        });
    }
}

/// The keys a family's items have, written once: `⌘1…9`, `⌃⌘0…9`, `⌥⌘←→↑↓`.
pub fn family_glyphs(mods: Mods, family: Family) -> String {
    let held = glyphs(&Shortcut::Mods(mods)).unwrap_or_default();
    match family {
        Family::Tabs => format!("{held}1…9"),
        Family::Groups => format!("{held}0…9"),
        Family::Arrows => format!("{held}←→↑↓"),
    }
}

fn chip_of(command: &Command, shortcut: &Shortcut) -> Chip {
    let text = match (shortcut, command.family) {
        (Shortcut::Mods(mods), Some(family)) => family_glyphs(*mods, family),
        _ => glyphs(shortcut).unwrap_or_default(),
    };
    Chip {
        shortcut: *shortcut,
        text,
    }
}

/// The words the palette finds the command by besides its title: its synonyms and its name as
/// words (`copy_last_output`).
fn words_of(command: &Command, selector: &str) -> String {
    let mut words = command_words(selector).to_owned();
    let name = command.name.replace('_', " ");
    if !words.is_empty() {
        words.push(' ');
    }
    words.push_str(&name);
    words
}

impl Page {
    /// The page for a menu (the bar's items, each holding its menu) and the shortcuts the file
    /// gives. A command of the table that the menu has no item for is not listed: it has no
    /// title to list it by.
    pub fn new(menu: &[MenuNode], resolved: &Resolved) -> Page {
        let mut sections: Vec<Section> = Vec::new();
        let mut places: Vec<Vec<Found>> = Vec::new();
        let mut titles: Vec<String> = Vec::new();
        for bar_item in menu {
            let Some(items) = &bar_item.children else {
                continue;
            };
            let title = if bar_item.title.trim().is_empty() {
                "bateri".to_owned()
            } else {
                bar_item.title.clone()
            };
            let mut path = vec![title.clone()];
            let mut found = Vec::new();
            collect(items, &mut path, bar_item.hidden, &mut found);
            titles.push(title);
            places.push(found);
        }
        // A command that is in the menu twice (the hidden twin of an item that a submenu the
        // delegate fills cannot key) is listed where it is seen first, as what is seen last
        // when that is the item the user can see.
        let mut visible: Vec<(&'static str, usize, usize)> = Vec::new();
        for (section, found) in places.iter().enumerate() {
            for (at, item) in found.iter().enumerate() {
                if !item.hidden {
                    visible.push((item.command.name, section, at));
                }
            }
        }
        let mut listed: Vec<&'static str> = Vec::new();
        for (section, found) in places.iter().enumerate() {
            let mut rows = Vec::new();
            for item in found {
                if listed.contains(&item.command.name) {
                    continue;
                }
                listed.push(item.command.name);
                let shown = visible
                    .iter()
                    .find(|(name, ..)| *name == item.command.name)
                    .map_or(item, |(_, s, a)| &places[*s][*a]);
                if let Some(row) = row_of(shown, resolved) {
                    rows.push(row);
                }
            }
            sections.push(Section {
                title: titles[section].clone(),
                rows,
            });
        }
        sections.retain(|section| !section.rows.is_empty());
        let mut page = Page {
            sections,
            built_in: Vec::new(),
        };
        page.note_shared_keys(resolved);
        page.built_in = built_in(&page, resolved);
        page
    }

    /// The sections, in the order of the bar's menus.
    pub fn sections(&self) -> &[Section] {
        &self.sections
    }

    /// The built-in keys, with the commands that have taken them.
    pub fn built_in(&self) -> &[BuiltIn] {
        &self.built_in
    }

    /// The row of the command named `name`.
    pub fn row(&self, name: &str) -> Option<&PageRow> {
        self.sections
            .iter()
            .flat_map(|section| &section.rows)
            .find(|row| row.command.name == name)
    }

    /// The title the menu gives the command, for a note that names it.
    pub fn title_of(&self, name: &str) -> String {
        match self.row(name) {
            Some(row) => row.title.clone(),
            None => name.replace('_', " "),
        }
    }

    /// What the page shows under a filter, a search (words, as in the palette) and the key being
    /// looked for. The built-in keys are listed with no filter but All, and with a key only if
    /// it is theirs.
    pub fn listing(&self, filter: Filter, query: &str, key: Option<&Chord>) -> Listing<'_> {
        // A command is also written as its name (`copy_last_output`): the underscores are spaces.
        let query = query.trim().replace('_', " ");
        let query = query.as_str();
        let sections = self
            .sections
            .iter()
            .map(|section| {
                let rows = section
                    .rows
                    .iter()
                    .filter(|row| match filter {
                        Filter::All => true,
                        Filter::Changed => row.changed,
                        Filter::NoShortcut => row.chips.is_empty(),
                    })
                    .filter(|row| key.is_none_or(|key| row.chords.contains(key)))
                    .filter(|row| score(&row.search, query).is_some())
                    .collect();
                (section.title.as_str(), rows)
            })
            .collect();
        let built_in = self
            .built_in
            .iter()
            .filter(|_| filter == Filter::All)
            .filter(|line| key.is_none_or(|key| line.chord == *key))
            .filter(|line| {
                query.is_empty() || {
                    let mut row = Row::new(Kind::Command, "", line.title).keywords(line.effect);
                    row.subtitle = BUILT_IN_HEADING.to_owned();
                    score(&row, query).is_some()
                }
            })
            .collect();
        Listing { sections, built_in }
    }

    /// A key two commands have, said on both rows.
    fn note_shared_keys(&mut self, resolved: &Resolved) {
        let titles: Vec<(&'static str, String)> = self
            .sections
            .iter()
            .flat_map(|section| &section.rows)
            .map(|row| (row.command.name, row.title.clone()))
            .collect();
        let title = |name: &str| {
            titles
                .iter()
                .find(|(own, _)| *own == name)
                .map_or_else(|| name.replace('_', " "), |(_, title)| title.clone())
        };
        // One sentence for each set of commands, however many of their keys they share.
        let sets = resolved.shared_sets();
        for section in &mut self.sections {
            for row in &mut section.rows {
                let mut said: Vec<String> = Vec::new();
                for SharedKeys {
                    commands,
                    shortcut,
                    count,
                } in &sets
                {
                    if !commands.contains(&row.command) {
                        continue;
                    }
                    let first = commands[0];
                    let others: Vec<String> = commands
                        .iter()
                        .filter(|other| **other != row.command)
                        .map(|other| title(other.name))
                        .collect();
                    let key = glyphs(shortcut).unwrap_or_default();
                    let more = if *count > 1 {
                        format!(" (and {} more keys)", count - 1)
                    } else {
                        String::new()
                    };
                    let who = if first == row.command {
                        "the menu runs this one".to_owned()
                    } else {
                        format!("the menu runs {}’s", title(first.name))
                    };
                    said.push(format!(
                        "{key} is also {}’s{more}; {who}.",
                        others.join(" and ")
                    ));
                }
                if let Some(family) = family_note(row) {
                    said.push(family);
                }
                // The menu bar has no way to show a double press: say so, or the row reads as a
                // command that has no menu key.
                if !row.chips.is_empty() && !row.in_menu_bar() {
                    said.push(DOUBLE_PRESS_NOTE.to_owned());
                }
                row.note = (!said.is_empty()).then(|| said.join(" "));
            }
        }
    }
}

/// What a row says when a double press is all it has.
pub const DOUBLE_PRESS_NOTE: &str = "Not shown in the menu bar: a double press has no key to show.";

/// What a family's modifiers do to the tab strip's hints: ⌥⌘ held is the layout mode of the
/// panes, so the tabs show no number for it.
fn family_note(row: &PageRow) -> Option<String> {
    let family = row.command.family?;
    if !matches!(family, Family::Tabs | Family::Groups) {
        return None;
    }
    let held = row.chips.iter().find_map(|chip| match chip.shortcut {
        Shortcut::Mods(mods) => Some(mods),
        _ => None,
    })?;
    (held.option && held.cmd && !held.ctrl && !held.shift)
        .then(|| "⌥⌘ held arranges the panes, so the tabs show no numbers for it.".to_owned())
}

fn row_of(found: &Found, resolved: &Resolved) -> Option<PageRow> {
    let binding = resolved.binding(found.command.name)?;
    let chips: Vec<Chip> = binding
        .shortcuts
        .iter()
        .map(|shortcut| chip_of(found.command, shortcut))
        .collect();
    let chords: Vec<Chord> = binding
        .shortcuts
        .iter()
        .flat_map(|shortcut| found.command.items(shortcut))
        .map(|item| item.chord)
        .collect();
    let mut search = Row::new(Kind::Command, found.command.name, found.title.clone())
        .keywords(words_of(found.command, &found.selector));
    search.subtitle.clone_from(&found.path);
    Some(PageRow {
        command: found.command,
        title: found.title.clone(),
        path: found.path.clone(),
        chips,
        changed: binding.changed(),
        note: None,
        chords,
        search,
    })
}

fn built_in(page: &Page, resolved: &Resolved) -> Vec<BuiltIn> {
    LINE_EDITING
        .iter()
        .map(|key| BuiltIn {
            glyph: glyphs(&Shortcut::Chord(key.chord)).unwrap_or_default(),
            title: key.title,
            effect: key.effect,
            taken_by: resolved
                .taker_of(key)
                .map(|command| page.title_of(command.name)),
            chord: key.chord,
        })
        .collect()
}

/// The names of every command of the table: what Restore Defaults clears from the file.
pub fn command_names() -> Vec<String> {
    COMMANDS
        .iter()
        .map(|command| command.name.to_owned())
        .collect()
}

/// Whether `chord` is one of the line-editing keys ([`LINE_EDITING`]).
pub fn is_line_editing(chord: &Chord) -> bool {
    line_editing(chord).is_some()
}

#[cfg(test)]
mod tests {
    use bt_core::Settings;

    use super::super::resolve::resolve;
    use super::*;

    fn chord(text: &str) -> Chord {
        match Shortcut::parse(text) {
            Ok(Shortcut::Chord(chord)) => chord,
            other => panic!("`{text}` is not a chord: {other:?}"),
        }
    }

    /// A small menu: the second copy of Show Next Tab is hidden and carries ⌃⇥; Open Themes
    /// Folder sits hidden under View and again, visible, in the Theme submenu a delegate fills;
    /// Select Tab is a submenu of its digits; Quit and End Programs is a hidden alternate.
    fn menu() -> Vec<MenuNode> {
        let digits = (1..=9)
            .map(|n| MenuNode::item(100 + n, &format!("Tab {n}"), "selectTab:").tag(n as isize))
            .collect();
        vec![
            MenuNode::holder(
                0,
                "bateri",
                vec![
                    MenuNode::item(1, "Settings…", "openSettings:"),
                    MenuNode::item(2, "Quit bateri", "terminate:"),
                    MenuNode::item(3, "Quit and End Programs", "quitAndEndPrograms:")
                        .tag(56)
                        .alternate()
                        .hidden(),
                ],
            ),
            MenuNode::holder(
                10,
                "Edit",
                vec![
                    MenuNode::item(11, "Copy", "copy:").disabled(),
                    MenuNode::separator(12),
                    MenuNode::item(13, "Copy Last Output", "copyLastOutput:"),
                    MenuNode::item(14, "Command History…", "showCommandHistory:"),
                ],
            ),
            MenuNode::holder(
                20,
                "View",
                vec![
                    MenuNode::item(21, "Open Themes Folder", "openThemesFolder:").hidden(),
                    MenuNode::holder(
                        22,
                        "Theme",
                        vec![
                            MenuNode::item(23, "Nord", "selectTheme:"),
                            MenuNode::item(24, "Open Themes Folder", "openThemesFolder:"),
                        ],
                    )
                    .paged(),
                ],
            ),
            MenuNode::holder(
                30,
                "Window",
                vec![
                    MenuNode::item(31, "Show Next Tab", "showNextTab:"),
                    MenuNode::item(32, "Show Next Tab", "showNextTab:").hidden(),
                    MenuNode::holder(33, "Select Tab", digits),
                    MenuNode::item(34, "Split Right", "splitRight:"),
                    MenuNode::item(35, "Split Down", "splitDown:"),
                ],
            ),
        ]
    }

    fn page(keys: &str) -> (Page, Resolved) {
        let text = format!("[keys]\n{keys}");
        let settings = Settings::parse(&text).expect("TOML").settings;
        let resolved = resolve(&settings);
        (Page::new(&menu(), &resolved), resolved)
    }

    fn titles<'a>(listing: &'a Listing<'a>) -> Vec<&'a str> {
        listing
            .sections
            .iter()
            .flat_map(|(_, rows)| rows.iter().map(|row| row.title.as_str()))
            .collect()
    }

    #[test]
    fn a_command_the_menu_greys_or_hides_is_still_a_row() {
        let (page, _) = page("");
        let all = page.listing(Filter::All, "", None);
        let listed = titles(&all);
        // Copy is greyed, Quit and End Programs is a hidden alternate: both can be given a key.
        assert!(listed.contains(&"Copy"), "{listed:?}");
        assert!(listed.contains(&"Quit and End Programs"), "{listed:?}");
        let quit = page.row("quit_and_end_programs").expect("a row");
        assert_eq!(quit.chips[0].text, "⌥⌘Q");
    }

    #[test]
    fn the_hidden_copy_is_its_owners_second_shortcut_not_a_row() {
        let (page, _) = page("");
        let next: Vec<&PageRow> = page
            .sections()
            .iter()
            .flat_map(|section| &section.rows)
            .filter(|row| row.command.name == "show_next_tab")
            .collect();
        assert_eq!(next.len(), 1);
        let texts: Vec<&str> = next[0]
            .chips
            .iter()
            .map(|chip| chip.text.as_str())
            .collect();
        assert_eq!(texts, ["⇧⌘]", "⌃⇥"]);
    }

    #[test]
    fn a_family_is_one_row_titled_after_its_submenu() {
        let (page, _) = page("");
        let tabs = page.row("select_tab").expect("a row");
        assert_eq!(tabs.title, "Select Tab");
        assert_eq!(tabs.chips[0].text, "⌘1…9");
        assert_eq!(tabs.submenu(), None);
        let rows: usize = page
            .sections()
            .iter()
            .flat_map(|section| &section.rows)
            .filter(|row| row.command.name.starts_with("select_tab"))
            .count();
        assert_eq!(rows, 1, "the nine items are not nine rows");
    }

    #[test]
    fn the_visible_copy_of_a_command_gives_its_title_and_place() {
        let (page, _) = page("");
        let folder = page.row("open_themes_folder").expect("a row");
        assert_eq!(folder.path, "View › Theme");
        assert_eq!(folder.submenu(), Some("Theme"));
        // Only the table's commands: the themes themselves are not rows.
        assert!(page.row("nord").is_none());
        assert!(
            page.sections()
                .iter()
                .flat_map(|section| &section.rows)
                .all(|row| row.title != "Nord")
        );
    }

    #[test]
    fn sections_follow_the_menus_and_rows_follow_the_table_not_the_menus_whim() {
        let (page, _) = page("");
        let headings: Vec<&str> = page
            .sections()
            .iter()
            .map(|section| section.title.as_str())
            .collect();
        assert_eq!(headings, ["bateri", "Edit", "View", "Window"]);
    }

    #[test]
    fn the_filters_split_the_rows_by_what_the_file_changed() {
        let (page, _) = page("copy_last_output = \"shift+cmd+o\"\nsplit_right = \"\"\n");
        let changed = page.listing(Filter::Changed, "", None);
        assert_eq!(titles(&changed), ["Copy Last Output", "Split Right"]);
        let none = page.listing(Filter::NoShortcut, "", None);
        let listed = titles(&none);
        assert!(listed.contains(&"Split Right"), "{listed:?}");
        assert!(!listed.contains(&"Copy Last Output"), "{listed:?}");
        assert!(!listed.contains(&"Split Down"), "{listed:?}");
        // The built-in keys are listed under All only.
        assert!(page.listing(Filter::Changed, "", None).built_in.is_empty());
        assert_eq!(
            page.listing(Filter::All, "", None).built_in.len(),
            LINE_EDITING.len()
        );
    }

    #[test]
    fn a_line_of_the_file_that_says_the_default_is_no_change() {
        let (page, _) = page("split_right = \"cmd+d\"\n");
        assert!(titles(&page.listing(Filter::Changed, "", None)).is_empty());
    }

    #[test]
    fn a_search_finds_a_command_by_title_menu_synonym_and_name() {
        let (page, _) = page("");
        let find = |query: &str| {
            let listing = page.listing(Filter::All, query, None);
            titles(&listing)
                .into_iter()
                .map(str::to_owned)
                .collect::<Vec<_>>()
        };
        assert_eq!(find("last output"), ["Copy Last Output"]);
        // The name as words: `copy_last_output`.
        assert_eq!(find("copy_last"), ["Copy Last Output"]);
        // The palette's synonyms: "pane" finds the splits.
        let pane = find("pane");
        assert!(pane.contains(&"Split Right".to_owned()), "{pane:?}");
        assert!(pane.contains(&"Split Down".to_owned()), "{pane:?}");
        // The menu as the second line.
        let theme = find("theme");
        assert!(
            theme.contains(&"Open Themes Folder".to_owned()),
            "{theme:?}"
        );
        assert!(find("zzzz").is_empty());
    }

    #[test]
    fn a_key_finds_the_commands_that_have_it() {
        let (page, _) = page("");
        let hit = |text: &str| {
            let key = chord(text);
            let listing = page.listing(Filter::All, "", Some(&key));
            (
                titles(&listing)
                    .into_iter()
                    .map(str::to_owned)
                    .collect::<Vec<_>>(),
                listing.built_in.len(),
            )
        };
        assert_eq!(hit("cmd+d").0, ["Split Right"]);
        // A family's item: ⌘3 is Select Tab's.
        assert_eq!(hit("cmd+3").0, ["Select Tab"]);
        // A command's second key.
        assert_eq!(hit("ctrl+tab").0, ["Show Next Tab"]);
        // A key of the line editor is the built-in list's.
        assert_eq!(hit("cmd+backspace"), (Vec::new(), 1));
        assert_eq!(hit("cmd+j"), (Vec::new(), 0));
    }

    #[test]
    fn a_line_key_a_command_took_says_so_in_the_built_in_list() {
        let (page, _) = page("split_right = \"cmd+left\"\n");
        let taken: Vec<(&str, Option<&str>)> = page
            .built_in()
            .iter()
            .map(|line| (line.glyph.as_str(), line.taken_by.as_deref()))
            .filter(|(_, by)| by.is_some())
            .collect();
        assert_eq!(taken, [("⌘←", Some("Split Right"))]);
    }

    #[test]
    fn a_key_two_commands_have_is_said_on_both_rows() {
        let (page, _) = page("split_down = \"cmd+d\"\n");
        let right = page.row("split_right").expect("a row");
        let down = page.row("split_down").expect("a row");
        // The menu's order: Split Right comes first and runs.
        assert_eq!(
            right.note.as_deref(),
            Some("⌘D is also Split Down’s; the menu runs this one.")
        );
        assert_eq!(
            down.note.as_deref(),
            Some("⌘D is also Split Right’s; the menu runs Split Right’s.")
        );
        assert!(page.row("select_tab").expect("a row").note.is_none());
    }

    #[test]
    fn a_family_held_with_option_and_command_says_the_tabs_show_no_numbers() {
        let (page, _) = page("select_tab = \"option+cmd\"\n");
        let note = page.row("select_tab").and_then(|row| row.note.clone());
        assert_eq!(
            note.as_deref(),
            Some("⌥⌘ held arranges the panes, so the tabs show no numbers for it.")
        );
        assert!(page.row("split_right").expect("a row").note.is_none());
    }

    #[test]
    fn a_double_press_is_listed_but_not_in_the_menu_bar() {
        let (only, _) = page("split_down = \"shift+shift\"\n");
        let row = only.row("split_down").expect("a row");
        assert_eq!(row.chips[0].text, "⇧ ⇧");
        assert!(!row.in_menu_bar());
        assert!(only.row("split_right").expect("a row").in_menu_bar());
        // Only that row says the menu bar does not show it: a command with a key beside the
        // double press is shown by the key.
        assert_eq!(row.note.as_deref(), Some(DOUBLE_PRESS_NOTE));
        assert!(only.row("split_right").expect("a row").note.is_none());
        let (beside, _) = page("split_down = [\"cmd+j\", \"shift+shift\"]\n");
        assert!(beside.row("split_down").expect("a row").note.is_none());
    }

    #[test]
    fn the_names_restore_defaults_clears_are_the_tables() {
        let names = command_names();
        assert_eq!(names.len(), COMMANDS.len());
        assert!(names.iter().any(|name| name == "copy_last_output"));
        assert!(is_line_editing(&chord("cmd+backspace")));
        assert!(!is_line_editing(&chord("cmd+d")));
    }
}
