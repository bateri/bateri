//! The bytes of a file crossing through a pipe in the folder bateri keeps on the program's machine
//! (specification §8.11), in place of the `read`s and `write`s of the files beside a view's bundle
//! (`files`): a person's file poured into a pipe the program reads (`stage`), and a pipe the
//! program fills poured into a part beside the place the person chose (`fetch`).
//!
//! **A transfer is a thread of its own**, not a job of the files' worker: that worker serves one
//! job at a time, and a transfer lasts as long as the file is long, so a large one would keep the
//! same pane's other reads waiting. Every wait on a pipe is a poll of a few milliseconds that
//! looks at the transfer's cancel flag, so that nothing in here blocks for good on a program that
//! never opens its end, and a transfer taken back (`unstage`, the view's end, the pane's) ends
//! at once and takes its pipe and its part with it.
//!
//! A program on a server has its pipe there: a script run through the account's main connection
//! makes it and carries the bytes to or from it ([`server`]); the thread that runs it is the same
//! kind of thread, and it is taken back the same way.
//!
//! The session names the pipe (32 hex digits of its own randomness) and rules on who may ask
//! for what; this is only the moving. What came of it is told to the session
//! ([`Outcomes`]): first that the pipe is there, then how it ended.

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::os::unix::fs::FileExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use bt_core::pencere::{FetchError, FileError, StageError};

use super::files::{GivenFile, Outcomes, Part, Quarantine, Row, lock, read_error, write_error};
use crate::stage::{self, Way};

mod server;

/// How a transfer waits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Waits {
    /// How long a pipe that the program was told of may stay unopened (to stage) or unwritten
    /// (to fetch) before the transfer is `io`: the program that does not take it is not coming,
    /// as it does not wait for a first byte for ever either.
    pub opened_within: Duration,
    /// How long a pipe whose other end is open may stay silent before the transfer is `io`.
    pub silent_for: Duration,
    /// What a transfer looks at between two tries of a pipe, and so the longest that a
    /// transfer taken back takes to notice.
    pub look_again: Duration,
    /// How long the end of a transfer on a server waits for the script there to have ended: the
    /// pipe is gone when it has, and the program is not told the transfer is over before.
    pub tidy_within: Duration,
}

impl Default for Waits {
    fn default() -> Self {
        Self {
            opened_within: Duration::from_secs(10),
            silent_for: Duration::from_secs(30),
            look_again: Duration::from_millis(5),
            tidy_within: Duration::from_secs(2),
        }
    }
}

/// The piece a file is poured in, and the longest a poll of a pipe sleeps.
const PIECE: usize = 64 * 1024;
const POLL_MS: i32 = 20;

/// What a transfer needs of the files it belongs to.
#[derive(Clone)]
pub(super) struct Ground {
    pub(super) table: Arc<Mutex<HashMap<String, Row>>>,
    /// The saves under way, by handle: the part a fetch writes is the one a `write` would.
    pub(super) parts: Arc<Mutex<HashMap<String, Part>>>,
    pub(super) outcomes: Arc<dyn Outcomes>,
    pub(super) quarantine: Quarantine,
    pub(super) way: Way,
    pub(super) waits: Waits,
}

struct Running {
    cancel: Arc<AtomicBool>,
    /// The pipe, when it is a file of this Mac's: the one of a server is taken with the script
    /// that made it.
    pipe: Option<PathBuf>,
}

/// The transfers of a view that are under way, by the id of the `stage` or `fetch` they answer.
#[derive(Default)]
pub(super) struct Transfers {
    running: Arc<Mutex<HashMap<u32, Running>>>,
}

impl Transfers {
    /// Pours the file `handle` names, from `offset`, into the pipe `pipe`.
    pub(super) fn stage(
        &self,
        ground: Ground,
        transfer: u32,
        handle: String,
        offset: u64,
        pipe: &str,
    ) {
        let started = if stage::is_pipe_name(pipe) {
            match ground.way.clone() {
                Way::Here(dir) => {
                    let path = dir.join(pipe);
                    let own = path.clone();
                    self.start(transfer, Some(path), &ground, move |ground, cancel| {
                        run_stage(ground, transfer, &handle, offset, &own, cancel)
                            .map(Finished::Staged)
                    })
                }
                Way::Server(server) => {
                    let name = pipe.to_owned();
                    self.start(transfer, None, &ground, move |ground, cancel| {
                        server::run_stage(ground, &server, transfer, &handle, offset, &name, cancel)
                            .map(Finished::Staged)
                    })
                }
            }
        } else {
            None
        };
        if started.is_none() {
            ground.outcomes.staged(transfer, Err(StageError::Io));
        }
    }

