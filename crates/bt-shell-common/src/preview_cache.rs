//! The preview cache on disk (045 R5, R6, Karar 9): the index file, the cached
//! copy's question (R5.4), the finished preview's seal, the rescue of a copy the
//! user changed and the sweep that carries out [`plan_sweep`]'s answer.
//!
//! The rules are `remote_files`' (pure, tested there); this is their I/O —
//! platform-free, so the sweep's own tests run in a temporary folder here and on
//! `make linux`. Every function blocks on the disk and runs **off the main
//! thread**: the helper's worker, the stream thread or the sweep's own thread.
//!
//! **One writer at a time:** the index is read and rewritten by the preview's
//! seal (stream thread), the cached open (helper thread) and the sweep (its own
//! thread); a process-wide lock ([`INDEX_LOCK`]) serialises the
//! read-modify-write. The index is replaced with one `rename`, so a crash never
//! leaves half a file.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use bt_core::PreviewKeep;

use crate::download::{TEMP_PREFIX, keep_both_name};
use crate::remote_files::{
    CacheState, CachedPreview, IndexRecord, PreviewIndex, Sweep, cache_state, plan_sweep,
};

/// The index's name in the preview folder. The host folders never start with a
/// dot (`remote_files::preview_path`), so it cannot clash with a copy.
pub const INDEX_NAME: &str = ".index";

/// The index's temporary name while it is rewritten.
const INDEX_TEMP: &str = ".index.tmp";

/// Serialises every read-modify-write of an index (the module's header).
static INDEX_LOCK: Mutex<()> = Mutex::new(());

fn lock() -> MutexGuard<'static, ()> {
    // A panic while holding the lock leaves nothing half-done on disk (the
    // index is replaced whole), so a poisoned lock is still a lock.
    INDEX_LOCK.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Now, in Unix seconds.
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

/// A file's size and mtime (Unix seconds); `None` if it is not a regular file.
fn size_and_mtime(path: &Path) -> Option<(u64, u64)> {
    let meta = fs::symlink_metadata(path).ok()?;
    if !meta.is_file() {
        return None;
    }
    let mtime = meta
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    Some((meta.len(), mtime))
}

/// The index of `dir`: an empty index if there is none yet, `Err` if it cannot
/// be read or is damaged.
fn load(dir: &Path) -> Result<PreviewIndex, String> {
    match fs::read_to_string(dir.join(INDEX_NAME)) {
        Ok(text) => PreviewIndex::parse(&text)
            .map_err(|damaged| format!("the preview index is damaged ({})", damaged.0)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(PreviewIndex::default()),
        Err(error) => Err(format!("the preview index can't be read: {error}")),
    }
}

/// Writes `index` to `dir` with one `rename`.
fn save(dir: &Path, index: &PreviewIndex) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    let temp = dir.join(INDEX_TEMP);
    fs::write(&temp, index.render())?;
    fs::rename(&temp, dir.join(INDEX_NAME))
}

/// `path`'s key in `dir`'s index: relative, `/`-separated. `None` if `path` is
/// not inside `dir` (the setting moved the folder meanwhile — the copy then has
/// no record, which is the safe side).
fn key(dir: &Path, path: &Path) -> Option<String> {
    let relative = path.strip_prefix(dir).ok()?;
    let key = relative.to_str()?.to_owned();
    (!key.is_empty()).then_some(key)
}

/// Whether the copy at `path` can open as it is (R5.4), given the remote file's
/// size and mtime ([`cache_state`]). A damaged index is no record: a copy then
/// reads as [`CacheState::Diverged`] and is rescued, never overwritten.
pub fn state(dir: &Path, path: &Path, remote: (Option<u64>, Option<u64>)) -> CacheState {
    let _guard = lock();
    let index = load(dir).unwrap_or_default();
    let record = key(dir, path).and_then(|key| index.records.get(&key).copied());
    cache_state(record.as_ref(), size_and_mtime(path), remote)
}

/// The cached copy at `path` was opened at `now`: its age restarts (the sweep
/// counts from the last opening). Nothing if it has no record.
pub fn touch(dir: &Path, path: &Path, now: u64) {
    let _guard = lock();
    let Ok(mut index) = load(dir) else {
        return;
    };
    let Some(record) = key(dir, path).and_then(|key| index.records.get_mut(&key)) else {
        return;
    };
    record.last_open = now;
    let _ = save(dir, &index);
}

