//! An image pasted into a pane a server keeps (⌘V with an image and no text on the clipboard): a
//! terminal's paste is text, and a program on the server reads the server's clipboard, which
//! holds nothing of this Mac's. So the image goes to the server as a PNG, into a folder of
//! bateri's own ([`ROOT`], never the project's directory), and the pane pastes its path, as a
//! drop pastes a path. A coding agent that takes an image path (Claude Code's `[Image #1]`) takes
//! it as it takes a file dropped on it; a shell has the path on its line and the file is there.
//!
//! The bytes cross over the one connection kept to the server ([`Served`]), as `cat` into a file
//! of the folder, under a name the server's clock gives. No sheet asks first: a screenshot is
//! small, and the paste is what the person asked for. The same run sweeps the folder of what
//! earlier pastes left there more than [`KEEP_DAYS`] ago; nothing outside the folder is touched.
//!
//! Pure and process halves: the script ([`script`]) and what its answer is read as
//! ([`landed`]) are the tested half; [`put`] runs it, on a thread of its own, and [`row`] is
//! the dock's line for an upload that is slow ([`SLOW`]).

use std::io::Read;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread;
use std::time::Duration;

use bt_core::{HostMark, Transfer};
use kablo_proto::v1::Exit;

use crate::kablo::remote::RemoteCommand;
use crate::kablo::served::Served;
use crate::ssh_route::Ask;
use crate::upload::{OPEN_WAIT, format_pair, is_safe};

/// The folder the images go to, relative to the server account's home directory.
pub const ROOT: &str = ".cache/bateri/paste";

/// How old a pasted image may grow before the next paste removes it — a **design constant**:
/// long enough for a conversation with an agent to come back to it, short enough that a folder
/// nobody looks into does not grow without end.
pub const KEEP_DAYS: u32 = 7;

/// How long the upload runs before the dock shows its line — a **design constant**: a screenshot
/// usually crosses before the eye could read a line, and a line that flashes is noise; past this
/// the person is left wondering where the paste went.
pub const SLOW: Duration = Duration::from_secs(1);

/// How long the connection may take none of the image before the paste gives up.
const SILENT: Duration = Duration::from_secs(30);

/// How often a write the connection's window holds back looks again.
const LOOK_AGAIN: Duration = Duration::from_millis(20);

/// How long the server may take to answer once the whole image is handed to the connection,
/// beside the time the bytes still in flight take to cross ([`SLOWEST`]).
const ANSWER: Duration = Duration::from_secs(30);

/// The slowest link a paste waits for, in bytes a second: the last of the image is handed to the
/// connection long before the server has it (the connection's window holds it), so the wait for
/// the answer grows with the image's size at this rate, and a good upload on a slow link is not
/// ended at its end.
const SLOWEST: u64 = 16 * 1024;

/// The most of what the script says that is kept: a path, or a line of why not.
const SAID: u64 = 4 * 1024;

/// The exit codes of the script that say what went wrong.
const EXIT_INCOMPLETE: i32 = 8;
const EXIT_UNWRITABLE: i32 = 9;

/// The script that takes the image on its standard input, `size` bytes of it: the folder is made
/// (closed to other users: a screenshot may hold anything), images older than [`KEEP_DAYS`] are
/// removed from it, the image is written whole beside its name and then given it, and the path
/// is said. The name is the server's local time; a second paste in the same second takes `-2`.
pub(crate) fn script(size: usize) -> String {
    let older = KEEP_DAYS.saturating_sub(1);
    format!(
        "umask 077; d=\"$HOME/{ROOT}\"; mkdir -p \"$d\" || exit {EXIT_UNWRITABLE}; \
         find \"$d/.\" ! -name . -prune -type f -mtime +{older} -exec rm -f {{}} \\; 2>/dev/null; \
         n=$(date +%Y-%m-%d-%H%M%S); f=\"$d/$n.png\"; i=1; \
         while [ -e \"$f\" ]; do i=$((i+1)); f=\"$d/$n-$i.png\"; done; \
         t=\"$d/.$n-$$.part\"; \
         cat >\"$t\" || {{ rm -f \"$t\"; exit {EXIT_UNWRITABLE}; }}; \
         [ \"$(wc -c <\"$t\" | tr -d ' ')\" = {size} ] || {{ rm -f \"$t\"; exit {EXIT_INCOMPLETE}; }}; \
         mv \"$t\" \"$f\" || {{ rm -f \"$t\"; exit {EXIT_UNWRITABLE}; }}; \
         printf '%s\\n' \"$f\""
    )
}

/// The path the script said, if it is one the pane may paste: absolute, in the folder, a PNG,
/// and with nothing a line could not carry (a backslash or a control character).
pub(crate) fn landed(out: &str) -> Option<String> {
    let path = out.lines().rev().find(|line| !line.is_empty())?;
    let name = path.rsplit_once(&format!("/{ROOT}/"))?.1;
    (path.starts_with('/')
        && !name.is_empty()
        && !name.contains('/')
        && !name.starts_with('.')
        && name.ends_with(".png")
        && is_safe(path))
    .then(|| path.to_owned())
}

