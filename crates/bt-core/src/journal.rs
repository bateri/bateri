//! The journal: what a pane's `Term` went through since its last base,
//! kept so another process can rebuild the screen after bateri dies.
//!
//! **What is recorded.** Every byte the reader reads from the PTY, in read
//! order (`TappedPty::read` — a byte read but not yet parsed is not lost),
//! and every other change to `Term`'s content as a **side record** stamped
//! with how many PTY bytes had been applied when it happened: a resize, ⌘K
//! and ⌥⌘K by their resolved effect (the rows scrolled out above the kept
//! block — the shell ledger that picked them is not replayed), a change of
//! the terminal options, the cursor style reset on leaving the alternate
//! screen, and the DEC 2026 timeout's `stop_sync`. Whatever else writes
//! `Term` — scrolling, selection, search — touches only what the snapshot
//! does not read, and says so in its doc.
//!
//! **Two streams, not one.** The bytes are written by the reader thread
//! alone, outside the terminal lock for the reads before it takes it; the
//! side records only under the terminal lock (the reader's timeout arm
//! included). One stream would need a lock between the two and the main
//! thread would wait behind a megabyte's copy; two streams have one writer
//! each at any moment, the byte stream needs no per-read header (so the
//! read gate's bound is exact), and the side stream is the main thread's
//! reserved room: when it is full the journal **breaks** rather than wait.
//! The stamp, not the position, orders them: the reader records bytes
//! before it applies them, so a side record may follow bytes it precedes.
//!
//! **The copy** ([`Journal::compact`]) replays the base and the journal
//! into a scratch `Term` — the replay block of `Session::assemble`: a fresh
//! parser through [`ClusterHandler`] and the session's `Config`; no scanner,
//! no shell ledger, and what the scratch writes to its PTY goes nowhere —
//! and encodes it with the update handover's encoder, whose destructive
//! probes are free on a scratch. The live `Term` is never touched. The cut
//! is at the bytes applied when the copy started, never further: every side
//! record stamped earlier is in the copy, every later one is stamped at
//! least there. What the parser holds at the cut travels with the base —
//! the bytes after its last ground state ([`Tail`], replayed after the VT by
//! a second fresh parser, the handover's order) and the cluster wrapper's
//! open-cluster bit; inside a synchronized update there is no cut (its
//! buffer is the parser's, not `Term`'s), the copy gives up and is asked
//! again after more output.
//!
//! **Known limit:** `CSI b` (repeat the preceding character) right after a
//! cut repeats nothing — the parser's preceding character is private to
//! vte and does not travel. Programs send the character and the repeat in
//! one write; a cut between them needs a read to end exactly there.
//!
//! **The body is the platform's** ([`JournalStore`], the [`crate::PtyOps`]
//! precedent: `bt-core` has no `libc`): shared memory a holder process
//! keeps, or the in-process body here ([`Journal::in_memory`]) for the
//! timed run and the tests, which frees what a compaction folded into the
//! base itself. The format is an internal detail of one build
//! ([`JOURNAL_FORMAT`]).

use std::collections::VecDeque;
use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};
use std::thread;
use std::time::Duration;

use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::term::Term;
use alacritty_terminal::vte::ansi::{self, Handler as _, Timeout as _};

use crate::handler::ClusterHandler;
use crate::reader::{MAX_LOCKED_READ, READ_BUFFER_SIZE};
use crate::session::{GridSize, Osc52, TerminalOptions, term_config, wipe};
use crate::settings::{CaretShape, CursorBlink};
use crate::snapshot::{self, Tail};

/// The journal format's version — the side records' layout and the meaning
/// of the two streams. Only one build reads what it wrote; a body that
/// hands the journal to another process writes this beside it.
pub const JOURNAL_FORMAT: u32 = 1;

/// The room one `pty_read` may need: it reads up to `READ_BUFFER_SIZE` before
/// it holds the terminal lock and, holding it, keeps reading into the whole
/// buffer until `MAX_LOCKED_READ` went through — so less than the sum. The
/// read gate closes when the byte stream has less free.
pub(crate) const GATE_ROOM: usize = READ_BUFFER_SIZE + MAX_LOCKED_READ;

/// How much earlier than the gate the compaction is asked for: one more
/// read's worth, so output keeps flowing while a compaction runs.
/// A design constant; the bodies size their streams around it.
const MARGIN: usize = GATE_ROOM;

/// The least journal a compaction waits for. The trigger is "the journal is
/// as long as the base" — each compaction replays both, so the work stays
/// linear in the output — and a fresh pane's base is a few bytes, which
/// would compact every few kilobytes. A design constant.
const BASE_FLOOR: usize = 1 << 20;

/// After a compaction that found no cut (the bytes applied ended inside a
/// synchronized update), how much more output before the next try — each
/// try replays the whole base. A new side record retries at once: the
/// update's timeout is one.
const RETRY_STEP: u64 = 64 << 10;

/// How many bytes the copy replays between two steps of its progress
/// count ([`Journal::progress`]).
const CHUNK: usize = 64 << 10;

/// How long the read gate waits for the compaction's progress before the
/// journal breaks: a compaction that replays steps every `CHUNK`, so
/// seconds of silence mean it died or hangs. A design constant.
pub const STALL: Duration = Duration::from_secs(2);

