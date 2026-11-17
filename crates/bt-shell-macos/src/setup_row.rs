//! The **first connection's row** of a pane: the question bateri asks after the login (*Keep
//! this terminal running if the connection drops?*), the wait while it goes up on the server, and
//! the moment it is there. It is the transfer row of the dock — putting bateri on a server is an
//! upload — in the words of [`bt_core::TransferKind`]; this file is its AppKit half: the state
//! the service's news ([`Setup`]) moves, the keys that answer it, and what comes after it.
//!
//! **Keys.** While the question is up *Enter* sets the server up and *Esc* is the plain `ssh`;
//! while bateri goes up *Esc* is *Cancel*. Nothing else a person types reaches the terminal in
//! either moment — the terminal is the holder, which is waiting for this very answer, and a line
//! typed into it would run on the server the person has not been told about. A key that was
//! pressed before the question could be read ([`GUARD`]), or is held down, answers nothing: the
//! *Enter* of the password's last line must not be the answer to a question it never saw.
//!
//! **Zero frames when idle.** The question waits for the person and asks for no frame while it
//! does; the progress asks for one at each whole percent (the service tells no more often), and
//! the done line goes after [`DONE_FOR`] with the server's own dock arriving as a new pane's
//! does ([`bt_gpu::DisplayLink::rearm_arrival`]) — one timer block, no animator of its own.

use std::time::Duration;

use bt_core::{Transfer, TransferKind, TransferTone};
use bt_shell_common::kablo::service::{Answer, Setup};
use bt_shell_common::upload::format_pair;
use dispatch2::{DispatchQueue, DispatchTime};
use objc2::MainThreadMarker;
use objc2_quartz_core::CACurrentMediaTime;

use crate::pane::TerminalPane;

/// How long after the question is up a key is not yet an answer, seconds — **a chosen number**:
/// longer than a person's second press of the same key, shorter than reading the question.
const GUARD: f64 = 0.4;

/// How long the row says the server is set up before the server's own dock takes its place.
const DONE_FOR: Duration = Duration::from_millis(2200);

/// What the first connection's row is doing in a pane.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Phase {
    /// No row.
    #[default]
    Idle,
    /// The question is up; `answered` once it has been answered (the service has not yet put
    /// the next moment up).
    Asking { answered: bool },
    /// bateri goes up.
    Putting,
    /// bateri is there, for [`DONE_FOR`].
    Done,
}

/// A pane's first connection row ([`TerminalPane::setup_row`]).
#[derive(Default)]
pub(crate) struct SetupRow {
    phase: Phase,
    /// When the question was put up, on the clock an event's timestamp is on
    /// (`CACurrentMediaTime`'s).
    asked_at: f64,
    /// Which done line the timer that takes it down was set for.
    serial: u64,
}

/// A key as the row reads it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SetupKey {
    /// *Enter* with no modifier (the numpad's too).
    Enter,
    /// *Esc* with no modifier.
    Escape,
    /// Any other.
    Other,
}

/// What a key does to the row.
#[derive(Debug, PartialEq, Eq)]
enum Keyed {
    /// Not the row's: the terminal's.
    Pass,
    /// The row's, and it does nothing: the terminal is not typed to.
    Swallow,
    /// The person answered.
    Answer(Answer),
}

/// The row's reading of a key at `at`; the rules are in the module's header.
fn read_key(row: &SetupRow, at: f64, key: SetupKey, repeat: bool) -> Keyed {
    match row.phase {
        Phase::Idle | Phase::Done => Keyed::Pass,
        Phase::Asking { answered } => match key {
            SetupKey::Enter | SetupKey::Escape
                if !answered && !repeat && at >= row.asked_at + GUARD =>
            {
                Keyed::Answer(if key == SetupKey::Enter {
                    Answer::SetUp
                } else {
                    Answer::Plain
                })
            }
            _ => Keyed::Swallow,
        },
        Phase::Putting if key == SetupKey::Escape && !repeat => Keyed::Answer(Answer::Cancel),
        Phase::Putting => Keyed::Swallow,
    }
}

impl TerminalPane {
    /// The first connection's row of this pane (`setup_row`).
    fn setup_state(&self) -> &std::cell::RefCell<SetupRow> {
        self.setup_row()
    }

