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
//! `gone`. A save goes to a part beside its place, `.<name>.<handle>.pencere-part`, made new (a
//! part left by a process that was killed is replaced, and a link put there is not written
//! through), and is given the place's name when the last of it is in; a save that ends any other
//! way — the program gives it up, fails, ends, the view ends, the pane is closed — leaves no part
//! behind.
//!
//! The platform's own mark on a file saved (macOS' quarantine) is put by a hook the platform's
//! shell gives ([`Quarantine`]): this crate names no platform library.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::os::unix::fs::{DirBuilderExt, FileExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};
use std::time::SystemTime;

use bt_core::Session;
use bt_core::pencere::{FetchError, FileError, FileGrant, StageError};
use pencere_sandbox::ImageLimits;
use pencere_view::{FileEntry, Saving};

use super::transfers::{Ground, Transfers, Waits};
use crate::stage::Way;

/// The name a file the person gave has for the view that draws it: `file:` and its handle.
pub const FILE_SCHEME: &str = "file:";

/// The most bytes of a file a page draws as a picture: the most the images of a view hold for a
/// file ([`ImageLimits::file_bytes`]). A larger one is not read.
fn picture_limit() -> u64 {
    u64::try_from(ImageLimits::default().file_bytes).unwrap_or(u64::MAX)
}

/// The mark the platform puts on a file the program saved, once it has its place's name.
pub type Quarantine = Arc<dyn Fn(&Path) + Send + Sync>;

