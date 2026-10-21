//! The commands the open panes ran, as the palette lists and acts on them: Command History, and
//! Jump to Command… (the same list with ⏎ going to the output).
//!
//! The shell reads what each pane remembers of its commands ([`Commands`]); the rules here turn
//! it into rows, order them, and say what a key does with one ([`Use`], [`run`]).
//!
//! **Nothing here sends a byte that runs.** The line a command goes to is **written, not run**:
//! ⏎ leaves it on the prompt of the pane on screen when that pane is idle, and the user's own ⏎
//! runs it. A pane that is busy (a program, a server, the far end of ssh) is never written to — the
//! command waits on the line of a new local tab in its own directory instead, and a command that
//! ran on a server is not written to a *local* tab at all (it would run on this Mac): it is copied.
//!
//! **What is listed.** The commands of every pane, this tab's first, then the others', each group
//! newest first; the same command once, in its latest run. Beside the command: how it ended (✓ and
//! how long, ✗ and the exit code, or still running), and under it the tab it ran in and when. A
//! command whose output has left the history can still be written; it has no output to go to.
//! A pane with a program on the whole screen (vim) has its earlier commands listed all the same.
//! The screen their rows are on is behind the program while it runs: the output can still be
//! **copied** from there ([`CommandFacts::can_copy`]) but the window cannot be taken to it
//! ([`CommandFacts::can_show`]) until the program closes, and the tab's section carries a note
//! ([`FullScreen`]); the commands of the tab's other panes are not affected.
//!
//! **Identity.** A command is `hist.<n>`, `n` its place in [`Commands::items`] — the list the
//! shell handed in when the palette opened; nothing keeps it between openings ([`super::Recent`]
//! does not remember commands' text).

use bt_core::RecordState;

use crate::tabs::Indicator;

use super::list::List;
use super::rank::{Hit, Nearness, search};
use super::row::{Kind, Row, Scope, Status, Tone};
use super::state::Modifier;
use super::switch::Places;
use super::targets::{PortFacts, port_row};

/// The heading over the commands of the tab on screen.
pub const THIS_TAB_HEADING: &str = "This Tab";
/// The heading over the commands of the other tabs.
pub const OTHER_TABS_HEADING: &str = "Other Tabs";
/// The heading over what the root suggests when the field is empty.
pub const SUGGESTED_HEADING: &str = "Suggested";
/// How many rows the root suggests.
pub const SUGGESTED_LIMIT: usize = 3;

/// What the palette is told of one command.
#[derive(Clone, Debug, PartialEq)]
pub struct CommandFacts {
    /// The tab it ran in.
    pub tab: u64,
    pub text: String,
    /// It ran on the far end of ssh.
    pub remote: bool,
    pub standing: RecordState,
    /// When it started, seconds since the Unix epoch.
    pub started: Option<u64>,
    /// Its output is still in the history and can be reached: its row is, and its pane is not
    /// under a program that holds the whole screen.
    pub can_show: bool,
    /// Its output can be read, which a program on the whole screen does not prevent: the rows
    /// are behind it. `can_show` implies it.
    pub can_copy: bool,
    pub tab_title: String,
    /// Which window it is in, counted from 1, when it is not the key window's.
    pub other_window: Option<usize>,
    /// Its tab is the one on screen in the key window.
    pub here: bool,
}

/// A tab with a program on the whole screen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FullScreen {
    pub tab: u64,
    /// The program's name (`vim`), for the note, when the shell could tell.
    pub program: Option<String>,
}

/// Everything the command history is made of.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Commands {
    /// The commands of every pane, each pane's newest first.
    pub items: Vec<CommandFacts>,
    /// The tabs with a program on the whole screen.
    pub full_screen: Vec<FullScreen>,
    /// The pane on screen can take a line written to it: a local prompt with nothing running.
    pub free: bool,
    /// Now, seconds since the Unix epoch: "5 min ago".
    pub now: u64,
}