/// The in-process body's byte stream: large enough that the trigger
/// ([`due`]) comes long before the gate for the timed run's scrollback.
const MEMORY_BYTES: usize = 16 << 20;

/// The in-process body's side stream — the main thread's reserved room.
/// A side record is at most [`SIDE_MAX`] bytes; this holds thousands, and
/// half of it asks for a compaction.
const MEMORY_SIDES: usize = 64 << 10;

/// The in-process byte stream's storage step: full blocks are shared with
/// the compaction's read, so a read never copies under the lock more than
/// the block being filled.
const BLOCK: usize = 64 << 10;

/// Whether a compaction is due: the journal is at least as long as the base
/// (never less than [`BASE_FLOOR`]) — or, however short, it fills the
/// stream up to the gate's room and [`MARGIN`]: a large base (a long
/// scrollback) would otherwise not trigger before the gate closes.
pub fn due(journal: usize, base: usize, capacity: usize) -> bool {
    let room = capacity.saturating_sub(GATE_ROOM + MARGIN);
    journal > 0 && journal >= base.max(BASE_FLOOR).min(room)
}

/// One of the journal's two streams.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JournalStream {
    /// The PTY's bytes, in read order; written by the reader thread alone.
    Bytes,
    /// The side records; written only under the session's terminal lock.
    Sides,
}

/// What [`JournalStore::read`] gives: each stream from its oldest kept byte,
/// with that byte's absolute position.
#[derive(Debug, Default)]
pub struct JournalRecords {
    pub bytes_at: u64,
    pub bytes: Vec<u8>,
    pub sides_at: u64,
    pub sides: Vec<u8>,
}

/// Where a base cut the journal: the absolute positions in the two streams
/// the journal continues from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct JournalCut {
    /// The PTY bytes before it are in the base.
    pub pty: u64,
    /// The side records before it are in the base.
    pub side: u64,
}

/// The journal's storage — the platform's body.
///
/// Two writers, never at once on the same stream: the byte stream is the
/// reader thread's, the side stream is written only under the session's
/// terminal lock. Neither may wait on the compaction's read.
pub trait JournalStore: Send + Sync {
    /// Copies `parts` after the stream's last byte, all of them or none.
    /// `false`: they did not fit, the stream is unchanged.
    fn append(&self, stream: JournalStream, parts: &[&[u8]]) -> bool;
    /// The stream's bytes kept.
    fn used(&self, stream: JournalStream) -> usize;
    /// The most the stream keeps.
    fn capacity(&self, stream: JournalStream) -> usize;
    /// Both streams from their oldest kept byte.
    fn read(&self) -> JournalRecords;
    /// Forgets the bytes before `to` in both streams — what a base holds.
    fn release(&self, to: JournalCut);
    /// Marks the journal broken; nothing is written after.
    fn set_broken(&self);
    fn is_broken(&self) -> bool;
    /// A compaction is due ([`Journal::poke`]): start one that calls
    /// [`Journal::compact`] and, once its base is safe, [`Journal::release`].
    /// Called outside any lock. `false`: none started.
    fn wake(&self, journal: &Arc<Journal>) -> bool;
}

/// The base a journal continues from: a VT the copy encoded (or the bytes
/// the session was born with), and what the parser held at the cut.
#[derive(Clone, Debug)]
pub(crate) struct Base {
    pub(crate) vt: Vec<u8>,
    /// The bytes after the parser's last ground state; replayed after `vt`
    /// by the journal's parser.
    pub(crate) tail: Vec<u8>,
    /// The cluster wrapper's open-cluster bit at the cut.
    pub(crate) last_input: bool,
    /// The grid size and the options `vt` is laid out under.
    pub(crate) cols: u16,
    pub(crate) rows: u16,
    pub(crate) options: TerminalOptions,
    pub(crate) cluster: bool,
    pub(crate) at: JournalCut,
}

impl Base {
    /// The base a session is born with: what it replays before its reader
    /// starts, at its opening size and options.
    pub(crate) fn birth(
        vt: Vec<u8>,
        cols: u16,
        rows: u16,
        options: TerminalOptions,
        cluster: bool,
    ) -> Self {
        Self {
            vt,
            tail: Vec::new(),
            last_input: false,
            cols,
            rows,
            options,
            cluster,
            at: JournalCut::default(),
        }
    }

    fn len(&self) -> usize {
        self.vt.len() + self.tail.len()
    }
}

/// A change to `Term`'s content that is not a PTY byte.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Side {
    /// `Term::resize` to this grid.
    Resize { cols: u16, rows: u16 },
    /// ⌘K/⌥⌘K: `top` screen rows scrolled into the history (zero for ⌥⌘K),
    /// then the history erased.
    Clear { top: u32 },
    /// `Term::set_options` with these options.
    Options(TerminalOptions),
    /// `Term::set_cursor_style(None)` on leaving the alternate screen.
    CursorStyleReset,
    /// The DEC 2026 timeout: the parser's buffer applied.
    StopSync,
}

/// A side record's most bytes: tag, stamp, the options' payload.
const SIDE_MAX: usize = 1 + 8 + 7;

