//! The files beside a native view's bundle, on the terminal's side (specification §8.8 to §8.10):
//! the handles the terminal made of the files the person gave the view and of the places the
//! person chose to save in, and the one thread that reads and writes them.
//!
//! The session rules and does no I/O (`bt-core` has none): it keeps the handles, counts, and asks
//! the terminal to read (`Action::ReadFile`) and write (`Action::WriteFile`, `Action::AbortWrite`)
//! and to be told what came of it. This is where that is done, in the pane's own words: a pane has
//! a [`ViewFiles`] while it shows a view, and drops it with the view. **The disk is never waited
//! for on a thread that draws or reads the program's output**: a job goes to the pane's worker
//! thread, which is started by the first one, and its result is told to the session from there
//! ([`Outcomes`]).
//!
//! **A file is read where the person left it and written where the person chose, and nowhere
//! else.** A handle names a row of the table; the table is filled by the pane, from what the
//! person did (the pane's drop, paste and panels), and never from what the program says. A read
//! finds the file as it was when it was given — its length and the time it changed — or says
//! `gone`. A save goes to a part beside its place, `.<name>.pencere-part`, made new (a part left
//! by a process that was killed is replaced, and a link put there is not written through), and
//! is given the place's name when the last of it is in; a save that ends any other way — the
//! program gives it up, fails, ends, the view ends, the pane is closed — leaves no part behind.
//!
//! The platform's own mark on a file saved (macOS' quarantine) is put by a hook the platform's
//! shell gives ([`Quarantine`]): this crate names no platform library.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::os::unix::fs::{FileExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};
use std::time::SystemTime;

use bt_core::Session;
use bt_core::pencere::FileError;

/// The name a file the person gave has for the view that draws it: `file:` and its handle.
pub const FILE_SCHEME: &str = "file:";

/// The most bytes of a file a page draws as a picture: the most the images of a view hold for a
/// file ([`pencere_sandbox::ImageLimits::file_bytes`]). A larger one is not read.
const PICTURE_LIMIT: u64 = 8 << 20;

/// The mark the platform puts on a file the program saved, once it has its place's name.
pub type Quarantine = Arc<dyn Fn(&Path) + Send + Sync>;

/// Where the worker tells what came of a job: the session, in the pane.
pub trait Outcomes: Send + Sync + 'static {
    /// What came of the read whose id this is.
    fn file_read(&self, read: u32, result: Result<Vec<u8>, FileError>);
    /// What came of the write whose id this is.
    fn file_written(&self, write: u32, result: Result<(), FileError>);
}

/// A session that is gone is told nothing: the view its files were for is gone with it.
impl Outcomes for Weak<Session> {
    fn file_read(&self, read: u32, result: Result<Vec<u8>, FileError>) {
        if let Some(session) = self.upgrade() {
            session.file_read(read, result);
        }
    }

    fn file_written(&self, write: u32, result: Result<(), FileError>) {
        if let Some(session) = self.upgrade() {
            session.file_written(write, result);
        }
    }
}

/// A file as it was when the person gave it. A read says `gone` when its length or the time it
/// changed is another.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GivenFile {
    pub path: PathBuf,
    pub size: u64,
    pub modified: Option<SystemTime>,
}

impl GivenFile {
    /// The file at `path` as it is now: a regular file, which a link is followed to (the person
    /// gave what the link shows them).
    ///
    /// # Errors
    ///
    /// The file cannot be looked at, or is not a regular file.
    pub fn of(path: &Path) -> io::Result<Self> {
        let meta = fs::metadata(path)?;
        if !meta.is_file() {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "not a file"));
        }
        Ok(Self {
            path: path.to_owned(),
            size: meta.len(),
            modified: meta.modified().ok(),
        })
    }
}

/// What a handle names.
#[derive(Clone)]
enum Row {
    /// A file the person gave.
    Given(GivenFile),
    /// A place the person chose to save in.
    Save(PathBuf),
}

enum Job {
    Read {
        read: u32,
        handle: String,
        offset: u64,
        length: u64,
    },
    Write {
        write: u32,
        handle: String,
        offset: u64,
        bytes: Vec<u8>,
        end: bool,
    },
    Abort {
        handle: String,
    },
}

