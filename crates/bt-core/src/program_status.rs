//! The program status protocol (`OSC 7501`): a program tells the terminal what it
//! is doing — at rest, working, blocked on the user, finished, failed — and the
//! terminal keeps one record per id.
//!
//! **What is kept and what is not.** A record holds its state, when it entered
//! it, and its `app`. `kind`, `progress`, `app`, `title` and `msg` are
//! **validated** (a report that breaks a limit, fails to decode or carries a
//! control character is discarded whole, as the protocol says); `kind`,
//! `progress`, `title` and `msg` are not stored: nothing shows them yet, and a text nobody
//! reads is attack surface for nothing. When a surface shows `msg` or `title` it
//! stores them then, and neutralizes bidirectional and invisible formatting
//! characters on the way out.
//!
//! **`app` is kept to be looked up, never shown** ([`ProgramStatus::app`]): a
//! restricted name of at most 32 bytes (letters, digits, `_ . + -`), which the
//! tab's icon matches against a table of known programs — nothing of it is drawn
//! or printed. A report for an id that already has a record and names no `app`
//! keeps the name the record had (a progress update does not make the program
//! anonymous). The answer is the root record's name, else the most recently
//! updated record's that has one, and it goes with its record: a `clear`, or the
//! next prompt for a record still alive. A `done` or `error` record survives the
//! prompt, so its name does too — the program's result is still on show.
//!
//! **An agent's last reply** is one more text beside the records, not in one: it does not come in
//! an `OSC 7501` report but from the agent's hook through the app channel (`bateri message`), so
//! [`ProgramStatus::set_reply`] is how it enters. It is cleaned like a notification's text
//! ([`crate::notify`]: control, bidirectional and invisible characters go) and held while a record
//! is: it goes with the last one, at the shell's prompt (`prompt`) or at `state=clear`, which is
//! when the program ended. A `done` or `error` record survives the prompt, and so does the reply
//! with it. Text that arrives with no record to hold it is dropped, so a reply never outlives the
//! program that said it. It is shown beside the tab ([`ProgramReport::reply`]).
//!
//! **`OSC 9 ; 4` is the root record's stand-in** until the first `OSC 7501` report
//! arrives ([`progress`]): ConEmu's progress sequence — what `cargo`, `zig` and
//! `winget` print — says "working" and "failed" and nothing else, and a program that
//! speaks the richer protocol must not be second-guessed by the poorer one.
//!
//! **Not in `Term`.** The records live in the shell ledger beside the other
//! terminal-owned state, so no `Session` method that changes the grid is involved. They
//! are rebuilt from the bytes when a terminal is shown again: a program re-reports on its
//! next change, and until then the tab shows what the shell's own marks say.
//!
//! **What reads them.** [`ProgramStatus::activity`] — the tab's indicator: a
//! program that reports owns the "running" ring (it spins while the program works
//! and stands still while it waits for the user), a blocked program is a question
//! the user cannot see, and a record entering `done` or `error` counts as an end
//! the way a command's `D` does.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use crate::shell::decode_base64;

/// The OSC number. Not our choice: the protocol's.
pub(crate) const STATUS_OSC: u32 = 7501;

/// The upper bound of the `ESC ] 7501 ;` payload, in bytes: the protocol's limit for
/// the whole sequence (4096), taken whole — the introducer and terminator it counts
/// are a few bytes, and a payload this long is far past any real report. A report over
/// it is dropped and the scanner skips to the terminator.
pub(crate) const STATUS_PAYLOAD_LIMIT: usize = 4096;

/// How many records the terminal keeps; the least recently updated goes first. The
/// protocol asks terminals for at least 64 and allows 256.
const MAX_RECORDS: usize = 256;
/// `id`: total bytes, bytes per segment, levels.
const MAX_ID: usize = 128;
const MAX_SEGMENT: usize = 32;
const MAX_DEPTH: usize = 8;
/// `app`, bytes. `title` and `msg`, bytes **decoded**.
const MAX_APP: usize = 32;
const MAX_TITLE: usize = 192;
const MAX_MSG: usize = 2048;
/// The agent's last reply, bytes after cleaning. `bateri message` cuts to the same length before
/// it sends, so this is the bound of a sender that does not.
const MAX_REPLY: usize = 1000;

/// The progress sequence's OSC number: iTerm2's `OSC 9` is a notification and
/// ConEmu's `OSC 9 ; 4 ; state ; percent` a progress bar, told apart by the `4;`.
pub(crate) const PROGRESS_OSC: u32 = 9;

/// The upper bound of the `ESC ] 9 ;` payload, in bytes. `4;3;100` is seven; the
/// arm also carries iTerm2's notification text ([`crate::notify::osc9`]), so the bound
/// is the notification's — past it the sequence is dropped and skipped to its
/// terminator, silently.
pub(crate) const PROGRESS_PAYLOAD_LIMIT: usize = crate::notify::NOTIFY_PAYLOAD_LIMIT;

