//! The pane's **sampling driver** for the remote load indicator (046 phase-4):
//! it runs [`Schedule`]'s actions on the main queue and turns the helper's
//! replies into the context row's value.
//!
//! - **The decision is pure and elsewhere** (`bt-shell-common::remote_stats`):
//!   when to sample, what a sample means. Here only the binding — a tick is a
//!   `dispatch` `after` carrying a **token** (`after` cannot be cancelled, a
//!   stale token is ignored by the schedule), a request is a
//!   [`Query::Load`] to the pane's helper session (045 Karar 10), the reply
//!   comes back to the main queue **with the pane id and the generation**
//!   (`hyperlink::verify_remote`'s pattern): a closed pane drops it, another
//!   generation's sample only frees the worker.
//! - **The events** come from the pane: the remote edge (a generation started
//!   or ended), visibility (`SplitView::apply_visibility`), the settings,
//!   interaction (the view's keys, presses, wheel and mouse moves, the window
//!   becoming key) and the close.
//! - **Zero frames when idle** (`CLAUDE.md` → "Boşta sıfır kare", 046 Karar 6):
//!   the timer never wakes the link; a frame is asked only by
//!   `Session::set_remote_stats`'s equality gate when the shown value changes.
//!
//! The rationale is in `.tasks/046-uzak-yuk-gostergesi/discussion.md` → Karar 1,
//! 5, 6 and 8.

use std::time::{Duration, Instant};

use bt_core::{RemoteStats, RemoteStatsSettings, STATS_HISTORY, StatsForm};
use bt_shell_common::remote_stats::{Action, Detail, Outcome, Sampler, Schedule};
use dispatch2::{DispatchQueue, DispatchTime};
use objc2::MainThreadMarker;

use crate::pane::TerminalPane;
use crate::remote_helper::{Answer, LoadReply, Query, Request};

/// The pane's sampling state: the schedule, the sampler and the form.
#[derive(Debug)]
pub(crate) struct StatsDriver {
    schedule: Schedule,
    sampler: Sampler,
    /// The drawn form; `None` → `stats = "off"`.
    form: Option<StatsForm>,
    /// The last sample's value, taken as a sparkline (history included): a
    /// form change redraws from it at once rather than a whole interval later
    /// (Karar 8, [`shaped`]). Only ever the schedule's current generation's
    /// and only while shown: a new generation, a restart and a hide clear it —
    /// otherwise a form change would bring back another host's numbers or a
    /// hidden value (Karar 5).
    last: Option<RemoteStats>,
    /// The last sample's details — the popover's content ([`crate::stats_popover`]);
    /// cleared with [`Self::last`] for the same reason (another host's
    /// processes must not show). The OS and the core count come only with a
    /// `detail` request and do not change within a generation: a plain sample
    /// keeps the previous ones.
    detail: Option<Detail>,
}

impl StatsDriver {
    /// From the settings' snapshot at the pane's birth (`PaneLaunch`).
    pub(crate) fn new(settings: &RemoteStatsSettings) -> Self {
        let form = settings.mode.form();
        Self {
            schedule: Schedule::new(form.is_some(), interval(settings)),
            sampler: Sampler::default(),
            form,
            last: None,
            detail: None,
        }
    }

    /// The last sample's details (the popover's content), if any.
    pub(crate) fn detail(&self) -> Option<&Detail> {
        self.detail.as_ref()
    }

    /// Forgets the last sample: a new generation, a restart, a hide.
    fn forget(&mut self) {
        self.last = None;
        self.detail = None;
    }

    /// Takes a sample's details; a plain sample keeps the OS and the cores of
    /// the previous `detail` one (they do not change within a generation).
    fn keep_detail(&mut self, mut detail: Detail) {
        if let Some(previous) = self.detail.take() {
            if detail.os.is_none() {
                detail.os = previous.os;
            }
            if detail.cores.is_none() {
                detail.cores = previous.cores;
            }
        }
        self.detail = Some(detail);
    }
}

/// `stats_interval` as a duration.
fn interval(settings: &RemoteStatsSettings) -> Duration {
    Duration::from_secs(u64::from(settings.interval))
}

/// A value taken as a sparkline, in `form`: the other forms carry no history
/// (`RemoteStats`'s rule — an invisible history change must not ask for a frame).
fn shaped(full: RemoteStats, form: StatsForm) -> RemoteStats {
    let mut stats = full;
    stats.form = form;
    if form != StatsForm::Sparkline {
        stats.history = [0; STATS_HISTORY];
        stats.len = 0;
    }
    stats
}

/// A load reply → what the schedule needs.
fn outcome(reply: &Result<Answer, String>) -> Outcome {
    match reply {
        Ok(Answer::Load(LoadReply::Sample(_))) => Outcome::Sample { cpu: false },
        Ok(Answer::Load(LoadReply::NoProc)) => Outcome::NoProc,
        Ok(Answer::Load(LoadReply::Unreachable(_))) => Outcome::Unreachable,
        // Another answer cannot come for a load request; it counts as a failure.
        Ok(_) | Err(_) => Outcome::Failed,
    }
}

