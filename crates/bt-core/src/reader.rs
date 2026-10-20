//! The PTY reader loop: I/O, parsing and the write queue.
//!
//! **This file is a copy.** Source: `alacritty_terminal` 0.26.0,
//! `src/event_loop.rs` — Copyright Christian Duerr, Joe Wilm and the
//! alacritty contributors, under the Apache License 2.0 (text in the bundle
//! as `THIRD-PARTY-LICENSES.txt`, attribution in `Credits.html`). Per
//! Apache-2.0 §4(b): the file was **modified**, the changes
//! being —
//!
//! - the parser sees `Term` not directly but through [`ClusterHandler`], both
//!   in `advance` and in `stop_sync` on the DEC 2026 timeout;
//! - the `ref_test` recording and `Notifier` were removed (nothing calls
//!   either);
//! - `log::error!` lines became `eprintln!` (`log` is not a dependency of
//!   this crate; logging is still a debt);
//! - a dead channel is an empty read, not a panic (no unjustified panic in
//!   `bt-core`; the branch is unreachable); the re-registration panic was
//!   kept with its justification;
//! - the PTY tokens are `pub(crate)` in alacritty, their values were copied
//!   here (and are `pub(crate)` here too: an adopted PTY registers on them);
//! - comments were translated (first to Turkish, later to English);
//! - an image a graphics command carries is decoded — and read, when the
//!   command names a file or shared memory object — with the terminal's lease
//!   and lock **let go** ([`crate::graphics::Hub::decode_pending`]) — a frame
//!   waits for the lease as well as the lock, so decoding under either would
//!   hold it back. The parser stops right behind such a command
//!   ([`ansi::Processor::advance_until_halt`]), the decoding is done, and the
//!   parse goes on from the byte after it: the bytes are parsed in order
//!   whatever the decoding takes, and the journal counts them as they are;
//! - the update handover: an opt-in read before the first poll
//!   ([`EventLoop::read_first`], so a carried prefix reaches the parser on
//!   a quiet PTY) and, on the loop handed back after `join`, the DEC 2026
//!   buffer applied through the wrapper ([`EventLoop::stop_sync`]), the
//!   PTY borrowed ([`EventLoop::pty_mut`]) and the input not yet written taken
//!   ([`EventLoop::unsent`]); `pty_read` returns the bytes it processed and,
//!   during that first read, waits for the terminal lock instead of reading
//!   on (every read is parsed before the next, so a carried prefix and the
//!   master's bytes never share one `advance` — the adopted session drops
//!   the prefix's replies, `Session::adopt`).
//!
//! Why a copy: grapheme clustering has to step **in between** the parser's
//! `Handler` calls, and alacritty's loop hands `Term` over as a fixed type.
//! The version is pinned with `=0.26.0` for that reason (root `Cargo.toml`).
//!
//! Preserved contracts — `session.rs`'s lock order and shutdown lean on
//! them: the terminal lease is held for the whole of `pty_read` but for that
//! decode (the `term` → `shell` order in the module header, which the
//! decode does not touch), the locked read is bounded by
//! [`MAX_LOCKED_READ`], `Wakeup` is sent only if unsynchronized bytes were
//! processed, and [`EventLoop::spawn`] returns the `(EventLoop, State)` pair
//! — the `Pty` drops, hence `SIGHUP`, when that pair drops.

use std::borrow::Cow;
use std::collections::VecDeque;
use std::io::{self, ErrorKind, Read, Write};
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::JoinHandle;
use std::time::Instant;

use alacritty_terminal::event::{Event, EventListener, OnResize, WindowSize};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::Term;
use alacritty_terminal::vte::ansi::{self, Handler as _};
use alacritty_terminal::{thread, tty};
use polling::{Event as PollingEvent, Events, PollMode, Poller};

use crate::graphics::{Commands, Hub, Sink};
use crate::handler::ClusterHandler;
use crate::journal::{Journal, Side};

/// The most bytes read from the PTY before a forced synchronization.
pub(crate) const READ_BUFFER_SIZE: usize = 0x10_0000;

/// The most bytes read from the PTY while the terminal is locked.
pub(crate) const MAX_LOCKED_READ: usize = u16::MAX as usize;

/// Poller token of the read/write fd (alacritty 0.26.0, `tty/unix.rs`,
/// `pub(crate)`). `Pty::register` does the registration, so the number must
/// equal the one given there — the version pin is its guard.
pub(crate) const PTY_READ_WRITE_TOKEN: usize = 0;

/// Poller token of the child-event pipe (same source, same reason).
pub(crate) const PTY_CHILD_EVENT_TOKEN: usize = 1;

/// Messages sent to the loop.
#[derive(Debug)]
pub(crate) enum Msg {
    /// Bytes to write to the PTY.
    Input(Cow<'static, [u8]>),

    /// The loop must shut down.
    Shutdown,

    /// The PTY must be resized.
    Resize(WindowSize),

    /// The journal's compaction made room (or the journal broke): the
    /// read gate is asked again. A channel message, not a bare poller
    /// wake: an empty wake with an empty channel is the DEC 2026 timeout's
    /// arm, which would apply a synchronized update early and ask for a
    /// frame. Sent only to a loop that has a journal.
    Room,
}

/// Sending end of the loop's channel: queues the message and wakes the
/// poller.
#[derive(Clone)]
pub(crate) struct EventLoopSender {
    sender: Sender<Msg>,
    poller: Arc<Poller>,
}

impl EventLoopSender {
    /// `Err` if the receiver dropped or the poller could not be woken; the
    /// caller's only question is "did it go through", no detail is carried.
    pub(crate) fn send(&self, msg: Msg) -> Result<(), ()> {
        self.sender.send(msg).map_err(|_| ())?;
        self.poller.notify().map_err(|_| ())
    }
}

/// Tracks how much of a buffer has been written.
struct Writing {
    source: Cow<'static, [u8]>,
    written: usize,
}

impl Writing {
    #[inline]
    fn new(c: Cow<'static, [u8]>) -> Writing {
        Writing {
            source: c,
            written: 0,
        }
    }

    #[inline]
    fn advance(&mut self, n: usize) {
        self.written += n;
    }

    #[inline]
    fn remaining_bytes(&self) -> &[u8] {
        &self.source[self.written..]
    }

    #[inline]
    fn finished(&self) -> bool {
        self.written >= self.source.len()
    }
}

/// All of the loop's mutable state: the write queue, the buffer being
/// written and the parser.
#[derive(Default)]
pub(crate) struct State {
    write_list: VecDeque<Cow<'static, [u8]>>,
    writing: Option<Writing>,
    parser: ansi::Processor,
    /// Whether the last `Handler` call was `input` — [`ClusterHandler`]'s
    /// only state. It lives here because the wrapper is reborn on every
    /// `advance`: a cluster split across two `read` chunks (`👍` · `🏽`) is
    /// still unclosed.
    last_input: bool,
    /// The graphics commands of the stream ([`Commands`]): the wrapper is
    /// reborn on every `advance`, a command is cut by the reads.
    commands: Commands,
}

impl State {
    #[inline]
    fn ensure_next(&mut self) {
        if self.writing.is_none() {
            self.goto_next();
        }
    }

    #[inline]
    fn goto_next(&mut self) {
        self.writing = self.write_list.pop_front().map(Writing::new);
    }

    #[inline]
    fn take_current(&mut self) -> Option<Writing> {
        self.writing.take()
    }

