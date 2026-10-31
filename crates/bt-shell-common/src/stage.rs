//! The folder bateri keeps on the machine a program runs on, for the bytes of a file to cross
//! through (specification §8.11 of the program-view protocol): a directory under the account's
//! home, one for each instance of bateri, closed to every other user (mode `0700`), with a
//! `marker` file that holds the token the `welcome` names. A program uses the folder only when it
//! proves the folder is bateri's (a directory of its own user, a marker of the token), and then
//! the bytes of a file cross as a pipe in it fills and empties instead of as messages in the
//! terminal's stream.
//!
//! This module is the folder and the little the transfers need of the system (a pipe made and
//! waited on); the transfers themselves are [`crate::native_view::ViewFiles`]'s. **bateri touches
//! nothing else on the program's machine**: the folder, its marker and its pipes, and what it
//! made in an earlier run of its own instance.
//!
//! Two places make one: this Mac's ([`prepare_here`], once, as the application opens) and a
//! server's (a fixed script, run through the account's main connection; it lives with the other
//! scripts bateri runs on servers, `kablo::install`).

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use bt_core::pencere::{StageDir, system_random};

/// Where the folders are, under the home directory: one for each instance of bateri.
pub const ROOT: &str = ".cache/pencere-stage";

/// The file in a folder that holds its token.
pub const MARKER: &str = "marker";

/// How long a folder of an instance that is not this one may go untouched before it is taken for
/// the folder of an instance that is gone and removed. A folder is touched whenever a pipe is made
/// or removed in it, and when the instance that keeps it makes it anew.
pub const STALE_AFTER: Duration = Duration::from_secs(30 * 24 * 60 * 60);

/// A folder bateri made on a machine, as it is offered to the programs there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Folder {
    dir: PathBuf,
    token: String,
}

impl Folder {
    /// The folder at `dir` (an absolute path), whose marker holds `token` (32 lowercase hex
    /// digits); `None` where the protocol would not offer it.
    #[must_use]
    pub fn new(dir: PathBuf, token: String) -> Option<Self> {
        StageDir::new(&dir.to_string_lossy(), &token)?;
        Some(Self { dir, token })
    }

    /// The folder's path.
    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// What the marker holds.
    #[must_use]
    pub fn token(&self) -> &str {
        &self.token
    }

    /// The offer a program's `welcome` carries.
    #[must_use]
    pub fn offer(&self) -> Option<StageDir> {
        StageDir::new(&self.dir.to_string_lossy(), &self.token)
    }
}

/// How the bytes of a transfer reach the program's machine, and so where a pipe is made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Way {
    /// The program runs on this Mac: the pipe is a file of the folder here.
    Here(PathBuf),
}

/// What a pane offers its programs: the folder as the `welcome` names it, and the way of getting
/// to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Offered {
    /// The folder, as the `welcome` offers it.
    pub dir: StageDir,
    /// How a transfer reaches it.
    pub way: Way,
}

impl Offered {
    /// The folder of this Mac, as it is offered to a program that runs here.
    #[must_use]
    pub fn here(folder: &Folder) -> Option<Self> {
        Some(Self {
            dir: folder.offer()?,
            way: Way::Here(folder.dir().to_owned()),
        })
    }
}