/// The sampling half of the pane (046 phase-4).
impl TerminalPane {
    /// The remote edge: the session's remote generation (or none) goes to the
    /// schedule. A new generation takes its first sample at once; an ended one
    /// stops arming (the value itself `Session` already dropped on `C`/`D`/`A`).
    pub(crate) fn sync_stats_generation(&self) {
        let generation = self
            .session()
            .and_then(|session| session.remote_target())
            .map(|(command, ..)| command);
        let actions = {
            let mut driver = self.stats_driver().borrow_mut();
            if driver.schedule.generation() != generation {
                driver.forget();
            }
            driver.schedule.set_generation(Instant::now(), generation)
        };
        self.run_stats(actions);
        self.stats_gauge_changed();
    }

    /// The pane became visible or covered (a background tab, a minimised
    /// window, behind a zoomed split — `SplitView::apply_visibility`).
    pub(crate) fn set_visible(&self, visible: bool) {
        let actions = self
            .stats_driver()
            .borrow_mut()
            .schedule
            .set_visible(Instant::now(), visible);
        self.run_stats(actions);
    }

    /// A key, a press, the wheel, a mouse move or the window becoming key.
    /// Called at mouse-move rate: while sampling runs it only stamps the time
    /// (no allocation, no `Term` lock).
    pub(crate) fn note_interaction(&self) {
        let actions = self
            .stats_driver()
            .borrow_mut()
            .schedule
            .interaction(Instant::now());
        self.run_stats(actions);
    }

    /// `[remote] stats`/`stats_interval` changed (`AppDelegate::reload_settings`).
    /// `off` hides the indicator and stops sampling; another form redraws the
    /// last value at once (Karar 8); a new interval holds from the next tick.
    pub(crate) fn set_stats_settings(&self, settings: &RemoteStatsSettings) {
        let form = settings.mode.form();
        let (actions, redraw) = {
            let mut driver = self.stats_driver().borrow_mut();
            let changed = driver.form != form;
            driver.form = form;
            let actions =
                driver
                    .schedule
                    .set_form(Instant::now(), form.is_some(), interval(settings));
            let redraw = match (changed, form, driver.last, driver.schedule.generation()) {
                (true, Some(form), Some(last), Some(generation)) => {
                    Some((generation, shaped(last, form)))
                }
                _ => None,
            };
            (actions, redraw)
        };
        if let (Some((generation, stats)), Some(session)) = (redraw, self.session()) {
            session.set_remote_stats(generation, Some(&stats));
        }
        self.run_stats(actions);
        self.stats_gauge_changed();
    }

    /// The popover opened or closed ([`crate::stats_popover`]): while it is
    /// open every request asks for the details, and opening asks at once.
    pub(crate) fn set_stats_detail(&self, open: bool) {
        let actions = self
            .stats_driver()
            .borrow_mut()
            .schedule
            .set_detail(Instant::now(), open);
        self.run_stats(actions);
    }

    /// The pane closes: sampling stops. The armed tick goes stale and the pane
    /// lookup would not find a closed pane anyway.
    pub(crate) fn stop_stats(&self) {
        let _ = self
            .stats_driver()
            .borrow_mut()
            .schedule
            .set_generation(Instant::now(), None);
    }

    /// Runs the schedule's actions. Called with the driver's borrow released:
    /// a request reaches the helper and a hide reaches the session.
    fn run_stats(&self, actions: Vec<Action>) {
        for action in actions {
            match action {
                Action::Arm { token, after } => self.arm_stats(token, after),
                Action::Request { detail, restart } => self.request_stats(detail, restart),
                Action::Hide => {
                    let generation = {
                        let mut driver = self.stats_driver().borrow_mut();
                        driver.forget();
                        driver.schedule.generation()
                    };
                    if let (Some(generation), Some(session)) = (generation, self.session()) {
                        session.set_remote_stats(generation, None);
                    }
                    self.stats_gauge_changed();
                }
            }
        }
    }

    /// A tick after `after`, by pane id; the schedule ignores a stale token.
    fn arm_stats(&self, token: u64, after: Duration) {
        let Ok(when) = DispatchTime::try_from(after) else {
            return;
        };
        let (id, lookup) = (self.id(), self.lookup());
        // The error arm is not represented today (the link clock's rationale);
        // if it dropped, sampling would resume at the next interaction.
        let _ = DispatchQueue::main().after(when, move || {
            // audit: a block running on the main queue is on the main thread by definition.
            let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
            if let Some(pane) = lookup(mtm, id) {
                let actions = pane
                    .stats_driver()
                    .borrow_mut()
                    .schedule
                    .tick(Instant::now(), token);
                pane.run_stats(actions);
            }
        });
    }

