//! A pane's journal in shared memory, the compaction that keeps it short,
//! and `bateri compact`, which rebuilds the screen from it after a crash.
//!
//! `bt-core` records what a pane's terminal goes through since its last base
//! ([`bt_core::Journal`]) and leaves the storage to the platform
//! ([`JournalStore`]). Here it is a **region** of shared memory per pane
//! ([`Region`]): bateri writes it, and the bound holder keeps a descriptor of
//! it, so when bateri dies without a word the holder still reads every byte
//! it recorded. Nothing reaches the disk: macOS' `shm_open` is unlinked the
//! moment it is created, Linux' `memfd_create` never has a name.
//!
//! **The region's layout** is an internal detail of one build — the holder
//! and the bateri that spawned it are the same binary, and `bateri compact`
//! refuses another build's input:
//!
//! ```text
//! "BTJR" | REGION_VERSION u32 | JOURNAL_FORMAT u32 | 0 u32
//! length u64 | bytes capacity u64 | sides capacity u64
//! broken u64 | bytes end u64 | bytes start u64 | sides end u64 | sides start u64
//! … up to HEADER, then the byte ring, then the side ring
//! ```
//!
//! The five words after the capacities are atomics shared by the two
//! processes. Each stream is a ring addressed by absolute position: `end` is
//! written by the stream's one writer (the reader thread, or whoever holds the
//! terminal lock), `start` only moves forward, when the holder confirmed a
//! base that holds what is before it.
//!
//! **The compaction** runs on a short-lived thread per pane, at most one at a
//! time ([`SharedStore`]): `bt-core`'s copy builds a new base, the base goes
//! to the holder through the pane's sink, and the region is released up to
//! the base's cut only once the holder confirmed it (`Journal::release`) —
//! release earlier, and a crash between the two would pair the holder's
//! older base with a journal that no longer reaches back to it. While no
//! holder has the pane (before its registration, or between two holders)
//! nobody keeps a base the release could outrun, and the compaction frees
//! the journal itself. A holder that never confirms leaves the compaction
//! waiting, the read gate closes, and the journal breaks at the stall: the
//! pane falls back to its program's redraw, nothing waits forever.
//!
//! **`bateri compact`** ([`compact_main`]) is the holder's child for one pane
//! after a crash: the build's identity, the pane's state blob, its last base
//! and the region's two streams in, the rebuilt VT, tail and history out
//! ([`bt_core::journal_rebuild`]). A child of its own, so a fault in the
//! replay costs one pane's screen, not the holder with every program.

use std::io::{self, Read, Write};
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, FromRawFd, OwnedFd};
use std::os::unix::net::UnixStream;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use bt_core::{
    JOURNAL_FORMAT, JOURNAL_HEADROOM, Journal, JournalCut, JournalRecords, JournalStore,
    JournalStream,
};

use crate::handover;

/// The region layout's version, beside [`JOURNAL_FORMAT`].
pub const REGION_VERSION: u32 = 1;

const MAGIC: [u8; 4] = *b"BTJR";

/// Where the rings start: the header's fields, rounded up to a cache line
/// pair so the atomics never share a line with ring bytes.
const HEADER: usize = 128;

/// The header's fields, by offset.
const LENGTH: usize = 16;
const BYTES_CAP: usize = 24;
const SIDES_CAP: usize = 32;
const BROKEN: usize = 40;
const BYTES_END: usize = 48;
const BYTES_START: usize = 56;
const SIDES_END: usize = 64;
const SIDES_START: usize = 72;

/// The side ring: the main thread's reserved room, as the in-process body's.
/// A side record is at most 16 bytes; this holds thousands, and half of it
/// asks for a compaction.
pub const SIDES_CAPACITY: usize = 64 << 10;

/// The most a region may be — what a holder agrees to map from a descriptor.
const REGION_LIMIT: u64 = 1 << 30;

/// The journal a region lets grow between two compactions — a **design
/// constant** derived from the pane's scrollback. Each compaction replays the
/// base and the journal, and a base holds the scrollback: with the room at
/// least the base's size the work per byte of output stays constant (the
/// trigger is "the journal is as long as the base", `bt_core::journal_due`).
/// 256 bytes a line covers a wide line with its colours; the floor keeps a
/// short scrollback from compacting every few kilobytes, the ceiling keeps
/// 100 000 lines from reserving a quarter gigabyte per pane — past it a
/// compaction simply comes sooner. The pages are touched only as output
/// arrives: an idle pane costs a page.
fn journal_room(scrollback: usize) -> usize {
    scrollback.saturating_mul(256).clamp(4 << 20, 64 << 20)
}

/// The byte ring's size for a pane with `scrollback` lines.
pub fn region_bytes(scrollback: usize) -> usize {
    journal_room(scrollback) + JOURNAL_HEADROOM
}

/// A pane's region: the mapping and the descriptor that names it.
///
/// Every access goes through the header's atomics or a raw copy into a
/// range nobody else writes: a stream's writer copies past `end`, which a
/// reader never reads, into bytes before `start`, which nobody needs any
/// more. No reference into the mapping is ever formed.
pub struct Region {
    fd: OwnedFd,
    map: NonNull<u8>,
    len: usize,
    bytes_cap: usize,
    sides_cap: usize,
}

// SAFETY: the mapping is shared memory accessed only through atomics and
// raw copies into disjoint ranges (the type's doc); the pointer stays valid
// until `Drop` unmaps it.
unsafe impl Send for Region {}
// SAFETY: as above — `&Region` hands out no reference into the mapping.
unsafe impl Sync for Region {}

impl std::fmt::Debug for Region {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Region")
            .field("bytes", &self.bytes_cap)
            .field("sides", &self.sides_cap)
            .finish_non_exhaustive()
    }
}

