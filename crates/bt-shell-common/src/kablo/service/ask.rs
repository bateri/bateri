//! A pane's request for a terminal of this Mac, from the pane's side.
//!
//! A pane is born over its stream at once and the terminal comes to it when it comes
//! ([`LocalAsk::new`]): the pane has its ends of the stream and a [`Ticket`] through which what its
//! session says (a size, its end) reaches the terminal, and the service's loop is handed the other
//! ends ([`super::Service::submit_local`]) whenever it can be. What a request ends in is one of two:
//! the terminal is there and the stream carries it, or the request is refused for a reason and the
//! stream carries an ordinary shell of this process instead ([`LocalAsk::refuse`]), which the pane
//! is told of with a dim line. A request that is dropped without either is refused too: a pane
//! whose terminal nobody answers for would wait for ever, and without a word.

use std::io::{self, Read as _, Write as _};
use std::os::fd::BorrowedFd;
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Instant;

use bt_core::{
    ChildLaunch, ChildPid, Ended, Ordinary, PaneUuid, PtyOps, PtySize, Silence, Stream, StreamExit,
};
use kablo_proto::v1::NewSession;

use super::{Backend, HoldId, Load, Msg, Out, Queue, SessionId, Size, Waited, waiting, widen};

/// What a pane's session says to its terminal, before and after the terminal is there. Made by
/// the pane with its stream and kept by the loop, which binds it to the terminal's holder: until
/// then the latest size is kept and that the pane let go of its stream is noted, and the loop
/// reads both when it starts the terminal.
#[derive(Default)]
pub(super) struct Ticket {
    state: Mutex<TicketState>,
}

#[derive(Default)]
struct TicketState {
    /// The loop's holder of the pane, once the terminal is asked for.
    bound: Option<(HoldId, Sender<Msg>)>,
    /// The size the pane last said, in the terminal's units: the size of a terminal that is
    /// started after it, for every one the pane is given (a terminal found again that is not
    /// there is made anew behind the same stream).
    size: Option<Size>,
    /// The same, as the PTY's size an ordinary shell is started with and resized by.
    last: Option<PtySize>,
    /// The pane let go of its stream before the terminal was asked for or came.
    gone: bool,
    /// The terminal was refused and an ordinary shell of this process carries the stream.
    ordinary: Option<Arc<Ordinary>>,
    /// Where the service writes its log, once the request is handed to it.
    log: Option<Arc<dyn Backend>>,
    /// The person chose an ordinary shell in place of the terminal ([`PtyOps::ordinary_instead`]):
    /// kept for the loop that has not taken the request up yet.
    chose: bool,
    /// How the loop is told of the choice, once the request is handed to the service.
    wake: Option<Sender<Msg>>,
}

impl Ticket {
    fn state(&self) -> std::sync::MutexGuard<'_, TicketState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The loop starts the terminal for `hold`: the size the pane has said since it was born (if
    /// it did) and whether the pane is gone already. What is said from now on goes to the loop as
    /// a message.
    pub(super) fn bind(&self, hold: HoldId, tx: Sender<Msg>) -> (Option<Size>, bool) {
        let mut state = self.state();
        state.bound = Some((hold, tx));
        (state.size, state.gone)
    }

    /// Whether the pane let go of its stream.
    pub(super) fn is_gone(&self) -> bool {
        self.state().gone
    }

    /// Whether the person chose an ordinary shell ([`PtyOps::ordinary_instead`]).
    pub(super) fn chose(&self) -> bool {
        self.state().chose
    }

    /// The choice was not taken (the terminal had come): the next one the person makes, when the
    /// pane offers it again, is told to the loop like the first.
    pub(super) fn forget_choice(&self) {
        self.state().chose = false;
    }

    /// The ordinary shell of this process the stream is carried by from now on, put there by the
    /// loop for a stream that a holder carries ([`super::Core::choose_ordinary`]): what the pane
    /// says of its size goes to it directly, and the last size the pane said is returned.
    pub(super) fn put_ordinary(&self, shell: &Arc<Ordinary>) -> Option<PtySize> {
        let mut state = self.state();
        state.ordinary = Some(Arc::clone(shell));
        state.last
    }
}

/// What a pane of this Mac asks of its terminal ([`PtyOps`]).
pub(super) struct LocalOps {
    pub(super) ticket: Arc<Ticket>,
    pub(super) trace: Arc<waiting::Trace>,
    pub(super) pane: PaneUuid,
    pub(super) pid: ChildPid,
}

