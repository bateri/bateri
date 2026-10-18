//! The tabs and the groups of every window, as the palette lists and acts on them (⌘P).
//!
//! The shell reads what each tab and group says of itself ([`Places`]); the rules here turn it
//! into rows, order them, and say what choosing a row or one of its actions comes to ([`Act`]).
//!
//! **Order.** The tabs come in the order they were looked at ([`listing_order`]): the one on
//! screen last, so the list opens on the tab before it and ⌘P ⏎ goes there, and again to come
//! back. An empty field lists the last five looked at under *Recent*, the groups, and the rest
//! under *Earlier* with the tab on screen at the very end, ticked; a query lists the tabs and the
//! groups it finds, the nearer tab first among equals ([`super::rank::tabs`]) — all of them: unlike
//! a root search the page does not cut a cluster to six.
//!
//! **Keys.** A tab shows the ⌘ digit the bar gives it and a group the ⌃⌘ digit, only in the key
//! window and only for the tabs and groups that window lists ([`tab_keys`], [`group_key`]): the
//! digits are the bar's own arithmetic ([`tabs::shortcut_hint`], [`Groups::numbered`]), so the
//! palette and the bar cannot number a tab differently.
//!
//! **Identity.** A tab is `tab.<id>`, a group `grp.<window>.<group>` (`.0` for the window's
//! ungrouped tabs — a group's identity is its window's own). The menu of a row (→) is the page of
//! `act.` rows its [`Scope::TabActions`] / [`Scope::GroupActions`] names.

use std::time::Duration;

use bt_core::Mods;

use crate::groups::{Groups, View};
use crate::tabs::{self, Indicator, Tabs};

use super::list::{List, RECENT_HEADING};
use super::rank::{self, Hit};
use super::row::{Kind, Row, Scope, Status, Tone};
use super::state::Modifier;
use super::visits::Visits;

/// The heading over the groups.
pub const GROUPS_HEADING: &str = "Groups";
/// The heading over the tabs not among the last looked at.
pub const EARLIER_HEADING: &str = "Earlier";
/// How many tabs an empty field lists under Recent.
pub const RECENT_TABS: usize = 5;

/// What a tab reports besides its title: the one thing its chip shows (`tabs::indicator`) and, for
/// a running command, how long it has run.
#[derive(Clone, Debug, PartialEq)]
pub struct Report {
    pub indicator: Indicator,
    pub running: Option<Duration>,
}

/// What the palette is told of one tab.
#[derive(Clone, Debug, PartialEq)]
pub struct TabFacts {
    pub tab: u64,
    pub window: u64,
    pub title: String,
    pub report: Option<Report>,
    /// The directory, or `host:directory` in a remote session ([`tabs::place`]).
    pub place: Option<String>,
    /// The group it is in: its identity in the window, and its name.
    pub group: Option<(u64, String)>,
    /// Which window it is in, counted from 1, when it is not the key window's.
    pub other_window: Option<usize>,
    /// The ⌘ digit that reaches it ([`tab_keys`]).
    pub key: Option<String>,
    /// It is the tab on screen in the key window.
    pub here: bool,
}

/// What the palette is told of one group, or of the ungrouped tabs of a window that has groups.
#[derive(Clone, Debug, PartialEq)]
pub struct GroupFacts {
    pub window: u64,
    /// The group's identity in its window; `None` for the ungrouped tabs.
    pub group: Option<u64>,
    pub name: String,
    /// The ⌃⌘ digit that reaches it ([`group_key`]).
    pub key: Option<String>,
    /// What asks for the user in it (`tabs::group_indicator`).
    pub indicator: Option<Indicator>,
    pub tabs: usize,
    /// The title of the tab the user was last on in it: where ⏎ goes.
    pub back_to: Option<String>,
    /// Its tabs are the ones the window shows now.
    pub shown: bool,
    pub other_window: Option<usize>,
}

/// Everything the tabs page and the tabs' rows at the root are made from.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Places {
    /// In the order they are listed ([`listing_order`]).
    pub tabs: Vec<TabFacts>,
    /// The key window's ungrouped tabs and groups first, then the other windows' that have any.
    pub groups: Vec<GroupFacts>,
}