    /// Fills the part of the save `handle` names from the pipe `pipe`, until the file is `size`
    /// long.
    pub(super) fn fetch(
        &self,
        ground: Ground,
        transfer: u32,
        handle: String,
        offset: u64,
        size: u64,
        pipe: &str,
    ) {
        let started = if stage::is_pipe_name(pipe) {
            match ground.way.clone() {
                Way::Here(dir) => {
                    let path = dir.join(pipe);
                    let own = path.clone();
                    self.start(transfer, Some(path), &ground, move |ground, cancel| {
                        run_fetch(ground, transfer, &handle, offset, size, &own, cancel)
                            .map(Finished::Fetched)
                    })
                }
                Way::Server(server) => {
                    let name = pipe.to_owned();
                    self.start(transfer, None, &ground, move |ground, cancel| {
                        server::run_fetch(
                            ground, &server, transfer, &handle, offset, size, &name, cancel,
                        )
                        .map(Finished::Fetched)
                    })
                }
            }
        } else {
            None
        };
        if started.is_none() {
            ground.outcomes.fetched(transfer, Err(FetchError::Io));
        }
    }

    /// The transfer is taken back: its thread stops, the pipe goes at once and what it had
    /// written of a part goes with the thread. Nothing is told to the session.
    pub(super) fn unstage(&self, transfer: u32) {
        if let Some(running) = lock(&self.running).get(&transfer) {
            running.cancel.store(true, Ordering::Release);
            if let Some(pipe) = &running.pipe {
                let _ = fs::remove_file(pipe);
            }
        }
    }

    /// Every transfer is taken back: the view is over, or the pane is.
    pub(super) fn end_all(&self) {
        for running in lock(&self.running).values() {
            running.cancel.store(true, Ordering::Release);
            if let Some(pipe) = &running.pipe {
                let _ = fs::remove_file(pipe);
            }
        }
    }

    /// How many are under way.
    #[cfg(test)]
    pub(super) fn open(&self) -> usize {
        lock(&self.running).len()
    }

    /// Starts `work` on a thread of its own; `None` when there is no thread to start. `work`
    /// returns how the transfer ended, or nothing when it was taken back. `pipe` is the file the
    /// transfer makes on this Mac, if it makes one.
    fn start(
        &self,
        transfer: u32,
        pipe: Option<PathBuf>,
        ground: &Ground,
        work: impl FnOnce(&Ground, &AtomicBool) -> Option<Finished> + Send + 'static,
    ) -> Option<()> {
        let cancel = Arc::new(AtomicBool::new(false));
        lock(&self.running).insert(
            transfer,
            Running {
                cancel: Arc::clone(&cancel),
                pipe: pipe.clone(),
            },
        );
        let ground = ground.clone();
        let running = Arc::clone(&self.running);
        let spawned = thread::Builder::new()
            .name("view-transfer".into())
            .spawn(move || {
                let finished = work(&ground, &cancel);
                // The pipe goes before the session hears how it ended: "closes the pipe and
                // deletes it, whatever the answer" is the protocol's, and a program that is told
                // `sent` and looks at the folder finds it empty.
                if let Some(pipe) = &pipe {
                    let _ = fs::remove_file(pipe);
                }
                lock(&running).remove(&transfer);
                if cancel.load(Ordering::Acquire) {
                    return;
                }
                match finished {
                    Some(Finished::Staged(result)) => ground.outcomes.staged(transfer, result),
                    Some(Finished::Fetched(result)) => ground.outcomes.fetched(transfer, result),
                    None => {}
                }
            });
        if spawned.is_err() {
            lock(&self.running).remove(&transfer);
            return None;
        }
        Some(())
    }
}

enum Finished {
    Staged(Result<u64, StageError>),
    Fetched(Result<u64, FetchError>),
}

/// A file given to the view as it is now, if it is as it was given.
fn same(file: &File, source: &GivenFile) -> Result<(), StageError> {
    let meta = file.metadata().map_err(|_| StageError::Gone)?;
    if !meta.is_file() || meta.len() != source.size || meta.modified().ok() != source.modified {
        return Err(StageError::Gone);
    }
    Ok(())
}

/// The file `handle` names, open, as it was given and with `offset` inside it: what a stage
/// pours from.
fn open_source(
    ground: &Ground,
    handle: &str,
    offset: u64,
) -> Result<(File, GivenFile), StageError> {
    let source = match lock(&ground.table).get(handle) {
        Some(Row::Given(source)) => source.clone(),
        _ => return Err(StageError::Gone),
    };
    let file = File::open(&source.path).map_err(|error| match read_error(error) {
        FileError::Gone => StageError::Gone,
        _ => StageError::Io,
    })?;
    // The file that is open is the one looked at: what is checked is what is poured.
    same(&file, &source)?;
    if offset > source.size {
        return Err(StageError::Size);
    }
    Ok((file, source))
}

