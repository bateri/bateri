//! File watching: `dispatch2`'s vnode sources.
//!
//! Nothing wakes until a change arrives — no polling, no thread; the kernel
//! queues work on the source's queue when a file is touched. The decision is
//! recorded in `.tasks/007-ayarlar-ve-tema/discussion.md` → Karar 2.
//!
//! **Knows nothing about settings:** which paths to watch is the caller's
//! business (`settings`'s path helpers, `app`'s applier); this module looks at
//! whether a path is a directory or a file and installs its source. Two kinds:
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
//! **Installation is one-shot.** The source outlives the event, but the inode
//! it watches may no longer be at the path (a file renamed over). The caller
//! reinstalls on every event and the order is **install first, then read**:
//! the other way round, a save landing between the read and the install
//! produces no event and stale content stays on screen. The cost of installing
//! first is at most one extra event, and that is an empty diff.
//!
//! **A missing path produces no source and is not an error.** Nothing sees a
//! directory created later (the parent directory is not watched); triggering
//! a reinstall from outside is the caller's job.
//!
//! The handler is installed via a function pointer (`set_event_handler_f`):
//! `bt-shell` has no `block2` edge. Each source carries a `Box` context holding
//! the fd and the notification; the **cancel handler** drops it, because that
//! is where libdispatch gives the safe moment to close the descriptor —
//! cancellation is asynchronous and a running handler may be reading the
//! context at that time.

use std::ffi::c_void;
use std::fs::{File, OpenOptions};
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use dispatch2::{
    _dispatch_source_type_vnode, DispatchObject, DispatchQueue, DispatchRetained, DispatchSource,
    dispatch_source_vnode_flags_t as Vnode,
};

/// An event's notification; runs on the source's queue.
///
/// `Send + Sync`, because the cancel handler drops it on the queue's thread.
/// The production one captures nothing (`app`'s targetless action), so its
/// tie to the main queue lives in the choice of queue, not in the type.
pub(crate) type Notify = Arc<dyn Fn() + Send + Sync>;

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

/// The sources of a list of paths. All are cancelled on drop.
pub(crate) struct Watch {
    sources: Vec<DispatchRetained<DispatchSource>>,
}

/// The source's context: the fd must stay open as long as the source lives, and
/// the notification is called once per event.
struct Context {
    /// Never read; holding it keeps the fd open and dropping it closes it.
    _file: File,
    notify: Notify,
}

