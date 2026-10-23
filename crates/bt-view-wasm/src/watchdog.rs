//! The clock that stops a module that does not come back.
//!
//! A module is stopped by its engine's epoch moving past the deadline the store was given
//! ([`wasmtime::Store::set_epoch_deadline`]); something has to move it. One thread does, for
//! every module of the process, and only when a call has overrun: it sleeps on the earliest
//! deadline of the calls that are running and on nothing when none is, so an idle page costs it
//! no wakeup at all.

use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

use wasmtime::Engine;

struct Armed {
    id: u64,
    at: Instant,
    engine: Engine,
}

#[derive(Default)]
struct Shared {
    armed: Mutex<(u64, Vec<Armed>)>,
    changed: Condvar,
}

fn shared() -> &'static Arc<Shared> {
    static SHARED: OnceLock<Arc<Shared>> = OnceLock::new();
    SHARED.get_or_init(|| {
        let shared = Arc::new(Shared::default());
        let watching = Arc::clone(&shared);
        // A process that cannot start the thread has no way to stop a runaway module: the
        // modules do not run (the caller finds out when it arms nothing and the call fails).
        let _ = std::thread::Builder::new()
            .name("view-watchdog".into())
            .spawn(move || watch(&watching));
        shared
    })
}

fn watch(shared: &Shared) {
    let Ok(mut state) = shared.armed.lock() else {
        return;
    };
    loop {
        let now = Instant::now();
        // Whatever is due has overrun: its engine's epoch moves, and the module traps at the
        // next loop head or function entry.
        state.1.retain(|armed| {
            let due = armed.at <= now;
            if due {
                armed.engine.increment_epoch();
            }
            !due
        });
        let next = state.1.iter().map(|armed| armed.at).min();
        state = match next {
            Some(at) => match shared
                .changed
                .wait_timeout(state, at.saturating_duration_since(now))
            {
                Ok((state, _)) => state,
                Err(_) => return,
            },
            None => match shared.changed.wait(state) {
                Ok(state) => state,
                Err(_) => return,
            },
        };
    }
}

/// A call that is being timed. Dropping it ends the timing: a call that came back in time
/// never moves the epoch.
pub(crate) struct Guard {
    id: u64,
}

/// Times a call of `engine`'s: after `within` its epoch moves once.
pub(crate) fn arm(engine: &Engine, within: Duration) -> Guard {
    let shared = shared();
    let mut state = shared
        .armed
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    state.0 += 1;
    let id = state.0;
    state.1.push(Armed {
        id,
        at: Instant::now() + within,
        engine: engine.clone(),
    });
    shared.changed.notify_one();
    Guard { id }
}

impl Drop for Guard {
    fn drop(&mut self) {
        let shared = shared();
        let mut state = shared
            .armed
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.1.retain(|armed| armed.id != self.id);
    }
}
