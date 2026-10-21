//! The disk store of the files a program's views are made of: what
//! [`bt_core::pencere::Store`] is, on disk, for every pane of the application.
//!
//! A program sends every file of its bundle at every `open` it makes unless the
//! host says it has the file already, so what lasts between opens is what makes
//! the second open of a tool send nothing. Files are **content addressed**: a
//! file is named by its SHA-256, in lowercase hex, and a name says what is in the
//! file — a program cannot put one thing under another's name, and two
//! programs that ship the same file share it.
//!
//! **What is trusted.** The folder is the application's own (`~/Library/Caches`,
//! under the bundle identifier, so a development build and the installed
//! application have separate ones), but a file in it may be damaged or may be
//! something else that wrote there. A file is believed when its bytes have the
//! digest of its name: the first time this process asks about it, [`DiskStore::contains`]
//! or [`DiskStore::get`] checks it, and what has been checked is remembered **with its
//! size**. A file read again is not hashed again while its size is what it was — the web view
//! asks for a file at every load, and up to 8 MiB hashed each time is what the file queue
//! would spend its time on — and a file that has gone since is not "contained". A file
//! that fails the check is deleted. Nothing is ever served from a file by name alone
//! that was not checked once.
//!
//! **What it keeps.** [`STORE_LIMIT`] bytes: past it, the files **least recently
//! used** go first — a file's modification time is when it was last stored or
//! read — and never the one just stored, so the open it is for finds it.
//!
//! **I/O.** Every function blocks on the disk and runs off the main thread (the
//! pane's reader thread, the file queue); a store may drop an entry or fail to keep one at any
//! time (§8.5 of the protocol), so no error is reported: a file that could not be
//! kept is asked for again at the next `open`. Writes are a temporary file and
//! one `rename`, so a crash leaves no half-written file under a digest's name.
//! **No lock is held across the disk for a read or a write**: the store's lock guards what was
//! checked and is taken for a few instructions, so the write of one file does not make the
//! read of another wait ([`Shared`]). Counting the folder and dropping the oldest files is
//! the one thing done a store at a time, behind a lock of its own that no read takes.
//! Platform-free: the tests run in a temporary folder here and on `make linux`.

use std::collections::HashMap;
use std::fs;
use std::io::Write as _;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::SystemTime;

use bt_core::pencere::{Digest, Shared, Verified};

/// The most bytes the store keeps. A view's bundle is at most 64 MiB (the
/// protocol's limit), so one bundle always fits with room for others; the number
/// is a policy, not a measurement.
pub const STORE_LIMIT: u64 = 256 * 1024 * 1024;

/// The temporary names' prefix: never a digest's, so a sweep leaves them out and
/// a `get` can never reach one.
const TEMP_PREFIX: &str = ".tmp-";

/// Makes the temporary names of one process distinct.
static TEMP_SERIAL: AtomicU64 = AtomicU64::new(0);

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    // Nothing the lock guards is left half-done by a panic.
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A store of bundle files on disk. See the module's documentation.
pub struct DiskStore {
    dir: PathBuf,
    limit: u64,
    state: Mutex<State>,
    /// Held by the store that counts the folder and drops what is over the limit: one at a
    /// time, so that the count it acts on is the folder's as it is. No read takes it.
    counting: Mutex<()>,
    /// How many times a file's bytes were hashed to check them: what the tests count.
    #[cfg(test)]
    hashed: AtomicU64,
}

struct State {
    /// The files whose bytes have been checked against their names in this
    /// process, with their sizes then.
    checked: HashMap<Digest, u64>,
}

