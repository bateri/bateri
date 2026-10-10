//! The program status protocol (`OSC 7501`): a program tells the terminal what it
//! is doing — at rest, working, blocked on the user, finished, failed — and the
//! terminal keeps one record per id.
//!
//! **What is kept and what is not.** A record holds its state, when it entered
//! it, and its `progress` — a number from 0 to 100 a host embedding the pane shows
//! ([`crate::Session::program_records`]). `kind`, `app`, `title` and `msg` are
//! **validated** (a report that breaks a limit, fails to decode or carries a
//! control character is discarded whole, as the protocol says) but not stored:
//! nothing shows them yet, and a text nobody reads is attack surface for nothing.
//! When a surface shows `msg` or `title` it stores them then, and neutralizes
//! bidirectional and invisible formatting characters on the way out.
//!
//! **`OSC 9 ; 4` is the root record's stand-in** until the first `OSC 7501` report
//! arrives ([`progress`]): ConEmu's progress sequence — what `cargo`, `zig` and
//! `winget` print — says "working" and "failed" and nothing else, and a program that
//! speaks the richer protocol must not be second-guessed by the poorer one.
//!
//! **Not journaled, not handed over.** The records live in the shell ledger beside
//! the other terminal-owned state, not in `Term`, so no `Session` method that
//! changes the grid is involved. They are not carried across an update's handover:
//! a program re-reports on its next change, and until then the tab shows what the
//! shell's own marks say.
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

/// The progress sequence's OSC number: iTerm2's `OSC 9` is a notification and
/// ConEmu's `OSC 9 ; 4 ; state ; percent` a progress bar, told apart by the `4;`.
pub(crate) const PROGRESS_OSC: u32 = 9;

/// The upper bound of the `ESC ] 9 ;` payload, in bytes. `4;3;100` is seven; the
/// arm also meets iTerm2's notification text, which is unbounded and not ours — past
/// the bound it is dropped and skipped to its terminator, silently.
pub(crate) const PROGRESS_PAYLOAD_LIMIT: usize = 64;

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
        /// From 0 to 100; `None` when the report gives none.
        progress: Option<u8>,
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
    let progress = match progress {
        Some(value) => Some(percent(value)?),
        None => None,
    };
    if let Some(value) = app
        && !(value.len() <= MAX_APP && value.iter().copied().all(name_byte))
    {
        return None;
    }
    for (value, limit) in [(title, MAX_TITLE), (msg, MAX_MSG)] {
        if let Some(value) = value {
            text(value, limit, decoded)?;
        }
    }
    if state == b"clear" {
        return Some(Report::Clear { id });
    }
    Some(Report::Set {
        id,
        state: State::parse(state)?,
        progress,
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
/// state, a missing one. The percentage is read where the state carries one
/// (`1`, `2`, `4`); one that is missing or not 0 to 100 is no percentage, not a
/// reason to drop the report — the bar's state is what the ring shows.
pub(crate) fn progress(payload: &[u8]) -> Option<Report> {
    let mut fields = payload.split(|&byte| byte == b';');
    if fields.next()? != b"4" {
        return None;
    }
    let root = String::new;
    let state = fields.next()?;
    let value = fields.next().and_then(percent);
    Some(match state {
        b"0" => Report::Clear { id: root() },
        b"1" | b"4" => Report::Set {
            id: root(),
            state: State::Working,
            progress: value,
        },
        b"3" => Report::Set {
            id: root(),
            state: State::Working,
            progress: None,
        },
        b"2" => Report::Set {
            id: root(),
            state: State::Error,
            progress: value,
        },
        _ => return None,
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
#[derive(Clone, Copy, Debug)]
struct Record {
    state: State,
    /// From 0 to 100, as its last report said; `None` when it gave none.
    progress: Option<u8>,
    /// When the record entered its state: a report that repeats the state (a
    /// progress update) keeps it, so a ring's clock and the counter run on.
    since: Instant,
    /// The update's number; the smallest is evicted first.
    updated: u64,
}

/// One record as a host reads it ([`crate::Session::program_records`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProgramRecord {
    /// The record's id, slash-separated; the root record's is empty.
    pub id: String,
    pub state: State,
    /// From 0 to 100; `None` when its last report gave none.
    pub progress: Option<u8>,
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
                progress,
            } => {
                self.clock += 1;
                let (entered, since) = match self.records.get(&id) {
                    Some(record) if record.state == state => (false, record.since),
                    _ => (true, now),
                };
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
                        progress,
                        since,
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
        self.records.len() != before
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

    /// Every record, by id.
    pub(crate) fn records(&self) -> Vec<ProgramRecord> {
        self.records
            .iter()
            .map(|(id, record)| ProgramRecord {
                id: id.clone(),
                state: record.state,
                progress: record.progress,
            })
            .collect()
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
            progress: None,
        }
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
            Some(set("", State::Idle))
        );
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
    fn a_reports_progress_is_kept_with_its_record() {
        assert_eq!(
            read("state=working:id=build:progress=40"),
            Some(Report::Set {
                id: "build".to_owned(),
                state: State::Working,
                progress: Some(40),
            })
        );
        let mut status = ProgramStatus::default();
        let now = Instant::now();
        status.apply(read("state=working:id=build:progress=40").unwrap(), now);
        status.apply(read("state=blocked:id=build/ask").unwrap(), now);
        assert_eq!(
            status.records(),
            [
                ProgramRecord {
                    id: "build".to_owned(),
                    state: State::Working,
                    progress: Some(40),
                },
                ProgramRecord {
                    id: "build/ask".to_owned(),
                    state: State::Blocked,
                    progress: None,
                },
            ]
        );
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
        let root = |state, progress| {
            Some(Report::Set {
                id: String::new(),
                state,
                progress,
            })
        };
        assert_eq!(progress(b"4;1;50"), root(State::Working, Some(50)));
        assert_eq!(
            progress(b"4;3"),
            root(State::Working, None),
            "indeterminate"
        );
        assert_eq!(
            progress(b"4;3;70"),
            root(State::Working, None),
            "indeterminate has no value"
        );
        assert_eq!(
            progress(b"4;4;30"),
            root(State::Working, Some(30)),
            "paused is not over"
        );
        assert_eq!(
            progress(b"4;2;80"),
            root(State::Error, Some(80)),
            "the red bar"
        );
        assert_eq!(
            progress(b"4;1;150"),
            root(State::Working, None),
            "a value out of range is no value"
        );
        assert_eq!(progress(b"4;1"), root(State::Working, None));
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
