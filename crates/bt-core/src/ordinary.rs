//! An ordinary shell for a pane whose terminal was refused: a PTY of this process with the
//! pane's program in it, which the owner of the pane's stream feeds the stream from and types
//! into.
//!
//! A pane that is born over a stream ([`crate::Session::streamed`]) cannot become a spawned
//! session afterwards — its session, its frame link and its keyboard are made once — so when
//! the terminal it was to show is refused, the stream's owner starts the shell here and moves its
//! bytes. What the session sees is a stream like any other; what the owner knows is that the
//! shell is **not kept** ([`crate::StreamExit::made_ordinary`]): bateri's quit ends it, and its
//! pane is told so.

use std::collections::HashMap;
use std::io::{self, Read as _, Write as _};
use std::os::fd::{AsRawFd as _, RawFd};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, Weak};
use std::thread;
use std::time::{Duration, Instant};

use alacritty_terminal::event::{OnResize, WindowSize};
use alacritty_terminal::tty::{self, EventedPty as _, Pty, Shell};

use crate::session::{ChildLaunch, PtySize};

/// How a shell of [`Ordinary`] ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ended {
    /// It called `exit` with this code.
    Code(i32),
    /// A signal killed it.
    Signal(i32),
}

/// How long the end of a shell is waited for once its terminal has closed.
const REAP: Duration = Duration::from_secs(2);

/// A shell started for a pane of a stream: its bytes go to `out` as they are read, how it ended
/// goes to `ended` once, and what the pane types is [`Ordinary::write`]. Dropping it hangs the
/// shell up, on a thread of its own (a PTY waits for its child when it is let go of).
pub struct Ordinary {
    pid: u32,
    input: Option<Sender<Vec<u8>>>,
    pty: Option<Arc<Mutex<Pty>>>,
}

impl Ordinary {
    /// Starts `launch`'s command in a PTY of its size.
    pub fn start(
        launch: &ChildLaunch,
        out: impl Fn(Vec<u8>) + Send + 'static,
        ended: impl FnOnce(Ended) + Send + 'static,
    ) -> io::Result<Self> {
        let shell = launch
            .argv
            .split_first()
            .map(|(program, args)| Shell::new(program.clone(), args.to_vec()));
        let env: HashMap<String, String> = launch.env.iter().cloned().collect();
        let options = tty::Options {
            shell,
            working_directory: launch.cwd.clone(),
            env,
            ..Default::default()
        };
        let pty = tty::new(&options, window(launch.size), 0)?;
        let pid = pty.child().id();
        // The PTY is made for a poll loop and is not blocking; the reader and the writer here
        // each have a thread of their own, and wait in the read and the write.
        blocking(pty.file().as_raw_fd());
        let mut reading = pty.file().try_clone()?;
        let mut writing = pty.file().try_clone()?;
        let pty = Arc::new(Mutex::new(pty));
        let weak: Weak<Mutex<Pty>> = Arc::downgrade(&pty);
        thread::Builder::new()
            .name("an ordinary shell reads".into())
            .spawn(move || {
                let mut buffer = vec![0u8; 16 * 1024];
                loop {
                    match reading.read(&mut buffer) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => out(buffer[..n].to_vec()),
                    }
                }
                // Its terminal closed: the shell is over, and how it ended is asked of the PTY
                // that started it — if its owner still wants to know.
                let deadline = Instant::now() + REAP;
                while Instant::now() < deadline {
                    let Some(pty) = weak.upgrade() else {
                        return;
                    };
                    let event = pty
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .next_child_event();
                    drop(pty);
                    if let Some(tty::ChildEvent::Exited(status)) = event {
                        ended(match status {
                            Some(status) => end_of(status),
                            None => Ended::Code(1),
                        });
                        return;
                    }
                    thread::sleep(Duration::from_millis(5));
                }
                ended(Ended::Code(1));
            })?;
        let (input, typed) = mpsc::channel::<Vec<u8>>();
        thread::Builder::new()
            .name("an ordinary shell is typed into".into())
            .spawn(move || {
                while let Ok(bytes) = typed.recv() {
                    if writing.write_all(&bytes).is_err() {
                        return;
                    }
                }
            })?;
        Ok(Self {
            pid,
            input: Some(input),
            pty: Some(pty),
        })
    }

    /// The shell's process id.
    pub fn pid(&self) -> u32 {
        self.pid
    }

    /// What the pane typed: written in order, on a thread of its own (a shell that does not read
    /// does not hold the caller).
    pub fn write(&self, bytes: Vec<u8>) {
        if let Some(input) = &self.input {
            let _ = input.send(bytes);
        }
    }

    /// The pane's size changed.
    pub fn resize(&self, size: PtySize) {
        if let Some(pty) = &self.pty {
            pty.lock()
                .unwrap_or_else(|e| e.into_inner())
                .on_resize(window(size));
        }
    }
}

