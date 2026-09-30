//! The Linux body: one inotify instance and one thread per [`Watch`].
//!
//! The thread sleeps in `poll` on two descriptors — the inotify instance and a
//! wake `eventfd` of its own — so nothing runs until the kernel queues an
//! event or the owner asks (stop, flush). The masks mirror the macOS body's:
//!
//! - **Directory** (`CREATE | DELETE | MOVED_FROM | MOVED_TO | DELETE_SELF |
//!   MOVE_SELF`): an entry being created, deleted or renamed over — the
//!   editor's "write to a temp file, rename over" save — and the directory
//!   **itself** going away.
//! - **File** (`MODIFY | ATTRIB | DELETE_SELF | MOVE_SELF`): an in-place write
//!   (`>>`, nano; append is `MODIFY` here), truncation without a write
//!   (`: >`; `MODIFY` too, `ATTRIB` is kept for parity) and a save to a
//!   symlink's **target** — `inotify_add_watch` follows the link unless told
//!   not to, so the watch attaches to the target.
//!
//! No access, open or close-without-write bit is in either mask: reading does
//! **not** notify (`reading_does_not_notify`).
//!
//! Events are not parsed: one wake-up's drain — every read until the queue is
//! empty, whatever it holds (including `IN_IGNORED` and `IN_Q_OVERFLOW`) — is
//! at most one notification — the caller rereads
//! and an extra notification is an empty diff.
//!
//! **Descriptors are never closed under a reader.** The thread owns the
//! inotify descriptor and closes it when it exits; the wake descriptor is
//! shared (`Arc<OwnedFd>`) between the thread and the [`Watch`], so `Drop`
//! writing to it after the thread has already exited still writes to a live
//! descriptor, never a closed or reused number.
//!
//! **After `Drop` returns, no notification follows.** The thread checks the
//! stop bit and calls `notify` under the same mutex `Drop` sets it under, so
//! `Drop` waits at most for one notification in flight, never for the thread.
//! Hence the contract's rule that `notify` neither blocks nor drops its own
//! `Watch` (the parent module).

use std::ffi::{CString, c_int, c_void};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};

use super::Notify;

/// Directory events; the rationale is at the top of the module.
const DIR_EVENTS: u32 = libc::IN_CREATE
    | libc::IN_DELETE
    | libc::IN_MOVED_FROM
    | libc::IN_MOVED_TO
    | libc::IN_DELETE_SELF
    | libc::IN_MOVE_SELF;

/// File events; the rationale is at the top of the module.
const FILE_EVENTS: u32 =
    libc::IN_MODIFY | libc::IN_ATTRIB | libc::IN_DELETE_SELF | libc::IN_MOVE_SELF;

/// The read buffer; large enough for the biggest single event
/// (`sizeof(inotify_event) + NAME_MAX + 1`), otherwise `read` fails with
/// `EINVAL`.
const BUFFER: usize = 4096;

/// The watches of a list of paths. The thread is stopped on drop.
pub struct Watch {
    /// The watch descriptors, one per path armed; only the tests count them.
    /// Closing the inotify descriptor removes them all, so `Drop` needs none.
    #[cfg_attr(
        not(test),
        allow(dead_code, reason = "counted by the tests, parity with macOS")
    )]
    pub(super) sources: Vec<c_int>,
    /// `None` when nothing was armed: no thread is spawned.
    worker: Option<Worker>,
}

/// The owner's handle on the thread.
struct Worker {
    shared: Arc<Shared>,
    wake: Arc<OwnedFd>,
}

/// State shared with the thread.
#[derive(Default)]
struct Shared {
    state: Mutex<State>,
    /// Signalled whenever `State::flushed` advances.
    flushed: Condvar,
}

