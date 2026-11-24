//! What a pane's wait for its terminal of this Mac went through, for the log.
//!
//! The pane's request and the service's loop share a [`Trace`]: the loop writes the step the wait
//! is in as it enters it, and reads it when the wait ends — or when the pane stops waiting, which
//! is when the loop has not answered and the step it is stuck in is the thing to know. A line is
//! written once per wait that was slow ([`Backend::slow_open`]), by the loop when the terminal
//! comes, and once per wait that did not end with a terminal, by whoever refuses it
//! ([`super::LocalAsk::refuse`]).

use std::fmt::Write as _;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// A wait for a terminal at least this long is written to the log, with how long each step took.
pub(in crate::kablo) const SLOW_OPEN: Duration = Duration::from_secs(2);

/// The steps a pane's request goes through, in order. A step that was not needed (the account was
/// there, the terminal was found again and not numbered) is not entered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Step {
    /// Asked, and not yet taken up by the service's loop: the loop was busy.
    Queue,
    /// This Mac's account is being made (kablo looked for and put there, the terminals that run an
    /// older kablo renewed, the bridge started), once for all the panes that wait for it.
    Account,
    /// The counter the `bateri` command uses is being asked for the terminal's number.
    Number,
    /// The terminal is being started on the bridge, and the pane's stream made.
    Start,
    /// The server has been asked, and has not said the terminal is there.
    Server,
}

impl Step {
    fn name(self) -> &'static str {
        match self {
            Step::Queue => "queue",
            Step::Account => "account",
            Step::Number => "number",
            Step::Start => "start",
            Step::Server => "server",
        }
    }
}

/// What is known of one wait. See the module's documentation.
pub(super) struct Trace {
    marks: Mutex<Marks>,
}

struct Marks {
    /// When the pane asked.
    asked: Instant,
    /// The steps entered, each with when.
    entered: Vec<(Step, Instant)>,
    /// A terminal found again, not a new one.
    found: bool,
    /// Why the pane was not given a terminal, when the loop knows.
    refused: Option<String>,
}

impl Trace {
    pub(super) fn new(asked: Instant) -> Trace {
        Trace {
            marks: Mutex::new(Marks {
                asked,
                entered: vec![(Step::Queue, asked)],
                found: false,
                refused: None,
            }),
        }
    }

    fn marks(&self) -> std::sync::MutexGuard<'_, Marks> {
        self.marks.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The wait is in `step` from now.
    pub(super) fn enter(&self, step: Step) {
        self.marks().entered.push((step, Instant::now()));
    }

    /// The terminal is one found again.
    pub(super) fn found_again(&self) {
        self.marks().found = true;
    }

    /// Says why the pane is not given a terminal; the first reason stands.
    pub(super) fn refuse(&self, why: impl Into<String>) {
        self.marks().refused.get_or_insert_with(|| why.into());
    }

    /// The reason the loop gave, if it did.
    pub(super) fn refusal(&self) -> Option<String> {
        self.marks().refused.clone()
    }

    /// How long the wait has been, at `now`.
    pub(super) fn total(&self, now: Instant) -> Duration {
        now.saturating_duration_since(self.marks().asked)
    }

    /// The step the wait is in, with how long it has been in it.
    pub(super) fn current(&self, now: Instant) -> (Step, Duration) {
        let marks = self.marks();
        let (step, since) = marks
            .entered
            .last()
            .copied()
            .unwrap_or((Step::Queue, marks.asked));
        (step, now.saturating_duration_since(since))
    }

    /// `queue 0.0 s, account 9.8 s, number 0.1 s, …`: each step entered, with how long the wait
    /// was in it, the last one up to `now`.
    pub(super) fn steps(&self, now: Instant) -> String {
        let marks = self.marks();
        let mut text = String::new();
        for (index, (step, since)) in marks.entered.iter().enumerate() {
            let until = marks.entered.get(index + 1).map_or(now, |(_, next)| *next);
            if !text.is_empty() {
                text.push_str(", ");
            }
            let _ = write!(
                text,
                "{} {}",
                step.name(),
                seconds(until.saturating_duration_since(*since))
            );
        }
        text
    }

    fn kind(&self) -> &'static str {
        if self.marks().found {
            "found again"
        } else {
            "new"
        }
    }

    /// The line for a wait that ended with a terminal after `now - asked`, when it was slow.
    pub(super) fn slow_line(&self, pane: &str, slow: Duration, now: Instant) -> Option<String> {
        let total = self.total(now);
        (total >= slow).then(|| {
            format!(
                "the terminal of pane {pane} ({}) came after {}: {}",
                self.kind(),
                seconds(total),
                self.steps(now)
            )
        })
    }

    /// The line for a wait that has not ended, at `now`: the step it is in and for how long, and
    /// the steps it has been through. Written when the pane says it is still waiting.
    pub(super) fn waiting_line(&self, pane: &str, now: Instant) -> String {
        let (step, in_step) = self.current(now);
        format!(
            "the pane {pane} still has no terminal of this Mac ({}) after {}: the wait is in `{}` for {}; {}",
            self.kind(),
            seconds(self.total(now)),
            step.name(),
            seconds(in_step),
            self.steps(now)
        )
    }

    /// The line for a wait that ended without a terminal, for the reason `why` (the loop's own,
    /// when it gave one) after `now - asked`.
    pub(super) fn refused_line(&self, pane: &str, why: &str, now: Instant) -> String {
        let (step, in_step) = self.current(now);
        let total = self.total(now);
        let mut line = format!(
            "the pane {pane} is an ordinary one: no terminal of this Mac after {}: {why}",
            seconds(total)
        );
        if self.refusal().is_none() {
            let _ = write!(
                line,
                " (the wait was in `{}` for {})",
                step.name(),
                seconds(in_step)
            );
        }
        let _ = write!(line, "; {}", self.steps(now));
        line
    }
}

