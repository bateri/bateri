//! The transfers of a program that runs on a server: the pipe is made there, by a script that
//! runs through the account's main connection ([`crate::kablo::install::pipe_script`]), and the
//! bytes ride that connection: a person's file is written to the script's standard input and the
//! script pours it into the pipe, a program's file is read from the script's standard output
//! after the two words the script says first.
//!
//! The script says **ready** when the pipe is there and **open** when the program has opened its
//! end. Nothing is sent before the second: a program that never opens the pipe (a container, a
//! mount that does not share it) must not have a file's first megabytes in the connection's
//! buffers counted as sent. The script ends when the transfer does, and takes the pipe with it.
//!
//! **What the connection's end means.** The ssh that ran the script is the one who knows what
//! became of the transfer: its exit status is the script's (`cat`'s: 0 for a file that went
//! through, the signal's 128 and more for a program that closed its end early), and 255 or no
//! status at all is the connection that went. The second is `route`, the one answer that says
//! "ask again from where you are": a stage starts again at the program's offset, and a fetch's
//! part is kept for the offset it ended at.

use std::fs::File;
use std::io::{self, Read};
use std::os::fd::OwnedFd;
use std::os::unix::fs::FileExt;
use std::process::{Child, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use bt_core::pencere::{FetchError, StageError};

use super::super::files::Part;
use super::{Ground, PIECE, POLL_MS, Waits, fetch_error, lock, open_source, pour, same, take_part};
use crate::kablo::install::{self, Cross, PIPE_OPEN, PIPE_READY};
use crate::stage::{self, Server};

/// The most that the tail of a stage is waited for: the last of the file is in the connection's
/// buffers when the pouring is done, and the script ends when the program has taken it, at the
/// program's pace. A program that stops taking it gives the transfer up itself, and the transfer
/// is then taken back; this is only a bound for a program that does neither.
const TAIL: Duration = Duration::from_secs(600);

/// How a transfer on a server ended when the script did not end it well.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Lost {
    /// The connection went: ask again.
    Route,
    /// The script failed, or the program closed its end.
    Io,
}

/// What a status of ssh says of a transfer that did not end well. 255 is ssh's own failure (the
/// connection, the login); no status is a process that was ended, which is a connection that was.
fn lost(code: Option<i32>) -> Lost {
    match code {
        Some(255) | None => Lost::Route,
        Some(_) => Lost::Io,
    }
}

impl Lost {
    /// How a stage that ended so is told to the session.
    fn stage(self) -> StageError {
        match self {
            Self::Route => StageError::Route,
            Self::Io => StageError::Io,
        }
    }
}

/// How long the script waits for the program to open its end: the Mac's own wait and room to
/// spare, so that it is the Mac that gives up first, and says what it saw (the program never came:
/// `io`), and not the script, whose ending the Mac reads as a connection that went.
fn script_waits(waits: Waits) -> u32 {
    u32::try_from(waits.opened_within.as_secs())
        .unwrap_or(u32::MAX)
        .saturating_add(5)
}

/// What waiting for a word of the script came to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Wait {
    /// The word came.
    Said,
    /// The transfer was taken back.
    Cancelled,
    /// The word did not come in time.
    Late,
    /// The script's output ended: ssh is going or gone.
    Ended,
}

/// The ssh that runs a pipe's script, the file its output is read from and the one its input is
/// written to. It is ended, with the processes it started, when this is dropped.
struct Remote {
    child: Child,
    /// Whether the child was waited for: a process that was is not signalled any more, its number
    /// is another process's.
    reaped: bool,
    out: File,
    input: Option<File>,
    /// What was read from the script and is not used yet: lines to look at, and after the second
    /// word, the first bytes of a file.
    seen: Vec<u8>,
}

