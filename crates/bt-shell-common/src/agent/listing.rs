//! `bateri ls`: what is open, as an agent on a server reads it.
//!
//! Two parts, in the order a person looks for them. First **where things are**: the windows
//! front to back, in each window its groups and tabs in the order of the strip, and in each tab
//! the split tree itself — `side by side` or `stacked`, the first and second share, the panes by
//! their ids and their labels and nothing else of them. Every line says what it is by a word and
//! gives its fields as `field:value`:
//!
//! ```text
//! window id:w1
//!   group id:g1 label:"api"
//!     tab id:t3 label:"web" title:"npm" (selected)
//!       pane id:p9 label:"server" (on screen)
//! ```
//!
//! `id:` is what a command is given (`w1`, `g1`, `t3`, `p9`), `label:` the name a person or an
//! agent gave the thing and `title:` what the program in it calls itself. A terminal that is no
//! agent's pane (`t9` before the ids, one `kablo new` named) has no id, so it says
//! `name:"t9"` instead: it cannot be written to a command.
//!
//! Then **what they are**: one table row per terminal the server has (`ID TAB LABEL PROGRAM STATE
//! AGE QUIET SIZE OPENED BY DIRECTORY`), every column of it from the server's own statement
//! ([`kablo_proto::v1::Status`]) so that it reads the same with the bateri app away; `TAB` is the id of
//! its tab. A terminal in no tab says `none` there (it is in the list of what runs in the
//! background) and one whose pane is waiting for the bateri app says `waiting`.
//!
//! **What an agent is not told.** A pane that is not a terminal of this server — a local shell,
//! another server's terminal — is `another machine` and nothing more: no id, no label, no title,
//! no directory, no program. A split whose both sides are other machines is one such node, and a
//! tab, a group or a window made of nothing else is one line, `tab another machine`, without an
//! id (what is not shown is not named). A tab has a label here only when the user or an agent
//! gave it one, and a title only when it comes from a terminal of this server (the one that builds
//! a [`Tab`] decides; this prints what it is given).
//!
//! **Where the words come from.** What a terminal is doing is one reading of the server's times
//! ([`crate::kablo::roster::activity_at`], the card's own), `idle` only for a shell waiting for
//! a command (a program started in place of the shell is `running` however quiet), a time is
//! [`bt_core::running_counter`]'s text and `running 3m 02s` is the card's
//! ([`crate::server_card::running_text`]). The shortcut of a tab is handed in by whoever builds
//! the scene ([`crate::palette::tab_keys`]).
//!
//! The same scene is also JSON ([`Scene::json`]), written by hand — no dependency for a dozen
//! keys — with the same fields (`id`, `label`, `title`; ids are always strings) and kablo's own
//! names for what kablo calls a thing (`list --json`: `state`, `program`, `directory`, `cols`,
//! `rows`, `exit`, `secret`, `label`, `started`, `front_started`, `front_is_own`, `output_at`) and
//! the rule for strings. Keys are added, never taken away; version 2 of the message is the one
//! break (`terminal` became `id`, the numbers of a window and a tab became ids).

use std::fmt::Write as _;
use std::time::Duration;

use bt_core::running_counter;
use kablo_proto::v1::{Exit, State, Status};

use super::Terminal;
use super::notes::Record;
use crate::ids::Kind as IdKind;
use crate::kablo::roster::{Activity, activity_at};
use crate::program::is_shell_name;
use crate::server_card::running_text;
use crate::split::{Axis, Tree};

/// What a pane in a tab shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pane {
    /// A terminal of this server, by its name there.
    Terminal(String),
    /// A terminal of another machine, told by that machine's name and, when it is an agent's
    /// pane, its number there (`p9@prod`). Only a level that sees every machine builds these
    /// ([`super::level`]); the others get [`Pane::Elsewhere`].
    Remote {
        machine: String,
        name: Option<String>,
    },
    /// Anything else: nothing of it is told.
    Elsewhere,
}

/// A tab's split tree, as `ls` draws it.
#[derive(Clone, Debug, PartialEq)]
pub enum Node {
    Pane(Pane),
    /// Two sides along `axis`; `ratio` is the first one's share, the tree's own.
    Split {
        axis: Axis,
        ratio: f64,
        first: Box<Node>,
        second: Box<Node>,
    },
}