impl Commands {
    /// The note over this tab's commands when a program holds its screen.
    fn note(&self) -> Option<String> {
        let here = self.items.iter().find(|item| item.here)?.tab;
        let screen = self.full_screen.iter().find(|screen| screen.tab == here)?;
        Some(match &screen.program {
            Some(program) => format!(
                "{program} is full screen in this tab: its earlier output shows again when {program} closes."
            ),
            None => "A program is full screen in this tab: its earlier output shows again when it closes.".to_owned(),
        })
    }
}

/// What a key does with a command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Use {
    /// The line, left on the prompt of the pane on screen.
    Write,
    /// A new local tab in the command's directory, the line left on its prompt.
    NewTab,
    /// A split of the pane on screen, the line left on the new pane's prompt.
    SplitRight,
    /// The window goes to the command's output.
    ShowOutput,
    /// The line to the pasteboard.
    CopyCommand,
    /// The output's text to the pasteboard.
    CopyOutput,
}

impl Use {
    /// The verb the bottom band and the command's menu say.
    pub fn verb(self) -> &'static str {
        match self {
            Use::Write => "Write to the Line",
            Use::NewTab => "Write in a New Tab",
            Use::SplitRight => "Split Right",
            Use::ShowOutput => "Show Output",
            Use::CopyCommand => "Copy Command",
            Use::CopyOutput => "Copy Output",
        }
    }

    fn name(self) -> &'static str {
        match self {
            Use::Write => "write",
            Use::NewTab => "newTab",
            Use::SplitRight => "splitRight",
            Use::ShowOutput => "showOutput",
            Use::CopyCommand => "copyCommand",
            Use::CopyOutput => "copyOutput",
        }
    }

    fn of_name(name: &str) -> Option<Use> {
        [
            Use::Write,
            Use::NewTab,
            Use::SplitRight,
            Use::ShowOutput,
            Use::CopyCommand,
            Use::CopyOutput,
        ]
        .into_iter()
        .find(|how| how.name() == name)
    }
}

/// What ⏎, ⌥⏎ and ⌘⏎ do with one command; `None`: the key does nothing with it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Uses {
    pub enter: Option<Use>,
    pub option: Option<Use>,
    pub command: Option<Use>,
}

/// What the keys do with `facts`. `jump`: the list Jump to Command… opens, where ⏎ goes to the
/// output and ⌘⏎ does what ⏎ does elsewhere.
///
/// ⏎ writes the line to the pane on screen when it is idle; when it is not, to a new local tab —
/// except a command that ran on a server, which is copied instead (a tab opened here would run
/// it on this Mac). ⌥⏎ splits and waits there, a server's command again excepted. ⌘⏎ goes to
/// the output while there is one to go to, and copies it while there is only one to read (under a
/// program on the whole screen).
pub fn uses(facts: &CommandFacts, commands: &Commands, jump: bool) -> Uses {
    let line = if commands.free {
        Use::Write
    } else if facts.remote {
        Use::CopyCommand
    } else {
        Use::NewTab
    };
    let show = facts.can_show.then_some(Use::ShowOutput);
    // Under a program on the whole screen the window cannot be taken to the output, but it can be
    // copied from behind the program: ⌘⏎ does that instead.
    let output = show.or_else(|| facts.can_copy.then_some(Use::CopyOutput));
    let option = (!facts.remote).then_some(Use::SplitRight);
    if jump {
        Uses {
            enter: show,
            option,
            command: Some(line),
        }
    } else {
        Uses {
            enter: Some(line),
            option,
            command: output,
        }
    }
}

/// The line to leave on a prompt for `text`, or `None`: nothing to write, or a control character
/// in it (a tab, an escape, a newline) that a shell could take for more than text. The line has
/// no newline: the user's ⏎ is what runs it.
pub fn write_line(text: &str) -> Option<String> {
    let line = text.trim();
    (!line.is_empty() && !line.chars().any(char::is_control)).then(|| line.to_owned())
}