impl PtyOps for LocalOps {
    fn resize(&self, _master: BorrowedFd<'_>, size: PtySize) {
        let pixels = |cells: u16, each: u16| u16::try_from(u32::from(cells) * u32::from(each));
        let size_of = Size {
            cols: size.cols,
            rows: size.rows,
            xpixel: pixels(size.cols, size.cell_width).unwrap_or(0),
            ypixel: pixels(size.rows, size.cell_height).unwrap_or(0),
        };
        let mut state = self.ticket.state();
        state.size = Some(size_of);
        state.last = Some(size);
        if let Some(shell) = &state.ordinary {
            shell.resize(size);
        } else if let Some((hold, tx)) = &state.bound {
            let _ = tx.send(Msg::Resize {
                hold: *hold,
                size: size_of,
            });
        }
    }

    /// The session lets go of its stream, and the terminal lives on: ending it is a request of
    /// its own (the user's close of the pane, a quit that ends the programs), made before the
    /// session goes. The stream closing is what the service takes for the pane's being gone
    /// ([`Msg::HoldEof`]), and the terminal is let go of: detached, and shown again by the next
    /// bateri that finds it in the ledger. A pane that lets go before its terminal is asked for
    /// or has come is noted, so that the loop does not start one for nobody, and ends the one
    /// that was already asked for when it comes.
    fn hangup(&self, _pid: Option<u32>, _exit: BorrowedFd<'_>) {
        self.ticket.state().gone = true;
    }

    /// The pane has waited long enough to say so, and the terminal has not come: one line to the
    /// log with the step the wait is in and the ones it has been through — what is known of a
    /// terminal that never comes, which no other line is written for.
    fn still_waiting(&self) {
        if self.pid.is_settled() {
            return;
        }
        let line = self.trace.waiting_line(self.pane.as_str(), Instant::now());
        // The ticket's lock is not held while the line is written.
        let log = self.ticket.state().log.clone();
        match log {
            Some(backend) => backend.log_line(&line),
            None => eprintln!("{} bateri: {line}", crate::clock::stamp()),
        }
    }

    /// The pane's program ended without reading what the pane sent it: the terminal that
    /// holds it is asked to drop it ([`Client::drop_input`]) — the terminal of this Mac, or,
    /// when the program ran on a server the pane shows, that server's, **behind what the
    /// pipe to it had passed on** ([`Core::drop_shown`]). The request is not ordered with the
    /// bytes the pane wrote a moment before: they reach the service from the stream's reader
    /// thread and the request from this one, so a byte still on its way follows the request
    /// and is not dropped. The window is narrow, for the end of the program that triggers the
    /// request comes back from the terminal after a round trip. A terminal that is not there yet
    /// has nothing to drop.
    fn drop_input(&self, remote: bool) {
        let state = self.ticket.state();
        if let Some((hold, tx)) = &state.bound {
            let _ = tx.send(Msg::DropInput {
                hold: *hold,
                remote,
            });
        }
    }

    /// The person chose an ordinary shell while the terminal has not come: the loop is told if
    /// the request is its own already, and finds the choice in the ticket when it takes the
    /// request up if it is not.
    fn ordinary_instead(&self) {
        let mut state = self.ticket.state();
        if std::mem::replace(&mut state.chose, true) {
            return;
        }
        if let Some(wake) = &state.wake {
            let _ = wake.send(Msg::Ordinary(Arc::clone(&self.ticket)));
        }
    }
}

/// A pane's request for a terminal of this Mac. Dropping it without having been answered refuses
/// it ([`LocalAsk::refuse`]) unless the service is stopping.
pub struct LocalAsk {
    parts: Option<Parts>,
    /// Set when the request is handed to a service: whether it is stopping, and where to write.
    handed: Option<(Arc<AtomicBool>, Arc<dyn Backend>)>,
}

/// What the loop starts a terminal from.
pub(super) struct Parts {
    pub(super) pane: PaneUuid,
    pub(super) spec: NewSession,
    pub(super) size: Size,
    /// What an ordinary shell for the pane is started with.
    pub(super) launch: ChildLaunch,
    /// The service's end of the pane's stream and of its exit signal.
    /// `None` once a holder of the loop carries the stream ([`super::Core::carry_local`]).
    pub(super) held: Option<UnixStream>,
    pub(super) exit: Option<UnixStream>,
    pub(super) status: Arc<StreamExit>,
    pub(super) pid: ChildPid,
    pub(super) silence: Option<Arc<Silence>>,
    pub(super) ticket: Arc<Ticket>,
    /// What the wait has been through, for the log ([`waiting`]).
    pub(super) trace: Arc<waiting::Trace>,
    /// The name its terminal is to have: the number a program that opened the pane was told
    /// ([`super::Service::expect_name`]), or the counter's ([`super::Core::local_numbered`]).
    pub(super) name: Option<SessionId>,
}