    #[inline]
    fn needs_write(&self) -> bool {
        self.writing.is_some() || !self.write_list.is_empty()
    }

    #[inline]
    fn set_current(&mut self, new: Option<Writing>) {
        self.writing = new;
    }
}

/// A receiver that can peek at the next message: the timeout arm asks "is
/// there a message in the channel" without consuming it.
struct PeekableReceiver<T> {
    rx: Receiver<T>,
    peeked: Option<T>,
}

impl<T> PeekableReceiver<T> {
    fn new(rx: Receiver<T>) -> Self {
        Self { rx, peeked: None }
    }

    fn peek(&mut self) -> Option<&T> {
        if self.peeked.is_none() {
            self.peeked = self.rx.try_recv().ok();
        }

        self.peeked.as_ref()
    }

    fn recv(&mut self) -> Option<T> {
        // alacritty panics when the channel dies (`Disconnected`). That
        // branch is unreachable here — one of the sender ends
        // (`EventLoop::tx`) is the loop's own field, so the channel does not
        // die while the loop lives — and the no-panic rule reduces it to an
        // empty read.
        self.peeked.take().or_else(|| self.rx.try_recv().ok())
    }
}

/// The reader loop: the PTY I/O and the parser that updates `Term`.
pub(crate) struct EventLoop<T: tty::EventedPty, U: EventListener> {
    poll: Arc<Poller>,
    pty: T,
    rx: PeekableReceiver<Msg>,
    tx: Sender<Msg>,
    terminal: Arc<FairMutex<Term<U>>>,
    event_proxy: U,
    drain_on_exit: bool,
    /// Whether clustering is on (`SessionOptions::cluster`); passed to the
    /// wrapper on every call.
    cluster: bool,
    /// How many carried bytes the thread reads before its first
    /// `poll.wait` ([`EventLoop::read_first`]); `None`: none.
    read_first: Option<usize>,
    /// The session's journal ([`EventLoop::journal`]); `None`: none, and
    /// the loop runs as alacritty's does.
    journal: Option<Arc<Journal>>,
    /// The session's graphics ([`EventLoop::graphics`]); `None`: none, and
    /// the graphics commands the stream may carry only move the cursor.
    graphics: Option<Arc<dyn Sink>>,
}

/// The read gate's state between two loop rounds: while closed, since when
/// the compaction's progress last moved and what it read then.
#[derive(Default)]
struct Gate {
    stalled: Option<(Instant, u64)>,
}

/// Whether an empty wake with an empty channel is the DEC 2026 timeout.
/// Without a journal it always is — alacritty's arm, kept as it was (a wake
/// whose message an earlier round already drained lands here too, and
/// applies a pending update early). With one, only once the update's own
/// deadline has passed: the poll also wakes for the read gate's stall
/// deadline, and every `stop_sync` is a side record — even one with nothing
/// pending closes the open cluster — so a spurious one would apply what
/// the application is still writing, ask for a frame and fill the side
/// stream.
fn sync_due(sync: Option<Instant>, journaled: bool, now: Instant) -> bool {
    !journaled || sync.is_some_and(|deadline| deadline <= now)
}

impl<T, U> EventLoop<T, U>
where
    T: tty::EventedPty + OnResize + Send + 'static,
    U: EventListener + Send + 'static,
{
    pub(crate) fn new(
        terminal: Arc<FairMutex<Term<U>>>,
        event_proxy: U,
        pty: T,
        drain_on_exit: bool,
        cluster: bool,
    ) -> io::Result<EventLoop<T, U>> {
        let (tx, rx) = mpsc::channel();
        let poll = Poller::new()?.into();
        Ok(EventLoop {
            poll,
            pty,
            tx,
            rx: PeekableReceiver::new(rx),
            terminal,
            event_proxy,
            drain_on_exit,
            cluster,
            read_first: None,
            journal: None,
            graphics: None,
        })
    }

    /// Applies the graphics commands the stream carries ([`crate::graphics`]):
    /// the parser's wrapper applies them to `graphics` where they end.
    pub(crate) fn graphics(&mut self, graphics: Arc<dyn Sink>) {
        self.graphics = Some(graphics);
    }

    /// Records the PTY into `journal`: the bytes `advance` applies are
    /// counted, the DEC 2026 timeout is recorded, and reading stops while
    /// the journal has no room for one more read ([`Journal::gate_open`]).
    pub(crate) fn journal(&mut self, journal: Arc<Journal>) {
        self.journal = Some(journal);
    }

    /// Whether the journal has room for the next read — always without a
    /// journal.
    fn gate_open(&self) -> bool {
        self.journal
            .as_ref()
            .is_none_or(|journal| journal.gate_open())
    }

    /// The read gate, before the next poll: closes the PTY's read interest
    /// while the journal has no room for a whole `pty_read` (the poll is
    /// level-triggered — a readable fd left registered would spin the
    /// loop) and opens it again once it has. While closed it asks the
    /// compaction to run and watches its progress: none for the journal's
    /// stall time breaks the journal and opens the gate, so a dead or stuck
    /// compaction cannot hold the pane. Returns the stall deadline while
    /// closed. Without a journal nothing happens and `None` comes back.
    fn gate(
        &mut self,
        gate: &mut Gate,
        interest: &mut PollingEvent,
        poll_opts: PollMode,
    ) -> Option<Instant> {
        let journal = Arc::clone(self.journal.as_ref()?);
        let mut open = journal.gate_open();
        if open {
            gate.stalled = None;
        } else {
            let now = Instant::now();
            let progress = journal.progress();
            match gate.stalled {
                Some((since, seen)) if seen == progress => {
                    if now >= since + journal.stall() {
                        eprintln!(
                            "bateri: the journal's compaction made no progress, the journal breaks"
                        );
                        journal.break_journal();
                        gate.stalled = None;
                        open = true;
                    }
                }
                _ => {
                    gate.stalled = Some((now, progress));
                    Journal::poke(&journal);
                }
            }
        }
        if open != interest.readable {
            interest.readable = open;
            // The same deliberate panic as the write interest's below.
            if let Err(err) = self.pty.reregister(&self.poll, *interest, poll_opts) {
                panic!("reader loop re-registration failed: {err}"); // audit: alacritty parity, the crash reaches the shutdown report through join's Err
            }
        }
        gate.stalled.map(|(since, _)| since + journal.stall())
    }

    /// The carried prefix's serial reads ([`EventLoop::read_first`]): `left`
    /// counts down as bytes go through and ends at zero when a round reads
    /// nothing or fails. A closed read gate stops the rounds with `left`
    /// kept; the loop calls this again once the gate opens — the prefix does
    /// not make the fd readable, so a quiet PTY would otherwise keep the
    /// rest until its next output.
    fn read_prefix(&mut self, state: &mut State, buf: &mut [u8], left: &mut usize) {
        while *left > 0 {
            if !self.gate_open() {
                return;
            }
            match self.pty_read(state, buf, true) {
                Ok(0) => *left = 0,
                Ok(processed) => *left = left.saturating_sub(processed),
                Err(err) => {
                    eprintln!("bateri: the first PTY read failed: {err}");
                    *left = 0;
                }
            }
        }
    }

    /// Reads once before the first `poll.wait`: an adopted PTY's
    /// carried prefix (`TappedPty`) must reach **this** loop's parser — it
    /// may end inside a sequence — and a quiet PTY gives no readable event
    /// to carry it. The reads stop once `prefix` bytes went through (a
    /// round may take some of the fd's too) — **not** when a round comes
    /// back short: a program writing without pause (`yes`, a build held at
    /// the holder's limit) fills every round, and the loop would never
    /// reach the channel (keys, resizes, the shutdown).
    ///
    /// These reads wait for the terminal lock rather than reading on while
    /// it is busy: each read is parsed before the next one, so no `advance`
    /// mixes the prefix's last bytes with the master's first — what lets the
    /// reader tell the prefix's replies from the live ones (`TappedPty`).
    pub(crate) fn read_first(&mut self, prefix: usize) {
        self.read_first = Some(prefix);
    }

    /// The PTY the loop owned — after `join`, when nothing reads it.
    pub(crate) fn pty_mut(&mut self) -> &mut T {
        &mut self.pty
    }

    /// Applies a pending DEC 2026 buffer to `Term` through the wrapper.
    /// Meant for the loop handed back after `join` (the handover's freeze):
    /// the snapshot must see what the application already sent. Graphics
    /// commands in the buffer only move the cursor here, as they do in the
    /// journal's replay: the images do not cross an update, nothing decodes
    /// while the freeze holds the terminal, and a query in the buffer gets
    /// no answer (the one thing an update that is still open at the freeze
    /// loses).
    pub(crate) fn stop_sync(&mut self, state: &mut State) {
        if let Some(graphics) = self.graphics.as_deref() {
            // The cell the movement is made from: the session's, as the
            // commands of the buffer would have found it.
            state.commands.set_cell(graphics.cell());
        }
        let mut terminal = self.terminal.lock();
        let mut handler = ClusterHandler::new(&mut terminal, self.cluster, &mut state.last_input)
            .commands(&mut state.commands);
        if let Some(graphics) = self.graphics.as_deref() {
            handler = handler.kept_modes(graphics.kept_modes());
        }
        state.parser.stop_sync(&mut handler);
    }

    /// The DEC 2026 timeout: the parser's buffer applied, through the wrapper
    /// and with the terminal locked — but for the decoding of an image in it,
    /// which is done with the parser halted behind the command, as
    /// [`EventLoop::pty_read`] does. `journaled` is called in the lock round
    /// the stop begins in, before any halt: the replay applies the buffer
    /// after exactly the bytes applied so far.
    fn stop_timed_out_sync(&self, state: &mut State, journaled: impl FnOnce()) {
        let fair = &*self.terminal;
        let mut lease = Some(fair.lease());
        let mut terminal = None;
        let mut halted = {
            let guard = terminal.get_or_insert_with(|| fair.lock_unfair());
            let mut handler =
                ClusterHandler::new(&mut **guard, self.cluster, &mut state.last_input)
                    .graphics(self.graphics.as_deref())
                    .commands(&mut state.commands);
            state.parser.stop_sync_until_halt(&mut handler)
        };
        journaled();
        while halted {
            let Some(graphics) = self.graphics.as_deref() else {
                break;
            };
            settle_halt(graphics.hub(), &mut lease, &mut terminal, || {
                let lease = fair.lease();
                (lease, fair.lock_unfair())
            });
            let guard = terminal.get_or_insert_with(|| fair.lock_unfair());
            let mut handler =
                ClusterHandler::new(&mut **guard, self.cluster, &mut state.last_input)
                    .graphics(self.graphics.as_deref())
                    .commands(&mut state.commands);
            handler.resume();
            // What is left of the replay (nothing new is given).
            let _ = state.parser.advance_until_halt(&mut handler, &[]);
            halted = handler.halt_requested();
        }
        // The update is over: what the reads noted of the commands in it has
        // been taken.
        if let Some(graphics) = self.graphics.as_deref()
            && !state.commands.receiving()
            && state.parser.sync_bytes_count() == 0
        {
            graphics.hub().forget_remote();
        }
    }

    /// The input bytes the loop never wrote, in order: the buffer being
    /// written, the queue behind it and the `Input` messages still in the
    /// channel (the `Shutdown` arm stops draining at itself). Meant for the
    /// loop handed back after `join`; a resize left in the channel is
    /// dropped — the adopting side sizes the PTY itself.
    pub(crate) fn unsent(&mut self, state: &mut State) -> Vec<u8> {
        let mut bytes = Vec::new();
        if let Some(writing) = state.writing.take() {
            bytes.extend_from_slice(writing.remaining_bytes());
        }
        for queued in state.write_list.drain(..) {
            bytes.extend_from_slice(&queued);
        }
        while let Some(msg) = self.rx.recv() {
            if let Msg::Input(input) = msg {
                bytes.extend_from_slice(&input);
            }
        }
        bytes
    }

    pub(crate) fn channel(&self) -> EventLoopSender {
        EventLoopSender {
            sender: self.tx.clone(),
            poller: self.poll.clone(),
        }
    }

    /// Drains the channel; `false` if `Shutdown` arrived.
    fn drain_recv_channel(&mut self, state: &mut State) -> bool {
        while let Some(msg) = self.rx.recv() {
            match msg {
                Msg::Input(input) => state.write_list.push_back(input),
                Msg::Resize(window_size) => self.pty.on_resize(window_size),
                // Only a wake: the gate is asked at the top of the round.
                Msg::Room => {}
                Msg::Shutdown => return false,
            }
        }

        true
    }

    /// `serial`: wait for the terminal lock after a read instead of reading
    /// on into the same buffer ([`EventLoop::read_first`]'s rounds).
    #[inline]
    fn pty_read(&mut self, state: &mut State, buf: &mut [u8], serial: bool) -> io::Result<usize> {
        let mut unprocessed = 0;
        let mut processed = 0;

        // Reserve the next terminal lock for the PTY read. The lease is held
        // for the whole read and `TappedPty::read` runs under it —
        // `session.rs`'s `term` → `shell` lock order depends on this. The one
        // time it is let go is to decode an image (below).
        let fair = &*self.terminal;
        let mut lease = Some(fair.lease());
        let mut terminal = None;

        loop {
            // Read from the PTY.
            match self.pty.reader().read(&mut buf[unprocessed..]) {
                // The answer on macOS when nothing is left to read on the PTY.
                Ok(0) if unprocessed == 0 => break,
                Ok(got) => unprocessed += got,
                Err(err) => match err.kind() {
                    ErrorKind::Interrupted | ErrorKind::WouldBlock => {
                        // If parsing has caught up and the PTY would block, go
                        // back to the poller.
                        if unprocessed == 0 {
                            break;
                        }
                    }
                    _ => return Err(err),
                },
            }

            // Try to lock the terminal.
            if terminal.is_none() {
                terminal = match fair.try_lock_unfair() {
                    // At the buffer limit, take the lock by waiting — and in
                    // the serial rounds always, so a read is parsed alone.
                    None if serial || unprocessed >= buf.len() => Some(fair.lock_unfair()),
                    None => continue,
                    locked => locked,
                };
            }

            // Parse the incoming bytes — into `Term` through the wrapper.
            //
            // A graphics command that needs decoding halts the parser right
            // behind it. The decoding is **between** two stretches of the
            // parse, with the terminal neither locked nor leased: a frame
            // waits for the lease as well as the lock, so inflating or
            // decoding a large image under either would hold every frame
            // back for as long as it takes. The bytes after the command wait
            // in `buf`, as they do when the lock is busy, and the parse goes
            // on from them, so the order is the stream's.
            let mut at = 0;
            loop {
                let guard = terminal.get_or_insert_with(|| fair.lock_unfair());
                let mut handler =
                    ClusterHandler::new(&mut **guard, self.cluster, &mut state.last_input)
                        .graphics(self.graphics.as_deref())
                        .commands(&mut state.commands);
                let used = state
                    .parser
                    .advance_until_halt(&mut handler, &buf[at..unprocessed]);
                let halted = handler.halt_requested();
                drop(handler);
                at += used;
                // The journal's applied count, under the terminal lock and by
                // the bytes the parser took — not the bytes read: the reads
                // before the lock accumulate into one parse. A side record
                // the main thread writes while the lock is let go is stamped
                // by it, and its replay finds it before the byte that ends
                // the command, where the command's effect on the cursor waits
                // for it: that last byte (`\`) is counted when the effect is
                // made. (Not inside a synchronized update's replay, where the
                // bytes the parser took are the update's, long ago.)
                let held_back =
                    usize::from(halted && used > 0 && state.parser.sync_bytes_count() == 0);
                if let Some(journal) = &self.journal {
                    journal.applied(used - held_back);
                }
                if !halted {
                    break;
                }
                if let Some(graphics) = self.graphics.as_deref() {
                    settle_halt(graphics.hub(), &mut lease, &mut terminal, || {
                        let lease = fair.lease();
                        (lease, fair.lock_unfair())
                    });
                }
                // (A request nothing is parked for is taken back all the same.)
                let guard = terminal.get_or_insert_with(|| fair.lock_unfair());
                ClusterHandler::new(&mut **guard, self.cluster, &mut state.last_input)
                    .graphics(self.graphics.as_deref())
                    .commands(&mut state.commands)
                    .resume();
                if let Some(journal) = &self.journal {
                    journal.applied(held_back);
                }
            }
            debug_assert_eq!(
                at, unprocessed,
                "the parser left bytes it was not halted for"
            );

            // The reads are all parsed: what was noted of them for a command
            // that is still arriving was taken by it. Not while the parser
            // holds bytes it has not played yet (a synchronized update): the
            // commands in them are the reads' still.
            if let Some(graphics) = self.graphics.as_deref()
                && !state.commands.receiving()
                && state.parser.sync_bytes_count() == 0
            {
                graphics.hub().forget_remote();
            }

            processed += unprocessed;
            unprocessed = 0;

            // Do not hold the terminal locked longer than needed.
            if processed >= MAX_LOCKED_READ {
                break;
            }
        }

        // Ask for a redraw unless all processed bytes were synchronized.
        if state.parser.sync_bytes_count() < processed && processed > 0 {
            self.event_proxy.send_event(Event::Wakeup);
        }

        Ok(processed)
    }

    #[inline]
    fn pty_write(&mut self, state: &mut State) -> io::Result<()> {
        state.ensure_next();

        'write_many: while let Some(mut current) = state.take_current() {
            'write_one: loop {
                match self.pty.writer().write(current.remaining_bytes()) {
                    Ok(0) => {
                        state.set_current(Some(current));
                        break 'write_many;
                    }
                    Ok(n) => {
                        current.advance(n);
                        if current.finished() {
                            state.goto_next();
                            break 'write_one;
                        }
                    }
                    Err(err) => {
                        state.set_current(Some(current));
                        match err.kind() {
                            ErrorKind::Interrupted | ErrorKind::WouldBlock => break 'write_many,
                            _ => return Err(err),
                        }
                    }
                }
            }
        }