const RESIZE: u8 = 1;
const CLEAR: u8 = 2;
const OPTIONS: u8 = 3;
const CURSOR_STYLE: u8 = 4;
const STOP_SYNC: u8 = 5;

impl Side {
    /// `[tag][stamp: u64 LE][payload]`; the record's length.
    fn encode(self, stamp: u64, out: &mut [u8; SIDE_MAX]) -> usize {
        let mut payload = [0u8; SIDE_MAX - 9];
        let (tag, len) = match self {
            Self::Resize { cols, rows } => {
                let [c0, c1] = cols.to_le_bytes();
                let [r0, r1] = rows.to_le_bytes();
                payload = [c0, c1, r0, r1, 0, 0, 0];
                (RESIZE, 4)
            }
            Self::Clear { top } => {
                let [t0, t1, t2, t3] = top.to_le_bytes();
                payload = [t0, t1, t2, t3, 0, 0, 0];
                (CLEAR, 4)
            }
            Self::Options(options) => {
                let scrollback = u32::try_from(options.scrollback).unwrap_or(u32::MAX);
                let [s0, s1, s2, s3] = scrollback.to_le_bytes();
                let osc52 = match options.osc52 {
                    Osc52::Off => 0,
                    Osc52::Copy => 1,
                };
                let cursor = match options.cursor {
                    CaretShape::Block => 0,
                    CaretShape::Underline => 1,
                    CaretShape::Beam => 2,
                };
                let blink = match options.blink {
                    CursorBlink::Auto => 0,
                    CursorBlink::On => 1,
                    CursorBlink::Off => 2,
                };
                payload = [s0, s1, s2, s3, osc52, cursor, blink];
                (OPTIONS, 7)
            }
            Self::CursorStyleReset => (CURSOR_STYLE, 0),
            Self::StopSync => (STOP_SYNC, 0),
        };
        let [b0, b1, b2, b3, b4, b5, b6, b7] = stamp.to_le_bytes();
        let [p0, p1, p2, p3, p4, p5, p6] = payload;
        *out = [
            tag, b0, b1, b2, b3, b4, b5, b6, b7, p0, p1, p2, p3, p4, p5, p6,
        ];
        9 + len
    }

    /// One record from the head of `bytes`: the side, its stamp and its
    /// length. `None`: not a record of this format.
    fn decode(bytes: &[u8]) -> Option<(Self, u64, usize)> {
        let tag = *bytes.first()?;
        let stamp = u64::from_le_bytes(bytes.get(1..9)?.try_into().ok()?);
        let len = match tag {
            RESIZE | CLEAR => 4,
            OPTIONS => 7,
            CURSOR_STYLE | STOP_SYNC => 0,
            _ => return None,
        };
        let mut payload = [0u8; SIDE_MAX - 9];
        payload
            .get_mut(..len)?
            .copy_from_slice(bytes.get(9..9 + len)?);
        let [p0, p1, p2, p3, p4, p5, p6] = payload;
        let side = match tag {
            RESIZE => Self::Resize {
                cols: u16::from_le_bytes([p0, p1]),
                rows: u16::from_le_bytes([p2, p3]),
            },
            CLEAR => Self::Clear {
                top: u32::from_le_bytes([p0, p1, p2, p3]),
            },
            OPTIONS => Self::Options(TerminalOptions {
                scrollback: u32::from_le_bytes([p0, p1, p2, p3]) as usize,
                osc52: match p4 {
                    0 => Osc52::Off,
                    1 => Osc52::Copy,
                    _ => return None,
                },
                cursor: match p5 {
                    0 => CaretShape::Block,
                    1 => CaretShape::Underline,
                    2 => CaretShape::Beam,
                    _ => return None,
                },
                blink: match p6 {
                    0 => CursorBlink::Auto,
                    1 => CursorBlink::On,
                    2 => CursorBlink::Off,
                    _ => return None,
                },
            }),
            CURSOR_STYLE => Self::CursorStyleReset,
            _ => Self::StopSync,
        };
        Some((side, stamp, 9 + len))
    }
}

/// A side record read back: where it starts in the side stream, its stamp.
struct SideAt {
    at: u64,
    stamp: u64,
    side: Side,
}

/// A pane's journal: the two streams in the platform's body, the counts the
/// reader and the main thread keep, and the base the journal continues from.
pub struct Journal {
    store: Box<dyn JournalStore>,
    stall: Duration,
    /// The PTY bytes applied to the live `Term`: the reader adds the slice
    /// it gave `advance`, under the terminal lock; the side records' stamp.
    applied: AtomicU64,
    /// The PTY bytes recorded — the byte stream's end.
    recorded: AtomicU64,
    /// The side records written.
    sides: AtomicU64,
    /// Steps of the compaction's work; the read gate's stall watch reads it.
    progress: AtomicU64,
    /// Whether a compaction was started and has not finished.
    compacting: AtomicBool,
    /// After a compaction found no cut: the recorded bytes and the side
    /// records the next one waits for ([`RETRY_STEP`]).
    retry_bytes: AtomicU64,
    retry_sides: AtomicU64,
    /// The base; `None` until the session seeds it.
    base: Mutex<Option<Arc<Base>>>,
    /// The base's length, for the trigger without the lock.
    base_len: AtomicUsize,
    /// One compaction at a time.
    one: Mutex<()>,
    /// The reader loop's wake ([`crate::reader::Msg::Room`]).
    room: OnceLock<Box<dyn Fn() + Send + Sync>>,
}