impl Node {
    /// The tree of a tab with each pane told by `pane`: the real tree, its shares as they are,
    /// with a side that is other machines throughout folded into one node.
    pub fn of(tree: &Tree, pane: &dyn Fn(u64) -> Pane) -> Node {
        match tree {
            Tree::Leaf(id) => Node::Pane(pane(*id)),
            Tree::Split {
                axis,
                ratio,
                first,
                second,
            } => Node::split(*axis, *ratio, Node::of(first, pane), Node::of(second, pane)),
        }
    }

    fn split(axis: Axis, ratio: f64, first: Node, second: Node) -> Node {
        let elsewhere = Node::Pane(Pane::Elsewhere);
        if first == elsewhere && second == elsewhere {
            return elsewhere;
        }
        Node::Split {
            axis,
            ratio,
            first: Box::new(first),
            second: Box::new(second),
        }
    }

    /// The same tree with every side that is other machines throughout folded into one node:
    /// what is drawn, whoever built the tree.
    fn folded(&self) -> Node {
        match self {
            Node::Pane(pane) => Node::Pane(pane.clone()),
            Node::Split {
                axis,
                ratio,
                first,
                second,
            } => Node::split(*axis, *ratio, first.folded(), second.folded()),
        }
    }

    fn terminals<'a>(&'a self, into: &mut Vec<&'a str>) {
        match self {
            Node::Pane(Pane::Terminal(name)) => into.push(name),
            Node::Pane(Pane::Remote { .. } | Pane::Elsewhere) => {}
            Node::Split { first, second, .. } => {
                first.terminals(into);
                second.terminals(into);
            }
        }
    }
}

/// A tab.
#[derive(Clone, Debug, PartialEq)]
pub struct Tab {
    /// Its number (`t3`, [`crate::ids`]): the same wherever the tab goes.
    pub id: u64,
    /// The number of the group it is in, one of its window's [`Window::groups`].
    pub group: Option<u64>,
    /// The ⌘ shortcut that selects it, when it has one ([`crate::palette::tab_keys`]).
    pub keys: Option<String>,
    /// The name the user or an agent gave it.
    pub label: Option<String>,
    /// The title of the terminal of this server it shows, when it does: what its program calls
    /// itself.
    pub title: Option<String>,
    pub selected: bool,
    /// The pane filling the tab, when one is zoomed.
    pub zoomed: Option<Pane>,
    pub tree: Node,
}

impl Tab {
    /// Nothing in it is a terminal of this server.
    fn elsewhere(&self) -> bool {
        self.tree.folded() == Node::Pane(Pane::Elsewhere)
    }
}

/// A group of tabs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Group {
    /// Its number (`g1`).
    pub id: u64,
    pub label: String,
}

/// A window. Its place in [`Scene::windows`] is how far forward it is, front first.
#[derive(Clone, Debug, PartialEq)]
pub struct Window {
    /// Its number (`w1`).
    pub id: u64,
    /// It can be seen: not minimized, not on another space.
    pub visible: bool,
    /// The groups of its strip, which [`Tab::group`] names.
    pub groups: Vec<Group>,
    /// In the order of the tab strip.
    pub tabs: Vec<Tab>,
}

/// A terminal of the server.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    /// What the server says of it.
    pub status: Status,
    /// The terminal that opened it, when one did.
    pub opened_by: Option<String>,
    /// Its pane waits for the bateri app: it was opened while the bateri app was away.
    pub waiting: bool,
}

/// What `ls` is asked about.
#[derive(Clone, Debug, PartialEq)]
pub struct Scene {
    /// The terminal that asked.
    pub caller: Option<String>,
    /// The terminal the user is typing in, when it is one of this server's.
    pub typing: Option<String>,
    /// The windows, or `None` when the bateri app is not connected and nothing of the layout is known.
    pub windows: Option<Vec<Window>>,
    pub terminals: Vec<Row>,
}

/// The table's rows: each terminal the server said it has, with the terminal that opened it as
/// the server's records say (a terminal with no record, or opened from none, has no opener) and
/// whether its pane waits for the bateri app (`waits`).
pub fn rows(
    terminals: Vec<Status>,
    records: &[(Terminal, Record)],
    waits: &dyn Fn(Terminal) -> bool,
) -> Vec<Row> {
    terminals
        .into_iter()
        .map(|status| {
            let terminal = Terminal::parse(status.id.as_str());
            let opened_by = terminal
                .and_then(|terminal| records.iter().find(|(found, _)| *found == terminal))
                .and_then(|(_, record)| record.opener.as_ref())
                .map(|opener| opener.name().to_owned());
            Row {
                waiting: terminal.is_some_and(waits),
                opened_by,
                status,
            }
        })
        .collect()
}