impl Region {
    /// A fresh region with these rings, close-on-exec, never on disk.
    pub fn create(bytes_cap: usize, sides_cap: usize) -> io::Result<Region> {
        let len = HEADER
            .checked_add(bytes_cap)
            .and_then(|len| len.checked_add(sides_cap))
            .filter(|&len| bytes_cap > 0 && sides_cap > 0 && len as u64 <= REGION_LIMIT)
            .ok_or(io::ErrorKind::InvalidInput)?;
        let fd = anonymous_memory()?;
        let size = libc::off_t::try_from(len).map_err(|_| io::ErrorKind::InvalidInput)?;
        // SAFETY: `fd` is open; `ftruncate` has no memory preconditions.
        if unsafe { libc::ftruncate(fd.as_raw_fd(), size) } != 0 {
            return Err(io::Error::last_os_error());
        }
        let map = map(fd.as_fd(), len)?;
        let region = Region {
            fd,
            map,
            len,
            bytes_cap,
            sides_cap,
        };
        // The memory is zero: the counters start at zero, unbroken.
        // SAFETY: the header's first 16 bytes are inside the mapping and
        // nobody else sees the region yet.
        unsafe {
            let header = region.map.as_ptr();
            header.copy_from_nonoverlapping(MAGIC.as_ptr(), 4);
            header
                .add(4)
                .copy_from_nonoverlapping(REGION_VERSION.to_le_bytes().as_ptr(), 4);
            header
                .add(8)
                .copy_from_nonoverlapping(JOURNAL_FORMAT.to_le_bytes().as_ptr(), 4);
        }
        region.word(LENGTH).store(len as u64, Ordering::Relaxed);
        region
            .word(BYTES_CAP)
            .store(bytes_cap as u64, Ordering::Relaxed);
        region
            .word(SIDES_CAP)
            .store(sides_cap as u64, Ordering::Release);
        Ok(region)
    }

    /// A region another process created, from its descriptor — the
    /// holder's. Refused unless its header is this build's layout and its
    /// sizes add up inside the object.
    pub fn open(fd: OwnedFd) -> io::Result<Region> {
        let invalid = || io::Error::from(io::ErrorKind::InvalidData);
        // The header alone first: the sizes come from it.
        let head = map(fd.as_fd(), HEADER)?;
        let read = || -> Option<(u64, u64, u64)> {
            let mut fixed = [0u8; 12];
            // SAFETY: the first 12 bytes of the header's mapping, and its
            // 8-aligned words, read only atomically (the writer's way).
            let (len, bytes_cap, sides_cap) = unsafe {
                head.as_ptr().copy_to_nonoverlapping(fixed.as_mut_ptr(), 12);
                let word = |offset: usize| {
                    AtomicU64::from_ptr(head.as_ptr().add(offset).cast()).load(Ordering::Acquire)
                };
                (word(LENGTH), word(BYTES_CAP), word(SIDES_CAP))
            };
            let sum = (HEADER as u64)
                .checked_add(bytes_cap)
                .and_then(|sum| sum.checked_add(sides_cap));
            (fixed[..4] == MAGIC
                && fixed[4..8] == REGION_VERSION.to_le_bytes()
                && fixed[8..12] == JOURNAL_FORMAT.to_le_bytes()
                && bytes_cap > 0
                && sides_cap > 0
                && sum == Some(len)
                && len <= REGION_LIMIT)
                .then_some((len, bytes_cap, sides_cap))
        };
        let sizes = read();
        // SAFETY: the header's own mapping, made above and no longer read.
        unsafe { libc::munmap(head.as_ptr().cast(), HEADER) };
        let (len, bytes_cap, sides_cap) = sizes.ok_or_else(invalid)?;
        // A mapping past the object's end faults on touch: the object must
        // hold the whole region (macOS rounds its size up to a page).
        // SAFETY: an all-zero `stat` is valid; `fstat` fills it or fails.
        let mut stat: libc::stat = unsafe { std::mem::zeroed() };
        // SAFETY: the descriptor is open; `stat` belongs to this frame.
        if unsafe { libc::fstat(fd.as_raw_fd(), &raw mut stat) } != 0 {
            return Err(io::Error::last_os_error());
        }
        if u64::try_from(stat.st_size).unwrap_or(0) < len {
            return Err(invalid());
        }
        let len = usize::try_from(len).map_err(|_| invalid())?;
        let map = map(fd.as_fd(), len)?;
        Ok(Region {
            fd,
            map,
            len,
            bytes_cap: usize::try_from(bytes_cap).map_err(|_| invalid())?,
            sides_cap: usize::try_from(sides_cap).map_err(|_| invalid())?,
        })
    }