impl Remote {
    /// Runs `script` through `argv` (the ssh, its options and the destination). The script's input
    /// is open to write when `feeds`, and is nothing otherwise.
    fn start(argv: &[String], script: &str, feeds: bool) -> io::Result<Self> {
        let (program, args) = argv
            .split_first()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "empty argv"))?;
        let mut command = crate::ssh_route::command(program);
        command
            .args(args)
            .arg(crate::upload::remote_command(script))
            .stdin(if feeds { Stdio::piped() } else { Stdio::null() })
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = command.spawn()?;
        let (Some(out), input) = (child.stdout.take(), child.stdin.take()) else {
            // Nothing owns the child yet: it is ended here, with what it started.
            crate::program::end_group(&mut child);
            return Err(io::Error::other("no output"));
        };
        // From here on the child is `remote`'s, and is ended with it when anything fails.
        let mut remote = Self {
            child,
            reaped: false,
            out: File::from(OwnedFd::from(out)),
            input: None,
            seen: Vec::new(),
        };
        stage::nonblocking(&remote.out)?;
        if let Some(input) = input {
            let input = File::from(OwnedFd::from(input));
            stage::nonblocking(&input)?;
            remote.input = Some(input);
        }
        Ok(remote)
    }

    /// Takes the next whole line of what the script said, if there is one.
    fn line(&mut self) -> Option<String> {
        let end = self.seen.iter().position(|&byte| byte == b'\n')?;
        let line: Vec<u8> = self.seen.drain(..=end).collect();
        Some(String::from_utf8_lossy(&line).trim().to_owned())
    }

    /// Waits for the script to say `word` on a line of its own, for as long as `within`. What a
    /// login shell says before it is not looked at.
    fn word(&mut self, word: &str, within: Duration, cancel: &AtomicBool) -> Wait {
        let began = Instant::now();
        let mut buffer = [0u8; 4096];
        loop {
            if cancel.load(Ordering::Acquire) {
                return Wait::Cancelled;
            }
            while let Some(line) = self.line() {
                if line == word {
                    return Wait::Said;
                }
            }
            if began.elapsed() > within {
                return Wait::Late;
            }
            match self.out.read(&mut buffer) {
                Ok(0) => return Wait::Ended,
                Ok(read) => self.seen.extend_from_slice(&buffer[..read]),
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    let _ = stage::ready(&self.out, true, POLL_MS);
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(_) => return Wait::Ended,
            }
        }
    }

    /// Waits for ssh to have ended, for as long as `within`, and says with what status; a process
    /// that is still there after that is ended and has none. `None` when the transfer was taken
    /// back first.
    fn exited(
        &mut self,
        within: Duration,
        look_again: Duration,
        cancel: &AtomicBool,
    ) -> Option<Option<i32>> {
        let began = Instant::now();
        loop {
            if cancel.load(Ordering::Acquire) {
                return None;
            }
            match self.child.try_wait() {
                Ok(Some(status)) => {
                    self.reaped = true;
                    return Some(status.code());
                }
                Ok(None) if began.elapsed() < within => thread::sleep(look_again),
                _ => {
                    crate::program::end_group(&mut self.child);
                    self.reaped = true;
                    return Some(None);
                }
            }
        }
    }
}

impl Drop for Remote {
    fn drop(&mut self) {
        if !self.reaped {
            crate::program::end_group(&mut self.child);
        }
    }
}

/// How a stage that could not go on ended, once ssh is asked what became of it. `None` when the
/// transfer was taken back while it waited.
fn stage_lost(remote: &mut Remote, ground: &Ground, cancel: &AtomicBool) -> Option<StageError> {
    let waits = ground.waits;
    let code = remote.exited(waits.tidy_within, waits.look_again, cancel)?;
    Some(lost(code).stage())
}