const ELSEWHERE: &str = "another machine";
/// Longer than a person's time is; keeps a clock that is off from asking the counter for a year
/// it has no room for.
const LONGEST_SECS: u64 = 1_000_000_000;

fn elapsed(now: u64, then: u64) -> Duration {
    Duration::from_secs(now.saturating_sub(then).min(LONGEST_SECS))
}

/// Text from the server for a person to read: lossy, a control character shown as `?` (kablo's
/// own rule) so that nothing in it moves the reader's terminal.
fn printable(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { '?' } else { c })
        .collect()
}

fn lossy(bytes: &[u8]) -> String {
    printable(&String::from_utf8_lossy(bytes))
}

/// What a terminal is doing, by the words `ls` tells it in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Doing {
    Idle,
    Running(Option<Duration>),
    Password,
    Exited(Option<Exit>),
    Broken,
    Unknown,
}

impl Doing {
    fn of(status: &Status, now: u64) -> Doing {
        match status.state {
            State::ENDED => Doing::Exited(status.exit),
            State::BROKEN => Doing::Broken,
            State::RUNNING if status.secret => Doing::Password,
            State::RUNNING => {
                let shell = status
                    .program
                    .as_deref()
                    .is_some_and(|name| is_shell_name(&String::from_utf8_lossy(name)));
                match activity_at(
                    status.front_is_own,
                    status.front_started,
                    status.started,
                    status.output_at,
                    now,
                ) {
                    Activity::Idle { .. } if shell => Doing::Idle,
                    // The terminal's own program is in front, and it is not a shell: it is not
                    // waiting for a command, it is the command.
                    Activity::Idle { .. } => Doing::Running(
                        status
                            .front_started
                            .or(status.started)
                            .map(|since| elapsed(now, since)),
                    ),
                    Activity::Running(time) => Doing::Running(time),
                    Activity::Unknown => Doing::Running(None),
                }
            }
            State(_) => Doing::Unknown,
        }
    }

    fn text(self) -> String {
        match self {
            Doing::Idle => "idle".to_owned(),
            Doing::Running(time) => running_text(time),
            Doing::Password => "password prompt".to_owned(),
            Doing::Exited(Some(Exit::Code(code))) => format!("exited {code}"),
            Doing::Exited(Some(Exit::Signal(signal))) => format!("signal {signal}"),
            Doing::Exited(None) => "exited".to_owned(),
            Doing::Broken => "broken".to_owned(),
            Doing::Unknown => "unknown".to_owned(),
        }
    }

    /// The word JSON carries.
    fn word(self) -> &'static str {
        match self {
            Doing::Idle => "idle",
            Doing::Running(_) => "running",
            Doing::Password => "password_prompt",
            Doing::Exited(_) => "exited",
            Doing::Broken => "broken",
            Doing::Unknown => "unknown",
        }
    }
}

/// kablo's word for a state.
fn state_name(state: State) -> String {
    match state {
        State::RUNNING => "running".to_owned(),
        State::ENDED => "ended".to_owned(),
        State::BROKEN => "broken".to_owned(),
        State(other) => format!("unknown({other})"),
    }
}

/// How long ago, in the counter's words; below its first second it is `0s`.
fn since(now: u64, then: Option<u64>) -> String {
    then.map(|then| running_counter(elapsed(now, then)).unwrap_or_else(|| "0s".to_owned()))
        .unwrap_or_else(|| "-".to_owned())
}

fn or_dash(text: Option<String>) -> String {
    text.filter(|text| !text.is_empty())
        .unwrap_or_else(|| "-".to_owned())
}

/// The place an id sorts at: the agent's numbers in order, then any other name.
fn order(id: &str) -> (bool, u64, &str) {
    match Terminal::number_of(id) {
        Some(number) => (false, number, ""),
        None => (true, 0, id),
    }
}