// ─── Order and keys ───────────────────────────────────────────────────────

/// The tabs `strip` lists (every window's, in the order the shell walks them) in the order the
/// palette shows them: the visited ones newest first, then those never visited in the strip's
/// order, and the tab on screen (`here`) last.
pub fn listing_order(visits: &Visits<u64>, strip: &[u64], here: Option<u64>) -> Vec<u64> {
    let mut order = visits.order(strip);
    if let Some(here) = here
        && let Some(at) = order.iter().position(|&tab| tab == here)
    {
        let tab = order.remove(at);
        order.push(tab);
    }
    order
}

/// The hint of each tab the window lists in `view`: the bar's numbers (⌘1…⌘8, the last tab ⌘9, with
/// the modifiers Select Tab has — `None` when the file took them away, and then no tab has a
/// hint), none for a tab no key reaches (a closed group's hidden ones, the middle of a long list).
pub fn tab_keys<K: Copy + Eq>(
    groups: &Groups<K>,
    tabs: &Tabs<K>,
    view: View,
    mods: Option<Mods>,
) -> Vec<(K, String)> {
    let Some(mods) = mods else {
        return Vec::new();
    };
    let listed = groups.numbered(tabs, view);
    let count = listed.len();
    listed
        .iter()
        .enumerate()
        .filter_map(|(at, &tab)| tabs::shortcut_hint(at, count, mods).map(|hint| (tab, hint)))
        .collect()
}

/// The hint of the group at `position` among `count` groups (`None`: the ungrouped tabs, key 0):
/// the digits the Select Group items carry, with Select Group's modifiers (`mods`; `None` when the
/// file took them away and then no group has a hint).
pub fn group_key(position: Option<usize>, count: usize, mods: Option<Mods>) -> Option<String> {
    let mods = mods?;
    let digit = match position {
        None => 0,
        Some(at) => tabs::shortcut_digit(at, count)?,
    };
    Some(crate::shortcuts::digit_glyphs(mods, digit))
}

// ─── Rows ─────────────────────────────────────────────────────────────────

/// The identity of the row of tab `tab`.
pub fn tab_id(tab: u64) -> String {
    format!("{}{tab}", Kind::Tab.prefix())
}

/// The identity of the row of group `group` of `window` (`None`: the ungrouped tabs).
pub fn group_id(window: u64, group: Option<u64>) -> String {
    format!("{}{window}.{}", Kind::Group.prefix(), group.unwrap_or(0))
}

fn parse_tab(id: &str) -> Option<u64> {
    id.strip_prefix(Kind::Tab.prefix())?.parse().ok()
}

fn parse_group(id: &str) -> Option<(u64, Option<u64>)> {
    let (window, group) = id.strip_prefix(Kind::Group.prefix())?.split_once('.')?;
    let group: u64 = group.parse().ok()?;
    Some((window.parse().ok()?, (group != 0).then_some(group)))
}

/// The role a status wears: the chip's own for what the tab reports ([`tabs::list_dot`]).
fn tone(indicator: Indicator) -> Tone {
    match tabs::list_dot(indicator) {
        tabs::Tone::Warning => Tone::Warning,
        tabs::Tone::Error => Tone::Error,
        tabs::Tone::Success => Tone::Success,
        _ => Tone::Accent,
    }
}

/// What a tab reports, as words at the right of its row: Asking, Running · 12s, Failed, Done.
pub fn status(indicator: Indicator, running: Option<Duration>) -> Status {
    let text = match indicator {
        Indicator::Question => "Asking".to_owned(),
        Indicator::Running => match running.and_then(bt_core::running_counter) {
            Some(time) => format!("Running · {time}"),
            None => "Running".to_owned(),
        },
        Indicator::Failed => "Failed".to_owned(),
        Indicator::Finished => "Done".to_owned(),
        Indicator::Uploading => "Uploading".to_owned(),
    };
    Status {
        text,
        tone: tone(indicator),
    }
}

fn window_note(other: Option<usize>) -> Option<String> {
    other.map(|number| format!("Window {number}"))
}

