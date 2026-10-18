//! What each page of the palette lists, and what choosing a settings row does.
//!
//! The root lists the **menu's commands** (the shell read the menu: [`MenuRows`]), the settings
//! the catalog offers, the themes, and the pages ("Themes…", "All Settings…"). A page lists one
//! thing: a submenu's items, every setting, a theme slot's themes, a setting's choices. The shell
//! hands the three sources in and asks for the rows; it keeps the machine ([`super::Palette`]) and
//! the drawing.
//!
//! Choosing a row that is not a menu command is a **write** — [`chosen`] says which edits, in
//! order — except that a switch is flipped where it stands and the palette stays open.

use bt_core::{Settings, SettingsEdit};

use super::catalog::{self, Control};
use super::history::{self, Commands};
use super::menu::MenuRows;
use super::row::{Kind, Row, Scope, Slot};
use super::state::Modifier;
use super::switch::{self, Places};
use super::targets::{self, Targets};
use super::themes::{self, ThemeEntry};

/// The key of the View ▸ Theme ▸ page in the menu rows: the themes are the palette's own rows
/// and this page would list them a second time.
const THEME_MENU: &str = "View.Theme";

/// What the rows are built from.
pub struct Sources<'a> {
    pub menu: &'a MenuRows,
    /// The settings the file holds: the values rows show and the ✓ they carry.
    pub settings: &'a Settings,
    pub themes: &'a [ThemeEntry],
    /// The tabs and the groups of every window.
    pub places: &'a Places,
    /// The folders, the servers and the ports.
    pub targets: &'a Targets,
    /// The commands the open panes ran.
    pub commands: &'a Commands,
    /// The keys of the ports that started listening since the palette was last opened.
    pub fresh_ports: &'a [String],
}

/// The pages the root offers beside what a query finds.
fn go_to() -> [Row; 3] {
    [
        Row::new(Kind::Scope, "history", "Command History…")
            .keywords("commands ran previous terminal shell output")
            .opens(
                Scope::History {
                    jump: false,
                    query: None,
                },
                true,
            ),
        Row::new(Kind::Scope, "themes", "Themes…")
            .keywords("theme colors color scheme appearance dark light")
            .opens(Scope::Theme(Slot::Active), true),
        Row::new(Kind::Scope, "settings", "All Settings…")
            .keywords("preferences options configure")
            .opens(Scope::Settings, true),
    ]
}

/// The root's rows.
pub fn root_rows(src: &Sources<'_>) -> Vec<Row> {
    let mut rows: Vec<Row> = src
        .menu
        .root_rows()
        .into_iter()
        .filter(|row| row.id != format!("scope.menu.{THEME_MENU}"))
        .collect();
    // What View ▸ Theme ▸ holds besides the themes (Open Themes Folder) is a command like the
    // others; its page is not offered, so it stands at the root.
    if let Some(page) = src.menu.page(THEME_MENU) {
        for entry in page {
            let id = entry.row.id.as_str();
            if id.starts_with("cmd.selectTheme") || id == "cmd.matchSystemTheme" {
                continue;
            }
            let mut row = entry.row.clone();
            row.subtitle = "View › Theme".to_owned();
            rows.push(row);
        }
    }
    rows.extend(catalog::rows(src.settings));
    rows.extend(themes::rows(Slot::Active, src.themes, src.settings));
    rows.extend(switch::tab_rows(src.places));
    rows.extend(switch::group_rows(src.places));
    rows.extend(targets::folder_rows(src.targets));
    rows.extend(targets::host_rows(src.targets));
    rows.extend(targets::port_rows(src.targets));
    rows.extend(history::command_rows(src.commands, false));
    rows.extend(go_to());
    rows.extend(targets::go_to(src.targets));
    rows
}