    /// The descriptor, for a copy that goes to the holder.
    pub fn fd(&self) -> BorrowedFd<'_> {
        self.fd.as_fd()
    }

    /// One of the header's atomics.
    fn word(&self, offset: usize) -> &AtomicU64 {
        debug_assert!(offset + 8 <= HEADER && offset % 8 == 0);
        // SAFETY: the offset is inside the header (all callers pass one of
        // the field constants) and 8-aligned in a page-aligned mapping; the
        // other process touches the word only atomically too.
        unsafe { AtomicU64::from_ptr(self.map.as_ptr().add(offset).cast()) }
    }

    /// A stream's ring: its offset in the mapping, its size and its two
    /// counters.
    fn ring(&self, stream: JournalStream) -> (usize, usize, &AtomicU64, &AtomicU64) {
        match stream {
            JournalStream::Bytes => (
                HEADER,
                self.bytes_cap,
                self.word(BYTES_END),
                self.word(BYTES_START),
            ),
            JournalStream::Sides => (
                HEADER + self.bytes_cap,
                self.sides_cap,
                self.word(SIDES_END),
                self.word(SIDES_START),
            ),
        }
    }

    /// Copies `bytes` into a ring at absolute position `at`, wrapping.
    ///
    /// # Safety
    ///
    /// `[at, at + bytes.len())` is this caller's to write: past the
    /// stream's end, no more than the ring's free room.
    unsafe fn copy_in(&self, offset: usize, cap: usize, at: u64, bytes: &[u8]) {
        let at = (at % cap as u64) as usize;
        let first = bytes.len().min(cap - at);
        // SAFETY: both ranges are inside the ring (`at < cap`, `first ≤ cap −
        // at`, the rest ≤ `cap`), the caller owns them.
        unsafe {
            let ring = self.map.as_ptr().add(offset);
            ring.add(at).copy_from_nonoverlapping(bytes.as_ptr(), first);
            ring.copy_from_nonoverlapping(bytes.as_ptr().add(first), bytes.len() - first);
        }
    }

    /// Copies a ring's `[from, to)` out.
    ///
    /// # Safety
    ///
    /// `to − from ≤ cap`, and nobody writes those positions meanwhile — or
    /// the caller checks afterwards and drops what was overwritten.
    unsafe fn copy_out(&self, offset: usize, cap: usize, from: u64, to: u64) -> Vec<u8> {
        let len = (to - from) as usize;
        let at = (from % cap as u64) as usize;
        let first = len.min(cap - at);
        let mut out = vec![0u8; len];
        // SAFETY: as in `copy_in`, the ranges are inside the ring.
        unsafe {
            let ring = self.map.as_ptr().add(offset);
            ring.add(at).copy_to_nonoverlapping(out.as_mut_ptr(), first);
            ring.copy_to_nonoverlapping(out.as_mut_ptr().add(first), len - first);
        }
        out
    }

    fn append(&self, stream: JournalStream, parts: &[&[u8]]) -> bool {
        let (offset, cap, end, start) = self.ring(stream);
        let len: usize = parts.iter().map(|part| part.len()).sum();
        let mut at = end.load(Ordering::Relaxed);
        let used = at.saturating_sub(start.load(Ordering::Acquire));
        if used.saturating_add(len as u64) > cap as u64 {
            return false;
        }
        for part in parts {
            // SAFETY: the stream's one writer, past its end, within the free
            // room checked above.
            unsafe { self.copy_in(offset, cap, at, part) };
            at += part.len() as u64;
        }
        end.store(at, Ordering::Release);
        true
    }

    fn used(&self, stream: JournalStream) -> usize {
        let (_, _, end, start) = self.ring(stream);
        let end = end.load(Ordering::Acquire);
        end.saturating_sub(start.load(Ordering::Acquire)) as usize
    }

    fn capacity(&self, stream: JournalStream) -> usize {
        self.ring(stream).1
    }

    /// A stream from its oldest kept byte, with that byte's position;
    /// bytes overwritten while they were copied are dropped from the front
    /// (the writer never reaches back further than a ring's length).
    fn stream(&self, stream: JournalStream) -> (u64, Vec<u8>) {
        let (offset, cap, end, start) = self.ring(stream);
        let from = start.load(Ordering::Acquire);
        let to = end.load(Ordering::Acquire).max(from);
        let from = from.max(to.saturating_sub(cap as u64));
        // SAFETY: at most a ring's length; overwritten bytes are dropped
        // below.
        let mut bytes = unsafe { self.copy_out(offset, cap, from, to) };
        let valid = end.load(Ordering::Acquire).saturating_sub(cap as u64);
        if valid > from {
            let gone = ((valid - from) as usize).min(bytes.len());
            bytes.drain(..gone);
            return (from + gone as u64, bytes);
        }
        (from, bytes)
    }

    fn read(&self) -> JournalRecords {
        let (bytes_at, bytes) = self.stream(JournalStream::Bytes);
        let (sides_at, sides) = self.stream(JournalStream::Sides);
        JournalRecords {
            bytes_at,
            bytes,
            sides_at,
            sides,
        }
    }

    /// Both streams as the region keeps them — the holder's read after its
    /// bateri died. `None` once the journal broke.
    pub fn snapshot(&self) -> Option<JournalRecords> {
        (!self.is_broken()).then(|| self.read())
    }

    fn release(&self, to: JournalCut) {
        for (stream, to) in [
            (JournalStream::Bytes, to.pty),
            (JournalStream::Sides, to.side),
        ] {
            let (_, _, end, start) = self.ring(stream);
            start.fetch_max(to.min(end.load(Ordering::Acquire)), Ordering::AcqRel);
        }
    }

    fn set_broken(&self) {
        self.word(BROKEN).store(1, Ordering::Release);
    }

    pub fn is_broken(&self) -> bool {
        self.word(BROKEN).load(Ordering::Acquire) != 0
    }
}

impl Drop for Region {
    fn drop(&mut self) {
        // SAFETY: the mapping of `len` bytes made at birth; nothing refers
        // into it past this point (the type's doc).
        unsafe { libc::munmap(self.map.as_ptr().cast(), self.len) };
    }
}

/// Shared memory with no name: Linux' `memfd_create`, macOS' `shm_open`
/// unlinked at once. Close-on-exec either way, set again explicitly: a copy
/// in a child (the shell) would keep the region past everyone.
fn anonymous_memory() -> io::Result<OwnedFd> {
    #[cfg(target_os = "linux")]
    let fd = {
        // SAFETY: a static name; the flags ask for close-on-exec.
        let fd = unsafe { libc::memfd_create(c"bateri-journal".as_ptr(), libc::MFD_CLOEXEC) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: a descriptor just returned to us, owned from here.
        unsafe { OwnedFd::from_raw_fd(fd) }
    };
    #[cfg(not(target_os = "linux"))]
    let fd = {
        use std::sync::atomic::AtomicU32;
        static SERIAL: AtomicU32 = AtomicU32::new(0);
        let mut tries = 0;
        loop {
            // macOS keeps a name to 31 bytes.
            let name = format!(
                "/bt.{:x}.{:x}",
                std::process::id(),
                SERIAL.fetch_add(1, Ordering::Relaxed)
            );
            let name = std::ffi::CString::new(name).map_err(|_| io::ErrorKind::InvalidInput)?;
            // SAFETY: `name` is a C string for the call; the mode is passed
            // promoted, as the variadic declaration requires.
            let fd = unsafe {
                libc::shm_open(
                    name.as_ptr(),
                    libc::O_RDWR | libc::O_CREAT | libc::O_EXCL,
                    0o600 as libc::c_uint,
                )
            };
            if fd >= 0 {
                // SAFETY: the name just created; the object lives while a
                // descriptor or a mapping does.
                unsafe { libc::shm_unlink(name.as_ptr()) };
                // SAFETY: a descriptor just returned to us, owned from here.
                break unsafe { OwnedFd::from_raw_fd(fd) };
            }
            let error = io::Error::last_os_error();
            tries += 1;
            if error.kind() != io::ErrorKind::AlreadyExists || tries > 8 {
                return Err(error);
            }
        }
    };
    // SAFETY: `fd` is open; setting a descriptor flag has no memory
    // preconditions.
    unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC) };
    Ok(fd)
}

