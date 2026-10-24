//! How fast a program and the module of its view may talk, in each direction.
//!
//! The two of them are not equals. The module is a page that must not hang the application, and
//! the program is a process in a tty that nothing can make read. What passes between them is
//! therefore measured by what the *receiving* side has not yet taken, never by how fast the
//! sending side goes: a fast sender that is read is not a problem, and a slow reader is not made
//! worse by a sender that waits.
//!
//! **Module to program.** What the module says goes to the pane's PTY writer, and a program
//! that does not read does not drain the tty's small input queue, so the writer's own queue is
//! where it waits. The bound is the bytes handed to the writer and not yet written, and the
//! bytes the module has said that the pane has not yet handed to the writer
//! ([`Flow::may_say`]). Over the bound the module is told *full*, keeps what it wanted to say and
//! is woken **once** when the program has read enough ([`Flow::on_room`]); it never polls. A
//! module that is fast and legitimate is never closed for its speed.
//!
//! **Program to module.** What the program says is read by the PTY reader, handed to the shell
//! and from there to the module's thread. The bound is the bytes of it that the module has not
//! yet taken ([`Flow::heard`], [`Flow::taken`]). Over the bound the reader stops *reading* the
//! PTY ([`Flow::inbox_closed`]): the program's own `write` slows by itself, because nothing
//! drains the tty, and nothing is dropped. The reader is not made to wait, because the loop it
//! runs is also the PTY's writer: a wait would keep ⌃C from the program for as long as the
//! module is slow. It closes the PTY's read interest instead, and the module's taking wakes it.
//!
//! **What the module said is never written once the program can no longer be its reader.** The
//! module's bytes have a queue of their own in the writer (whole frames, tagged with the view they
//! belong to), and a gate on it ([`Gate`]). A signal character that goes to the program (⌃C) closes
//! the gate at once, before it is queued, and the writer drops the module's frames before it writes
//! the character: the character flushes the tty's input, and a byte written after it is read by
//! whatever reads the tty next. Any other end of the view (`bye`, `dismiss`, the person, the
//! module's fault, a restart, the program's end) closes it when the end is learned, letting the
//! frame under way finish so that the program is not left a half frame. The end of the program
//! also throws away what the program left unread ([`Flow::module_was_written`]), and a program
//! that dies without a word is found out by the writer itself, which asks before it writes a
//! word of the module's whether the program is still the one that reads the tty.
//!
//! Whatever ends the view, [`Flow::end`] lets go of the two bounds at that instant and drops what
//! was counted: nothing is left waiting for a module that is gone.

use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

/// The bytes that may be waiting for the program to read them before a module is told *full*: the
/// writer's queue and what the module said that has not reached it. A message is accepted while
/// the bytes waiting are under the bound and may take it over, by at most one message (the
/// protocol's limit for one is 1 MiB).
pub const BACKLOG_LIMIT: usize = 256 << 10;

/// The bytes waiting when a module that was told *full* is woken: room for several of the
/// messages it holds, and not so little that the program has to drain everything first.
pub const BACKLOG_LOW: usize = BACKLOG_LIMIT / 4;

/// The bytes of the program's messages the module may not have taken before the reader stops
/// reading the PTY. The reader stops *after* the message that took it over, and a read is
/// bounded by what the kernel holds for the tty, so the module has at most this and one read.
pub const INBOX_LIMIT: usize = 256 << 10;

/// The bytes the module has not taken when the reader reads again.
pub const INBOX_LOW: usize = INBOX_LIMIT / 4;

/// What a message that waits costs besides its bytes: its place in a queue and its allocation.
/// Many tiny messages are not free.
const MESSAGE_COST: usize = 64;

/// What the pane tells whoever waits that the other side has room.
type Waker = Box<dyn Fn() + Send + Sync>;

/// What the writer may do with the module's frames.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Gate {
    /// The view is up: the module's frames are written.
    #[default]
    Open,
    /// The view ended and the program may still read: the frame under way is written to its
    /// end, so that the program is not left half a frame, and the ones behind it are dropped.
    Draining,
    /// A signal character is going to the program, or the program is gone: not a byte more, and
    /// the frame under way is given up.
    Closed,
}