    /// Whether the row can be shown in this pane: it has a dock to put it in.
    fn can_show(&self) -> bool {
        self.session().is_some() && self.has_dock()
    }

    /// The service's news about the row of the server `host`, on the main thread. `true` when the
    /// pane shows it — a question that cannot be shown is answered by the caller as not shown.
    pub(crate) fn set_setup(&self, host: &str, setup: Setup) -> bool {
        let Some(session) = self.session() else {
            return false;
        };
        // A row that cannot be shown is not held to either: no key is taken for a question nobody
        // sees, and the service is told so by the answer the caller gives.
        if setup != Setup::Gone && !self.can_show() {
            self.setup_state().borrow_mut().phase = Phase::Idle;
            self.take_setup_row_down();
            return false;
        }
        let mark = session.host_mark_of(host);
        let line = |kind, body: String, tone, progress| Transfer {
            host: host.to_owned(),
            mark,
            body,
            tone,
            progress,
            kind,
            ..Transfer::default()
        };
        let shown = match setup {
            Setup::Asking => {
                {
                    let mut row = self.setup_state().borrow_mut();
                    row.phase = Phase::Asking { answered: false };
                    row.asked_at = CACurrentMediaTime();
                }
                line(
                    TransferKind::Ask,
                    Transfer::ask_words(host),
                    TransferTone::Plain,
                    None,
                )
            }
            Setup::Putting { sent, total } => {
                self.setup_state().borrow_mut().phase = Phase::Putting;
                let body = if total == 0 {
                    "\u{2191} bateri".to_owned()
                } else {
                    format!("\u{2191} bateri  {}", format_pair(sent.min(total), total))
                };
                let progress = (sent.min(total) * 10_000)
                    .checked_div(total)
                    .map_or(0, |share| u16::try_from(share).unwrap_or(10_000));
                line(
                    TransferKind::Setup,
                    body,
                    TransferTone::Quiet,
                    Some(progress),
                )
            }
            Setup::Done => {
                let serial = {
                    let mut row = self.setup_state().borrow_mut();
                    row.phase = Phase::Done;
                    row.serial += 1;
                    row.serial
                };
                self.setup_done_timer(serial);
                line(
                    TransferKind::Done,
                    String::new(),
                    TransferTone::Success,
                    None,
                )
            }
            Setup::Gone => {
                self.setup_state().borrow_mut().phase = Phase::Idle;
                self.take_setup_row_down();
                return true;
            }
        };
        self.show_transfer(Some(shown));
        self.rehover_footer();
        true
    }

    /// Takes the row down when it is the one that is up — an upload's line is not ours to take.
    fn take_setup_row_down(&self) {
        let ours = self
            .uploads()
            .borrow()
            .shown()
            .is_some_and(|line| line.kind != TransferKind::Upload);
        if ours {
            self.show_transfer(None);
            self.rehover_footer();
        }
    }

    /// One block on the main queue for the done line `serial`: when it runs and the line is still
    /// that one, the row goes and the server's dock comes in as a new pane's does.
    fn setup_done_timer(&self, serial: u64) {
        let Ok(when) = DispatchTime::try_from(DONE_FOR) else {
            return;
        };
        let (id, lookup) = (self.id(), self.lookup());
        let _ = DispatchQueue::main().after(when, move || {
            // audit: a block running on the main queue is on the main thread by definition.
            let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
            if let Some(pane) = lookup(mtm, id) {
                pane.setup_done_over(serial);
            }
        });
    }

    fn setup_done_over(&self, serial: u64) {
        {
            let mut row = self.setup_state().borrow_mut();
            if row.phase != Phase::Done || row.serial != serial {
                return;
            }
            row.phase = Phase::Idle;
        }
        self.take_setup_row_down();
        if let Some(link) = self.link() {
            link.rearm_arrival();
        }
    }

    /// Whether the row is up in a way that holds the terminal's input: the question or the wait.
    pub(crate) fn setup_holds_input(&self) -> bool {
        matches!(
            self.setup_state().borrow().phase,
            Phase::Asking { .. } | Phase::Putting
        )
    }

