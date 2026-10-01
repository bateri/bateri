//! Downloading a remote item to this Mac (045 Karar 11): `upload`'s mirror. The
//! remote side `tar c`s one item to its standard output
//! ([`crate::remote_files::download_script`]), the bytes pass through us
//! ([`crate::upload`]'s `TarWatcher`, so progress is exact) and a local
//! `/usr/bin/tar x` unpacks them.
//!
//! **Nothing half-written ever takes the item's name.** The archive unpacks into a
//! hidden temporary folder next to the destination (same folder, same volume) and
//! only the finished item is moved out with one `rename`; cancel, failure and a full
//! disk delete the temporary folder. Only the expected item leaves it — anything
//! else a hostile archive carries is deleted with the folder (local tar already
//! refuses `..` and absolute paths). tar keeps the remote mtime: the local half of
//! the preview cache's "did it change" question.
//!
//! On a background thread, like the upload; the queue is `upload::Transfers`.

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use crate::remote_files::{download_script, split_remote};
use crate::upload::{
    NO_DIRECTORY, Outcome, Shared, TICK, TarWatcher, collect_stderr, last_line, remote_command,
    wait_untracked,
};

/// What happens when the landing name is taken (045 R4, `download_conflict`):
/// keep both — the new item takes the next free `name 2` — or replace the old one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Conflict {
    #[default]
    KeepBoth,
    Replace,
}

/// The temporary folders' counter: unique within this process, the pid makes it
/// unique across processes.
static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

/// The prefix of the hidden temporary folder an item unpacks into (the preview
/// cache's sweep skips such folders: they are a stream in flight).
pub const TEMP_PREFIX: &str = ".bateri-download-";

/// Downloads the remote absolute path `remote` to `landing`: `ssh … tar c` remotely,
/// `tar x` locally into a temporary folder next to `landing` (its folder is
/// created here, when the stream starts — nothing is made on disk before the
/// user confirmed, 045 phase-4), then `seal` (the
/// caller's last word on the finished item — the quarantine mark, while it is still
/// hidden) and one `rename` to `landing`, or by `conflict` to its next free name.
/// **On a background thread**; `tick` posts the progress report to the main queue (at
/// most once per [`TICK`]).
///
/// Returns the outcome and where the item landed (`Done` only).
pub fn transfer(
    ssh: &[String],
    remote: &str,
    landing: &Path,
    conflict: Conflict,
    shared: &Arc<Shared>,
    tick: impl Fn(),
    seal: impl FnOnce(&Path),
) -> (Outcome, Option<PathBuf>) {
    let (Some(script), Some((_, name))) = (download_script(remote), split_remote(remote)) else {
        return (
            Outcome::Failed(format!("{remote} can't be downloaded safely")),
            None,
        );
    };
    let Some(parent) = landing.parent().filter(|_| landing.file_name().is_some()) else {
        return (
            Outcome::Failed(format!("{} is not a file name", landing.display())),
            None,
        );
    };
    if let Err(error) = fs::create_dir_all(parent) {
        return (
            Outcome::Failed(format!("{} can't be written: {error}", parent.display())),
            None,
        );
    }
    let temp = match make_temp(parent) {
        Ok(temp) => temp,
        Err(error) => {
            return (
                Outcome::Failed(format!("{} can't be written: {error}", parent.display())),
                None,
            );
        }
    };
    let result = match stream(ssh, &script, &temp, shared, tick) {
        Outcome::Done => match land(&temp.join(name), landing, conflict, seal) {
            Ok(landed) => (Outcome::Done, Some(landed)),
            Err(reason) => (Outcome::Failed(reason), None),
        },
        outcome => (outcome, None),
    };
    let _ = fs::remove_dir_all(&temp);
    result
}