/// The folder of the instance `instance` of bateri under `home`, made anew: whatever an earlier
/// run of the same instance left in it (the pipes and the parts of a process that was killed) is
/// removed, and a marker with a token of its own is written, under a name of its own and moved
/// into place, so that no program ever reads half a token.
///
/// The folders above it are made if they are not there: `~/.cache` is whatever the user's umask
/// makes it, the folder of all the instances and the instance's own are `0700`.
///
/// # Errors
///
/// A folder cannot be made or is not the user's own, or the system gave no randomness.
pub fn prepare_here(home: &Path, instance: &str) -> io::Result<Folder> {
    if !is_instance(instance) {
        return Err(invalid("not the name of an instance"));
    }
    let root = home.join(ROOT);
    if let Some(parent) = root.parent() {
        fs::create_dir_all(parent)?;
    }
    make_private_dir(&root)?;
    let dir = root.join(instance);
    remove_any(&dir)?;
    fs::DirBuilder::new().mode(0o700).create(&dir)?;
    // The path a program opens is the one with no link in it: it opens the folder itself without
    // following a link, and a home reached through one is common.
    let dir = fs::canonicalize(&dir)?;
    let token = hex(&system_random().ok_or_else(|| io::Error::other("no randomness"))?);
    let new = dir.join(".marker-new");
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&new)?;
    file.write_all(token.as_bytes())?;
    file.sync_all()?;
    drop(file);
    fs::rename(&new, dir.join(MARKER))?;
    Folder::new(dir, token).ok_or_else(|| invalid("not a folder that can be offered"))
}

/// Removes the folders of the instances other than `keep` that nothing has touched for `stale`
/// before `now`: the instances that were never closed properly (a crash, a power cut) leave one
/// each. Only a directory of the user's own is removed, and a link is never followed. Returns how
/// many went.
pub fn sweep(home: &Path, keep: &str, now: SystemTime, stale: Duration) -> usize {
    let Ok(entries) = fs::read_dir(home.join(ROOT)) else {
        return 0;
    };
    let me = euid();
    let mut removed = 0;
    for entry in entries.flatten() {
        if entry.file_name().to_str() == Some(keep) {
            continue;
        }
        let path = entry.path();
        let Ok(meta) = fs::symlink_metadata(&path) else {
            continue;
        };
        let untouched = meta
            .modified()
            .ok()
            .and_then(|touched| now.duration_since(touched).ok())
            .is_some_and(|age| age >= stale);
        if meta.is_dir() && meta.uid() == me && untouched && fs::remove_dir_all(&path).is_ok() {
            removed += 1;
        }
    }
    removed
}

/// Whether `name` can name an instance of bateri in a path: what `ssh_route::instance_for` makes
/// (a few hex digits), and no more than that — the name is part of a path.
#[must_use]
pub fn is_instance(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 32
        && name
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Whether `name` is a name the session gives a pipe: 32 lowercase hex digits. A pipe's path is
/// made of the folder and this, and of nothing a person or a program wrote.
#[must_use]
pub fn is_pipe_name(name: &str) -> bool {
    name.len() == 32
        && name
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Lowercase hexadecimal digits of `bytes`.
#[must_use]
pub fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut text, byte| {
        let _ = write!(text, "{byte:02x}");
        text
    })
}