        Ok(())
    }

    pub(crate) fn spawn(mut self) -> JoinHandle<(Self, State)> {
        thread::spawn_named("PTY reader", move || {
            let mut state = State::default();
            let mut buf = [0u8; READ_BUFFER_SIZE];

            let poll_opts = PollMode::Level;
            let mut interest = PollingEvent::readable(0);

            // Register the TTY through the `EventedReadWrite` interface.
            //
            // SAFETY: the registration's condition is that the sources
            // outlive it; `self.pty` owns the fds and the registration is
            // removed by the `deregister` below, before `self` leaves this
            // thread (alacritty's same call).
            if let Err(err) = unsafe { self.pty.register(&self.poll, interest, poll_opts) } {
                eprintln!("bateri: reader loop registration failed: {err}");
                return (self, state);
            }

            // One `pty_read` stops at `MAX_LOCKED_READ`; a long prefix on a
            // quiet PTY would wait for the next output, so read rounds until
            // the prefix went through — bounded by its length, so a program
            // writing without pause cannot keep the loop from the channel.
            let mut prefix_left = self.read_first.map_or(0, |prefix| prefix.max(1));
            self.read_prefix(&mut state, &mut buf, &mut prefix_left);

            let mut events = Events::with_capacity(EVENTS_CAPACITY);
            let mut gate = Gate::default();

            'event_loop: loop {
                // The journal's read gate (nothing without a journal), and the
                // prefix a closed gate stopped.
                let stall = self.gate(&mut gate, &mut interest, poll_opts);
                if prefix_left > 0 && interest.readable {
                    self.read_prefix(&mut state, &mut buf, &mut prefix_left);
                }

                // Wake at the deadline of a synchronized update (DEC 2026) —
                // or at the read gate's stall deadline, if sooner.
                let handler = state.parser.sync_timeout();
                let sync = handler.sync_timeout();
                let deadline = match (sync, stall) {
                    (Some(sync), Some(stall)) => Some(sync.min(stall)),
                    (sync, stall) => sync.or(stall),
                };
                let timeout = deadline.map(|st| st.saturating_duration_since(Instant::now()));

                events.clear();
                if let Err(err) = self.poll.wait(&mut events, timeout) {
                    match err.kind() {
                        ErrorKind::Interrupted => continue,
                        _ => {
                            eprintln!("bateri: reader loop poll failed: {err}");
                            break 'event_loop;
                        }
                    }
                }

                // Timeout of a synchronized update: the buffered bytes go to
                // the **wrapper**, by the same path `advance` sees — handed
                // to `Term` directly, clustering would be skipped in
                // this arm.
                if events.is_empty() && self.rx.peek().is_none() {
                    if sync_due(sync, self.journal.is_some(), Instant::now()) {
                        self.stop_timed_out_sync(&mut state, || {
                            // In the same lock round: the replay applies the
                            // buffer after exactly the bytes applied so far.
                            if let Some(journal) = &self.journal {
                                journal.record(Side::StopSync);
                            }
                        });
                        self.event_proxy.send_event(Event::Wakeup);
                        // A compaction that found the update open gave up
                        // until a new side record — this one: ask again, or
                        // a closed gate would wait for the stall.
                        if let Some(journal) = &self.journal {
                            Journal::poke(journal);
                        }
                    }
                    continue;
                }

                // Process any messages in the channel.
                if !self.drain_recv_channel(&mut state) {
                    break;
                }

                for event in events.iter() {
                    match event.key {
                        PTY_CHILD_EVENT_TOKEN => {
                            if let Some(tty::ChildEvent::Exited(status)) =
                                self.pty.next_child_event()
                            {
                                if let Some(status) = status {
                                    self.event_proxy.send_event(Event::ChildExit(status));
                                }
                                if self.drain_on_exit {
                                    let _ = self.pty_read(&mut state, &mut buf, false);
                                }
                                self.terminal.lock().exit();
                                self.event_proxy.send_event(Event::Wakeup);
                                break 'event_loop;
                            }
                        }

                        PTY_READ_WRITE_TOKEN => {
                            if event.is_interrupt() {
                                // Do not attempt I/O on a dead PTY.
                                continue;
                            }

                            let read = if event.readable {
                                let read = self.pty_read(&mut state, &mut buf, false);
                                // Outside the lease: the compaction may start a thread.
                                if let Some(journal) = &self.journal {
                                    Journal::poke(journal);
                                }
                                read
                            } else {
                                Ok(0)
                            };
                            if let Err(err) = read {
                                // On Linux, when the client end closes, the
                                // master's `read` may return `EIO`; go back to
                                // the loop for the inevitable `Exited` event.
                                // `libc` is not a dependency of this crate: 5
                                // is Linux's `EIO`.
                                #[cfg(target_os = "linux")]
                                if err.raw_os_error() == Some(5) {
                                    continue;
                                }

                                eprintln!("bateri: PTY read failed: {err}");
                                break 'event_loop;
                            }

                            if event.writable
                                && let Err(err) = self.pty_write(&mut state)
                            {
                                eprintln!("bateri: PTY write failed: {err}");
                                break 'event_loop;
                            }
                        }
                        _ => (),
                    }
                }

                // Register write interest if needed.
                let needs_write = state.needs_write();
                if needs_write != interest.writable {
                    interest.writable = needs_write;

                    // Re-register with the new interest. The panic is
                    // **deliberate** and the same as alacritty's:
                    // `Session::begin_shutdown` recognizes the reader's crash
                    // from `join`'s `Err` (`teardown=`); a silent `break`
                    // would leave the child alive and the window frozen, and
                    // report the shutdown as `clean`.
                    if let Err(err) = self.pty.reregister(&self.poll, interest, poll_opts) {
                        panic!("reader loop re-registration failed: {err}"); // audit: alacritty parity, the crash reaches the shutdown report through join's Err
                    }
                }
            }

            // The event sources are not dropped here, the registration is
            // removed explicitly.
            let _ = self.pty.deregister(&self.poll);

            (self, state)
        })
    }
}