/// Creates the hidden temporary folder in `parent`; a name left behind by an earlier
/// process is skipped.
fn make_temp(parent: &Path) -> std::io::Result<PathBuf> {
    loop {
        let serial = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let temp = parent.join(format!("{TEMP_PREFIX}{}-{serial}", std::process::id()));
        match fs::create_dir(&temp) {
            Ok(()) => return Ok(temp),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
}

/// The stream: the remote `tar c`'s output through [`TarWatcher`] into the local
/// `tar x`. Cancel and disk full kill both processes ([`Shared`]).
fn stream(
    ssh: &[String],
    script: &str,
    temp: &Path,
    shared: &Arc<Shared>,
    tick: impl Fn(),
) -> Outcome {
    let local_tar = Command::new("/usr/bin/tar")
        .args(["-x", "-f", "-", "-C"])
        .arg(temp)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn();
    let mut local_tar = match local_tar {
        Ok(child) => child,
        Err(error) => return Outcome::Failed(format!("tar could not be started: {error}")),
    };
    let remote = Command::new(&ssh[0])
        .args(&ssh[1..])
        .arg(remote_command(script))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    let mut remote = match remote {
        Ok(child) => child,
        Err(error) => {
            let _ = local_tar.kill();
            let _ = local_tar.wait();
            return Outcome::Failed(format!("ssh could not be started: {error}"));
        }
    };
    shared.track(&[&remote, &local_tar]);

    let remote_err = collect_stderr(remote.stderr.take(), None);
    // The full disk is this Mac's: the local tar says so.
    let local_err = collect_stderr(local_tar.stderr.take(), Some(Arc::clone(shared)));
    let mut watcher = TarWatcher::default();
    if let (Some(mut source), Some(mut sink)) = (remote.stdout.take(), local_tar.stdin.take()) {
        let mut buffer = vec![0; 64 * 1024];
        let mut last = Instant::now();
        loop {
            let read = match source.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(read) => read,
            };
            watcher.feed(&buffer[..read]);
            if sink.write_all(&buffer[..read]).is_err() {
                break;
            }
            shared.bytes.store(watcher.bytes, Ordering::Release);
            shared.files.store(watcher.files, Ordering::Release);
            if last.elapsed() >= TICK {
                last = Instant::now();
                tick();
            }
        }
        // Both ends close here: the local tar sees the end of the stream, and if it
        // stopped early ssh's next write fails instead of blocking on a full pipe.
    }
    let remote_status = wait_untracked(&mut remote, shared);
    let local_status = wait_untracked(&mut local_tar, shared);
    let remote_err = remote_err.join().unwrap_or_default();
    let local_err = local_err.join().unwrap_or_default();

    if shared.disk_full.load(Ordering::Acquire) {
        return Outcome::DiskFull;
    }
    if shared.cancel.load(Ordering::Acquire) {
        return Outcome::Cancelled;
    }
    // The remote side is the cause when it says why; a silent remote failure after a
    // local one is the consequence (the broken pipe).
    let remote_failed = remote_status.as_ref().map_or_else(
        |error| Some(error.to_string()),
        |status| {
            (!status.success()).then(|| match status.code() {
                Some(NO_DIRECTORY) => "the remote folder can no longer be opened".to_owned(),
                code => {
                    let line = last_line(&remote_err);
                    if line.is_empty() {
                        format!("ssh exited with {}", code.unwrap_or(-1))
                    } else {
                        line.to_owned()
                    }
                }
            })
        },
    );
    let local_failed = (!local_status.is_ok_and(|status| status.success())).then(|| {
        let line = last_line(&local_err);
        if line.is_empty() {
            "the local tar failed".to_owned()
        } else {
            line.to_owned()
        }
    });
    let remote_spoke = !last_line(&remote_err).is_empty()
        || remote_status
            .as_ref()
            .is_ok_and(|status| status.code() == Some(NO_DIRECTORY));
    match (remote_failed, local_failed) {
        (Some(reason), _) if remote_spoke => Outcome::Failed(reason),
        (_, Some(reason)) | (Some(reason), None) => Outcome::Failed(reason),
        (None, None) => Outcome::Done,
    }
}

/// Moves the unpacked `item` to `landing` (or its next free name), after `seal`.
fn land(
    item: &Path,
    landing: &Path,
    conflict: Conflict,
    seal: impl FnOnce(&Path),
) -> Result<PathBuf, String> {
    let meta = fs::symlink_metadata(item)
        .map_err(|_| "the remote item did not arrive in the archive".to_owned())?;
    seal(item);
    let target = match conflict {
        Conflict::KeepBoth => keep_both_name(landing, meta.is_dir(), |path| {
            fs::symlink_metadata(path).is_ok()
        }),
        Conflict::Replace => {
            // A file over a file: `rename` replaces it atomically. Anything else
            // (a folder on either side) cannot be renamed over: the old item is
            // moved aside first and removed only once the new one is in place.
            if let Ok(old) = fs::symlink_metadata(landing)
                && (old.is_dir() || meta.is_dir())
            {
                // A file never replaces a folder: "Replace" was asked about a
                // name, and a whole folder of the user's must not go silently
                // under `download_conflict = "replace"` (`/code-review`, 045).
                if old.is_dir() && !meta.is_dir() {
                    return Err(format!(
                        "{} is a folder; a file does not replace it",
                        landing.display()
                    ));
                }
                return replace_aside(item, landing, old.is_dir());
            }
            landing.to_owned()
        }
    };
    fs::rename(item, &target)
        .map_err(|error| format!("{} can't be written: {error}", target.display()))?;
    Ok(target)
}

/// Replaces `landing` with `item` when a `rename` cannot do it in one step: the
/// old item goes to a hidden sibling (the download's temporary prefix, so the
/// preview sweep skips it), the new one takes its name, then the old one is
/// removed. If the new one cannot take the name the old one comes back — a
/// failure never leaves the user with neither.
fn replace_aside(item: &Path, landing: &Path, old_dir: bool) -> Result<PathBuf, String> {
    let parent = landing.parent().unwrap_or(Path::new("."));
    let aside = parent.join(format!("{TEMP_PREFIX}replaced-{}", std::process::id()));
    fs::rename(landing, &aside)
        .map_err(|error| format!("{} can't be replaced: {error}", landing.display()))?;
    if let Err(error) = fs::rename(item, landing) {
        let _ = fs::rename(&aside, landing);
        return Err(format!("{} can't be written: {error}", landing.display()));
    }
    let _ = if old_dir {
        fs::remove_dir_all(&aside)
    } else {
        fs::remove_file(&aside)
    };
    Ok(landing.to_owned())
}

/// "Keep both" (045 R4): `path` if it is free, otherwise the first free `name 2`,
/// `name 3` … — Finder's form, the number before the **last** extension
/// (`a.tar 2.gz`). A folder and a dot file (`.bashrc`) have no extension.
pub fn keep_both_name(path: &Path, dir: bool, taken: impl Fn(&Path) -> bool) -> PathBuf {
    if !taken(path) {
        return path.to_owned();
    }
    let parent = path.parent().unwrap_or(Path::new(""));
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let (stem, extension) = match name.rfind('.') {
        Some(at) if at > 0 && !dir => name.split_at(at),
        _ => (name.as_str(), ""),
    };
    let mut number: u64 = 2;
    loop {
        let candidate = parent.join(format!("{stem} {number}{extension}"));
        if !taken(&candidate) {
            return candidate;
        }
        number += 1;
    }
}

/// Free bytes on the volume that holds `dir` (`statvfs`: the blocks an
/// unprivileged process may use) — the download sheet's "not enough space"
/// (045 R4). A folder that does not exist yet (it is created when the stream
/// starts) is asked through its nearest existing ancestor. `None` if it cannot be
/// asked; blocking (a network volume), so off the main thread.
pub fn free_space(dir: &Path) -> Option<u64> {
    use std::os::unix::ffi::OsStrExt;
    let dir = dir.ancestors().find(|at| at.exists())?;
    let path = std::ffi::CString::new(dir.as_os_str().as_bytes()).ok()?;
    let mut stat = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    // SAFETY: `path` is a NUL-terminated string that outlives the call and
    // `stat` is a writable `statvfs` the call fills on success.
    if unsafe { libc::statvfs(path.as_ptr(), stat.as_mut_ptr()) } != 0 {
        return None;
    }
    // SAFETY: `statvfs` returned 0, so it filled `stat`.
    let stat = unsafe { stat.assume_init() };
    // `fsblkcnt_t` is `u32` on macOS and `u64` on Linux, `c_ulong` is `u64` on
    // both: a conversion is the identity on some target.
    #[allow(clippy::useless_conversion)]
    let (blocks, size) = (u64::from(stat.f_bavail), u64::from(stat.f_frsize));
    blocks.checked_mul(size)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("bt-download-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp directory");
        dir
    }

    /// A local shell in place of ssh: `sh -c "<remote command>"` — the very script and
    /// stream that would run remotely, without a connection (`upload`'s precedent).
    fn local_ssh() -> Vec<String> {
        vec!["/bin/sh".to_owned(), "-c".to_owned()]
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    fn seconds(time: SystemTime) -> u64 {
        time.duration_since(UNIX_EPOCH).unwrap().as_secs()
    }

    #[test]
    fn a_file_lands_whole_through_a_hidden_temporary_with_its_remote_mtime() {
        let root = scratch("file");
        let remote = root.join("it's remote");
        let downloads = root.join("Downloads");
        fs::create_dir_all(&remote).unwrap();
        fs::create_dir_all(&downloads).unwrap();
        let source = remote.join("report.txt");
        fs::write(&source, vec![b'r'; 3000]).unwrap();
        let old = UNIX_EPOCH + Duration::from_secs(1_000_000_000);
        fs::File::options()
            .write(true)
            .open(&source)
            .unwrap()
            .set_modified(old)
            .unwrap();

        let shared = Arc::new(Shared::default());
        let sealed = std::cell::RefCell::new(None);
        let (outcome, landed) = transfer(
            &local_ssh(),
            source.to_str().unwrap(),
            &downloads.join("report.txt"),
            Conflict::KeepBoth,
            &shared,
            || {},
            |item| *sealed.borrow_mut() = Some(item.to_owned()),
        );
        assert_eq!(outcome, Outcome::Done);
        assert_eq!(landed, Some(downloads.join("report.txt")));
        assert_eq!(fs::read(downloads.join("report.txt")).unwrap().len(), 3000);
        assert_eq!(
            seconds(
                fs::metadata(downloads.join("report.txt"))
                    .unwrap()
                    .modified()
                    .unwrap()
            ),
            seconds(old),
            "tar keeps the remote mtime"
        );
        assert!(shared.progress().0 >= 3000, "{:?}", shared.progress());
        // The seal saw the item while it was still hidden, and the temporary is gone.
        let sealed = sealed.into_inner().expect("sealed");
        let hidden = sealed.parent().unwrap().file_name().unwrap();
        assert!(
            hidden.to_string_lossy().starts_with(TEMP_PREFIX),
            "{sealed:?}"
        );
        assert_eq!(names(&downloads), ["report.txt"]);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_linked_item_lands_as_its_target() {
        // `/code-review` (045): the helper answered for the target (`stat -L`).
        let root = scratch("link");
        let remote = root.join("remote");
        let downloads = root.join("Downloads");
        fs::create_dir_all(remote.join("available")).unwrap();
        fs::create_dir_all(&downloads).unwrap();
        fs::write(remote.join("available/default"), b"server {}").unwrap();
        std::os::unix::fs::symlink("available/default", remote.join("default")).unwrap();
        let (outcome, landed) = transfer(
            &local_ssh(),
            remote.join("default").to_str().unwrap(),
            &downloads.join("default"),
            Conflict::KeepBoth,
            &Arc::new(Shared::default()),
            || {},
            |_| {},
        );
        assert_eq!(outcome, Outcome::Done);
        let landed = landed.expect("landed");
        assert!(fs::symlink_metadata(&landed).unwrap().is_file());
        assert_eq!(fs::read(&landed).unwrap(), b"server {}");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_taken_name_is_kept_beside_or_replaced() {
        let root = scratch("conflict");
        let remote = root.join("remote");
        let downloads = root.join("Downloads");
        fs::create_dir_all(remote.join("static/css")).unwrap();
        fs::create_dir_all(&downloads).unwrap();
        fs::write(remote.join("report.txt"), b"new").unwrap();
        fs::write(remote.join("static/css/site.css"), b"body{}").unwrap();
        let get = |path: &str, conflict| {
            transfer(
                &local_ssh(),
                remote.join(path).to_str().unwrap(),
                &downloads.join(path),
                conflict,
                &Arc::new(Shared::default()),
                || {},
                |_| {},
            )
        };

        fs::write(downloads.join("report.txt"), b"old").unwrap();
        let (outcome, landed) = get("report.txt", Conflict::KeepBoth);
        assert_eq!(outcome, Outcome::Done);
        assert_eq!(landed, Some(downloads.join("report 2.txt")));
        assert_eq!(fs::read(downloads.join("report.txt")).unwrap(), b"old");
        assert_eq!(fs::read(downloads.join("report 2.txt")).unwrap(), b"new");

        let (_, landed) = get("report.txt", Conflict::Replace);
        assert_eq!(landed, Some(downloads.join("report.txt")));
        assert_eq!(fs::read(downloads.join("report.txt")).unwrap(), b"new");

        // A folder: kept beside as `static 2`, replaced whole.
        let (_, landed) = get("static", Conflict::KeepBoth);
        assert_eq!(landed, Some(downloads.join("static")));
        fs::write(downloads.join("static/stale"), b"x").unwrap();
        let (_, landed) = get("static", Conflict::KeepBoth);
        assert_eq!(landed, Some(downloads.join("static 2")));
        assert_eq!(
            fs::read(downloads.join("static 2/css/site.css")).unwrap(),
            b"body{}"
        );
        let (_, landed) = get("static", Conflict::Replace);
        assert_eq!(landed, Some(downloads.join("static")));
        assert!(!downloads.join("static/stale").exists(), "replaced whole");
        assert_eq!(
            names(&downloads),
            ["report 2.txt", "report.txt", "static", "static 2"],
            "no temporary left"
        );

        // A file never replaces a folder of the same name: the folder stays whole.
        fs::write(remote.join("notes"), b"remote").unwrap();
        fs::create_dir_all(downloads.join("notes")).unwrap();
        fs::write(downloads.join("notes/mine.txt"), b"keep").unwrap();
        let (outcome, landed) = get("notes", Conflict::Replace);
        assert!(matches!(outcome, Outcome::Failed(_)), "{outcome:?}");
        assert_eq!(landed, None);
        assert_eq!(fs::read(downloads.join("notes/mine.txt")).unwrap(), b"keep");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_cancel_or_a_failure_leaves_nothing_behind() {
        let root = scratch("cancel");
        let remote = root.join("remote");
        let downloads = root.join("Downloads");
        fs::create_dir_all(&remote).unwrap();
        fs::create_dir_all(&downloads).unwrap();
        fs::write(remote.join("big.bin"), vec![0u8; 200_000]).unwrap();

        // Cancelled before its turn came: both processes die at once.
        let shared = Arc::new(Shared::default());
        shared.cancel();
        let (outcome, landed) = transfer(
            &local_ssh(),
            remote.join("big.bin").to_str().unwrap(),
            &downloads.join("big.bin"),
            Conflict::KeepBoth,
            &shared,
            || {},
            |_| panic!("a cancelled item is never sealed"),
        );
        assert_eq!((outcome, landed), (Outcome::Cancelled, None));
        assert!(names(&downloads).is_empty(), "{:?}", names(&downloads));

        // The remote folder is gone: its own reason, nothing left.
        let (outcome, landed) = transfer(
            &local_ssh(),
            root.join("gone/big.bin").to_str().unwrap(),
            &downloads.join("big.bin"),
            Conflict::KeepBoth,
            &Arc::new(Shared::default()),
            || {},
            |_| {},
        );
        assert_eq!(
            (outcome, landed),
            (
                Outcome::Failed("the remote folder can no longer be opened".into()),
                None
            )
        );
        // The file is missing in a folder that exists: the remote tar's line.
        let (outcome, _) = transfer(
            &local_ssh(),
            remote.join("missing.bin").to_str().unwrap(),
            &downloads.join("missing.bin"),
            Conflict::KeepBoth,
            &Arc::new(Shared::default()),
            || {},
            |_| {},
        );
        assert!(matches!(outcome, Outcome::Failed(_)), "{outcome:?}");
        assert!(names(&downloads).is_empty(), "{:?}", names(&downloads));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn keep_both_numbers_before_the_last_extension_like_finder() {
        let taken = |names: &'static [&'static str]| {
            move |path: &Path| names.iter().any(|name| path == Path::new("/d").join(name))
        };
        let free = taken(&[]);
        assert_eq!(
            keep_both_name(Path::new("/d/a.txt"), false, free),
            Path::new("/d/a.txt")
        );
        let names = taken(&["a.tar.gz", "a.tar 2.gz", ".bashrc", "my.folder", "static"]);
        assert_eq!(
            keep_both_name(Path::new("/d/a.tar.gz"), false, names),
            Path::new("/d/a.tar 3.gz")
        );
        assert_eq!(
            keep_both_name(Path::new("/d/.bashrc"), false, names),
            Path::new("/d/.bashrc 2")
        );
        assert_eq!(
            keep_both_name(Path::new("/d/my.folder"), true, names),
            Path::new("/d/my.folder 2")
        );
        assert_eq!(
            keep_both_name(Path::new("/d/static"), true, names),
            Path::new("/d/static 2")
        );
    }

    #[test]
    fn free_space_answers_for_a_folder_and_for_one_not_made_yet() {
        let root = scratch("free");
        assert!(free_space(&root).is_some_and(|free| free > 0));
        // The download's folder is created when the stream starts: before that
        // its nearest existing ancestor's volume answers.
        assert!(free_space(&root.join("missing/deeper")).is_some_and(|free| free > 0));
        assert!(!root.join("missing").exists(), "asking makes nothing");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn the_landing_folder_is_made_when_the_stream_starts() {
        let root = scratch("deep");
        let remote = root.join("remote");
        fs::create_dir_all(&remote).unwrap();
        let source = remote.join("app.log");
        fs::write(&source, b"line\n").unwrap();
        // The preview's `{dir}/{host}/{remote path}`: none of it exists yet.
        let landing = root.join("Previews/prod/var/log/app.log");
        let (outcome, landed) = transfer(
            &local_ssh(),
            source.to_str().unwrap(),
            &landing,
            Conflict::Replace,
            &Arc::new(Shared::default()),
            || {},
            |_| {},
        );
        assert_eq!(outcome, Outcome::Done);
        assert_eq!(landed, Some(landing.clone()));
        assert_eq!(fs::read(&landing).unwrap(), b"line\n");
        assert_eq!(
            names(landing.parent().unwrap()),
            ["app.log"],
            "no temporary left"
        );
        let _ = fs::remove_dir_all(&root);
    }
}