/// Pours the file `handle` names, from `offset` to its end, into the pipe `name` on the server,
/// once the program has opened it. `None` when the transfer was taken back.
pub(super) fn run_stage(
    ground: &Ground,
    server: &Server,
    transfer: u32,
    handle: &str,
    offset: u64,
    name: &str,
    cancel: &AtomicBool,
) -> Option<Result<u64, StageError>> {
    let (file, source) = match open_source(ground, handle, offset) {
        Ok(opened) => opened,
        Err(error) => return Some(Err(error)),
    };
    let waits = ground.waits;
    let Some(script) =
        install::pipe_script_waiting(server.dir(), name, Cross::ToProgram, script_waits(waits))
    else {
        return Some(Err(StageError::Io));
    };
    // No connection to be had without asking a person is a connection that is not there.
    let Some(argv) = server.argv() else {
        return Some(Err(StageError::Route));
    };
    let Ok(mut remote) = Remote::start(&argv, &script, true) else {
        return Some(Err(StageError::Io));
    };
    match remote.word(PIPE_READY, waits.silent_for, cancel) {
        Wait::Said => {}
        Wait::Cancelled => return None,
        Wait::Late => return Some(Err(StageError::Route)),
        Wait::Ended => return stage_lost(&mut remote, ground, cancel).map(Err),
    }
    ground.outcomes.pipe_ready(transfer);
    match remote.word(PIPE_OPEN, waits.opened_within, cancel) {
        Wait::Said => {}
        Wait::Cancelled => return None,
        Wait::Late => return Some(Err(StageError::Io)),
        Wait::Ended => return stage_lost(&mut remote, ground, cancel).map(Err),
    }
    let Some(mut input) = remote.input.take() else {
        return Some(Err(StageError::Io));
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
        match pour(&mut input, &buffer[..read], cancel, waits.silent_for) {
            Ok(true) => at += read as u64,
            Ok(false) => return None,
            // The connection went, or the program closed its end and the script with it.
            Err(_) => return stage_lost(&mut remote, ground, cancel).map(Err),
        }
    }
    if let Err(error) = same(&file, &source) {
        return Some(Err(error));
    }
    // The script ends when its input does, and its status is whether the pipe took it all.
    drop(input);
    let code = remote.exited(TAIL, waits.look_again, cancel)?;
    Some(match code {
        Some(0) => Ok(source.size),
        other => Err(lost(other).stage()),
    })
}

/// A fetch that is cut by the connection: the part stays, for the offset it ended at, and the
/// program is told where that is.
fn route(ground: &Ground, handle: &str, part: Part) -> FetchError {
    let at = part.length;
    lock(&ground.parts).insert(handle.to_owned(), part);
    FetchError::Route { at }
}

/// A fetch whose script ended without the file being whole, once ssh says how: the connection that
/// went keeps the part ([`route`]), the rest is a failure.
fn fetch_lost(ground: &Ground, handle: &str, part: Part, code: Option<i32>) -> FetchError {
    match lost(code) {
        Lost::Route => route(ground, handle, part),
        Lost::Io => FetchError::Io,
    }
}