impl fmt::Debug for Journal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Journal")
            .field("applied", &self.applied.load(Ordering::Relaxed))
            .field("recorded", &self.recorded.load(Ordering::Relaxed))
            .field("broken", &self.store.is_broken())
            .finish_non_exhaustive()
    }
}

impl Journal {
    /// A journal in `store`; `stall` is how long the read gate waits for
    /// the compaction's progress ([`STALL`] outside the tests).
    pub fn new(store: Box<dyn JournalStore>, stall: Duration) -> Arc<Self> {
        Arc::new(Self {
            store,
            stall,
            applied: AtomicU64::new(0),
            recorded: AtomicU64::new(0),
            sides: AtomicU64::new(0),
            progress: AtomicU64::new(0),
            compacting: AtomicBool::new(false),
            retry_bytes: AtomicU64::new(0),
            retry_sides: AtomicU64::new(0),
            base: Mutex::new(None),
            base_len: AtomicUsize::new(0),
            one: Mutex::new(()),
            room: OnceLock::new(),
        })
    }

    /// A journal kept in this process, compacting itself on its own thread
    /// and freeing what a base holds at once — no holder confirms it. The
    /// timed run's `BT_JOURNAL`.
    pub fn in_memory() -> Arc<Self> {
        Self::new(
            Box::new(MemoryStore::new(MEMORY_BYTES, MEMORY_SIDES, true)),
            STALL,
        )
    }

    /// The base the session starts from — once, before its reader starts.
    pub(crate) fn seed(&self, base: Base) {
        self.base_len.store(base.len(), Ordering::Relaxed);
        *lock(&self.base) = Some(Arc::new(base));
    }

    /// The reader loop's wake for [`Journal::release`] and a break.
    pub(crate) fn set_room(&self, room: Box<dyn Fn() + Send + Sync>) {
        let _ = self.room.set(room);
    }

    fn room(&self) {
        if let Some(room) = self.room.get() {
            room();
        }
    }

    /// Records the bytes a read gave — the reader thread, maybe under the
    /// terminal lock: a copy, no wait. A read the gate let through always
    /// fits; one that does not breaks the journal.
    pub(crate) fn record_bytes(&self, bytes: &[u8]) {
        if bytes.is_empty() || self.store.is_broken() {
            return;
        }
        if self.store.append(JournalStream::Bytes, &[bytes]) {
            self.recorded
                .fetch_add(bytes.len() as u64, Ordering::Release);
        } else {
            // No wake: the reader is the caller, and under the terminal lock
            // nothing makes a system call.
            self.store.set_broken();
        }
    }

    /// Counts the bytes `advance` applied — the reader, under the terminal
    /// lock.
    pub(crate) fn applied(&self, bytes: usize) {
        self.applied.fetch_add(bytes as u64, Ordering::Release);
    }

    /// Records `side`, stamped with the bytes applied — **under the terminal
    /// lock**, in the round that made the change. The side stream is the
    /// main thread's reserved room: when it is full the journal breaks,
    /// nothing waits.
    pub(crate) fn record(&self, side: Side) {
        if self.store.is_broken() {
            return;
        }
        let stamp = self.applied.load(Ordering::Acquire);
        let mut buf = [0; SIDE_MAX];
        let len = side.encode(stamp, &mut buf);
        if self.store.append(JournalStream::Sides, &[&buf[..len]]) {
            self.sides.fetch_add(1, Ordering::Release);
        } else {
            // No wake under the terminal lock: a reader behind a closed gate
            // sees the break at its stall deadline, and an open one does not
            // need it.
            self.store.set_broken();
        }
    }

    /// Whether the byte stream has room for one more `pty_read` — always
    /// once broken (nothing is written then).
    pub(crate) fn gate_open(&self) -> bool {
        let stream = JournalStream::Bytes;
        self.store.is_broken()
            || self
                .store
                .capacity(stream)
                .saturating_sub(self.store.used(stream))
                >= GATE_ROOM
    }

    /// The compaction's progress count.
    pub(crate) fn progress(&self) -> u64 {
        self.progress.load(Ordering::Acquire)
    }

    /// How long the read gate waits for progress.
    pub(crate) fn stall(&self) -> Duration {
        self.stall
    }

    /// Asks the body for a compaction if one is due and none runs — outside
    /// any lock (the body may start a thread).
    pub(crate) fn poke(this: &Arc<Self>) {
        if this.store.is_broken() || !this.wants_compaction() {
            return;
        }
        if this.compacting.swap(true, Ordering::AcqRel) {
            return;
        }
        if !this.store.wake(this) {
            this.compacting.store(false, Ordering::Release);
        }
    }

    fn wants_compaction(&self) -> bool {
        let recorded = self.recorded.load(Ordering::Acquire);
        let sides = self.sides.load(Ordering::Acquire);
        if recorded < self.retry_bytes.load(Ordering::Acquire)
            && sides <= self.retry_sides.load(Ordering::Acquire)
        {
            return false;
        }
        let bytes = self.store.used(JournalStream::Bytes);
        let side_used = self.store.used(JournalStream::Sides);
        due(
            bytes,
            self.base_len.load(Ordering::Relaxed),
            self.store.capacity(JournalStream::Bytes),
        ) || side_used * 2 >= self.store.capacity(JournalStream::Sides)
    }