/// How far an upload has gone, for the main thread's line: written by the thread that sends.
#[derive(Debug, Default)]
pub struct Sending {
    sent: AtomicU64,
    done: AtomicBool,
}

impl Sending {
    /// The bytes handed to the connection so far.
    pub fn sent(&self) -> u64 {
        self.sent.load(Ordering::Acquire)
    }

    /// Whether the upload is over, either way.
    pub fn is_done(&self) -> bool {
        self.done.load(Ordering::Acquire)
    }
}

/// Writes `png` into the folder on the server `served` reaches, and says the path it got. `Err`
/// is a line for the person. Waits for the server: run on a thread of its own. The connection is
/// the one the pane's terminal rides, so no login is asked for.
pub fn put(served: &Served, png: &[u8], sending: &Sending) -> Result<String, String> {
    let result = send(served, png, sending);
    sending.done.store(true, Ordering::Release);
    result
}

fn send(served: &Served, png: &[u8], sending: &Sending) -> Result<String, String> {
    let RemoteCommand {
        stdin,
        stdout,
        stderr,
        end,
    } = served.sh(&script(png.len()), Ask::Never, OPEN_WAIT)?;
    let kept = |from: Box<dyn Read + Send>| {
        thread::spawn(move || {
            let mut kept = Vec::new();
            let mut from = from;
            let _ = from.by_ref().take(SAID).read_to_end(&mut kept);
            let _ = std::io::copy(&mut from, &mut std::io::sink());
            String::from_utf8_lossy(&kept).into_owned()
        })
    };
    let out = kept(Box::new(stdout));
    let err = kept(Box::new(stderr));
    let mut stalled = false;
    if let Some(mut stdin) = stdin {
        let never = AtomicBool::new(false);
        for chunk in png.chunks(64 * 1024) {
            match stdin.write_all_within(chunk, SILENT, LOOK_AGAIN, &never) {
                Ok(_) => {
                    sending.sent.fetch_add(chunk.len() as u64, Ordering::AcqRel);
                }
                // The script stopped reading (it could not write the folder): what it says is
                // the reason, below.
                Err(error) => {
                    stalled = error.kind() == std::io::ErrorKind::TimedOut;
                    break;
                }
            }
        }
    }
    if stalled {
        end.kill();
    }
    let exit = end.wait_within(ANSWER + Duration::from_secs(png.len() as u64 / SLOWEST));
    let (out, err) = (
        out.join().unwrap_or_default(),
        err.join().unwrap_or_default(),
    );
    let why = || {
        err.lines()
            .rev()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("no reason was given")
            .to_owned()
    };
    match exit {
        _ if stalled => Err(format!(
            "The connection took nothing for {} seconds.",
            SILENT.as_secs()
        )),
        Ok(Exit::Code(0)) => {
            landed(&out).ok_or_else(|| "The server did not say where it put the image.".to_owned())
        }
        Ok(Exit::Code(EXIT_UNWRITABLE)) => Err(format!(
            "~/{ROOT} can't be written on the server: {}",
            why()
        )),
        Ok(Exit::Code(EXIT_INCOMPLETE)) => Err("The image did not arrive whole.".to_owned()),
        Ok(_) => Err(format!("Writing the image failed: {}", why())),
        Err(text) => Err(text),
    }
}