/// What the root suggests when the field is empty ([`history::suggested`]).
pub fn suggested_rows(src: &Sources<'_>) -> Vec<Row> {
    history::suggested(
        src.places,
        src.commands,
        &src.targets.ports,
        src.fresh_ports,
    )
}

/// The rows of the page `scope` names.
/// (The tabs page lists them in the order of [`switch::tab_list`]'s empty field; its headings
/// and its clusters are that function's.)
pub fn page_rows(scope: &Scope, src: &Sources<'_>) -> Vec<Row> {
    match scope {
        Scope::Menu(key) => src
            .menu
            .page(key)
            .map(|rows| rows.iter().map(|entry| entry.row.clone()).collect())
            .unwrap_or_default(),
        Scope::Settings => catalog::rows(src.settings),
        Scope::Theme(slot) => themes::rows(*slot, src.themes, src.settings),
        Scope::Choices(key) => catalog::find(key)
            .map(|setting| setting.choice_rows(src.settings))
            .unwrap_or_default(),
        Scope::Tabs => switch::tab_rows(src.places)
            .into_iter()
            .chain(switch::group_rows(src.places))
            .collect(),
        Scope::TabActions(_) | Scope::GroupActions(_) => switch::actions(scope, src.places),
        Scope::Folders => targets::folder_rows(src.targets),
        Scope::Hosts => targets::host_rows(src.targets),
        Scope::Ports => targets::port_rows(src.targets),
        Scope::History { jump, .. } => history::command_rows(src.commands, *jump),
        Scope::HistoryActions(_) => history::actions(scope, src.commands),
        _ => Vec::new(),
    }
}

/// What choosing a settings row does.
#[derive(Clone, Debug, PartialEq)]
pub struct Chosen {
    /// The edits to write, in order.
    pub edits: Vec<SettingsEdit>,
    /// Whether the palette closes after: a switch flips in place and the list stays.
    pub close: bool,
}

/// The write the row `id` stands for, chosen with `modifier` on the page `scope` (`None`: the
/// root); `None` for a row that is not a settings row — a menu command, a page — or a use the row
/// does not have. `settings` is what the file holds (a switch flips from it); `dark` is the
/// system's appearance now.
pub fn chosen(
    id: &str,
    scope: Option<&Scope>,
    modifier: Modifier,
    settings: &Settings,
    dark: bool,
) -> Option<Chosen> {
    if let Some(key) = id.strip_prefix(Kind::Setting.prefix()) {
        let setting = catalog::find(key)?;
        if modifier != Modifier::None || setting.control() != Control::Toggle {
            return None;
        }
        let flipped = if setting.current(settings) == "on" {
            "off"
        } else {
            "on"
        };
        return Some(Chosen {
            edits: setting.edit(flipped).into_iter().collect(),
            close: false,
        });
    }
    if let Some(token) = id.strip_prefix(Kind::Choice.prefix()) {
        let Some(Scope::Choices(key)) = scope else {
            return None;
        };
        if modifier != Modifier::None {
            return None;
        }
        let edit = catalog::find(key)?.edit(token)?;
        return Some(Chosen {
            edits: vec![edit],
            close: true,
        });
    }
    if let Some(name) = id.strip_prefix(Kind::Theme.prefix()) {
        let slot = match scope {
            Some(Scope::Theme(slot)) => *slot,
            None => Slot::Active,
            Some(_) => return None,
        };
        let edits = themes::edits(slot, name, modifier, dark);
        return (!edits.is_empty()).then_some(Chosen { edits, close: true });
    }
    None
}

#[cfg(test)]
mod tests {
    use bt_core::SYSTEM_THEME;

    use super::super::menu::{MenuNode, rows as menu_rows};
    use super::super::themes::{Group, swatch};
    use super::*;