impl DiskStore {
    /// A store in `dir` with [`STORE_LIMIT`]. The folder is made at the first
    /// file stored, and nothing is read here.
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self::with_limit(dir, STORE_LIMIT)
    }

    /// A store in `dir` that keeps `limit` bytes.
    pub fn with_limit(dir: impl Into<PathBuf>, limit: u64) -> Self {
        Self {
            dir: dir.into(),
            limit,
            state: Mutex::new(State {
                checked: HashMap::new(),
            }),
            counting: Mutex::new(()),
            #[cfg(test)]
            hashed: AtomicU64::new(0),
        }
    }

    /// A shelf ([`bt_core::pencere::Shelf`]) over a store of this folder, to
    /// hand to every pane.
    pub fn shelf(dir: impl Into<PathBuf>) -> bt_core::pencere::Shelf {
        bt_core::pencere::Shelf::shared(Self::new(dir))
    }

    fn path(&self, digest: &Digest) -> PathBuf {
        self.dir.join(digest.to_string())
    }

    /// The bytes of the file named by `digest` if they have it; a file that
    /// does not is deleted. The file is read whole: at most 8 MiB, the
    /// protocol's limit for one file, and larger files are not stored. A file checked before
    /// is believed while its size is the size it had then; any other is hashed, once.
    fn read_checked(&self, digest: &Digest) -> Option<Vec<u8>> {
        let path = self.path(digest);
        let bytes = fs::read(&path).ok()?;
        let size = bytes.len() as u64;
        if lock(&self.state).checked.get(digest) == Some(&size) {
            return Some(bytes);
        }
        #[cfg(test)]
        self.hashed.fetch_add(1, Ordering::Relaxed);
        if Digest::of(&bytes) == *digest {
            lock(&self.state).checked.insert(*digest, size);
            return Some(bytes);
        }
        let _ = fs::remove_file(&path);
        lock(&self.state).checked.remove(digest);
        None
    }

    /// Marks the file as used now, for the order it is dropped in.
    fn touch(path: &Path) {
        if let Ok(file) = fs::OpenOptions::new().write(true).open(path) {
            let _ = file.set_modified(SystemTime::now());
        }
    }

    /// Whether the store holds the file with this digest, checking it the first time.
    pub fn contains(&self, digest: &Digest) -> bool {
        if lock(&self.state).checked.contains_key(digest) {
            // Checked before; still there?
            let there = self.path(digest).is_file();
            if !there {
                lock(&self.state).checked.remove(digest);
            }
            return there;
        }
        self.read_checked(digest).is_some()
    }

    /// Keeps a verified file. The write is outside every lock: a read of another file is not
    /// held up by up to 8 MiB going to the disk.
    pub fn insert(&self, file: Verified) {
        let (digest, bytes) = file.into_parts();
        let path = self.path(&digest);
        if path.is_file() {
            // Already here: this is a use, not a new file. What is there is not believed
            // for having the name: the first read checks it.
            Self::touch(&path);
            return;
        }
        if write_whole(&self.dir, &path, &bytes).is_err() {
            return;
        }
        // The bytes were verified and the rename is whole: what is here is what was checked.
        lock(&self.state).checked.insert(digest, bytes.len() as u64);
        // The folder is counted as it is when this store's turn comes, after every write that
        // finished before it: the last turn sees them all, so the folder is within the limit
        // when the stores are done. Nothing is carried from one count to the next to drift.
        let _turn = lock(&self.counting);
        let kept = sweep(&self.dir, self.limit, &digest);
        let mut state = lock(&self.state);
        for gone in kept.dropped {
            state.checked.remove(&gone);
        }
    }

    /// The file with this digest, checked the first time it is asked for and marked used.
    pub fn get(&self, digest: &Digest) -> Option<Arc<[u8]>> {
        let bytes = self.read_checked(digest)?;
        Self::touch(&self.path(digest));
        Some(bytes.into())
    }
}

impl Shared for DiskStore {
    fn contains(&self, digest: &Digest) -> bool {
        Self::contains(self, digest)
    }

    fn insert(&self, file: Verified) {
        Self::insert(self, file);
    }

    fn get(&self, digest: &Digest) -> Option<Arc<[u8]>> {
        Self::get(self, digest)
    }
}

/// The digest a file name spells, if it is one: 64 lowercase hex digits.
fn digest_of(name: &str) -> Option<Digest> {
    Digest::parse(name)
}

/// The bytes of the digest-named files in `dir`; 0 if it cannot be read.
#[cfg(test)]
fn folder_size(dir: &Path) -> u64 {
    let Ok(entries) = fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .filter(|entry| entry.file_name().to_str().and_then(digest_of).is_some())
        .filter_map(|entry| entry.metadata().ok())
        .filter(fs::Metadata::is_file)
        .map(|meta| meta.len())
        .sum()
}

/// Writes `bytes` to `path` whole: a temporary file in the folder, then one
/// `rename`. The folder is made if it is not there, readable by the user alone.
fn write_whole(dir: &Path, path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)?;
    let serial = TEMP_SERIAL.fetch_add(1, Ordering::Relaxed);
    let temp = dir.join(format!("{TEMP_PREFIX}{}-{serial}", std::process::id()));
    let written = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temp)
        .and_then(|mut file| file.write_all(bytes).and_then(|()| file.flush()))
        .and_then(|()| fs::rename(&temp, path));
    if written.is_err() {
        let _ = fs::remove_file(&temp);
    }
    written
}

/// What a sweep left.
struct Swept {
    /// The files that were dropped.
    dropped: Vec<Digest>,
}