/// The line about the account of this Mac having been slow to make, when it was: how long kablo
/// took to look for and put there, and how long the terminals that run an older kablo took to be
/// renewed and the versions nothing runs from to be removed (the two halves of what a connection
/// does, and the second is the one that grows with the terminals the machine has).
pub(in crate::kablo) fn slow_account_line(looking: Duration, tidying: Duration) -> Option<String> {
    (looking + tidying >= SLOW_OPEN).then(|| {
        format!(
            "kablo on this Mac took {}: looking for it and putting it there {}, renewing the terminals that run an older kablo and removing the versions nothing runs from {}",
            seconds(looking + tidying),
            seconds(looking),
            seconds(tidying)
        )
    })
}

/// `1.5 s`.
pub(super) fn seconds(duration: Duration) -> String {
    format!("{:.1} s", duration.as_secs_f64())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(start: Instant, millis: u64) -> Instant {
        start + Duration::from_millis(millis)
    }

    #[test]
    fn a_wait_tells_the_steps_it_was_in_and_for_how_long() {
        let start = Instant::now();
        let trace = Trace::new(start);
        // The steps are entered at the moments the loop enters them: here, laid out by hand.
        {
            let mut marks = trace.marks();
            marks.entered.push((Step::Account, at(start, 100)));
            marks.entered.push((Step::Number, at(start, 9_900)));
            marks.entered.push((Step::Start, at(start, 10_000)));
            marks.entered.push((Step::Server, at(start, 10_010)));
        }
        assert_eq!(
            trace.steps(at(start, 10_400)),
            "queue 0.1 s, account 9.8 s, number 0.1 s, start 0.0 s, server 0.4 s"
        );
        let (step, in_step) = trace.current(at(start, 10_400));
        assert_eq!(step, Step::Server);
        assert_eq!(in_step, Duration::from_millis(390));
    }

    #[test]
    fn a_slow_account_says_which_half_of_it_was_slow() {
        let line = slow_account_line(Duration::from_millis(100), Duration::from_millis(9_700))
            .expect("slow");
        assert!(line.contains("took 9.8 s"), "{line}");
        assert!(line.contains("putting it there 0.1 s"), "{line}");
        assert!(
            line.contains("removing the versions nothing runs from 9.7 s"),
            "{line}"
        );
        assert_eq!(
            slow_account_line(Duration::from_millis(100), Duration::from_millis(1_800)),
            None
        );
    }

    #[test]
    fn a_wait_that_has_not_ended_says_the_step_it_is_in() {
        let start = Instant::now();
        let trace = Trace::new(start);
        {
            let mut marks = trace.marks();
            marks.entered.push((Step::Account, at(start, 100)));
            marks.entered.push((Step::Start, at(start, 1_500)));
            marks.entered.push((Step::Server, at(start, 1_510)));
        }
        let line = trace.waiting_line("P1", at(start, 2_010));
        assert!(line.contains("pane P1 still has no terminal"), "{line}");
        assert!(line.contains("(new)"), "{line}");
        assert!(line.contains("after 2.0 s"), "{line}");
        assert!(line.contains("in `server` for 0.5 s"), "{line}");
        assert!(
            line.ends_with("queue 0.1 s, account 1.4 s, start 0.0 s, server 0.5 s"),
            "{line}"
        );
    }

    #[test]
    fn only_a_slow_wait_is_told_and_a_refusal_always_is() {
        let start = Instant::now();
        let trace = Trace::new(start);
        assert_eq!(trace.slow_line("P1", SLOW_OPEN, at(start, 1_900)), None);
        let line = trace
            .slow_line("P1", SLOW_OPEN, at(start, 2_100))
            .expect("a slow wait");
        assert!(line.contains("pane P1 (new)"), "{line}");
        assert!(line.contains("2.1 s"), "{line}");
        trace.found_again();
        let line = trace
            .slow_line("P1", SLOW_OPEN, at(start, 2_100))
            .expect("a slow wait");
        assert!(line.contains("(found again)"), "{line}");

        // A refusal names the step it was in when nobody said why, and says why when somebody did.
        let line = trace.refused_line("P1", "no terminal came in 10 s", at(start, 10_000));
        assert!(line.contains("`queue` for 10.0 s"), "{line}");
        trace.refuse("the account was refused");
        trace.refuse("another reason does not replace the first");
        let line = trace.refused_line("P1", &trace.refusal().expect("a reason"), at(start, 50));
        assert!(line.contains("the account was refused"), "{line}");
        assert!(!line.contains("another"), "{line}");
        assert!(!line.contains("the wait was in"), "{line}");
    }
}