impl LocalAsk {
    /// The pane's stream, at once, and the request to hand to the service. `shown_again` says the
    /// pane's terminal is one the ledger has for it, which is to be found again: its stream starts
    /// with a past that the service fills ([`Stream::silence`]) or closes empty; a stream without
    /// one is a new terminal's, every byte of which is live.
    pub fn new(
        pane: &PaneUuid,
        spec: NewSession,
        size: Size,
        launch: ChildLaunch,
        shown_again: bool,
    ) -> io::Result<(Stream, LocalAsk)> {
        let (held, pane_end) = UnixStream::pair()?;
        let (exit_held, exit_end) = UnixStream::pair()?;
        pane_end.set_nonblocking(true)?;
        // The stream is read in bursts: a buffer of the system's default size is a wake-up per
        // eight kilobytes.
        widen(&held);
        widen(&pane_end);
        let ticket = Arc::new(Ticket::default());
        let status = Arc::new(StreamExit::default());
        let pid = ChildPid::unknown();
        let silence = shown_again.then(Silence::new);
        let trace = Arc::new(waiting::Trace::new(Instant::now()));
        let stream = Stream {
            master: pane_end.into(),
            exit: exit_end.into(),
            status: Arc::clone(&status),
            pid: pid.clone(),
            ops: Arc::new(LocalOps {
                ticket: Arc::clone(&ticket),
                trace: Arc::clone(&trace),
                pane: pane.clone(),
                pid: pid.clone(),
            }),
            silence: silence.clone(),
        };
        let parts = Parts {
            pane: pane.clone(),
            spec,
            size,
            launch,
            held: Some(held),
            exit: Some(exit_held),
            status,
            pid,
            silence,
            ticket,
            trace,
            name: None,
        };
        Ok((
            stream,
            LocalAsk {
                parts: Some(parts),
                handed: None,
            },
        ))
    }

    /// The service takes the request: it is refused with the service's words if it is dropped.
    pub(super) fn handed(
        &mut self,
        stop: Arc<AtomicBool>,
        backend: Arc<dyn Backend>,
        wake: Sender<Msg>,
    ) {
        if let Some(parts) = &self.parts {
            let mut state = parts.ticket.state();
            state.log = Some(Arc::clone(&backend));
            state.wake = Some(wake);
        }
        self.handed = Some((stop, backend));
    }

    /// The request's parts, for the loop; what is left answers for nothing.
    pub(super) fn take(&mut self) -> Option<Parts> {
        self.parts.take()
    }

    /// The pane the request is for.
    pub(super) fn pane(&self) -> Option<&PaneUuid> {
        self.parts.as_ref().map(|parts| &parts.pane)
    }

    /// The loop names the terminal the pane is to have.
    pub(super) fn named(&mut self, name: Option<SessionId>) {
        if let Some(parts) = &mut self.parts {
            parts.name = name;
        }
    }

    /// The pane is not given a terminal, for `why`: an ordinary shell of this process carries its
    /// stream instead and the pane is told in a dim line. Logged with the steps the wait went
    /// through.
    pub fn refuse(mut self, why: &str) {
        if let Some(parts) = self.parts.take() {
            let log = self.handed.as_ref().map(|(_, backend)| Arc::clone(backend));
            refuse(parts, why, log.as_deref());
        }
    }
}

impl Drop for LocalAsk {
    fn drop(&mut self) {
        let Some(parts) = self.parts.take() else {
            return;
        };
        if let Some((stop, backend)) = &self.handed {
            if stop.load(Ordering::SeqCst) {
                return;
            }
            refuse(
                parts,
                "the request was dropped before it was answered",
                Some(backend.as_ref()),
            );
        } else {
            refuse(
                parts,
                "the request was dropped before it was answered",
                None,
            );
        }
    }
}

/// What a pane that chose an ordinary shell over its terminal is told in the dim line, and what
/// the log says: the person's own choice.
pub(super) const CHOSEN: &str = "you chose it instead of waiting";

/// The dim line a pane is told its terminal was refused in.
pub(super) fn refusal_line(why: &str) -> Vec<u8> {
    format!(
        "\x1b[2mbateri: this is an ordinary shell and ends when bateri quits ({why})\x1b[22m\r\n"
    )
    .into_bytes()
}