const COLUMNS: [&str; 10] = [
    "ID",
    "TAB",
    "LABEL",
    "PROGRAM",
    "STATE",
    "AGE",
    "QUIET",
    "SIZE",
    "OPENED BY",
    "DIRECTORY",
];

impl Scene {
    /// Where a terminal is on screen: its window and tab.
    fn tab_of(&self, terminal: &str) -> Option<(&Window, &Tab)> {
        let windows = self.windows.as_ref()?;
        windows.iter().find_map(|window| {
            window.tabs.iter().find_map(|tab| {
                let mut found = Vec::new();
                tab.tree.terminals(&mut found);
                found.contains(&terminal).then_some((window, tab))
            })
        })
    }

    /// The label kablo has for a terminal, as a person set it.
    fn label_of(&self, terminal: &str) -> Option<String> {
        self.terminals
            .iter()
            .find(|row| row.status.id.as_str() == terminal)
            .and_then(|row| row.status.label.as_deref())
            .filter(|label| !label.is_empty())
            .map(printable)
    }

    fn rows(&self) -> Vec<&Row> {
        let mut rows: Vec<&Row> = self.terminals.iter().collect();
        rows.sort_by(|a, b| order(a.status.id.as_str()).cmp(&order(b.status.id.as_str())));
        rows
    }

    // ─── Text ────────────────────────────────────────────────────────────

    /// The text `ls` prints, `now` being the seconds since the Unix epoch on the clock the
    /// server's times are measured against.
    pub fn text(&self, now: u64) -> String {
        let mut out = String::new();
        match (&self.caller, &self.typing) {
            (Some(caller), Some(typing)) if caller == typing => {
                let _ = writeln!(
                    out,
                    "You are {}, and the user is typing in it.\n",
                    printable(caller)
                );
            }
            (Some(caller), Some(typing)) => {
                let _ = writeln!(
                    out,
                    "You are {}. The user is typing in {}.\n",
                    printable(caller),
                    printable(typing)
                );
            }
            (Some(caller), None) => {
                let _ = writeln!(out, "You are {}.\n", printable(caller));
            }
            (None, Some(typing)) => {
                let _ = writeln!(out, "The user is typing in {}.\n", printable(typing));
            }
            (None, None) => {}
        }
        match &self.windows {
            None => out
                .push_str("The bateri app is not connected, so what is on screen is not known.\n"),
            Some(windows) if windows.is_empty() => out.push_str("No windows are open.\n"),
            Some(windows) => {
                for window in windows {
                    write_window(&mut out, window, &|name| self.label_of(name));
                }
            }
        }
        out.push('\n');
        self.write_table(&mut out, now);
        out
    }

    fn write_table(&self, out: &mut String, now: u64) {
        let rows = self.rows();
        if rows.is_empty() {
            out.push_str("No terminals.\n");
            return;
        }
        let mut cells: Vec<[String; 10]> = vec![COLUMNS.map(str::to_owned)];
        for row in rows {
            let status = &row.status;
            let tab = match self.tab_of(status.id.as_str()) {
                Some((_, tab)) => named(IdKind::Tab, tab.id),
                None if row.waiting => "waiting".to_owned(),
                None if self.windows.is_none() => "-".to_owned(),
                None => "none".to_owned(),
            };
            cells.push([
                cell_name(status.id.as_str()),
                tab,
                or_dash(status.label.as_deref().map(printable)),
                or_dash(status.program.as_deref().map(lossy)),
                Doing::of(status, now).text(),
                since(now, status.started),
                since(now, status.output_at),
                format!("{}x{}", status.size.cols, status.size.rows),
                or_dash(row.opened_by.as_deref().map(printable)),
                or_dash(status.cwd.as_deref().map(lossy)),
            ]);
        }
        let mut widths = [0usize; 10];
        for line in &cells {
            for (width, cell) in widths.iter_mut().zip(line) {
                *width = (*width).max(cell.chars().count());
            }
        }
        for line in &cells {
            let mut text = String::new();
            for (column, (cell, width)) in line.iter().zip(widths).enumerate() {
                if column > 0 {
                    text.push_str("  ");
                }
                text.push_str(cell);
                if column + 1 < line.len() {
                    for _ in cell.chars().count()..width {
                        text.push(' ');
                    }
                }
            }
            out.push_str(&text);
            out.push('\n');
        }
    }

    // ─── JSON ────────────────────────────────────────────────────────────