/// What the terminal answers to `OSC 7501 ; ?`: the same body, so a program that
/// asked knows the terminal reads reports. Fixed bytes — nothing of the stream is
/// in it, so it is the same class of answer as a device-attributes reply.
pub(crate) const SUPPORT_REPLY: &str = "\x1b]7501;?\x1b\\";

/// A record's state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    /// At rest, waiting for the user's next instruction.
    Idle,
    Working,
    /// Cannot continue until the user acts.
    Blocked,
    /// Finished, with a result the user has not seen.
    Done,
    /// Failed and stopped.
    Error,
}

impl State {
    fn parse(value: &[u8]) -> Option<Self> {
        Some(match value {
            b"idle" => Self::Idle,
            b"working" => Self::Working,
            b"blocked" => Self::Blocked,
            b"done" => Self::Done,
            b"error" => Self::Error,
            _ => return None,
        })
    }

    /// The program is still there: it is not a result waiting to be seen.
    fn live(self) -> bool {
        matches!(self, Self::Idle | Self::Working | Self::Blocked)
    }
}

/// A parsed report.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Report {
    /// `OSC 7501 ; ?` — the program asks whether the terminal reads reports.
    Query,
    /// `state=clear`: the addressed record and its children go; no id → all.
    Clear { id: String },
    /// A record, replaced whole.
    Set {
        id: String,
        state: State,
        /// The validated `app` name ([`MAX_APP`] bytes of [`name_byte`]s);
        /// `None` when the report gives none or an empty one.
        app: Option<String>,
        /// The decoded `title`: what the program calls the work, shown as its
        /// name in the side column's Activity.
        title: Option<String>,
        /// The decoded `msg`: what a blocked program asks the user.
        msg: Option<String>,
    },
}

/// Reads the payload after `7501;`; `None` for what must be ignored — a malformed
/// report is dropped whole, never half-applied.
///
/// Pairs are `key=value` separated by `:`. A pair without `=` is skipped, so is an
/// unknown key; the last of a repeated key wins; an unrecognized `state` drops the
/// report (a newer protocol's state is not ours to guess).
pub(crate) fn parse(payload: &[u8], decoded: &mut Vec<u8>) -> Option<Report> {
    if payload.starts_with(b"?") {
        return Some(Report::Query);
    }
    let mut state: Option<&[u8]> = None;
    let mut id: Option<&[u8]> = None;
    let mut kind: Option<&[u8]> = None;
    let mut progress: Option<&[u8]> = None;
    let mut app: Option<&[u8]> = None;
    let mut title: Option<&[u8]> = None;
    let mut msg: Option<&[u8]> = None;
    for pair in payload.split(|&byte| byte == b':') {
        let Some(at) = pair.iter().position(|&byte| byte == b'=') else {
            continue;
        };
        let (key, value) = (&pair[..at], &pair[at + 1..]);
        match key {
            b"state" => state = Some(value),
            b"id" => id = Some(value),
            b"kind" => kind = Some(value),
            b"progress" => progress = Some(value),
            b"app" => app = Some(value),
            b"title" => title = Some(value),
            b"msg" => msg = Some(value),
            _ => {}
        }
    }
    let state = state?;
    let id = match id {
        Some(value) => path(value)?,
        None => String::new(),
    };
    // The validated-but-not-stored keys: a bad one drops the report, as the
    // protocol says, so a terminal that stores them later accepts the same set.
    if let Some(value) = kind
        && !matches!(value, b"permission" | b"question" | b"auth")
    {
        return None;
    }
    if let Some(value) = progress {
        percent(value)?;
    }
    let app = match app {
        Some(value) if !(value.len() <= MAX_APP && value.iter().copied().all(name_byte)) => {
            return None;
        }
        // Every byte is ASCII by the check above.
        Some(value) if !value.is_empty() => String::from_utf8(value.to_vec()).ok(),
        _ => None,
    };
    // A text that is there must be valid, or the report drops: read both
    // before `clear`, which takes neither.
    let mut read_text = |value: Option<&[u8]>, limit| match value {
        Some(value) => {
            text(value, limit, decoded)?;
            Some(Some(String::from_utf8_lossy(decoded).into_owned()))
        }
        None => Some(None),
    };
    let title = read_text(title, MAX_TITLE)?;
    let msg = read_text(msg, MAX_MSG)?;
    if state == b"clear" {
        return Some(Report::Clear { id });
    }
    Some(Report::Set {
        id,
        state: State::parse(state)?,
        app,
        title: title.filter(|title| !title.is_empty()),
        msg: msg.filter(|msg| !msg.is_empty()),
    })
}