/// Drops the least recently used files of `dir` until it holds at most `limit`
/// bytes, never `keep`.
fn sweep(dir: &Path, limit: u64, keep: &Digest) -> Swept {
    let mut files: Vec<(SystemTime, u64, Digest)> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let digest = entry.file_name().to_str().and_then(digest_of)?;
            let meta = entry.metadata().ok().filter(fs::Metadata::is_file)?;
            Some((
                meta.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                meta.len(),
                digest,
            ))
        })
        .collect();
    let mut total: u64 = files.iter().map(|(_, size, _)| size).sum();
    files.sort_by_key(|(used, _, _)| *used);
    let mut dropped = Vec::new();
    for (_, size, digest) in files {
        if total <= limit {
            break;
        }
        if digest == *keep {
            continue;
        }
        if fs::remove_file(dir.join(digest.to_string())).is_ok() {
            total = total.saturating_sub(size);
            dropped.push(digest);
        }
    }
    Swept { dropped }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    /// A folder of this test's own, removed when it goes.
    struct Folder(PathBuf);

    impl Folder {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir()
                .join(format!("bateri-view-store-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            Self(dir)
        }
    }

    impl Drop for Folder {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn file(fill: u8, size: usize) -> Verified {
        Verified::from_bytes(vec![fill; size])
    }

    /// Marks a file as last used `seconds` after the epoch: the sweep's order,
    /// without waiting for the clock to tick.
    fn used_at(dir: &Path, verified: &Verified, seconds: u64) {
        let path = dir.join(verified.digest().to_string());
        let file = fs::OpenOptions::new().write(true).open(path).unwrap();
        file.set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(seconds))
            .unwrap();
    }

    #[test]
    fn a_file_stored_is_given_back_and_contained() {
        let folder = Folder::new("round-trip");
        let store = DiskStore::new(&folder.0);
        let held = file(7, 1000);
        let digest = *held.digest();
        assert!(!store.contains(&digest), "nothing there yet");
        store.insert(held);
        assert!(store.contains(&digest));
        assert_eq!(store.get(&digest).as_deref(), Some(&[7u8; 1000][..]));
        assert!(!store.contains(&Digest::of(b"another")));
        assert!(store.get(&Digest::of(b"another")).is_none());
    }

    #[test]
    fn what_was_stored_is_there_for_the_next_process() {
        let folder = Folder::new("next-process");
        let held = file(3, 64);
        let digest = *held.digest();
        DiskStore::new(&folder.0).insert(held);
        // A store that never saw the file stored finds it, and checks it.
        let next = DiskStore::new(&folder.0);
        assert!(next.contains(&digest));
        assert_eq!(next.get(&digest).as_deref(), Some(&[3u8; 64][..]));
    }

    #[test]
    fn a_file_with_other_bytes_than_its_name_says_is_never_given() {
        let folder = Folder::new("damaged");
        let store = DiskStore::new(&folder.0);
        let held = file(1, 100);
        let digest = *held.digest();
        store.insert(held);
        let path = folder.0.join(digest.to_string());
        // Another writer, or the disk: the name is the same, the bytes are not.
        fs::write(&path, b"not what the name says").unwrap();
        assert!(store.get(&digest).is_none(), "checked at every read");
        assert!(!path.exists(), "and a file that fails is deleted");

        // The first question about a file in a process checks it too.
        fs::write(&path, b"not what the name says").unwrap();
        let fresh = DiskStore::new(&folder.0);
        assert!(!fresh.contains(&digest));
        assert!(!path.exists());
    }

    #[test]
    fn a_file_is_hashed_the_first_time_it_is_asked_about_and_not_at_every_read() {
        let folder = Folder::new("hashed");
        let held = file(4, 4096);
        let digest = *held.digest();
        DiskStore::new(&folder.0).insert(held);
        // A store of the next process: nothing is known of the file yet.
        let store = DiskStore::new(&folder.0);
        let hashed = || store.hashed.load(Ordering::Relaxed);
        assert!(store.contains(&digest));
        assert_eq!(hashed(), 1, "checked when first asked about");
        // The web view asks at every load: no more hashing of up to 8 MiB each time.
        for _ in 0..3 {
            assert_eq!(store.get(&digest).as_deref(), Some(&[4u8; 4096][..]));
            assert!(store.contains(&digest));
        }
        assert_eq!(
            hashed(),
            1,
            "believed after, as long as its size is its size"
        );
        // What this process stored is verified bytes and is not hashed for its own sake.
        let kept = file(5, 100);
        let kept_digest = *kept.digest();
        store.insert(kept);
        assert_eq!(store.get(&kept_digest).as_deref(), Some(&[5u8; 100][..]));
        assert_eq!(hashed(), 1);
        // A file whose size is not what it was is checked again, and fails.
        fs::write(folder.0.join(kept_digest.to_string()), b"cut").unwrap();
        assert!(store.get(&kept_digest).is_none());
        assert_eq!(hashed(), 2);
    }

    #[test]
    fn a_file_found_in_the_folder_when_one_is_stored_is_not_believed_for_its_name() {
        let folder = Folder::new("found");
        let held = file(6, 50);
        let digest = *held.digest();
        fs::create_dir_all(&folder.0).unwrap();
        // Another writer put something under the name before this process stored the file.
        fs::write(folder.0.join(digest.to_string()), vec![0u8; 50]).unwrap();
        let store = DiskStore::new(&folder.0);
        store.insert(held);
        assert!(
            store.get(&digest).is_none(),
            "checked at the first read, and it fails"
        );
    }

    #[test]
    fn stores_at_once_count_every_file_and_keep_to_the_limit() {
        let folder = Folder::new("at-once");
        let store = Arc::new(DiskStore::with_limit(&folder.0, 250));
        let threads: Vec<_> = (0..8u8)
            .map(|fill| {
                let store = Arc::clone(&store);
                std::thread::spawn(move || store.insert(file(fill + 1, 60)))
            })
            .collect();
        for thread in threads {
            thread.join().unwrap();
        }
        // The last turn counted every file that was written before it: within the limit, and
        // whatever was stored last is there.
        let left = folder_size(&folder.0);
        assert!(left <= 250, "the folder is within the limit: {left}");
        assert!(left >= 60, "and what was stored last is there");
    }

    #[test]
    fn a_file_that_has_gone_is_not_contained() {
        let folder = Folder::new("gone");
        let store = DiskStore::new(&folder.0);
        let held = file(2, 10);
        let digest = *held.digest();
        store.insert(held);
        assert!(store.contains(&digest));
        fs::remove_file(folder.0.join(digest.to_string())).unwrap();
        assert!(
            !store.contains(&digest),
            "checked before is not kept forever"
        );
        assert!(store.get(&digest).is_none());
    }

    #[test]
    fn past_the_limit_the_oldest_goes_and_never_the_one_just_stored() {
        let folder = Folder::new("limit");
        let store = DiskStore::with_limit(&folder.0, 100);
        let (a, b, c) = (file(1, 60), file(2, 60), file(3, 60));
        let digests = [*a.digest(), *b.digest(), *c.digest()];
        store.insert(a.clone());
        used_at(&folder.0, &a, 10);
        store.insert(b.clone());
        // 120 bytes: a is older and goes; b, which was just stored, stays.
        assert!(!store.contains(&digests[0]));
        assert!(store.contains(&digests[1]));
        used_at(&folder.0, &b, 20);
        store.insert(c);
        assert!(!store.contains(&digests[1]));
        assert!(store.contains(&digests[2]));
    }

    #[test]
    fn what_was_read_last_stays_longest() {
        let folder = Folder::new("used");
        let store = DiskStore::with_limit(&folder.0, 130);
        let (a, b, c) = (file(1, 60), file(2, 60), file(3, 60));
        let digests = [*a.digest(), *b.digest(), *c.digest()];
        store.insert(a.clone());
        store.insert(b.clone());
        used_at(&folder.0, &a, 10);
        used_at(&folder.0, &b, 20);
        // Reading a makes it the newest: b is now the oldest.
        assert!(store.get(&digests[0]).is_some());
        store.insert(c);
        assert!(store.contains(&digests[0]));
        assert!(!store.contains(&digests[1]));
        assert!(store.contains(&digests[2]));
    }

    #[test]
    fn the_folder_is_the_users_alone_and_leaves_no_temporary_file() {
        use std::os::unix::fs::PermissionsExt;

        let folder = Folder::new("private");
        let store = DiskStore::new(folder.0.join("nested").join("pencere"));
        let held = file(9, 8);
        let digest = held.digest().to_string();
        store.insert(held);
        let dir = folder.0.join("nested").join("pencere");
        let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&dir), 0o700);
        assert_eq!(mode(&dir.join(&digest)), 0o600);
        let names: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().into_string().unwrap())
            .collect();
        assert_eq!(names, [digest], "only the file, no temporary one");
    }

    #[test]
    fn a_store_with_no_folder_is_empty_and_keeps_nothing_it_cannot() {
        let folder = Folder::new("absent");
        let store = DiskStore::new(folder.0.join("never-made"));
        let held = file(5, 5);
        assert!(!store.contains(held.digest()));
        // A folder that cannot be made (its parent is a file) loses the file
        // and says nothing: the program sends it again at the next open.
        fs::create_dir_all(&folder.0).unwrap();
        let blocker = folder.0.join("blocker");
        fs::write(&blocker, b"x").unwrap();
        let blocked = DiskStore::new(blocker.join("inside"));
        let digest = *held.digest();
        blocked.insert(held.clone());
        assert!(!blocked.contains(&digest));
        store.insert(held);
        assert!(store.contains(&digest));
    }
}