/// The files of the view a pane shows: its handles and the thread that reads and writes them.
pub struct ViewFiles {
    table: Arc<Mutex<HashMap<String, Row>>>,
    /// The worker's queue, made by the first job.
    jobs: Mutex<Option<Sender<Job>>>,
    outcomes: Arc<dyn Outcomes>,
    quarantine: Quarantine,
}

impl ViewFiles {
    /// Files with nothing given yet. `outcomes` is told what comes of the jobs and `quarantine`
    /// marks what is saved.
    #[must_use]
    pub fn new(outcomes: Arc<dyn Outcomes>, quarantine: Quarantine) -> Self {
        Self {
            table: Arc::default(),
            jobs: Mutex::new(None),
            outcomes,
            quarantine,
        }
    }

    /// `handle` names a file the person gave, as it is (see [`GivenFile::of`]). To be called before
    /// the handle is told to the page.
    pub fn keep_given(&self, handle: &str, source: GivenFile) {
        lock(&self.table).insert(handle.to_owned(), Row::Given(source));
    }

    /// `handle` names the place `target` the person chose in the Save panel. To be called before
    /// the handle is told to the page.
    pub fn keep_save(&self, handle: &str, target: PathBuf) {
        lock(&self.table).insert(handle.to_owned(), Row::Save(target));
    }

    /// The file `handle` names, if it names a file the person gave.
    #[must_use]
    pub fn given(&self, handle: &str) -> Option<GivenFile> {
        match lock(&self.table).get(handle)? {
            Row::Given(source) => Some(source.clone()),
            Row::Save(_) => None,
        }
    }

    /// The bytes of the file `handle` names, for a page to draw as a picture (the name
    /// `file:` and the handle, [`FILE_SCHEME`]); `None` for a handle that names no file, a file
    /// that is not what it was, or one too large to be a picture. Reads on the thread it is
    /// called on, which is the images' worker.
    #[must_use]
    pub fn picture(&self, handle: &str) -> Option<Arc<[u8]>> {
        let source = self.given(handle)?;
        if source.size > PICTURE_LIMIT {
            return None;
        }
        read_range(&source, 0, source.size).ok().map(Arc::from)
    }

    /// Reads up to `length` bytes of the file `handle` names from `offset`; the result is told to
    /// the session ([`Outcomes::file_read`]).
    pub fn read(&self, read: u32, handle: &str, offset: u64, length: u64) {
        self.send(Job::Read {
            read,
            handle: handle.to_owned(),
            offset,
            length,
        });
    }

    /// Writes `bytes` at `offset` of the save `handle` names; with `end` they are the last, and
    /// the file is given its place's name. The result is told to the session
    /// ([`Outcomes::file_written`]).
    pub fn write(&self, write: u32, handle: &str, offset: u64, bytes: Vec<u8>, end: bool) {
        self.send(Job::Write {
            write,
            handle: handle.to_owned(),
            offset,
            bytes,
            end,
        });
    }

    /// The save `handle` names is given up: what was written for it is deleted.
    pub fn abort(&self, handle: &str) {
        self.send(Job::Abort {
            handle: handle.to_owned(),
        });
    }

    fn send(&self, job: Job) {
        let mut jobs = lock(&self.jobs);
        if jobs.is_none() {
            let (sender, receiver) = mpsc::channel();
            let work = Worker {
                table: Arc::clone(&self.table),
                outcomes: Arc::clone(&self.outcomes),
                quarantine: Arc::clone(&self.quarantine),
                parts: HashMap::new(),
            };
            if std::thread::Builder::new()
                .name("view-files".into())
                .spawn(move || work.run(&receiver))
                .is_err()
            {
                // No thread to work on: the job is answered as a disk that failed would.
                drop(jobs);
                self.fail(job);
                return;
            }
            *jobs = Some(sender);
        }
        if let Some(Err(mpsc::SendError(job))) = jobs.as_ref().map(|sender| sender.send(job)) {
            drop(jobs);
            self.fail(job);
        }
    }

    fn fail(&self, job: Job) {
        match job {
            Job::Read { read, .. } => self.outcomes.file_read(read, Err(FileError::Io)),
            Job::Write { write, .. } => self.outcomes.file_written(write, Err(FileError::Io)),
            Job::Abort { .. } => {}
        }
    }
}

/// The thread that does the jobs, in the order they came. It ends when the pane's files are
/// dropped, after the jobs that are in its queue, and deletes the parts that were not finished.
struct Worker {
    table: Arc<Mutex<HashMap<String, Row>>>,
    outcomes: Arc<dyn Outcomes>,
    quarantine: Quarantine,
    /// The saves under way, by handle.
    parts: HashMap<String, Part>,
}

