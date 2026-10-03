//! File watching: `dispatch2`'s vnode sources on macOS, inotify on Linux.
//!
//! **Knows nothing about settings:** which paths to watch is the caller's
//! business (`settings`'s path helpers, the shell's applier); this module looks
//! at whether a path is a directory or a file and installs its watch. The
//! event masks and their rationale live in each body (`dispatch`, `inotify`).
//!
//! **One notification contract on both platforms**: `notify`
//! runs on the module's **own background** queue (macOS: one private serial
//! queue for every source) or thread (Linux: one per [`Watch`]), never on the
//! caller's thread. Carrying the event to the UI loop is the caller's job —
//! on macOS one `DispatchQueue::main().exec_async`, under winit an
//! `EventLoopProxy` event — so the bodies do not change with the event loop.
//! `notify` must return promptly and must not drop its own `Watch`: on Linux
//! `Drop` waits for a notification in flight (so none follows it).
//!
//! **Installation is one-shot.** The watch outlives the event, but the inode
//! it watches may no longer be at the path (a file renamed over). The caller
//! reinstalls on every event and the order is **install first, then read**:
//! the other way round, a save landing between the read and the install
//! produces no event and stale content stays on screen. The cost of installing
//! first is at most one extra event, and that is an empty diff. Installing the
//! new one and dropping the old one **afterwards** (`slot.replace(..)`) leaves
//! no gap between two installations; two watches reporting at once during that
//! moment is harmless.
//!
//! **A missing path produces no watch and is not an error.** Nothing sees a
//! directory created later (the parent directory is not watched); triggering
//! a reinstall from outside is the caller's job.

use std::sync::Arc;

#[cfg(target_os = "macos")]
mod dispatch;
#[cfg(target_os = "linux")]
mod inotify;

#[cfg(target_os = "macos")]
pub use dispatch::Watch;
#[cfg(target_os = "linux")]
pub use inotify::Watch;

/// An event's notification; runs on the module's background queue or thread.
///
/// `Send + Sync`, because it is called off the installing thread and dropped
/// wherever the body releases it (macOS: the cancel handler, on the queue).
pub type Notify = Arc<dyn Fn() + Send + Sync>;

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

    /// The test's notification: one `()` on the channel per event.
    struct Probe {
        notify: Notify,
        events: Receiver<()>,
    }

    impl Probe {
        fn new() -> Self {
            let (sender, events) = mpsc::channel();
            Self {
                notify: Arc::new(move || {
                    let _ = sender.send(());
                }),
                events,
            }
        }

        fn install(&self, root: &TempRoot) -> Watch {
            Watch::install(&settings::watched_paths(&root.0), &self.notify)
        }

        /// An event must arrive; if not, fails with `what`.
        fn expect_event(&self, what: &str) {
            assert!(self.events.recv_timeout(EVENT_TIMEOUT).is_ok(), "{what}");
        }

        /// Drains the events: let every event caused so far be notified —
        /// a cancelled source's **running** handler included — (the module's
        /// barrier, `Watch::flush`), then discard the events accumulated on
        /// the channel. A single save producing several events is normal
        /// (directory + file).
        fn drain(&self, watch: &Watch) {
            watch.flush();
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
        // does not change either (a review finding, measured on this
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
        let watch = probe.install(&root);

        for _ in 0..3 {
            let _ = settings::load(&root.0);
        }
        watch.flush();
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
        probe.drain(&watch);

        save_by_rename(&root, "[terminal]\nscrollback = 2\n");
        probe.expect_event("second save after reinstall produced no event");
        watch = probe.install(&root);
        assert_eq!(watch.sources.len(), 2);
        probe.drain(&watch);

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
        // directory **on its own** (the parent is not watched); a
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
        probe.drain(&watch);

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
        let watch = Watch::install(&settings::watched_paths(&absent), &probe.notify);
        assert_eq!(watch.sources.len(), 0);
    }
}