/// A preview landed at `path` (the stream thread, before it is reported): made
/// read-only (`0444`) if `read_only`, then recorded as bateri wrote it — size
/// and mtime **after** the mode change, which keeps the mtime — opened at `now`.
///
/// A damaged index is started afresh: the copies it knew become unknown and are
/// never deleted, the safe side. A failure to record leaves the copy unknown too.
pub fn seal(dir: &Path, path: &Path, read_only: bool, now: u64) {
    if read_only {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o444));
    }
    let Some(written) = size_and_mtime(path) else {
        return;
    };
    let Some(key) = key(dir, path) else {
        return;
    };
    let _guard = lock();
    let mut index = load(dir).unwrap_or_default();
    index.records.insert(
        key,
        IndexRecord {
            written,
            last_open: now,
        },
    );
    let _ = save(dir, &index);
}

/// Moves a copy the user changed out of the cache into `download_dir` (its next
/// free name, Finder's "Keep both") and forgets its record; the moved file is made
/// writable again — it is the user's now. Where it went, or why it stayed.
pub fn rescue(dir: &Path, path: &Path, download_dir: &Path) -> Result<PathBuf, String> {
    let _guard = lock();
    let landed = move_out(path, download_dir)?;
    if let (Ok(mut index), Some(key)) = (load(dir), key(dir, path))
        && index.records.remove(&key).is_some()
    {
        let _ = save(dir, &index);
    }
    Ok(landed)
}

/// The move itself (the lock held by the caller).
fn move_out(path: &Path, download_dir: &Path) -> Result<PathBuf, String> {
    use std::os::unix::fs::PermissionsExt;
    let name = path
        .file_name()
        .ok_or_else(|| format!("{} has no name", path.display()))?;
    fs::create_dir_all(download_dir)
        .map_err(|error| format!("{} can't be written: {error}", download_dir.display()))?;
    let target = keep_both_name(&download_dir.join(name), false, |at| {
        fs::symlink_metadata(at).is_ok()
    });
    if fs::rename(path, &target).is_err() {
        // Another volume: copy, then remove the cached one.
        fs::copy(path, &target)
            .map_err(|error| format!("{} can't be written: {error}", target.display()))?;
        let _ = fs::remove_file(path);
    }
    if let Ok(meta) = fs::metadata(&target) {
        let mut mode = meta.permissions();
        mode.set_mode(mode.mode() | 0o200);
        let _ = fs::set_permissions(&target, mode);
    }
    Ok(target)
}

/// What a sweep did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SweepReport {
    /// How many copies it deleted.
    pub deleted: usize,
    /// Where the copies the user changed went (the download folder).
    pub rescued: Vec<PathBuf>,
    /// Why it did nothing, or what it could not do (a damaged index, a copy that
    /// could not be moved). Not shown today; the sweep's next run tries again.
    pub errors: Vec<String>,
}

/// Every copy in the preview folder: regular files at any depth, except the
/// index and a download's temporary folder (a stream in flight). Dot files are
/// copies too (`.bashrc` is a legitimate preview).
fn scan(dir: &Path) -> Vec<(PathBuf, (u64, u64))> {
    let mut found = Vec::new();
    let mut pending = vec![dir.to_owned()];
    while let Some(folder) = pending.pop() {
        let Ok(entries) = fs::read_dir(&folder) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if folder == dir && (name == INDEX_NAME || name == INDEX_TEMP) {
                continue;
            }
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                if !name.starts_with(TEMP_PREFIX) {
                    pending.push(path);
                }
            } else if let Some(stat) = size_and_mtime(&path) {
                found.push((path, stat));
            }
        }
    }
    found
}

/// Removes the empty folders under `dir` (not `dir` itself), deepest first.
fn prune(dir: &Path) {
    fn walk(folder: &Path, root: bool) -> bool {
        let Ok(entries) = fs::read_dir(folder) else {
            return false;
        };
        let mut empty = true;
        for entry in entries.flatten() {
            let path = entry.path();
            let is_dir = entry.file_type().is_ok_and(|kind| kind.is_dir());
            let name = entry.file_name();
            if is_dir && !name.to_string_lossy().starts_with(TEMP_PREFIX) && walk(&path, false) {
                continue;
            }
            empty = false;
        }
        empty && !root && fs::remove_dir(folder).is_ok()
    }
    walk(dir, true);
}

