//! What a caller holds of a program or a connection the server runs for it over kablo's
//! connection: handles with blocking reads and writes, the way a `Child` is held.
//!
//! The caller never touches the service's thread or the client: a read waits for what the
//! server sent and says it has taken it (which is what credits the server, so a caller that reads
//! slowly makes the server wait and the app's memory does not grow), a write waits when the
//! server's window is full, and dropping the handles ends the stream (a program the server
//! runs for it is ended with it). The service's ends of the same pipes are [`Far`].
//!
//! **The end.** A read that finds the stream over before the server said all it had returns an
//! error with the reason (the connection went, the server closed it); one that finds the server's
//! `eof` returns 0. [`RemoteCommand::wait`] gives how the program ended, or why the stream is
//! over without it.

use std::io::{self, Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, SyncSender, TrySendError};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use kablo_proto::v1::Exit;
use kablo_proto::v1::stream::MAX_DATA;

use super::streams::Over;

/// How many chunks a writer may have waiting for the service before its write waits: the memory
/// a writer can hold is this many `data` messages, whatever the window.
const WRITER_CHUNKS: usize = 8;

/// Why a stream ended with the connection.
const LOST: &str = "the connection to the server was lost";

/// What the service gives a reader: bytes, or why the stream is over.
type Item = Result<Vec<u8>, String>;

/// What the handles ask of the stream they stand for.
pub(super) trait Port: Send + Sync {
    /// A reader handed `bytes` of what the server sent to its caller: the server may send that
    /// much more.
    fn took(&self, bytes: usize);
    /// A writer queued something, or ended: the service is to look.
    fn wrote(&self);
    /// The caller is done with the stream: it is closed, and a program the server runs for it is
    /// ended.
    fn hang_up(&self);
}

/// How a stream ended, as far as the caller is told.
#[derive(Default)]
struct Ended {
    /// How the program ended, when the server said.
    exit: Option<Exit>,
    /// The stream is over, and the reason a program's end was not heard before it.
    over: Option<String>,
}

/// What the handles and the service share of one stream.
pub(super) struct Shared {
    ended: Mutex<Ended>,
    ended_cv: Condvar,
    /// The caller gave up waiting for the open.
    abandoned: AtomicBool,
}