impl Worker {
    fn run(mut self, jobs: &Receiver<Job>) {
        for job in jobs {
            match job {
                Job::Read {
                    read,
                    handle,
                    offset,
                    length,
                } => {
                    let source = match lock(&self.table).get(&handle) {
                        Some(Row::Given(source)) => Some(source.clone()),
                        _ => None,
                    };
                    let result = source.map_or(Err(FileError::Gone), |source| {
                        read_piece(&source, offset, length)
                    });
                    self.outcomes.file_read(read, result);
                }
                Job::Write {
                    write,
                    handle,
                    offset,
                    bytes,
                    end,
                } => {
                    let result = self.write(&handle, offset, &bytes, end);
                    if result.is_err() {
                        self.parts.remove(&handle);
                    }
                    self.outcomes.file_written(write, result);
                }
                Job::Abort { handle } => {
                    self.parts.remove(&handle);
                }
            }
        }
    }

    fn write(
        &mut self,
        handle: &str,
        offset: u64,
        bytes: &[u8],
        end: bool,
    ) -> Result<(), FileError> {
        let target = match lock(&self.table).get(handle) {
            Some(Row::Save(target)) => target.clone(),
            _ => return Err(FileError::Io),
        };
        if offset == 0 && !self.parts.contains_key(handle) {
            self.parts.insert(
                handle.to_owned(),
                Part::create(&target).map_err(write_error)?,
            );
        }
        let part = self.parts.get_mut(handle).ok_or(FileError::Io)?;
        // The session writes in order and with no gap; a part that is not where the write is
        // would be a file the program did not write.
        if part.length != offset {
            return Err(FileError::Io);
        }
        part.file.write_all_at(bytes, offset).map_err(write_error)?;
        part.length = part.length.saturating_add(bytes.len() as u64);
        if end && let Some(part) = self.parts.remove(handle) {
            part.keep(&target).map_err(write_error)?;
            (self.quarantine)(&target);
        }
        Ok(())
    }
}

/// A save under way: the part, beside the place, which is deleted if it is dropped unkept.
struct Part {
    path: PathBuf,
    file: fs::File,
    length: u64,
}

impl Part {
    fn create(target: &Path) -> io::Result<Self> {
        let name = target
            .file_name()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "a place with no name"))?;
        let mut part_name = std::ffi::OsString::from(".");
        part_name.push(name);
        part_name.push(".pencere-part");
        let path = target.with_file_name(part_name);
        // A part a process that was killed left is ours to replace. What is there now is not
        // followed if it is a link, and one put there meanwhile fails the save rather than be
        // written through.
        let _ = fs::remove_file(&path);
        let file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&path)?;
        Ok(Self {
            path,
            file,
            length: 0,
        })
    }

    /// Gives the part the name of its place.
    fn keep(self, target: &Path) -> io::Result<()> {
        self.file.sync_all()?;
        fs::rename(&self.path, target)?;
        // The name is the place's now: there is nothing to delete.
        std::mem::forget(self);
        Ok(())
    }
}

impl Drop for Part {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

/// What a failure of writing is told to the program as: a disk with no room, or any other.
fn write_error(error: io::Error) -> FileError {
    match error.raw_os_error() {
        Some(libc::ENOSPC | libc::EDQUOT) => FileError::Space,
        _ => FileError::Io,
    }
}

/// Up to `length` bytes of `source` from `offset` (fewer where the file ends), if the file is as
/// it was given.
fn read_piece(source: &GivenFile, offset: u64, length: u64) -> Result<Vec<u8>, FileError> {
    read_range(source, offset, length)
}

fn read_range(source: &GivenFile, offset: u64, length: u64) -> Result<Vec<u8>, FileError> {
    let file = fs::File::open(&source.path).map_err(read_error)?;
    // The file that is open is the one looked at: what is checked is what is read.
    let meta = file.metadata().map_err(read_error)?;
    if !meta.is_file() || meta.len() != source.size || meta.modified().ok() != source.modified {
        return Err(FileError::Gone);
    }
    if offset > source.size {
        return Err(FileError::Range);
    }
    let wanted = length.min(source.size - offset);
    let mut bytes = vec![0; usize::try_from(wanted).map_err(|_| FileError::Range)?];
    let mut filled = 0;
    while filled < bytes.len() {
        match file.read_at(&mut bytes[filled..], offset + filled as u64) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(read_error(error)),
        }
    }
    // A file that ends sooner than it said has changed.
    if filled < bytes.len() {
        return Err(FileError::Gone);
    }
    Ok(bytes)
}