/// Gives the pane an ordinary shell of this process in place of its terminal: logged, said to the
/// pane in a dim line, and carried over its stream. A shell that cannot be started ends the pane's
/// stream with how, as a program that failed to start does.
pub(super) fn refuse(parts: Parts, why: &str, log: Option<&dyn Backend>) {
    // The pane let go of its stream while it waited: nobody is told, and no shell is started
    // for nobody.
    if parts.ticket.is_gone() {
        if let Some(silence) = &parts.silence {
            silence.close();
        }
        parts.status.made_ordinary();
        parts.pid.settle();
        return;
    }
    let now = Instant::now();
    parts.trace.refuse(why);
    let line = parts.trace.refused_line(parts.pane.as_str(), why, now);
    match log {
        Some(backend) => backend.log_line(&line),
        None => eprintln!("{} bateri: {line}", crate::clock::stamp()),
    }
    ordinary_behind(parts.into_behind(), why);
}

/// What an ordinary shell behind a pane's stream is made of: the service's ends of the stream and
/// of its exit signal, and what the request shares with the pane.
pub(super) struct Behind {
    launch: ChildLaunch,
    held: Option<UnixStream>,
    exit: Option<UnixStream>,
    status: Arc<StreamExit>,
    pid: ChildPid,
    silence: Option<Arc<Silence>>,
    ticket: Arc<Ticket>,
}

impl Parts {
    /// The request ends here: all of it goes to the shell.
    fn into_behind(self) -> Behind {
        Behind {
            launch: self.launch,
            held: self.held,
            exit: self.exit,
            status: self.status,
            pid: self.pid,
            silence: self.silence,
            ticket: self.ticket,
        }
    }

    /// The request goes on waiting for its terminal (a new one is asked of the server and will
    /// come to nobody): the shell takes the ends of the stream and shares the rest.
    pub(super) fn lend_behind(&mut self) -> Behind {
        Behind {
            launch: self.launch.clone(),
            held: self.held.take(),
            exit: self.exit.take(),
            status: Arc::clone(&self.status),
            pid: self.pid.clone(),
            silence: self.silence.clone(),
            ticket: Arc::clone(&self.ticket),
        }
    }

    /// What a stream a holder carries is given an ordinary shell with ([`ordinary_carried`]).
    pub(super) fn lend_carried(&self) -> Behind {
        Behind {
            launch: self.launch.clone(),
            held: None,
            exit: None,
            status: Arc::clone(&self.status),
            pid: self.pid.clone(),
            silence: self.silence.clone(),
            ticket: Arc::clone(&self.ticket),
        }
    }
}

/// Gives the stream a shell of this process as [`refuse`] does, to a request that goes on
/// waiting for its terminal (the choice of the person, [`PtyOps::ordinary_instead`]).
pub(super) fn ordinary_behind(behind: Behind, why: &str) {
    let Behind {
        mut launch,
        held,
        exit,
        status,
        pid,
        silence,
        ticket,
    } = behind;
    status.made_ordinary();
    // What the pane was waiting to hear of its past is nothing: every byte of the shell is live.
    if let Some(silence) = &silence {
        silence.close();
    }
    let (Some(held), Some(exit)) = (held, exit) else {
        return;
    };
    let mut output = match held.try_clone() {
        Ok(output) => output,
        Err(_) => return,
    };
    let _ = output.write_all(&refusal_line(why));
    let ended = {
        let status = Arc::clone(&status);
        let output = output.try_clone();
        let mut exit = Some(exit);
        move |how: Ended| {
            match how {
                Ended::Code(code) => status.code(code),
                Ended::Signal(signal) => status.signal(signal),
            }
            // The pane has read every byte there is when it hears its program is gone.
            if let Ok(output) = &output {
                let _ = output.shutdown(std::net::Shutdown::Both);
            }
            drop(exit.take());
        }
    };
    // The size the pane has now, not the one it was born with.
    let size_at_start = ticket.state().last;
    if let Some(size) = size_at_start {
        launch.size = size;
    }
    let shell = {
        let output = match output.try_clone() {
            Ok(output) => output,
            Err(_) => return,
        };
        let output = Mutex::new(output);
        Ordinary::start(
            &launch,
            move |bytes| {
                let _ = output
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .write_all(&bytes);
            },
            ended,
        )
    };
    match shell {
        Ok(shell) => {
            let shell = Arc::new(shell);
            {
                // What the pane said while the shell was starting is said to it now, and from
                // here on to the shell directly.
                let mut state = ticket.state();
                state.ordinary = Some(Arc::clone(&shell));
                if let Some(size) = state.last.filter(|size| Some(*size) != size_at_start) {
                    shell.resize(size);
                }
            }
            pid.set(shell.pid());
            // What the pane types goes to the shell until the pane lets go of its stream, and
            // then the shell is hung up.
            let mut input = held;
            let _ = thread::Builder::new()
                .name("an ordinary pane is typed into".into())
                .spawn(move || {
                    let mut buffer = vec![0u8; 16 * 1024];
                    loop {
                        match input.read(&mut buffer) {
                            Ok(0) | Err(_) => break,
                            Ok(n) => shell.write(buffer[..n].to_vec()),
                        }
                    }
                });
        }
        Err(error) => {
            let _ = output.write_all(
                format!("\x1b[2mbateri: no shell could be started: {error}\x1b[0m\r\n").as_bytes(),
            );
            status.code(1);
            let _ = output.shutdown(std::net::Shutdown::Both);
            // The pane does not wait for a process that will not come.
            pid.settle();
        }
    }
}

