//! The thread that follows the sources: a check at launch, another when the list changes, and one
//! a day (sooner and sooner again after a check the network was down for). The window never waits for it and never asks it anything it cannot answer from memory:
//! [`Tracker::statuses`] is the result of the last check that finished. When a check ends the
//! worker runs what it was given to do next (`after`): the application puts what landed on this
//! computer's own account there, on this thread, which nothing else waits for.

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, SystemTime};

use super::check::check;
use super::fetch::{Curl, Fetch};
use super::{Config, Failure, State, Status};

/// How often the worker looks at the clock between checks: an hour, so a computer that slept past the
/// day is checked within the hour it wakes (an `Instant` does not run while the computer sleeps).
const WAKE: Duration = Duration::from_secs(60 * 60);

/// The time between two checks.
const DAY: Duration = Duration::from_secs(24 * 60 * 60);

/// The first wait before a check that could not reach the network is made again; it doubles after
/// each such check up to [`WAKE`]. A program does not wait a day to arrive after the computer was
/// opened offline.
const RETRY: Duration = Duration::from_secs(5 * 60);

/// Whether a check left something that a later try may mend ([`Failure::mendable`]).
fn mendable(found: &[Status]) -> bool {
    found.iter().any(|status| {
        matches!(&status.state, State::Error(failure) if failure.mendable())
            || status.problem.as_ref().is_some_and(Failure::mendable)
    })
}

/// The wait until the next check, from what the last one found: the day it is, or a short one that
/// grows (`backoff`, which starts again from [`RETRY`] when a check leaves nothing to mend).
fn wait_after(found: &[Status], backoff: &mut Duration) -> Duration {
    if mendable(found) {
        let wait = *backoff;
        *backoff = (*backoff * 2).min(WAKE);
        wait
    } else {
        *backoff = RETRY;
        DAY
    }
}

/// What the worker does when a check has ended.
pub type After = Box<dyn Fn() + Send + 'static>;

/// The list a check follows, and whether it is the one the person wrote ([`check`]).
struct Sources {
    list: Vec<String>,
    authoritative: bool,
}