impl Shared {
    fn ended(&self) -> std::sync::MutexGuard<'_, Ended> {
        self.ended.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// The caller's ends of a stream, before the open was answered.
pub(super) struct Handles {
    port: Arc<dyn Port>,
    shared: Arc<Shared>,
    out: Receiver<Item>,
    err: Receiver<Item>,
    input: SyncSender<Vec<u8>>,
    answer: Receiver<Result<(), String>>,
}

/// The service's ends of a stream.
pub(super) struct Far {
    out: Option<Sender<Item>>,
    err: Option<Sender<Item>>,
    input: Receiver<Vec<u8>>,
    input_ended: bool,
    shared: Arc<Shared>,
    answer: Option<Sender<Result<(), String>>>,
}

impl Drop for Far {
    /// A stream the service lets go of without ending it (the service itself stopped) is over for
    /// the caller too: its readers get the reason and its wait for the end is released.
    fn drop(&mut self) {
        if self.shared.ended().over.is_none() {
            self.over(&Over::Lost);
        }
    }
}

/// The two ends of the pipes of one stream.
pub(super) fn pipes(port: Arc<dyn Port>) -> (Handles, Far) {
    let shared = Arc::new(Shared {
        ended: Mutex::new(Ended::default()),
        ended_cv: Condvar::new(),
        abandoned: AtomicBool::new(false),
    });
    let (out_tx, out) = mpsc::channel();
    let (err_tx, err) = mpsc::channel();
    let (input, input_rx) = mpsc::sync_channel(WRITER_CHUNKS);
    let (answer_tx, answer) = mpsc::channel();
    (
        Handles {
            port,
            shared: Arc::clone(&shared),
            out,
            err,
            input,
            answer,
        },
        Far {
            out: Some(out_tx),
            err: Some(err_tx),
            input: input_rx,
            input_ended: false,
            shared,
            answer: Some(answer_tx),
        },
    )
}

impl Handles {
    /// Waits for the server's answer to the open, up to `wait`. `Err` is why there is no stream.
    pub(super) fn opened(self, wait: Duration) -> Result<Opened, String> {
        match self.answer.recv_timeout(wait) {
            Ok(Ok(())) => {}
            Ok(Err(text)) => return Err(text),
            Err(_) => {
                self.shared.abandoned.store(true, Ordering::SeqCst);
                return Err("the server did not answer in time".to_owned());
            }
        }
        let hold = Arc::new(Hold {
            port: Arc::clone(&self.port),
        });
        let end = Arc::new(End {
            shared: self.shared,
            hold: Arc::clone(&hold),
        });
        Ok(Opened {
            stdout: Reader::new(self.out, Arc::clone(&self.port), Arc::clone(&hold)),
            stderr: Reader::new(self.err, Arc::clone(&self.port), Arc::clone(&hold)),
            stdin: Writer {
                input: Some(self.input),
                port: Arc::clone(&self.port),
                _hold: hold,
            },
            end,
        })
    }
}

/// The handles of a stream the server took.
pub(super) struct Opened {
    pub(super) stdout: Reader,
    pub(super) stderr: Reader,
    pub(super) stdin: Writer,
    pub(super) end: Arc<End>,
}

/// A program the server runs for the caller, held as a `Child` is: write its standard input, read
/// its standard output and error (each on a thread of its own when both are read), and wait for
/// how it ended. Dropping all of it ends the program.
pub struct RemoteCommand {
    /// What the program reads. Dropping it is the program's end of input.
    pub stdin: Option<Writer>,
    /// What the program writes to its standard output.
    pub stdout: Reader,
    /// What the program writes to its standard error.
    pub stderr: Reader,
    /// How it ends, and the way to end it.
    pub end: Arc<End>,
}

impl RemoteCommand {
    pub(super) fn of(opened: Opened) -> RemoteCommand {
        RemoteCommand {
            stdin: Some(opened.stdin),
            stdout: opened.stdout,
            stderr: opened.stderr,
            end: opened.end,
        }
    }

    /// Waits until the program has ended, and says how ([`End::wait`]).
    #[cfg(test)]
    pub fn wait(&self) -> Result<Exit, String> {
        self.end.wait()
    }

    /// How the program ended, if it has ([`End::try_wait`]).
    #[cfg(test)]
    pub fn try_wait(&self) -> Option<Result<Exit, String>> {
        self.end.try_wait()
    }

    /// Ends the program now ([`End::kill`]).
    #[cfg(test)]
    pub fn kill(&self) {
        self.end.kill();
    }
}

/// A TCP connection the server makes for the caller. Dropping `writer` is the connection's end of
/// what the caller says (the server's end sees `eof`, and what it says back is still read);
/// dropping all of it closes the connection.
pub struct RemoteStream {
    /// What the server's end says.
    pub reader: Reader,
    /// What the caller says to the server's end.
    pub writer: Option<Writer>,
    /// The way to close it.
    pub end: Arc<End>,
}

impl RemoteStream {
    pub(super) fn of(opened: Opened) -> RemoteStream {
        RemoteStream {
            reader: opened.stdout,
            writer: Some(opened.stdin),
            end: opened.end,
        }
    }
}

/// Ends the stream when the last handle of it is dropped.
struct Hold {
    port: Arc<dyn Port>,
}

impl Drop for Hold {
    fn drop(&mut self) {
        self.port.hang_up();
    }
}

/// What the server sent, as a caller reads it.
pub struct Reader {
    items: Receiver<Item>,
    chunk: Vec<u8>,
    at: usize,
    port: Arc<dyn Port>,
    _hold: Arc<Hold>,
}

impl Reader {
    fn new(items: Receiver<Item>, port: Arc<dyn Port>, hold: Arc<Hold>) -> Reader {
        Reader {
            items,
            chunk: Vec::new(),
            at: 0,
            port,
            _hold: hold,
        }
    }
}

impl Read for Reader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        if self.at >= self.chunk.len() {
            match self.items.recv() {
                Ok(Ok(chunk)) => {
                    self.chunk = chunk;
                    self.at = 0;
                }
                Ok(Err(text)) => {
                    return Err(io::Error::new(io::ErrorKind::ConnectionAborted, text));
                }
                Err(_) => return Ok(0),
            }
        }
        let rest = self.chunk.get(self.at..).unwrap_or_default();
        let n = rest.len().min(buf.len());
        if let (Some(from), Some(to)) = (rest.get(..n), buf.get_mut(..n)) {
            to.copy_from_slice(from);
        }
        self.at += n;
        self.port.took(n);
        Ok(n)
    }
}