    /// The scene as JSON: an object with `caller`, `typing`, `windows` (`null` when the bateri app is
    /// not connected) and `terminals`, a window or a terminal to a line.
    pub fn json(&self) -> String {
        let mut out = String::from("{\"caller\":");
        json_optional(&mut out, self.caller.as_deref());
        out.push_str(",\"typing\":");
        json_optional(&mut out, self.typing.as_deref());
        out.push_str(",\"windows\":");
        match &self.windows {
            None => out.push_str("null"),
            Some(windows) => {
                out.push('[');
                for (w, window) in windows.iter().enumerate() {
                    out.push_str(if w == 0 { "\n" } else { ",\n" });
                    self.json_window(&mut out, window);
                }
                out.push_str(if windows.is_empty() { "]" } else { "\n]" });
            }
        }
        out.push_str(",\"terminals\":[");
        let rows = self.rows();
        for (i, row) in rows.iter().enumerate() {
            out.push_str(if i == 0 { "\n" } else { ",\n" });
            self.json_row(&mut out, row);
        }
        out.push_str(if rows.is_empty() { "]}\n" } else { "\n]}\n" });
        out
    }

    fn json_row(&self, out: &mut String, row: &Row) {
        let status = &row.status;
        // The one reading of the times is the text's: `ls` says `now` only in words, and the
        // words are not in the data, so the data says what it is doing and when it began.
        out.push('{');
        json_pane_name(out, status.id.as_str());
        out.push_str(",\"state\":");
        json_string(out, &state_name(status.state));
        out.push_str(",\"activity\":");
        // The word does not depend on the clock; only the time in `running 3m 02s` does.
        json_string(out, Doing::of(status, 0).word());
        out.push_str(",\"program\":");
        json_optional(
            out,
            status
                .program
                .as_deref()
                .map(String::from_utf8_lossy)
                .as_deref(),
        );
        out.push_str(",\"directory\":");
        json_optional(
            out,
            status
                .cwd
                .as_deref()
                .map(String::from_utf8_lossy)
                .as_deref(),
        );
        let _ = write!(
            out,
            ",\"cols\":{},\"rows\":{},\"exit\":",
            status.size.cols, status.size.rows
        );
        match status.exit {
            None => out.push_str("null"),
            Some(Exit::Code(code)) => {
                let _ = write!(out, "{{\"code\":{code}}}");
            }
            Some(Exit::Signal(signal)) => {
                let _ = write!(out, "{{\"signal\":{signal}}}");
            }
        }
        let _ = write!(out, ",\"secret\":{}", status.secret);
        out.push_str(",\"label\":");
        json_optional(out, status.label.as_deref());
        out.push_str(",\"started\":");
        json_number(out, status.started);
        out.push_str(",\"front_started\":");
        json_number(out, status.front_started);
        out.push_str(",\"front_is_own\":");
        out.push_str(match status.front_is_own {
            Some(true) => "true",
            Some(false) => "false",
            None => "null",
        });
        out.push_str(",\"output_at\":");
        json_number(out, status.output_at);
        out.push_str(",\"opened_by\":");
        json_optional(out, row.opened_by.as_deref());
        let (placed, at) = match self.tab_of(status.id.as_str()) {
            Some(at) => ("tab", Some(at)),
            None if row.waiting => ("waiting", None),
            None if self.windows.is_none() => ("unknown", None),
            None => ("list", None),
        };
        out.push_str(",\"placed\":");
        json_string(out, placed);
        out.push_str(",\"window\":");
        json_id(out, at.map(|(window, _)| (IdKind::Window, window.id)));
        out.push_str(",\"group\":");
        json_id(
            out,
            at.and_then(|(window, tab)| group_of(window, tab))
                .map(|group| (IdKind::Group, group.id)),
        );
        out.push_str(",\"tab\":");
        json_id(out, at.map(|(_, tab)| (IdKind::Tab, tab.id)));
        out.push('}');
    }