/// A directory `path` of the user's own, closed to everyone else: made if it is not there, and
/// made so if it is.
fn make_private_dir(path: &Path) -> io::Result<()> {
    match fs::DirBuilder::new().mode(0o700).create(path) {
        Ok(()) => return Ok(()),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error),
    }
    let meta = fs::symlink_metadata(path)?;
    if !meta.is_dir() || meta.uid() != euid() {
        return Err(invalid("not a directory of the user's own"));
    }
    if meta.mode() & 0o077 != 0 {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// Removes whatever is at `path`, a link as the link it is.
fn remove_any(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() => fs::remove_dir_all(path),
        Ok(_) => fs::remove_file(path),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn invalid(what: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, what)
}

/// The effective user of this process.
fn euid() -> u32 {
    // SAFETY: `geteuid` takes nothing and cannot fail.
    unsafe { libc::geteuid() }
}

/// A pipe (FIFO) made at `path`, for its user alone.
///
/// # Errors
///
/// The system's: the path exists, the folder is not there or is not writable.
pub fn make_fifo(path: &Path) -> io::Result<()> {
    let name = std::ffi::CString::new(path.as_os_str().as_bytes())
        .map_err(|_| invalid("a NUL in a path"))?;
    // SAFETY: a NUL-terminated path that lives for the call.
    if unsafe { libc::mkfifo(name.as_ptr(), 0o600) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// The pipe at `path`, opened for reading or for writing without waiting for the other end, and
/// without following a link. For writing it fails with `ENXIO` while nothing has the pipe open to
/// read; for reading it opens at once, and reads nothing (end of file) until something writes.
///
/// # Errors
///
/// The system's.
pub fn open_pipe(path: &Path, reading: bool) -> io::Result<File> {
    OpenOptions::new()
        .read(reading)
        .write(!reading)
        .custom_flags(libc::O_NONBLOCK | libc::O_NOFOLLOW)
        .open(path)
}

/// Waits up to `millis` for `file` to be ready to read (`reading`) or to write.
#[must_use]
pub fn ready(file: &File, reading: bool, millis: i32) -> bool {
    let fd: RawFd = file.as_raw_fd();
    let mut poll = libc::pollfd {
        fd,
        events: if reading { libc::POLLIN } else { libc::POLLOUT },
        revents: 0,
    };
    // SAFETY: one valid `pollfd` that lives for the call, and a count of one.
    unsafe { libc::poll(&raw mut poll, 1, millis) > 0 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    /// A home of its own for a test, removed with it.
    struct Home(PathBuf);

    impl Home {
        fn new(name: &str) -> Self {
            let path =
                std::env::temp_dir().join(format!("bateri-stage-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).expect("a home");
            Self(path)
        }
    }

    impl Drop for Home {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn a_folder_is_private_and_its_marker_holds_the_token_the_welcome_names() {
        let home = Home::new("private");
        let folder = prepare_here(&home.0, "c148d55a").expect("a folder");
        let meta = fs::symlink_metadata(folder.dir()).expect("the folder");
        assert!(meta.is_dir());
        assert_eq!(meta.permissions().mode() & 0o777, 0o700);
        assert_eq!(meta.uid(), euid());
        assert!(folder.dir().is_absolute());
        assert!(folder.dir().ends_with("pencere-stage/c148d55a"));
        let root = fs::metadata(home.0.join(ROOT)).expect("the root");
        assert_eq!(root.permissions().mode() & 0o777, 0o700);
        let marker = fs::read_to_string(folder.dir().join(MARKER)).expect("a marker");
        assert_eq!(marker, folder.token());
        assert_eq!(marker.len(), 32);
        assert_eq!(
            fs::metadata(folder.dir().join(MARKER))
                .expect("the marker")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        // Nothing else is in it, not even the name the marker was written under.
        let names: Vec<_> = fs::read_dir(folder.dir())
            .expect("a listing")
            .flatten()
            .map(|entry| entry.file_name())
            .collect();
        assert_eq!(names, [std::ffi::OsString::from(MARKER)]);
        let offer = folder.offer().expect("an offer");
        assert_eq!(offer.token(), folder.token());
        assert_eq!(Path::new(offer.dir()), folder.dir());
    }

    #[test]
    fn making_the_folder_again_removes_what_the_run_before_left_and_names_a_new_token() {
        let home = Home::new("again");
        let first = prepare_here(&home.0, "c148d55a").expect("a folder");
        make_fifo(&first.dir().join("0b5d7c1e9a3f4682d0e1c7a95b3f8d24")).expect("a pipe");
        fs::write(first.dir().join(".leftover.pencere-part"), b"x").expect("a part");
        let second = prepare_here(&home.0, "c148d55a").expect("a folder");
        assert_eq!(first.dir(), second.dir());
        assert_ne!(first.token(), second.token());
        let names: Vec<_> = fs::read_dir(second.dir())
            .expect("a listing")
            .flatten()
            .map(|entry| entry.file_name())
            .collect();
        assert_eq!(names, [std::ffi::OsString::from(MARKER)]);
    }

    #[test]
    fn a_link_put_where_the_folder_goes_is_removed_and_never_written_through() {
        let home = Home::new("link");
        let elsewhere = home.0.join("elsewhere");
        fs::create_dir_all(&elsewhere).expect("a directory");
        fs::create_dir_all(home.0.join(ROOT)).expect("the root");
        std::os::unix::fs::symlink(&elsewhere, home.0.join(ROOT).join("c148d55a")).expect("a link");
        let folder = prepare_here(&home.0, "c148d55a").expect("a folder");
        assert!(
            fs::read_dir(&elsewhere)
                .expect("a listing")
                .next()
                .is_none(),
            "nothing was written where the link went"
        );
        assert!(folder.dir().join(MARKER).is_file());
    }

    #[test]
    fn a_name_that_is_not_an_instances_is_refused_before_it_is_part_of_a_path() {
        let home = Home::new("name");
        for name in ["", "../x", "a/b", "C148D55A", "zz", &"a".repeat(33)] {
            assert!(prepare_here(&home.0, name).is_err(), "{name:?}");
        }
        assert!(is_instance("1d9697d4"));
        assert!(is_pipe_name("0b5d7c1e9a3f4682d0e1c7a95b3f8d24"));
        assert!(!is_pipe_name("0b5d7c1e9a3f4682d0e1c7a95b3f8d2"));
        assert!(!is_pipe_name("../5d7c1e9a3f4682d0e1c7a95b3f8d24"));
    }

    #[test]
    fn the_folders_of_other_instances_go_by_age_alone() {
        let home = Home::new("sweep");
        let mine = prepare_here(&home.0, "aaaaaaaa").expect("a folder");
        let old = prepare_here(&home.0, "bbbbbbbb").expect("a folder");
        let young = prepare_here(&home.0, "cccccccc").expect("a folder");
        let now = SystemTime::now();
        let long_ago = now - STALE_AFTER - Duration::from_secs(60);
        File::open(old.dir())
            .and_then(|dir| dir.set_modified(long_ago))
            .expect("an old folder");
        File::open(mine.dir())
            .and_then(|dir| dir.set_modified(long_ago))
            .expect("an old folder");
        // A stray file is not an instance's folder and is left alone.
        fs::write(home.0.join(ROOT).join("stray"), b"x").expect("a file");
        assert_eq!(sweep(&home.0, "aaaaaaaa", now, STALE_AFTER), 1);
        assert!(mine.dir().is_dir(), "the instance's own is not swept");
        assert!(!old.dir().exists(), "an old one of another instance goes");
        assert!(young.dir().is_dir(), "a recent one stays");
        assert!(home.0.join(ROOT).join("stray").is_file());
        assert_eq!(sweep(&home.0, "aaaaaaaa", now, STALE_AFTER), 0);
    }

    #[test]
    fn a_pipe_is_made_for_its_user_alone_and_opens_without_waiting() {
        let home = Home::new("pipe");
        let folder = prepare_here(&home.0, "c148d55a").expect("a folder");
        let path = folder.dir().join("0b5d7c1e9a3f4682d0e1c7a95b3f8d24");
        make_fifo(&path).expect("a pipe");
        assert!(make_fifo(&path).is_err(), "made once");
        let meta = fs::symlink_metadata(&path).expect("the pipe");
        assert_eq!(meta.permissions().mode() & 0o777, 0o600);
        assert!(std::os::unix::fs::FileTypeExt::is_fifo(&meta.file_type()));
        let error = open_pipe(&path, false).expect_err("no one reads it yet");
        assert_eq!(error.raw_os_error(), Some(libc::ENXIO));
        let reading = open_pipe(&path, true).expect("the reading end opens at once");
        let mut writing = open_pipe(&path, false).expect("and then the writing end");
        assert!(ready(&writing, false, 100));
        assert!(!ready(&reading, true, 10), "nothing is written yet");
        writing.write_all(b"abc").expect("bytes");
        assert!(ready(&reading, true, 100));
    }
}
