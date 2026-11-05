//! A program run on this machine and held as the server's would be ([`RemoteCommand`]): for the
//! tests of what is built on the handles, which then run over the same pipes that kablo's streams
//! feed, with a real process at the far end.

use std::io::{Read, Write};
use std::os::unix::process::ExitStatusExt;
use std::process::Child;
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use kablo_proto::v1::Exit;

use super::{Far, Port, RemoteCommand, pipes};
use crate::kablo::streams::Over;

/// The process the handles stand for.
struct Local {
    pid: u32,
    wake: Mutex<Sender<()>>,
}

impl Port for Local {
    fn took(&self, _: usize) {}

    fn wrote(&self) {
        let _ = self.wake.lock().unwrap_or_else(|e| e.into_inner()).send(());
    }

    fn hang_up(&self) {
        // The process and what it started: the child was made the leader of its own group.
        // SAFETY: `kill(2)` takes a group and a signal; the group is this child's own, and the
        // child is not reaped before its pipes are all read (`run`'s waiter).
        // audit: pid from `u32` to `pid_t`; on macOS pids are positive `i32`.
        unsafe {
            libc::kill(-(self.pid as libc::pid_t), libc::SIGKILL);
        }
    }
}

/// `child` (its standard input, output and error piped, and the leader of its own process group)
/// as a program the server runs.
pub(crate) fn run(mut child: Child) -> RemoteCommand {
    let (wake, wakes) = mpsc::channel();
    let port = Arc::new(Local {
        pid: child.id(),
        wake: Mutex::new(wake),
    });
    let (handles, far) = pipes(port);
    let far = Arc::new(Mutex::new(far));
    lock(&far).opened();

    let reading = |stream: Option<Box<dyn Read + Send>>, error: bool| {
        let far = Arc::clone(&far);
        thread::spawn(move || {
            let Some(mut stream) = stream else {
                return;
            };
            let mut buf = vec![0u8; 32 * 1024];
            loop {
                match stream.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        let data = buf.get(..n).unwrap_or_default().to_vec();
                        let mut far = lock(&far);
                        if error {
                            far.err(data);
                        } else {
                            far.out(data);
                        }
                    }
                }
            }
        })
    };
    let out = reading(
        child
            .stdout
            .take()
            .map(|stream| Box::new(stream) as Box<dyn Read + Send>),
        false,
    );
    let err = reading(
        child
            .stderr
            .take()
            .map(|stream| Box::new(stream) as Box<dyn Read + Send>),
        true,
    );
    let mut stdin = child.stdin.take();
    {
        let far = Arc::clone(&far);
        thread::spawn(move || {
            loop {
                let next = lock(&far).next_input();
                match next {
                    Ok(Some(chunk)) => {
                        // A process that stopped reading has its input closed: what is written
                        // after is lost, as it is to a server whose program ended.
                        if let Some(input) = &mut stdin
                            && input.write_all(&chunk).is_err()
                        {
                            stdin = None;
                        }
                    }
                    Ok(None) => {
                        let _ = wakes.recv_timeout(Duration::from_millis(20));
                    }
                    Err(()) => break,
                }
            }
        });
    }
    thread::spawn(move || {
        let status = child.wait();
        let _ = out.join();
        let _ = err.join();
        let mut far = lock(&far);
        far.eof();
        if let Ok(status) = status {
            far.exited(match (status.code(), status.signal()) {
                (Some(code), _) => Exit::Code(code),
                (None, signal) => Exit::Signal(signal.unwrap_or(0)),
            });
        }
        far.over(&Over::Closed);
    });
    match handles.opened(Duration::from_secs(10)) {
        Ok(opened) => RemoteCommand::of(opened),
        Err(text) => panic!("the local program could not be held: {text}"),
    }
}

fn lock(far: &Mutex<Far>) -> std::sync::MutexGuard<'_, Far> {
    far.lock().unwrap_or_else(|e| e.into_inner())
}