    /// The copy: replays the base and the journal up to the bytes applied
    /// now into a scratch `Term`, encodes it as the new base and returns
    /// where it cut — the caller releases the journal there once the base
    /// is safe ([`Journal::release`]). `None`: no cut (nothing new, the
    /// bytes applied end inside a synchronized update, the journal broke).
    /// One at a time; the live `Term` is not touched.
    pub fn compact(&self) -> Option<JournalCut> {
        self.compact_until(u64::MAX)
    }

    /// [`Journal::compact`], cutting no later than `limit` PTY bytes.
    pub(crate) fn compact_until(&self, limit: u64) -> Option<JournalCut> {
        let one = lock(&self.one);
        let cut = self.compact_locked(limit);
        drop(one);
        self.compacting.store(false, Ordering::Release);
        cut
    }

    fn compact_locked(&self, limit: u64) -> Option<JournalCut> {
        if self.store.is_broken() {
            return None;
        }
        // The applied count **before** the records: every side record stamped
        // below it was written before it was reached, so it is in the read.
        let applied = self.applied.load(Ordering::Acquire);
        let recorded = self.recorded.load(Ordering::Acquire);
        let sides_written = self.sides.load(Ordering::Acquire);
        let records = self.store.read();
        let base = lock(&self.base).clone()?;
        let limit = limit.min(applied);
        let Some(replay) = Replay::new(&base, &records, limit) else {
            eprintln!("bateri: the journal does not read back, it breaks");
            self.break_journal();
            return None;
        };
        if replay.nothing_new() {
            return None;
        }
        match replay.run(&self.progress) {
            Some(next) => {
                let cut = next.at;
                self.base_len.store(next.len(), Ordering::Relaxed);
                *lock(&self.base) = Some(Arc::new(next));
                self.retry_bytes.store(0, Ordering::Release);
                self.retry_sides.store(0, Ordering::Release);
                Some(cut)
            }
            None => {
                self.retry_bytes
                    .store(recorded.saturating_add(RETRY_STEP), Ordering::Release);
                self.retry_sides.store(sides_written, Ordering::Release);
                None
            }
        }
    }

    /// Forgets what the base at `cut` holds and wakes the reader: the gate
    /// may open. Counts as the compaction's progress.
    pub fn release(&self, cut: JournalCut) {
        self.store.release(cut);
        self.progress.fetch_add(1, Ordering::AcqRel);
        self.room();
    }

    /// Breaks the journal: nothing is written after, the read gate stays
    /// open, and a crash brings this pane's screen back from its program's
    /// redraw. Wakes the reader, whose gate may be closed.
    pub fn break_journal(&self) {
        self.store.set_broken();
        self.room();
    }

    pub fn is_broken(&self) -> bool {
        self.store.is_broken()
    }
}

#[cfg(test)]
impl Journal {
    /// The PTY bytes applied to the live `Term`.
    pub(crate) fn applied_bytes(&self) -> u64 {
        self.applied.load(Ordering::Acquire)
    }

    /// The side records written.
    pub(crate) fn sides_written(&self) -> u64 {
        self.sides.load(Ordering::Acquire)
    }

    /// The current base.
    pub(crate) fn base(&self) -> Option<Arc<Base>> {
        lock(&self.base).clone()
    }

    /// A step of progress, as a compaction would make.
    pub(crate) fn step(&self) {
        self.progress.fetch_add(1, Ordering::AcqRel);
    }

    /// The side records still in the store, decoded.
    pub(crate) fn side_records(&self) -> Vec<Side> {
        let records = self.store.read();
        decode_sides(records.sides_at, &records.sides)
            .unwrap_or_default()
            .into_iter()
            .map(|side| side.side)
            .collect()
    }
}

/// The side records in `bytes`, which start at `at`; `None` if one does not
/// decode.
fn decode_sides(at: u64, bytes: &[u8]) -> Option<Vec<SideAt>> {
    let mut sides = Vec::new();
    let mut offset = 0;
    while offset < bytes.len() {
        let (side, stamp, len) = Side::decode(&bytes[offset..])?;
        sides.push(SideAt {
            at: at + offset as u64,
            stamp,
            side,
        });
        offset += len;
    }
    Some(sides)
}

/// One compaction's input: the base, the journal after it and the limit.
struct Replay<'a> {
    base: &'a Base,
    /// The PTY bytes after the base, up to the limit.
    bytes: &'a [u8],
    sides: Vec<SideAt>,
    /// Where the side stream read ends.
    sides_end: u64,
    limit: u64,
}

impl<'a> Replay<'a> {
    /// `None` if the records do not line up with the base: the body forgot
    /// what the base still needs, or a record does not decode.
    fn new(base: &'a Base, records: &'a JournalRecords, limit: u64) -> Option<Self> {
        let skip = usize::try_from(base.at.pty.checked_sub(records.bytes_at)?).ok()?;
        let bytes = records.bytes.get(skip..)?;
        let limit = limit.max(base.at.pty);
        let take = usize::try_from(limit - base.at.pty).ok()?;
        let bytes = bytes.get(..take)?;
        let side_skip = usize::try_from(base.at.side.checked_sub(records.sides_at)?).ok()?;
        let sides = decode_sides(base.at.side, records.sides.get(side_skip..)?)?;
        Some(Self {
            base,
            bytes,
            sides,
            sides_end: records.sides_at + records.sides.len() as u64,
            limit,
        })
    }