#[derive(Default)]
struct State {
    /// Bytes handed to the PTY writer (`Adapter::input`) that it has not written.
    unwritten: usize,
    /// Bytes the module has said that the pane has not yet handed to the writer.
    in_transit: usize,
    /// The module was told *full* and has not been woken since.
    full: bool,
    /// Bytes of the program's messages the module has not taken.
    inbox: usize,
    /// The reader has stopped reading the PTY because the inbox is over its bound.
    closed: bool,
    /// The view the module's frames belong to: each view has a number, and a frame of another
    /// one is never written.
    epoch: u64,
    /// What the writer may do with the module's frames.
    gate: Gate,
    /// The writer has written a byte of the module's in this view.
    wrote: bool,
}

impl State {
    fn pending(&self) -> usize {
        self.unwritten + self.in_transit
    }

    /// The module was told *full* and the backlog has gone down to where it is woken: the wake
    /// is owed, once.
    fn room_owed(&mut self) -> bool {
        if self.full && self.pending() <= BACKLOG_LOW {
            self.full = false;
            true
        } else {
            false
        }
    }
}

#[derive(Default)]
struct Shared {
    state: Mutex<State>,
    /// Tells the shell the module can say more. Set once by the session.
    room: OnceLock<Waker>,
    /// Wakes the PTY reader: the inbox is under its low mark. Set once by the session.
    reader: OnceLock<Waker>,
}

/// The two counters of a pane's view, shared by the PTY reader, the pane's writer, the shell and
/// the module's thread. Cloned freely.
///
/// One lock holds all of it, because a wake that is missed between "told full" and "woken" would
/// leave a module waiting for ever; the wakers are called with the lock let go.
#[derive(Clone, Default)]
pub struct Flow(Arc<Shared>);

