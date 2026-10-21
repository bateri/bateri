//! What a server keeps for the user, as ⌘P lists it: the terminals that run on it and no pane of
//! this Mac shows ("running in the background"), the panes of this Mac that are on it, and the
//! page of the server (→ on its row) that gathers them.
//!
//! **Found by searching.** A terminal in the background is a row like a tab: typing `claude` or
//! `rasp` finds it under *Running in the background*, ⏎ opens it in a new tab, ⌥⏎ beside the pane
//! on screen ([`super::targets::beside`], the same rule a server's own ⌥⏎ has). The empty root does
//! not list them — every server's terminals would fill it; a query is the way to them.
//!
//! **The server's page** ([`server_list`]), under the row of a server that kablo keeps terminals
//! on (a plain `ssh` has none of this, and its row has no page and no count): *Open All in New
//! Tab* when something runs in the background; *Running in the background*, the last to write
//! first; *Tabs*, every pane of this Mac on the server on a row of its own, with its tab and the
//! tab's ⌘ digit at the right (⏎ brings the tab and the pane forward, nothing moves); *On other
//! Macs*, the terminals another Mac has open, each with that Mac's name at the right (⏎ moves it
//! here: the pane there goes dim); and, last, *New Tab on rasp*, which is what ⏎ on the server's
//! row does. A heading is a noun, never a verb. The terminals on other Macs are on this page only:
//! a search from the root does not find them, so ⏎ on a row found by chance never takes a terminal
//! from the Mac that is using it.
//!
//! **Identity.** A terminal in the background is `bg.<terminal>@<account>` (a terminal's name has
//! no `@`, the account's key has one in it), one on another Mac `mv.<terminal>@<account>`, a pane
//! `pane.<tab>.<pane>`, Open All `act.openAll.<server>`; the new tab of the page is the server's
//! own `host.<name>`.

use std::time::Duration;

use bt_core::{bare_host, running_counter};

use crate::kablo::roster::{Activity, Background};

use super::list::List;
use super::rank::{Hit, Nearness, search};
use super::row::{Kind, Row, Tone};
use super::state::Modifier;
use super::targets::{Go, Targets};

/// The heading over the terminals in the background on a server's page: the cluster they are
/// found under in a search.
pub const BACKGROUND_HEADING: &str = "Running in the background";
/// The heading over the panes of this Mac on a server's page.
pub const PANES_HEADING: &str = "Tabs";
/// The heading over the terminals another Mac has open on a server's page.
pub const ELSEWHERE_HEADING: &str = crate::server_card::OTHERS_HEADING;
/// The identity prefix of Open All: `act.openAll.<server>`.
const OPEN_ALL: &str = "act.openAll.";

/// A terminal a server keeps for the user that no pane of this Mac shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackgroundFacts {
    /// The server's account (`user@host:port`): with the terminal's name, what it is called.
    pub account: String,
    /// The server as the user typed it.
    pub host: String,
    /// The terminal's name on the server.
    pub terminal: String,
    /// What the row is called: the terminal's label, else the program in front.
    pub title: String,
    /// The directory of the program in front, on the server.
    pub place: Option<String>,
    /// For how long a program other than the shell has been in front.
    pub running: Option<Duration>,
    /// The Macs that have the terminal open, by the names they gave themselves, joined with
    /// `, `; empty for a terminal nobody has open.
    pub by: String,
}

impl BackgroundFacts {
    /// The facts of a terminal in the background at `now` (seconds since the Unix epoch).
    pub fn of(background: &Background, now: u64) -> BackgroundFacts {
        BackgroundFacts {
            account: background.account.clone(),
            host: background.host.clone(),
            terminal: background.terminal.as_str().to_owned(),
            title: background
                .label
                .clone()
                .or_else(|| background.program.clone())
                .unwrap_or_else(|| "terminal".to_owned()),
            place: background.cwd.clone(),
            running: match background.activity(now) {
                Activity::Running(time) => time,
                _ => None,
            },
            by: background.elsewhere().join(", "),
        }
    }
}