/// The pieces joined into a row's second line.
fn joined(parts: impl IntoIterator<Item = Option<String>>) -> String {
    parts
        .into_iter()
        .flatten()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" · ")
}

/// The row of a tab: its title, then the window (when it is another's), its group and where its
/// shell is; the ⌘ digit, what it reports, and the ✓ for the one on screen. → opens its menu.
pub fn tab_row(facts: &TabFacts) -> Row {
    let id = tab_id(facts.tab);
    let mut row = Row::new(Kind::Tab, &facts.tab.to_string(), facts.title.clone())
        .subtitle(joined([
            window_note(facts.other_window),
            facts.group.as_ref().map(|(_, name)| name.clone()),
            facts.place.clone(),
        ]))
        .checked(facts.here)
        .opens(Scope::TabActions(id), false);
    if let Some(key) = &facts.key {
        row = row.shortcut(key.clone());
    }
    row.status = facts
        .report
        .as_ref()
        .map(|report| status(report.indicator, report.running));
    row
}

/// The row of a group: its name, the ⌃⌘ digit, what in it asks for the user, and under the name
/// how many tabs it holds and where ⏎ goes ("on screen" for the group the window shows). → opens
/// its menu — the ungrouped tabs have none.
pub fn group_row(facts: &GroupFacts) -> Row {
    let key = format!("{}.{}", facts.window, facts.group.unwrap_or(0));
    let count = match facts.tabs {
        0 => "no tabs".to_owned(),
        1 => "1 tab".to_owned(),
        many => format!("{many} tabs"),
    };
    let inside = if facts.shown {
        "on screen".to_owned()
    } else {
        joined([
            Some(count),
            facts
                .back_to
                .as_ref()
                .map(|title| format!("back to {title}")),
        ])
    };
    let mut row = Row::new(Kind::Group, &key, facts.name.clone())
        .subtitle(joined([window_note(facts.other_window), Some(inside)]));
    if facts.group.is_some() {
        row = row.opens(
            Scope::GroupActions(group_id(facts.window, facts.group)),
            false,
        );
    }
    if let Some(shortcut) = &facts.key {
        row = row.shortcut(shortcut.clone());
    }
    row.status = facts.indicator.map(|indicator| status(indicator, None));
    row
}

/// The tabs' rows, in the order they are listed.
pub fn tab_rows(places: &Places) -> Vec<Row> {
    places.tabs.iter().map(tab_row).collect()
}

/// The groups' rows.
pub fn group_rows(places: &Places) -> Vec<Row> {
    places.groups.iter().map(group_row).collect()
}

// ─── The list ─────────────────────────────────────────────────────────────

fn unscored(row: Row) -> Hit {
    Hit {
        row,
        score: 0.0,
        marks: Vec::new(),
    }
}

/// The tabs page. An empty field: Recent (the last [`RECENT_TABS`] looked at, the one on screen
/// not among them), the groups, and Earlier (the rest, the one on screen last). A query: the tabs
/// and the groups it finds under their headings.
pub fn tab_list(places: &Places, query: &str) -> List {
    let tabs = tab_rows(places);
    let groups = group_rows(places);
    let mut list = List::default();
    if query.trim().is_empty() {
        let (here, others): (Vec<Row>, Vec<Row>) = tabs.into_iter().partition(|row| row.checked);
        let mut others = others;
        let rest = others.split_off(RECENT_TABS.min(others.len()));
        let earlier: Vec<Row> = rest.into_iter().chain(here).collect();
        for (heading, rows) in [
            (RECENT_HEADING, others),
            (GROUPS_HEADING, groups),
            (EARLIER_HEADING, earlier),
        ] {
            if rows.is_empty() {
                continue;
            }
            list.headings.push((list.hits.len(), heading.to_owned()));
            list.hits.extend(rows.into_iter().map(unscored));
        }
        return list;
    }
    let pool: Vec<Row> = tabs.into_iter().chain(groups).collect();
    for cluster in rank::tabs(&pool, query) {
        list.headings.push((list.hits.len(), cluster.title));
        list.hits.extend(cluster.hits);
    }
    list
}

// ─── The menu of a row ────────────────────────────────────────────────────