/// The dock's line while an upload that is slow runs: the arrow, what goes, how much of it has,
/// and the bar. No buttons: the paste is one small file, and ⌘. is the queue's.
pub fn row(host: &str, mark: HostMark, sent: u64, total: u64) -> Transfer {
    let shown = sent.min(total);
    let progress = if total == 0 {
        0
    } else {
        // audit: clamped to `shown ≤ total`; ratio in ten-thousandths, fits u16.
        (shown as f64 / total as f64 * 10_000.0) as u16
    };
    Transfer {
        host: host.to_owned(),
        mark,
        body: format!("↑ Pasted image  {}", format_pair(shown, total)),
        progress: Some(progress),
        ..Transfer::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::TempRoot;
    use std::fs;
    use std::os::unix::fs::PermissionsExt as _;
    use std::path::{Path, PathBuf};

    fn server(label: &str) -> (TempRoot, PathBuf, Served) {
        let root = TempRoot::new(label);
        let home = root.0.join("home");
        fs::create_dir_all(&home).expect("a home");
        let served = Served::at_home(&home, Path::new("/bin/sh"));
        (root, home, served)
    }

    fn paste(served: &Served, png: &[u8]) -> Result<String, String> {
        let sending = Sending::default();
        let result = put(served, png, &sending);
        assert!(sending.is_done(), "the line hears the upload is over");
        if result.is_ok() {
            assert_eq!(sending.sent(), png.len() as u64);
        }
        result
    }

    #[test]
    fn the_image_lands_whole_in_the_folder_and_its_absolute_path_is_said() {
        let (_root, home, served) = server("paste-image-whole");
        let png = b"\x89PNG\r\n\x1a\n not really an image".repeat(5000);
        let path = paste(&served, &png).expect("pasted");
        let folder = home.join(ROOT);
        assert!(
            path.starts_with(&format!("{}/", folder.display())),
            "{path}"
        );
        assert!(path.ends_with(".png"), "{path}");
        assert_eq!(fs::read(&path).expect("the file"), png);
        // A screenshot may hold anything: the folder is the account's alone.
        let mode = fs::metadata(&folder).expect("folder").permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);
        // Nothing half-written is left beside it.
        let names: Vec<_> = fs::read_dir(&folder)
            .expect("listing")
            .map(|entry| entry.expect("entry").file_name())
            .collect();
        assert_eq!(names.len(), 1, "{names:?}");
    }

    #[test]
    fn a_second_paste_in_the_same_second_takes_a_name_of_its_own() {
        let (_root, _home, served) = server("paste-image-twice");
        let first = paste(&served, b"one").expect("first");
        let second = paste(&served, b"two").expect("second");
        assert_ne!(first, second);
        assert_eq!(fs::read(&first).expect("first file"), b"one");
        assert_eq!(fs::read(&second).expect("second file"), b"two");
    }

    #[test]
    fn old_pastes_are_swept_and_young_ones_and_everything_outside_the_folder_stay() {
        let (_root, home, served) = server("paste-image-sweep");
        let folder = home.join(ROOT);
        fs::create_dir_all(folder.join("inner")).expect("folders");
        let old = folder.join("2000-01-01-000000.png");
        let young = folder.join("young.png");
        let nested = folder.join("inner/2000-01-01-000000.png");
        let outside = home.join(".cache/bateri/2000-01-01-000000.png");
        for file in [&old, &young, &nested, &outside] {
            fs::write(file, b"x").expect("a file");
        }
        for file in [&old, &nested, &outside] {
            let status = std::process::Command::new("touch")
                .args(["-t", "200001010000"])
                .arg(file)
                .status()
                .expect("touch");
            assert!(status.success());
        }
        paste(&served, b"new").expect("pasted");
        assert!(!old.exists(), "older than {KEEP_DAYS} days");
        assert!(young.exists());
        assert!(nested.exists(), "only the folder's own files are swept");
        assert!(outside.exists(), "nothing outside the folder is touched");
    }

    #[test]
    fn a_folder_that_cannot_be_made_says_so() {
        let (_root, home, served) = server("paste-image-unwritable");
        // `.cache` is a file: the folder cannot be made below it.
        fs::write(home.join(".cache"), b"").expect("a file in the way");
        let error = paste(&served, b"image").expect_err("refused");
        assert!(error.contains("can't be written"), "{error}");
    }

    #[test]
    fn a_server_that_refuses_says_why() {
        let served = Served::refusing("the server is gone");
        let sending = Sending::default();
        let error = put(&served, b"image", &sending).expect_err("refused");
        assert_eq!(error, "the server is gone");
        assert!(sending.is_done());
    }

    #[test]
    fn only_a_png_path_in_the_folder_is_pasted() {
        let ok = format!("/home/me/{ROOT}/2026-11-19-143012.png");
        assert_eq!(landed(&format!("{ok}\n")), Some(ok.clone()));
        assert_eq!(landed(&format!("noise\n{ok}\n\n")), Some(ok));
        for out in [
            String::new(),
            format!("~/{ROOT}/a.png"),
            format!("/home/me/{ROOT}/a.txt"),
            format!("/home/me/{ROOT}/.a.png.part"),
            format!("/home/me/{ROOT}/sub/a.png"),
            format!("/home/me/{ROOT}/a\u{1b}[2J.png"),
            format!("/home/me/{ROOT}/a\\b.png"),
            "/home/me/elsewhere/a.png".to_owned(),
        ] {
            assert_eq!(landed(&out), None, "{out:?}");
        }
    }

    #[test]
    fn the_slow_line_shows_how_far_the_image_has_gone_and_has_no_buttons() {
        let line = row("prod", HostMark::None, 512 * 1024, 2 * 1024 * 1024);
        assert!(line.body.starts_with("↑ Pasted image  "), "{}", line.body);
        assert_eq!(line.progress, Some(2500));
        assert_eq!(line.controls.items, 0);
        assert_eq!(row("prod", HostMark::None, 9, 0).progress, Some(0));
        assert_eq!(row("prod", HostMark::None, 9, 3).progress, Some(10_000));
    }
}
