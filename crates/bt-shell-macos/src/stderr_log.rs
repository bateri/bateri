//! Where bateri's standard error goes when nothing reads it.
//!
//! Started from Finder, the Dock or `open`, the application's standard error is `/dev/null`:
//! every `bateri: …` line the code writes for a launch that did not settle, a session that could
//! not be saved or a tab that could not be restored is lost, and a launch that went wrong leaves
//! nothing to read afterwards. Such a process writes them to
//! `~/Library/Logs/bateri/<bundle id>.log` instead (Console finds it there), with one line that
//! says which launch the lines after it are from. A standard error that goes somewhere — a
//! terminal, a pipe, a file the caller chose (`make smoke`, a test) — is left as it is.
//!
//! The file is the launch's, appended to; once it is over [`LIMIT`] when a launch starts, it is
//! moved to `<name>.1` (the one there before goes) and a new one is begun, so two launches' worth
//! is always kept and the directory never grows past two files a bundle.

use std::fs::{self, OpenOptions};
use std::io::{self, Write as _};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{
    DirBuilderExt as _, FileTypeExt as _, MetadataExt as _, OpenOptionsExt as _,
};
use std::path::{Path, PathBuf};

/// How large the file may grow before a launch starts a new one. A launch writes a few lines;
/// this is many launches, and small enough to attach to a report.
const LIMIT: u64 = 1024 * 1024;

/// The file below the home directory that a bundle's lines go to.
pub(crate) fn path(home: &Path, bundle: &str) -> PathBuf {
    home.join("Library/Logs/bateri")
        .join(format!("{bundle}.log"))
}

/// Whether `fd` is the null device: a character device with `/dev/null`'s device number.
fn is_null(fd: i32) -> bool {
    // SAFETY: `stat` is plain data; all zeroes is a valid value of it.
    let mut stat: libc::stat = unsafe { std::mem::zeroed() };
    // SAFETY: `fstat` fills a `stat` this function owns; a bad descriptor is an error, not UB.
    if unsafe { libc::fstat(fd, &mut stat) } != 0 {
        return false;
    }
    let Ok(null) = fs::metadata("/dev/null") else {
        return false;
    };
    null.file_type().is_char_device()
        && (stat.st_mode & libc::S_IFMT) == libc::S_IFCHR
        && u64::try_from(stat.st_rdev).ok() == Some(null.rdev())
}

/// Moves a file that is over `limit` to `<name>.1`, so that the launch begins a new one.
fn rotate(file: &Path, limit: u64) {
    if fs::metadata(file).is_ok_and(|meta| meta.len() > limit) {
        let mut old = file.as_os_str().to_owned();
        old.push(".1");
        let _ = fs::rename(file, old);
    }
}

/// Opens the file for appending (the directory private, the file the user's alone), rotated
/// first, with the launch's line written.
fn open(file: &Path, header: &str) -> io::Result<fs::File> {
    if let Some(dir) = file.parent() {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)?;
    }
    rotate(file, LIMIT);
    let mut opened = OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(file)?;
    writeln!(opened, "{header}")?;
    Ok(opened)
}

/// Sends this process's standard error to the bundle's file when it goes to `/dev/null`; silent
/// when it cannot (no home, a directory that cannot be made): the lines then go where they went.
pub(crate) fn keep(home: Option<PathBuf>, bundle: Option<String>) {
    if !is_null(libc::STDERR_FILENO) {
        return;
    }
    let (Some(home), Some(bundle)) = (home, bundle) else {
        return;
    };
    let header = format!(
        "--- {bundle} {} pid {} started at {}",
        env!("CARGO_PKG_VERSION"),
        std::process::id(),
        now()
    );
    if let Ok(file) = open(&path(&home, &bundle), &header) {
        // SAFETY: both descriptors are open; `dup2` replaces descriptor 2 atomically, and the
        // file's own descriptor closes when `file` drops.
        unsafe { libc::dup2(file.as_raw_fd(), libc::STDERR_FILENO) };
    }
}

/// The local time as `2026-11-06 22:43:24`, for the launch's line.
fn now() -> String {
    // SAFETY: `tm` is plain data; all zeroes is a valid value of it.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    // SAFETY: `time` with a null pointer only returns the time.
    let seconds = unsafe { libc::time(std::ptr::null_mut()) };
    // SAFETY: `localtime_r` reads `seconds` and writes only into `tm`, both this function's.
    if unsafe { libc::localtime_r(&seconds, &mut tm) }.is_null() {
        return seconds.to_string();
    }
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        tm.tm_year + 1900,
        tm.tm_mon + 1,
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min,
        tm.tm_sec
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("bateri-stderr-log-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("a scratch directory");
        dir
    }

    #[test]
    fn the_null_device_is_told_from_a_file_and_a_pipe() {
        let null = fs::File::open("/dev/null").expect("/dev/null");
        assert!(is_null(null.as_raw_fd()));
        let dir = scratch("null");
        let file = fs::File::create(dir.join("f")).expect("a file");
        assert!(!is_null(file.as_raw_fd()));
        let (reader, _writer) = std::io::pipe().expect("a pipe");
        assert!(!is_null(reader.as_raw_fd()));
        assert!(!is_null(-1));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_bundles_lines_go_below_the_logs_directory() {
        assert_eq!(
            path(Path::new("/Users/u"), "dev.bateri.bateri"),
            Path::new("/Users/u/Library/Logs/bateri/dev.bateri.bateri.log")
        );
    }

    #[test]
    fn a_launch_appends_its_line_and_a_full_file_is_moved_aside_first() {
        let dir = scratch("rotate");
        let file = dir.join("Logs/b.log");
        drop(open(&file, "--- one").expect("opened"));
        drop(open(&file, "--- two").expect("opened"));
        assert_eq!(
            fs::read_to_string(&file).expect("read"),
            "--- one\n--- two\n"
        );
        assert_eq!(fs::metadata(&file).expect("meta").mode() & 0o777, 0o600);
        assert_eq!(
            fs::metadata(dir.join("Logs")).expect("meta").mode() & 0o777,
            0o700
        );
        // Over the limit: the launch moves it to `.1` and begins a new one.
        fs::write(&file, vec![b'x'; usize::try_from(LIMIT).expect("fits") + 1]).expect("filled");
        drop(open(&file, "--- three").expect("opened"));
        assert_eq!(fs::read_to_string(&file).expect("read"), "--- three\n");
        assert_eq!(
            fs::metadata(dir.join("Logs/b.log.1")).expect("moved").len(),
            LIMIT + 1
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn the_launchs_time_reads_as_a_date_and_a_time() {
        let now = now();
        assert_eq!(now.len(), 19, "{now}");
        assert_eq!(&now[4..5], "-");
        assert_eq!(&now[13..14], ":");
    }
}