/// The rows of the page a tab's or a group's menu opens ([`Scope::TabActions`],
/// [`Scope::GroupActions`]); empty for any other scope or a row that is gone. They are the
/// right-click menus of the tab's chip and the group's header.
pub fn actions(scope: &Scope, places: &Places) -> Vec<Row> {
    let action = |name: &str, title: &str| Row::new(Kind::Action, name, title);
    match scope {
        Scope::TabActions(id) => {
            let Some(tab) = parse_tab(id).and_then(|tab| places.tabs.iter().find(|t| t.tab == tab))
            else {
                return Vec::new();
            };
            let mut rows = vec![action("renameTab", "Rename Tab…").keywords("title name label")];
            // The groups of the tab's own window it is not in already, then a new one.
            let own = tab.group.as_ref().map(|(group, _)| *group);
            for group in places
                .groups
                .iter()
                .filter(|group| group.window == tab.window && group.group.is_some())
                .filter(|group| group.group != own)
            {
                if let Some(at) = group.group {
                    rows.push(
                        action(&format!("moveTo.{at}"), &format!("Move to {}", group.name))
                            .keywords("group folder"),
                    );
                }
            }
            rows.push(action("moveToNewGroup", "Move to New Group…").keywords("group folder"));
            if own.is_some() {
                rows.push(action("removeFromGroup", "Remove from Group").keywords("ungroup"));
            }
            rows.push(
                action("moveToNewWindow", "Move Tab to New Window").keywords("detach pop out"),
            );
            rows.push(action("closeTab", "Close Tab").keywords("kill exit"));
            rows
        }
        Scope::GroupActions(id) => {
            let known = parse_group(id).is_some_and(|(window, group)| {
                group.is_some()
                    && places
                        .groups
                        .iter()
                        .any(|g| g.window == window && g.group == group)
            });
            if !known {
                return Vec::new();
            }
            vec![
                action("newTabInGroup", "New Tab in Group").keywords("open shell create"),
                action("renameGroup", "Rename Group…").keywords("title name label"),
                action("ungroup", "Ungroup").keywords("dissolve"),
                action("moveGroupToNewWindow", "Move Group to New Window")
                    .keywords("detach pop out"),
                action("closeGroup", "Close Group").keywords("kill exit"),
            ]
        }
        _ => Vec::new(),
    }
}

/// What stands over the rows of a menu's page: the name of the tab or group it is the menu of,
/// so a page of "Close Tab" and "Rename Tab…" says whose.
pub fn actions_heading(scope: &Scope, places: &Places) -> Option<String> {
    match scope {
        Scope::TabActions(id) => places
            .tabs
            .iter()
            .find(|tab| tab_id(tab.tab) == *id)
            .map(|tab| tab.title.clone()),
        Scope::GroupActions(id) => {
            let (window, group) = parse_group(id)?;
            places
                .groups
                .iter()
                .find(|g| g.window == window && g.group == group)
                .map(|g| g.name.clone())
        }
        _ => None,
    }
}

// ─── What choosing does ───────────────────────────────────────────────────

/// What choosing a tab or a group row, or one of the actions of its menu, comes to; the shell
/// does it once the palette is down.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Act {
    /// The tab comes on screen, its window forward.
    GoToTab(u64),
    /// The group (the ungrouped tabs for `None`) comes on screen, on the tab last visited in it.
    GoToGroup {
        window: u64,
        group: Option<u64>,
    },
    RenameTab(u64),
    MoveTabToGroup {
        tab: u64,
        group: u64,
    },
    MoveTabToNewGroup(u64),
    RemoveTabFromGroup(u64),
    MoveTabToNewWindow(u64),
    CloseTab(u64),
    NewTabInGroup {
        window: u64,
        group: u64,
    },
    RenameGroup {
        window: u64,
        group: u64,
    },
    Ungroup {
        window: u64,
        group: u64,
    },
    MoveGroupToNewWindow {
        window: u64,
        group: u64,
    },
    CloseGroup {
        window: u64,
        group: u64,
    },
}