    fn json_window(&self, out: &mut String, window: &Window) {
        if window.tabs.iter().all(Tab::elsewhere) {
            out.push_str("{\"elsewhere\":true}");
            return;
        }
        out.push_str("{\"id\":");
        json_id(out, Some((IdKind::Window, window.id)));
        let _ = write!(out, ",\"visible\":{},\"groups\":[", window.visible);
        let mut first = true;
        for group in &window.groups {
            // A group none of whose tabs can be seen is not told.
            let seen = window
                .tabs
                .iter()
                .any(|tab| tab.group == Some(group.id) && !tab.elsewhere());
            if !seen {
                continue;
            }
            if !first {
                out.push(',');
            }
            first = false;
            out.push_str("{\"id\":");
            json_id(out, Some((IdKind::Group, group.id)));
            out.push_str(",\"label\":");
            json_string(out, &printable(&group.label));
            out.push('}');
        }
        out.push_str("],\"tabs\":[");
        for (t, tab) in window.tabs.iter().enumerate() {
            if t > 0 {
                out.push(',');
            }
            self.json_tab(out, window, tab);
        }
        out.push_str("]}");
    }

    fn json_tab(&self, out: &mut String, window: &Window, tab: &Tab) {
        let tree = tab.tree.folded();
        out.push('{');
        if tree == Node::Pane(Pane::Elsewhere) {
            out.push_str("\"elsewhere\":true,\"keys\":");
            json_optional(out, tab.keys.as_deref());
            let _ = write!(out, ",\"selected\":{}}}", tab.selected);
            return;
        }
        out.push_str("\"id\":");
        json_id(out, Some((IdKind::Tab, tab.id)));
        out.push_str(",\"group\":");
        json_id(
            out,
            group_of(window, tab).map(|group| (IdKind::Group, group.id)),
        );
        out.push_str(",\"keys\":");
        json_optional(out, tab.keys.as_deref());
        out.push_str(",\"label\":");
        json_optional(out, tab.label.as_deref());
        out.push_str(",\"title\":");
        json_optional(out, tab.title.as_deref());
        let _ = write!(out, ",\"selected\":{},\"zoomed\":", tab.selected);
        match &tab.zoomed {
            Some(pane) => self.json_pane(out, pane, None),
            None => out.push_str("null"),
        }
        out.push_str(",\"tree\":");
        let on_screen = |name: &str| {
            window.visible
                && tab.selected
                && tab
                    .zoomed
                    .as_ref()
                    .is_none_or(|zoomed| *zoomed == Pane::Terminal(name.to_owned()))
        };
        self.json_node(out, &tree, &on_screen);
        out.push('}');
    }

    fn json_pane(&self, out: &mut String, pane: &Pane, on_screen: Option<bool>) {
        match pane {
            Pane::Terminal(name) => {
                out.push('{');
                json_pane_name(out, name);
                out.push_str(",\"label\":");
                json_optional(out, self.label_of(name).as_deref());
                if let Some(on_screen) = on_screen {
                    let _ = write!(out, ",\"on_screen\":{on_screen}");
                }
                out.push('}');
            }
            Pane::Remote { machine, name } => {
                out.push('{');
                json_remote_name(out, machine, name.as_deref());
                if let Some(on_screen) = on_screen {
                    let _ = write!(out, ",\"on_screen\":{on_screen}");
                }
                out.push('}');
            }
            Pane::Elsewhere => out.push_str("{\"elsewhere\":true}"),
        }
    }

    fn json_node(&self, out: &mut String, node: &Node, on_screen: &dyn Fn(&str) -> bool) {
        match node {
            Node::Pane(pane) => {
                let shown = match pane {
                    Pane::Terminal(name) => Some(on_screen(name)),
                    Pane::Remote { .. } | Pane::Elsewhere => None,
                };
                self.json_pane(out, pane, shown);
            }
            Node::Split {
                axis,
                ratio,
                first,
                second,
            } => {
                out.push_str("{\"axis\":");
                json_string(
                    out,
                    match axis {
                        Axis::Horizontal => "side_by_side",
                        Axis::Vertical => "stacked",
                    },
                );
                out.push_str(",\"ratio\":");
                json_ratio(out, *ratio);
                out.push_str(",\"first\":");
                self.json_node(out, first, on_screen);
                out.push_str(",\"second\":");
                self.json_node(out, second, on_screen);
                out.push('}');
            }
        }
    }
}

// ─── The layout, as text ─────────────────────────────────────────────────

/// A name a command takes: the kind's letter and the number (`t3`).
fn named(kind: IdKind, number: u64) -> String {
    format!("{}{number}", kind.prefix())
}