impl Drop for Ordinary {
    fn drop(&mut self) {
        self.input = None;
        if let Some(pty) = self.pty.take() {
            let _ = thread::Builder::new()
                .name("an ordinary shell hangs up".into())
                .spawn(move || drop(pty));
        }
    }
}

fn blocking(fd: RawFd) {
    // SAFETY: `fd` is the master of a PTY this process holds open.
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFL);
        if flags >= 0 {
            libc::fcntl(fd, libc::F_SETFL, flags & !libc::O_NONBLOCK);
        }
    }
}

fn window(size: PtySize) -> WindowSize {
    WindowSize {
        num_cols: size.cols,
        num_lines: size.rows,
        cell_width: size.cell_width,
        cell_height: size.cell_height,
    }
}

fn end_of(status: std::process::ExitStatus) -> Ended {
    use std::os::unix::process::ExitStatusExt as _;
    match (status.code(), status.signal()) {
        (Some(code), _) => Ended::Code(code),
        (None, Some(signal)) => Ended::Signal(signal),
        (None, None) => Ended::Code(1),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::Duration;

    use super::*;

    fn launch(script: &str) -> ChildLaunch {
        ChildLaunch {
            argv: vec!["/bin/sh".to_owned(), "-c".to_owned(), script.to_owned()],
            env: vec![("TERM".to_owned(), "xterm-256color".to_owned())],
            cwd: None,
            size: PtySize {
                cols: 80,
                rows: 24,
                cell_width: 8,
                cell_height: 16,
            },
        }
    }

    #[test]
    fn a_shell_started_for_a_pane_says_what_it_writes_and_how_it_ended() {
        let (out, heard) = mpsc::channel();
        let (end, ended) = mpsc::channel();
        let shell = Ordinary::start(
            &launch("printf hello; exit 3"),
            move |bytes| {
                let _ = out.send(bytes);
            },
            move |how| {
                let _ = end.send(how);
            },
        )
        .expect("a shell");
        assert!(shell.pid() > 1);
        assert_eq!(
            ended.recv_timeout(Duration::from_secs(10)),
            Ok(Ended::Code(3))
        );
        let mut text = Vec::new();
        while let Ok(bytes) = heard.try_recv() {
            text.extend(bytes);
        }
        assert!(String::from_utf8_lossy(&text).contains("hello"), "{text:?}");
    }

    #[test]
    fn what_the_pane_types_reaches_the_shell_in_order_and_its_size_is_the_terminals() {
        let (out, heard) = mpsc::channel();
        let (end, ended) = mpsc::channel();
        let shell = Ordinary::start(
            &launch("stty raw -echo; printf ready; head -c 6 >/dev/null; stty size"),
            move |bytes| {
                let _ = out.send(bytes);
            },
            move |how| {
                let _ = end.send(how);
            },
        )
        .expect("a shell");
        // Typed once the shell is reading raw: before, the line discipline would keep it.
        let mut text = Vec::new();
        while !String::from_utf8_lossy(&text).contains("ready") {
            text.extend(
                heard
                    .recv_timeout(Duration::from_secs(10))
                    .expect("the shell is up"),
            );
        }
        shell.resize(PtySize {
            cols: 91,
            rows: 33,
            cell_width: 8,
            cell_height: 16,
        });
        shell.write(b"abc".to_vec());
        shell.write(b"def".to_vec());
        assert_eq!(
            ended.recv_timeout(Duration::from_secs(10)),
            Ok(Ended::Code(0))
        );
        while let Ok(bytes) = heard.try_recv() {
            text.extend(bytes);
        }
        let text = String::from_utf8_lossy(&text).into_owned();
        assert!(text.contains("33 91"), "{text:?}");
    }
}