/// Where the worker tells what came of a job: the session, in the pane.
pub trait Outcomes: Send + Sync + 'static {
    /// What came of the read whose id this is.
    fn file_read(&self, read: u32, result: Result<Vec<u8>, FileError>);
    /// What came of the write whose id this is.
    fn file_written(&self, write: u32, result: Result<(), FileError>);
    /// The pipe of the transfer whose id this is exists: the program may open it.
    fn pipe_ready(&self, transfer: u32);
    /// How the `stage` whose id this is ended: where in the file the end was, or why not.
    fn staged(&self, transfer: u32, result: Result<u64, StageError>);
    /// How the `fetch` whose id this is ended: how long the saved file is, or why not.
    fn fetched(&self, transfer: u32, result: Result<u64, FetchError>);
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

    fn pipe_ready(&self, transfer: u32) {
        if let Some(session) = self.upgrade() {
            session.pipe_ready(transfer);
        }
    }

    fn staged(&self, transfer: u32, result: Result<u64, StageError>) {
        if let Some(session) = self.upgrade() {
            session.staged(transfer, result);
        }
    }

    fn fetched(&self, transfer: u32, result: Result<u64, FetchError>) {
        if let Some(session) = self.upgrade() {
            session.fetched(transfer, result);
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
        Self::from_meta(path, &fs::metadata(path)?)
    }

    /// The file at `path` as `meta` (what `fs::metadata` said of it) shows it.
    fn from_meta(path: &Path, meta: &fs::Metadata) -> io::Result<Self> {
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
pub(super) enum Row {
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
    /// The folder of files the pane made itself, made by the first (see [`Self::scratch`]).
    scratch: Mutex<Option<PathBuf>>,
    /// How many files were made there, to give each a folder of its own.
    made: AtomicU64,
    /// The saves under way, by handle: the worker's writes and a transfer's are one table.
    parts: Arc<Mutex<HashMap<String, Part>>>,
    /// The way to the folder the program was offered, if it was offered one (see
    /// [`Self::with_way`]).
    way: Option<Way>,
    /// How long the transfers wait on a pipe.
    waits: Waits,
    /// The transfers under way: each is a thread of its own, out of the worker's queue.
    transfers: Transfers,
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
            scratch: Mutex::new(None),
            made: AtomicU64::new(0),
            parts: Arc::default(),
            way: None,
            waits: Waits::default(),
            transfers: Transfers::default(),
        }
    }

    /// These files' program was offered the folder that `way` leads to (see [`crate::stage`]):
    /// its `stage` and `fetch` are carried out there. Without it they are answered `io`, the
    /// session having offered nothing to be asked for.
    #[must_use]
    pub fn with_way(mut self, way: Way) -> Self {
        self.way = Some(way);
        self
    }

    /// How long the transfers wait on a pipe (a test's, shorter than the product's).
    #[must_use]
    pub fn with_waits(mut self, waits: Waits) -> Self {
        self.waits = waits;
        self
    }

    /// A file of `name` made of `bytes` in a folder of the pane's own, for something the person
    /// gave that is no file (a picture pasted from the pasteboard): the program is given it as
    /// it is given any other, by a handle. The folder is the pane's alone (`0700`, under the
    /// temporary folder) and goes with these files, when the view's page is gone; every file is
    /// in a folder of its own, so that two of one name never replace each other.
    ///
    /// # Errors
    ///
    /// The folder or the file cannot be made, or `name` is more than a name.
    pub fn scratch(&self, name: &str, bytes: &[u8]) -> io::Result<PathBuf> {
        if Path::new(name).file_name().and_then(|n| n.to_str()) != Some(name) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "not a name"));
        }
        let root = {
            let mut slot = lock(&self.scratch);
            if slot.is_none() {
                static FOLDERS: AtomicU64 = AtomicU64::new(0);
                let folder = std::env::temp_dir().join(format!(
                    "bateri-view-{}-{}",
                    std::process::id(),
                    FOLDERS.fetch_add(1, Ordering::Relaxed)
                ));
                fs::DirBuilder::new().mode(0o700).create(&folder)?;
                *slot = Some(folder);
            }
            slot.clone().ok_or_else(|| io::Error::other("no folder"))?
        };
        let folder = root.join(self.made.fetch_add(1, Ordering::Relaxed).to_string());
        fs::DirBuilder::new().mode(0o700).create(&folder)?;
        let path = folder.join(name);
        fs::write(&path, bytes)?;
        Ok(path)
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
        if source.size > picture_limit() {
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

    /// Pours the file `handle` names, from `offset`, into the pipe `pipe` of the folder, on a
    /// thread of its own. The session is told that the pipe is there
    /// ([`Outcomes::pipe_ready`]) and then how the transfer ended ([`Outcomes::staged`]).
    pub fn stage(&self, transfer: u32, handle: &str, offset: u64, pipe: &str) {
        match self.ground() {
            Some(ground) => {
                self.transfers
                    .stage(ground, transfer, handle.to_owned(), offset, pipe);
            }
            None => self.outcomes.staged(transfer, Err(StageError::Io)),
        }
    }

    /// Fills the part of the save `handle` names from the pipe `pipe` of the folder, from `offset`
    /// until the file is `size` long, on a thread of its own, and gives the part the place's name
    /// ([`Outcomes::pipe_ready`], then [`Outcomes::fetched`]).
    pub fn fetch(&self, transfer: u32, handle: &str, offset: u64, size: u64, pipe: &str) {
        match self.ground() {
            Some(ground) => {
                self.transfers
                    .fetch(ground, transfer, handle.to_owned(), offset, size, pipe);
            }
            None => self.outcomes.fetched(transfer, Err(FetchError::Io)),
        }
    }

    /// The transfer whose id this is is taken back: it stops, its pipe goes and so does what it had
    /// written of a part. The session is told nothing: it asked for this.
    pub fn unstage(&self, transfer: u32) {
        self.transfers.unstage(transfer);
    }

    /// Every transfer is taken back (the view ended, or the pane is closing). Does not wait for
    /// the threads: each notices at its next look at its pipe and goes.
    pub fn end_transfers(&self) {
        self.transfers.end_all();
    }

    /// How many transfers are under way.
    #[cfg(test)]
    pub(super) fn open_transfers(&self) -> usize {
        self.transfers.open()
    }

    /// What a transfer needs, when the program was offered a folder.
    fn ground(&self) -> Option<Ground> {
        Some(Ground {
            table: Arc::clone(&self.table),
            parts: Arc::clone(&self.parts),
            outcomes: Arc::clone(&self.outcomes),
            quarantine: Arc::clone(&self.quarantine),
            way: self.way.clone()?,
            waits: self.waits,
        })
    }

    fn send(&self, job: Job) {
        let mut jobs = lock(&self.jobs);
        if jobs.is_none() {
            let (sender, receiver) = mpsc::channel();
            let work = Worker {
                table: Arc::clone(&self.table),
                outcomes: Arc::clone(&self.outcomes),
                quarantine: Arc::clone(&self.quarantine),
                parts: Arc::clone(&self.parts),
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

impl Drop for ViewFiles {
    fn drop(&mut self) {
        self.transfers.end_all();
        if let Some(folder) = lock(&self.scratch).take() {
            let _ = fs::remove_dir_all(folder);
        }
    }
}

/// How the files a person lets go over a pane are taken, by what is up in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropWay {
    /// A page whose view has files (`lua` 2.1) is up: it is given them by handle,
    /// with the path of each where the program reads what the person does (a local pane).
    Handles,
    /// A page whose view has none is up: it is told the paths, as it was before files.
    Paths,
    /// No page is up, in a pane of a remote session: the files are uploaded.
    Upload,
    /// No page is up, in a local pane: the paths are written to the line, quoted.
    Paste,
}

/// The way a drop is taken. `page` is `None` where no page of a program's view is up, and
/// otherwise whether its view has files.
#[must_use]
pub fn drop_way(page: Option<bool>, remote: bool) -> DropWay {
    match (page, remote) {
        (Some(true), _) => DropWay::Handles,
        (Some(false), _) => DropWay::Paths,
        (None, true) => DropWay::Upload,
        (None, false) => DropWay::Paste,
    }
}

/// What a page is told of the files the person gave it: an entry for each of `paths` that is
/// a file or a folder, in their order. A file is made a handle of by the session (`None` from
/// it — the view has no files, or no room for another — leaves the file out), noted in `files`
/// before the page hears of it, and given the path it has when `local`: only when the program
/// reads the same files as the person, which it does not in a remote session. A folder is no
/// file to read: it is told with no handle (§8.9). What is neither, or cannot be looked at, is
/// left out. `media_type` is the terminal's guess at a file's type.
pub fn entries_given(
    session: &Session,
    files: &ViewFiles,
    paths: &[PathBuf],
    local: bool,
    media_type: &dyn Fn(&Path) -> String,
) -> Vec<FileEntry> {
    let mut entries = Vec::new();
    for path in paths {
        let Some(name) = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
        else {
            continue;
        };
        let Ok(meta) = fs::metadata(path) else {
            continue;
        };
        let shown = if local {
            path.to_str().map(str::to_owned)
        } else {
            None
        };
        if meta.is_dir() {
            entries.push(FileEntry {
                handle: None,
                name,
                size: 0,
                media_type: "inode/directory".to_owned(),
                path: shown,
                folder: true,
            });
            continue;
        }
        let Ok(given) = GivenFile::from_meta(path, &meta) else {
            continue;
        };
        let Some(handle) = session.grant_file(&FileGrant { size: given.size }) else {
            continue;
        };
        let size = given.size;
        files.keep_given(&handle, given);
        entries.push(FileEntry {
            handle: Some(handle),
            name,
            size,
            media_type: media_type(path),
            path: shown,
            folder: false,
        });
    }
    entries
}

/// The place the person chose in the Save panel, as the page is told of it: the handle the
/// session made of it, noted in `files` before the page hears of it, with the name the person
/// gave the file. `None` when the view has no files (it ended while the panel was up).
pub fn place_saved(session: &Session, files: &ViewFiles, target: PathBuf) -> Option<Saving> {
    let name = target.file_name()?.to_string_lossy().into_owned();
    let handle = session.grant_save()?;
    files.keep_save(&handle, target);
    Some(Saving { handle, name })
}

/// The thread that does the jobs, in the order they came. It ends when the pane's files are
/// dropped, after the jobs that are in its queue, and deletes the parts that were not finished.
struct Worker {
    table: Arc<Mutex<HashMap<String, Row>>>,
    outcomes: Arc<dyn Outcomes>,
    quarantine: Quarantine,
    /// The saves under way, by handle.
    parts: Arc<Mutex<HashMap<String, Part>>>,
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
                        read_range(&source, offset, length)
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
                        lock(&self.parts).remove(&handle);
                    }
                    self.outcomes.file_written(write, result);
                }
                Job::Abort { handle } => {
                    lock(&self.parts).remove(&handle);
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
        let mut parts = lock(&self.parts);
        if offset == 0 && !parts.contains_key(handle) {
            parts.insert(
                handle.to_owned(),
                Part::create(&target, handle).map_err(write_error)?,
            );
        }
        let part = parts.get_mut(handle).ok_or(FileError::Io)?;
        // The session writes in order and with no gap; a part that is not where the write is
        // would be a file the program did not write.
        if part.length != offset {
            return Err(FileError::Io);
        }
        part.file.write_all_at(bytes, offset).map_err(write_error)?;
        part.length = part.length.saturating_add(bytes.len() as u64);
        if end && let Some(part) = parts.remove(handle) {
            part.keep(&target).map_err(write_error)?;
            (self.quarantine)(&target);
        }
        Ok(())
    }
}

/// The longest a file's name may be on the disks bateri runs on.
const NAME_MAX: usize = 255;

/// What ends the name of a part.
const PART_SUFFIX: &str = ".pencere-part";

/// Where the part of a save goes: beside `target`, hidden, and named for the save as well as for
/// its place (the last of its handle), so that two saves to one place do not share a part, and
/// short enough to be a name wherever the place's own is.
fn part_path(target: &Path, handle: &str) -> io::Result<PathBuf> {
    let name = target
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "a place with no name"))?
        .to_string_lossy();
    let tag = &handle[handle.len().saturating_sub(8)..];
    let room = NAME_MAX - ".".len() - ".".len() - tag.len() - PART_SUFFIX.len();
    let mut end = name.len().min(room);
    while !name.is_char_boundary(end) {
        end -= 1;
    }
    Ok(target.with_file_name(format!(".{}.{tag}{PART_SUFFIX}", &name[..end])))
}

