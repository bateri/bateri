//! A kablo terminal's screen as a session with nothing behind it: the picture the big view draws
//! of a terminal no pane shows.
//!
//! The server answers a `read` that asks for a picture with the bytes that build the screen
//! when they are written to a freshly reset terminal of the size it says. [`pictured`] writes
//! them to one — a [`Session`] over a stream nobody ever writes to — and the session is then
//! what a pane's session is for everything that reads a screen (`Session::frame`, the
//! composition of a still), without a PTY, a program, a journal or a connection to the
//! terminal's server.
//!
//! **Journal-free.** The bytes go through the session's replay path (the parser and the
//! cluster wrapper of the reader, past the scanner), not through the stream: nothing is typed
//! to, nothing is asked of, and nothing is noted about the terminal the picture is of, so a
//! picture of a terminal that is a command's program or a password prompt is only a picture.
//!
//! **What the bytes do not carry** (`.claude/rules/akis.md`): kitty graphics commands are not
//! in a server's copy of the screen, so no image is drawn in a picture — this path parses with
//! no command state of its own and would drop one without a note, which is right only because
//! none comes. The pixel-mouse (`1016`) and size-report (`2048`) modes the picture turns on
//! are the `KeptModes` of the replay's session and mean nothing in a session nobody types
//! into.

use std::io;
use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream;
use std::sync::Arc;

use crate::{PtyOps, PtySize, Session, SessionOptions, Stream, StreamExit, Wake};

/// The terminal's side of the stream, held for as long as the session lives: closing either
/// end would tell the reader the program had ended.
struct Held {
    _terminal: UnixStream,
    _exit: UnixStream,
}

impl PtyOps for Held {
    fn resize(&self, _master: std::os::fd::BorrowedFd<'_>, _size: PtySize) {}

    fn hangup(&self, _pid: u32, _exit: std::os::fd::BorrowedFd<'_>) {}
}

/// A wake that hears everything and does nothing: a picture is read when someone draws it.
struct Quiet;

impl Wake for Quiet {
    fn wake(&self) {}
    fn child_exit(&self, _code: Option<i32>) {}
    fn copy_to_clipboard(&self, _text: String) {}
    fn title_changed(&self) {}
    fn search_changed(&self) {}
    fn command_started(&self) {}
    fn phase_edge(&self) {}
    fn remote_command_edge(&self) {}
    fn mirror_changed(&self) {}
    fn environment_changed(&self) {}
    fn remote_up(&self) {}
    fn remote_typed(&self) {}
    fn link_hover_lost(&self) {}
    fn pointer_changed(&self) {}
    fn blocks_changed(&self) {}
    fn unseen_changed(&self) {}
    fn program_status_changed(&self) {}
    fn notification(&self) {}
}

/// The session whose screen `picture` builds, `options.cols` × `options.rows` large (the size
/// the server said the picture is of). Nothing else of `options` is a command to run: its
/// `command`, `working_directory`, `env`, `initial_input` and `views` are not used.
///
/// The bytes are written to the session's terminal before this returns, so a frame read at
/// once is the picture's.
///
/// # Errors
///
/// The sockets or the session's reader could not be made.
pub fn pictured(mut options: SessionOptions, picture: &[u8]) -> io::Result<Session> {
    let (terminal, master) = UnixStream::pair()?;
    master.set_nonblocking(true)?;
    let (exit, exit_end) = UnixStream::pair()?;
    options.replay = Some(picture.to_vec());
    options.initial_input = None;
    options.shell_marks = false;
    options.views = None;
    Session::streamed(
        options,
        Arc::new(Quiet),
        Stream {
            master: OwnedFd::from(master),
            exit: OwnedFd::from(exit_end),
            status: Arc::new(StreamExit::default()),
            pid: 0,
            ops: Arc::new(Held {
                _terminal: terminal,
                _exit: exit,
            }),
            silence: None,
        },
    )
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::time::{Duration, Instant};

    use super::*;
    use crate::{Settings, Theme};

    fn options() -> SessionOptions {
        SessionOptions {
            command: None,
            working_directory: None,
            home: None,
            env: HashMap::new(),
            cols: 20,
            rows: 5,
            cell_px: (8, 16),
            terminal: Settings::default().terminal(),
            theme: Theme::BATERI,
            dock: false,
            cluster: true,
            initial_input: None,
            shell_marks: false,
            pane_uuid: None,
            hostname: None,
            replay: None,
            views: None,
        }
    }

    #[test]
    fn a_picture_session_lets_go_at_once_not_after_the_shutdown_grace() {
        // The big view drops one of these for every screen that changes, on the main thread.
        let started = Instant::now();
        for _ in 0..10 {
            let session =
                pictured(options(), b"\x1b[2J\x1b[H\x1b[41m   \x1b[0m").expect("a session");
            drop(session);
        }
        let took = started.elapsed();
        eprintln!("ten pictured sessions built and dropped in {took:?}");
        assert!(
            took < Duration::from_millis(500),
            "ten sessions took {took:?}: a drop waits on the reader"
        );
    }
}