/// How a terminal is told in a line: `id:p9` for an agent's pane, `name:"b-0a1b"` for any other
/// terminal, which cannot be written to a command.
fn pane_field(terminal: &str) -> String {
    match Terminal::parse(terminal) {
        Some(pane) => format!("id:{pane}"),
        None => format!("name:\"{}\"", printable(terminal)),
    }
}

/// A terminal in the table's first column: its id, or `name:"…"` when it has none.
fn cell_name(terminal: &str) -> String {
    match Terminal::parse(terminal) {
        Some(pane) => pane.to_string(),
        None => pane_field(terminal),
    }
}

/// How a pane of another machine is told: `id:p9@prod` for an agent's pane there,
/// `name:"b-0a1b" machine:"prod"` for any other terminal and `machine:"local"` for a pane whose
/// terminal is not known by name.
fn remote_field(machine: &str, name: Option<&str>) -> String {
    let machine = printable(machine);
    match name {
        Some(name) => match Terminal::parse(name) {
            Some(pane) => format!("id:{pane}@{machine}"),
            None => format!("name:\"{}\" machine:\"{machine}\"", printable(name)),
        },
        None => format!("machine:\"{machine}\""),
    }
}

/// The group a tab is in, if the window has it.
fn group_of<'a>(window: &'a Window, tab: &Tab) -> Option<&'a Group> {
    let number = tab.group?;
    window.groups.iter().find(|group| group.id == number)
}

fn write_window(out: &mut String, window: &Window, label_of: &dyn Fn(&str) -> Option<String>) {
    if window.tabs.iter().all(Tab::elsewhere) {
        out.push_str("window another machine\n");
        return;
    }
    let _ = write!(out, "window id:{}", named(IdKind::Window, window.id));
    if !window.visible {
        out.push_str(" (hidden)");
    }
    out.push('\n');
    let mut at = 0;
    while at < window.tabs.len() {
        // The tabs of one group stand together; a tab in none stands alone.
        let group = group_of(window, &window.tabs[at]);
        let mut end = at + 1;
        while end < window.tabs.len() && group_of(window, &window.tabs[end]) == group {
            end += 1;
        }
        let run = &window.tabs[at..end];
        match group {
            Some(_) if run.iter().all(Tab::elsewhere) => {
                out.push_str("  group another machine\n");
            }
            Some(group) => {
                let _ = writeln!(
                    out,
                    "  group id:{} label:\"{}\"",
                    named(IdKind::Group, group.id),
                    printable(&group.label)
                );
                for tab in run {
                    write_tab(out, window, tab, 4, label_of);
                }
            }
            None => {
                for tab in run {
                    write_tab(out, window, tab, 2, label_of);
                }
            }
        }
        at = end;
    }
}

fn write_tab(
    out: &mut String,
    window: &Window,
    tab: &Tab,
    indent: usize,
    label_of: &dyn Fn(&str) -> Option<String>,
) {
    let tree = tab.tree.folded();
    let whole = tree == Node::Pane(Pane::Elsewhere);
    let _ = write!(out, "{}tab", " ".repeat(indent));
    if !whole {
        let _ = write!(out, " id:{}", named(IdKind::Tab, tab.id));
    }
    if let Some(keys) = &tab.keys {
        let _ = write!(out, " keys:{}", printable(keys));
    }
    if whole {
        let _ = write!(out, " {ELSEWHERE}");
    } else {
        if let Some(label) = &tab.label {
            let _ = write!(out, " label:\"{}\"", printable(label));
        }
        if let Some(title) = &tab.title {
            let _ = write!(out, " title:\"{}\"", printable(title));
        }
    }
    let mut notes: Vec<String> = Vec::new();
    if tab.selected {
        notes.push("selected".to_owned());
    }
    if !whole {
        match &tab.zoomed {
            Some(Pane::Terminal(name)) => notes.push(format!("zoomed on {}", printable(name))),
            Some(Pane::Remote { machine, name }) => {
                let name = name.as_deref().map(printable).unwrap_or_default();
                notes.push(format!("zoomed on {name}@{}", printable(machine)));
            }
            Some(Pane::Elsewhere) => notes.push(format!("zoomed on {ELSEWHERE}")),
            None => {}
        }
    }
    if !notes.is_empty() {
        let _ = write!(out, " ({})", notes.join(", "));
    }
    out.push('\n');
    if !whole {
        let on_screen = |name: &str| {
            window.visible
                && tab.selected
                && tab
                    .zoomed
                    .as_ref()
                    .is_none_or(|zoomed| *zoomed == Pane::Terminal(name.to_owned()))
        };
        write_node(out, &tree, indent + 2, &on_screen, label_of);
    }
}