fn lock(mutex: &Mutex<State>) -> MutexGuard<'_, State> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl Flow {
    /// A flow with nothing counted.
    pub fn new() -> Self {
        Self::default()
    }

    /// What the shell is told when a module that was told *full* may say more
    /// ([`Session::take_room`](crate::Session::take_room) is where it takes the news). Once.
    pub(crate) fn on_room(&self, tell: impl Fn() + Send + Sync + 'static) {
        let _ = self.0.room.set(Box::new(tell));
    }

    /// What wakes the PTY reader when the module has taken enough for it to read again. Once.
    pub(crate) fn on_reader(&self, wake: impl Fn() + Send + Sync + 'static) {
        let _ = self.0.reader.set(Box::new(wake));
    }

    fn tell_room(&self) {
        if let Some(tell) = self.0.room.get() {
            tell();
        }
    }

    fn wake_reader(&self) {
        if let Some(wake) = self.0.reader.get() {
            wake();
        }
    }

    // ---- module to program -------------------------------------------------------------

    /// The module wants to say `bytes` to its program: whether it may now. If not it is *full*,
    /// the module keeps what it meant to say, and it is woken once when the program has read
    /// enough ([`Flow::on_room`]). From the module's thread.
    pub fn may_say(&self, bytes: usize) -> bool {
        let mut state = lock(&self.0.state);
        if state.pending() >= BACKLOG_LIMIT {
            state.full = true;
            return false;
        }
        state.in_transit += bytes;
        true
    }

    /// The pane took `bytes` the module said and handed them to the writer (or dropped them,
    /// the view being over): they are counted in the writer's queue now, if anywhere.
    pub(crate) fn said(&self, bytes: usize) {
        let owed = {
            let mut state = lock(&self.0.state);
            state.in_transit = state.in_transit.saturating_sub(bytes);
            state.room_owed()
        };
        if owed {
            self.tell_room();
        }
    }

    /// `bytes` went to the PTY writer's queue.
    pub(crate) fn queued(&self, bytes: usize) {
        lock(&self.0.state).unwritten += bytes;
    }

    /// The writer wrote `bytes` to the PTY: the program has read that much.
    pub(crate) fn written(&self, bytes: usize) {
        let owed = {
            let mut state = lock(&self.0.state);
            state.unwritten = state.unwritten.saturating_sub(bytes);
            state.room_owed()
        };
        if owed {
            self.tell_room();
        }
    }

    // ---- program to module -------------------------------------------------------------

    /// A message of `bytes` came from the program for the module. From the PTY reader.
    pub(crate) fn heard(&self, bytes: usize) {
        lock(&self.0.state).inbox += bytes + MESSAGE_COST;
    }

    /// The module took a message of `bytes` (or the shell had nowhere to give it). From any
    /// thread: the module's, as a rule.
    pub fn taken(&self, bytes: usize) {
        let wake = {
            let mut state = lock(&self.0.state);
            state.inbox = state.inbox.saturating_sub(bytes + MESSAGE_COST);
            if state.closed && state.inbox <= INBOX_LOW {
                state.closed = false;
                true
            } else {
                false
            }
        };
        if wake {
            self.wake_reader();
        }
    }

    /// Whether the PTY reader is to leave the PTY unread for now. Once it says yes it keeps
    /// saying it until the module has taken the inbox down to its low mark, and then wakes the
    /// reader ([`Flow::on_reader`]). From the reader.
    pub(crate) fn inbox_closed(&self) -> bool {
        let mut state = lock(&self.0.state);
        if state.closed {
            return true;
        }
        if state.inbox >= INBOX_LIMIT {
            state.closed = true;
            return true;
        }
        false
    }

    // ---- the life of the view and the gate on the module's bytes ------------------------

    /// A view starts: nothing is counted, the module's bytes are written again, and the frames
    /// of the view before are no one's.
    pub(crate) fn begin(&self) {
        let wake = {
            let mut state = lock(&self.0.state);
            state.inbox = 0;
            state.in_transit = 0;
            state.full = false;
            state.epoch += 1;
            state.gate = Gate::Open;
            state.wrote = false;
            std::mem::take(&mut state.closed)
        };
        if wake {
            self.wake_reader();
        }
    }

    /// The view is over, whatever ended it: nothing it said or was told is waiting for anyone, the
    /// reader is woken if it had stopped, and the writer is told to drop the module's frames that
    /// are waiting (the one under way is finished). The writer's queue stays what it is, being
    /// bytes the program is owed all the same.
    pub(crate) fn end(&self) {
        {
            let mut state = lock(&self.0.state);
            state.inbox = 0;
            state.in_transit = 0;
            state.full = false;
            state.closed = false;
            if state.gate == Gate::Open {
                state.gate = Gate::Draining;
            }
        }
        // The loop that reads and writes is woken whether or not it had stopped reading: it
        // drops the module's frames that wait, and reads on.
        self.wake_reader();
    }

    /// A signal character is going to the program, or the program is gone: from now on not a byte
    /// of the module's is written, and the frame under way is given up. Before the character is
    /// queued — the writer then finds the gate shut when it comes to write it.
    pub(crate) fn interrupt(&self) {
        lock(&self.0.state).gate = Gate::Closed;
    }

    /// A signal character is on its way to the program: as [`Flow::interrupt`], and what the
    /// module wrote before it is the tty's to throw away — the character flushes the tty's input
    /// as it signals the program — so the program's end has nothing of the module's left to
    /// discard, and the keys typed after the character are the shell's.
    pub(crate) fn signal(&self) {
        let mut state = lock(&self.0.state);
        state.gate = Gate::Closed;
        state.wrote = false;
    }

    /// The number of the view the module may say something for now, if it may: what a frame is
    /// tagged with when it is queued.
    pub(crate) fn module_ticket(&self) -> Option<u64> {
        let state = lock(&self.0.state);
        (state.gate == Gate::Open).then_some(state.epoch)
    }

    /// Whether the writer may write (more of) a frame of the module's from view `epoch`:
    /// `started` is whether some of it is written already.
    pub(crate) fn module_may_write(&self, epoch: u64, started: bool) -> bool {
        let state = lock(&self.0.state);
        state.epoch == epoch
            && match state.gate {
                Gate::Open => true,
                Gate::Draining => started,
                Gate::Closed => false,
            }
    }

    /// The writer wrote a byte of the module's.
    pub(crate) fn wrote_module(&self) {
        lock(&self.0.state).wrote = true;
    }

    /// Whether the module's bytes reached the tty in this view, and so whether the program may
    /// have left some unread; the answer is given once.
    pub(crate) fn module_was_written(&self) -> bool {
        std::mem::take(&mut lock(&self.0.state).wrote)
    }

    /// Bytes waiting for the program, as a module is held to: for the tests and the shell's
    /// diagnostics.
    pub fn pending(&self) -> usize {
        lock(&self.0.state).pending()
    }

    /// Bytes of the program's messages the module has not taken.
    pub fn inbox(&self) -> usize {
        lock(&self.0.state).inbox
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    fn counted() -> (Flow, Arc<AtomicUsize>, Arc<AtomicUsize>) {
        let flow = Flow::new();
        let rooms = Arc::new(AtomicUsize::new(0));
        let reads = Arc::new(AtomicUsize::new(0));
        let count = Arc::clone(&rooms);
        flow.on_room(move || {
            count.fetch_add(1, Ordering::SeqCst);
        });
        let count = Arc::clone(&reads);
        flow.on_reader(move || {
            count.fetch_add(1, Ordering::SeqCst);
        });
        (flow, rooms, reads)
    }

    #[test]
    fn a_module_is_told_full_over_the_bound_and_woken_once_when_the_program_has_read() {
        let (flow, rooms, _) = counted();
        // The pane hands what the module says to the writer: counted in transit first, then in
        // the queue.
        let mut said = 0;
        while flow.may_say(64 << 10) {
            flow.queued(64 << 10);
            flow.said(64 << 10);
            said += 1;
            assert!(said < 100, "never told full");
        }
        assert_eq!(said, 4, "four messages of 64 KiB make the bound of 256 KiB");
        assert!(!flow.may_say(1));
        assert_eq!(rooms.load(Ordering::SeqCst), 0);
        // The program reads, but not enough: still no word.
        flow.written(64 << 10);
        flow.written(64 << 10);
        assert_eq!(rooms.load(Ordering::SeqCst), 0);
        // Down to the low mark: woken, once, however much it reads after.
        flow.written(64 << 10);
        assert_eq!(rooms.load(Ordering::SeqCst), 1);
        flow.written(64 << 10);
        assert_eq!(rooms.load(Ordering::SeqCst), 1);
        assert!(flow.may_say(1));
        assert_eq!(flow.pending(), 1);
    }

    #[test]
    fn a_module_that_was_never_told_full_is_never_woken() {
        let (flow, rooms, _) = counted();
        assert!(flow.may_say(100));
        flow.queued(100);
        flow.said(100);
        flow.written(100);
        assert_eq!(rooms.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn what_is_in_transit_counts_and_a_message_over_the_bound_is_not_cut() {
        let (flow, _, _) = counted();
        // One message the size of the protocol's limit is accepted from an empty backlog…
        assert!(flow.may_say(1 << 20));
        // …and then the module is full until the program has read it.
        assert!(!flow.may_say(1));
        flow.queued(1 << 20);
        flow.said(1 << 20);
        assert_eq!(flow.pending(), 1 << 20);
        assert!(!flow.may_say(1));
    }

    #[test]
    fn a_view_that_ends_while_the_module_is_full_forgets_the_wait() {
        let (flow, rooms, reads) = counted();
        assert!(flow.may_say(BACKLOG_LIMIT));
        assert!(!flow.may_say(1));
        flow.end();
        assert_eq!(flow.pending(), 0);
        assert!(flow.may_say(1));
        // It was not told full any more: reading later owes it nothing.
        flow.queued(10);
        flow.written(10);
        assert_eq!(rooms.load(Ordering::SeqCst), 0);
        // The loop was woken once, by the end, to drop what waits for the module: no more.
        assert_eq!(reads.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn the_reader_stops_over_the_inbox_bound_and_is_woken_once_when_the_module_has_taken_enough() {
        let (flow, _, reads) = counted();
        let message = 16 << 10;
        let mut heard = 0;
        while !flow.inbox_closed() {
            flow.heard(message);
            heard += 1;
            assert!(heard < 1000, "never closed");
        }
        // Closed stays closed until the low mark, however the module takes.
        assert!(flow.inbox() >= INBOX_LIMIT);
        for _ in 0..heard {
            assert!(flow.inbox_closed() || flow.inbox() <= INBOX_LOW);
            flow.taken(message);
        }
        assert_eq!(flow.inbox(), 0);
        assert_eq!(
            reads.load(Ordering::SeqCst),
            1,
            "woken when it crossed the low mark"
        );
        assert!(!flow.inbox_closed());
    }

    #[test]
    fn a_reader_that_never_stopped_is_not_woken() {
        let (flow, _, reads) = counted();
        flow.heard(10);
        flow.taken(10);
        assert_eq!(reads.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn the_end_of_the_view_opens_the_reader_and_drops_the_inbox() {
        let (flow, _, reads) = counted();
        while !flow.inbox_closed() {
            flow.heard(64 << 10);
        }
        flow.end();
        assert_eq!(reads.load(Ordering::SeqCst), 1);
        assert_eq!(flow.inbox(), 0);
        assert!(!flow.inbox_closed());
        // What the module takes of the old view's messages afterwards is not counted below zero.
        flow.taken(1 << 20);
        assert_eq!(flow.inbox(), 0);
    }

    #[test]
    fn the_gate_on_the_modules_frames_follows_the_life_of_the_view() {
        let flow = Flow::new();
        flow.begin();
        let epoch = flow.module_ticket().expect("the view is up");
        assert!(flow.module_may_write(epoch, false) && flow.module_may_write(epoch, true));
        // The view ends: the frame under way is finished, and no other is begun.
        flow.end();
        assert_eq!(flow.module_ticket(), None);
        assert!(!flow.module_may_write(epoch, false));
        assert!(flow.module_may_write(epoch, true));
        // A signal character is going to the program: not even that one.
        flow.interrupt();
        assert!(!flow.module_may_write(epoch, true));
        // The end of the view does not open it again, nor soften it.
        flow.end();
        assert!(!flow.module_may_write(epoch, true));
        // The next view has a number of its own, and the frames of the one before are no one's.
        flow.begin();
        let next = flow.module_ticket().expect("the next view is up");
        assert_ne!(next, epoch);
        assert!(!flow.module_may_write(epoch, true));
        assert!(flow.module_may_write(next, false));
    }

    #[test]
    fn an_interrupt_shuts_the_gate_before_anything_else_asks_and_the_end_of_a_view_wakes_the_loop()
    {
        let (flow, _, reads) = counted();
        flow.begin();
        let epoch = flow.module_ticket().expect("open");
        flow.interrupt();
        assert_eq!(flow.module_ticket(), None);
        assert!(!flow.module_may_write(epoch, false));
        assert_eq!(
            reads.load(Ordering::SeqCst),
            0,
            "the character that follows wakes the loop"
        );
        flow.end();
        assert_eq!(reads.load(Ordering::SeqCst), 1, "the loop drops what waits");
    }

    #[test]
    fn whether_the_module_reached_the_tty_is_told_once_and_forgotten_by_the_next_view() {
        let flow = Flow::new();
        flow.begin();
        assert!(!flow.module_was_written());
        flow.wrote_module();
        assert!(flow.module_was_written());
        assert!(!flow.module_was_written());
        flow.wrote_module();
        flow.begin();
        assert!(!flow.module_was_written());
    }

    /// A module that is told full and a program that reads in bursts, on threads of their own: no
    /// wake is lost between "told full" and "woken", and everything the module meant to say is
    /// said.
    #[test]
    fn a_module_told_full_while_the_program_reads_is_always_woken() {
        use std::sync::mpsc::{RecvTimeoutError, channel};
        let flow = Flow::new();
        let (woken, room) = channel::<()>();
        flow.on_room(move || {
            let _ = woken.send(());
        });
        let total = 400;
        let size = 40 << 10;
        let pane = flow.clone();
        let (written_tx, written_rx) = channel::<usize>();
        let writer = {
            let flow = flow.clone();
            std::thread::spawn(move || {
                // The program reads what has been queued, a piece at a time.
                while let Ok(bytes) = written_rx.recv() {
                    let mut left = bytes;
                    while left > 0 {
                        let piece = left.min(7 << 10);
                        flow.written(piece);
                        left -= piece;
                        std::thread::yield_now();
                    }
                }
            })
        };
        let mut sent = 0;
        let mut waits = 0;
        while sent < total {
            if pane.may_say(size) {
                // The pane's side of it: the message reaches the writer's queue.
                flow.queued(size);
                flow.said(size);
                written_tx.send(size).unwrap();
                sent += 1;
            } else {
                waits += 1;
                match room.recv_timeout(Duration::from_secs(5)) {
                    Ok(()) | Err(RecvTimeoutError::Disconnected) => {}
                    Err(RecvTimeoutError::Timeout) => panic!("the module was never woken"),
                }
            }
        }
        drop(written_tx);
        writer.join().unwrap();
        assert!(waits > 0, "the test never made the module wait");
        assert_eq!(flow.pending(), 0);
    }
}