/// Maps `len` bytes of `fd`, shared, readable and writable.
fn map(fd: BorrowedFd<'_>, len: usize) -> io::Result<NonNull<u8>> {
    // SAFETY: a fresh mapping chosen by the kernel; the descriptor is open.
    let map = unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            len,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_SHARED,
            fd.as_raw_fd(),
            0,
        )
    };
    if map == libc::MAP_FAILED {
        return Err(io::Error::last_os_error());
    }
    NonNull::new(map.cast()).ok_or_else(|| io::ErrorKind::InvalidData.into())
}

// ─── the body and its compaction ─────────────────────────────────────────

/// Where a pane's bases go: its bound holder. `send` only queues; the
/// holder's confirmation comes back as `Journal::release` with the same cut.
pub trait BaseSink: Send + Sync {
    fn send(&self, cut: JournalCut, base: Vec<u8>);
}

/// The compaction's state, shared by the body and the pane.
#[derive(Default)]
struct Link {
    /// The holder the bases go to; `None` while no holder has the pane —
    /// then nobody keeps a base that a release could outrun, and the
    /// compaction frees the journal itself, as the in-process body does.
    sink: Option<Box<dyn BaseSink>>,
    /// A compaction runs or waits for its base's confirmation: no other
    /// starts — each would replay the whole base while the journal cannot
    /// shrink before the holder answers.
    busy: bool,
    /// The cut of the base sent and not yet confirmed.
    in_flight: Option<JournalCut>,
    /// Counts the compactions started and the registrations: a compaction
    /// whose number is no longer the last touches nothing when it ends — a
    /// registration (a new holder) took the state over meanwhile.
    ticket: u64,
}

/// The largest base a pane sends its holder — a **design constant** under
/// the frame's limit on one field (256 MiB): after a crash the pane's blob
/// carries a VT about the base's size and a history about as large again.
/// A pane past it (a scrollback of 100 000 wide, colourful lines) breaks its
/// journal rather than make the holder refuse the stream and every pane
/// with it.
pub const BASE_LIMIT: usize = 96 << 20;

fn lock(link: &Mutex<Link>) -> MutexGuard<'_, Link> {
    link.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The region as a `bt-core` journal body.
struct SharedStore {
    region: Arc<Region>,
    link: Arc<Mutex<Link>>,
}

impl JournalStore for SharedStore {
    fn append(&self, stream: JournalStream, parts: &[&[u8]]) -> bool {
        self.region.append(stream, parts)
    }

    fn used(&self, stream: JournalStream) -> usize {
        self.region.used(stream)
    }

    fn capacity(&self, stream: JournalStream) -> usize {
        self.region.capacity(stream)
    }

    fn read(&self) -> JournalRecords {
        self.region.read()
    }

    /// The holder confirmed the base at `to`: what is before it is free,
    /// and the next compaction may start once it was the one in flight.
    fn release(&self, to: JournalCut) {
        self.region.release(to);
        let mut link = lock(&self.link);
        if link.in_flight == Some(to) {
            link.in_flight = None;
            link.busy = false;
        }
    }

    fn set_broken(&self) {
        self.region.set_broken();
    }

    fn is_broken(&self) -> bool {
        self.region.is_broken()
    }

    fn wake(&self, journal: &Arc<Journal>) -> bool {
        let ticket = {
            let mut link = lock(&self.link);
            if link.busy {
                return false;
            }
            link.busy = true;
            link.ticket += 1;
            link.ticket
        };
        let journal = Arc::clone(journal);
        let link = Arc::clone(&self.link);
        let started = std::thread::Builder::new()
            .name("PTY journal".to_owned())
            .spawn(move || compaction(&journal, &link, ticket))
            .is_ok();
        if !started {
            lock(&self.link).busy = false;
        }
        started
    }
}

/// One compaction's thread ([`SharedStore::wake`]): the copy, then the base
/// to the holder — encoded outside the link's lock, sent only while this
/// compaction is still the last one started (`ticket`) and its base the
/// current one. Without a holder the journal is freed at once: nobody keeps
/// an older base. A base past [`BASE_LIMIT`] breaks the journal.
fn compaction(journal: &Arc<Journal>, link: &Mutex<Link>, ticket: u64) {
    let finish = || {
        let mut link = lock(link);
        if link.ticket == ticket {
            link.busy = false;
        }
    };
    let Some(cut) = journal.compact() else {
        finish();
        return;
    };
    let held = {
        let link = lock(link);
        if link.ticket != ticket {
            return;
        }
        link.sink.is_some()
    };
    if !held {
        journal.release(cut);
        finish();
        return;
    }
    let base = journal
        .current_base()
        .filter(|(at, _)| *at == cut)
        .map(|(_, base)| base);
    let mut guard = lock(link);
    if guard.ticket != ticket {
        return;
    }
    match (base, guard.sink.as_ref()) {
        (Some(base), Some(sink)) if base.len() <= BASE_LIMIT => {
            sink.send(cut, base);
            guard.in_flight = Some(cut);
        }
        (Some(_), Some(_)) => {
            guard.sink = None;
            guard.busy = false;
            drop(guard);
            eprintln!("bateri: a pane's screen is too large to keep through a crash");
            journal.break_journal();
        }
        _ => guard.busy = false,
    }
}