/// Pours the file `handle` names, from `offset` to its end, into the pipe at `pipe`, once the
/// program has opened it. `None` when the transfer was taken back.
fn run_stage(
    ground: &Ground,
    transfer: u32,
    handle: &str,
    offset: u64,
    pipe: &Path,
    cancel: &AtomicBool,
) -> Option<Result<u64, StageError>> {
    let (file, source) = match open_source(ground, handle, offset) {
        Ok(opened) => opened,
        Err(error) => return Some(Err(error)),
    };
    if stage::make_fifo(pipe).is_err() {
        return Some(Err(StageError::Io));
    }
    if cancel.load(Ordering::Acquire) {
        return None;
    }
    ground.outcomes.pipe_ready(transfer);
    let waits = ground.waits;
    // The writing end opens once the program has the reading end open.
    let began = Instant::now();
    let mut writing = loop {
        if cancel.load(Ordering::Acquire) {
            return None;
        }
        match stage::open_pipe(pipe, false) {
            Ok(writing) => break writing,
            Err(error) if error.raw_os_error() == Some(libc::ENXIO) => {
                if began.elapsed() > waits.opened_within {
                    return Some(Err(StageError::Io));
                }
                thread::sleep(waits.look_again);
            }
            Err(_) => return Some(Err(StageError::Io)),
        }
    };
    let mut buffer = vec![0u8; PIECE];
    let mut at = offset;
    while at < source.size {
        if cancel.load(Ordering::Acquire) {
            return None;
        }
        let wanted = usize::try_from(source.size - at).map_or(PIECE, |left| left.min(PIECE));
        let read = match file.read_at(&mut buffer[..wanted], at) {
            // A file that ends sooner than it said has changed.
            Ok(0) => return Some(Err(StageError::Gone)),
            Ok(read) => read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => return Some(Err(StageError::Io)),
        };
        match pour(&mut writing, &buffer[..read], cancel, waits.silent_for) {
            Ok(true) => at += read as u64,
            Ok(false) => return None,
            Err(_) => return Some(Err(StageError::Io)),
        }
    }
    // What was poured is what was given: a file that changed under the pour is not the file.
    if let Err(error) = same(&file, &source) {
        return Some(Err(error));
    }
    Some(Ok(source.size))
}

