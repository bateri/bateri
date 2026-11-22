//! What a browser's first page and a profile's requests wait for, and for how long.
//!
//! A profile that joins its home is brought up to date in two steps that a browser must not run
//! ahead of (Karar 4: an empty profile must not be the first thing a site sees): what lies in
//! the profile's folders is put there before the engine opens it ([`Stage::BeforeOpen`]), and
//! the rest before the first address is visited ([`Stage::AfterOpen`]). Both waits are
//! bounded by the same moment: a home that has not answered by then is left to catch up while
//! the browser works with what the Mac has.
//!
//! A computer that comes back to a home it lost holds the profile's **requests** until the
//! difference is in ([`Gate::wait_released`]): the tabs of a Mac that woke up would otherwise
//! go on with the token the Mac had before another computer's turn of it.

use std::sync::{Condvar, Mutex, PoisonError};
use std::time::Instant;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    BeforeOpen,
    AfterOpen,
}

#[derive(Default)]
struct State {
    before_open: bool,
    after_open: bool,
    held: bool,
}

#[derive(Default)]
pub struct Gate {
    state: Mutex<State>,
    changed: Condvar,
}

impl Gate {
    pub fn new() -> Gate {
        Gate::default()
    }

    fn with(&self, f: impl FnOnce(&mut State)) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        f(&mut state);
        self.changed.notify_all();
    }

    pub fn release(&self, stage: Stage) {
        self.with(|s| match stage {
            Stage::BeforeOpen => s.before_open = true,
            Stage::AfterOpen => {
                s.before_open = true;
                s.after_open = true;
            }
        });
    }

    /// Nothing is waited for any more: the home is not coming.
    pub fn release_all(&self) {
        self.release(Stage::AfterOpen);
    }

    /// Whether `stage` has been released, without waiting.
    pub fn released(&self, stage: Stage) -> bool {
        let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        match stage {
            Stage::BeforeOpen => state.before_open,
            Stage::AfterOpen => state.after_open,
        }
    }

    /// Waits until `stage` is released or `until` has come; whether it was released.
    pub fn wait(&self, stage: Stage, until: Instant) -> bool {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        loop {
            let done = match stage {
                Stage::BeforeOpen => state.before_open,
                Stage::AfterOpen => state.after_open,
            };
            if done {
                return true;
            }
            let Some(left) = until.checked_duration_since(Instant::now()) else {
                return false;
            };
            state = self
                .changed
                .wait_timeout(state, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }

    /// The profile's requests are held (a home is being caught up with) or let go.
    pub fn hold(&self, held: bool) {
        self.with(|s| s.held = held);
    }

    /// Waits until the requests are not held or `until` has come; whether they are not.
    pub fn wait_released(&self, until: Instant) -> bool {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        loop {
            if !state.held {
                return true;
            }
            let Some(left) = until.checked_duration_since(Instant::now()) else {
                return false;
            };
            state = self
                .changed
                .wait_timeout(state, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::time::Duration;

    #[test]
    fn a_wait_ends_when_the_stage_is_released_and_not_later_than_its_time() {
        let gate = Arc::new(Gate::new());
        let soon = Instant::now() + Duration::from_millis(30);
        assert!(!gate.wait(Stage::BeforeOpen, soon), "nobody released it");
        let other = gate.clone();
        let thread = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(20));
            other.release(Stage::BeforeOpen);
        });
        assert!(gate.wait(Stage::BeforeOpen, Instant::now() + Duration::from_secs(5)));
        thread.join().unwrap();
        // The first stage does not release the second; the second releases both.
        assert!(!gate.released(Stage::AfterOpen));
        gate.release(Stage::AfterOpen);
        assert!(gate.released(Stage::BeforeOpen) && gate.released(Stage::AfterOpen));
    }

    #[test]
    fn requests_are_held_only_while_a_hold_is_on() {
        let gate = Arc::new(Gate::new());
        assert!(gate.wait_released(Instant::now()));
        gate.hold(true);
        assert!(!gate.wait_released(Instant::now() + Duration::from_millis(20)));
        let other = gate.clone();
        let thread = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(20));
            other.hold(false);
        });
        assert!(gate.wait_released(Instant::now() + Duration::from_secs(5)));
        thread.join().unwrap();
    }
}