/// `progress`: an integer from 0 to 100; `None` for anything else.
fn percent(value: &[u8]) -> Option<u8> {
    if !(1..=3).contains(&value.len()) || !value.iter().all(u8::is_ascii_digit) {
        return None;
    }
    std::str::from_utf8(value)
        .ok()?
        .parse::<u8>()
        .ok()
        .filter(|&number| number <= 100)
}

/// Reads the payload after `9;` as ConEmu's progress report and maps it onto the
/// root record: `0` clears it; `1` (a value), `3` (indeterminate) and `4`
/// (paused, with or without a value) are work; `2` (the red bar) is a failure.
/// `None` for what is not a progress report — a notification text, an unknown
/// state, a missing one. The percentage is not read: the bar's state is what the
/// ring shows.
pub(crate) fn progress(payload: &[u8]) -> Option<Report> {
    let mut fields = payload.split(|&byte| byte == b';');
    if fields.next()? != b"4" {
        return None;
    }
    let root = String::new;
    let state = match fields.next()? {
        b"0" => return Some(Report::Clear { id: root() }),
        b"1" | b"3" | b"4" => State::Working,
        b"2" => State::Error,
        _ => return None,
    };
    Some(Report::Set {
        id: root(),
        state,
        app: None,
        title: None,
        msg: None,
    })
}

/// A byte of a restricted name (an `app`, an id's segment).
fn name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'+' | b'-')
}

/// A slash-separated id within the protocol's limits; the root is the empty string.
fn path(value: &[u8]) -> Option<String> {
    if value.len() > MAX_ID {
        return None;
    }
    let mut depth = 0;
    for segment in value.split(|&byte| byte == b'/') {
        depth += 1;
        if depth > MAX_DEPTH
            || segment.is_empty()
            || segment.len() > MAX_SEGMENT
            || !segment.iter().copied().all(name_byte)
        {
            return None;
        }
    }
    // Every byte is ASCII by the checks above.
    String::from_utf8(value.to_vec()).ok()
}

/// A base64 text within `limit` decoded bytes, valid UTF-8 and free of control
/// characters; `None` otherwise.
fn text(value: &[u8], limit: usize, decoded: &mut Vec<u8>) -> Option<()> {
    decoded.clear();
    decode_base64(value, decoded)?;
    if decoded.len() > limit {
        return None;
    }
    let text = std::str::from_utf8(decoded).ok()?;
    (!text.chars().any(char::is_control)).then_some(())
}

/// One record.
#[derive(Clone, Debug)]
struct Record {
    state: State,
    /// The program's name as a report gave it, kept to be looked up
    /// ([`ProgramStatus::app`]); a report that names none keeps the last one.
    app: Option<String>,
    /// When the record entered its state: a report that repeats the state (a
    /// progress update) keeps it, so a ring's clock and the counter run on.
    since: Instant,
    /// What the program calls the work; a report that names none keeps the
    /// last one, as `app` does.
    title: Option<String>,
    /// What the program asks while blocked; gone with the blocked state.
    msg: Option<String>,
    /// The update's number; the smallest is evicted first.
    updated: u64,
}

/// What a tab reads: the reporting program's side of its indicator
/// ([`crate::Activity::program`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProgramActivity {
    /// How long the longest-working record has worked; `None` when nothing works
    /// (the program is idle or waiting for the user).
    pub working: Option<Duration>,
    /// A record waits for the user.
    pub blocked: bool,
}

/// What a pane's reporting program says of itself ([`ProgramStatus::report`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProgramReport {
    /// The state the user sees.
    pub state: State,
    /// How long it has been in it: a duration, not an instant, so a reader
    /// with another clock compares nothing.
    pub held: Duration,
    /// What the program calls the work.
    pub title: Option<String>,
    /// What a blocked program asks.
    pub message: Option<String>,
    /// The last thing an agent in the pane said ([`ProgramStatus::set_reply`]).
    pub reply: Option<String>,
}

/// The terminal's records, by id.
#[derive(Debug, Default)]
pub(crate) struct ProgramStatus {
    records: BTreeMap<String, Record>,
    /// The update counter ([`Record::updated`]).
    clock: u64,
    /// An `OSC 7501` report arrived: the program speaks the protocol, and the
    /// progress sequence is no longer read ([`Self::apply_progress`]).
    native: bool,
    /// How many times a record **entered** `done` / `error`: counts that only
    /// grow, which the tab compares between two looks like a command's ends.
    pub(crate) done: u64,
    pub(crate) failed: u64,
    /// What the agent last said, cleaned; held while a record is.
    reply: Option<String>,
}

impl ProgramStatus {
    /// Applies a progress report ([`progress`]) unless the terminal has met
    /// `OSC 7501` since; `true` if anything the tab reads may have changed.
    pub(crate) fn apply_progress(&mut self, report: Report, now: Instant) -> bool {
        !self.native && self.apply_report(report, now)
    }