/// The worker's handle. Dropping it ends the thread once the check in progress is done.
pub struct Tracker {
    commands: Sender<Sources>,
    statuses: Arc<Mutex<Vec<Status>>>,
    /// A check is asked for or under way.
    running: Arc<AtomicBool>,
    /// When the last check ended.
    checked: Arc<Mutex<Option<SystemTime>>>,
    /// What the worker was last given, so that the same list is not checked again.
    given: Mutex<(Vec<String>, bool)>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Tracker {
    /// Starts the worker with `curl`: the first check begins at once, on the thread.
    pub fn start(
        config: Config,
        sources: Vec<String>,
        authoritative: bool,
        after: After,
    ) -> io::Result<Self> {
        Self::start_with(config, Arc::new(Curl), sources, authoritative, after)
    }

    /// [`Tracker::start`] with another way to fetch.
    pub fn start_with(
        config: Config,
        fetch: Arc<dyn Fetch>,
        sources: Vec<String>,
        authoritative: bool,
        after: After,
    ) -> io::Result<Self> {
        let (commands, inbox) = mpsc::channel::<Sources>();
        let statuses = Arc::new(Mutex::new(Vec::new()));
        let shared = Arc::clone(&statuses);
        // The first check begins at once.
        let running = Arc::new(AtomicBool::new(true));
        let checked_at = Arc::new(Mutex::new(None));
        let (busy, ended) = (Arc::clone(&running), Arc::clone(&checked_at));
        let given = Mutex::new((sources.clone(), authoritative));
        let mut current = Sources {
            list: sources,
            authoritative,
        };
        thread::Builder::new()
            .name("bateri-apps".into())
            .spawn(move || {
                let mut checked: Option<SystemTime> = None;
                let (mut wait, mut backoff) = (DAY, RETRY);
                loop {
                    let elapsed = |at: SystemTime| SystemTime::now().duration_since(at);
                    let due = checked.is_none_or(|at| {
                        elapsed(at).is_ok_and(|elapsed| elapsed >= wait) || SystemTime::now() < at
                    });
                    if due {
                        busy.store(true, Ordering::SeqCst);
                        let found = check(
                            &config,
                            &current.list,
                            current.authoritative,
                            fetch.as_ref(),
                            SystemTime::now(),
                        );
                        wait = wait_after(&found, &mut backoff);
                        *lock(&shared) = found;
                        checked = Some(SystemTime::now());
                        *lock(&ended) = checked;
                        busy.store(false, Ordering::SeqCst);
                        after();
                    }
                    // Sleeps until the check that is next, or the hour at most (the clock is
                    // looked at again then).
                    let left = checked
                        .and_then(|at| elapsed(at).ok())
                        .map_or(Duration::ZERO, |elapsed| wait.saturating_sub(elapsed));
                    match inbox.recv_timeout(left.clamp(Duration::from_secs(1), WAKE)) {
                        Ok(mut latest) => {
                            // A burst of saves is one check, of the last list.
                            while let Ok(next) = inbox.try_recv() {
                                latest = next;
                            }
                            current = latest;
                            checked = None;
                            backoff = RETRY;
                        }
                        Err(RecvTimeoutError::Timeout) => {}
                        Err(RecvTimeoutError::Disconnected) => return,
                    }
                }
            })?;
        Ok(Self {
            commands,
            statuses,
            running,
            checked: checked_at,
            given,
        })
    }

    /// The settings file was read: check its list now, unless the worker has been given this one
    /// already (a save of another key is not a reason to go to the network).
    pub fn set_sources(&self, sources: Vec<String>, authoritative: bool) {
        let mut given = lock(&self.given);
        if given.0 == sources && given.1 == authoritative {
            return;
        }
        *given = (sources.clone(), authoritative);
        self.running.store(true, Ordering::SeqCst);
        // The worker is gone only when the process is ending.
        let _ = self.commands.send(Sources {
            list: sources,
            authoritative,
        });
    }

    /// Checks the list the worker has now, at once: a key was approved, or the person asked.
    pub fn refresh(&self) {
        let (list, authoritative) = lock(&self.given).clone();
        // Said at once, before the worker wakes: the page shows the ring from the click on.
        self.running.store(true, Ordering::SeqCst);
        let _ = self.commands.send(Sources {
            list,
            authoritative,
        });
    }

    /// Whether a check is asked for or under way.
    pub fn running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    /// When the last check ended; `None` until the first one has.
    pub fn checked_at(&self) -> Option<SystemTime> {
        *lock(&self.checked)
    }

    /// All the page is drawn from, at once.
    pub fn tracked(&self) -> super::page::Tracked {
        super::page::Tracked {
            statuses: self.statuses(),
            running: self.running(),
            checked: self.checked_at(),
        }
    }

    /// How each source stood at the last check that finished; empty until the first one has.
    pub fn statuses(&self) -> Vec<Status> {
        lock(&self.statuses).clone()
    }
}

#[cfg(test)]
mod tests {
    use bt_core::pencere::apps::Version;

    use super::*;

    fn status(state: State, problem: Option<Failure>) -> Status {
        Status {
            source: "acme/notes".to_owned(),
            name: Some("notes".to_owned()),
            state,
            problem,
        }
    }

    fn ready() -> State {
        State::Ready {
            version: Version::parse("1.0.0").expect("a version"),
            newer: None,
        }
    }

    #[test]
    fn a_check_the_network_failed_is_tried_again_soon_at_growing_intervals() {
        let down = [status(
            State::Error(Failure::Fetch("no route".into())),
            None,
        )];
        let mut backoff = RETRY;
        let waits: Vec<u64> = (0..6)
            .map(|_| wait_after(&down, &mut backoff).as_secs() / 60)
            .collect();
        // Five minutes, doubling, never more than the hour.
        assert_eq!(waits, [5, 10, 20, 40, 60, 60]);
        // A release that landed before and could not be refreshed is the same.
        let stale = [status(ready(), Some(Failure::Fetch("timeout".into())))];
        let mut backoff = RETRY;
        assert_eq!(wait_after(&stale, &mut backoff), RETRY);
        // When it works again the day is back, and the next failure starts from the short wait.
        assert_eq!(wait_after(&[status(ready(), None)], &mut backoff), DAY);
        assert_eq!(backoff, RETRY);
    }

    #[test]
    fn a_source_that_is_wrong_in_itself_waits_for_the_person_not_the_clock() {
        for failure in [Failure::Source, Failure::NameTaken, Failure::Reserved] {
            let mut backoff = RETRY;
            let found = [status(State::Error(failure), None)];
            assert_eq!(wait_after(&found, &mut backoff), DAY);
        }
    }
}