/// The name of the program a command line starts: its first word after the prompt (a lone `$`,
/// `%`, `#`, `>` or `❯` ahead of it), without its path. `None` for an empty line.
pub fn program_of(text: &str) -> Option<String> {
    const PROMPTS: [&str; 7] = ["$", "%", "#", ">", "❯", "›", "»"];
    let mut words = text.split_whitespace();
    let first = words.next()?;
    let word = if PROMPTS.contains(&first) {
        words.next()?
    } else {
        first
    };
    let name = word.rsplit('/').next().unwrap_or(word);
    (!name.is_empty()).then(|| name.to_owned())
}

// ─── Rows ─────────────────────────────────────────────────────────────────

/// "just now", "5 min ago", "3 h ago", "yesterday", "4 days ago".
pub fn ago(now: u64, then: u64) -> String {
    let secs = now.saturating_sub(then);
    match secs {
        0..45 => "just now".to_owned(),
        45..3600 => format!("{} min ago", (secs / 60).max(1)),
        3600..86_400 => format!("{} h ago", secs / 3600),
        86_400..172_800 => "yesterday".to_owned(),
        _ => format!("{} days ago", secs / 86_400),
    }
}

fn status(standing: RecordState) -> Option<Status> {
    let (text, tone) = match standing {
        RecordState::Running(time) => (
            match time.and_then(bt_core::running_counter) {
                Some(time) => format!("running · {time}"),
                None => "running".to_owned(),
            },
            Tone::Accent,
        ),
        RecordState::Finished {
            exit: Some(0),
            elapsed,
        } => (
            match elapsed.and_then(bt_core::finished_counter) {
                Some(time) => format!("✓ {time}"),
                None => "✓".to_owned(),
            },
            Tone::Success,
        ),
        RecordState::Finished {
            exit: Some(code), ..
        } => (format!("✗ exit {code}"), Tone::Error),
        RecordState::Finished { exit: None, .. } => ("ended".to_owned(), Tone::Dim),
        RecordState::Restored { failed: true } => ("✗".to_owned(), Tone::Error),
        RecordState::Restored { failed: false } => ("✓".to_owned(), Tone::Success),
    };
    Some(Status { text, tone })
}

/// The line of a command as one line of the list: a command typed over several lines reads as one.
fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn joined(parts: impl IntoIterator<Item = Option<String>>) -> String {
    parts
        .into_iter()
        .flatten()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" · ")
}

pub fn history_id(index: usize) -> String {
    format!("{}{index}", Kind::History.prefix())
}

fn parse_id(id: &str) -> Option<usize> {
    id.strip_prefix(Kind::History.prefix())?.parse().ok()
}

/// The row of command `index`: its line, how it ended, the tab it ran in and when. → opens its
/// menu; the keys at the bottom are the ones [`uses`] gives it.
pub fn command_row(index: usize, commands: &Commands, jump: bool) -> Option<Row> {
    let facts = commands.items.get(index)?;
    let title = one_line(&facts.text);
    let when = match facts.standing {
        RecordState::Restored { .. } => Some("earlier session".to_owned()),
        _ => facts.started.map(|started| ago(commands.now, started)),
    };
    let mut row = Row::new(Kind::History, &index.to_string(), title)
        .keywords("command history ran shell terminal")
        .subtitle(joined([
            facts.other_window.map(|number| format!("Window {number}")),
            Some(facts.tab_title.clone()),
            when,
        ]))
        .opens(Scope::HistoryActions(history_id(index)), false);
    row.status = status(facts.standing);
    let how = uses(facts, commands, jump);
    for (key, use_) in [("↵", how.enter), ("⌥↵", how.option), ("⌘↵", how.command)] {
        if let Some(use_) = use_ {
            row = row.hint(key, use_.verb());
        }
    }
    Some(row)
}

/// The indexes of the commands in the order they are listed: the same command once, in its
/// latest run, this tab's first, each group newest first.
fn listed(commands: &Commands) -> Vec<usize> {
    let mut newest: Vec<usize> = (0..commands.items.len()).collect();
    // Commands without a start (a restored screen's) go last, in the order they were given.
    newest.sort_by(|&a, &b| commands.items[b].started.cmp(&commands.items[a].started));
    let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
    newest.retain(|&at| seen.insert(commands.items[at].text.as_str()));
    let (here, others): (Vec<usize>, Vec<usize>) =
        newest.into_iter().partition(|&at| commands.items[at].here);
    here.into_iter().chain(others).collect()
}

