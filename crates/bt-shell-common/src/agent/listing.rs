//! `bateri ls`: what is open, as an agent on a server reads it.
//!
//! Two parts, in the order a person looks for them. First **where things are**: the windows
//! front to back, each window's tabs in the order of its strip, and in each tab the split tree
//! itself — `side by side` or `stacked`, the first and second share, the terminals by their
//! numbers and nothing else of them. Then **what they are**: one table row per terminal the
//! server has (`ID TAB LABEL PROGRAM STATE AGE QUIET SIZE OPENED BY DIRECTORY`), every column of
//! it from the server's own statement ([`kablo_proto::v1::Status`]) so that it reads the same
//! with the Mac away. A terminal in no tab says `none` there (it is in the list of what runs in
//! the background) and one whose pane is waiting for the Mac says `waiting`.
//!
//! **What an agent is not told.** A pane that is not a terminal of this server — a local shell,
//! another server's terminal — is `another machine` and nothing more: no title, no directory,
//! no program. A split whose both sides are other machines is one such node, a tab made of
//! nothing else is one nameless line, and a tab has a name here only when the user gave it one
//! or it comes from a terminal of this server (the one that builds a [`Tab`] decides; this
//! prints what it is given).
//!
//! **Where the words come from.** What a terminal is doing is one reading of the server's times
//! ([`crate::kablo::roster::activity_at`], the card's own), `idle` only for a shell waiting for
//! a command (a program started in place of the shell is `running` however quiet), a time is
//! [`bt_core::running_counter`]'s text and `running 3m 02s` is the card's
//! ([`crate::server_card::running_text`]). The shortcut of a tab is handed in by whoever builds
//! the scene ([`crate::palette::tab_keys`]).
//!
//! The same scene is also JSON ([`Scene::json`]), written by hand — no dependency for a dozen
//! keys — with kablo's own names for what kablo calls a thing (`list --json`: `id`, `state`,
//! `program`, `directory`, `cols`, `rows`, `exit`, `secret`, `label`, `started`, `front_started`,
//! `front_is_own`, `output_at`) and the rule for strings. Keys are added, never taken away.

use std::fmt::Write as _;
use std::time::Duration;

use bt_core::running_counter;
use kablo_proto::v1::{Exit, State, Status};

use super::Terminal;
use super::notes::Record;
use crate::kablo::roster::{Activity, activity_at};
use crate::program::is_shell_name;
use crate::server_card::running_text;
use crate::split::{Axis, Tree};

/// What a pane in a tab shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pane {
    /// A terminal of this server, by its name there.
    Terminal(String),
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
            Node::Pane(Pane::Elsewhere) => {}
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
    /// The ⌘ shortcut that selects it, when it has one ([`crate::palette::tab_keys`]).
    pub keys: Option<String>,
    /// Its name, only if the user gave it or it comes from a terminal of this server.
    pub name: Option<String>,
    pub selected: bool,
    /// The pane filling the tab, when one is zoomed.
    pub zoomed: Option<Pane>,
    pub tree: Node,
}

/// A window. Its place in [`Scene::windows`] is its number, front to back.
#[derive(Clone, Debug, PartialEq)]
pub struct Window {
    /// It can be seen: not minimized, not on another space.
    pub visible: bool,
    /// In the order of the tab strip; the place is the tab's number.
    pub tabs: Vec<Tab>,
}

/// A terminal of the server.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    /// What the server says of it.
    pub status: Status,
    /// The terminal that opened it, when one did.
    pub opened_by: Option<String>,
    /// Its pane waits for the Mac: it was opened while the Mac was away.
    pub waiting: bool,
}

/// What `ls` is asked about.
#[derive(Clone, Debug, PartialEq)]
pub struct Scene {
    /// The terminal that asked.
    pub caller: Option<String>,
    /// The terminal the user is typing in, when it is one of this server's.
    pub typing: Option<String>,
    /// The windows, or `None` when the Mac is not connected and nothing of the layout is known.
    pub windows: Option<Vec<Window>>,
    pub terminals: Vec<Row>,
}