    /// A key reached the pane at `at` (the event's timestamp); `true` when the row takes it.
    pub(crate) fn setup_key(&self, at: f64, key: SetupKey, repeat: bool) -> bool {
        let keyed = read_key(&self.setup_state().borrow(), at, key, repeat);
        match keyed {
            Keyed::Pass => false,
            Keyed::Swallow => true,
            Keyed::Answer(answer) => {
                self.answer_setup_row(answer);
                true
            }
        }
    }

    /// The person answers the row — by key or by click. A question is answered once.
    pub(crate) fn answer_setup_row(&self, answer: Answer) {
        {
            let mut row = self.setup_state().borrow_mut();
            match (row.phase, answer) {
                (Phase::Asking { answered: false }, Answer::SetUp | Answer::Plain) => {
                    row.phase = Phase::Asking { answered: true };
                }
                (Phase::Putting, Answer::Cancel) => {}
                _ => return,
            }
        }
        self.host().answer_setup(self.uuid(), answer);
    }

    /// *Cancel* of the row while bateri goes up (⌘., Shell ▸ Cancel Upload, the button): `true`
    /// when it was the row's to take.
    pub(crate) fn setup_cancel(&self) -> bool {
        if self.setup_state().borrow().phase != Phase::Putting {
            return false;
        }
        self.answer_setup_row(Answer::Cancel);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(phase: Phase) -> SetupRow {
        SetupRow {
            phase,
            asked_at: 100.0,
            serial: 0,
        }
    }

    const ASKING: Phase = Phase::Asking { answered: false };

    #[test]
    fn enter_sets_up_and_esc_is_the_plain_ssh_once_the_question_can_have_been_read() {
        let asking = row(ASKING);
        let after = 100.0 + GUARD;
        assert_eq!(
            read_key(&asking, after, SetupKey::Enter, false),
            Keyed::Answer(Answer::SetUp)
        );
        assert_eq!(
            read_key(&asking, after, SetupKey::Escape, false),
            Keyed::Answer(Answer::Plain)
        );
    }

    #[test]
    fn a_key_from_before_the_question_or_held_down_answers_nothing_and_is_not_typed() {
        let asking = row(ASKING);
        // The password's last Enter, pressed as the question came up.
        assert_eq!(
            read_key(&asking, 100.0 + GUARD - 0.01, SetupKey::Enter, false),
            Keyed::Swallow
        );
        assert_eq!(
            read_key(&asking, 99.0, SetupKey::Escape, false),
            Keyed::Swallow
        );
        // A key held down repeats; the repeat is not a second answer.
        assert_eq!(
            read_key(&asking, 105.0, SetupKey::Enter, true),
            Keyed::Swallow
        );
    }

    #[test]
    fn a_question_is_answered_once_and_other_keys_are_not_typed_into_the_terminal() {
        let answered = row(Phase::Asking { answered: true });
        assert_eq!(
            read_key(&answered, 105.0, SetupKey::Escape, false),
            Keyed::Swallow
        );
        let asking = row(ASKING);
        assert_eq!(
            read_key(&asking, 105.0, SetupKey::Other, false),
            Keyed::Swallow
        );
    }

    #[test]
    fn while_bateri_goes_up_esc_cancels_and_nothing_is_typed() {
        let putting = row(Phase::Putting);
        assert_eq!(
            read_key(&putting, 105.0, SetupKey::Escape, false),
            Keyed::Answer(Answer::Cancel)
        );
        assert_eq!(
            read_key(&putting, 105.0, SetupKey::Escape, true),
            Keyed::Swallow
        );
        // *Enter* is not *Set Up* again.
        assert_eq!(
            read_key(&putting, 105.0, SetupKey::Enter, false),
            Keyed::Swallow
        );
        assert_eq!(
            read_key(&putting, 105.0, SetupKey::Other, false),
            Keyed::Swallow
        );
    }

    #[test]
    fn with_no_row_or_a_done_one_every_key_is_the_terminals() {
        for phase in [Phase::Idle, Phase::Done] {
            for key in [SetupKey::Enter, SetupKey::Escape, SetupKey::Other] {
                assert_eq!(read_key(&row(phase), 105.0, key, false), Keyed::Pass);
            }
        }
    }
}