/// What a halted parser waits for: the terminal and its lease let go, the
/// parked transmission decoded (and its file read) with neither held, both
/// taken back. The caller then lets the wrapper apply the record
/// ([`ClusterHandler::resume`]).
///
/// The guards are the caller's and `relock` makes new ones, in the order the
/// lock order asks (the lease, then the terminal — by waiting: the stream is
/// in the middle, and nothing else is to be read before it goes on).
fn settle_halt<L, G>(
    hub: &Hub,
    lease: &mut Option<L>,
    terminal: &mut Option<G>,
    relock: impl FnOnce() -> (L, G),
) {
    *terminal = None;
    drop(lease.take());
    hub.decode_pending();
    let (new_lease, new_terminal) = relock();
    *lease = Some(new_lease);
    *terminal = Some(new_terminal);
}

/// The most events the poller returns in one round (alacritty's number).
const EVENTS_CAPACITY: NonZeroUsize = match NonZeroUsize::new(1024) {
    Some(capacity) => capacity,
    None => panic!("event capacity cannot be zero"), // audit: const evaluation, zero is a compile error
};

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, Ordering};

    use crate::pointer::PointerShapes;
    use std::time::Duration;

    use alacritty_terminal::event::VoidListener;
    use alacritty_terminal::grid::Dimensions as _;
    use alacritty_terminal::index::{Column, Line};
    use alacritty_terminal::term::Config;
    use alacritty_terminal::term::test::TermSize;

    use super::*;
    use crate::graphics::Hub;
    use crate::handler::KeptModes;

    /// The graphics side of a session, without a session.
    struct TestSink {
        hub: Hub,
        modes: KeptModes,
        pointer: PointerShapes,
    }

    impl Sink for TestSink {
        fn hub(&self) -> &Hub {
            &self.hub
        }

        fn cell(&self) -> (u16, u16) {
            (10, 20)
        }

        fn reply(&self, _: Vec<u8>) {}

        fn kept_modes(&self) -> &KeptModes {
            &self.modes
        }

        fn pointer(&self) -> &PointerShapes {
            &self.pointer
        }

        fn pointer_changed(&self) {}
    }

    /// What the scripted PTY gives, a step at a time.
    enum Step {
        Text(&'static str),
        /// Bytes as the PTY would give them, graphics commands in them
        /// included: nothing takes them out on the way.
        Wire(Vec<u8>),
    }

    struct ScriptedPty {
        steps: VecDeque<Step>,
        /// The bytes of the step being given that the PTY still has.
        pending: Vec<u8>,
        /// The most bytes a read of it gives (a pipe gives what it has).
        chunk: usize,
        void: io::Sink,
        /// How much of the buffer each read of the script was offered.
        offered: Vec<usize>,
    }

    impl ScriptedPty {
        fn new(steps: impl IntoIterator<Item = Step>) -> Self {
            Self {
                steps: steps.into_iter().collect(),
                pending: Vec::new(),
                chunk: usize::MAX,
                void: io::sink(),
                offered: Vec::new(),
            }
        }

        fn chunked(mut self, chunk: usize) -> Self {
            self.chunk = chunk;
            self
        }
    }

    impl Read for ScriptedPty {
        /// A pipe: what the next step has, as much of it as the buffer and the
        /// chunk take, and `WouldBlock` when the script is through.
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            self.offered.push(buf.len());
            if self.pending.is_empty() {
                self.pending = match self.steps.pop_front() {
                    Some(Step::Text(text)) => text.as_bytes().to_vec(),
                    Some(Step::Wire(bytes)) => bytes,
                    None => return Err(ErrorKind::WouldBlock.into()),
                };
            }
            let take = self.pending.len().min(buf.len()).min(self.chunk);
            buf[..take].copy_from_slice(&self.pending[..take]);
            self.pending.drain(..take);
            Ok(take)
        }
    }

    impl tty::EventedReadWrite for ScriptedPty {
        type Reader = Self;
        type Writer = io::Sink;

        unsafe fn register(
            &mut self,
            _: &Arc<Poller>,
            _: PollingEvent,
            _: PollMode,
        ) -> io::Result<()> {
            Ok(())
        }

        fn reregister(&mut self, _: &Arc<Poller>, _: PollingEvent, _: PollMode) -> io::Result<()> {
            Ok(())
        }

        fn deregister(&mut self, _: &Arc<Poller>) -> io::Result<()> {
            Ok(())
        }

        fn reader(&mut self) -> &mut Self::Reader {
            self
        }

        fn writer(&mut self) -> &mut Self::Writer {
            &mut self.void
        }
    }

    impl tty::EventedPty for ScriptedPty {
        fn next_child_event(&mut self) -> Option<tty::ChildEvent> {
            None
        }
    }

    impl OnResize for ScriptedPty {
        fn on_resize(&mut self, _: WindowSize) {}
    }

    /// An image carried by a read that comes **after** the loop has taken the
    /// terminal's lock for the one before it: decoding it must not happen
    /// under that lock, nor under the lease a frame also waits for.
    #[test]
    fn an_image_is_decoded_with_neither_the_terminal_lease_nor_its_lock_held() {
        use crate::graphics::tests::wire;
        let sink = quiet_sink();
        let image = wire("a=t,f=32,s=1,v=1,i=1", &[0; 4]);
        let steps = [
            Step::Text("hello"),
            Step::Wire(image.clone()),
            Step::Text("world"),
        ];
        let (mut event_loop, terminal) = scripted_loop(ScriptedPty::new(steps), &sink, false);
        let frame = frame_thread(&terminal, &sink);

        let mut state = State::default();
        let mut buf = vec![0; 4096];
        let processed = event_loop
            .pty_read(&mut state, &mut buf, true)
            .expect("read");
        frame.finish("an image was decoded");

        assert_eq!(processed, 5 + image.len() + 5, "every byte is the parser's");
        // The order is kept: the text on both sides of the command reached the
        // grid, and the image it carried is stored.
        let row = grid_row(&terminal.lock(), 0);
        assert!(row.starts_with("helloworld"), "{row:?}");
        assert!(!sink.hub.is_parked());
        assert_eq!(sink.hub.with_images(|images| images.main().ids()), [1]);
    }

    /// A frame that wants the terminal while the loop decodes: it asks for the
    /// lock the moment the decode begins and must get it before the decode ends.
    struct Frame {
        free: Arc<AtomicBool>,
        thread: std::thread::JoinHandle<()>,
    }

    impl Frame {
        fn finish(self, what: &str) {
            self.thread.join().expect("the frame thread");
            assert!(
                self.free.load(Ordering::Acquire),
                "a frame could not take the terminal while {what}"
            );
        }
    }

    fn frame_thread(terminal: &Arc<FairMutex<Term<VoidListener>>>, sink: &Arc<TestSink>) -> Frame {
        let (go, go_rx) = mpsc::channel::<()>();
        let (got, got_rx) = mpsc::channel::<()>();
        let thread = {
            let terminal = Arc::clone(terminal);
            std::thread::spawn(move || {
                if go_rx.recv_timeout(Duration::from_secs(5)).is_ok() {
                    drop(terminal.lock());
                    let _ = got.send(());
                }
            })
        };
        let free = Arc::new(AtomicBool::new(false));
        {
            let free = Arc::clone(&free);
            // Called just before an image is decoded or a file is opened.
            sink.hub.on_decode(Box::new(move || {
                let _ = go.send(());
                free.store(
                    got_rx.recv_timeout(Duration::from_secs(5)).is_ok(),
                    Ordering::Release,
                );
            }));
        }
        Frame { free, thread }
    }

    /// A file in the temporary directory for a command to name, removed when
    /// the guard goes.
    struct TempFile(std::path::PathBuf);

    impl TempFile {
        fn new(stem: &str, bytes: &[u8]) -> Self {
            let path = std::env::temp_dir().join(format!("{stem}-{}", std::process::id()));
            std::fs::write(&path, bytes).expect("a file to name");
            Self(path)
        }

        /// The file's name as a command carries it.
        fn name(&self) -> String {
            use std::os::unix::ffi::OsStrExt;
            crate::graphics::tests::base64(self.0.as_os_str().as_bytes())
        }
    }

    impl Drop for TempFile {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    /// A one-cell PNG (10 × 20 pixels) in a file of its own.
    fn one_cell_png(stem: &str) -> TempFile {
        let mut bytes = Vec::new();
        let mut encoder = png::Encoder::new(&mut bytes, 10, 20);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().expect("header");
        writer.write_image_data(&[7; 10 * 20 * 4]).expect("data");
        writer.finish().expect("finish");
        TempFile::new(stem, &bytes)
    }

    /// `kitten icat`'s command for a one-cell PNG as the PTY would carry it:
    /// the file, its kind and its size, nothing about the cursor.
    fn icat_command(file: &TempFile) -> Vec<u8> {
        format!("\x1b_Ga=T,t=f,f=100,s=10,v=20,i=1;{}\x1b\\", file.name()).into_bytes()
    }

    /// An image of `side` × `side` pixels sent as the protocol asks for a big
    /// one: in 4096-character commands, `m=1` on all but the last.
    fn chunked_image(side: u32, id: u32) -> Vec<u8> {
        use crate::graphics::tests::{base64, rgba};
        let text = base64(&rgba(side, side, 3));
        let chunks: Vec<&[u8]> = text.as_bytes().chunks(4096).collect();
        let mut wire = Vec::new();
        for (at, chunk) in chunks.iter().enumerate() {
            let more = u8::from(at + 1 < chunks.len());
            let control = if at == 0 {
                format!("a=t,f=32,s={side},v={side},i={id},m={more}")
            } else {
                format!("m={more}")
            };
            wire.extend_from_slice(b"\x1b_G");
            wire.extend_from_slice(control.as_bytes());
            wire.push(b';');
            wire.extend_from_slice(chunk);
            wire.extend_from_slice(b"\x1b\\");
        }
        wire
    }

    fn grid_row(terminal: &Term<VoidListener>, line: i32) -> String {
        (0..terminal.columns())
            .map(|column| terminal.grid()[Line(line)][Column(column)].c)
            .collect()
    }

    fn all_text(terminal: &Term<VoidListener>) -> String {
        (0..4).map(|line| grid_row(terminal, line)).collect()
    }

    fn scripted_loop(
        pty: ScriptedPty,
        sink: &Arc<TestSink>,
        drain_on_exit: bool,
    ) -> (
        EventLoop<ScriptedPty, VoidListener>,
        Arc<FairMutex<Term<VoidListener>>>,
    ) {
        let terminal = Arc::new(FairMutex::new(Term::new(
            Config::default(),
            &TermSize::new(20, 4),
            VoidListener,
        )));
        let mut event_loop = EventLoop::new(
            Arc::clone(&terminal),
            VoidListener,
            pty,
            drain_on_exit,
            false,
        )
        .expect("loop");
        event_loop.graphics(Arc::clone(sink) as Arc<dyn Sink>);
        (event_loop, terminal)
    }

    fn quiet_sink() -> Arc<TestSink> {
        Arc::new(TestSink {
            hub: Hub::new(),
            modes: KeptModes::default(),
            pointer: PointerShapes::default(),
        })
    }

    /// The file a command names is read by the decode, with neither the
    /// terminal's lock nor its lease held — and the cursor still lands between
    /// the text on both sides of the command, by the size the command gave.
    /// The read of the command comes after the loop has taken the lock for the
    /// one before it, as a frame would be waiting on it.
    #[test]
    fn a_file_is_read_with_neither_the_lease_nor_the_lock_held() {
        let png = one_cell_png("bt-reader-png");
        let sink = quiet_sink();
        let pty = ScriptedPty::new([
            Step::Text("hello"),
            Step::Wire(icat_command(&png)),
            Step::Text("world"),
        ]);
        let (mut event_loop, terminal) = scripted_loop(pty, &sink, false);
        let frame = frame_thread(&terminal, &sink);

        let mut state = State::default();
        let mut buf = vec![0; 4096];
        event_loop
            .pty_read(&mut state, &mut buf, true)
            .expect("read");
        frame.finish("a file was read");

        // The image is one cell: the cursor goes one column right of the
        // text before it, and the text after it follows.
        let row = grid_row(&terminal.lock(), 0);
        assert!(row.starts_with("hello world"), "{row:?}");
        assert!(!sink.hub.is_parked());
        assert_eq!(sink.hub.with_images(|images| images.main().ids()), [1]);
    }

    /// A DEC 2026 update that timed out is applied with the same care: the
    /// decoding of an image in it is not done under the terminal's lock.
    #[test]
    fn an_image_in_an_update_that_timed_out_is_decoded_with_the_terminal_free() {
        use crate::graphics::tests::wire;
        let sink = quiet_sink();
        let steps = [
            Step::Wire(b"\x1b[?2026h".to_vec()),
            Step::Wire(wire("a=T,f=32,s=10,v=20,i=1", &[0; 10 * 20 * 4])),
            Step::Text("after"),
        ];
        let (mut event_loop, terminal) = scripted_loop(ScriptedPty::new(steps), &sink, false);
        let mut state = State::default();
        let mut buf = vec![0; 4096];
        event_loop
            .pty_read(&mut state, &mut buf, true)
            .expect("read");
        // All of it waits in the parser's buffer.
        assert!(all_text(&terminal.lock()).trim().is_empty());
        assert!(
            sink.hub
                .with_images(|images| images.main().ids())
                .is_empty()
        );

        let frame = frame_thread(&terminal, &sink);
        let mut journaled = false;
        event_loop.stop_timed_out_sync(&mut state, || journaled = true);
        frame.finish("the image of a timed out update was decoded");

        assert!(journaled, "the stop is journaled in the round it begins in");
        assert_eq!(sink.hub.with_images(|images| images.main().ids()), [1]);
        // One cell across, no row down: the image is a cell big, then the text.
        let row = grid_row(&terminal.lock(), 0);
        assert!(row.starts_with(" after"), "{row:?}");
    }

    /// The handover applies an update that is still open without the images,
    /// and moves the cursor as the commands in it would have: by the cell size
    /// the session has, not one the parser never heard of.
    #[test]
    fn an_update_open_at_the_freeze_moves_the_cursor_by_the_sessions_cell() {
        use crate::graphics::tests::wire;
        let sink = quiet_sink();
        let steps = [Step::Wire(
            [
                b"\x1b[?2026h\x1b[3;6H".to_vec(),
                // 20 × 40 pixels are two columns by two rows of the sink's cell.
                wire("a=T,f=32,s=20,v=40,i=1", &[0; 20 * 40 * 4]),
            ]
            .concat(),
        )];
        let (mut event_loop, terminal) = scripted_loop(ScriptedPty::new(steps), &sink, false);
        let mut state = State::default();
        let mut buf = vec![0; 1 << 16];
        event_loop
            .pty_read(&mut state, &mut buf, true)
            .expect("read");
        event_loop.stop_sync(&mut state);
        let point = terminal.lock().grid().cursor.point;
        assert_eq!((point.line.0, point.column.0), (3, 7));
        assert!(
            sink.hub
                .with_images(|images| images.main().ids())
                .is_empty()
        );
    }

    /// A name in an update that is still being written is the reads' still:
    /// the parser has caught up with them, but not with the commands it holds
    /// back, and what the reads noted of the pane waits for those.
    #[test]
    fn what_the_reads_noted_waits_for_the_commands_an_update_holds_back() {
        use std::os::unix::ffi::OsStrExt;

        use crate::graphics::tests::wire;
        let sink = quiet_sink();
        let file = TempFile::new("tty-graphics-protocol-reader-held", &[0; 4]);
        let steps = [Step::Wire(b"\x1b[?2026h".to_vec())];
        let (mut event_loop, _terminal) = scripted_loop(ScriptedPty::new(steps), &sink, false);
        let mut state = State::default();
        let mut buf = vec![0; 4096];
        event_loop
            .pty_read(&mut state, &mut buf, true)
            .expect("read");
        // The next read has a name in it, and noted the pane as remote (a
        // session that ended in that read is not remote when it is played).
        let command = wire("a=t,t=t,f=32,s=1,v=1,i=1", file.0.as_os_str().as_bytes());
        event_loop.pty_mut().steps.push_back(Step::Wire(command));
        sink.hub.note_remote();
        event_loop
            .pty_read(&mut state, &mut buf, true)
            .expect("read");
        assert!(
            sink.hub.remote_seen(),
            "the update's commands are not played"
        );
        event_loop.stop_timed_out_sync(&mut state, || ());
        assert!(!sink.hub.remote_seen(), "played: nothing is held back");
        assert!(file.0.exists(), "the name was refused, not read");
        assert!(
            sink.hub
                .with_images(|images| images.main().ids())
                .is_empty()
        );
    }

    /// The journal counts the bytes the parser took before the terminal is let
    /// go for a decode, but for the last byte of the command: a side record the
    /// main thread writes in that gap is stamped by them, and its replay finds
    /// it before the byte that makes the command's effect on the cursor, where
    /// the live terminal made that effect after it.
    #[test]
    fn the_journal_counts_the_bytes_the_parser_took_before_the_terminal_is_let_go() {
        use crate::graphics::tests::wire;
        let sink = quiet_sink();
        let journal = Journal::in_memory();
        let image = wire("a=t,f=32,s=1,v=1,i=1", &[0; 4]);
        let steps = [Step::Wire([&b"hello"[..], &image, b"world"].concat())];
        let (mut event_loop, terminal) = scripted_loop(ScriptedPty::new(steps), &sink, false);
        event_loop.journal(Arc::clone(&journal));
        let seen = Arc::new(Mutex::new(Vec::new()));
        {
            let (journal, seen) = (Arc::clone(&journal), Arc::clone(&seen));
            sink.hub.on_decode(Box::new(move || {
                seen.lock().expect("lock").push(journal.applied_bytes());
            }));
        }
        let mut state = State::default();
        let mut buf = vec![0; 4096];
        let processed = event_loop
            .pty_read(&mut state, &mut buf, true)
            .expect("read");
        assert_eq!(*seen.lock().expect("lock"), [5 + image.len() as u64 - 1]);
        assert_eq!(journal.applied_bytes(), processed as u64);
        assert!(grid_row(&terminal.lock(), 0).starts_with("helloworld"));
    }

    /// The shortest commands there are, a great many of them, in a buffer far
    /// smaller than the stream: each is parsed where it ends, in order, and
    /// nothing of what was read is left anywhere.
    #[test]
    fn a_flood_of_short_commands_is_parsed_to_its_end() {
        use crate::graphics::tests::wire;
        let sink = quiet_sink();
        // The size the placements below are made from, from a command that
        // names a file (so the flood itself is nothing but commands).
        let image = TempFile::new("bt-reader-flood", &vec![3; 30 * 40 * 4]);
        let mut wire_bytes =
            format!("\x1b_Ga=t,t=f,f=32,s=30,v=40,i=1;{}\x1b\\", image.name()).into_bytes();
        for _ in 0..300 {
            wire_bytes.extend_from_slice(&wire("a=p,i=1", &[]));
        }
        wire_bytes.extend_from_slice(b"END");
        let total = wire_bytes.len();
        let pty = ScriptedPty::new([Step::Wire(wire_bytes)]);
        let (mut event_loop, terminal) = scripted_loop(pty, &sink, false);

        let mut state = State::default();
        let mut buf = vec![0; 512];
        let mut processed = 0;
        for _ in 0..1000 {
            processed += event_loop
                .pty_read(&mut state, &mut buf, true)
                .expect("read");
            let pty = event_loop.pty_mut();
            if pty.steps.is_empty() && pty.pending.is_empty() {
                break;
            }
        }

        assert!(
            all_text(&terminal.lock()).contains("END"),
            "the text after the commands did not arrive"
        );
        assert_eq!(processed, total, "every byte the PTY gave is the parser's");
        assert!(event_loop.pty_mut().pending.is_empty());
    }

    /// The promise made for a terminal that has seen no image command: the
    /// read path is what it was before graphics — the parser gets the bytes
    /// the PTY gave, each read is offered the whole buffer, and there is one
    /// read of the PTY for each piece it gave. Every read in this corpus ends
    /// where the next sequence may begin, and sequences that look like
    /// graphics, and are not, are in it.
    #[test]
    fn a_stream_with_no_command_is_the_parsers_input_byte_for_byte() {
        let long_list = format!(
            "\x1b[?{}l",
            (1000..1032)
                .map(|mode| mode.to_string())
                .collect::<Vec<_>>()
                .join(";")
        );
        let pieces: Vec<&[u8]> = vec![
            b"plain text\r\nwith a second line\r\n",
            b"\x1b[31mred\x1b[0m \x1b[1;38;2;1;2;3mtrue\x1b[m",
            b"\x1b[?25l",
            b"\x1b[?25h\x1b[?2026h",
            b"\x1b[?2026l\x1b[?2004h\x1b[?1049h\x1b[?1049l",
            long_list.as_bytes(),
            b"\x1b[?8166h\x1b[?16385;16384;16384;16384;16385;8166h",
            b"\x1b[?8166\x07h\x1b[?8166\x7fh\x1b[\x07?8166h\x1b\x07[?8166h",
            b"\x1b",
            b"[?25h",
            b"\x1b[",
            b"?1049l",
            b"text\x1b",
            b"]0;a title\x07",
            b"\x1b",
            b"\x1b",
            b"_Xan APC that is not ours\x1b\\",
            b"\x1b_",
            b"Y\x1b\\ and plain again\r\n",
        ];
        let sink = quiet_sink();
        let steps = pieces.iter().map(|piece| Step::Wire(piece.to_vec()));
        let (mut event_loop, terminal) = scripted_loop(ScriptedPty::new(steps), &sink, false);
        let mut state = State::default();
        let mut buf = vec![0u8; 4096];
        let processed = event_loop
            .pty_read(&mut state, &mut buf, true)
            .expect("read");
        assert_eq!(processed, pieces.concat().len());
        // One read for each piece, and one that found nothing more; each was
        // offered the whole buffer.
        let pty = event_loop.pty_mut();
        assert_eq!(pty.offered, vec![buf.len(); pieces.len() + 1]);
        assert!(pty.steps.is_empty() && pty.pending.is_empty());
        // And what the parser made of them is what a parser makes of the same
        // bytes given whole.
        let mut reference = Term::new(Config::default(), &TermSize::new(20, 4), VoidListener);
        let mut parser: ansi::Processor = ansi::Processor::new();
        let mut last_input = false;
        parser.advance(
            &mut ClusterHandler::new(&mut reference, false, &mut last_input),
            &pieces.concat(),
        );
        assert_eq!(all_text(&terminal.lock()), all_text(&reference));
    }

    /// An image in parts — 4 KB commands, as the protocol asks — delivers
    /// nothing until its last part. Live, the loop's turn goes on to the text
    /// behind it.
    #[test]
    fn a_big_image_in_parts_and_the_text_after_it_are_read_in_one_turn() {
        let sink = quiet_sink();
        let mut wire_bytes = chunked_image(100, 1);
        wire_bytes.extend_from_slice(b"END");
        let pty = ScriptedPty::new([Step::Wire(wire_bytes)]);
        let (mut event_loop, terminal) = scripted_loop(pty, &sink, false);

        let mut state = State::default();
        let mut buf = vec![0; 4096];
        event_loop
            .pty_read(&mut state, &mut buf, false)
            .expect("read");

        assert!(all_text(&terminal.lock()).contains("END"));
        assert_eq!(sink.hub.with_images(|images| images.main().ids()), [1]);
    }

    /// The same image in a handover's prefix: the rounds that read it stop
    /// when the prefix is through.
    #[test]
    fn a_big_image_in_parts_in_a_carried_prefix_is_read_to_its_end() {
        let sink = quiet_sink();
        let mut prefix = chunked_image(100, 1);
        prefix.extend_from_slice(b"END");
        let carried = prefix.len();
        // A pipe that gives 4 KB a read.
        let pty = ScriptedPty::new([Step::Wire(prefix)]).chunked(4096);
        let (mut event_loop, terminal) = scripted_loop(pty, &sink, false);
        let sender = event_loop.channel();
        event_loop.read_first(carried);
        let thread = event_loop.spawn();

        let deadline = Instant::now() + Duration::from_secs(10);
        let mut seen = false;
        while Instant::now() < deadline && !seen {
            seen = all_text(&terminal.lock()).contains("END");
            std::thread::sleep(Duration::from_millis(5));
        }
        sender.send(Msg::Shutdown).expect("the loop is there");
        thread.join().expect("the loop");

        assert!(seen, "the end of the prefix was left unread");
    }

    /// And the output a child wrote last before it exited: drained, whatever
    /// the parts it came in.
    #[test]
    fn a_big_image_in_parts_a_child_wrote_last_is_drained_to_its_end() {
        let sink = quiet_sink();
        let mut wire_bytes = chunked_image(100, 1);
        wire_bytes.extend_from_slice(b"END");
        let pty = ScriptedPty::new([Step::Wire(wire_bytes)]).chunked(4096);
        let (mut event_loop, terminal) = scripted_loop(pty, &sink, true);

        let mut state = State::default();
        let mut buf = vec![0; READ_BUFFER_SIZE];
        event_loop
            .pty_read(&mut state, &mut buf, false)
            .expect("read");

        assert!(all_text(&terminal.lock()).contains("END"));
    }

    /// A carried prefix is counted in the bytes the parser gets, which are the
    /// bytes it was carried in: the loop's first rounds stop when the prefix is
    /// through, and a quiet PTY gives nothing to wake the poller for the rest.
    #[test]
    fn a_long_carried_prefix_of_commands_is_read_to_its_end() {
        use crate::graphics::tests::wire;
        let sink = quiet_sink();
        let image = TempFile::new("bt-reader-prefix", &vec![3; 30 * 40 * 4]);
        let mut prefix =
            format!("\x1b_Ga=t,t=f,f=32,s=30,v=40,i=1;{}\x1b\\", image.name()).into_bytes();
        for _ in 0..4000 {
            prefix.extend_from_slice(&wire("a=p,i=1", &[]));
        }
        prefix.extend_from_slice(b"END");
        let carried = prefix.len();
        // A pipe that gives a thousand bytes a read: the rounds stop at the
        // lock's limit long before the prefix is through.
        let pty = ScriptedPty::new([Step::Wire(prefix)]).chunked(1000);
        let (mut event_loop, terminal) = scripted_loop(pty, &sink, false);
        let sender = event_loop.channel();
        event_loop.read_first(carried);
        let thread = event_loop.spawn();

        let deadline = Instant::now() + Duration::from_secs(10);
        let mut seen = false;
        while Instant::now() < deadline && !seen {
            seen = all_text(&terminal.lock()).contains("END");
            std::thread::sleep(Duration::from_millis(5));
        }
        sender.send(Msg::Shutdown).expect("the loop is there");
        thread.join().expect("the loop");

        assert!(seen, "the end of the prefix was left unread");
    }
}