/// Reads the pipe `name` on the server into the part of the save `handle` names until the file is
/// `size` long, then gives the part the place's name. `None` when the transfer was taken back.
#[allow(
    clippy::too_many_arguments,
    reason = "the words the session gives a transfer (id, handle, offset, size, pipe name) with \
              the ground, the server and the cancel flag; a struct would only rename them"
)]
pub(super) fn run_fetch(
    ground: &Ground,
    server: &Server,
    transfer: u32,
    handle: &str,
    offset: u64,
    size: u64,
    name: &str,
    cancel: &AtomicBool,
) -> Option<Result<u64, FetchError>> {
    let (target, mut part) = match take_part(ground, handle, offset, size) {
        Ok(taken) => taken,
        Err(error) => return Some(Err(error)),
    };
    let waits = ground.waits;
    let Some(script) =
        install::pipe_script_waiting(server.dir(), name, Cross::FromProgram, script_waits(waits))
    else {
        return Some(Err(FetchError::Io));
    };
    let Some(argv) = server.argv() else {
        return Some(Err(route(ground, handle, part)));
    };
    let Ok(mut remote) = Remote::start(&argv, &script, false) else {
        return Some(Err(FetchError::Io));
    };
    for (word, within, told) in [
        (PIPE_READY, waits.silent_for, false),
        (PIPE_OPEN, waits.opened_within, true),
    ] {
        match remote.word(word, within, cancel) {
            Wait::Said => {}
            Wait::Cancelled => return None,
            Wait::Late if told => return Some(Err(FetchError::Io)),
            Wait::Late => return Some(Err(route(ground, handle, part))),
            Wait::Ended => {
                let code = remote.exited(waits.tidy_within, waits.look_again, cancel)?;
                return Some(Err(fetch_lost(ground, handle, part, code)));
            }
        }
        if !told {
            ground.outcomes.pipe_ready(transfer);
        }
    }
    let want = size - offset;
    let mut buffer = vec![0u8; PIECE];
    let mut got = 0u64;
    let mut last = Instant::now();
    while got < want {
        if cancel.load(Ordering::Acquire) {
            return None;
        }
        let step = usize::try_from(want - got).map_or(PIECE, |left| left.min(PIECE));
        // What came with the second word is the file's first bytes.
        let read = if remote.seen.is_empty() {
            match remote.out.read(&mut buffer[..step]) {
                Ok(0) => {
                    // The script ended before the file did: the program closed its end after
                    // fewer bytes (status 0), or the connection went.
                    let code = remote.exited(waits.tidy_within, waits.look_again, cancel)?;
                    return Some(Err(match code {
                        Some(0) => FetchError::Size,
                        other => fetch_lost(ground, handle, part, other),
                    }));
                }
                Ok(read) => read,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    // Nothing for this long is a connection that is not carrying anything, as
                    // a stage reads it (the part stays, the program asks again from `at`); `remote`
                    // ends ssh when it is dropped.
                    if !stage::ready(&remote.out, true, POLL_MS)
                        && last.elapsed() > waits.silent_for
                    {
                        return Some(Err(route(ground, handle, part)));
                    }
                    continue;
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(_) => return Some(Err(FetchError::Io)),
            }
        } else {
            let taken = remote.seen.len().min(step);
            buffer[..taken].copy_from_slice(&remote.seen[..taken]);
            remote.seen.drain(..taken);
            taken
        };
        if let Err(error) = part.file.write_all_at(&buffer[..read], part.length) {
            return Some(Err(fetch_error(error)));
        }
        part.length += read as u64;
        got += read as u64;
        last = Instant::now();
    }
    // The program has closed its end when it has written the file; the script ends with it, and
    // takes the pipe with it before the program hears that the file is saved.
    remote.exited(waits.tidy_within, waits.look_again, cancel)?;
    // A pipe from which more came than the file has: the bytes are not the file's. Looked at once
    // the script is over, so that what the program wrote last is in what ssh left.
    let more =
        !remote.seen.is_empty() || matches!(remote.out.read(&mut buffer), Ok(extra) if extra > 0);
    if more {
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
    use std::fs;
    use std::io::Write as _;
    use std::time::Duration;

    use super::super::tests::{PIPE, Rig, Told, bytes, fill, read_pipe};
    use super::super::{FetchError, StageError, Waits, lock};
    use crate::child::wait_until;

    /// A second pipe, for the attempt after a connection that went: the pipe of the first is
    /// gone with the script, when the script had the time.
    const AGAIN: &str = "1f2e3d4c5b6a79880a1b2c3d4e5f6071";

    fn quick() -> Waits {
        Waits {
            opened_within: Duration::from_millis(600),
            tidy_within: Duration::from_secs(2),
            ..Waits::default()
        }
    }

    #[test]
    #[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
    fn a_file_is_poured_through_the_connection_into_the_pipe_on_the_server() {
        let rig = Rig::server("server-pour", Waits::default());
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
        assert_eq!(
            rig.left_in_folder(),
            Vec::<String>::new(),
            "the script took its pipe with it before the program heard"
        );
        wait_until("the transfer to be over", || {
            rig.files.open_transfers() == 0
        });
        rig.nothing_more();
    }

    #[test]
    #[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
    fn a_stage_that_begins_at_an_offset_pours_the_rest_of_the_file() {
        let rig = Rig::server("server-offset", Waits::default());
        let file = bytes(300_000);
        let handle = rig.give(&file);
        rig.files.stage(1, &handle, 123_456, PIPE);
        assert_eq!(rig.next(), Told::PipeReady(1));
        let program = read_pipe(rig.pipe());
        assert_eq!(rig.next(), Told::Staged(1, Ok(file.len() as u64)));
        assert!(program.join().expect("the program") == file[123_456..]);
    }

    /// A program that does not open the pipe is waited for a while and then `io`; and none of the
    /// file has gone into the connection by then, so that nothing is counted sent that a buffer
    /// holds.
    #[test]
    #[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
    fn a_pipe_the_program_never_opens_is_io_and_not_a_byte_of_the_file_was_sent() {
        let rig = Rig::server("server-never", quick());
        rig.mode("tee");
        let handle = rig.give(&bytes(1_000_000));
        rig.files.stage(1, &handle, 0, PIPE);
        assert_eq!(rig.next(), Told::PipeReady(1));
        assert_eq!(rig.next(), Told::Staged(1, Err(StageError::Io)));
        let sent = fs::read(rig.home.join("stdin-log")).unwrap_or_default();
        assert_eq!(sent.len(), 0, "nothing was sent before the program opened");
        wait_until("the transfer to be over", || {
            rig.files.open_transfers() == 0
        });
    }

    #[test]
    #[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
    fn a_connection_that_cannot_be_made_is_the_route_gone_with_no_pipe() {
        let rig = Rig::server("server-refused", Waits::default());
        rig.mode("refuse");
        let handle = rig.give(&bytes(1000));
        rig.files.stage(1, &handle, 0, PIPE);
        assert_eq!(rig.next(), Told::Staged(1, Err(StageError::Route)));
        let (save, _) = rig.place("out.bin");
        rig.files.fetch(2, &save, 0, 10, PIPE);
        assert_eq!(
            rig.next(),
            Told::Fetched(2, Err(FetchError::Route { at: 0 }))
        );

        let rig = Rig::unreachable("server-nobody-asks", Waits::default());
        let handle = rig.give(&bytes(1000));
        rig.files.stage(1, &handle, 0, PIPE);
        assert_eq!(
            rig.next(),
            Told::Staged(1, Err(StageError::Route)),
            "no connection without asking a person is no connection"
        );
    }

    /// The connection goes while the file is being poured: `route`, and the program asks again
    /// from the offset it has got to.
    #[test]
    #[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
    fn a_connection_that_goes_in_the_middle_of_a_stage_is_the_route_and_the_rest_goes_after() {
        let rig = Rig::server("server-route-stage", Waits::default());
        let file = bytes(2 * 1024 * 1024);
        let handle = rig.give(&file);
        rig.mode("leave-after:100000");
        rig.files.stage(1, &handle, 0, PIPE);
        assert_eq!(rig.next(), Told::PipeReady(1));
        let first = read_pipe(rig.pipe());
        assert_eq!(rig.next(), Told::Staged(1, Err(StageError::Route)));
        let got = first.join().expect("the program");
        assert_eq!(got.len(), 100_000);
        assert!(got == file[..100_000]);

        // The program asks again from where it is.
        rig.mode("");
        rig.files.stage(2, &handle, 100_000, AGAIN);
        assert_eq!(rig.next(), Told::PipeReady(2));
        let path = rig.folder.dir().join(AGAIN);
        let second = super::super::tests::read_pipe(path);
        assert_eq!(rig.next(), Told::Staged(2, Ok(file.len() as u64)));
        assert!(second.join().expect("the program") == file[100_000..]);
    }

    #[test]
    #[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
    fn a_stage_taken_back_ends_the_connection_and_tells_nothing() {
        let rig = Rig::server("server-unstage", Waits::default());
        let handle = rig.give(&bytes(1000));
        rig.files.stage(1, &handle, 0, PIPE);
        assert_eq!(rig.next(), Told::PipeReady(1));
        rig.files.unstage(1);
        wait_until("the transfer to end", || rig.files.open_transfers() == 0);
        rig.nothing_more();
    }

    #[test]
    #[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
    fn a_pipe_the_program_fills_on_the_server_is_saved_whole_under_the_places_name() {
        let rig = Rig::server("server-fetch", Waits::default());
        let file = bytes(3 * 1024 * 1024 + 5);
        let (handle, target) = rig.place("out.bin");
        rig.files.fetch(1, &handle, 0, file.len() as u64, PIPE);
        assert_eq!(rig.next(), Told::PipeReady(1));
        let program = fill(&rig, file.clone());
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

    /// The connection goes while a file is coming: the part is kept, the program is told where it
    /// ended, and the next fetch from there completes the file.
    #[test]
    #[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
    fn a_connection_that_goes_while_a_file_comes_keeps_the_part_and_the_rest_completes_it() {
        let rig = Rig::server("server-route-fetch", Waits::default());
        let file = bytes(2 * 1024 * 1024);
        let (handle, target) = rig.place("out.bin");
        // The two words, then a hundred thousand bytes of the file.
        rig.mode(&format!(
            "die-after:{}",
            "pencere-ready\npencere-open\n".len() + 100_000
        ));
        rig.files.fetch(1, &handle, 0, file.len() as u64, PIPE);
        assert_eq!(rig.next(), Told::PipeReady(1));
        let _ = fill(&rig, file.clone());
        let Told::Fetched(1, Err(FetchError::Route { at })) = rig.next() else {
            panic!("the route was gone");
        };
        assert_eq!(at, 100_000);
        let kept = fs::metadata(
            rig.parts()
                .first()
                .map(|name| rig.home.join(name))
                .expect("a part"),
        )
        .expect("the part")
        .len();
        assert_eq!(kept, at, "the part holds what the program was told");
        assert_eq!(rig.parts().len(), 1, "the part waits for the rest");
        assert!(!target.exists());

        rig.mode("");
        rig.files.fetch(2, &handle, at, file.len() as u64, AGAIN);
        assert_eq!(rig.next(), Told::PipeReady(2));
        let path = rig.folder.dir().join(AGAIN);
        let rest = file[usize::try_from(at).expect("an offset")..].to_vec();
        let program = std::thread::spawn(move || {
            let mut pipe = fs::OpenOptions::new()
                .write(true)
                .open(path)
                .expect("opens");
            pipe.write_all(&rest).expect("the bytes");
        });
        assert_eq!(rig.next(), Told::Fetched(2, Ok(file.len() as u64)));
        program.join().expect("the program");
        assert!(fs::read(&target).expect("the saved file") == file);
        assert_eq!(rig.parts(), Vec::<String>::new());
    }

    /// A connection that is there and carries nothing (a network that dropped without ssh knowing)
    /// is a connection that went, as a stage reads it: the part stays and the program is told where
    /// it ended, and ssh does not outlive the transfer.
    #[test]
    #[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
    fn a_connection_that_falls_silent_while_a_file_comes_keeps_the_part_like_one_that_goes() {
        let waits = Waits {
            opened_within: Duration::from_millis(600),
            silent_for: Duration::from_millis(800),
            ..Waits::default()
        };
        let rig = Rig::server("server-silent-fetch", waits);
        let file = bytes(1024 * 1024);
        let (handle, target) = rig.place("out.bin");
        rig.mode(&format!(
            "stall-after:{}",
            "pencere-ready\npencere-open\n".len() + 50_000
        ));
        rig.files.fetch(1, &handle, 0, file.len() as u64, PIPE);
        assert_eq!(rig.next(), Told::PipeReady(1));
        let _ = fill(&rig, file);
        assert_eq!(
            rig.next(),
            Told::Fetched(1, Err(FetchError::Route { at: 50_000 }))
        );
        assert_eq!(rig.parts().len(), 1, "the part waits for the rest");
        assert!(!target.exists());
        wait_until("the transfer to end", || rig.files.open_transfers() == 0);
    }

    #[test]
    #[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
    fn a_pipe_on_the_server_that_ends_short_or_holds_more_is_size_and_leaves_no_part() {
        let rig = Rig::server("server-size", Waits::default());
        let (handle, target) = rig.place("out.bin");
        rig.files.fetch(1, &handle, 0, 1000, PIPE);
        assert_eq!(rig.next(), Told::PipeReady(1));
        let program = fill(&rig, bytes(400));
        assert_eq!(rig.next(), Told::Fetched(1, Err(FetchError::Size)));
        program.join().expect("the program");
        assert_eq!(rig.parts(), Vec::<String>::new());
        assert!(!target.exists());

        rig.files.fetch(2, &handle, 0, 1000, AGAIN);
        assert_eq!(rig.next(), Told::PipeReady(2));
        let path = rig.folder.dir().join(AGAIN);
        let program = std::thread::spawn(move || {
            let mut pipe = fs::OpenOptions::new()
                .write(true)
                .open(path)
                .expect("opens");
            let _ = pipe.write_all(&bytes(2000));
        });
        assert_eq!(rig.next(), Told::Fetched(2, Err(FetchError::Size)));
        program.join().expect("the program");
        assert_eq!(rig.parts(), Vec::<String>::new());
        assert!(!target.exists());
    }

    #[test]
    #[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
    fn a_fetch_taken_back_ends_the_connection_and_leaves_no_part() {
        let rig = Rig::server("server-fetch-back", Waits::default());
        let (handle, target) = rig.place("out.bin");
        rig.files.fetch(1, &handle, 0, 5000, PIPE);
        assert_eq!(rig.next(), Told::PipeReady(1));
        rig.files.unstage(1);
        wait_until("the transfer to end", || rig.files.open_transfers() == 0);
        rig.nothing_more();
        assert_eq!(rig.parts(), Vec::<String>::new());
        assert!(!target.exists());
    }
}