    /// Applies an `OSC 7501` report; `true` if anything the tab reads may have
    /// changed.
    pub(crate) fn apply(&mut self, report: Report, now: Instant) -> bool {
        if !matches!(report, Report::Query) {
            self.native = true;
        }
        self.apply_report(report, now)
    }

    fn apply_report(&mut self, report: Report, now: Instant) -> bool {
        match report {
            Report::Query => false,
            Report::Clear { id } => self.clear(&id),
            Report::Set {
                id,
                state,
                app,
                title,
                msg,
            } => {
                self.clock += 1;
                let (entered, since) = match self.records.get(&id) {
                    Some(record) if record.state == state => (false, record.since),
                    _ => (true, now),
                };
                let app = app.or_else(|| self.records.get(&id).and_then(|r| r.app.clone()));
                let title = title.or_else(|| self.records.get(&id).and_then(|r| r.title.clone()));
                let msg = msg.filter(|_| state == State::Blocked);
                if entered {
                    match state {
                        State::Done => self.done = self.done.wrapping_add(1),
                        State::Error => self.failed = self.failed.wrapping_add(1),
                        State::Idle | State::Working | State::Blocked => {}
                    }
                }
                self.records.insert(
                    id,
                    Record {
                        state,
                        app,
                        since,
                        title,
                        msg,
                        updated: self.clock,
                    },
                );
                self.evict();
                true
            }
        }
    }

    /// A new shell prompt began: what was running there has ended. Working,
    /// blocked and idle records go; `done` and `error` stay — they are results
    /// the user has not seen. `true` if anything went.
    pub(crate) fn prompt(&mut self) -> bool {
        let before = self.records.len();
        self.records.retain(|_, record| !record.state.live());
        self.drop_reply_when_alone();
        self.records.len() != before
    }

    /// The agent said something (`bateri message`): kept, cleaned, for [`Self::report`]. `true` if
    /// what the tab reads changed. Nothing is kept while no record is there to hold it, and an
    /// empty text (nothing left once cleaned) leaves the last reply as it was.
    pub(crate) fn set_reply(&mut self, text: &str) -> bool {
        if self.records.is_empty() {
            return false;
        }
        let Some(text) = crate::notify::clean(text.as_bytes(), MAX_REPLY) else {
            return false;
        };
        if self.reply.as_deref() == Some(text.as_str()) {
            return false;
        }
        self.reply = Some(text);
        true
    }

    /// The reply leaves with the last record.
    fn drop_reply_when_alone(&mut self) {
        if self.records.is_empty() {
            self.reply = None;
        }
    }

    /// `state=clear`: the record and its children (`build` takes `build/test`),
    /// or all of them for the empty id.
    fn clear(&mut self, id: &str) -> bool {
        let before = self.records.len();
        if id.is_empty() {
            self.records.clear();
        } else {
            self.records.retain(|key, _| {
                key != id
                    && !key
                        .strip_prefix(id)
                        .is_some_and(|rest| rest.starts_with('/'))
            });
        }
        self.drop_reply_when_alone();
        self.records.len() != before
    }

    fn evict(&mut self) {
        while self.records.len() > MAX_RECORDS {
            let Some(oldest) = self
                .records
                .iter()
                .min_by_key(|(_, record)| record.updated)
                .map(|(key, _)| key.clone())
            else {
                return;
            };
            self.records.remove(&oldest);
        }
    }

    /// The reporting program's name, to be looked up in a table and never
    /// shown: the root record's, else the most recently updated record's
    /// that carries one; `None` when no record does. It leaves with its record.
    pub(crate) fn app(&self) -> Option<&str> {
        let root = self
            .records
            .get("")
            .and_then(|record| record.app.as_deref());
        root.or_else(|| {
            self.records
                .values()
                .filter(|record| record.app.is_some())
                .max_by_key(|record| record.updated)
                .and_then(|record| record.app.as_deref())
        })
    }

    /// What the pane's program says of itself, for the side column's Activity
    /// and the tab's details: the record that decides what the user sees — a
    /// question first, then work, a failure, a finish, rest — with how long it
    /// has been so, the work's title (that record's, else the newest one's),
    /// what a blocked program asks and what the agent last said. `None` while
    /// no record is there.
    pub(crate) fn report(&self, now: Instant) -> Option<ProgramReport> {
        let rank = |state: State| match state {
            State::Blocked => 4,
            State::Working => 3,
            State::Error => 2,
            State::Done => 1,
            State::Idle => 0,
        };
        // The highest state; among equals the one that entered it first, so a
        // long piece of work and a long wait read as long.
        let decisive = self.records.values().max_by(|a, b| {
            rank(a.state)
                .cmp(&rank(b.state))
                .then(b.since.cmp(&a.since))
        })?;
        let title = decisive.title.clone().or_else(|| {
            self.records
                .values()
                .filter(|record| record.title.is_some())
                .max_by_key(|record| record.updated)
                .and_then(|record| record.title.clone())
        });
        Some(ProgramReport {
            state: decisive.state,
            held: now.saturating_duration_since(decisive.since),
            title,
            message: decisive.msg.clone(),
            reply: self.reply.clone(),
        })
    }