fn percent(ratio: f64) -> (u32, u32) {
    let first = (ratio.clamp(0.0, 1.0) * 100.0).round() as u32;
    (first, 100 - first)
}

fn write_node(
    out: &mut String,
    node: &Node,
    indent: usize,
    on_screen: &dyn Fn(&str) -> bool,
    label_of: &dyn Fn(&str) -> Option<String>,
) {
    let pad = " ".repeat(indent);
    match node {
        Node::Pane(Pane::Elsewhere) => {
            let _ = writeln!(out, "{pad}{ELSEWHERE}");
        }
        Node::Pane(Pane::Terminal(name)) => {
            let _ = write!(out, "{pad}pane {}", pane_field(name));
            if let Some(label) = label_of(name) {
                let _ = write!(out, " label:\"{label}\"");
            }
            if on_screen(name) {
                out.push_str(" (on screen)");
            }
            out.push('\n');
        }
        Node::Pane(Pane::Remote { machine, name }) => {
            let _ = writeln!(out, "{pad}pane {}", remote_field(machine, name.as_deref()));
        }
        Node::Split {
            axis,
            ratio,
            first,
            second,
        } => {
            let (a, b) = percent(*ratio);
            let word = match axis {
                Axis::Horizontal => "side by side",
                Axis::Vertical => "stacked",
            };
            let _ = writeln!(out, "{pad}{word} {a}/{b}");
            write_node(out, first, indent + 2, on_screen, label_of);
            write_node(out, second, indent + 2, on_screen, label_of);
        }
    }
}

// ─── The layout, as JSON ─────────────────────────────────────────────────

/// A JSON string with kablo's rule: quote and backslash escaped, the usual control characters by
/// letter, any other as `\u00xx`.
fn json_string(out: &mut String, text: &str) {
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

fn json_optional(out: &mut String, text: Option<&str>) {
    match text {
        Some(text) => json_string(out, text),
        None => out.push_str("null"),
    }
}

fn json_number(out: &mut String, number: Option<u64>) {
    match number {
        Some(number) => {
            let _ = write!(out, "{number}");
        }
        None => out.push_str("null"),
    }
}

/// A share as a short decimal: `0.5`, `0.333`.
fn json_ratio(out: &mut String, ratio: f64) {
    let text = format!("{:.3}", ratio.clamp(0.0, 1.0));
    // `0.500` is `0.5`, `1.000` is `1` and `0.000` is `0`.
    out.push_str(text.trim_end_matches('0').trim_end_matches('.'));
}

/// A name as a JSON string, or `null` for nothing: `"t3"`.
fn json_id(out: &mut String, id: Option<(IdKind, u64)>) {
    match id {
        Some((kind, number)) => json_string(out, &named(kind, number)),
        None => out.push_str("null"),
    }
}

/// The keys that name a terminal of this server: `id` for an agent's pane, else `name`; the
/// other is `null`, so the keys are always both there, and `machine` is `null` for it.
fn json_pane_name(out: &mut String, terminal: &str) {
    out.push_str("\"id\":");
    match Terminal::parse(terminal) {
        Some(pane) => {
            json_string(out, &pane.to_string());
            out.push_str(",\"name\":null");
        }
        None => {
            out.push_str("null,\"name\":");
            json_string(out, terminal);
        }
    }
    out.push_str(",\"machine\":null");
}

/// The same keys for a pane of another machine: `id` is the address (`p9@prod`).
fn json_remote_name(out: &mut String, machine: &str, name: Option<&str>) {
    out.push_str("\"id\":");
    let pane = name.and_then(Terminal::parse);
    match pane {
        Some(pane) => json_string(out, &format!("{pane}@{}", printable(machine))),
        None => out.push_str("null"),
    }
    out.push_str(",\"name\":");
    match (pane, name) {
        (None, Some(name)) => json_string(out, name),
        _ => out.push_str("null"),
    }
    out.push_str(",\"machine\":");
    json_string(out, &printable(machine));
}

#[cfg(test)]
mod tests;