/// A pane's journal: `bt-core`'s journal over a region of its own, and the
/// link its compaction sends bases through.
pub struct PaneJournal {
    journal: Arc<Journal>,
    region: Arc<Region>,
    link: Arc<Mutex<Link>>,
}

impl std::fmt::Debug for PaneJournal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PaneJournal")
            .field("region", &self.region)
            .finish_non_exhaustive()
    }
}

/// What a registration carries for the holder: a copy of the region's
/// descriptor and the current base with its cut.
#[derive(Debug)]
pub struct Registration {
    pub region: OwnedFd,
    pub cut: JournalCut,
    pub base: Vec<u8>,
}

impl PaneJournal {
    /// A journal in a fresh region sized for `scrollback` lines.
    pub fn open(scrollback: usize) -> io::Result<PaneJournal> {
        let region = Arc::new(Region::create(region_bytes(scrollback), SIDES_CAPACITY)?);
        let link = Arc::new(Mutex::new(Link::default()));
        let store = SharedStore {
            region: Arc::clone(&region),
            link: Arc::clone(&link),
        };
        Ok(PaneJournal {
            journal: Journal::new(Box::new(store), bt_core::JOURNAL_STALL),
            region,
            link,
        })
    }

    /// `bt-core`'s journal — the session's `SessionOptions::journal`.
    pub fn journal(&self) -> &Arc<Journal> {
        &self.journal
    }

    /// Registers the pane with a holder: `add` gets what the registration
    /// carries (`None` once the journal broke, or before the session seeded
    /// it: the pane registers without a screen) and queues it, returning the
    /// holder's sink; bases go there from then on. One step under the link's
    /// lock, so a compaction finishing meanwhile sends its base **after** the
    /// registration. A base in flight to an earlier holder is forgotten —
    /// that holder is gone, and the registration carries the current base.
    pub fn register(&self, add: impl FnOnce(Option<Registration>) -> Option<Box<dyn BaseSink>>) {
        let mut link = lock(&self.link);
        let registration = (!self.journal.is_broken())
            .then(|| self.journal.current_base())
            .flatten()
            .filter(|(_, base)| base.len() <= BASE_LIMIT)
            .and_then(|(cut, base)| {
                let region = self.region.fd().try_clone_to_owned().ok()?;
                Some(Registration { region, cut, base })
            });
        let journaled = registration.is_some();
        let sink = add(registration);
        link.sink = sink.filter(|_| journaled);
        link.in_flight = None;
        link.busy = false;
        link.ticket += 1;
        drop(link);
        // Registered without a screen (no base, or no copy of the region):
        // nobody will confirm a base, so the journal would only fill.
        if !journaled {
            self.journal.break_journal();
        }
    }

    /// The pane is no holder's for now (its registration failed, a holder
    /// is being replaced): the compaction frees the journal itself until the
    /// next registration — nobody keeps a base it could outrun. A base in
    /// flight to the old holder is forgotten.
    pub fn disconnect(&self) {
        let mut link = lock(&self.link);
        link.sink = None;
        link.in_flight = None;
        link.busy = false;
        link.ticket += 1;
    }

    /// Breaks the journal: nothing is recorded any more, the read gate stays
    /// open, and a crash brings the pane's screen back from its program's
    /// redraw.
    pub fn break_journal(&self) {
        self.disconnect();
        self.journal.break_journal();
    }

    pub fn is_broken(&self) -> bool {
        self.journal.is_broken()
    }

    /// Where the byte stream starts now: past zero once a holder confirmed
    /// a compaction's base.
    #[cfg(test)]
    pub(crate) fn released(&self) -> u64 {
        self.region.read().bytes_at
    }
}

// ─── `bateri compact` ────────────────────────────────────────────────────

/// The exit codes of `bateri compact`.
pub const COMPACT_DONE: i32 = 0;
pub const COMPACT_FAILED: i32 = 1;
pub const COMPACT_USAGE: i32 = 2;
/// The input is another build's: its base and region are not this one's to
/// read.
pub const COMPACT_OTHER_BUILD: i32 = 3;

pub const COMPACT_USAGE_TEXT: &str = "usage: bateri compact --fd FD";

/// The request's and the reply's first bytes.
const REQUEST: [u8; 4] = *b"BTCQ";
const REPLY: [u8; 4] = *b"BTCR";

/// The most a request or a reply may announce: a region with its base and
/// history, with room — anything past it is broken.
const WIRE_LIMIT: u64 = 2 << 30;

/// What the holder hands `bateri compact` for one pane.
#[derive(Debug)]
pub struct CompactRequest<'a> {
    pub blob: &'a [u8],
    pub base: &'a [u8],
    pub records: &'a JournalRecords,
}

/// The request on the wire:
///
/// ```text
/// "BTCQ" | build id: len u32, bytes | blob: len u64, bytes | base: len u64, bytes
/// bytes at u64 | bytes: len u64, bytes | sides at u64 | sides: len u64, bytes
/// ```
pub fn write_request(out: &mut impl Write, request: &CompactRequest<'_>) -> io::Result<()> {
    let id = handover::build_id();
    out.write_all(&REQUEST)?;
    let len = u32::try_from(id.len()).map_err(|_| io::ErrorKind::InvalidInput)?;
    out.write_all(&len.to_le_bytes())?;
    out.write_all(id.as_bytes())?;
    field(out, request.blob)?;
    field(out, request.base)?;
    out.write_all(&request.records.bytes_at.to_le_bytes())?;
    field(out, &request.records.bytes)?;
    out.write_all(&request.records.sides_at.to_le_bytes())?;
    field(out, &request.records.sides)
}