    fn menu() -> MenuRows {
        menu_rows(&[MenuNode::holder(
            0,
            "View",
            vec![
                MenuNode::item(1, "Bigger", "makeFontBigger:"),
                MenuNode::holder(
                    2,
                    "Theme",
                    vec![
                        MenuNode::item(3, "Match System", "matchSystemTheme:"),
                        MenuNode::item(4, "Nord", "selectTheme:"),
                        MenuNode::item(5, "Open Themes Folder", "openThemesFolder:"),
                    ],
                )
                .paged(),
            ],
        )])
    }

    fn entries() -> Vec<ThemeEntry> {
        vec![ThemeEntry {
            name: "nord".to_owned(),
            title: "Nord".to_owned(),
            group: Group::Dark,
            swatch: swatch(&bt_core::Theme::BATERI),
        }]
    }

    fn ids(rows: &[Row]) -> Vec<&str> {
        rows.iter().map(|row| row.id.as_str()).collect()
    }

    #[test]
    fn the_root_lists_commands_settings_themes_and_the_pages_but_not_the_theme_menus_page() {
        let (menu, settings, themes) = (menu(), Settings::default(), entries());
        let rows = root_rows(&Sources {
            menu: &menu,
            settings: &settings,
            themes: &themes,
            places: &Places::default(),
            targets: &Targets::default(),
            commands: &Commands::default(),
            fresh_ports: &[],
        });
        let ids = ids(&rows);
        assert!(ids.contains(&"cmd.makeFontBigger"));
        assert!(ids.contains(&"set.appearance.theme"));
        assert!(ids.contains(&"theme.nord"));
        assert!(ids.contains(&"theme.system"));
        assert!(ids.contains(&"scope.themes"));
        assert!(ids.contains(&"scope.settings"));
        assert!(!ids.contains(&"scope.menu.View.Theme"));
        // The theme menu's own themes are the palette's rows, not menu commands...
        assert!(!ids.iter().any(|id| id.starts_with("cmd.selectTheme")));
        assert!(!ids.contains(&"cmd.matchSystemTheme"));
        // ...but what else it holds is a command like the rest, and runs the menu's item.
        assert!(ids.contains(&"cmd.openThemesFolder"));
        assert_eq!(menu.token_of("cmd.openThemesFolder"), Some(5));
    }

    #[test]
    fn a_page_lists_its_own_thing() {
        let (menu, settings, themes) = (menu(), Settings::default(), entries());
        let places = Places::default();
        let src = Sources {
            menu: &menu,
            settings: &settings,
            themes: &themes,
            places: &places,
            targets: &Targets::default(),
            commands: &Commands::default(),
            fresh_ports: &[],
        };
        assert_eq!(page_rows(&Scope::Settings, &src).len(), 35);
        let themed = page_rows(&Scope::Theme(Slot::Active), &src);
        assert_eq!(ids(&themed), ["theme.system", "theme.nord"]);
        let shapes = page_rows(&Scope::Choices("terminal.cursor".to_owned()), &src);
        assert!(shapes.iter().all(|row| row.kind == Kind::Choice));
        assert!(shapes.iter().any(|row| row.checked));
        assert!(page_rows(&Scope::Choices("no.such".to_owned()), &src).is_empty());
        assert!(page_rows(&Scope::Tabs, &src).is_empty());
        assert!(page_rows(&Scope::Folders, &src).is_empty());
    }