    /// No byte and no side record to fold in.
    fn nothing_new(&self) -> bool {
        self.bytes.is_empty() && self.sides.iter().all(|side| side.stamp > self.limit)
    }

    /// Feeds the scratch up to the limit, the side records at their stamps,
    /// and encodes the new base; `None` inside a synchronized update.
    fn run(self, progress: &AtomicU64) -> Option<Base> {
        let mut scratch = Scratch::new(self.base, progress);
        let start = self.base.at.pty;
        let mut fed = start;
        let mut next = 0;
        loop {
            while let Some(side) = self.sides.get(next)
                && side.stamp <= fed
            {
                scratch.apply(side.side);
                next += 1;
            }
            if fed >= self.limit {
                break;
            }
            let stop = self
                .sides
                .get(next)
                .map_or(self.limit, |side| side.stamp.min(self.limit))
                .min(fed + CHUNK as u64);
            let from = (fed - start) as usize;
            let to = (stop - start) as usize;
            // In range by construction: `bytes` ends at the limit.
            scratch.feed(self.bytes.get(from..to).unwrap_or_default());
            fed = stop;
            progress.fetch_add(1, Ordering::AcqRel);
        }
        if scratch.in_sync() {
            return None;
        }
        let side = self.sides.get(next).map_or(self.sides_end, |side| side.at);
        Some(scratch.finish(
            JournalCut {
                pty: self.limit,
                side,
            },
            progress,
        ))
    }
}

/// The title as `Title`/`ResetTitle` leave it — `Adapter`'s slot, for the
/// scratch. `Term::set_options` sends one too, from `Term`'s own title.
#[derive(Clone, Default)]
struct TitleSlot(Arc<Mutex<Option<String>>>);

impl EventListener for TitleSlot {
    fn send_event(&self, event: Event) {
        match event {
            Event::Title(title) => *lock(&self.0) = Some(title),
            Event::ResetTitle => *lock(&self.0) = None,
            _ => {}
        }
    }
}

/// The scratch `Term` a copy replays into, with the parser state the
/// journal's bytes continue.
struct Scratch {
    term: Term<TitleSlot>,
    title: TitleSlot,
    parser: ansi::Processor,
    last_input: bool,
    tail: Tail,
    options: TerminalOptions,
    cluster: bool,
}

impl Scratch {
    /// The base replayed: its VT through one fresh parser, its tail through
    /// the journal's — the handover's order (VT, then the carried bytes).
    /// The VT goes in [`CHUNK`]s, each a step of `progress`: a long
    /// scrollback's replay is work the stall watch must see.
    fn new(base: &Base, progress: &AtomicU64) -> Self {
        let title = TitleSlot::default();
        let grid = GridSize::for_spawn(base.cols, base.rows);
        let mut term = Term::new(term_config(base.options), &grid, title.clone());
        let mut parser: ansi::Processor = ansi::Processor::new();
        let mut last_input = false;
        for chunk in base.vt.chunks(CHUNK) {
            parser.advance(
                &mut ClusterHandler::new(&mut term, base.cluster, &mut last_input),
                chunk,
            );
            progress.fetch_add(1, Ordering::AcqRel);
        }
        let mut scratch = Self {
            term,
            title,
            parser: ansi::Processor::new(),
            last_input: base.last_input,
            tail: Tail::default(),
            options: base.options,
            cluster: base.cluster,
        };
        scratch.feed(&base.tail);
        scratch
    }

    /// The journal's PTY bytes, as the reader's `advance` gave them.
    fn feed(&mut self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        self.parser.advance(
            &mut ClusterHandler::new(&mut self.term, self.cluster, &mut self.last_input),
            bytes,
        );
        self.tail.feed(bytes);
    }

    /// A side record, as the live session made it.
    fn apply(&mut self, side: Side) {
        match side {
            Side::Resize { cols, rows } => {
                if cols > 0 && rows > 0 {
                    self.term.resize(GridSize::exact(cols, rows));
                }
            }
            Side::Clear { top } => wipe(&mut self.term, top as usize),
            Side::Options(options) => {
                self.term.set_options(term_config(options));
                self.options = options;
            }
            Side::CursorStyleReset => self.term.set_cursor_style(None),
            Side::StopSync => self.parser.stop_sync(&mut ClusterHandler::new(
                &mut self.term,
                self.cluster,
                &mut self.last_input,
            )),
        }
    }

    /// Whether the parser is inside a synchronized update.
    fn in_sync(&self) -> bool {
        self.parser.sync_timeout().pending_timeout()
    }