/// Every command row, in the order they are listed, for the root and a page. `jump`: only the
/// commands whose output can be gone to.
pub fn command_rows(commands: &Commands, jump: bool) -> Vec<Row> {
    listed(commands)
        .into_iter()
        .filter(|&at| !jump || commands.items[at].can_show)
        .filter_map(|at| command_row(at, commands, jump))
        .collect()
}

/// The command history page. An empty field: this tab's commands under *This Tab* (a note over
/// them when a program holds its screen) and the others' under *Other Tabs*. A query: the
/// commands it finds, best first.
pub fn history_list(commands: &Commands, query: &str, jump: bool) -> List {
    let rows = command_rows(commands, jump);
    if !query.trim().is_empty() {
        return List {
            hits: search(&rows, query, Nearness::None),
            headings: Vec::new(),
        };
    }
    let mut list = List::default();
    let is_here = |row: &Row| {
        parse_id(&row.id)
            .and_then(|at| commands.items.get(at))
            .is_some_and(|facts| facts.here)
    };
    let (here, others): (Vec<Row>, Vec<Row>) = rows.into_iter().partition(is_here);
    let note = commands.note();
    for (heading, rows) in [
        (note.unwrap_or_else(|| THIS_TAB_HEADING.to_owned()), here),
        (OTHER_TABS_HEADING.to_owned(), others),
    ] {
        if rows.is_empty() {
            continue;
        }
        list.headings.push((list.hits.len(), heading));
        list.hits.extend(rows.into_iter().map(|row| Hit {
            row,
            score: 0.0,
            marks: Vec::new(),
        }));
    }
    list
}

// ─── The menu of a command ────────────────────────────────────────────────

/// The rows of the page a command's menu opens ([`Scope::HistoryActions`]); empty for any other
/// scope or a command that is gone. Everything [`uses`] can come to, in a fixed order, those the
/// command has.
pub fn actions(scope: &Scope, commands: &Commands) -> Vec<Row> {
    let Scope::HistoryActions(id) = scope else {
        return Vec::new();
    };
    let Some(facts) = parse_id(id).and_then(|at| commands.items.get(at)) else {
        return Vec::new();
    };
    [
        Use::Write,
        Use::ShowOutput,
        Use::CopyCommand,
        Use::CopyOutput,
        Use::NewTab,
        Use::SplitRight,
    ]
    .into_iter()
    .filter(|how| match how {
        Use::Write => commands.free,
        Use::ShowOutput => facts.can_show,
        Use::CopyOutput => facts.can_copy,
        Use::NewTab | Use::SplitRight => !facts.remote,
        Use::CopyCommand => true,
    })
    .map(|how| Row::new(Kind::Action, how.name(), how.verb()))
    .collect()
}

/// What stands over the rows of a command's menu: the command.
pub fn actions_heading(scope: &Scope, commands: &Commands) -> Option<String> {
    let Scope::HistoryActions(id) = scope else {
        return None;
    };
    parse_id(id)
        .and_then(|at| commands.items.get(at))
        .map(|facts| one_line(&facts.text))
}

// ─── What choosing does ───────────────────────────────────────────────────

/// What choosing a command, or one of the actions of its menu, comes to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Run {
    /// The command's place in [`Commands::items`].
    pub index: usize,
    pub how: Use,
}