/// What a caller says to the server's end. Dropping it says there is no more (`eof`).
pub struct Writer {
    input: Option<SyncSender<Vec<u8>>>,
    port: Arc<dyn Port>,
    _hold: Arc<Hold>,
}

impl Write for Writer {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let chunk = buf.get(..buf.len().min(MAX_DATA)).unwrap_or_default();
        if chunk.is_empty() {
            return Ok(0);
        }
        let Some(input) = &self.input else {
            return Err(io::Error::new(io::ErrorKind::BrokenPipe, "already closed"));
        };
        input
            .send(chunk.to_vec())
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "the stream is over"))?;
        self.port.wrote();
        Ok(chunk.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Writer {
    /// Hands over all of `bytes` as the server's window lets them go, looking at `stop` every
    /// `look_again`. `Ok(false)` when `stop` was set first; `TimedOut` when the window took
    /// nothing for `silent`; `BrokenPipe` when the stream is over (the server closed it, the
    /// connection went, or the stream was ended from the other side).
    pub fn write_all_within(
        &mut self,
        mut bytes: &[u8],
        silent: Duration,
        look_again: Duration,
        stop: &AtomicBool,
    ) -> io::Result<bool> {
        let mut moved = Instant::now();
        while !bytes.is_empty() {
            if stop.load(Ordering::Acquire) {
                return Ok(false);
            }
            let Some(input) = &self.input else {
                return Err(io::Error::new(io::ErrorKind::BrokenPipe, "already closed"));
            };
            let size = bytes.len().min(MAX_DATA);
            let (chunk, rest) = bytes.split_at(size);
            match input.try_send(chunk.to_vec()) {
                Ok(()) => {
                    self.port.wrote();
                    bytes = rest;
                    moved = Instant::now();
                }
                Err(TrySendError::Full(_)) => {
                    if moved.elapsed() > silent {
                        return Err(io::ErrorKind::TimedOut.into());
                    }
                    std::thread::sleep(look_again);
                }
                Err(TrySendError::Disconnected(_)) => {
                    return Err(io::Error::new(
                        io::ErrorKind::BrokenPipe,
                        "the stream is over",
                    ));
                }
            }
        }
        Ok(true)
    }
}

impl Drop for Writer {
    fn drop(&mut self) {
        self.input = None;
        self.port.wrote();
    }
}

/// How the stream ended, for whoever waits on it.
pub struct End {
    shared: Arc<Shared>,
    hold: Arc<Hold>,
}

impl std::fmt::Debug for End {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("End").finish_non_exhaustive()
    }
}

impl End {
    /// Waits until the program has ended, and says how. `Err` is why the stream is over without
    /// it (the connection went, the server closed it).
    pub fn wait(&self) -> Result<Exit, String> {
        let mut ended = self.shared.ended();
        loop {
            if let Some(exit) = ended.exit {
                return Ok(exit);
            }
            if let Some(over) = &ended.over {
                return Err(over.clone());
            }
            ended = self
                .shared
                .ended_cv
                .wait(ended)
                .unwrap_or_else(|e| e.into_inner());
        }
    }