    #[test]
    fn the_folders_hosts_and_ports_are_rows_at_the_root_and_on_their_pages() {
        use super::super::targets::{FolderFacts, HostFacts, PortFacts};
        let (menu, settings, themes) = (menu(), Settings::default(), entries());
        let targets = Targets {
            home: "/Users/a".to_owned(),
            folders: vec![FolderFacts {
                path: "/Users/a/api".to_owned(),
                tabs: 1,
                branch: None,
            }],
            hosts: vec![HostFacts {
                name: "db-1".to_owned(),
                mark: bt_core::HostMark::None,
                configured: true,
                tabs: 0,
            }],
            ports: vec![PortFacts {
                key: "3000".to_owned(),
                title: "localhost:3000".to_owned(),
                subtitle: "node".to_owned(),
                url: "http://localhost:3000".to_owned(),
            }],
        };
        let src = Sources {
            menu: &menu,
            settings: &settings,
            themes: &themes,
            places: &Places::default(),
            targets: &targets,
            commands: &Commands::default(),
            fresh_ports: &[],
        };
        let root = root_rows(&src);
        let root = ids(&root);
        for id in [
            "dir./Users/a/api",
            "host.db-1",
            "port.3000",
            "scope.hosts",
            "scope.ports",
        ] {
            assert!(root.contains(&id), "{id}");
        }
        assert_eq!(ids(&page_rows(&Scope::Folders, &src)), ["dir./Users/a/api"]);
        assert_eq!(ids(&page_rows(&Scope::Hosts, &src)), ["host.db-1"]);
        assert_eq!(ids(&page_rows(&Scope::Ports, &src)), ["port.3000"]);
    }

    #[test]
    fn a_switch_flips_where_it_stands_and_the_palette_stays() {
        let settings = Settings {
            shell_ports: false,
            ..Settings::default()
        };
        let on = chosen("set.shell.ports", None, Modifier::None, &settings, true).expect("a write");
        assert_eq!(on.edits, [SettingsEdit::ShellPorts(true)]);
        assert!(!on.close);
        let settings = Settings {
            shell_ports: true,
            ..settings
        };
        let off = chosen("set.shell.ports", None, Modifier::None, &settings, true).expect("write");
        assert_eq!(off.edits, [SettingsEdit::ShellPorts(false)]);
        // A setting with values opens its page; its row is not a write.
        assert_eq!(
            chosen("set.terminal.cursor", None, Modifier::None, &settings, true),
            None
        );
    }

    #[test]
    fn a_choice_writes_its_value_on_the_settings_page_and_closes() {
        let page = Scope::Choices("terminal.cursor".to_owned());
        let settings = Settings::default();
        let beam = chosen("opt.beam", Some(&page), Modifier::None, &settings, true).expect("write");
        assert_eq!(
            beam.edits,
            [SettingsEdit::Cursor(bt_core::CaretShape::Beam)]
        );
        assert!(beam.close);
        // Off its page, or with ⌥, a choice is nothing.
        assert_eq!(
            chosen("opt.beam", None, Modifier::None, &settings, true),
            None
        );
        assert_eq!(
            chosen("opt.beam", Some(&page), Modifier::Option, &settings, true),
            None
        );
        assert_eq!(
            chosen("opt.sideways", Some(&page), Modifier::None, &settings, true),
            None
        );
    }

    #[test]
    fn a_theme_chosen_at_the_root_is_the_one_theme_and_on_a_page_that_pages_slot() {
        let settings = Settings::default();
        let root = chosen("theme.nord", None, Modifier::None, &settings, true).expect("write");
        assert_eq!(root.edits, [SettingsEdit::Theme("nord".to_owned())]);
        assert!(root.close);
        let page = Scope::Theme(Slot::Light);
        let light = chosen("theme.nord", Some(&page), Modifier::None, &settings, true);
        assert_eq!(
            light.map(|chosen| chosen.edits),
            Some(vec![SettingsEdit::LightTheme("nord".to_owned())])
        );
        let option = chosen("theme.nord", None, Modifier::Option, &settings, false).expect("write");
        assert_eq!(
            option.edits,
            [
                SettingsEdit::LightTheme("nord".to_owned()),
                SettingsEdit::Theme(SYSTEM_THEME.to_owned()),
            ]
        );
    }

    #[test]
    fn a_menu_command_is_not_a_write() {
        let settings = Settings::default();
        assert_eq!(
            chosen("cmd.splitRight", None, Modifier::None, &settings, true),
            None
        );
        assert_eq!(
            chosen("scope.themes", None, Modifier::None, &settings, true),
            None
        );
    }
}