/// A pane of this Mac that shows a terminal of a server.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServerPane {
    pub tab: u64,
    pub pane: u64,
    /// What the pane is called: its directory on the server, or its program.
    pub title: String,
    /// The tab it is in.
    pub tab_title: String,
    /// The ⌘ digit that reaches the tab ([`super::tab_keys`]).
    pub key: Option<String>,
}

/// What the palette is told of one server kablo keeps terminals on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServerFacts {
    /// The server as [`super::HostFacts::name`] has it.
    pub name: String,
    /// The terminals in the background, the last to write first.
    pub background: Vec<BackgroundFacts>,
    /// The terminals another Mac has open, the last to write first.
    pub elsewhere: Vec<BackgroundFacts>,
    /// The panes of this Mac on the server, in the order the tabs are listed.
    pub panes: Vec<ServerPane>,
}

/// The server `name` is, among those kablo keeps terminals on; `deploy@db-1` and `db-1` are one.
pub fn server_of<'a>(targets: &'a Targets, name: &str) -> Option<&'a ServerFacts> {
    targets
        .servers
        .iter()
        .find(|server| bare_host(&server.name) == bare_host(name))
}

fn joined(parts: impl IntoIterator<Item = Option<String>>) -> String {
    parts
        .into_iter()
        .flatten()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" · ")
}

/// The key of a terminal's row: the terminal, an `@`, and the account.
pub fn background_key(account: &str, terminal: &str) -> String {
    format!("{terminal}@{account}")
}

/// The row of a terminal in the background: what it is, the server and the directory under it,
/// and how long a program has been in front. ⏎ opens it in a new tab, ⌥⏎ beside the pane on screen.
pub fn background_row(facts: &BackgroundFacts) -> Row {
    let mut row = Row::new(
        Kind::Background,
        &background_key(&facts.account, &facts.terminal),
        facts.title.clone(),
    )
    .keywords("terminal session server remote kablo")
    .subtitle(joined([Some(facts.host.clone()), facts.place.clone()]))
    .hint("↵", crate::server_card::OPEN)
    .hint("⌥↵", "Open Beside");
    if let Some(time) = facts.running.and_then(running_counter) {
        row = row.status(format!("Running · {time}"), Tone::Dim);
    }
    row
}

/// The row of a terminal another Mac has open: the same, with that Mac's name at the right. ⏎
/// moves it here; nothing else is done to it from here (⌥⏎ is not a use of this row).
pub fn elsewhere_row(facts: &BackgroundFacts) -> Row {
    Row::new(
        Kind::Elsewhere,
        &background_key(&facts.account, &facts.terminal),
        facts.title.clone(),
    )
    .keywords(format!(
        "terminal session server remote kablo mac computer {}",
        facts.by
    ))
    .subtitle(joined([Some(facts.host.clone()), facts.place.clone()]))
    .value(facts.by.clone())
    .hint("↵", crate::server_card::move_hint(&facts.by))
}

/// The rows of every terminal in the background, for a search at the root.
pub fn background_rows(targets: &Targets) -> Vec<Row> {
    targets
        .servers
        .iter()
        .flat_map(|server| server.background.iter())
        .map(background_row)
        .collect()
}

/// The row of a pane of this Mac on the server: its directory, its tab at the right with the tab's
/// ⌘ digit. ⏎ goes to the tab and the pane.
pub fn pane_row(pane: &ServerPane, host: &str) -> Row {
    let mut row = Row::new(
        Kind::Pane,
        &format!("{}.{}", pane.tab, pane.pane),
        pane.title.clone(),
    )
    .keywords(format!("{host} {}", pane.tab_title))
    .value(pane.tab_title.clone())
    .hint("↵", "Go to");
    if let Some(key) = &pane.key {
        row = row.shortcut(key.clone());
    }
    row
}

/// The row of Open All: every terminal in the background, together, in a new tab.
pub fn open_all_row(host: &str, count: usize) -> Row {
    Row::new(
        Kind::Action,
        &format!("openAll.{host}"),
        crate::server_card::OPEN_ALL,
    )
    .keywords("open all background terminals")
    .subtitle(if count == 1 {
        "1 terminal".to_owned()
    } else {
        format!("{count} terminals")
    })
    .hint("↵", "Open All")
}

