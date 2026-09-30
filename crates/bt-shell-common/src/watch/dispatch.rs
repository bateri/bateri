//! The macOS body: `dispatch2`'s vnode sources on the module's own serial
//! queue.
//!
//! Nothing wakes until a change arrives — no polling, no thread of ours; the
//! kernel queues work on the source's queue when a file is touched. The
//! decision is recorded in `.tasks/007-ayarlar-ve-tema/discussion.md` →
//! Karar 2.
//!
//! - **Directory** (`WRITE | DELETE | RENAME`): an entry being created, deleted
//!   or renamed over — the editor's "write to a temp file, rename over" save.
//!   `DELETE` and `RENAME` report when the directory **itself** goes away: the
//!   next install should drop the stale descriptor.
//! - **File** (`WRITE | EXTEND | ATTRIB | DELETE | RENAME`): an in-place write
//!   (`>>`, nano), truncation without a write (`: >`) and a save to a
//!   symlink's **target** leave no trace in the directory. The file is opened
//!   with `O_EVTONLY` (an event-only descriptor) and the open follows the
//!   link; the source attaches to the target.
//!
//! **Every source shares one private serial queue** ([`queue`]): handlers run
//! one at a time, off the main thread, and a single barrier on that queue
//! ([`Watch::flush`]) fences every source at once — including a cancelled
//! source's handler that is still running.
//!
//! The handler is installed via a function pointer (`set_event_handler_f`):
//! this crate has no `block2` edge. Each source carries a `Box` context holding
//! the fd and the notification; the **cancel handler** drops it, because that
//! is where libdispatch gives the safe moment to close the descriptor —
//! cancellation is asynchronous and a running handler may be reading the
//! context at that time.

use std::ffi::c_void;
use std::fs::{File, OpenOptions};
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use dispatch2::{
    _dispatch_source_type_vnode, DispatchObject, DispatchQueue, DispatchRetained, DispatchSource,
    dispatch_source_vnode_flags_t as Vnode,
};

use super::Notify;

/// Directory events; the rationale is at the top of the module.
const DIR_EVENTS: usize = (Vnode::DISPATCH_VNODE_WRITE.0
    | Vnode::DISPATCH_VNODE_DELETE.0
    | Vnode::DISPATCH_VNODE_RENAME.0) as usize;

/// File events; `EXTEND` is append's (`>>`) own flag.
///
/// `ATTRIB` is for truncation without a write (`: > file`, `truncate -s 0`):
/// kqueue reports it as an attribute event, not `WRITE`. The cost is that
/// `touch` and `chmod` also trigger a reread, and that is an empty diff. Reading
/// itself does **not** produce an event (`reading_does_not_notify`); if it did,
/// the applier that installs the source and reads on every event would wake
/// itself up forever.
const FILE_EVENTS: usize = (Vnode::DISPATCH_VNODE_WRITE.0
    | Vnode::DISPATCH_VNODE_EXTEND.0
    | Vnode::DISPATCH_VNODE_ATTRIB.0
    | Vnode::DISPATCH_VNODE_DELETE.0
    | Vnode::DISPATCH_VNODE_RENAME.0) as usize;

/// The module's serial queue; created on first install and never released.
fn queue() -> &'static DispatchQueue {
    static QUEUE: OnceLock<DispatchRetained<DispatchQueue>> = OnceLock::new();
    QUEUE.get_or_init(|| DispatchQueue::new("dev.bateri.watch", None))
}

/// The sources of a list of paths. All are cancelled on drop.
pub struct Watch {
    pub(super) sources: Vec<DispatchRetained<DispatchSource>>,
}

/// The source's context: the fd must stay open as long as the source lives, and
/// the notification is called once per event.
struct Context {
    /// Never read; holding it keeps the fd open and dropping it closes it.
    _file: File,
    notify: Notify,
}

impl Watch {
    /// Installs sources on those of `paths` that exist; events call `notify`
    /// on the module's queue (the contract is in the parent module).
    pub fn install(paths: &[PathBuf], notify: &Notify) -> Self {
        Self {
            sources: paths
                .iter()
                .filter_map(|path| arm(path, queue(), notify))
                .collect(),
        }
    }

    /// The tests' barrier: returns once every handler queued so far — of any
    /// source, cancelled ones included — has run.
    #[cfg(test)]
    pub fn flush(&self) {
        queue().exec_sync(|| {});
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        // Cancellation is asynchronous: the cancel handler releases the context
        // and the fd. The source itself lives on libdispatch's reference until
        // then.
        for source in &self.sources {
            source.cancel();
        }
    }
}

/// The source for a single path; `None` if the path does not exist, is neither a
/// directory nor a regular file, or cannot be opened.
///
/// The kind is asked via `metadata` **before** opening: opening a FIFO
/// read-only would block the caller until a writer arrives (the same filter
/// as `settings::read_text`). `metadata` follows the link, and so does the
/// open.
fn arm(
    path: &Path,
    queue: &DispatchQueue,
    notify: &Notify,
) -> Option<DispatchRetained<DispatchSource>> {
    let meta = std::fs::metadata(path).ok()?;
    let mask = if meta.is_dir() {
        DIR_EVENTS
    } else if meta.is_file() {
        FILE_EVENTS
    } else {
        return None;
    };
    // A path that cannot be opened (permissions) is silently not watched: the
    // reader already writes the error to the subtitle when it reads the same
    // path.
    //
    // `O_EVTONLY`, not read-only (a `/code-review` finding): the descriptor is
    // for events only. A read descriptor would keep an external disk holding
    // the link's target "in use" and prevent its ejection, and on a file
    // evicted to iCloud it could trigger a download on every reinstall.
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_EVTONLY)
        .open(path)
        .ok()?;
    let fd = file.as_raw_fd();
    let context = Box::into_raw(Box::new(Context {
        _file: file,
        notify: Arc::clone(notify),
    }));
    // SAFETY: the type is libdispatch's vnode constant and the mask is that
    // type's flags; the handle is an open fd and `context` holds it until the
    // cancel handler. The queue comes from a live reference; the source retains
    // it too. `new` returns NULL only on invalid arguments.
    let source = unsafe {
        DispatchSource::new(
            (&raw const _dispatch_source_type_vnode).cast_mut(),
            fd as usize,
            mask,
            Some(queue),
        )
    };
    // The order is mandatory: the context passed to the handler is the context
    // **at the moment the handler is installed** (`set_event_handler_f`'s doc)
    // — first the context, then the handlers, activation last. An unactivated
    // source cannot be dropped either.
    //
    // SAFETY: `context` is a `Box<Context>`; the event handler only reads it,
    // the cancel handler takes it back once, and libdispatch calls the cancel
    // handler only after a running event handler finishes, and never calls the
    // event handler after cancellation.
    unsafe { source.set_context(context.cast()) };
    source.set_event_handler_f(on_event);
    source.set_cancel_handler_f(on_cancel);
    source.activate();
    Some(source)
}

extern "C" fn on_event(context: *mut c_void) {
    // SAFETY: the `Box<Context>` installed by `arm`; the cancel handler has not
    // run yet (the contract is in `arm`).
    let context = unsafe { &*context.cast::<Context>() };
    (context.notify)();
}

extern "C" fn on_cancel(context: *mut c_void) {
    // SAFETY: the `Box<Context>` installed by `arm`; the cancel handler runs
    // once per source and the event handler is not called after it.
    drop(unsafe { Box::from_raw(context.cast::<Context>()) });
}