    /// A load request to the helper session. The reply always comes back on
    /// the main queue — also when there is no remote target to ask (the edge
    /// has not reached the schedule yet): the schedule's one request in flight
    /// must be answered, otherwise it would wait forever.
    fn request_stats(&self, detail: bool, restart: bool) {
        let generation = {
            let mut driver = self.stats_driver().borrow_mut();
            if restart {
                driver.sampler.reset();
                driver.forget();
            }
            driver.schedule.generation()
        };
        let Some(generation) = generation else {
            return;
        };
        let (id, lookup) = (self.id(), self.lookup());
        let reply = Box::new(move |answer: Result<Answer, String>, _: &[String]| {
            DispatchQueue::main().exec_async(move || {
                // audit: a block running on the main queue is on the main thread by definition.
                let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
                if let Some(pane) = lookup(mtm, id) {
                    pane.stats_answered(generation, answer);
                }
            });
        });
        let target = self
            .session()
            .and_then(|session| session.remote_target())
            .filter(|(command, ..)| *command == generation);
        let Some((command, target, _)) = target else {
            reply(Err("The remote session ended".to_owned()), &[]);
            return;
        };
        self.remote_helper().borrow_mut().ask(Request {
            command,
            host: target.host.clone(),
            // A background job: rides a live master or today's argv, never asks.
            dial: self.dial(target, None),
            query: Query::Load { detail },
            reply,
        });
    }

    /// A load reply of `generation`, on the main queue. A sample feeds the
    /// sampler only for the schedule's generation — another one's counters
    /// would be the next CPU difference's base; the session gates the value by
    /// generation and equality (one frame only when the shown value changed).
    fn stats_answered(&self, generation: u64, answer: Result<Answer, String>) {
        let mut outcome = outcome(&answer);
        let reading = {
            let mut driver = self.stats_driver().borrow_mut();
            match (&answer, driver.form) {
                (Ok(Answer::Load(LoadReply::Sample(sample))), Some(form))
                    if driver.schedule.generation() == Some(generation) =>
                {
                    let reading = driver.sampler.take(sample, StatsForm::Sparkline);
                    driver.last = Some(reading.stats);
                    // A first process scan is "Measuring…" like a first CPU
                    // reading: the next sample follows as soon.
                    let measuring = sample.scan.is_some() && reading.detail.processes.is_none();
                    outcome = Outcome::Sample {
                        cpu: reading.stats.cpu.is_some() && !measuring,
                    };
                    Some((shaped(reading.stats, form), reading.detail))
                }
                _ => None,
            }
        };
        if let Some((stats, detail)) = reading {
            if let Some(session) = self.session() {
                session.set_remote_stats(generation, Some(&stats));
            }
            self.stats_driver().borrow_mut().keep_detail(detail);
            self.refresh_stats_popover();
            // Every sample, not only a changed one: the indicator's rectangle
            // also moves with the window and the hand cursor must follow.
            self.stats_gauge_changed();
        }
        let actions =
            self.stats_driver()
                .borrow_mut()
                .schedule
                .answered(Instant::now(), generation, outcome);
        self.run_stats(actions);
    }
}

#[cfg(test)]
mod tests {
    use bt_shell_common::remote_files::Process;

    use super::*;

    #[test]
    fn a_form_change_keeps_the_values_and_carries_history_only_as_a_sparkline() {
        let mut full = RemoteStats {
            form: StatsForm::Sparkline,
            cpu: Some(23),
            mem: 61,
            disk: 40,
            ..RemoteStats::default()
        };
        full.history[..3].copy_from_slice(&[1, 2, 3]);
        full.len = 3;
        assert_eq!(shaped(full, StatsForm::Sparkline), full);
        let numbers = shaped(full, StatsForm::Numbers);
        assert_eq!(numbers.form, StatsForm::Numbers);
        assert_eq!((numbers.cpu, numbers.mem, numbers.disk), (Some(23), 61, 40));
        assert_eq!((numbers.len, numbers.history), (0, [0; STATS_HISTORY]));
    }

    #[test]
    fn a_plain_sample_keeps_the_os_and_cores_but_not_the_processes() {
        let mut driver = StatsDriver::new(&RemoteStatsSettings::default());
        driver.keep_detail(Detail {
            os: Some("Ubuntu 24.04".to_owned()),
            cores: Some(8),
            cpu: Some(12),
            processes: Some(vec![Process {
                name: "postgres".to_owned(),
                cpu: 123,
            }]),
            ..Detail::default()
        });
        driver.keep_detail(Detail {
            cpu: Some(40),
            ..Detail::default()
        });
        let detail = driver.detail().expect("a sample arrived");
        assert_eq!(detail.os.as_deref(), Some("Ubuntu 24.04"));
        assert_eq!((detail.cores, detail.cpu), (Some(8), Some(40)));
        assert_eq!(detail.processes, None, "processes are live, not kept");
        driver.forget();
        assert_eq!(driver.detail(), None, "another host's details do not stay");
    }

    #[test]
    fn every_failure_of_a_load_request_is_named_for_the_schedule() {
        let unreachable = Ok(Answer::Load(LoadReply::Unreachable("no".to_owned())));
        assert_eq!(outcome(&unreachable), Outcome::Unreachable);
        assert_eq!(
            outcome(&Ok(Answer::Load(LoadReply::NoProc))),
            Outcome::NoProc
        );
        assert_eq!(outcome(&Err("closed".to_owned())), Outcome::Failed);
        assert_eq!(outcome(&Ok(Answer::Counted(None))), Outcome::Failed);
    }
}
