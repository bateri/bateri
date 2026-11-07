//! The thread that follows the sources: a check at launch, another when the list changes, and one
//! a day. The window never waits for it and never asks it anything it cannot answer from memory:
//! [`Tracker::statuses`] is the result of the last check that finished. When a check ends the
//! worker runs what it was given to do next (`after`): the application puts what landed on this
//! Mac's own account there, on this thread, which nothing else waits for.

use std::io;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, SystemTime};

use super::check::check;
use super::fetch::{Curl, Fetch};
use super::{Config, Status};

/// How often the worker looks at the clock between checks: an hour, so a Mac that slept past the
/// day is checked within the hour it wakes (an `Instant` does not run while the Mac sleeps).
const WAKE: Duration = Duration::from_secs(60 * 60);

/// The time between two checks.
const DAY: Duration = Duration::from_secs(24 * 60 * 60);

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
        let given = Mutex::new((sources.clone(), authoritative));
        let mut current = Sources {
            list: sources,
            authoritative,
        };
        thread::Builder::new()
            .name("bateri-apps".into())
            .spawn(move || {
                let mut checked: Option<SystemTime> = None;
                loop {
                    let due = checked.is_none_or(|at| {
                        SystemTime::now()
                            .duration_since(at)
                            .is_ok_and(|elapsed| elapsed >= DAY)
                            || SystemTime::now() < at
                    });
                    if due {
                        let found = check(
                            &config,
                            &current.list,
                            current.authoritative,
                            fetch.as_ref(),
                            SystemTime::now(),
                        );
                        *lock(&shared) = found;
                        checked = Some(SystemTime::now());
                        after();
                    }
                    match inbox.recv_timeout(WAKE) {
                        Ok(mut latest) => {
                            // A burst of saves is one check, of the last list.
                            while let Ok(next) = inbox.try_recv() {
                                latest = next;
                            }
                            current = latest;
                            checked = None;
                        }
                        Err(RecvTimeoutError::Timeout) => {}
                        Err(RecvTimeoutError::Disconnected) => return,
                    }
                }
            })?;
        Ok(Self {
            commands,
            statuses,
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
        // The worker is gone only when the process is ending.
        let _ = self.commands.send(Sources {
            list: sources,
            authoritative,
        });
    }

    /// Checks the list the worker has now, at once: a key was approved, or the person asked.
    pub fn refresh(&self) {
        let (list, authoritative) = lock(&self.given).clone();
        let _ = self.commands.send(Sources {
            list,
            authoritative,
        });
    }

    /// How each source stood at the last check that finished; empty until the first one has.
    pub fn statuses(&self) -> Vec<Status> {
        lock(&self.statuses).clone()
    }
}
