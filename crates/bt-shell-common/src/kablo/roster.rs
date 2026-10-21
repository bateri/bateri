//! The terminals a server keeps that no pane of this Mac shows: "running in the background".
//!
//! The server's `list` answers with every terminal of the account; the bridge does not say when
//! one is made or ends unless this client is attached to it. So what is known here is **a cache**
//! of what the last answers said, brought up to date by the events that do come
//! ([`Roster::apply`]: `Listed` replaces it, `Status` and `Created` set one terminal, `State`
//! renews the program in front, `Ended` takes one away) and by asking again when something that
//! could change it happened (a connection came up, a pane let go of its terminal, the palette or the
//! card opened — [`super::service::Service::refresh_background`]). Nothing asks on a clock: the list
//! is read where it is wanted, without waiting for an answer, and may be a moment old.
//!
//! "In the background" is the terminals of the account that are **running**, that **no pane of this
//! Mac shows** (the service tells which: [`Roster::background`] takes that as a question) and that
//! nobody asked to end. The ones a pane shows are on screen already; the ones that ended wait for
//! someone to hear of it and are not running; the ones the relay cannot serve cannot be attached.
//! The most recently written come first — what a person remembers having been busy with.
//!
//! A terminal that another Mac has open is not in the background: it is **on another Mac**
//! ([`Background::elsewhere`], the server's list of who is attached — kablo names its clients).
//! [`Roster::split`] gives the two halves of the one list, so no reader can take both for one; a server too old to name its clients cannot tell, and its
//! terminals stay in the background.
//!
//! Everything a person is shown is cleaned of control characters here, once, the way
//! [`super::service::Shown`] is. The times are the server's: seconds of **its** clock
//! ([`Background::activity`] takes the moment to measure them against from the caller, and a
//! server whose clock is off says "running" for as long as the clocks differ).

use std::collections::BTreeMap;
use std::time::Duration;

use kablo_proto::v1::{State, Status};

use super::{ErrorCode, Event, SessionId};

/// A terminal a server keeps for the user that no pane of this Mac shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Background {
    /// The server's account ([`crate::ssh_wrap::host_key`]).
    pub account: String,
    /// The server as the user typed it (`rasp`): the word that names it in the lines they read.
    pub host: String,
    /// The `ssh` arguments that reach the server, the program's name first
    /// ([`super::service::Shown::argv`]): what a pane that shows the terminal is started with.
    pub argv: Vec<String>,
    /// The terminal's name on the server.
    pub terminal: SessionId,
    /// The program in front of the terminal, as the server last said.
    pub program: Option<String>,
    /// The directory of that program on the server.
    pub cwd: Option<String>,
    /// The name a person or a program gave the terminal.
    pub label: Option<String>,
    /// When the terminal's own program started (seconds of the server's clock).
    pub started: Option<u64>,
    /// When the program in front started.
    pub front_started: Option<u64>,
    /// The program in front is the terminal's own: a shell waiting for a command.
    pub front_is_own: Option<bool>,
    /// When the server last read output from the terminal.
    pub output_at: Option<u64>,
    /// How many clients are attached to it (not this Mac's: it shows none of them).
    pub clients: u32,
    /// Who they are, one entry per client the server named, `None` for one that gave no name.
    pub clients_named: Vec<Option<String>>,
}

/// What a terminal in the background is doing, as far as the server's times say.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Activity {
    /// A program other than the shell is in front, for this long when the server said since when.
    Running(Option<Duration>),
    /// The shell is waiting for a command; the last output was at this second of the server's
    /// clock (the shell's start when it wrote none).
    Idle { since: Option<u64> },
    /// The server did not say (a relay from before the times, a program it could not read).
    Unknown,
}

impl Background {
    /// The names of the other computers this terminal is open on, in the order they attached,
    /// each once: empty for a terminal nobody has open. Only a relay that names its clients is
    /// believed (it says one entry per client it counts; an older one leaves the list short, and
    /// then nothing is known of who is attached) and only a client that gave a name is a computer
    /// — an agent's `kablo wait` is attached too, with none, and the terminal is still nobody's.
    pub fn elsewhere(&self) -> Vec<&str> {
        let counted = usize::try_from(self.clients).unwrap_or(usize::MAX);
        if self.clients_named.len() < counted {
            return Vec::new();
        }
        let mut names: Vec<&str> = Vec::new();
        for name in self
            .clients_named
            .iter()
            .flatten()
            .map(String::as_str)
            .filter(|name| !name.trim().is_empty())
        {
            if !names.contains(&name) {
                names.push(name);
            }
        }
        names
    }