/// The reply on the wire:
///
/// ```text
/// "BTCR" | cols u16 | rows u16 | vt: len u64, bytes | tail: len u64, bytes
/// history: len u64, bytes
/// ```
fn write_reply(out: &mut impl Write, rebuilt: &bt_core::Rebuilt) -> io::Result<()> {
    out.write_all(&REPLY)?;
    out.write_all(&rebuilt.cols.to_le_bytes())?;
    out.write_all(&rebuilt.rows.to_le_bytes())?;
    field(out, &rebuilt.vt)?;
    field(out, &rebuilt.tail)?;
    field(out, &rebuilt.history)
}

/// A reply ([`write_reply`]); `None` for anything else.
pub fn read_reply(input: &mut impl Read) -> Option<bt_core::Rebuilt> {
    let mut magic = [0u8; 4];
    input.read_exact(&mut magic).ok()?;
    if magic != REPLY {
        return None;
    }
    let cols = u16::from_le_bytes(bytes(input).ok()?);
    let rows = u16::from_le_bytes(bytes(input).ok()?);
    let vt = read_field(input).ok()?;
    let tail = read_field(input).ok()?;
    let history = read_field(input).ok()?;
    // Exactly one reply.
    let mut more = [0u8];
    matches!(input.read(&mut more), Ok(0)).then_some(bt_core::Rebuilt {
        cols,
        rows,
        vt,
        tail,
        history,
    })
}

fn field(out: &mut impl Write, bytes: &[u8]) -> io::Result<()> {
    out.write_all(&(bytes.len() as u64).to_le_bytes())?;
    out.write_all(bytes)
}

fn bytes<const N: usize>(input: &mut impl Read) -> io::Result<[u8; N]> {
    let mut bytes = [0u8; N];
    input.read_exact(&mut bytes)?;
    Ok(bytes)
}

/// A field, read as it arrives — never allocated from the announced
/// length alone.
fn read_field(input: &mut impl Read) -> io::Result<Vec<u8>> {
    let len = u64::from_le_bytes(bytes(input)?);
    if len > WIRE_LIMIT {
        return Err(io::ErrorKind::InvalidData.into());
    }
    let mut out = Vec::new();
    input.take(len).read_to_end(&mut out)?;
    if out.len() as u64 != len {
        return Err(io::ErrorKind::UnexpectedEof.into());
    }
    Ok(out)
}

/// `bateri compact --fd FD`: reads one request from `FD` (a socket the
/// holder keeps the other end of), rebuilds the pane and writes the reply
/// there. The exit code says why there is none: another build's input
/// ([`COMPACT_OTHER_BUILD`]), a request that does not read or a journal that
/// does not continue its base ([`COMPACT_FAILED`]), arguments
/// ([`COMPACT_USAGE`]).
pub fn compact_main(args: &[String]) -> i32 {
    let fd = match args {
        [flag, fd] if flag == "--fd" => fd.parse::<libc::c_int>().ok().filter(|&fd| fd > 2),
        _ => None,
    };
    let Some(fd) = fd else {
        eprintln!("{COMPACT_USAGE_TEXT}");
        return COMPACT_USAGE;
    };
    // Only a socket is taken as the holder's end: a number that is
    // something else (or nothing) is not ours to own.
    // SAFETY: an all-zero `stat` is valid; `fstat` fills it or fails.
    let mut meta: libc::stat = unsafe { std::mem::zeroed() };
    // SAFETY: `meta` belongs to this frame.
    if unsafe { libc::fstat(fd, &raw mut meta) } != 0
        || meta.st_mode & libc::S_IFMT != libc::S_IFSOCK
    {
        eprintln!("bateri compact: fd {fd} is not a socket");
        return COMPACT_FAILED;
    }
    // SAFETY: the socket the holder put at `fd` for this process alone;
    // owned from here.
    let mut stream = unsafe { UnixStream::from_raw_fd(fd) };
    match compact(&mut stream) {
        Ok(Some(rebuilt)) => match write_reply(&mut stream, &rebuilt) {
            Ok(()) => COMPACT_DONE,
            Err(_) => COMPACT_FAILED,
        },
        Ok(None) => COMPACT_OTHER_BUILD,
        Err(_) => COMPACT_FAILED,
    }
}