/// A save under way: the part, beside the place, which is deleted if it is dropped unkept.
pub(super) struct Part {
    path: PathBuf,
    pub(super) file: fs::File,
    pub(super) length: u64,
    /// The part has been given its place's name: there is nothing to delete.
    kept: bool,
}

impl Part {
    pub(super) fn create(target: &Path, handle: &str) -> io::Result<Self> {
        let path = part_path(target, handle)?;
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
            kept: false,
        })
    }

    /// Gives the part the name of its place.
    pub(super) fn keep(mut self, target: &Path) -> io::Result<()> {
        self.file.sync_all()?;
        fs::rename(&self.path, target)?;
        // The name is the place's now: the file closes with the part and nothing is deleted.
        self.kept = true;
        Ok(())
    }
}

impl Drop for Part {
    fn drop(&mut self) {
        if !self.kept {
            let _ = fs::remove_file(&self.path);
        }
    }
}

/// What a failure of writing is told to the program as: a disk with no room, or any other.
pub(super) fn write_error(error: io::Error) -> FileError {
    match error.raw_os_error() {
        Some(libc::ENOSPC | libc::EDQUOT) => FileError::Space,
        _ => FileError::Io,
    }
}

/// Up to `length` bytes of `source` from `offset` (fewer where the file ends), if the file is as
/// it was given.
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
pub(super) fn read_error(error: io::Error) -> FileError {
    match error.kind() {
        io::ErrorKind::NotFound | io::ErrorKind::PermissionDenied => FileError::Gone,
        _ => FileError::Io,
    }
}