/// What the row `id` chosen with `modifier` on the page `scope` (`None`: the root) comes to;
/// `None` for a row that is not a command's or a use the command does not have.
pub fn run(
    id: &str,
    scope: Option<&Scope>,
    modifier: Modifier,
    commands: &Commands,
) -> Option<Run> {
    if let Some(index) = parse_id(id) {
        let jump = matches!(scope, Some(Scope::History { jump: true, .. }));
        let facts = commands.items.get(index)?;
        let how = uses(facts, commands, jump);
        let how = match modifier {
            Modifier::None => how.enter,
            Modifier::Option => how.option,
            Modifier::Command => how.command,
        }?;
        return Some(Run { index, how });
    }
    let name = id.strip_prefix(Kind::Action.prefix())?;
    let Some(Scope::HistoryActions(target)) = scope else {
        return None;
    };
    if modifier != Modifier::None {
        return None;
    }
    let index = parse_id(target)?;
    let how = Use::of_name(name)?;
    // Only what the menu offers.
    actions(scope?, commands)
        .iter()
        .any(|row| row.id == format!("{}{name}", Kind::Action.prefix()))
        .then_some(Run { index, how })
}

// ─── Suggested ────────────────────────────────────────────────────────────

/// What the root suggests when the field is empty, at most [`SUGGESTED_LIMIT`] rows: a tab that
/// asks something of the user, a command that failed in a tab the user has not looked at since,
/// a port that started listening since the palette was last opened. Nothing to suggest, no rows.
///
/// A suggestion is the row the thing has elsewhere (a tab's, a command's, a port's), unchanged.
pub fn suggested(
    places: &Places,
    commands: &Commands,
    ports: &[PortFacts],
    fresh_ports: &[String],
) -> Vec<Row> {
    let mut rows: Vec<Row> = Vec::new();
    let asking = places.tabs.iter().filter(|tab| {
        tab.report
            .as_ref()
            .is_some_and(|report| report.indicator == Indicator::Question)
    });
    rows.extend(asking.map(super::switch::tab_row));
    let failed = places.tabs.iter().filter(|tab| {
        tab.report
            .as_ref()
            .is_some_and(|report| report.indicator == Indicator::Failed)
    });
    for tab in failed {
        // Its newest failed command, if the pane remembers it; else the tab itself.
        let command = commands
            .items
            .iter()
            .position(|item| {
                item.tab == tab.tab
                    && matches!(
                        item.standing,
                        RecordState::Finished {
                            exit: Some(code), ..
                        } if code != 0
                    )
            })
            .and_then(|at| command_row(at, commands, false));
        rows.extend(command.or_else(|| Some(super::switch::tab_row(tab))));
    }
    rows.extend(
        ports
            .iter()
            .filter(|port| fresh_ports.contains(&port.key))
            .map(port_row),
    );
    rows.truncate(SUGGESTED_LIMIT);
    rows
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::tabs::Indicator;

    fn facts(tab: u64, text: &str, started: u64) -> CommandFacts {
        CommandFacts {
            tab,
            text: text.to_owned(),
            remote: false,
            standing: RecordState::Finished {
                exit: Some(0),
                elapsed: Some(Duration::from_millis(3200)),
            },
            started: Some(started),
            can_show: true,
            can_copy: true,
            tab_title: format!("tab {tab}"),
            other_window: None,
            here: tab == 1,
        }
    }

    fn commands(items: Vec<CommandFacts>) -> Commands {
        Commands {
            items,
            full_screen: Vec::new(),
            free: true,
            now: 10_000,
        }
    }

    fn ids(list: &List) -> Vec<&str> {
        list.hits.iter().map(|hit| hit.row.id.as_str()).collect()
    }

    #[test]
    fn this_tabs_commands_come_first_and_each_group_newest_first() {
        let commands = commands(vec![
            facts(2, "other new", 9_000),
            facts(1, "here old", 1_000),
            facts(1, "here new", 5_000),
            facts(2, "other old", 2_000),
        ]);
        let list = history_list(&commands, "", false);
        assert_eq!(ids(&list), ["hist.2", "hist.1", "hist.0", "hist.3"]);
        assert_eq!(list.headings[0], (0, THIS_TAB_HEADING.to_owned()));
        assert_eq!(list.headings[1], (2, OTHER_TABS_HEADING.to_owned()));
    }

    #[test]
    fn the_same_command_is_listed_once_in_its_latest_run() {
        let mut older = facts(1, "make", 100);
        older.standing = RecordState::Finished {
            exit: Some(2),
            elapsed: None,
        };
        let commands = commands(vec![facts(2, "make", 900), older, facts(1, "ls", 500)]);
        let rows = command_rows(&commands, false);
        let titles: Vec<&str> = rows.iter().map(|row| row.title.as_str()).collect();
        assert_eq!(titles, ["ls", "make"]);
        let make = rows.iter().find(|row| row.title == "make").unwrap();
        assert_eq!(make.id, "hist.0", "the run in tab 2, which is the later");
        assert_eq!(make.status.as_ref().unwrap().text, "✓ 3.2s");
    }

    #[test]
    fn how_a_command_ended_is_said_in_words_and_a_tone() {
        let text = |standing| status(standing).unwrap();
        let failed = text(RecordState::Finished {
            exit: Some(1),
            elapsed: None,
        });
        assert_eq!(
            (failed.text.as_str(), failed.tone),
            ("✗ exit 1", Tone::Error)
        );
        let quick = text(RecordState::Finished {
            exit: Some(0),
            elapsed: Some(Duration::from_millis(40)),
        });
        assert_eq!(
            quick.text, "✓",
            "a duration below the counter's floor is not said"
        );
        let running = text(RecordState::Running(Some(Duration::from_secs(75))));
        assert_eq!(running.text, "running · 1m 15s");
        assert_eq!(
            text(RecordState::Restored { failed: true }).text,
            "✗",
            "a restored screen keeps the stripe, not the code"
        );
    }

    #[test]
    fn the_second_line_says_the_tab_and_when() {
        let mut far = facts(2, "make", 10_000 - 7_300);
        far.other_window = Some(2);
        let mut restored = facts(1, "ls", 0);
        restored.standing = RecordState::Restored { failed: false };
        restored.started = None;
        let commands = commands(vec![far, restored]);
        let rows = command_rows(&commands, false);
        let by_title = |title: &str| rows.iter().find(|row| row.title == title).unwrap();
        assert_eq!(by_title("make").subtitle, "Window 2 · tab 2 · 2 h ago");
        assert_eq!(by_title("ls").subtitle, "tab 1 · earlier session");
        assert_eq!(ago(100, 100), "just now");
        assert_eq!(ago(1000, 400), "10 min ago");
        assert_eq!(ago(100_000, 100_000 - 90_000), "yesterday");
        assert_eq!(ago(1_000_000, 100_000), "10 days ago");
    }

    #[test]
    fn a_command_over_several_lines_reads_as_one() {
        let commands = commands(vec![facts(1, "echo a \\\n   b", 1)]);
        assert_eq!(command_rows(&commands, false)[0].title, "echo a \\ b");
    }

    #[test]
    fn enter_writes_to_an_idle_pane_and_opens_a_tab_for_a_busy_one() {
        let local = facts(1, "make", 1);
        let mut idle = commands(vec![local.clone()]);
        assert_eq!(uses(&local, &idle, false).enter, Some(Use::Write));
        idle.free = false;
        assert_eq!(uses(&local, &idle, false).enter, Some(Use::NewTab));
        assert_eq!(uses(&local, &idle, false).option, Some(Use::SplitRight));
    }

    #[test]
    fn a_servers_command_is_never_written_into_a_local_tab() {
        let mut remote = facts(1, "systemctl restart nginx", 1);
        remote.remote = true;
        let mut busy = commands(vec![remote.clone()]);
        busy.free = false;
        let how = uses(&remote, &busy, false);
        assert_eq!(how.enter, Some(Use::CopyCommand));
        assert_eq!(how.option, None, "a split is a local pane too");
        let page = actions(&Scope::HistoryActions("hist.0".to_owned()), &busy);
        let names: Vec<&str> = page.iter().map(|row| row.id.as_str()).collect();
        assert_eq!(
            names,
            ["act.showOutput", "act.copyCommand", "act.copyOutput"]
        );
    }

    #[test]
    fn jump_to_command_swaps_enter_and_command_enter() {
        let local = facts(1, "make", 1);
        let all = commands(vec![local.clone()]);
        let how = uses(&local, &all, true);
        assert_eq!(how.enter, Some(Use::ShowOutput));
        assert_eq!(how.command, Some(Use::Write));
        let jump = Scope::History {
            jump: true,
            query: None,
        };
        let go = run("hist.0", Some(&jump), Modifier::None, &all).unwrap();
        assert_eq!(
            go,
            Run {
                index: 0,
                how: Use::ShowOutput
            }
        );
        let go = run("hist.0", None, Modifier::None, &all).unwrap();
        assert_eq!(go.how, Use::Write, "on the root ⏎ writes");
    }

    #[test]
    fn a_command_whose_output_is_gone_has_no_output_to_go_to() {
        let mut gone = facts(1, "yes", 1);
        gone.can_show = false;
        gone.can_copy = false;
        let all = commands(vec![gone.clone(), facts(1, "ls", 2)]);
        assert_eq!(uses(&gone, &all, false).command, None);
        assert_eq!(
            command_rows(&all, true).len(),
            1,
            "Jump lists only the reachable"
        );
        assert_eq!(command_rows(&all, false).len(), 2);
        assert!(run("hist.0", None, Modifier::Command, &all).is_none());
    }

    #[test]
    fn a_tab_with_a_program_on_its_screen_lists_its_commands_under_a_note_and_copies_their_output()
    {
        let mut under = [facts(1, "vim notes", 2), facts(1, "ls", 1)];
        // The pane under the program has rows to read behind it but none to take the window to.
        under.iter_mut().for_each(|facts| facts.can_show = false);
        let mut sibling = facts(1, "make test", 3);
        sibling.standing = RecordState::Finished {
            exit: Some(0),
            elapsed: None,
        };
        let mut all = commands(vec![under[0].clone(), under[1].clone(), sibling]);
        all.full_screen = vec![FullScreen {
            tab: 1,
            program: Some("vim".to_owned()),
        }];
        let list = history_list(&all, "", false);
        assert_eq!(
            list.headings[0].1,
            "vim is full screen in this tab: its earlier output shows again when vim closes."
        );
        assert_eq!(list.hits.len(), 3);
        let output_key = |id: &str| {
            let hit = list.hits.iter().find(|hit| hit.row.id == id).unwrap();
            hit.row
                .hints
                .iter()
                .find(|(key, _)| *key == "⌘↵")
                .map(|(_, verb)| verb.to_string())
        };
        assert_eq!(
            output_key("hist.0"),
            Some(Use::CopyOutput.verb().to_owned()),
            "⌘⏎ copies under the program"
        );
        assert_eq!(output_key("hist.1"), output_key("hist.0"));
        assert_eq!(
            output_key("hist.2"),
            Some(Use::ShowOutput.verb().to_owned()),
            "the tab's other pane is gone to"
        );
        assert_eq!(
            run("hist.0", None, Modifier::Command, &all).map(|run| run.how),
            Some(Use::CopyOutput)
        );
        let jump: Vec<String> = command_rows(&all, true)
            .into_iter()
            .map(|row| row.id)
            .collect();
        assert_eq!(jump, ["hist.2"], "only what has an output to go to");
        let page = actions(&Scope::HistoryActions("hist.1".to_owned()), &all);
        assert!(page.iter().all(|row| row.id != "act.showOutput"));
        assert!(page.iter().any(|row| row.id == "act.copyOutput"));
        let mut unread = all.clone();
        unread.items[1].can_copy = false;
        assert_eq!(
            uses(&unread.items[1], &unread, false).command,
            None,
            "a row the program covered and that had already left the history"
        );
    }

    #[test]
    fn a_query_finds_commands_by_their_words_and_their_tab() {
        let mut all = commands(vec![
            facts(1, "terraform plan", 5),
            facts(2, "git status", 4),
        ]);
        all.items[1].tab_title = "prod server".to_owned();
        let list = history_list(&all, "terra", false);
        assert_eq!(ids(&list), ["hist.0"]);
        let list = history_list(&all, "server", false);
        assert_eq!(ids(&list), ["hist.1"], "the second line is searched");
        assert!(list.headings.is_empty());
    }

    #[test]
    fn a_menu_choice_is_only_what_the_menu_offers() {
        let all = commands(vec![facts(1, "make", 1)]);
        let page = Scope::HistoryActions("hist.0".to_owned());
        let go = |name: &str| run(&format!("act.{name}"), Some(&page), Modifier::None, &all);
        assert_eq!(go("copyOutput").map(|run| run.how), Some(Use::CopyOutput));
        assert_eq!(go("newTab").map(|run| run.how), Some(Use::NewTab));
        assert_eq!(go("bogus"), None);
        let mut busy = all.clone();
        busy.free = false;
        assert_eq!(
            run("act.write", Some(&page), Modifier::None, &busy),
            None,
            "a busy pane takes no line"
        );
    }

    #[test]
    fn a_line_is_written_only_when_nothing_in_it_could_run_something_else() {
        assert_eq!(write_line("  git status "), Some("git status".to_owned()));
        assert_eq!(write_line(""), None);
        assert_eq!(write_line("echo a\nrm -rf x"), None);
        assert_eq!(write_line("echo \u{1b}[31m"), None);
        assert_eq!(write_line("echo\ttab"), None);
    }

    #[test]
    fn the_program_of_a_line_is_its_first_word_past_the_prompt_mark_without_its_path() {
        assert_eq!(program_of("vim notes.md"), Some("vim".to_owned()));
        assert_eq!(program_of("$ vim notes.md"), Some("vim".to_owned()));
        assert_eq!(program_of("❯ /usr/bin/less log"), Some("less".to_owned()));
        assert_eq!(program_of(""), None);
        assert_eq!(program_of("$"), None);
    }

    #[test]
    fn the_history_fallback_opens_the_history_with_the_query_in_the_field() {
        use super::super::rank::fallbacks;
        use super::super::state::{Key, Palette};
        let row = fallbacks("terraform").hits[1].row.clone();
        assert_eq!(row.id, "search.history");
        let mut machine = Palette::new();
        machine.typed("terraform");
        machine.key(Key::Enter(Modifier::None), &[row]);
        assert_eq!(
            machine.scope(),
            Some(&Scope::History {
                jump: false,
                query: Some("terraform".to_owned())
            })
        );
        assert_eq!(machine.query(), "terraform");
    }

    fn tab_with(tab: u64, indicator: Indicator) -> super::super::switch::TabFacts {
        super::super::switch::TabFacts {
            tab,
            window: 1,
            title: format!("tab {tab}"),
            report: Some(super::super::switch::Report {
                indicator,
                running: None,
            }),
            place: None,
            hosts: Vec::new(),
            group: None,
            other_window: None,
            key: None,
            here: false,
        }
    }

    #[test]
    fn what_the_root_suggests_is_what_asks_for_the_user_and_is_at_most_three() {
        let places = Places {
            tabs: vec![
                tab_with(2, Indicator::Question),
                tab_with(3, Indicator::Failed),
                tab_with(4, Indicator::Running),
            ],
            groups: Vec::new(),
        };
        let mut failed = facts(3, "terraform plan", 5);
        failed.standing = RecordState::Finished {
            exit: Some(1),
            elapsed: None,
        };
        let all = commands(vec![facts(3, "ls", 9), failed]);
        let ports = [PortFacts {
            key: "3000".to_owned(),
            title: "3000".to_owned(),
            subtitle: String::new(),
            url: "http://localhost:3000".to_owned(),
        }];
        let rows = suggested(&places, &all, &ports, &["3000".to_owned()]);
        let ids: Vec<&str> = rows.iter().map(|row| row.id.as_str()).collect();
        assert_eq!(
            ids,
            ["tab.2", "hist.1", "port.3000"],
            "the tab asking, the command that failed, the port that came"
        );
        assert!(suggested(&places, &all, &ports, &[]).len() == 2);
        assert!(suggested(&Places::default(), &all, &ports, &[]).is_empty());
    }
}