/// One request's rebuild; `Ok(None)` for another build's.
fn compact(input: &mut impl Read) -> io::Result<Option<bt_core::Rebuilt>> {
    let invalid = || io::Error::from(io::ErrorKind::InvalidData);
    let magic: [u8; 4] = bytes(input)?;
    if magic != REQUEST {
        return Err(invalid());
    }
    let id_len = u32::from_le_bytes(bytes(input)?);
    if id_len > 1024 {
        return Err(invalid());
    }
    let mut id = Vec::new();
    input.take(u64::from(id_len)).read_to_end(&mut id)?;
    if id != handover::build_id().as_bytes() {
        return Ok(None);
    }
    let blob = read_field(input)?;
    let base = read_field(input)?;
    let bytes_at = u64::from_le_bytes(bytes(input)?);
    let journal = read_field(input)?;
    let sides_at = u64::from_le_bytes(bytes(input)?);
    let sides = read_field(input)?;
    let records = JournalRecords {
        bytes_at,
        bytes: journal,
        sides_at,
        sides,
    };
    bt_core::journal_rebuild(&blob, &base, &records)
        .map(Some)
        .ok_or_else(invalid)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};
    use std::sync::mpsc;
    use std::time::Duration;

    use bt_core::{
        CaretShape, CursorBlink, Osc52, Session, SessionOptions, TerminalOptions, Theme,
    };

    #[test]
    fn a_ring_wraps_keeps_what_is_not_released_and_refuses_what_does_not_fit() {
        let region = Region::create(16, 8).expect("region");
        assert!(region.append(JournalStream::Bytes, &[b"0123456789", b"ab"]));
        assert!(!region.append(JournalStream::Bytes, &[b"too much"]));
        assert_eq!(region.used(JournalStream::Bytes), 12);
        region.release(JournalCut { pty: 10, side: 0 });
        assert_eq!(region.used(JournalStream::Bytes), 2);
        // Across the ring's end.
        assert!(region.append(JournalStream::Bytes, &[b"cdefghij", b"kl"]));
        assert!(region.append(JournalStream::Sides, &[b"side"]));
        let read = region.read();
        assert_eq!(
            (read.bytes_at, read.bytes.as_slice()),
            (10, &b"abcdefghijkl"[..])
        );
        assert_eq!((read.sides_at, read.sides.as_slice()), (0, &b"side"[..]));
        // A release never moves back, nor past the end.
        region.release(JournalCut { pty: 4, side: 99 });
        assert_eq!(region.read().bytes_at, 10);
        assert_eq!(region.read().sides_at, 4);
        region.set_broken();
        assert!(region.is_broken());
        assert!(region.snapshot().is_none());
    }

    /// The environment that turns this test binary into a region's writer
    /// ([`region_writer_process`]).
    const WRITER_ENV: &str = "BT_TEST_REGION_WRITER";

    /// Not a test of its own: the writer the next test spawns. It creates a
    /// region, records into it, hands the descriptor over fd 3 and dies by
    /// `SIGKILL` — no unmap, no goodbye.
    #[test]
    fn region_writer_process() {
        if std::env::var_os(WRITER_ENV).is_none() {
            return;
        }
        // SAFETY: the socket the test put at fd 3 for this process.
        let stream = unsafe { UnixStream::from_raw_fd(3) };
        let region = Region::create(64, 32).expect("region");
        assert!(region.append(JournalStream::Bytes, &[b"before the release "]));
        region.release(JournalCut { pty: 7, side: 0 });
        assert!(region.append(JournalStream::Bytes, &[b"and after"]));
        assert!(region.append(JournalStream::Sides, &[b"\x05side"]));
        handover::send_fd(&stream, region.fd().as_raw_fd()).expect("send");
        let mut word = [0u8];
        let _ = (&stream).read_exact(&mut word);
        // SAFETY: `kill` has no memory preconditions; this process ends.
        unsafe { libc::kill(libc::getpid(), libc::SIGKILL) };
    }

    #[test]
    fn a_region_outlives_its_writer() {
        let (ours, theirs) = UnixStream::pair().unwrap();
        let raw = theirs.as_raw_fd();
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "journal::tests::region_writer_process",
                "--nocapture",
            ])
            .args(["--test-threads", "1", "-q"])
            .env(WRITER_ENV, "1")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        // SAFETY: only async-signal-safe calls between fork and exec.
        unsafe {
            command.pre_exec(move || {
                if raw == 3 {
                    libc::fcntl(3, libc::F_SETFD, 0);
                } else if libc::dup2(raw, 3) < 0 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let mut writer = command.spawn().expect("the writer did not start");
        drop(theirs);
        ours.set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let fd = handover::recv_fd(&ours).expect("the region's descriptor");
        (&ours).write_all(b"x").unwrap();
        let status = writer.wait().unwrap();
        assert!(!status.success(), "the writer did not die");
        let region = Region::open(fd).expect("the region opens without its writer");
        let read = region.snapshot().expect("not broken");
        assert_eq!(read.bytes_at, 7);
        assert_eq!(read.bytes, b"the release and after");
        assert_eq!(read.sides, b"\x05side");
    }

    #[test]
    fn another_layout_or_a_short_object_is_refused() {
        let region = Region::create(64, 32).expect("region");
        // A file is not a region.
        let file = std::fs::File::open("/dev/null").unwrap();
        assert!(Region::open(OwnedFd::from(file)).is_err());
        // Damage the version: refused.
        let fd = region.fd().try_clone_to_owned().unwrap();
        // SAFETY: four bytes of the header, nobody else writes them.
        unsafe {
            region
                .map
                .as_ptr()
                .add(4)
                .copy_from_nonoverlapping(99u32.to_le_bytes().as_ptr(), 4);
        }
        assert!(Region::open(fd).is_err());
    }

    /// A sink that hands each base to the test.
    struct Channel(Mutex<mpsc::Sender<(JournalCut, Vec<u8>)>>);

    impl BaseSink for Channel {
        fn send(&self, cut: JournalCut, base: Vec<u8>) {
            let _ = lock_sender(&self.0).send((cut, base));
        }
    }

    fn lock_sender<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
        mutex.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn session(script: &str, journal: &PaneJournal) -> Session {
        Session::spawn(
            SessionOptions {
                command: Some((
                    "/bin/sh".to_owned(),
                    vec!["-c".to_owned(), script.to_owned()],
                )),
                working_directory: None,
                home: None,
                env: HashMap::new(),
                cols: 80,
                rows: 24,
                cell_px: (9, 18),
                terminal: TerminalOptions {
                    scrollback: 100,
                    osc52: Osc52::Off,
                    cursor: CaretShape::default(),
                    blink: CursorBlink::default(),
                },
                theme: Theme::BATERI,
                dock: false,
                cluster: true,
                initial_input: None,
                shell_marks: false,
                pane_uuid: None,
                hostname: None,
                replay: None,
                journal: Some(Arc::clone(journal.journal())),
            },
            Arc::new(crate::child::SilentWake),
        )
        .expect("session")
    }

    /// The compaction runs past the trigger, sends its base, and frees the
    /// region only on the holder's word — which opens the read gate again:
    /// output twice the region's size gets through.
    #[test]
    fn a_base_goes_out_and_the_region_frees_only_on_the_holders_word() {
        let journal = PaneJournal::open(100).expect("journal");
        let capacity = journal.region.capacity(JournalStream::Bytes);
        let lines = 2 * capacity / 64;
        // The output waits for the registration (a line of input).
        let script = format!(
            "stty -echo; read x; i=0; while [ $i -lt {lines} ]; do printf '%063d\\n' $i; \
             i=$((i + 1)); done; sleep 30"
        );
        let session = session(&script, &journal);
        let (bases, arrived) = mpsc::channel();
        journal.register(|registration| {
            let registration = registration.expect("a seeded journal registers its base");
            assert_eq!(registration.cut, JournalCut::default());
            Some(Box::new(Channel(Mutex::new(bases))))
        });
        session.write(b"go\n");
        let mut confirmed = 0;
        let mut fullest = 0;
        let mut last = JournalCut::default();
        // Each output line is 65 bytes on the PTY (`\r\n`).
        let total = lines as u64 * 65;
        let recorded = || {
            let read = journal.region.read();
            read.bytes_at + read.bytes.len() as u64
        };
        // Every base is confirmed late — the region stays full meanwhile —
        // until the output is through.
        while recorded() < total {
            let (cut, base) = arrived
                .recv_timeout(Duration::from_secs(10))
                .expect("no base came while the output waited");
            assert!(cut.pty > last.pty, "a base did not move on");
            assert!(!base.is_empty());
            assert!(
                journal.region.read().bytes_at <= last.pty,
                "the region was freed before the holder's word"
            );
            // Late enough for the region to fill up to the read gate.
            std::thread::sleep(Duration::from_millis(400));
            fullest = fullest.max(journal.region.used(JournalStream::Bytes));
            journal.journal().release(cut);
            assert!(journal.region.read().bytes_at >= cut.pty);
            last = cut;
            confirmed += 1;
        }
        assert!(confirmed >= 2, "only one base went out");
        assert!(
            fullest > capacity - JOURNAL_HEADROOM,
            "the region never filled up to the gate ({fullest} of {capacity})"
        );
        assert!(!journal.is_broken(), "the journal broke");
        session.shutdown();
    }

    /// No holder has the pane: nobody keeps a base the release could
    /// outrun, so the compaction frees the journal itself — output past the
    /// region's size gets through and the journal stays whole for a later
    /// registration.
    #[test]
    fn without_a_holder_the_compaction_frees_the_journal_itself() {
        let journal = PaneJournal::open(100).expect("journal");
        let capacity = journal.region.capacity(JournalStream::Bytes);
        let lines = 2 * capacity / 64;
        let script = format!(
            "i=0; while [ $i -lt {lines} ]; do printf '%063d\\n' $i; i=$((i + 1)); done; \
             printf 'ALL-DONE'; sleep 30"
        );
        let session = session(&script, &journal);
        let total = lines as u64 * 65;
        crate::child::wait_until("the output did not get through", || {
            let read = journal.region.read();
            read.bytes_at + read.bytes.len() as u64 >= total
        });
        assert!(journal.released() > 0, "nothing was freed");
        assert!(!journal.is_broken(), "the journal broke");
        session.shutdown();
    }

    #[test]
    fn a_broken_journal_registers_without_a_screen() {
        let journal = PaneJournal::open(100).expect("journal");
        let session = session("sleep 30", &journal);
        journal.break_journal();
        let mut asked = false;
        journal.register(|registration| {
            asked = true;
            assert!(registration.is_none());
            None
        });
        assert!(asked);
        session.shutdown();
    }

    #[test]
    fn compact_answers_its_own_build_and_refuses_another() {
        let journal = PaneJournal::open(100).expect("journal");
        let session = session("printf 'compacted text'; sleep 30", &journal);
        crate::child::wait_until("the output was not recorded", || {
            journal.region.used(JournalStream::Bytes) >= 14
        });
        let (_, base) = journal.journal().current_base().expect("seeded");
        let records = journal.region.read();
        let request = CompactRequest {
            blob: &session.state_blob(),
            base: &base,
            records: &records,
        };

        let run = |bytes: Vec<u8>| -> (i32, Vec<u8>) {
            let (ours, theirs) = UnixStream::pair().unwrap();
            let fd = std::os::fd::IntoRawFd::into_raw_fd(theirs);
            let child = std::thread::spawn(move || compact_main(&["--fd".into(), fd.to_string()]));
            // A child that refuses stops reading: the rest of the request may
            // meet a closed socket (Linux resets it), which is no reply.
            let _ = (&ours).write_all(&bytes);
            let _ = ours.shutdown(std::net::Shutdown::Write);
            let mut reply = Vec::new();
            let _ = (&ours).read_to_end(&mut reply);
            (child.join().unwrap(), reply)
        };

        let mut good = Vec::new();
        write_request(&mut good, &request).unwrap();
        let (code, reply) = run(good.clone());
        assert_eq!(code, COMPACT_DONE);
        let rebuilt = read_reply(&mut reply.as_slice()).expect("a reply");
        assert_eq!((rebuilt.cols, rebuilt.rows), (80, 24));
        assert!(String::from_utf8_lossy(&rebuilt.vt).contains("compacted text"));

        // Another build's identity: no reply, its own code.
        let id = handover::build_id();
        let mut other = good.clone();
        let at = 8;
        assert_eq!(&other[at..at + id.len()], id.as_bytes());
        other[at] ^= 1;
        assert_eq!(run(other), (COMPACT_OTHER_BUILD, Vec::new()));

        // A request cut short: a failure, no reply.
        let (code, reply) = run(good[..good.len() - 3].to_vec());
        assert_eq!((code, reply.is_empty()), (COMPACT_FAILED, true));
        assert_eq!(compact_main(&["--fd".into()]), COMPACT_USAGE);
        // A number that is no socket is not taken over (nor closed).
        assert_eq!(
            compact_main(&["--fd".into(), "1009".into()]),
            COMPACT_FAILED
        );
        session.shutdown();
    }
}