/// The lock, whether or not a thread that held it panicked: the tables are changed a row at a
/// time and never found half-changed.
pub(super) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
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
        fn pipe_ready(&self, _: u32) {}
        fn staged(&self, _: u32, _: Result<u64, StageError>) {}
        fn fetched(&self, _: u32, _: Result<u64, FetchError>) {}
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
        part_path(
            &dir.join(name),
            &format!("{:032x}", 0x5a5a_0000 + name.len()),
        )
        .unwrap()
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
    fn two_saves_to_one_place_keep_a_part_each() {
        let dir = Dir::new("twice");
        let (files, receiver, _) = files();
        let (first, second) = ("1".repeat(32), "2".repeat(32));
        files.keep_save(&first, dir.join("out.txt"));
        files.keep_save(&second, dir.join("out.txt"));
        files.write(1, &first, 0, b"first".to_vec(), false);
        files.write(2, &second, 0, b"second".to_vec(), false);
        assert_eq!(told(&receiver), Told::Written(1, Ok(())));
        assert_eq!(told(&receiver), Told::Written(2, Ok(())));
        let parts = |dir: &Dir| {
            fs::read_dir(&dir.0)
                .unwrap()
                .filter(|entry| {
                    entry
                        .as_ref()
                        .unwrap()
                        .file_name()
                        .to_string_lossy()
                        .ends_with(PART_SUFFIX)
                })
                .count()
        };
        assert_eq!(parts(&dir), 2, "one part for each save");
        files.write(3, &first, 5, Vec::new(), true);
        assert_eq!(told(&receiver), Told::Written(3, Ok(())));
        assert_eq!(fs::read(dir.join("out.txt")).unwrap(), b"first");
        assert_eq!(parts(&dir), 1, "the other save still has its part");
        files.write(4, &second, 6, Vec::new(), true);
        assert_eq!(told(&receiver), Told::Written(4, Ok(())));
        assert_eq!(fs::read(dir.join("out.txt")).unwrap(), b"second");
        assert_eq!(parts(&dir), 0);
    }

    #[test]
    fn a_place_with_the_longest_name_is_saved_to() {
        let dir = Dir::new("long");
        let (files, receiver, _) = files();
        // The place's own name fits; the part's would not if it were the name and more.
        let name = format!("{}.txt", "a".repeat(NAME_MAX - 4));
        let handle = save(&files, &dir, &name);
        files.write(1, &handle, 0, b"long".to_vec(), true);
        assert_eq!(told(&receiver), Told::Written(1, Ok(())));
        assert_eq!(fs::read(dir.join(&name)).unwrap(), b"long");
        let part = part_path(&dir.join(&name), &handle).unwrap();
        assert!(part.file_name().unwrap().len() <= NAME_MAX);
        // Cut inside a character is cut before it.
        let wide = dir.join(&format!("{}.txt", "é".repeat(120)));
        assert!(
            part_path(&wide, &handle)
                .unwrap()
                .file_name()
                .unwrap()
                .len()
                <= NAME_MAX
        );
    }

    #[test]
    fn a_save_that_is_given_its_name_closes_its_file() {
        let dir = Dir::new("closes");
        let (files, receiver, _) = files();
        let open = || fs::read_dir("/dev/fd").unwrap().count();
        let before = open();
        for n in 0..150u32 {
            let handle = format!("{n:032x}");
            files.keep_save(&handle, dir.join(&format!("out-{n}.txt")));
            files.write(n, &handle, 0, b"x".to_vec(), true);
            assert_eq!(told(&receiver), Told::Written(n, Ok(())));
        }
        // Other tests open and close files beside this one; a file left open by each save would
        // be a hundred and fifty.
        assert!(
            open() < before + 75,
            "{before} open before, {} after",
            open()
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
    fn what_the_pane_makes_a_file_of_is_in_a_folder_of_its_own_that_goes_with_the_files() {
        use std::os::unix::fs::PermissionsExt;
        let (files, _receiver, _) = files();
        let first = files.scratch("image.png", b"one").unwrap();
        let second = files.scratch("image.png", b"two").unwrap();
        assert_ne!(first, second, "two of one name do not replace each other");
        assert_eq!(fs::read(&first).unwrap(), b"one");
        assert_eq!(fs::read(&second).unwrap(), b"two");
        let root = first.parent().unwrap().parent().unwrap().to_owned();
        assert_eq!(
            fs::metadata(&root).unwrap().permissions().mode() & 0o777,
            0o700
        );
        // A name is a name: nothing a path could be made of.
        for bad in ["", "..", "a/b", "/etc/passwd", "../x"] {
            assert!(files.scratch(bad, b"x").is_err(), "{bad:?}");
        }
        drop(files);
        assert!(!root.exists(), "the folder goes with the files");
    }

    #[test]
    fn a_drop_is_taken_by_the_page_that_is_up_and_by_the_pane_when_none_is() {
        assert_eq!(drop_way(Some(true), false), DropWay::Handles);
        assert_eq!(drop_way(Some(true), true), DropWay::Handles);
        assert_eq!(drop_way(Some(false), false), DropWay::Paths);
        assert_eq!(drop_way(Some(false), true), DropWay::Paths);
        assert_eq!(drop_way(None, true), DropWay::Upload);
        assert_eq!(drop_way(None, false), DropWay::Paste);
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