/// A file that is not there or may not be read is gone from the program's point of view.
fn read_error(error: io::Error) -> FileError {
    match error.kind() {
        io::ErrorKind::NotFound | io::ErrorKind::PermissionDenied => FileError::Gone,
        _ => FileError::Io,
    }
}

/// The lock, whether or not a thread that held it panicked: the tables are changed a row at a
/// time and never found half-changed.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::RecvTimeoutError;
    use std::time::Duration;

    /// What the worker told, in order.
    #[derive(Debug, PartialEq)]
    enum Told {
        Read(u32, Result<Vec<u8>, FileError>),
        Written(u32, Result<(), FileError>),
    }

    struct Tell(Mutex<Sender<Told>>);

    impl Outcomes for Tell {
        fn file_read(&self, read: u32, result: Result<Vec<u8>, FileError>) {
            let _ = lock(&self.0).send(Told::Read(read, result));
        }
        fn file_written(&self, write: u32, result: Result<(), FileError>) {
            let _ = lock(&self.0).send(Told::Written(write, result));
        }
    }

    /// Files whose outcomes come back on the receiver, and whose quarantine counts the paths it
    /// is asked to mark.
    fn files() -> (ViewFiles, Receiver<Told>, Arc<Mutex<Vec<PathBuf>>>) {
        let (sender, receiver) = mpsc::channel();
        let marked = Arc::new(Mutex::new(Vec::new()));
        let mark = Arc::clone(&marked);
        let files = ViewFiles::new(
            Arc::new(Tell(Mutex::new(sender))),
            Arc::new(move |path: &Path| lock(&mark).push(path.to_owned())),
        );
        (files, receiver, marked)
    }

    fn told(receiver: &Receiver<Told>) -> Told {
        receiver
            .recv_timeout(Duration::from_secs(10))
            .expect("the worker told what came of the job")
    }

    /// A directory of the test's own, deleted at its end.
    struct Dir(PathBuf);

    impl Dir {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "bateri-view-files-{name}-{}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).expect("a directory");
            Self(path)
        }
        fn join(&self, name: &str) -> PathBuf {
            self.0.join(name)
        }
    }

    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn given(files: &ViewFiles, dir: &Dir, name: &str, bytes: &[u8]) -> String {
        let path = dir.join(name);
        fs::write(&path, bytes).expect("a file");
        let handle = format!("{:032x}", name.len());
        files.keep_given(&handle, GivenFile::of(&path).expect("a source"));
        handle
    }

    #[test]
    fn a_file_is_read_a_piece_at_a_time_and_the_end_is_a_short_piece() {
        let dir = Dir::new("pieces");
        let (files, receiver, _) = files();
        let handle = given(&files, &dir, "a.txt", b"0123456789");
        files.read(1, &handle, 0, 4);
        files.read(2, &handle, 4, 4);
        files.read(3, &handle, 8, 4);
        files.read(4, &handle, 10, 4);
        assert_eq!(told(&receiver), Told::Read(1, Ok(b"0123".to_vec())));
        assert_eq!(told(&receiver), Told::Read(2, Ok(b"4567".to_vec())));
        assert_eq!(told(&receiver), Told::Read(3, Ok(b"89".to_vec())));
        assert_eq!(told(&receiver), Told::Read(4, Ok(Vec::new())));
    }

    #[test]
    fn a_file_that_changed_or_went_since_it_was_given_is_gone() {
        let dir = Dir::new("gone");
        let (files, receiver, _) = files();
        let changed = given(&files, &dir, "changed.txt", b"abcdef");
        let moved = given(&files, &dir, "moved.txt!", b"abcdef");
        // Another length.
        fs::write(dir.join("changed.txt"), b"abcdefg").unwrap();
        // Gone altogether.
        fs::remove_file(dir.join("moved.txt!")).unwrap();
        files.read(1, &changed, 0, 4);
        files.read(2, &moved, 0, 4);
        // A handle the table does not have.
        files.read(3, &"0".repeat(32), 0, 4);
        assert_eq!(told(&receiver), Told::Read(1, Err(FileError::Gone)));
        assert_eq!(told(&receiver), Told::Read(2, Err(FileError::Gone)));
        assert_eq!(told(&receiver), Told::Read(3, Err(FileError::Gone)));
    }

    #[test]
    fn a_file_of_the_same_length_that_was_written_again_is_gone() {
        let dir = Dir::new("mtime");
        let (files, receiver, _) = files();
        let handle = given(&files, &dir, "same.txt", b"abcdef");
        let path = dir.join("same.txt");
        fs::write(&path, b"ABCDEF").unwrap();
        let later = SystemTime::now() + Duration::from_secs(60);
        fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(later)
            .unwrap();
        files.read(1, &handle, 0, 6);
        assert_eq!(told(&receiver), Told::Read(1, Err(FileError::Gone)));
    }

    #[test]
    fn a_picture_is_the_whole_file_if_it_is_as_it_was_given_and_nothing_if_it_is_not() {
        let dir = Dir::new("picture");
        let (files, _receiver, _) = files();
        let handle = given(&files, &dir, "p.png", b"not really a png");
        assert_eq!(
            files.picture(&handle).as_deref(),
            Some(&b"not really a png"[..])
        );
        assert_eq!(files.picture(&"1".repeat(32)), None);
        fs::write(dir.join("p.png"), b"changed").unwrap();
        assert_eq!(files.picture(&handle), None);
        // A place to save in is not a picture.
        files.keep_save(&"2".repeat(32), dir.join("out.txt"));
        assert_eq!(files.picture(&"2".repeat(32)), None);
    }

    fn save(files: &ViewFiles, dir: &Dir, name: &str) -> String {
        let handle = format!("{:032x}", 0x5a5a_0000 + name.len());
        files.keep_save(&handle, dir.join(name));
        handle
    }

    fn part_of(dir: &Dir, name: &str) -> PathBuf {
        dir.join(&format!(".{name}.pencere-part"))
    }

    #[test]
    fn a_save_is_written_to_a_part_and_given_its_places_name_when_the_last_of_it_is_in() {
        let dir = Dir::new("save");
        let (files, receiver, marked) = files();
        let handle = save(&files, &dir, "out.txt");
        files.write(1, &handle, 0, b"hello, ".to_vec(), false);
        assert_eq!(told(&receiver), Told::Written(1, Ok(())));
        assert_eq!(fs::read(part_of(&dir, "out.txt")).unwrap(), b"hello, ");
        assert!(!dir.join("out.txt").exists(), "the place has no file yet");
        files.write(2, &handle, 7, b"world".to_vec(), true);
        assert_eq!(told(&receiver), Told::Written(2, Ok(())));
        assert_eq!(fs::read(dir.join("out.txt")).unwrap(), b"hello, world");
        assert!(
            !part_of(&dir, "out.txt").exists(),
            "the part is the file now"
        );
        assert_eq!(*lock(&marked), vec![dir.join("out.txt")]);
    }

    #[test]
    fn a_save_replaces_the_file_at_its_place_only_when_it_is_finished() {
        let dir = Dir::new("replace");
        let (files, receiver, _) = files();
        fs::write(dir.join("out.txt"), b"the old one").unwrap();
        let handle = save(&files, &dir, "out.txt");
        files.write(1, &handle, 0, b"new".to_vec(), false);
        assert_eq!(told(&receiver), Told::Written(1, Ok(())));
        assert_eq!(fs::read(dir.join("out.txt")).unwrap(), b"the old one");
        files.write(2, &handle, 3, Vec::new(), true);
        assert_eq!(told(&receiver), Told::Written(2, Ok(())));
        assert_eq!(fs::read(dir.join("out.txt")).unwrap(), b"new");
    }

    #[test]
    fn a_file_of_no_bytes_is_saved() {
        let dir = Dir::new("empty");
        let (files, receiver, _) = files();
        let handle = save(&files, &dir, "empty.txt");
        files.write(1, &handle, 0, Vec::new(), true);
        assert_eq!(told(&receiver), Told::Written(1, Ok(())));
        assert_eq!(fs::read(dir.join("empty.txt")).unwrap(), b"");
    }

    #[test]
    fn a_save_that_is_given_up_leaves_no_part_and_no_file() {
        let dir = Dir::new("abort");
        let (files, receiver, _) = files();
        let handle = save(&files, &dir, "out.txt");
        files.write(1, &handle, 0, b"half".to_vec(), false);
        assert_eq!(told(&receiver), Told::Written(1, Ok(())));
        assert!(part_of(&dir, "out.txt").exists());
        files.abort(&handle);
        // The queue is in order: a job after the abort is done after it.
        files.read(2, &"0".repeat(32), 0, 1);
        assert_eq!(told(&receiver), Told::Read(2, Err(FileError::Gone)));
        assert!(!part_of(&dir, "out.txt").exists());
        assert!(!dir.join("out.txt").exists());
    }

    #[test]
    fn a_save_the_view_ends_in_the_middle_of_leaves_no_part() {
        let dir = Dir::new("ends");
        let (files, receiver, _) = files();
        let handle = save(&files, &dir, "out.txt");
        files.write(1, &handle, 0, b"half".to_vec(), false);
        assert_eq!(told(&receiver), Told::Written(1, Ok(())));
        assert!(part_of(&dir, "out.txt").exists());
        // The pane drops the files with the view; the worker finishes its queue and goes.
        drop(files);
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while part_of(&dir, "out.txt").exists() {
            assert!(std::time::Instant::now() < deadline, "the part was left");
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(!dir.join("out.txt").exists());
        assert_eq!(
            receiver.recv_timeout(Duration::from_millis(50)),
            Err(RecvTimeoutError::Disconnected)
        );
    }

    #[test]
    fn a_link_put_where_the_part_goes_is_not_written_through() {
        let dir = Dir::new("link");
        let (files, receiver, _) = files();
        let victim = dir.join("victim.txt");
        fs::write(&victim, b"mine").unwrap();
        let handle = save(&files, &dir, "out.txt");
        // A part a killed process left, as a link: replaced, not followed.
        std::os::unix::fs::symlink(&victim, part_of(&dir, "out.txt")).unwrap();
        files.write(1, &handle, 0, b"theirs".to_vec(), true);
        assert_eq!(told(&receiver), Told::Written(1, Ok(())));
        assert_eq!(fs::read(&victim).unwrap(), b"mine");
        assert_eq!(fs::read(dir.join("out.txt")).unwrap(), b"theirs");
    }

    #[test]
    fn a_place_whose_folder_is_gone_fails_the_save_and_leaves_nothing() {
        let dir = Dir::new("nofolder");
        let (files, receiver, _) = files();
        let handle = "7".repeat(32);
        files.keep_save(&handle, dir.join("missing").join("out.txt"));
        files.write(1, &handle, 0, b"x".to_vec(), true);
        assert_eq!(told(&receiver), Told::Written(1, Err(FileError::Io)));
        // A write for a handle that names no place is a failure too.
        files.write(2, &"8".repeat(32), 0, b"x".to_vec(), true);
        assert_eq!(told(&receiver), Told::Written(2, Err(FileError::Io)));
    }

    #[test]
    fn a_write_that_is_not_where_the_file_ends_fails_the_save_and_deletes_the_part() {
        let dir = Dir::new("gap");
        let (files, receiver, _) = files();
        let handle = save(&files, &dir, "out.txt");
        files.write(1, &handle, 0, b"ab".to_vec(), false);
        assert_eq!(told(&receiver), Told::Written(1, Ok(())));
        files.write(2, &handle, 9, b"cd".to_vec(), true);
        assert_eq!(told(&receiver), Told::Written(2, Err(FileError::Io)));
        assert!(!part_of(&dir, "out.txt").exists());
        assert!(!dir.join("out.txt").exists());
    }

    #[test]
    fn a_save_over_a_folder_fails_and_leaves_no_part() {
        let dir = Dir::new("folder");
        let (files, receiver, _) = files();
        fs::create_dir(dir.join("out.txt")).unwrap();
        let handle = save(&files, &dir, "out.txt");
        files.write(1, &handle, 0, b"x".to_vec(), true);
        assert_eq!(told(&receiver), Told::Written(1, Err(FileError::Io)));
        assert!(!part_of(&dir, "out.txt").exists());
    }

    #[test]
    fn a_source_is_a_regular_file() {
        let dir = Dir::new("source");
        assert!(GivenFile::of(&dir.0).is_err(), "a folder is no file");
        assert!(GivenFile::of(&dir.join("nothing")).is_err());
        fs::write(dir.join("f"), b"abc").unwrap();
        let source = GivenFile::of(&dir.join("f")).unwrap();
        assert_eq!(source.size, 3);
    }
}