/// The sections of the page of the server `host`, each with the heading that stands over its rows
/// (`None`: none). The page of a server kablo keeps nothing on, or that the palette was not told
/// of, has only the new tab.
fn sections(host: &str, targets: &Targets) -> Vec<(Option<String>, Vec<Row>)> {
    let mut sections = Vec::new();
    if let Some(server) = server_of(targets, host) {
        if !server.background.is_empty() {
            sections.push((
                None,
                vec![open_all_row(&server.name, server.background.len())],
            ));
            sections.push((
                Some(BACKGROUND_HEADING.to_owned()),
                server.background.iter().map(background_row).collect(),
            ));
        }
        if !server.panes.is_empty() {
            sections.push((
                Some(PANES_HEADING.to_owned()),
                server
                    .panes
                    .iter()
                    .map(|pane| pane_row(pane, &server.name))
                    .collect(),
            ));
        }
        if !server.elsewhere.is_empty() {
            sections.push((
                Some(ELSEWHERE_HEADING.to_owned()),
                server.elsewhere.iter().map(elsewhere_row).collect(),
            ));
        }
    }
    // What ⏎ on the server's own row does: its id is the server's, so choosing it is that.
    sections.push((
        Some(host.to_owned()),
        vec![
            Row::new(Kind::Host, host, format!("New Tab on {host}"))
                .keywords("ssh connect terminal"),
        ],
    ));
    sections
}

/// The rows of the page of the server `host`, in the order [`server_list`] lists them.
pub fn server_rows(host: &str, targets: &Targets) -> Vec<Row> {
    sections(host, targets)
        .into_iter()
        .flat_map(|(_, rows)| rows)
        .collect()
}

/// The page of the server `host`: its sections under their headings; a query keeps the rows it
/// finds in each, best first, and a section it finds nothing in is left out.
pub fn server_list(host: &str, targets: &Targets, query: &str) -> List {
    let mut list = List::default();
    for (heading, rows) in sections(host, targets) {
        let hits: Vec<Hit> = if query.trim().is_empty() {
            rows.into_iter()
                .map(|row| Hit {
                    row,
                    score: 0.0,
                    marks: Vec::new(),
                })
                .collect()
        } else {
            search(&rows, query, Nearness::None)
        };
        if hits.is_empty() {
            continue;
        }
        if let Some(heading) = heading {
            list.headings.push((list.hits.len(), heading));
        }
        list.hits.extend(hits);
    }
    list
}

/// What choosing the row `id` with `modifier` comes to, for the rows of this module; `None` for
/// any other row or a use the row does not have.
pub(super) fn go(id: &str, modifier: Modifier) -> Option<Go> {
    if let Some(key) = id.strip_prefix(Kind::Background.prefix()) {
        let (terminal, account) = key.split_once('@')?;
        let (terminal, account) = (terminal.to_owned(), account.to_owned());
        return match modifier {
            Modifier::None => Some(Go::OpenBackground { account, terminal }),
            Modifier::Option => Some(Go::SplitBackground { account, terminal }),
            Modifier::Command => None,
        };
    }
    if let Some(key) = id.strip_prefix(Kind::Elsewhere.prefix()) {
        let (terminal, account) = key.split_once('@')?;
        let (terminal, account) = (terminal.to_owned(), account.to_owned());
        return (modifier == Modifier::None).then_some(Go::MoveHere { account, terminal });
    }
    if let Some(host) = id.strip_prefix(OPEN_ALL) {
        return (modifier == Modifier::None).then(|| Go::OpenAllBackground(host.to_owned()));
    }
    if let Some(key) = id.strip_prefix(Kind::Pane.prefix()) {
        let (tab, pane) = key.split_once('.')?;
        let (tab, pane) = (tab.parse().ok()?, pane.parse().ok()?);
        return (modifier == Modifier::None).then_some(Go::FocusPane { tab, pane });
    }
    None
}

#[cfg(test)]
mod tests;