    /// The new base — destructive: the encoder probes the scratch. Its walk
    /// steps `progress` as it goes.
    fn finish(mut self, at: JournalCut, progress: &AtomicU64) -> Base {
        let cols = u16::try_from(self.term.columns()).unwrap_or(u16::MAX);
        let rows = u16::try_from(self.term.screen_lines()).unwrap_or(u16::MAX);
        let slot = self.title.clone();
        let (vt, _) =
            snapshot::encode_live(&mut self.term, &move || lock(&slot.0).clone(), &|| {
                progress.fetch_add(1, Ordering::AcqRel);
            });
        Base {
            vt,
            tail: self.tail.bytes().to_vec(),
            last_input: self.last_input,
            cols,
            rows,
            options: self.options,
            cluster: self.cluster,
            at,
        }
    }
}

/// The in-process body: both streams in memory.
pub(crate) struct MemoryStore {
    bytes: Mutex<ByteStream>,
    sides: Mutex<SideStream>,
    bytes_used: AtomicUsize,
    sides_used: AtomicUsize,
    bytes_capacity: usize,
    sides_capacity: usize,
    broken: AtomicBool,
    /// Whether [`JournalStore::wake`] compacts on its own thread; the tests
    /// that drive the compaction themselves turn it off.
    auto: bool,
}

/// The byte stream: full blocks, shared with a read, and the block being
/// filled.
#[derive(Default)]
struct ByteStream {
    full: VecDeque<Arc<Vec<u8>>>,
    filling: Vec<u8>,
    /// The absolute position of the first byte stored (in `full[0]` or
    /// `filling`).
    first: u64,
    /// The first byte kept: what a release left.
    start: u64,
    end: u64,
}

#[derive(Default)]
struct SideStream {
    bytes: VecDeque<u8>,
    start: u64,
}

impl MemoryStore {
    pub(crate) fn new(bytes_capacity: usize, sides_capacity: usize, auto: bool) -> Self {
        Self {
            bytes: Mutex::new(ByteStream::default()),
            sides: Mutex::new(SideStream::default()),
            bytes_used: AtomicUsize::new(0),
            sides_used: AtomicUsize::new(0),
            bytes_capacity,
            sides_capacity,
            broken: AtomicBool::new(false),
            auto,
        }
    }
}

impl JournalStore for MemoryStore {
    fn append(&self, stream: JournalStream, parts: &[&[u8]]) -> bool {
        let len: usize = parts.iter().map(|part| part.len()).sum();
        match stream {
            JournalStream::Bytes => {
                let mut store = lock(&self.bytes);
                let used = (store.end - store.start) as usize;
                if used + len > self.bytes_capacity {
                    return false;
                }
                for part in parts {
                    let mut part = *part;
                    while !part.is_empty() {
                        let room = BLOCK - store.filling.len();
                        let (now, rest) = part.split_at(room.min(part.len()));
                        store.filling.extend_from_slice(now);
                        part = rest;
                        if store.filling.len() == BLOCK {
                            let block =
                                std::mem::replace(&mut store.filling, Vec::with_capacity(BLOCK));
                            store.full.push_back(Arc::new(block));
                        }
                    }
                }
                store.end += len as u64;
                self.bytes_used
                    .store((store.end - store.start) as usize, Ordering::Release);
                true
            }
            JournalStream::Sides => {
                let mut store = lock(&self.sides);
                if store.bytes.len() + len > self.sides_capacity {
                    return false;
                }
                for part in parts {
                    store.bytes.extend(part.iter().copied());
                }
                self.sides_used.store(store.bytes.len(), Ordering::Release);
                true
            }
        }
    }

    fn used(&self, stream: JournalStream) -> usize {
        match stream {
            JournalStream::Bytes => self.bytes_used.load(Ordering::Acquire),
            JournalStream::Sides => self.sides_used.load(Ordering::Acquire),
        }
    }

    fn capacity(&self, stream: JournalStream) -> usize {
        match stream {
            JournalStream::Bytes => self.bytes_capacity,
            JournalStream::Sides => self.sides_capacity,
        }
    }

    fn read(&self) -> JournalRecords {
        let (full, filling, first, start) = {
            let store = lock(&self.bytes);
            (
                store.full.iter().cloned().collect::<Vec<_>>(),
                store.filling.clone(),
                store.first,
                store.start,
            )
        };
        let (sides, sides_at) = {
            let store = lock(&self.sides);
            (store.bytes.iter().copied().collect(), store.start)
        };
        let skip = (start - first) as usize;
        let mut bytes =
            Vec::with_capacity((full.len() * BLOCK + filling.len()).saturating_sub(skip));
        for block in &full {
            bytes.extend_from_slice(block);
        }
        bytes.extend_from_slice(&filling);
        bytes.drain(..skip.min(bytes.len()));
        JournalRecords {
            bytes_at: start,
            bytes,
            sides_at,
            sides,
        }
    }

    fn release(&self, to: JournalCut) {
        {
            let mut store = lock(&self.bytes);
            let to_pty = to.pty.min(store.end);
            if to_pty > store.start {
                store.start = to_pty;
                while let Some(block) = store.full.front() {
                    let block_end = store.first + block.len() as u64;
                    if block_end > store.start {
                        break;
                    }
                    store.first = block_end;
                    store.full.pop_front();
                }
                self.bytes_used
                    .store((store.end - store.start) as usize, Ordering::Release);
            }
        }
        let mut store = lock(&self.sides);
        let end = store.start + store.bytes.len() as u64;
        let to_side = to.side.min(end);
        if to_side > store.start {
            let drop = (to_side - store.start) as usize;
            store.bytes.drain(..drop);
            store.start = to_side;
            self.sides_used.store(store.bytes.len(), Ordering::Release);
        }
    }