/// Writes all of `bytes` into the pipe; `false` when the transfer was taken back first. A pipe
/// that nobody reads for `silent_for` is a failure.
fn pour(
    pipe: &mut File,
    mut bytes: &[u8],
    cancel: &AtomicBool,
    silent_for: Duration,
) -> io::Result<bool> {
    let mut moved = Instant::now();
    while !bytes.is_empty() {
        if cancel.load(Ordering::Acquire) {
            return Ok(false);
        }
        match pipe.write(bytes) {
            Ok(written) => {
                bytes = &bytes[written..];
                moved = Instant::now();
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                if !stage::ready(pipe, false, POLL_MS) && moved.elapsed() > silent_for {
                    return Err(io::ErrorKind::TimedOut.into());
                }
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            // The program closed its end before the file was in: it gave the file up, or ended.
            Err(error) => return Err(error),
        }
    }
    Ok(true)
}

/// What a failure of writing a part is told to the program as.
fn fetch_error(error: io::Error) -> FetchError {
    match write_error(error) {
        FileError::Space => FetchError::Space,
        _ => FetchError::Io,
    }
}

/// The place of the save `handle` names and the part its bytes go to: made anew for the first
/// bytes, and the one that holds `offset` bytes after.
fn take_part(
    ground: &Ground,
    handle: &str,
    offset: u64,
    size: u64,
) -> Result<(PathBuf, Part), FetchError> {
    let target = match lock(&ground.table).get(handle) {
        Some(Row::Save(target)) => target.clone(),
        _ => return Err(FetchError::Io),
    };
    if offset > size {
        return Err(FetchError::Size);
    }
    let earlier = lock(&ground.parts).remove(handle);
    let part = if offset == 0 {
        drop(earlier);
        Part::create(&target, handle).map_err(fetch_error)?
    } else {
        match earlier {
            Some(part) if part.length == offset => part,
            _ => return Err(FetchError::Size),
        }
    };
    Ok((target, part))
}

/// Reads the pipe at `pipe` into the part of the save `handle` names until the file is `size` long,
/// then gives the part the place's name. `None` when the transfer was taken back.
fn run_fetch(
    ground: &Ground,
    transfer: u32,
    handle: &str,
    offset: u64,
    size: u64,
    pipe: &Path,
    cancel: &AtomicBool,
) -> Option<Result<u64, FetchError>> {
    let (target, mut part) = match take_part(ground, handle, offset, size) {
        Ok(taken) => taken,
        Err(error) => return Some(Err(error)),
    };
    if stage::make_fifo(pipe).is_err() {
        return Some(Err(FetchError::Io));
    }
    // The reading end is open before the program is told of the pipe, so that it can open the
    // writing end at once.
    let Ok(mut reading) = stage::open_pipe(pipe, true) else {
        return Some(Err(FetchError::Io));
    };
    if cancel.load(Ordering::Acquire) {
        return None;
    }
    ground.outcomes.pipe_ready(transfer);
    let waits = ground.waits;
    let want = size - offset;
    let mut buffer = vec![0u8; PIECE];
    let mut got = 0u64;
    let began = Instant::now();
    let mut last = began;
    while got < want {
        if cancel.load(Ordering::Acquire) {
            return None;
        }
        let step = usize::try_from(want - got).map_or(PIECE, |left| left.min(PIECE));
        match reading.read(&mut buffer[..step]) {
            // No end is open to write: the program has not opened its yet, or it closed it.
            Ok(0) => {
                if got > 0 {
                    // What had come is all that does.
                    return Some(Err(FetchError::Size));
                }
                if began.elapsed() > waits.opened_within {
                    return Some(Err(FetchError::Io));
                }
                thread::sleep(waits.look_again);
            }
            Ok(read) => {
                if let Err(error) = part.file.write_all_at(&buffer[..read], part.length) {
                    return Some(Err(fetch_error(error)));
                }
                part.length += read as u64;
                got += read as u64;
                last = Instant::now();
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                if !stage::ready(&reading, true, POLL_MS) && last.elapsed() > waits.silent_for {
                    return Some(Err(FetchError::Io));
                }
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(_) => return Some(Err(FetchError::Io)),
        }
    }
    // A pipe from which more would come than the file has: the bytes are not the file's.
    let _ = stage::ready(&reading, true, POLL_MS * 2);
    if matches!(reading.read(&mut buffer), Ok(more) if more > 0) {
        return Some(Err(FetchError::Size));
    }
    if cancel.load(Ordering::Acquire) {
        return None;
    }
    if let Err(error) = part.keep(&target) {
        return Some(Err(fetch_error(error)));
    }
    (ground.quarantine)(&target);
    Some(Ok(size))
}

#[cfg(test)]
mod tests {
    use super::super::files::{GivenFile, ViewFiles};
    use super::*;
    use crate::child::wait_until;
    use crate::kablo::remote::RemoteCommand;
    use crate::kablo::served::Served;
    use crate::ssh_route::Ask;
    use crate::stage::{Folder, prepare_here};
    use std::sync::mpsc::{self, Receiver, Sender};
    use std::time::SystemTime;

    pub(super) const PIPE: &str = "0b5d7c1e9a3f4682d0e1c7a95b3f8d24";

    /// What the session was told, in order.
    #[derive(Debug, PartialEq)]
    pub(super) enum Told {
        PipeReady(u32),
        Staged(u32, Result<u64, StageError>),
        Fetched(u32, Result<u64, FetchError>),
    }

    pub(super) struct Tell(Mutex<Sender<Told>>);

    impl Outcomes for Tell {
        fn file_read(&self, _: u32, _: Result<Vec<u8>, FileError>) {}
        fn file_written(&self, _: u32, _: Result<(), FileError>) {}
        fn pipe_ready(&self, transfer: u32) {
            let _ = lock(&self.0).send(Told::PipeReady(transfer));
        }
        fn staged(&self, transfer: u32, result: Result<u64, StageError>) {
            let _ = lock(&self.0).send(Told::Staged(transfer, result));
        }
        fn fetched(&self, transfer: u32, result: Result<u64, FetchError>) {
            let _ = lock(&self.0).send(Told::Fetched(transfer, result));
        }
    }

    /// A home, the folder bateri keeps there, and the files of a view that was offered it.
    pub(super) struct Rig {
        pub(super) home: PathBuf,
        pub(super) folder: Folder,
        pub(super) files: ViewFiles,
        pub(super) told: Receiver<Told>,
        pub(super) marked: Arc<Mutex<Vec<PathBuf>>>,
        _clean: Clean,
    }

    /// The home goes when the rig does.
    struct Clean(PathBuf);

    impl Drop for Clean {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    impl Rig {
        pub(super) fn new(name: &str) -> Self {
            Self::with_waits(name, Waits::default())
        }

        pub(super) fn with_waits(name: &str, waits: Waits) -> Self {
            Self::build(name, waits, |home| {
                let folder = prepare_here(home, "c148d55a").expect("a folder");
                let way = Way::Here(folder.dir().to_owned());
                (folder, way)
            })
        }

        /// A rig whose folder is the one of a server, made by the script that makes it, and whose
        /// way is an ssh that runs the commands here.
        pub(super) fn server(name: &str, waits: Waits) -> Self {
            Self::server_over(name, waits, |home| {
                let ssh = home.join("ssh");
                crate::child::tests::write_program(&ssh, &fake_ssh(home));
                let argv = vec![
                    ssh.to_string_lossy().into_owned(),
                    "-T".into(),
                    "host".into(),
                ];
                Served::fake(argv)
            })
        }

        /// A rig whose way is the server `served` makes of this computer, its folder made by the
        /// script that makes it.
        pub(super) fn server_over(
            name: &str,
            waits: Waits,
            served: impl FnOnce(&Path) -> Served,
        ) -> Self {
            Self::build(name, waits, |home| {
                let folder = crate::kablo::install::prepare_stage(&Sh(home.to_owned()), "c148d55a")
                    .expect("a folder");
                let dir = folder.dir().to_string_lossy().into_owned();
                let way = Way::Server(crate::stage::Server::new(
                    dir,
                    Arc::new(Fixed(served(home))),
                ));
                (folder, way)
            })
        }

        /// A rig whose way is a server that cannot be reached with no one asked.
        pub(super) fn unreachable(name: &str, waits: Waits) -> Self {
            Self::build(name, waits, |home| {
                let folder = crate::kablo::install::prepare_stage(&Sh(home.to_owned()), "c148d55a")
                    .expect("a folder");
                let dir = folder.dir().to_string_lossy().into_owned();
                let way = Way::Server(crate::stage::Server::new(
                    dir,
                    Arc::new(Fixed(Served::none())),
                ));
                (folder, way)
            })
        }

        fn build(
            name: &str,
            waits: Waits,
            folder_and_way: impl FnOnce(&Path) -> (Folder, Way),
        ) -> Self {
            let home = std::env::temp_dir()
                .join(format!("bateri-transfers-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&home);
            fs::create_dir_all(&home).expect("a home");
            let clean = Clean(home.clone());
            let (folder, way) = folder_and_way(&home);
            let (sender, told) = mpsc::channel();
            let marked = Arc::new(Mutex::new(Vec::new()));
            let mark = Arc::clone(&marked);
            let files = ViewFiles::new(
                Arc::new(Tell(Mutex::new(sender))),
                Arc::new(move |path: &Path| lock(&mark).push(path.to_owned())),
            )
            .with_way(way)
            .with_waits(waits);
            Self {
                _clean: clean,
                home,
                folder,
                files,
                told,
                marked,
            }
        }

        /// What the fake ssh does from now on (see [`fake_ssh`]).
        pub(super) fn mode(&self, mode: &str) {
            fs::write(self.home.join("fake-mode"), mode).expect("a mode");
        }

        pub(super) fn pipe(&self) -> PathBuf {
            self.folder.dir().join(PIPE)
        }

        /// A file the person gave, of `bytes`.
        pub(super) fn give(&self, bytes: &[u8]) -> String {
            let path = self.home.join("given.bin");
            fs::write(&path, bytes).expect("a file");
            let handle = "1".repeat(32);
            self.files
                .keep_given(&handle, GivenFile::of(&path).expect("a source"));
            handle
        }

        /// A place the person chose to save in.
        pub(super) fn place(&self, name: &str) -> (String, PathBuf) {
            let target = self.home.join(name);
            let handle = "2".repeat(32);
            self.files.keep_save(&handle, target.clone());
            (handle, target)
        }

        pub(super) fn next(&self) -> Told {
            self.told
                .recv_timeout(Duration::from_secs(10))
                .expect("the session was told")
        }

        /// Nothing more was told, though the transfer had time to say it.
        pub(super) fn nothing_more(&self) {
            thread::sleep(Duration::from_millis(150));
            assert_eq!(self.told.try_recv().ok(), None);
        }

        /// What is in the folder besides its marker.
        pub(super) fn left_in_folder(&self) -> Vec<String> {
            let mut names: Vec<String> = fs::read_dir(self.folder.dir())
                .expect("a listing")
                .flatten()
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .filter(|name| name != crate::stage::MARKER)
                .collect();
            names.sort();
            names
        }

        /// Every file in the home's top level that is a part.
        pub(super) fn parts(&self) -> Vec<String> {
            fs::read_dir(&self.home)
                .expect("a listing")
                .flatten()
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .filter(|name| name.ends_with(".pencere-part"))
                .collect()
        }
    }

    /// A channel that runs a script for real with `sh`, in a home of its own.
    struct Sh(PathBuf);

    impl crate::kablo::install::Channel for Sh {
        fn run(&self, script: &str, input: &[u8]) -> io::Result<crate::kablo::install::Reply> {
            let home = crate::upload::sq(&self.0.to_string_lossy());
            crate::kablo::install::run_command(
                &[
                    "sh".into(),
                    "-c".into(),
                    format!("cd {home} && HOME={home} {script}"),
                ],
                input,
                Duration::from_secs(20),
            )
        }
    }

    /// The connection a way reaches the server by.
    struct Fixed(Served);

    impl stage::Reach for Fixed {
        fn run(&self, script: &str, wait: Duration) -> Result<RemoteCommand, String> {
            self.0.sh(script, Ask::Never, wait)
        }

        fn key(&self) -> String {
            "fixed".to_owned()
        }
    }

    /// An ssh that runs its last word (the command) here, in `home`, and does what the file
    /// `fake-mode` there says besides: `refuse` is a connection that goes at once, `tee` keeps
    /// what it is sent, `die-after:N` loses the connection (the process is killed, as a stream
    /// that has no exit status) after N bytes of the script's output (`dd` of a byte at a time
    /// writes what it has read at once, which `head` does not), `leave-after:N` after N bytes of
    /// what it is sent.
    fn fake_ssh(home: &Path) -> String {
        format!(
            r#"#!/bin/sh
for last; do :; done
HOME={home}
export HOME
cd "$HOME" || exit 1
mode=$(cat "$HOME/fake-mode" 2>/dev/null)
case $mode in
  refuse) kill -9 $$;;
  tee) tee "$HOME/stdin-log" | sh -c "$last"; exit $?;;
  die-after:*) sh -c "$last" | dd bs=1 count="${{mode#die-after:}}" 2>/dev/null; kill -9 $$;;
  leave-after:*) head -c "${{mode#leave-after:}}" | sh -c "$last"; kill -9 $$;;
  stall-after:*) sh -c "$last" | dd bs=1 count="${{mode#stall-after:}}" 2>/dev/null; sleep 60; exit 0;;
esac
exec sh -c "$last"
"#,
            home = crate::upload::sq(&home.to_string_lossy())
        )
    }

    /// Bytes that are not the same from one piece to the next.
    pub(super) fn bytes(len: usize) -> Vec<u8> {
        (0..len)
            .map(|at| u8::try_from((at * 31 + at / 251) % 256).unwrap_or(0))
            .collect()
    }

    /// What a program does with a pipe it was told of: opens it to read and reads to the end.
    pub(super) fn read_pipe(path: PathBuf) -> thread::JoinHandle<Vec<u8>> {
        thread::spawn(move || {
            let mut pipe = File::open(path).expect("the pipe opens");
            let mut got = Vec::new();
            pipe.read_to_end(&mut got).expect("the pipe reads");
            got
        })
    }

    #[test]
    fn a_file_is_poured_into_the_pipe_and_the_pipe_is_gone_when_the_session_hears_it_ended() {
        let rig = Rig::new("pour");
        let file = bytes(3 * 1024 * 1024 + 17);
        let handle = rig.give(&file);
        rig.files.stage(1, &handle, 0, PIPE);
        assert_eq!(rig.next(), Told::PipeReady(1));
        let program = read_pipe(rig.pipe());
        assert_eq!(rig.next(), Told::Staged(1, Ok(file.len() as u64)));
        assert!(
            program.join().expect("the program") == file,
            "the same bytes"
        );
        assert_eq!(rig.left_in_folder(), Vec::<String>::new());
        wait_until("the transfer to be over", || {
            rig.files.open_transfers() == 0
        });
        rig.nothing_more();
    }

    #[test]
    fn a_file_is_poured_from_the_offset_and_the_end_it_reports_is_the_files() {
        let rig = Rig::new("offset");
        let file = bytes(200_000);
        let handle = rig.give(&file);
        rig.files.stage(3, &handle, 150_000, PIPE);
        assert_eq!(rig.next(), Told::PipeReady(3));
        let program = read_pipe(rig.pipe());
        assert_eq!(rig.next(), Told::Staged(3, Ok(200_000)));
        assert!(program.join().expect("the program") == file[150_000..]);
        // From the end of the file: nothing to pour, and the end is where it was asked for.
        rig.files.stage(4, &handle, 200_000, PIPE);
        assert_eq!(rig.next(), Told::PipeReady(4));
        let program = read_pipe(rig.pipe());
        assert_eq!(rig.next(), Told::Staged(4, Ok(200_000)));
        assert!(program.join().expect("the program").is_empty());
    }

    #[test]
    fn a_stage_that_cannot_begin_is_answered_without_a_pipe() {
        let rig = Rig::new("refused");
        let handle = rig.give(b"0123456789");
        // Past the end of the file.
        rig.files.stage(1, &handle, 11, PIPE);
        assert_eq!(rig.next(), Told::Staged(1, Err(StageError::Size)));
        // A file that is not what it was when it was given.
        fs::write(rig.home.join("given.bin"), b"0123456789ABC").expect("a file");
        rig.files.stage(2, &handle, 0, PIPE);
        assert_eq!(rig.next(), Told::Staged(2, Err(StageError::Gone)));
        // A handle the table does not have.
        rig.files.stage(3, &"9".repeat(32), 0, PIPE);
        assert_eq!(rig.next(), Told::Staged(3, Err(StageError::Gone)));
        // A name that is not one the session gives is never part of a path.
        rig.files.stage(4, &handle, 0, "../escape");
        assert_eq!(rig.next(), Told::Staged(4, Err(StageError::Io)));
        assert_eq!(rig.left_in_folder(), Vec::<String>::new());
    }

    #[test]
    fn a_file_that_changes_while_it_is_poured_is_gone() {
        let rig = Rig::new("changes");
        let file = bytes(2 * 1024 * 1024);
        let handle = rig.give(&file);
        rig.files.stage(1, &handle, 0, PIPE);
        assert_eq!(rig.next(), Told::PipeReady(1));
        fs::OpenOptions::new()
            .write(true)
            .open(rig.home.join("given.bin"))
            .expect("the file")
            .set_modified(SystemTime::now() + Duration::from_secs(60))
            .expect("a new time");
        let program = read_pipe(rig.pipe());
        assert_eq!(rig.next(), Told::Staged(1, Err(StageError::Gone)));
        let _ = program.join();
    }

    #[test]
    fn a_program_that_closes_its_end_early_fails_the_transfer() {
        let rig = Rig::new("closes");
        let handle = rig.give(&bytes(4 * 1024 * 1024));
        rig.files.stage(1, &handle, 0, PIPE);
        assert_eq!(rig.next(), Told::PipeReady(1));
        let path = rig.pipe();
        let program = thread::spawn(move || {
            let mut pipe = File::open(path).expect("the pipe opens");
            let mut some = vec![0; 1000];
            pipe.read_exact(&mut some).expect("a little");
        });
        assert_eq!(rig.next(), Told::Staged(1, Err(StageError::Io)));
        program.join().expect("the program");
        assert_eq!(rig.left_in_folder(), Vec::<String>::new());
    }

    #[test]
    fn a_pipe_the_program_never_opens_is_waited_for_a_while_and_then_io() {
        let waits = Waits {
            opened_within: Duration::from_millis(150),
            ..Waits::default()
        };
        let rig = Rig::with_waits("never", waits);
        let handle = rig.give(&bytes(1000));
        rig.files.stage(1, &handle, 0, PIPE);
        assert_eq!(rig.next(), Told::PipeReady(1));
        assert!(
            rig.pipe().exists(),
            "the pipe is there while it is waited for"
        );
        assert_eq!(rig.next(), Told::Staged(1, Err(StageError::Io)));
        assert_eq!(rig.left_in_folder(), Vec::<String>::new());
    }

    #[test]
    fn a_stage_taken_back_ends_at_once_takes_its_pipe_and_tells_nothing() {
        let rig = Rig::new("unstage");
        let handle = rig.give(&bytes(1000));
        rig.files.stage(1, &handle, 0, PIPE);
        assert_eq!(rig.next(), Told::PipeReady(1));
        rig.files.unstage(1);
        assert!(!rig.pipe().exists(), "the pipe goes with the call");
        wait_until("the transfer to end", || rig.files.open_transfers() == 0);
        rig.nothing_more();
        // A transfer that is not out is nothing to take back.
        rig.files.unstage(9);
    }

    #[test]
    fn the_views_end_takes_the_transfers_with_it() {
        let rig = Rig::new("ends");
        let file = bytes(4 * 1024 * 1024);
        let handle = rig.give(&file);
        let (save, target) = rig.place("out.bin");
        rig.files.stage(1, &handle, 0, PIPE);
        assert_eq!(rig.next(), Told::PipeReady(1));
        let fetch_pipe = "5".repeat(32);
        rig.files.fetch(2, &save, 0, 10, &fetch_pipe);
        assert_eq!(rig.next(), Told::PipeReady(2));
        assert_eq!(rig.parts().len(), 1, "the part of the fetch is there");
        let Rig {
            files,
            folder,
            home,
            told,
            ..
        } = &rig;
        files.end_transfers();
        assert!(!folder.dir().join(PIPE).exists());
        assert!(!folder.dir().join(&fetch_pipe).exists());
        wait_until("the part to go", || {
            fs::read_dir(home)
                .expect("a listing")
                .flatten()
                .all(|entry| {
                    !entry
                        .file_name()
                        .to_string_lossy()
                        .ends_with(".pencere-part")
                })
        });
        wait_until("the transfers to end", || files.open_transfers() == 0);
        assert!(!target.exists());
        thread::sleep(Duration::from_millis(100));
        assert_eq!(told.try_recv().ok(), None);
    }

    #[test]
    fn dropping_the_files_takes_the_pipes_at_once() {
        let rig = Rig::new("drop");
        let handle = rig.give(&bytes(1000));
        rig.files.stage(1, &handle, 0, PIPE);
        assert_eq!(rig.next(), Told::PipeReady(1));
        let Rig { files, folder, .. } = rig;
        let pipe = folder.dir().join(PIPE);
        assert!(pipe.exists());
        drop(files);
        assert!(!pipe.exists());
    }

    #[test]
    fn a_pipe_the_program_fills_is_saved_whole_under_the_places_name() {
        let rig = Rig::new("fetch");
        let file = bytes(3 * 1024 * 1024 + 5);
        let (handle, target) = rig.place("out.bin");
        rig.files.fetch(1, &handle, 0, file.len() as u64, PIPE);
        assert_eq!(rig.next(), Told::PipeReady(1));
        let path = rig.pipe();
        let sent = file.clone();
        let program = thread::spawn(move || {
            let mut pipe = fs::OpenOptions::new()
                .write(true)
                .open(path)
                .expect("opens");
            pipe.write_all(&sent).expect("the bytes");
        });
        assert_eq!(rig.next(), Told::Fetched(1, Ok(file.len() as u64)));
        program.join().expect("the program");
        assert!(fs::read(&target).expect("the saved file") == file);
        assert_eq!(rig.parts(), Vec::<String>::new());
        assert_eq!(rig.left_in_folder(), Vec::<String>::new());
        assert_eq!(*lock(&rig.marked), [target], "marked as a saved file is");
        wait_until("the transfer to be over", || {
            rig.files.open_transfers() == 0
        });
    }

    /// Fills the pipe of a fetch with `bytes` and closes it.
    pub(super) fn fill(rig: &Rig, bytes: Vec<u8>) -> thread::JoinHandle<()> {
        let path = rig.pipe();
        thread::spawn(move || {
            let mut pipe = fs::OpenOptions::new()
                .write(true)
                .open(path)
                .expect("opens");
            let _ = pipe.write_all(&bytes);
        })
    }

    #[test]
    fn a_pipe_that_ends_short_or_holds_more_is_size_and_leaves_no_part_and_no_file() {
        let rig = Rig::new("size");
        let (handle, target) = rig.place("out.bin");
        for (transfer, sent) in [(1, 90), (2, 106)] {
            rig.files.fetch(transfer, &handle, 0, 100, PIPE);
            assert_eq!(rig.next(), Told::PipeReady(transfer));
            let program = fill(&rig, bytes(sent));
            assert_eq!(
                rig.next(),
                Told::Fetched(transfer, Err(FetchError::Size)),
                "{sent} bytes for 100"
            );
            program.join().expect("the program");
            assert!(!target.exists());
            assert_eq!(rig.parts(), Vec::<String>::new());
            assert_eq!(rig.left_in_folder(), Vec::<String>::new());
            wait_until("the transfer to be over", || {
                rig.files.open_transfers() == 0
            });
        }
    }

    #[test]
    fn a_pipe_nobody_writes_is_waited_for_a_while_and_then_io_with_no_part() {
        let waits = Waits {
            opened_within: Duration::from_millis(150),
            ..Waits::default()
        };
        let rig = Rig::with_waits("silent", waits);
        let (handle, _) = rig.place("out.bin");
        rig.files.fetch(1, &handle, 0, 100, PIPE);
        assert_eq!(rig.next(), Told::PipeReady(1));
        assert_eq!(rig.next(), Told::Fetched(1, Err(FetchError::Io)));
        assert_eq!(rig.parts(), Vec::<String>::new());
        assert_eq!(rig.left_in_folder(), Vec::<String>::new());
    }

    #[test]
    fn a_fetch_taken_back_leaves_no_part_and_no_pipe_and_tells_nothing() {
        let rig = Rig::new("fetch-back");
        let (handle, target) = rig.place("out.bin");
        rig.files.fetch(1, &handle, 0, 100, PIPE);
        assert_eq!(rig.next(), Told::PipeReady(1));
        assert_eq!(rig.parts().len(), 1);
        rig.files.unstage(1);
        wait_until("the part to go", || rig.parts().is_empty());
        assert!(!rig.pipe().exists());
        wait_until("the transfer to end", || rig.files.open_transfers() == 0);
        assert!(!target.exists());
        rig.nothing_more();
    }

    #[test]
    fn a_fetch_that_cannot_begin_is_answered_without_a_pipe() {
        let rig = Rig::new("fetch-refused");
        let (handle, _) = rig.place("out.bin");
        // Bytes after the first with no part of the earlier try to add them to.
        rig.files.fetch(1, &handle, 40, 100, PIPE);
        assert_eq!(rig.next(), Told::Fetched(1, Err(FetchError::Size)));
        // A place whose folder is gone.
        let lost = "3".repeat(32);
        rig.files
            .keep_save(&lost, rig.home.join("no-such-folder").join("out.bin"));
        rig.files.fetch(2, &lost, 0, 100, PIPE);
        assert_eq!(rig.next(), Told::Fetched(2, Err(FetchError::Io)));
        // A handle that names no place to save in.
        rig.files.fetch(3, &"8".repeat(32), 0, 100, PIPE);
        assert_eq!(rig.next(), Told::Fetched(3, Err(FetchError::Io)));
        assert_eq!(rig.left_in_folder(), Vec::<String>::new());
        assert_eq!(rig.parts(), Vec::<String>::new());
    }

    #[test]
    fn files_not_offered_a_folder_answer_io_and_make_no_pipe() {
        let (sender, told) = mpsc::channel();
        let files = ViewFiles::new(Arc::new(Tell(Mutex::new(sender))), Arc::new(|_: &Path| {}));
        files.stage(1, &"1".repeat(32), 0, PIPE);
        files.fetch(2, &"2".repeat(32), 0, 10, PIPE);
        assert_eq!(
            told.recv_timeout(Duration::from_secs(1)).ok(),
            Some(Told::Staged(1, Err(StageError::Io)))
        );
        assert_eq!(
            told.recv_timeout(Duration::from_secs(1)).ok(),
            Some(Told::Fetched(2, Err(FetchError::Io)))
        );
    }
}