    /// What the terminal is doing at `now`, in seconds of the clock the caller measures against
    /// (the Unix epoch of this computer; the server's own is not known).
    pub fn activity(&self, now: u64) -> Activity {
        activity_at(
            self.front_is_own,
            self.front_started,
            self.started,
            self.output_at,
            now,
        )
    }
}

/// What a terminal is doing at `now` by the times its server states (a status' `front_is_own`,
/// `front_started`, `started` and `output_at`): the one reading of them, for the card's rows
/// ([`Background::activity`]) and for what an agent's `ls` says of any terminal.
pub fn activity_at(
    front_is_own: Option<bool>,
    front_started: Option<u64>,
    started: Option<u64>,
    output_at: Option<u64>,
    now: u64,
) -> Activity {
    match front_is_own {
        Some(false) => Activity::Running(
            front_started.map(|since| Duration::from_secs(now.saturating_sub(since))),
        ),
        Some(true) => Activity::Idle {
            since: output_at.or(started),
        },
        None => Activity::Unknown,
    }
}

/// What the cache holds of one terminal.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Entry {
    running: bool,
    program: Option<String>,
    cwd: Option<String>,
    label: Option<String>,
    started: Option<u64>,
    front_started: Option<u64>,
    front_is_own: Option<bool>,
    output_at: Option<u64>,
    clients: u32,
    clients_named: Vec<Option<String>>,
    /// The counts above were taken after this Mac last attached to the terminal or was let go of
    /// it. Until then they may count this Mac itself (it was attached when the server answered),
    /// and the terminal is not said to be on another computer on their word.
    counted_since: bool,
}

/// Text from the server for a person to read: lossy, and with no control character in it.
fn tidy(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .chars()
        .filter(|c| !c.is_control())
        .collect()
}

fn tidy_some(bytes: &Option<Vec<u8>>) -> Option<String> {
    bytes
        .as_deref()
        .map(tidy)
        .filter(|text| !text.trim().is_empty())
}

impl Entry {
    fn of(status: &Status) -> Entry {
        Entry {
            running: status.state == State::RUNNING,
            program: tidy_some(&status.program),
            cwd: tidy_some(&status.cwd),
            label: status
                .label
                .as_deref()
                .map(|label| tidy(label.as_bytes()))
                .filter(|label| !label.trim().is_empty()),
            started: status.started,
            front_started: status.front_started,
            front_is_own: status.front_is_own,
            output_at: status.output_at,
            clients: status.clients,
            clients_named: status
                .clients_named
                .iter()
                .map(|name| name.as_ref().map(|name| tidy(name.as_str().as_bytes())))
                .collect(),
            counted_since: true,
        }
    }
}

/// The terminals one account's server has, as last heard.
#[derive(Debug, Default)]
pub(super) struct Roster {
    terminals: BTreeMap<SessionId, Entry>,
}

impl Roster {
    /// Takes in what the client said.
    pub(super) fn apply(&mut self, event: &Event) {
        match event {
            Event::Listed(list) => {
                self.terminals = list
                    .iter()
                    .map(|status| (status.id.clone(), Entry::of(status)))
                    .collect();
            }
            Event::Status(status) | Event::Created(status) => {
                self.terminals.insert(status.id.clone(), Entry::of(status));
            }
            // This Mac was let go of the terminal: who is attached changed, and what the server
            // counted before is not about the terminal as it is now.
            Event::Taken { terminal, .. } => self.attached(terminal),
            Event::State { terminal, state } => {
                if let Some(entry) = self.terminals.get_mut(terminal) {
                    entry.program = tidy_some(&state.program);
                    entry.cwd = tidy_some(&state.cwd);
                    // The times described the program that was in front before.
                    entry.front_started = None;
                    entry.front_is_own = None;
                }
            }
            Event::Ended { terminal, .. }
            | Event::Failed {
                terminal: Some(terminal),
                code: ErrorCode::UNKNOWN_SESSION,
                ..
            } => {
                self.terminals.remove(terminal);
            }
            _ => {}
        }
    }