    /// What the tab reads; `None` while no program reports being there (no record,
    /// or only results waiting to be seen) — the shell's own marks are then the
    /// whole story.
    pub(crate) fn activity(&self, now: Instant) -> Option<ProgramActivity> {
        let mut live = false;
        let mut working: Option<Duration> = None;
        let mut blocked = false;
        for record in self.records.values() {
            live |= record.state.live();
            match record.state {
                State::Working => {
                    let elapsed = now.saturating_duration_since(record.since);
                    working = Some(working.map_or(elapsed, |longest| longest.max(elapsed)));
                }
                State::Blocked => blocked = true,
                State::Idle | State::Done | State::Error => {}
            }
        }
        live.then_some(ProgramActivity { working, blocked })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(body: &str) -> Option<Report> {
        parse(body.as_bytes(), &mut Vec::new())
    }

    fn set(id: &str, state: State) -> Report {
        Report::Set {
            id: id.to_owned(),
            state,
            app: None,
            title: None,
            msg: None,
        }
    }

    fn named(id: &str, state: State, app: &str) -> Report {
        Report::Set {
            id: id.to_owned(),
            state,
            app: Some(app.to_owned()),
            title: None,
            msg: None,
        }
    }

    #[test]
    fn a_record_keeps_its_title_and_a_blocked_one_its_question() {
        // "Refactor" / "Allow edit?" / "Next".
        let now = Instant::now();
        let later = now + Duration::from_secs(90);
        let mut status = ProgramStatus::default();
        let report = |body: &str| read(body).expect("a report");
        status.apply(report("state=working:title=UmVmYWN0b3I="), now);
        let working = status.report(later).expect("a record");
        assert_eq!(working.state, State::Working);
        assert_eq!(working.title.as_deref(), Some("Refactor"));
        assert_eq!(working.message, None);
        assert_eq!(working.held, Duration::from_secs(90));
        // A question: its text is kept, the title stays from before.
        status.apply(report("state=blocked:msg=QWxsb3cgZWRpdD8="), later);
        let asking = status.report(later).expect("a record");
        assert_eq!(asking.state, State::Blocked);
        assert_eq!(asking.title.as_deref(), Some("Refactor"));
        assert_eq!(asking.message.as_deref(), Some("Allow edit?"));
        assert_eq!(asking.held, Duration::ZERO, "a new state starts its clock");
        // Out of the question, the question goes; a msg off the blocked state is not kept.
        status.apply(report("state=working:msg=QWxsb3cgZWRpdD8="), later);
        assert_eq!(status.report(later).expect("a record").message, None);
        // A new title replaces the old one.
        status.apply(report("state=idle:title=TmV4dA=="), later);
        assert_eq!(
            status.report(later).expect("a record").title.as_deref(),
            Some("Next")
        );
        // A question in another record outranks this one's rest.
        status.apply(report("state=blocked:id=sub"), later);
        assert_eq!(
            status.report(later).expect("a record").state,
            State::Blocked
        );
        status.apply(report("state=clear"), later);
        assert_eq!(status.report(later), None);
    }

    #[test]
    fn an_agents_reply_is_held_beside_the_records_and_goes_with_the_last() {
        let now = Instant::now();
        let mut status = ProgramStatus::default();
        // With no record to hold it, a reply is not kept.
        assert!(!status.set_reply("Done."));
        status.apply(set("", State::Working), now);
        assert_eq!(status.report(now).expect("a record").reply, None);
        assert!(status.set_reply("Added a limiter and 12 tests."));
        assert_eq!(
            status.report(now).expect("a record").reply.as_deref(),
            Some("Added a limiter and 12 tests.")
        );
        // The same text again changes nothing the tab reads; a new one replaces the old.
        assert!(!status.set_reply("Added a limiter and 12 tests."));
        assert!(status.set_reply("The Redis fallback is still missing."));
        // The state changes under it: the reply stays (a new turn has not answered yet).
        status.apply(set("", State::Idle), now);
        assert_eq!(
            status.report(now).expect("a record").reply.as_deref(),
            Some("The Redis fallback is still missing.")
        );
        // A prompt takes the live records, and the reply with the last of them.
        assert!(status.prompt());
        assert_eq!(status.report(now), None);
        assert!(!status.set_reply("late"), "no record, no reply");
        status.apply(set("", State::Idle), now);
        assert_eq!(status.report(now).expect("a record").reply, None);
    }

    #[test]
    fn a_reply_stays_while_a_result_waits_and_goes_when_it_is_cleared() {
        let now = Instant::now();
        let mut status = ProgramStatus::default();
        status.apply(set("", State::Done), now);
        assert!(status.set_reply("All green."));
        // A `done` record survives the prompt, so its reply does.
        assert!(!status.prompt());
        assert_eq!(
            status.report(now).expect("a record").reply.as_deref(),
            Some("All green.")
        );
        // Another record going leaves the reply while one is left.
        status.apply(set("build", State::Working), now);
        assert!(status.prompt());
        assert_eq!(
            status.report(now).expect("a record").reply.as_deref(),
            Some("All green.")
        );
        status.apply(set("build", State::Working), now);
        assert!(status.apply(Report::Clear { id: "build".into() }, now));
        assert_eq!(
            status.report(now).expect("a record").reply.as_deref(),
            Some("All green.")
        );
        // `state=clear` of the rest takes it.
        assert!(status.apply(Report::Clear { id: String::new() }, now));
        status.apply(set("", State::Idle), now);
        assert_eq!(status.report(now).expect("a record").reply, None);
    }

    #[test]
    fn a_reply_is_cleaned_trimmed_and_bounded() {
        let now = Instant::now();
        let mut status = ProgramStatus::default();
        status.apply(set("", State::Idle), now);
        // Control characters, bidirectional overrides and invisible ones go.
        assert!(status.set_reply("  a\u{1b}[31mb \u{202e}c\u{200b}d\te\n  "));
        assert_eq!(
            status.report(now).expect("a record").reply.as_deref(),
            Some("a[31mb cde")
        );
        // Nothing left once cleaned: the last reply stays.
        assert!(!status.set_reply(" \u{1b}\u{202e}\n "));
        assert!(!status.set_reply(""));
        assert_eq!(
            status.report(now).expect("a record").reply.as_deref(),
            Some("a[31mb cde")
        );
        // Longer than the bound is cut at a character, never in one.
        assert!(status.set_reply(&"\u{20ac}".repeat(600)));
        let reply = status
            .report(now)
            .expect("a record")
            .reply
            .expect("a reply");
        assert_eq!(reply, "\u{20ac}".repeat(333));
        assert!(reply.len() <= MAX_REPLY);
    }

    #[test]
    fn the_query_is_a_question_mark() {
        assert_eq!(read("?"), Some(Report::Query));
        assert_eq!(read("?junk"), Some(Report::Query));
    }

    #[test]
    fn a_report_needs_a_state_and_the_root_has_no_id() {
        assert_eq!(read("state=working"), Some(set("", State::Working)));
        assert_eq!(read("app=cargo"), None, "no state");
        assert_eq!(read("state=sleeping"), None, "an unknown state drops it");
        assert_eq!(read(""), None);
    }

    #[test]
    fn unknown_keys_and_pairs_without_a_value_are_skipped() {
        assert_eq!(
            read("state=idle:future=1:junk:app=claude-code"),
            Some(named("", State::Idle, "claude-code"))
        );
        assert_eq!(read("state=idle:future=1:junk"), Some(set("", State::Idle)));
    }

    #[test]
    fn the_last_of_a_repeated_key_wins() {
        assert_eq!(read("state=idle:state=done"), Some(set("", State::Done)));
    }

    #[test]
    fn an_id_is_a_bounded_path() {
        assert_eq!(
            read("state=working:id=build/test"),
            Some(set("build/test", State::Working))
        );
        assert_eq!(read("state=working:id=a//b"), None, "an empty segment");
        assert_eq!(read("state=working:id=/a"), None);
        assert_eq!(read("state=working:id=a/"), None);
        assert_eq!(read("state=working:id=a b"), None, "outside the charset");
        let deep = ["a"; MAX_DEPTH + 1].join("/");
        assert_eq!(read(&format!("state=working:id={deep}")), None, "9 levels");
        let fine = ["a"; MAX_DEPTH].join("/");
        assert!(
            read(&format!("state=working:id={fine}")).is_some(),
            "8 levels"
        );
        let long = "x".repeat(MAX_SEGMENT + 1);
        assert_eq!(read(&format!("state=working:id={long}")), None);
        let wide = vec!["x".repeat(MAX_SEGMENT); 5].join("/");
        assert_eq!(
            read(&format!("state=working:id={wide}")),
            None,
            "over 128 bytes"
        );
    }

    #[test]
    fn the_app_is_read_whole_and_an_empty_one_is_none() {
        assert_eq!(
            read("state=working:app=Claude.Code_2+x"),
            Some(named("", State::Working, "Claude.Code_2+x"))
        );
        assert_eq!(read("state=working:app="), Some(set("", State::Working)));
        let longest = "a".repeat(MAX_APP);
        assert_eq!(
            read(&format!("state=working:app={longest}")),
            Some(named("", State::Working, &longest))
        );
        assert_eq!(read("state=working:app=a b"), None, "outside the charset");
        assert_eq!(read("state=working:app=a/b"), None, "no path");
    }

    #[test]
    fn the_app_is_kept_with_its_record_and_goes_with_it() {
        let mut status = ProgramStatus::default();
        let now = Instant::now();
        assert_eq!(status.app(), None, "nothing reported");
        status.apply(set("", State::Working), now);
        assert_eq!(status.app(), None, "a report that names none");
        status.apply(named("", State::Working, "claude-code"), now);
        assert_eq!(status.app(), Some("claude-code"));
        // A progress update that names none keeps the name.
        status.apply(read("state=working:progress=40").unwrap(), now);
        assert_eq!(status.app(), Some("claude-code"));
        // Another name replaces it.
        status.apply(named("", State::Idle, "codex"), now);
        assert_eq!(status.app(), Some("codex"));
        status.apply(Report::Clear { id: String::new() }, now);
        assert_eq!(status.app(), None, "cleared with its record");
    }

    #[test]
    fn the_root_names_the_program_else_the_latest_record_that_has_a_name() {
        let mut status = ProgramStatus::default();
        let now = Instant::now();
        status.apply(named("a", State::Working, "first"), now);
        status.apply(named("b", State::Working, "second"), now);
        status.apply(set("c", State::Working), now);
        assert_eq!(status.app(), Some("second"), "the latest with a name");
        status.apply(named("", State::Idle, "root"), now);
        assert_eq!(status.app(), Some("root"), "the root outranks the latest");
        status.apply(Report::Clear { id: "b".into() }, now);
        status.apply(Report::Clear { id: String::new() }, now);
        assert_eq!(status.app(), None);
    }

    #[test]
    fn a_prompt_ends_the_name_of_a_live_record_but_not_of_a_result() {
        let mut status = ProgramStatus::default();
        let now = Instant::now();
        status.apply(named("", State::Working, "claude"), now);
        assert!(status.prompt());
        assert_eq!(status.app(), None, "the program ended with its record");
        status.apply(named("", State::Done, "claude"), now);
        assert!(!status.prompt());
        assert_eq!(status.app(), Some("claude"), "its result is still on show");
    }

    #[test]
    fn the_progress_sequence_names_no_program() {
        let mut status = ProgramStatus::default();
        status.apply_progress(progress(b"4;1;50").unwrap(), Instant::now());
        assert_eq!(status.app(), None);
    }

    #[test]
    fn the_keys_the_terminal_does_not_keep_are_still_checked() {
        assert!(read("state=blocked:kind=permission:progress=40").is_some());
        assert_eq!(read("state=blocked:kind=nap"), None);
        assert_eq!(read("state=working:progress=101"), None);
        assert_eq!(read("state=working:progress=-1"), None);
        assert_eq!(read("state=working:progress=x"), None);
        assert_eq!(read("state=working:progress="), None);
        let app = "a".repeat(MAX_APP + 1);
        assert_eq!(read(&format!("state=idle:app={app}")), None);
    }

    #[test]
    fn free_text_is_base64_without_control_characters() {
        // "hi there" / "a\nb" / invalid UTF-8 (0xff).
        assert!(read("state=blocked:msg=aGkgdGhlcmU=").is_some());
        assert_eq!(read("state=blocked:msg=YQpi"), None, "a newline");
        assert_eq!(read("state=blocked:msg=/w=="), None, "not UTF-8");
        assert_eq!(read("state=blocked:msg=!!!!"), None, "not base64");
        assert!(read("state=idle:title=QQ==").is_some(), "one byte is fine");
        let big = "QUFB".repeat(MAX_MSG / 3 + 2);
        assert_eq!(
            read(&format!("state=idle:msg={big}")),
            None,
            "over 2048 decoded"
        );
    }

    #[test]
    fn the_progress_sequence_maps_onto_the_root_record() {
        let root = |state| {
            Some(Report::Set {
                id: String::new(),
                state,
                app: None,
                title: None,
                msg: None,
            })
        };
        assert_eq!(progress(b"4;1;50"), root(State::Working));
        assert_eq!(progress(b"4;3"), root(State::Working), "indeterminate");
        assert_eq!(
            progress(b"4;4;30"),
            root(State::Working),
            "paused is not over"
        );
        assert_eq!(progress(b"4;2;80"), root(State::Error), "the red bar");
        assert_eq!(
            progress(b"4;1;150"),
            root(State::Working),
            "a value out of range does not drop the bar"
        );
        assert_eq!(progress(b"4;1"), root(State::Working));
        assert_eq!(progress(b"4;0"), Some(Report::Clear { id: String::new() }));
        assert_eq!(progress(b"4"), None, "no state");
        assert_eq!(progress(b"4;9"), None, "an unknown state");
        assert_eq!(progress(b"build finished"), None, "a notification's text");
        assert_eq!(progress(b"40;1"), None, "not the `4` field");
    }

    #[test]
    fn the_progress_sequence_stands_in_only_until_a_program_reports() {
        let now = Instant::now();
        let mut status = ProgramStatus::default();
        let work = progress(b"4;1;10").unwrap();
        assert!(status.apply_progress(work.clone(), now));
        assert!(status.activity(now).is_some_and(|a| a.working.is_some()));
        assert!(status.apply_progress(progress(b"4;0").unwrap(), now));
        assert_eq!(status.activity(now), None, "the bar was hidden");
        status.apply(Report::Query, now);
        assert!(
            status.apply_progress(work.clone(), now),
            "a question is no report"
        );
        status.apply(set("", State::Idle), now);
        assert!(
            !status.apply_progress(work, now),
            "the program speaks for itself"
        );
        assert_eq!(
            status.activity(now).map(|a| a.working),
            Some(None),
            "idle stays idle"
        );
    }

    #[test]
    fn clear_takes_the_record_and_its_children() {
        let now = Instant::now();
        let mut status = ProgramStatus::default();
        for (id, state) in [
            ("", State::Working),
            ("build", State::Working),
            ("build/test", State::Blocked),
            ("builder", State::Working),
        ] {
            status.apply(set(id, state), now);
        }
        assert!(status.apply(Report::Clear { id: "build".into() }, now));
        assert!(status.records.contains_key(""));
        assert!(
            status.records.contains_key("builder"),
            "a prefix is not a parent"
        );
        assert!(!status.records.contains_key("build"));
        assert!(!status.records.contains_key("build/test"));
        assert!(status.apply(Report::Clear { id: String::new() }, now));
        assert!(status.records.is_empty());
        assert!(
            !status.apply(Report::Clear { id: String::new() }, now),
            "nothing to clear"
        );
    }

    #[test]
    fn a_report_replaces_its_record_and_a_repeated_state_keeps_the_clock() {
        let start = Instant::now();
        let mut status = ProgramStatus::default();
        status.apply(set("", State::Working), start);
        let later = start + Duration::from_secs(5);
        status.apply(set("", State::Working), later);
        assert_eq!(
            status.activity(later).and_then(|activity| activity.working),
            Some(Duration::from_secs(5)),
            "a progress update does not restart the ring's clock"
        );
        status.apply(set("", State::Blocked), later);
        status.apply(set("", State::Working), later);
        assert_eq!(
            status.activity(later).and_then(|activity| activity.working),
            Some(Duration::ZERO),
            "a new stretch of work starts one"
        );
    }

    #[test]
    fn a_program_that_waits_reports_no_work_and_a_blocked_one_says_so() {
        let now = Instant::now();
        let mut status = ProgramStatus::default();
        assert_eq!(status.activity(now), None, "nothing reports");
        status.apply(set("", State::Idle), now);
        assert_eq!(
            status.activity(now),
            Some(ProgramActivity {
                working: None,
                blocked: false
            }),
            "idle: the program is there and not working"
        );
        status.apply(set("agent", State::Blocked), now);
        assert_eq!(
            status.activity(now),
            Some(ProgramActivity {
                working: None,
                blocked: true
            })
        );
    }

    #[test]
    fn done_and_error_are_counted_once_each_time_they_are_entered() {
        let now = Instant::now();
        let mut status = ProgramStatus::default();
        status.apply(set("", State::Done), now);
        status.apply(set("", State::Done), now);
        assert_eq!(
            (status.done, status.failed),
            (1, 0),
            "a repeat is not an end"
        );
        status.apply(set("", State::Working), now);
        status.apply(set("", State::Error), now);
        status.apply(set("", State::Done), now);
        assert_eq!((status.done, status.failed), (2, 1));
        assert_eq!(
            status.activity(now),
            None,
            "a result waiting to be seen is not a program that is there"
        );
    }

    #[test]
    fn a_prompt_ends_what_was_running_and_keeps_the_results() {
        let now = Instant::now();
        let mut status = ProgramStatus::default();
        status.apply(set("a", State::Working), now);
        status.apply(set("b", State::Blocked), now);
        status.apply(set("c", State::Idle), now);
        status.apply(set("d", State::Done), now);
        status.apply(set("e", State::Error), now);
        assert!(status.prompt());
        assert_eq!(status.records.len(), 2);
        assert!(!status.prompt(), "nothing left to end");
    }

    #[test]
    fn the_oldest_update_is_evicted_past_the_cap() {
        let now = Instant::now();
        let mut status = ProgramStatus::default();
        for index in 0..=MAX_RECORDS {
            status.apply(set(&format!("r{index}"), State::Working), now);
        }
        assert_eq!(status.records.len(), MAX_RECORDS);
        assert!(!status.records.contains_key("r0"));
        assert!(status.records.contains_key(&format!("r{MAX_RECORDS}")));
    }
}