/// The table's rows: each terminal the server said it has, with the terminal that opened it as
/// the server's records say (a terminal with no record, or opened from none, has no opener) and
/// whether its pane waits for the Mac (`waits`).
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
    /// Where a terminal is on screen: its window and tab, counted from 1.
    fn tab_of(&self, terminal: &str) -> Option<(usize, usize)> {
        let windows = self.windows.as_ref()?;
        windows.iter().enumerate().find_map(|(w, window)| {
            window.tabs.iter().enumerate().find_map(|(t, tab)| {
                let mut found = Vec::new();
                tab.tree.terminals(&mut found);
                found.contains(&terminal).then_some((w + 1, t + 1))
            })
        })
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
            None => out.push_str("The Mac is not connected, so what is on screen is not known.\n"),
            Some(windows) if windows.is_empty() => out.push_str("No windows are open.\n"),
            Some(windows) => {
                for (w, window) in windows.iter().enumerate() {
                    write_window(&mut out, w + 1, window);
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
                Some((window, tab)) => format!("{window}.{tab}"),
                None if row.waiting => "waiting".to_owned(),
                None if self.windows.is_none() => "-".to_owned(),
                None => "none".to_owned(),
            };
            cells.push([
                status.id.as_str().to_owned(),
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

    /// The scene as JSON: an object with `caller`, `typing`, `windows` (`null` when the Mac is
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
                    json_window(&mut out, w + 1, window);
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
        out.push_str("{\"id\":");
        json_string(out, status.id.as_str());
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
        let (placed, window, tab) = match self.tab_of(status.id.as_str()) {
            Some((window, tab)) => ("tab", Some(window), Some(tab)),
            None if row.waiting => ("waiting", None, None),
            None if self.windows.is_none() => ("unknown", None, None),
            None => ("list", None, None),
        };
        out.push_str(",\"placed\":");
        json_string(out, placed);
        out.push_str(",\"window\":");
        json_number(out, window.map(|n| n as u64));
        out.push_str(",\"tab\":");
        json_number(out, tab.map(|n| n as u64));
        out.push('}');
    }
}

// ─── The layout, as text ─────────────────────────────────────────────────

fn write_window(out: &mut String, number: usize, window: &Window) {
    let _ = write!(out, "Window {number}");
    if !window.visible {
        out.push_str(" (hidden)");
    }
    out.push('\n');
    for (t, tab) in window.tabs.iter().enumerate() {
        let tree = tab.tree.folded();
        let whole = tree == Node::Pane(Pane::Elsewhere);
        let _ = write!(out, "  Tab {number}.{}", t + 1);
        if let Some(keys) = &tab.keys {
            let _ = write!(out, " {}", printable(keys));
        }
        if let Some(name) = &tab.name {
            let _ = write!(out, " \"{}\"", printable(name));
        }
        if whole {
            let _ = write!(out, " {ELSEWHERE}");
        }
        let mut notes: Vec<String> = Vec::new();
        if tab.selected {
            notes.push("selected".to_owned());
        }
        if !whole {
            match &tab.zoomed {
                Some(Pane::Terminal(name)) => notes.push(format!("zoomed on {}", printable(name))),
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
            write_node(out, &tree, 1, &on_screen);
        }
    }
}

fn percent(ratio: f64) -> (u32, u32) {
    let first = (ratio.clamp(0.0, 1.0) * 100.0).round() as u32;
    (first, 100 - first)
}

fn write_node(out: &mut String, node: &Node, depth: usize, on_screen: &dyn Fn(&str) -> bool) {
    let indent = "  ".repeat(depth + 1);
    match node {
        Node::Pane(Pane::Elsewhere) => {
            let _ = writeln!(out, "{indent}{ELSEWHERE}");
        }
        Node::Pane(Pane::Terminal(name)) => {
            let _ = write!(out, "{indent}{}", printable(name));
            if on_screen(name) {
                out.push_str(" (on screen)");
            }
            out.push('\n');
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
            let _ = writeln!(out, "{indent}{word} {a}/{b}");
            write_node(out, first, depth + 1, on_screen);
            write_node(out, second, depth + 1, on_screen);
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

fn json_pane(out: &mut String, pane: &Pane, on_screen: Option<bool>) {
    match pane {
        Pane::Terminal(name) => {
            out.push_str("{\"terminal\":");
            json_string(out, name);
            if let Some(on_screen) = on_screen {
                let _ = write!(out, ",\"on_screen\":{on_screen}");
            }
            out.push('}');
        }
        Pane::Elsewhere => out.push_str("{\"elsewhere\":true}"),
    }
}

fn json_node(out: &mut String, node: &Node, on_screen: &dyn Fn(&str) -> bool) {
    match node {
        Node::Pane(pane) => {
            let shown = match pane {
                Pane::Terminal(name) => Some(on_screen(name)),
                Pane::Elsewhere => None,
            };
            json_pane(out, pane, shown);
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
            json_node(out, first, on_screen);
            out.push_str(",\"second\":");
            json_node(out, second, on_screen);
            out.push('}');
        }
    }
}

fn json_window(out: &mut String, number: usize, window: &Window) {
    let _ = write!(
        out,
        "{{\"number\":{number},\"visible\":{},\"tabs\":[",
        window.visible
    );
    for (t, tab) in window.tabs.iter().enumerate() {
        if t > 0 {
            out.push(',');
        }
        let tree = tab.tree.folded();
        let _ = write!(out, "{{\"number\":{},\"keys\":", t + 1);
        json_optional(out, tab.keys.as_deref());
        out.push_str(",\"name\":");
        json_optional(out, tab.name.as_deref());
        let _ = write!(out, ",\"selected\":{},\"zoomed\":", tab.selected);
        match &tab.zoomed {
            Some(pane) if tree != Node::Pane(Pane::Elsewhere) => json_pane(out, pane, None),
            _ => out.push_str("null"),
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
        json_node(out, &tree, &on_screen);
        out.push('}');
    }
    out.push_str("]}");
}

#[cfg(test)]
mod tests;