    /// This Mac attached to the terminal: what the server counted of who is attached before is not
    /// believed until it answers again (the count may have this Mac in it). The hub does not pass
    /// the attach on, so the service says it.
    pub(super) fn attached(&mut self, terminal: &SessionId) {
        if let Some(entry) = self.terminals.get_mut(terminal) {
            entry.counted_since = false;
        }
    }

    /// The terminals of `account` (the server `host`) that run and are not `hidden` — a pane of
    /// this Mac shows it, or it was asked to end — in two halves: those in the background (nobody
    /// has them open) and those open on another Mac. Each the most recently written first, then
    /// those that never wrote or whose time is not known, by their start, newest first.
    pub(super) fn split(
        &self,
        account: &str,
        host: &str,
        argv: &[String],
        hidden: impl Fn(&SessionId) -> bool,
    ) -> (Vec<Background>, Vec<Background>) {
        let (elsewhere, background) = self
            .not_shown(account, host, argv, hidden)
            .into_iter()
            .partition(|background| !background.elsewhere().is_empty());
        (background, elsewhere)
    }

    /// The terminals the server lists that are not running any more (nobody has attached since
    /// the program ended) and that are not `hidden`, in the order of their names.
    pub(super) fn exited(&self, hidden: impl Fn(&SessionId) -> bool) -> Vec<SessionId> {
        self.terminals
            .iter()
            .filter(|(id, entry)| !entry.running && !hidden(id))
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// The running terminals no pane of this Mac shows and nobody asked to end, the most recently
    /// written first.
    fn not_shown(
        &self,
        account: &str,
        host: &str,
        argv: &[String],
        hidden: impl Fn(&SessionId) -> bool,
    ) -> Vec<Background> {
        let mut found: Vec<Background> = self
            .terminals
            .iter()
            .filter(|(id, entry)| entry.running && !hidden(id))
            .map(|(id, entry)| Background {
                account: account.to_owned(),
                host: host.to_owned(),
                argv: argv.to_vec(),
                terminal: id.clone(),
                program: entry.program.clone(),
                cwd: entry.cwd.clone(),
                label: entry.label.clone(),
                started: entry.started,
                front_started: entry.front_started,
                front_is_own: entry.front_is_own,
                output_at: entry.output_at,
                // Who is attached is not known between this Mac's attaching and the next answer.
                clients: if entry.counted_since {
                    entry.clients
                } else {
                    0
                },
                clients_named: if entry.counted_since {
                    entry.clients_named.clone()
                } else {
                    Vec::new()
                },
            })
            .collect();
        sort_recent_first(&mut found);
        found
    }
}

/// The order a person remembers terminals in: the last to write first; a terminal that never wrote
/// (or whose time is not known) after those, the one started last first; ties by name so the order
/// is the same every time it is asked.
pub fn sort_recent_first(terminals: &mut [Background]) {
    terminals.sort_by(|a, b| {
        // `None` sorts below every time, so a terminal that never wrote comes last.
        b.output_at
            .cmp(&a.output_at)
            .then(b.started.cmp(&a.started))
            .then_with(|| a.terminal.cmp(&b.terminal))
    });
}

/// What a person opening a new terminal on a server is told when others wait there in the
/// background, in one dim line: how many, and where the card that lists them is. `last` is the
/// terminals the previous line named for this server (sorted, as returned here in `told`); the
/// line comes only when `waiting` is not what it named, so the same news is not told at every
/// `ssh`. `None` when nothing waits or when the line was said for exactly these.
pub fn background_note(
    host: &str,
    waiting: &[Background],
    last: Option<&[SessionId]>,
) -> Option<(String, Vec<SessionId>)> {
    if waiting.is_empty() {
        return None;
    }
    let mut named: Vec<SessionId> = waiting.iter().map(|b| b.terminal.clone()).collect();
    named.sort();
    if last == Some(&named[..]) {
        return None;
    }
    // The name is what the user typed; the line is one line whatever it holds.
    let host: String = host.chars().filter(|c| !c.is_control()).collect();
    let (subject, object) = match named.len() {
        1 => ("1 more terminal is".to_owned(), "it"),
        n => (format!("{n} more terminals are"), "them"),
    };
    let text =
        format!("bateri: {subject} running on {host}; click \u{21c4} {host} below to see {object}");
    Some((text, named))
}

#[cfg(test)]
mod tests;