impl Watch {
    /// Installs sources on those of `paths` that exist; events call `notify` on
    /// `queue`.
    ///
    /// Installing the new one and dropping the old one **afterwards**
    /// (`slot.replace(..)`) leaves no gap between two installations; two
    /// sources reporting at once during that moment is harmless.
    pub(crate) fn install(paths: &[PathBuf], queue: &DispatchQueue, notify: &Notify) -> Self {
        Self {
            sources: paths
                .iter()
                .filter_map(|path| arm(path, queue, notify))
                .collect(),
        }
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
/// read-only would block the main thread until a writer arrives (the same
/// filter as `settings::read_text`). `metadata` follows the link, and so does
/// the open.
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

#[cfg(test)]
mod tests {
    use std::io::Write as _;
    use std::sync::mpsc::{self, Receiver};
    use std::time::Duration;

    use super::*;
    use crate::settings::{self, TempRoot};

    /// The ceiling on waiting for an event. An event normally arrives within
    /// milliseconds; the ceiling is only how long a failing test waits.
    const EVENT_TIMEOUT: Duration = Duration::from_secs(5);

    /// The test's queue and notification: one `()` on the channel per event.
    struct Probe {
        queue: DispatchRetained<DispatchQueue>,
        notify: Notify,
        events: Receiver<()>,
    }

    impl Probe {
        fn new() -> Self {
            let (sender, events) = mpsc::channel();
            Self {
                // Serial queue: handlers run in order, like the main queue in production.
                queue: DispatchQueue::new("bateri.watch.test", None),
                notify: Arc::new(move || {
                    let _ = sender.send(());
                }),
                events,
            }
        }

        fn install(&self, root: &TempRoot) -> Watch {
            Watch::install(&settings::watched_paths(&root.0), &self.queue, &self.notify)
        }

        /// An event must arrive; if not, fails with `what`.
        fn expect_event(&self, what: &str) {
            assert!(self.events.recv_timeout(EVENT_TIMEOUT).is_ok(), "{what}");
        }

        /// Drains the queue: let the cancelled source's **running** handler
        /// finish (a barrier), then discard the events accumulated on the
        /// channel. A single save producing several events is normal
        /// (directory + file).
        fn drain(&self) {
            self.queue.exec_sync(|| {});
            while self.events.try_recv().is_ok() {}
        }
    }

    fn append(path: &std::path::Path, text: &str) {
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(path)
            .expect("cannot open file");
        file.write_all(text.as_bytes()).expect("write failed");
    }

    /// The editor's save: write to a temp file, rename over.
    fn save_by_rename(root: &TempRoot, text: &str) {
        let tmp = root.0.join("settings.toml.tmp");
        std::fs::write(&tmp, text).expect("write failed");
        std::fs::rename(&tmp, root.0.join(settings::FILE_NAME)).expect("rename failed");
    }

    #[test]
    fn append_in_place_is_seen() {
        // An in-place write (`>>`, nano) leaves no trace in the directory: the
        // event comes from the file's own source.
        let root = TempRoot::new("watch-append");
        std::fs::write(root.0.join(settings::FILE_NAME), "").expect("write failed");
        let probe = Probe::new();
        let watch = probe.install(&root);
        // Root + `settings.toml`; no `themes/`.
        assert_eq!(watch.sources.len(), 2);

        append(&root.0.join(settings::FILE_NAME), "[terminal]\n");
        probe.expect_event("in-place write produced no event");
    }

    #[test]
    fn truncation_without_write_is_seen() {
        // `: > settings.toml` and `truncate -s 0` never write to the file:
        // kqueue gives only an attribute event (`ATTRIB`) and the directory
        // does not change either (a `/code-review` finding, measured on this
        // machine).
        let root = TempRoot::new("watch-truncate");
        std::fs::write(root.0.join(settings::FILE_NAME), "[terminal]\n").expect("write failed");
        let probe = Probe::new();
        let _watch = probe.install(&root);

        std::fs::OpenOptions::new()
            .write(true)
            .open(root.0.join(settings::FILE_NAME))
            .expect("cannot open file")
            .set_len(0)
            .expect("truncate failed");
        probe.expect_event("truncation produced no event");
    }

    #[test]
    fn reading_does_not_notify() {
        // `ATTRIB` is watched and the reader reads the file **after**
        // installing the source on every event: if reading produced an
        // attribute event (access time), the read → event → reinstall → read
        // loop would spin the main thread forever. It waits for a period in
        // which no event arrives; short, but far above the scale at which
        // events arrive (milliseconds).
        let root = TempRoot::new("watch-read");
        std::fs::write(root.0.join(settings::FILE_NAME), "[terminal]\n").expect("write failed");
        let probe = Probe::new();
        let _watch = probe.install(&root);

        for _ in 0..3 {
            let _ = settings::load(&root.0);
        }
        probe.queue.exec_sync(|| {});
        assert!(
            probe
                .events
                .recv_timeout(Duration::from_millis(500))
                .is_err(),
            "reading produced an event: a reread loop follows"
        );
    }

    #[test]
    fn rename_over_is_seen_again_after_reinstall() {
        // A file renamed over is a new inode: the old file source now looks at
        // a deleted file. After reinstalling, both the second rename and an
        // in-place write to the **new** file must be seen — only a source
        // installed on the new file can see the latter.
        let root = TempRoot::new("watch-rename");
        std::fs::write(root.0.join(settings::FILE_NAME), "").expect("write failed");
        let probe = Probe::new();
        let mut watch = probe.install(&root);
        assert_eq!(watch.sources.len(), 2);

        save_by_rename(&root, "[terminal]\nscrollback = 1\n");
        probe.expect_event("rename-over produced no event");
        watch = probe.install(&root);
        assert_eq!(watch.sources.len(), 2);
        probe.drain();

        save_by_rename(&root, "[terminal]\nscrollback = 2\n");
        probe.expect_event("second save after reinstall produced no event");
        watch = probe.install(&root);
        assert_eq!(watch.sources.len(), 2);
        probe.drain();

        append(&root.0.join(settings::FILE_NAME), "# son\n");
        probe.expect_event("in-place write to the new file produced no event");
    }

    #[test]
    fn symlink_target_write_is_seen() {
        // Dotfile repo: `settings.toml` is a link to a file in another
        // directory. Writing to the target leaves no trace in the root
        // directory; the source must have followed the link on open and
        // attached to the target.
        let root = TempRoot::new("watch-symlink");
        let repo = TempRoot::new("watch-symlink-repo");
        let target = repo.0.join("settings.toml");
        std::fs::write(&target, "").expect("write failed");
        std::os::unix::fs::symlink(&target, root.0.join(settings::FILE_NAME))
            .expect("symlink failed");
        let probe = Probe::new();
        let _watch = probe.install(&root);

        append(&target, "[terminal]\n");
        probe.expect_event("write to the link target produced no event");
    }

    #[test]
    fn recreated_directory_is_watched_after_reinstall() {
        // When the directory is deleted an event arrives and the reinstall
        // installs nothing: the path is gone. Nothing sees the recreated
        // directory **on its own** (the parent is not watched, Karar 2); a
        // reinstall on an external trigger installs the sources on the new
        // directory and the next write is seen.
        let root = TempRoot::new("watch-recreate");
        std::fs::write(root.0.join(settings::FILE_NAME), "").expect("write failed");
        let probe = Probe::new();
        let mut watch = probe.install(&root);
        assert_eq!(watch.sources.len(), 2);

        std::fs::remove_dir_all(&root.0).expect("remove failed");
        probe.expect_event("directory deletion produced no event");
        watch = probe.install(&root);
        assert_eq!(watch.sources.len(), 0, "source on a deleted directory");
        probe.drain();

        std::fs::create_dir(&root.0).expect("create_dir failed");
        std::fs::write(root.0.join(settings::FILE_NAME), "").expect("write failed");
        watch = probe.install(&root);
        assert_eq!(watch.sources.len(), 2);
        append(&root.0.join(settings::FILE_NAME), "[terminal]\n");
        probe.expect_event("write in the new directory produced no event");
    }

    #[test]
    fn missing_paths_install_nothing() {
        let root = TempRoot::new("watch-missing");
        let absent = root.0.join("absent");
        let probe = Probe::new();
        let watch = Watch::install(
            &settings::watched_paths(&absent),
            &probe.queue,
            &probe.notify,
        );
        assert_eq!(watch.sources.len(), 0);
    }
}