    /// Only the flag: it may be set under the terminal lock, where taking a
    /// stream's lock or freeing its memory has no place. A broken journal's
    /// memory goes with the journal.
    fn set_broken(&self) {
        self.broken.store(true, Ordering::Release);
    }

    fn is_broken(&self) -> bool {
        self.broken.load(Ordering::Acquire)
    }

    fn wake(&self, journal: &Arc<Journal>) -> bool {
        if !self.auto {
            return false;
        }
        let journal = Arc::clone(journal);
        thread::Builder::new()
            .name("PTY journal".to_owned())
            .spawn(move || {
                // No holder confirms the base: what it holds goes now.
                if let Some(cut) = journal.compact() {
                    journal.release(cut);
                }
            })
            .is_ok()
    }
}

/// Takes the content back from a poisoned lock: the guarded data is a byte
/// queue and a base, whole at any moment.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options() -> TerminalOptions {
        TerminalOptions {
            scrollback: 100,
            osc52: Osc52::Copy,
            cursor: CaretShape::Beam,
            blink: CursorBlink::Auto,
        }
    }

    #[test]
    fn every_side_record_reads_back() {
        let sides = [
            Side::Resize { cols: 80, rows: 24 },
            Side::Clear { top: 7 },
            Side::Clear { top: 0 },
            Side::Options(options()),
            Side::Options(TerminalOptions {
                scrollback: 100_000,
                osc52: Osc52::Off,
                cursor: CaretShape::Underline,
                blink: CursorBlink::Off,
            }),
            Side::CursorStyleReset,
            Side::StopSync,
        ];
        let mut stream = Vec::new();
        for (stamp, side) in sides.iter().enumerate() {
            let mut buf = [0; SIDE_MAX];
            let len = side.encode(stamp as u64 * 1000, &mut buf);
            stream.extend_from_slice(&buf[..len]);
        }
        let back = decode_sides(40, &stream).expect("decodes");
        assert_eq!(back.len(), sides.len());
        for (index, (read, side)) in back.iter().zip(sides).enumerate() {
            assert_eq!(read.side, side);
            assert_eq!(read.stamp, index as u64 * 1000);
        }
        assert_eq!(back[0].at, 40);
        assert_eq!(back[1].at, 40 + 13);
        // An unknown tag and a cut record do not read back.
        assert!(decode_sides(0, &[99; 9]).is_none());
        assert!(decode_sides(0, &stream[..12]).is_none());
    }

    #[test]
    fn a_large_base_triggers_before_the_gate() {
        let capacity = 16 << 20;
        // A small base waits for the floor.
        assert!(!due(BASE_FLOOR - 1, 10, capacity));
        assert!(due(BASE_FLOOR, 10, capacity));
        // The journal as long as the base.
        assert!(!due(3 << 20, 4 << 20, capacity));
        assert!(due(4 << 20, 4 << 20, capacity));
        // A base larger than the stream: the trigger comes while the gate
        // is still open.
        let trigger = capacity - GATE_ROOM - MARGIN;
        assert!(due(trigger, 100 << 20, capacity));
        assert!(!due(trigger - 1, 100 << 20, capacity));
        assert!(capacity - trigger >= GATE_ROOM);
        // Nothing to compact is never due.
        assert!(!due(0, 0, GATE_ROOM));
    }

    #[test]
    fn the_memory_store_keeps_each_stream_until_it_is_released() {
        let store = MemoryStore::new(3 * BLOCK, 64, false);
        let chunk: Vec<u8> = (0..BLOCK + 10).map(|n| n as u8).collect();
        assert!(store.append(JournalStream::Bytes, &[&chunk, b"xyz"]));
        assert!(store.append(JournalStream::Sides, &[b"0123456789"]));
        assert_eq!(store.used(JournalStream::Bytes), BLOCK + 13);
        // Too much for the rest: nothing is written.
        assert!(!store.append(JournalStream::Bytes, &[&chunk, &chunk]));
        assert!(!store.append(JournalStream::Sides, &[&[0; 60]]));
        let read = store.read();
        assert_eq!(read.bytes_at, 0);
        assert_eq!(&read.bytes[..chunk.len()], &chunk[..]);
        assert_eq!(&read.bytes[chunk.len()..], b"xyz");
        assert_eq!(read.sides, b"0123456789");
        store.release(JournalCut {
            pty: BLOCK as u64 + 5,
            side: 4,
        });
        let read = store.read();
        assert_eq!(read.bytes_at, BLOCK as u64 + 5);
        assert_eq!(read.bytes.len(), 8);
        assert_eq!(&read.bytes[5..], b"xyz");
        assert_eq!(read.sides_at, 4);
        assert_eq!(read.sides, b"456789");
        assert_eq!(store.used(JournalStream::Bytes), 8);
        assert_eq!(store.used(JournalStream::Sides), 6);
        // Released room is room again.
        assert!(store.append(JournalStream::Bytes, &[&chunk, &chunk]));
        store.set_broken();
        assert!(store.is_broken());
    }
}