/// An ordinary shell that carries a holder's stream ([`ordinary_carried`]), and what it has written
/// and the pane has not read. Dropping it (the holder is gone) lets a shell that waits for its pane
/// go on to its end.
pub(super) struct Carried {
    pub(super) shell: Arc<Ordinary>,
    queue: Arc<Queue>,
}

impl Drop for Carried {
    fn drop(&mut self) {
        self.queue.close();
    }
}

/// Gives a stream that a holder of the loop carries an ordinary shell of this process: what the
/// shell writes goes to the holder's writer (`out`), and how it ended is written in the stream's
/// status before the writer is told to close, so that the pane has read every byte when it hears
/// its program is gone. What the pane types is the loop's to give the shell it returns
/// ([`super::Core::choose_ordinary`]). `None` when no shell could be started: the pane is told.
///
/// The shell writes no faster than the pane reads, as a program of a terminal does: what it has
/// written and the pane has not taken is counted, and past [`super::HIGH_WATER`] the shell's
/// terminal is not read until the pane has taken some, so that its program waits for the pane and
/// a pane that reads nothing does not fill the memory of the process.
///
/// The pane may have been shown something before the shell (a screen a program left on the
/// alternate screen, its mouse reporting on): the terminal's modes are set back first
/// ([`bt_core::reset_modes`]), what it showed staying where it is.
pub(super) fn ordinary_carried(behind: Behind, out: Sender<Out>, why: &str) -> Option<Carried> {
    let Behind {
        mut launch,
        status,
        pid,
        silence,
        ticket,
        ..
    } = behind;
    status.made_ordinary();
    if let Some(silence) = &silence {
        silence.close();
    }
    // What the shell has written and the pane has not taken, counted apart from the account's
    // terminals: a shell that waits for its pane holds nobody else back.
    let queue = Arc::new(Queue::new(false));
    let load = Load::new();
    let say = {
        let (queue, load) = (Arc::clone(&queue), Arc::clone(&load));
        move |out: &Sender<Out>, bytes: Vec<u8>| {
            load.add(bytes.len(), &queue);
            out.send(Out::Bytes(bytes, Arc::clone(&load))).is_ok()
        }
    };
    let mut first = bt_core::reset_modes();
    first.extend_from_slice(&refusal_line(why));
    let _ = say(&out, first);
    if let Some(size) = ticket.state().last {
        launch.size = size;
    }
    let shell = {
        let (output, closing, status) = (out.clone(), out.clone(), Arc::clone(&status));
        let (say, queue) = (say.clone(), Arc::clone(&queue));
        Ordinary::start(
            &launch,
            move |bytes| {
                // Past the mark the shell's terminal is not read until the pane has taken some, or
                // until the holder is gone and nobody waits for what it writes.
                if say(&output, bytes) {
                    let _: Waited = queue.wait();
                }
            },
            move |how: Ended| {
                match how {
                    Ended::Code(code) => status.code(code),
                    Ended::Signal(signal) => status.signal(signal),
                }
                let _ = closing.send(Out::Close);
            },
        )
    };
    match shell {
        Ok(shell) => {
            let shell = Arc::new(shell);
            // What the pane said while the shell was starting is said to it now.
            if let Some(size) = ticket.put_ordinary(&shell)
                && size != launch.size
            {
                shell.resize(size);
            }
            pid.set(shell.pid());
            Some(Carried { shell, queue })
        }
        Err(error) => {
            let _ = say(
                &out,
                format!("\x1b[2mbateri: no shell could be started: {error}\x1b[0m\r\n")
                    .into_bytes(),
            );
            status.code(1);
            let _ = out.send(Out::Close);
            pid.settle();
            None
        }
    }
}