#[derive(Default)]
struct State {
    /// Set by `Drop`; the thread exits at its next wake-up.
    stopped: bool,
    /// Flush requests made so far (`Watch::flush`).
    requested: u64,
    /// The last request whose events have all been notified; `u64::MAX` once
    /// the thread has exited, so no flush waits on a dead thread.
    flushed: u64,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        // A panicking `notify` poisons the mutex; the state is still coherent
        // (plain counters and a bit), so carry on.
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Watch {
    /// Installs watches on those of `paths` that exist; events call `notify`
    /// on this watch's thread (the contract is in the parent module).
    pub fn install(paths: &[PathBuf], notify: &Notify) -> Self {
        let empty = Self {
            sources: Vec::new(),
            worker: None,
        };
        // SAFETY: plain syscall; the flags are inotify's own.
        let raw = unsafe { libc::inotify_init1(libc::IN_NONBLOCK | libc::IN_CLOEXEC) };
        if raw < 0 {
            return empty;
        }
        // SAFETY: `raw` is a fresh descriptor owned by nobody else.
        let inotify = unsafe { OwnedFd::from_raw_fd(raw) };
        let sources: Vec<c_int> = paths.iter().filter_map(|p| arm(&inotify, p)).collect();
        if sources.is_empty() {
            return empty;
        }
        // SAFETY: plain syscall.
        let raw = unsafe { libc::eventfd(0, libc::EFD_NONBLOCK | libc::EFD_CLOEXEC) };
        if raw < 0 {
            return empty;
        }
        // SAFETY: `raw` is a fresh descriptor owned by nobody else.
        let wake = Arc::new(unsafe { OwnedFd::from_raw_fd(raw) });
        let shared = Arc::new(Shared::default());
        let spawned = std::thread::Builder::new()
            .name("bateri-watch".into())
            .spawn({
                let wake = Arc::clone(&wake);
                let shared = Arc::clone(&shared);
                let notify = Arc::clone(notify);
                move || run(&inotify, &wake, &shared, &notify)
            });
        // A thread that cannot be spawned watches nothing, and says so in the
        // count; the caller treats it like a missing path.
        match spawned {
            Ok(_detached) => Self {
                sources,
                worker: Some(Worker { shared, wake }),
            },
            Err(_) => empty,
        }
    }

    /// The tests' barrier: returns once every event queued before the call has
    /// been notified.
    ///
    /// A mutating syscall queues its inotify event before it returns, so
    /// "queued before the call" is "caused before the call". A dropped
    /// `Watch`'s thread needs no fence: it notifies nothing after `Drop`.
    #[cfg(test)]
    pub fn flush(&self) {
        let Some(worker) = &self.worker else {
            return;
        };
        let mut state = worker.shared.lock();
        state.requested += 1;
        let target = state.requested;
        wake(&worker.wake);
        while state.flushed < target {
            state = worker
                .shared
                .flushed
                .wait(state)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        // Does not wait for the thread: it sees the bit at its next wake-up,
        // closes the inotify descriptor and exits.
        if let Some(worker) = &self.worker {
            worker.shared.lock().stopped = true;
            wake(&worker.wake);
        }
    }
}

/// The watch for a single path; `None` if the path does not exist, is neither
/// a directory nor a regular file, or cannot be watched (permissions).
///
/// The same filter as the macOS body: a FIFO or a device is not a settings
/// file. `metadata` follows the link, and so does `inotify_add_watch`.
fn arm(inotify: &OwnedFd, path: &Path) -> Option<c_int> {
    let meta = std::fs::metadata(path).ok()?;
    let mask = if meta.is_dir() {
        DIR_EVENTS
    } else if meta.is_file() {
        FILE_EVENTS
    } else {
        return None;
    };
    let path = CString::new(path.as_os_str().as_bytes()).ok()?;
    // SAFETY: a live inotify descriptor and a NUL-terminated path.
    let wd = unsafe { libc::inotify_add_watch(inotify.as_raw_fd(), path.as_ptr(), mask) };
    (wd >= 0).then_some(wd)
}

/// Adds one to the wake counter; never blocks (`EFD_NONBLOCK`, and the counter
/// cannot realistically overflow).
fn wake(fd: &OwnedFd) {
    let one: u64 = 1;
    // SAFETY: a live eventfd and an 8-byte buffer, as eventfd requires.
    let _ = unsafe {
        libc::write(
            fd.as_raw_fd(),
            (&raw const one).cast::<c_void>(),
            size_of::<u64>(),
        )
    };
}

/// The thread's body; returns (and closes the inotify descriptor) on stop or
/// on a descriptor error.
fn run(inotify: &OwnedFd, wake: &OwnedFd, shared: &Shared, notify: &Notify) {
    let mut buffer = [0u8; BUFFER];
    loop {
        let mut fds = [
            libc::pollfd {
                fd: inotify.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: wake.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        // SAFETY: two initialised `pollfd`s on live descriptors.
        let ready = unsafe { libc::poll(fds.as_mut_ptr(), 2, -1) };
        if ready < 0 {
            if std::io::Error::last_os_error().raw_os_error() == Some(libc::EINTR) {
                continue;
            }
            break;
        }
        let broken = libc::POLLERR | libc::POLLHUP | libc::POLLNVAL;
        if fds.iter().any(|fd| fd.revents & broken != 0) {
            break;
        }
        if fds[1].revents != 0 {
            let mut counter: u64 = 0;
            // SAFETY: a live eventfd and an 8-byte buffer; resets the counter.
            let _ = unsafe {
                libc::read(
                    wake.as_raw_fd(),
                    (&raw mut counter).cast::<c_void>(),
                    size_of::<u64>(),
                )
            };
        }
        // The request is read **before** draining: every event caused before
        // that flush call is already queued, so the drain below covers it.
        let target = shared.lock().requested;
        let changed = drain(inotify, &mut buffer);
        let mut state = shared.lock();
        if state.stopped {
            break;
        }
        if changed {
            notify();
        }
        state.flushed = target;
        shared.flushed.notify_all();
    }
    shared.lock().flushed = u64::MAX;
    shared.flushed.notify_all();
}

/// Reads the inotify queue until it is empty; `true` if anything was read.
fn drain(inotify: &OwnedFd, buffer: &mut [u8; BUFFER]) -> bool {
    let mut changed = false;
    loop {
        // SAFETY: a live, non-blocking descriptor and a buffer of the stated
        // length.
        let read = unsafe {
            libc::read(
                inotify.as_raw_fd(),
                buffer.as_mut_ptr().cast::<c_void>(),
                buffer.len(),
            )
        };
        if read > 0 {
            changed = true;
            continue;
        }
        if read < 0 && std::io::Error::last_os_error().raw_os_error() == Some(libc::EINTR) {
            continue;
        }
        // `EAGAIN`: the queue is empty. Any other error ends this drain; a
        // broken descriptor shows up in the next `poll`.
        return changed;
    }
}