/// Carries out `sweep` on the preview folder `dir` (Karar 9): [`plan_sweep`]'s
/// deletions, its rescues into `download_dir`, the index updated, the emptied
/// folders removed. `keep`, `limit` (bytes) and `now` (Unix seconds) as there.
///
/// **A damaged or unreadable index does nothing at all** — not a deletion, not a
/// move: without it bateri cannot tell its copies from the user's. A folder that
/// does not exist is an empty cache.
pub fn sweep(
    dir: &Path,
    download_dir: &Path,
    sweep: Sweep,
    keep: PreviewKeep,
    limit: u64,
    now: u64,
) -> SweepReport {
    let mut report = SweepReport::default();
    if !dir.is_dir() {
        return report;
    }
    let guard = lock();
    let mut index = match load(dir) {
        Ok(index) => index,
        Err(error) => {
            report.errors.push(error);
            return report;
        }
    };
    let found = scan(dir);
    let previews: Vec<CachedPreview> = found
        .iter()
        .map(|(path, (size, mtime))| {
            let record = key(dir, path).and_then(|key| index.records.get(&key).copied());
            CachedPreview {
                path: path.clone(),
                size: *size,
                mtime: *mtime,
                last_open: record.map_or(0, |record| record.last_open),
                written: record.map(|record| record.written),
            }
        })
        .collect();
    let plan = plan_sweep(&previews, sweep, keep, limit, now);
    for path in &plan.delete {
        match fs::remove_file(path) {
            Ok(()) => {
                report.deleted += 1;
                if let Some(key) = key(dir, path) {
                    index.records.remove(&key);
                }
            }
            Err(error) => report
                .errors
                .push(format!("{} can't be removed: {error}", path.display())),
        }
    }
    for path in &plan.rescue {
        match move_out(path, download_dir) {
            Ok(landed) => {
                report.rescued.push(landed);
                if let Some(key) = key(dir, path) {
                    index.records.remove(&key);
                }
            }
            Err(error) => report.errors.push(error),
        }
    }
    // A record whose copy is gone (deleted by hand) is forgotten.
    index.records.retain(|key, _| {
        found.iter().any(|(path, _)| {
            path.strip_prefix(dir).ok().and_then(Path::to_str) == Some(key.as_str())
        })
    });
    if let Err(error) = save(dir, &index) {
        report
            .errors
            .push(format!("the preview index can't be written: {error}"));
    }
    drop(guard);
    prune(dir);
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    const DAY: u64 = 86_400;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("bt-previews-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp directory");
        dir
    }

    /// A preview as the stream lands it: `bytes` long, the remote mtime, sealed
    /// read-only and recorded as opened at `opened`.
    fn land(dir: &Path, relative: &str, bytes: usize, opened: u64) -> PathBuf {
        let path = dir.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, vec![b'x'; bytes]).unwrap();
        fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(UNIX_EPOCH + Duration::from_secs(1_000_000))
            .unwrap();
        seal(dir, &path, true, opened);
        path
    }

    /// The user unlocked and edited the copy.
    fn edit(path: &Path) {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o644)).unwrap();
        fs::write(path, b"my edit").unwrap();
    }

    #[test]
    fn a_sealed_copy_is_read_only_recorded_and_opens_from_the_cache() {
        use std::os::unix::fs::PermissionsExt;
        let root = scratch("seal");
        let dir = root.join("Previews");
        let path = land(&dir, "prod/var/log/app.log", 120, 7);
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o444
        );
        let index = load(&dir).unwrap();
        assert_eq!(
            index.records.get("prod/var/log/app.log"),
            Some(&IndexRecord {
                written: (120, 1_000_000),
                last_open: 7,
            })
        );
        assert_eq!(
            state(&dir, &path, (Some(120), Some(1_000_000))),
            CacheState::Fresh
        );
        assert_eq!(
            state(&dir, &path, (Some(121), Some(1_000_000))),
            CacheState::Stale
        );
        touch(&dir, &path, 99);
        assert_eq!(
            load(&dir).unwrap().records["prod/var/log/app.log"].last_open,
            99
        );
        edit(&path);
        assert_eq!(
            state(&dir, &path, (Some(120), Some(1_000_000))),
            CacheState::Diverged
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn the_launch_sweep_keeps_by_age_then_trims_the_oldest_over_the_limit() {
        let root = scratch("launch");
        let dir = root.join("Previews");
        let now = 100 * DAY;
        let old = land(&dir, "prod/old.log", 10, now - 9 * DAY);
        let mid = land(&dir, "prod/a/mid.log", 50, now - 3 * DAY);
        let new = land(&dir, "prod/new.log", 50, now - DAY);
        let report = sweep(
            &dir,
            &root.join("Downloads"),
            Sweep::Launch,
            PreviewKeep::Week,
            60,
            now,
        );
        assert_eq!(report.deleted, 2, "{report:?}");
        assert!(report.errors.is_empty(), "{report:?}");
        assert!(!old.exists(), "expired");
        assert!(!mid.exists(), "oldest over the limit");
        assert!(new.exists());
        assert!(!dir.join("prod/a").exists(), "emptied folder pruned");
        assert_eq!(
            load(&dir).unwrap().records.keys().collect::<Vec<_>>(),
            ["prod/new.log"]
        );
        // The daily sweep never removes for size.
        let daily = sweep(
            &dir,
            &root.join("Downloads"),
            Sweep::Daily,
            PreviewKeep::Week,
            0,
            now,
        );
        assert_eq!(daily.deleted, 0);
        assert!(new.exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_changed_copy_is_moved_to_downloads_writable_never_deleted() {
        use std::os::unix::fs::PermissionsExt;
        let root = scratch("rescue");
        let dir = root.join("Previews");
        let downloads = root.join("Downloads");
        let now = 100 * DAY;
        let path = land(&dir, "prod/etc/app.conf", 10, now);
        edit(&path);
        fs::create_dir_all(&downloads).unwrap();
        fs::write(downloads.join("app.conf"), b"taken").unwrap();
        let report = sweep(
            &dir,
            &downloads,
            Sweep::Launch,
            PreviewKeep::Week,
            u64::MAX,
            now,
        );
        assert_eq!(report.deleted, 0);
        assert_eq!(report.rescued, [downloads.join("app 2.conf")]);
        assert_eq!(fs::read(downloads.join("app 2.conf")).unwrap(), b"my edit");
        assert_eq!(fs::read(downloads.join("app.conf")).unwrap(), b"taken");
        assert!(
            fs::metadata(downloads.join("app 2.conf"))
                .unwrap()
                .permissions()
                .mode()
                & 0o200
                != 0,
            "the user's file is writable again"
        );
        assert!(!path.exists());
        assert!(load(&dir).unwrap().records.is_empty());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_damaged_index_deletes_nothing_and_unknown_copies_stay() {
        let root = scratch("damaged");
        let dir = root.join("Previews");
        let now = 100 * DAY;
        let path = land(&dir, "prod/old.log", 10, now - 90 * DAY);
        fs::write(dir.join(INDEX_NAME), "not an index").unwrap();
        for kind in [Sweep::Launch, Sweep::Daily, Sweep::ClearNow] {
            let report = sweep(
                &dir,
                &root.join("Downloads"),
                kind,
                PreviewKeep::Day,
                0,
                now,
            );
            assert_eq!(report.deleted, 0, "{kind:?}");
            assert!(report.rescued.is_empty(), "{kind:?}");
            assert_eq!(report.errors.len(), 1, "{kind:?}");
            assert!(path.exists(), "{kind:?}");
        }
        assert_eq!(
            fs::read_to_string(dir.join(INDEX_NAME)).unwrap(),
            "not an index"
        );
        // A copy with no record (a missing index) is not known to be bateri's.
        fs::remove_file(dir.join(INDEX_NAME)).unwrap();
        let report = sweep(
            &dir,
            &root.join("Downloads"),
            Sweep::ClearNow,
            PreviewKeep::Day,
            0,
            now,
        );
        assert_eq!(report, SweepReport::default());
        assert!(path.exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn clear_now_takes_every_copy_but_not_a_stream_in_flight() {
        let root = scratch("clear");
        let dir = root.join("Previews");
        let now = 100 * DAY;
        let a = land(&dir, "prod/a.log", 1, now);
        let dot = land(&dir, "prod/home/me/.bashrc", 1, now);
        let temp = dir.join(format!("prod/{TEMP_PREFIX}1-1"));
        fs::create_dir_all(&temp).unwrap();
        fs::write(temp.join("half"), b"h").unwrap();
        let report = sweep(
            &dir,
            &root.join("Downloads"),
            Sweep::ClearNow,
            PreviewKeep::Month,
            u64::MAX,
            now,
        );
        assert_eq!(report.deleted, 2, "{report:?}");
        assert!(!a.exists() && !dot.exists());
        assert!(
            temp.join("half").exists(),
            "the stream's temporary is not a copy"
        );
        assert!(dir.join(INDEX_NAME).exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_missing_folder_is_an_empty_cache() {
        let root = scratch("missing");
        let report = sweep(
            &root.join("nope"),
            &root,
            Sweep::Launch,
            PreviewKeep::Day,
            0,
            1,
        );
        assert_eq!(report, SweepReport::default());
        assert!(!root.join("nope").exists());
        let _ = fs::remove_dir_all(&root);
    }
}