/// What the row `id` chosen with `modifier` on the page `scope` (`None`: the root) comes to;
/// `None` for a row that is not a tab's or a group's, or a use the row does not have (⌘⏎ and ⌥⏎
/// are the command history's).
pub fn act(id: &str, scope: Option<&Scope>, modifier: Modifier) -> Option<Act> {
    if modifier != Modifier::None {
        return None;
    }
    if let Some(tab) = parse_tab(id) {
        return Some(Act::GoToTab(tab));
    }
    if let Some((window, group)) = parse_group(id) {
        return Some(Act::GoToGroup { window, group });
    }
    let name = id.strip_prefix(Kind::Action.prefix())?;
    match scope? {
        Scope::TabActions(target) => {
            let tab = parse_tab(target)?;
            Some(match name {
                "renameTab" => Act::RenameTab(tab),
                "moveToNewGroup" => Act::MoveTabToNewGroup(tab),
                "removeFromGroup" => Act::RemoveTabFromGroup(tab),
                "moveToNewWindow" => Act::MoveTabToNewWindow(tab),
                "closeTab" => Act::CloseTab(tab),
                other => Act::MoveTabToGroup {
                    tab,
                    group: other.strip_prefix("moveTo.")?.parse().ok()?,
                },
            })
        }
        Scope::GroupActions(target) => {
            let (window, group) = parse_group(target)?;
            let group = group?;
            Some(match name {
                "newTabInGroup" => Act::NewTabInGroup { window, group },
                "renameGroup" => Act::RenameGroup { window, group },
                "ungroup" => Act::Ungroup { window, group },
                "moveGroupToNewWindow" => Act::MoveGroupToNewWindow { window, group },
                "closeGroup" => Act::CloseGroup { window, group },
                _ => return None,
            })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::groups::Target;

    /// Select Group's modifiers, as the table has them.
    const CTRL_CMD: Mods = Mods::new(true, false, false, true);

    fn tab(id: u64, title: &str) -> TabFacts {
        TabFacts {
            tab: id,
            window: 1,
            title: title.to_owned(),
            report: None,
            place: Some("~/src".to_owned()),
            group: None,
            other_window: None,
            key: None,
            here: false,
        }
    }

    fn group(id: Option<u64>, name: &str) -> GroupFacts {
        GroupFacts {
            window: 1,
            group: id,
            name: name.to_owned(),
            key: None,
            indicator: None,
            tabs: 2,
            back_to: None,
            shown: false,
            other_window: None,
        }
    }

    fn ids(list: &List) -> Vec<&str> {
        list.hits.iter().map(|hit| hit.row.id.as_str()).collect()
    }

    fn places(count: u64) -> Places {
        let mut tabs: Vec<TabFacts> = (1..=count).map(|n| tab(n, &format!("tab {n}"))).collect();
        if let Some(last) = tabs.last_mut() {
            last.here = true;
        }
        Places {
            tabs,
            groups: Vec::new(),
        }
    }

    #[test]
    fn the_tab_on_screen_is_listed_last_and_the_rest_by_how_lately_they_were_looked_at() {
        let mut visits = Visits::new();
        for tab in [1, 4, 2, 3] {
            visits.visit(tab);
        }
        // Looked at last: 3 (on screen), 2, 4, 1; tab 5 never.
        let order = listing_order(&visits, &[1, 2, 3, 4, 5], Some(3));
        assert_eq!(order, vec![2, 4, 1, 5, 3]);
        // Never visited at all: the strip's order, the one on screen still last.
        let fresh = listing_order(&Visits::new(), &[1, 2, 3], Some(1));
        assert_eq!(fresh, vec![2, 3, 1]);
    }

    #[test]
    fn an_empty_field_lists_recent_then_the_groups_then_the_rest_with_the_tab_on_screen_last() {
        let mut places = places(9);
        places.groups = vec![group(None, "Ungrouped"), group(Some(1), "ops")];
        let list = tab_list(&places, "");
        let ids = ids(&list);
        // The first five are Recent: tab 1…5; the tab on screen (9) is not among them.
        assert_eq!(&ids[..5], ["tab.1", "tab.2", "tab.3", "tab.4", "tab.5"]);
        assert_eq!(list.heading_before(0), Some(RECENT_HEADING));
        assert_eq!(&ids[5..7], ["grp.1.0", "grp.1.1"]);
        assert_eq!(list.heading_before(5), Some(GROUPS_HEADING));
        assert_eq!(&ids[7..], ["tab.6", "tab.7", "tab.8", "tab.9"]);
        assert_eq!(list.heading_before(7), Some(EARLIER_HEADING));
        assert!(list.hits.last().is_some_and(|hit| hit.row.checked));
    }

    #[test]
    fn with_few_tabs_the_one_on_screen_is_the_whole_of_earlier_and_nothing_is_empty() {
        let list = tab_list(&places(3), "");
        assert_eq!(ids(&list), ["tab.1", "tab.2", "tab.3"]);
        assert_eq!(list.heading_before(0), Some(RECENT_HEADING));
        assert_eq!(list.heading_before(2), Some(EARLIER_HEADING));
        // No groups: no Groups heading.
        assert!(list.headings.iter().all(|(_, name)| name != GROUPS_HEADING));
        assert!(tab_list(&Places::default(), "").is_empty());
    }

    #[test]
    fn a_query_finds_tabs_and_groups_and_the_nearer_tab_wins_a_tie() {
        let mut places = places(3);
        places.tabs[0].title = "api server".to_owned();
        places.tabs[1].title = "api worker".to_owned();
        places.groups = vec![group(Some(1), "api")];
        let list = tab_list(&places, "api");
        let ids = ids(&list);
        // The listing order is the nearness: tab 1 was looked at more lately than tab 2.
        assert_eq!(ids.iter().position(|id| *id == "tab.1"), Some(0));
        assert!(ids.contains(&"tab.2") && ids.contains(&"grp.1.1"));
        assert!(
            list.headings
                .iter()
                .any(|(_, name)| name == "Tabs" || name == "Groups")
        );
        // The group's name also finds its tabs, through their second line.
        let mut grouped = places.clone();
        grouped.tabs[2].group = Some((1, "ops".to_owned()));
        assert!(ids_of(&tab_list(&grouped, "ops")).contains(&"tab.3".to_owned()));
    }

    fn ids_of(list: &List) -> Vec<String> {
        list.hits.iter().map(|hit| hit.row.id.clone()).collect()
    }

    #[test]
    fn the_tabs_page_shows_every_tab_a_query_finds_where_a_root_search_cuts_to_six() {
        let mut places = places(10);
        for facts in &mut places.tabs {
            facts.title = format!("server {}", facts.tab);
        }
        let page = tab_list(&places, "server");
        assert_eq!(page.len(), 10, "all of them on the page of the tabs");
        // The root's search of the same rows keeps to a cluster's six.
        let root = rank::root(&tab_rows(&places), "server", &[]);
        let tabs = root.iter().find(|cluster| cluster.title == "Tabs");
        assert_eq!(tabs.map(|cluster| cluster.hits.len()), Some(6));
    }

    #[test]
    fn a_tab_row_says_its_window_group_and_place_and_what_it_reports() {
        let mut facts = tab(7, "build");
        facts.other_window = Some(2);
        facts.group = Some((3, "ops".to_owned()));
        facts.report = Some(Report {
            indicator: Indicator::Running,
            running: Some(Duration::from_secs(75)),
        });
        let row = tab_row(&facts);
        assert_eq!(row.id, "tab.7");
        assert_eq!(row.subtitle, "Window 2 · ops · ~/src");
        let status = row.status.expect("a status");
        assert!(status.text.starts_with("Running · "), "{}", status.text);
        assert_eq!(status.tone, Tone::Accent);
        assert_eq!(row.opens, Some(Scope::TabActions("tab.7".to_owned())));
        assert!(!row.enters, "⏎ goes to the tab; → opens the menu");
        for (indicator, text, tone) in [
            (Indicator::Question, "Asking", Tone::Warning),
            (Indicator::Failed, "Failed", Tone::Error),
            (Indicator::Finished, "Done", Tone::Success),
        ] {
            let status = status_of(indicator);
            assert_eq!((status.text.as_str(), status.tone), (text, tone));
        }
    }

    fn status_of(indicator: Indicator) -> Status {
        status(indicator, None)
    }

    #[test]
    fn a_group_row_says_how_many_tabs_and_where_it_goes_or_that_it_is_on_screen() {
        let mut facts = group(Some(2), "ops");
        facts.tabs = 3;
        facts.back_to = Some("deploy".to_owned());
        facts.key = group_key(Some(1), 3, Some(CTRL_CMD));
        facts.indicator = Some(Indicator::Question);
        let row = group_row(&facts);
        assert_eq!(row.id, "grp.1.2");
        assert_eq!(row.subtitle, "3 tabs · back to deploy");
        assert_eq!(row.shortcut.as_deref(), Some("⌃⌘2"));
        assert_eq!(row.status.map(|s| s.text), Some("Asking".to_owned()));
        facts.shown = true;
        facts.other_window = Some(2);
        assert_eq!(group_row(&facts).subtitle, "Window 2 · on screen");
        facts.tabs = 0;
        facts.shown = false;
        assert_eq!(
            group_row(&facts).subtitle,
            "Window 2 · no tabs · back to deploy"
        );
        // The ungrouped tabs are a row but have no menu.
        let loose = group_row(&group(None, "Ungrouped"));
        assert_eq!((loose.id.as_str(), loose.opens), ("grp.1.0", None));
        assert_eq!(group_key(None, 3, Some(CTRL_CMD)).as_deref(), Some("⌃⌘0"));
        // The file moves the modifiers and the hint follows; with none, no group has a hint.
        let option = Mods::NONE.with(bt_core::Modifier::Option);
        assert_eq!(group_key(Some(0), 3, Some(option)).as_deref(), Some("⌥1"));
        assert_eq!(group_key(Some(0), 3, None), None);
    }

    #[test]
    fn the_digit_a_tab_shows_is_the_one_the_bar_gives_it() {
        // Twelve tabs in a strip: ⌘1…⌘8 the first eight, ⌘9 the last, none between.
        let mut order = Tabs::new(1u32);
        for id in 2..=12 {
            order.append(id);
        }
        let groups = Groups::new();
        let keys = tab_keys(&groups, &order, View::Top, Some(Mods::CMD));
        assert_eq!(keys.len(), 9);
        assert_eq!(keys[0], (1, "⌘1".to_owned()));
        assert_eq!(keys[7], (8, "⌘8".to_owned()));
        assert_eq!(keys[8], (12, "⌘9".to_owned()));
        // The column counts the same strip.
        assert_eq!(tab_keys(&groups, &order, View::Side, Some(Mods::CMD)), keys);
    }

    #[test]
    fn a_closed_groups_hidden_tabs_have_no_digit_and_the_bar_numbers_the_cluster_on_screen() {
        // Tabs 1…6: a = 1, b = 2 3, c = 4 5, and 6 among the ungrouped, on screen.
        let mut order = Tabs::new(1u32);
        for id in 2..=6 {
            order.append(id);
        }
        let mut groups = Groups::new();
        for (name, members) in [("a", [1].as_slice()), ("b", &[2, 3]), ("c", &[4, 5])] {
            groups.place(&mut order, members, Target::New(name.into()));
        }
        groups.select(&mut order, 6);
        let closed = groups.groups()[1].id;
        groups.set_collapsed(closed, true);
        // The column: 6, then a's 1, b is closed and 6 is not its tab, then c's 4 and 5.
        let column = tab_keys(&groups, &order, View::Side, Some(Mods::CMD));
        assert_eq!(
            column,
            [
                (6, "⌘1".to_owned()),
                (1, "⌘2".to_owned()),
                (4, "⌘3".to_owned()),
                (5, "⌘4".to_owned())
            ]
        );
        // The bar: the ungrouped cluster on screen, counted from 1.
        assert_eq!(
            tab_keys(&groups, &order, View::Top, Some(Mods::CMD)),
            [(6, "⌘1".to_owned())]
        );
        groups.select(&mut order, 4);
        assert_eq!(
            tab_keys(&groups, &order, View::Top, Some(Mods::CMD)),
            [(4, "⌘1".to_owned()), (5, "⌘2".to_owned())]
        );
    }

    #[test]
    fn a_tabs_menu_offers_the_other_groups_of_its_window_and_removal_only_from_its_own() {
        let mut places = places(2);
        places.tabs[0].group = Some((1, "ops".to_owned()));
        places.groups = vec![
            group(Some(1), "ops"),
            group(Some(2), "dev"),
            GroupFacts {
                window: 9,
                ..group(Some(1), "elsewhere")
            },
        ];
        let rows = actions(&Scope::TabActions("tab.1".to_owned()), &places);
        let titles: Vec<&str> = rows.iter().map(|row| row.title.as_str()).collect();
        assert_eq!(
            titles,
            [
                "Rename Tab…",
                "Move to dev",
                "Move to New Group…",
                "Remove from Group",
                "Move Tab to New Window",
                "Close Tab",
            ]
        );
        // The second tab is in none: no removal, and both groups are offered.
        let loose = actions(&Scope::TabActions("tab.2".to_owned()), &places);
        let titles: Vec<&str> = loose.iter().map(|row| row.title.as_str()).collect();
        assert!(titles.contains(&"Move to ops") && titles.contains(&"Move to dev"));
        assert!(!titles.contains(&"Remove from Group"));
        // A tab that is gone has no menu.
        assert!(actions(&Scope::TabActions("tab.99".to_owned()), &places).is_empty());
    }

    #[test]
    fn a_groups_menu_is_the_header_menu_and_the_ungrouped_tabs_have_none() {
        let mut places = places(1);
        places.groups = vec![group(None, "Ungrouped"), group(Some(1), "ops")];
        let rows = actions(&Scope::GroupActions("grp.1.1".to_owned()), &places);
        let titles: Vec<&str> = rows.iter().map(|row| row.title.as_str()).collect();
        assert_eq!(
            titles,
            [
                "New Tab in Group",
                "Rename Group…",
                "Ungroup",
                "Move Group to New Window",
                "Close Group",
            ]
        );
        assert!(actions(&Scope::GroupActions("grp.1.0".to_owned()), &places).is_empty());
    }

    #[test]
    fn a_menus_page_is_headed_by_whose_menu_it_is() {
        let mut places = places(2);
        places.groups = vec![group(Some(4), "ops")];
        assert_eq!(
            actions_heading(&Scope::TabActions("tab.2".to_owned()), &places),
            Some("tab 2".to_owned())
        );
        assert_eq!(
            actions_heading(&Scope::GroupActions("grp.1.4".to_owned()), &places),
            Some("ops".to_owned())
        );
        assert_eq!(actions_heading(&Scope::Tabs, &places), None);
        assert_eq!(
            actions_heading(&Scope::TabActions("tab.99".to_owned()), &places),
            None
        );
    }

    #[test]
    fn choosing_a_row_or_an_action_says_what_to_do() {
        let none = Modifier::None;
        assert_eq!(act("tab.7", None, none), Some(Act::GoToTab(7)));
        assert_eq!(
            act("grp.2.5", Some(&Scope::Tabs), none),
            Some(Act::GoToGroup {
                window: 2,
                group: Some(5)
            })
        );
        assert_eq!(
            act("grp.2.0", None, none),
            Some(Act::GoToGroup {
                window: 2,
                group: None
            })
        );
        let tab = Scope::TabActions("tab.7".to_owned());
        assert_eq!(
            act("act.closeTab", Some(&tab), none),
            Some(Act::CloseTab(7))
        );
        assert_eq!(
            act("act.moveTo.3", Some(&tab), none),
            Some(Act::MoveTabToGroup { tab: 7, group: 3 })
        );
        let group = Scope::GroupActions("grp.2.5".to_owned());
        assert_eq!(
            act("act.moveGroupToNewWindow", Some(&group), none),
            Some(Act::MoveGroupToNewWindow {
                window: 2,
                group: 5
            })
        );
        // An action off its page, an unknown one, ⌘⏎ and ⌥⏎: nothing.
        assert_eq!(act("act.closeTab", None, none), None);
        assert_eq!(act("act.nonsense", Some(&tab), none), None);
        assert_eq!(act("tab.7", None, Modifier::Command), None);
        assert_eq!(act("cmd.newTab", None, none), None);
    }
}