    /// How the program ended, if it has.
    pub fn try_wait(&self) -> Option<Result<Exit, String>> {
        let ended = self.shared.ended();
        if let Some(exit) = ended.exit {
            return Some(Ok(exit));
        }
        ended.over.clone().map(Err)
    }

    /// Ends the stream now: the program the server runs for it is ended. How it ended may still
    /// be asked for after.
    pub fn kill(&self) {
        self.hold.port.hang_up();
    }
}

impl Far {
    /// The server took the open: the caller's `opened` returns.
    pub(super) fn opened(&mut self) {
        if let Some(answer) = self.answer.take() {
            let _ = answer.send(Ok(()));
        }
    }

    /// The stream could not be opened, and why.
    pub(super) fn refused(&mut self, text: &str) {
        if let Some(answer) = self.answer.take() {
            let _ = answer.send(Err(text.to_owned()));
        }
        self.finish(text);
    }

    /// The caller stopped waiting for the open.
    pub(super) fn abandoned(&self) -> bool {
        self.shared.abandoned.load(Ordering::SeqCst)
    }

    /// Standard output (or a connection's bytes) arrived. `false` when nobody reads them any
    /// more: they count as taken.
    pub(super) fn out(&mut self, data: Vec<u8>) -> bool {
        self.out
            .as_ref()
            .is_some_and(|out| out.send(Ok(data)).is_ok())
    }

    /// Standard error arrived. `false` when nobody reads it.
    pub(super) fn err(&mut self, data: Vec<u8>) -> bool {
        self.err
            .as_ref()
            .is_some_and(|err| err.send(Ok(data)).is_ok())
    }

    /// The server has nothing more to send: the readers get 0.
    pub(super) fn eof(&mut self) {
        self.out = None;
        self.err = None;
    }

    /// The program ended, and how.
    pub(super) fn exited(&mut self, exit: Exit) {
        self.shared.ended().exit = Some(exit);
        self.shared.ended_cv.notify_all();
    }

    /// The stream is over. What was not delivered before the end is an error to the readers when
    /// the end was not the server's own `eof`.
    pub(super) fn over(&mut self, over: &Over) {
        let (broken, reason) = match over {
            Over::Finished => (
                false,
                "the stream ended before the program said how it ended",
            ),
            Over::Closed => (
                true,
                "the server closed the stream before the program said how it ended",
            ),
            Over::Failed { text, .. } => (true, text.as_str()),
            Over::Lost => (true, LOST),
        };
        if broken {
            for sender in [&self.out, &self.err].into_iter().flatten() {
                let _ = sender.send(Err(reason.to_owned()));
            }
        }
        if let Some(answer) = self.answer.take() {
            let _ = answer.send(Err(reason.to_owned()));
        }
        self.out = None;
        self.err = None;
        self.finish(reason);
    }

    fn finish(&mut self, reason: &str) {
        let mut ended = self.shared.ended();
        if ended.over.is_none() {
            ended.over = Some(reason.to_owned());
        }
        drop(ended);
        self.shared.ended_cv.notify_all();
    }

    /// The next chunk the caller wrote, `None` when there is none now. `Err` when the caller has
    /// nothing more to write: all its handles are dropped.
    pub(super) fn next_input(&mut self) -> Result<Option<Vec<u8>>, ()> {
        if self.input_ended {
            return Err(());
        }
        match self.input.try_recv() {
            Ok(chunk) => Ok(Some(chunk)),
            Err(mpsc::TryRecvError::Empty) => Ok(None),
            Err(mpsc::TryRecvError::Disconnected) => {
                self.input_ended = true;
                Err(())
            }
        }
    }
}

#[cfg(test)]
mod local;

#[cfg(test)]
impl RemoteCommand {
    /// `child` (piped, and the leader of its own process group) as a program the server runs.
    pub(crate) fn local(child: std::process::Child) -> RemoteCommand {
        local::run(child)
    }
}
