//! Programs that outlive bateri: the holder process (`bateri hold`), its
//! frame and its live connection.
//!
//! A holder keeps a copy of every pane's PTY master so the programs survive
//! the bateri that spawned them; the next bateri connects, takes them and
//! acknowledges, and the holder exits. It is born one of two ways, and the
//! first bytes its spawner sends tell which:
//!
//! - **At the moment of an update** (the frame's magic first): the old
//!   bateri freezes every pane, gives the holder the whole bundle at once and
//!   exits without hanging the shells up. Bounded by [`HOLD_LIMIT`]: no shell
//!   outlives an update nobody completed. Its socket is [`HANDOVER_SOCKET`].
//! - **Bound** ([`spawn_bound`], [`Bound`]): spawned when bateri starts and
//!   kept connected. bateri registers each pane as it is born (a copy of the
//!   master, its identity and `bt-core`'s state blob), sends the layout and
//!   newer blobs as they change, and releases a pane when it closes. While
//!   bound the holder never reads a master — bateri does — and drops the
//!   copy of a pane whose program exited ([`jobs::exit_fd`]), so a lost
//!   release leaks no device. When the connection ends without a goodbye
//!   (bateri crashed, was force-quit or killed) the holder goes
//!   **detached**: it builds the bundle from what it was sent last and holds
//!   until a client takes it — without a time limit, because the programs
//!   are meant to run on. A deliberate handover (quit, update) sends the
//!   frozen bundle over the live connection instead, and a quiet exit closes
//!   the copies without touching a program. Its socket is
//!   `handover-<pid>` — one per holder, so the detached holder of a bateri
//!   that crashed and the bound holder of the next one share a directory.
//!
//! The pieces:
//!
//! - **The frame** ([`Bundle`], [`write_frame`], [`read_frame`]) is **frozen**:
//!   the holder is the old binary and the client the new one, so the bytes
//!   below never change under [`FRAME_VERSION`] 1 — a change is a new
//!   version, and a client facing a version it does not know says "release
//!   all" ([`take`]). The frame does not look into the panes' blobs or
//!   buffers: those are the platform shell's and `bt-core`'s, versioned on
//!   their own.
//!
//!   ```text
//!   "BTHO" | version u32
//!   layout: len u64, bytes
//!   panes: count u32, then per pane:
//!     'F' (one byte carrying the master by SCM_RIGHTS)
//!     tab: len u32, UUID text | pid u32 | start u64
//!     flags u32 (bit 0: ended, bit 1: cut — the oldest bytes were dropped)
//!     blob: len u64, bytes | buffer: len u64, bytes
//!   ```
//!
//!   Integers are little-endian. A reader ignores a flag bit it does not
//!   know, so a bit is added without a new version (the cut bit was). The
//!   client opens with [`TAKE`] and answers
//!   with one-byte messages; their values are fixed **across every version**
//!   (a newer client must be able to release an older holder's panes):
//!   [`ACK`], [`RELEASE`] + the pane's position as `u32`, [`RELEASE_ALL`]. The
//!   holder tells its spawner [`READY`] once it listens and owns the instance
//!   directories.
//! - **The bound connection** is an internal detail of one binary: the
//!   spawner's handshake carries the build's identity ([`build_id`]) and a
//!   holder of another build refuses to hold anything. Its messages are in
//!   the bound section below.
//! - **The holder** ([`hold`], [`hold_main`]): detached from the spawner's
//!   session (`setsid`, `SIGHUP` ignored, `chdir /`, standard I/O on
//!   `/dev/null`, every other inherited descriptor closed — a stray copy of a
//!   master would defeat the hang-up), no AppKit. It binds its socket in the
//!   instance's first directory (`0700`) and, once it holds the bundle, takes
//!   every directory of the instance over from its spawner
//!   ([`ssh_route::adopt_instance`]). While nobody is connected it **drains**
//!   the masters into each pane's buffer and records a master that reports
//!   the end (EOF/EIO). The update's holder stops at [`BUFFER_LIMIT`] (the
//!   program blocks on back-pressure for the seconds an update takes); a
//!   detached holder never stops (a server left running overnight must not
//!   freeze) and drops the **oldest** bytes past the limit, marking the pane
//!   cut. A peer of another uid is closed without a byte, and so is one that
//!   does not say [`TAKE`] (a liveness probe — the sweep's — costs nothing);
//!   a bound holder closes every client without a byte, after it processed
//!   an end of its spawner that was already waiting (a launch right after a
//!   crash must find the detached holder). Once the bundle is given it stops
//!   draining — the client reads the masters now — and waits:
//!   [`ACK`] closes its copies and exits, [`RELEASE`] closes one copy at once,
//!   [`RELEASE_ALL`] closes all and exits, a disconnect without an ACK sends
//!   it back to waiting (the buffers kept: the next client needs them).
//!   Every end that means "nobody took these programs" — [`RELEASE_ALL`], the
//!   limit — closes the copies **and** sends `SIGHUP` to each program still
//!   the process it was (its start time): closing the last copy of a master
//!   hangs the shell up only when nothing else holds one, and a child bateri
//!   spawned before the master was close-on-exec may.
//!
//! **Known limits:** bytes a client read from a master before it went away
//! without an ACK are lost to the next one. On macOS `recvmsg` has no
//! `MSG_CMSG_CLOEXEC`: a received master is close-on-exec only after it
//! arrives, so a child spawned by another thread of the receiving process in
//! that window inherits a copy — the new bateri takes before it spawns
//! anything.
//!
//! **Order for the new bateri**: [`arrive`] (it takes every holder, drops
//! duplicates, counts the attempt in each holder's directory and adopts the
//! instance directories) → adopt the panes ([`Arrival::release`] those that
//! fall back) → [`Arrival::finish`] → once the launch settled, clear the
//! attempt markers ([`Arrival::marked`]). A launch that never settles leaves
//! its count: the next one restores no screen, the one after gives the
//! programs up ([`restore::attempt_mode`]).

use std::collections::VecDeque;
use std::io::{self, Read, Write};
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use bt_core::TabId;

use crate::jobs::{self, ShellParent};
use crate::restore;
use crate::ssh_route::{self, SUN_PATH};

/// The socket of a holder born at the moment of an update, in the instance
/// directory. The name tells the new bateri which kind of holder it took
/// from ([`is_bound_socket`]).
pub const HANDOVER_SOCKET: &str = "handover";

/// A bound holder's socket: this prefix and the holder's pid.
const BOUND_SOCKET_PREFIX: &str = "handover-";

/// A bound holder's socket name.
fn bound_socket(pid: u32) -> String {
    format!("{BOUND_SOCKET_PREFIX}{pid}")
}

/// Whether `name` is a holder's socket: [`HANDOVER_SOCKET`] or a bound
/// holder's `handover-<pid>`. The sweep spares a directory one of them
/// listens in and removes them with it.
pub fn is_holder_socket_name(name: &str) -> bool {
    name == HANDOVER_SOCKET
        || name.strip_prefix(BOUND_SOCKET_PREFIX).is_some_and(|pid| {
            (1..=10).contains(&pid.len()) && pid.bytes().all(|byte| byte.is_ascii_digit())
        })
}

/// Whether `socket` is a bound holder's — one that held for a crash or a
/// quit — rather than the update's ([`HANDOVER_SOCKET`]).
pub fn is_bound_socket(socket: &Path) -> bool {
    socket
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name != HANDOVER_SOCKET && is_holder_socket_name(name))
}

/// The frame's first bytes.
const MAGIC: [u8; 4] = *b"BTHO";

/// The frame's version: **frozen** — the layout in the module doc is
/// version 1, byte for byte.
pub const FRAME_VERSION: u32 = 1;

/// How long a holder lives without an acknowledgement — a **design
/// constant**. Long enough for Sparkle to install and start the
/// new bateri on a slow disk and for that bateri to come up; short enough
/// that an update nobody completes does not keep the user's programs
/// blocked out of sight for long. After it every shell is hung up.
pub const HOLD_LIMIT: Duration = Duration::from_secs(120);

/// How many bytes the holder keeps per pane — a **design constant**. More
/// than a full default scrollback of output (10 000 lines of a wide grid), so
/// a program that prints during the handover is not held back by an ordinary
/// burst. Past it the update's holder stops reading (the pane blocks on the
/// PTY until the new bateri reads, nothing is dropped) and a detached holder
/// drops the oldest bytes.
pub const BUFFER_LIMIT: usize = 4 << 20;

/// How many panes a bound holder keeps — a **design constant**. Each costs it
/// up to three descriptors while bound (the master, the exit watch, a journal
/// region) and a fourth for the moment a handover's frame brings its own copy
/// of the master; all of them, beside the standard three, the spawner's
/// connection, the listener and a client, must stay below `select`'s
/// `FD_SETSIZE`. Far above any set of windows anyone keeps open; a pane past
/// it is refused out loud rather than silently left undrained.
pub const PANE_CAP: usize = 250;

// Four per pane, and six besides: the standard three, the spawner's
// connection, the listener and one client.
const _: () = assert!(4 * PANE_CAP + 6 < libc::FD_SETSIZE);

/// How long a holder waits after a connection it could not accept — a
/// **design constant**: the failure is a full descriptor table, which frees
/// up on its own; short against a client's wait, long against a busy loop.
const ACCEPT_PAUSE: Duration = Duration::from_millis(100);

/// How often a detached holder asks whether the holder an unconfirmed pane
/// was taken from still listens — a **design constant**: only while such a
/// pane exists (a crash in the moment between a takeover and its
/// acknowledgement), so it costs nothing otherwise; short enough that a
/// program left to nobody blocks only briefly.
const UNCONFIRMED_POLL: Duration = Duration::from_secs(1);

/// The limit on one blocking read or write between the two sides: the
/// spawner's bundle, a client's frame, the client's wait for the holder's
/// close. The holder's operations are bounded by [`HOLD_LIMIT`] as a whole
/// on top ([`Wire`]).
pub const HAND_WAIT: Duration = Duration::from_secs(5);

/// How long the holder waits for an accepted connection's [`TAKE`] — a
/// **design constant**: a client sends it right after connecting, a probe
/// closes at once; the holder does not drain meanwhile, so it is short.
const TAKE_WAIT: Duration = Duration::from_secs(1);

/// The decoder's sanity limits: a field, a count or a frame past them is
/// broken and refused. A field's bytes are allocated as they arrive, never
/// from the announced length. A buffer is at most [`BUFFER_LIMIT`] plus a
/// tail; a blob holds a pane's VT with its scrollback — both far below.
const FIELD_LIMIT: u64 = 256 << 20;
const TOTAL_LIMIT: u64 = 1 << 30;
const PANE_LIMIT: u32 = 4096;
const TAB_LIMIT: u64 = 64;

/// The byte that carries a master (`SCM_RIGHTS`).
const FD_MARK: u8 = b'F';
/// Holder → spawner: listening, the directories are the holder's.
pub const READY: u8 = b'K';
/// Client → holder, first: "give me the bundle".
pub const TAKE: u8 = b'T';
/// Client → holder: every pane is taken; close the copies and exit.
pub const ACK: u8 = b'A';
/// Client → holder: close this pane's copy now (`u32` position follows).
pub const RELEASE: u8 = b'R';
/// Client → holder: close every copy and exit.
pub const RELEASE_ALL: u8 = b'X';

/// The `ended` bit of a pane's flags.
const ENDED: u32 = 1;
/// The `cut` bit of a pane's flags: a detached holder dropped the buffer's
/// oldest bytes.
const CUT: u32 = 2;

/// One read or write's size: the deadline is checked between them.
const CHUNK: usize = 64 * 1024;

/// The exit codes of `bateri hold`.
pub const EXIT_DONE: i32 = 0;
pub const EXIT_FAILED: i32 = 1;
pub const EXIT_USAGE: i32 = 2;

pub const USAGE: &str = "usage: bateri hold --fd FD --dir INSTANCE_DIR [--dir INSTANCE_DIR]...";

// ─── the frame ───────────────────────────────────────────────────────────

/// One pane across the frame.
#[derive(Debug)]
pub struct HeldPane {
    pub tab: TabId,
    /// The PTY's child and its start time (`jobs::start_time`): the new
    /// bateri's exit watch is made from them (`jobs::exit_fd`).
    pub pid: u32,
    pub start: u64,
    /// The holder saw the master end (EOF/EIO): the program is gone, the
    /// buffer is its last output.
    pub ended: bool,
    /// The holder dropped the buffer's oldest bytes: it starts mid-stream.
    pub cut: bool,
    /// Opaque: the platform shell's state for the pane.
    pub blob: Vec<u8>,
    /// Opaque: the bytes to read before the master's — the frozen side's
    /// tail, then whatever the holder drained.
    pub buffer: Vec<u8>,
    /// The PTY master (non-blocking, a shared open file description).
    pub master: OwnedFd,
    /// Its position in the frame it came in: what [`Link::release`] names.
    position: u32,
}

impl HeldPane {
    /// A pane the old bateri gives: nothing ended yet, the buffer its tail.
    pub fn new(
        tab: TabId,
        pid: u32,
        start: u64,
        blob: Vec<u8>,
        buffer: Vec<u8>,
        master: OwnedFd,
    ) -> HeldPane {
        HeldPane {
            tab,
            pid,
            start,
            ended: false,
            cut: false,
            blob,
            buffer,
            master,
            position: 0,
        }
    }
}

/// What crosses: the window layout (opaque) and the panes.
#[derive(Debug, Default)]
pub struct Bundle {
    pub layout: Vec<u8>,
    pub panes: Vec<HeldPane>,
}

/// A frame or handover failure.
#[derive(Debug)]
pub enum HandoverError {
    Io(io::Error),
    /// The peer is not this user (`LOCAL_PEERCRED` / `SO_PEERCRED`).
    Peer,
    /// Not our frame at all.
    NotAFrame,
    /// Our frame of a version this binary does not know.
    Version(u32),
    Malformed(&'static str),
    /// The holder closed without a byte: a bound holder, whose bateri lives
    /// ([`open`]).
    Declined,
    /// A descriptor arrived truncated — the descriptor table is full here
    /// (EMFILE shows as this, not as an error): a passing condition, not a
    /// broken frame, and its byte is read, so the stream can be followed.
    Truncated,
}

impl From<io::Error> for HandoverError {
    fn from(error: io::Error) -> HandoverError {
        HandoverError::Io(error)
    }
}

/// One side of a connection: every read and write is bounded by
/// [`HAND_WAIT`] and, with a deadline, by the deadline as a whole — a peer
/// that trickles a byte now and then cannot stretch an operation past it.
/// A **patient** wire has no bound at all: the bound connection's two ends
/// write whole messages, and a timeout in the middle of one would be taken
/// for the other side's death while it lives.
struct Wire<'a> {
    stream: &'a UnixStream,
    deadline: Option<Instant>,
    patient: bool,
}

impl<'a> Wire<'a> {
    fn new(stream: &'a UnixStream, deadline: Option<Instant>) -> Wire<'a> {
        Wire {
            stream,
            deadline,
            patient: false,
        }
    }

    fn patient(stream: &'a UnixStream) -> Wire<'a> {
        Wire {
            stream,
            deadline: None,
            patient: true,
        }
    }

    /// Sets the next call's timeout; past the deadline nothing is tried.
    fn arm(&self, write: bool) -> io::Result<()> {
        let mut wait = Some(HAND_WAIT);
        if self.patient {
            wait = None;
        } else if let Some(deadline) = self.deadline {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(io::ErrorKind::TimedOut.into());
            }
            wait = Some(HAND_WAIT.min(remaining));
        }
        // A failure is not one: macOS refuses `setsockopt` with EINVAL once
        // the peer closed, and then the call cannot block anyway — the
        // buffered bytes or the end are there.
        let _ = if write {
            self.stream.set_write_timeout(wait)
        } else {
            self.stream.set_read_timeout(wait)
        };
        Ok(())
    }

    fn write_all(&self, mut bytes: &[u8]) -> io::Result<()> {
        while !bytes.is_empty() {
            self.arm(true)?;
            match (&*self.stream).write(&bytes[..bytes.len().min(CHUNK)]) {
                Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
                Ok(n) => bytes = &bytes[n..],
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }

    fn read_exact(&self, mut buf: &mut [u8]) -> io::Result<()> {
        while !buf.is_empty() {
            self.arm(false)?;
            match (&*self.stream).read(buf) {
                Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
                Ok(n) => buf = &mut buf[n..],
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }

    /// `len` bytes, the vector growing with what arrives.
    fn read_vec(&self, len: usize) -> io::Result<Vec<u8>> {
        let mut bytes = Vec::new();
        while bytes.len() < len {
            let start = bytes.len();
            bytes.resize(start + (len - start).min(CHUNK), 0);
            self.read_exact(&mut bytes[start..])?;
        }
        Ok(bytes)
    }

    fn u32(&self) -> io::Result<u32> {
        let mut bytes = [0u8; 4];
        self.read_exact(&mut bytes)?;
        Ok(u32::from_le_bytes(bytes))
    }

    fn u64(&self) -> io::Result<u64> {
        let mut bytes = [0u8; 8];
        self.read_exact(&mut bytes)?;
        Ok(u64::from_le_bytes(bytes))
    }
}

/// One pane as the writer sees it: the old side's [`HeldPane`] or the
/// holder's own record.
struct PaneView<'a> {
    tab: &'a str,
    pid: u32,
    start: u64,
    ended: bool,
    cut: bool,
    blob: &'a [u8],
    buffer: &'a [u8],
    master: RawFd,
}

/// Writes `bundle` as one frame — the old bateri's side of the
/// `socketpair`. Each call is bounded by [`HAND_WAIT`].
pub fn write_frame(stream: &UnixStream, bundle: &Bundle) -> io::Result<()> {
    write_bundle(&Wire::new(stream, None), bundle)
}

fn write_bundle(wire: &Wire<'_>, bundle: &Bundle) -> io::Result<()> {
    let views = bundle.panes.iter().map(|pane| PaneView {
        tab: pane.tab.as_str(),
        pid: pane.pid,
        start: pane.start,
        ended: pane.ended,
        cut: pane.cut,
        blob: &pane.blob,
        buffer: &pane.buffer,
        master: pane.master.as_raw_fd(),
    });
    write_views(wire, &bundle.layout, views.collect())
}

fn write_views(wire: &Wire<'_>, layout: &[u8], panes: Vec<PaneView<'_>>) -> io::Result<()> {
    wire.write_all(&MAGIC)?;
    wire.write_all(&FRAME_VERSION.to_le_bytes())?;
    write_field(wire, layout)?;
    let count = u32::try_from(panes.len()).map_err(|_| io::ErrorKind::InvalidInput)?;
    wire.write_all(&count.to_le_bytes())?;
    for pane in panes {
        wire.arm(true)?;
        send_fd(wire.stream, pane.master)?;
        write_tab(wire, pane.tab)?;
        wire.write_all(&pane.pid.to_le_bytes())?;
        wire.write_all(&pane.start.to_le_bytes())?;
        let mut flags = 0;
        if pane.ended {
            flags |= ENDED;
        }
        if pane.cut {
            flags |= CUT;
        }
        wire.write_all(&flags.to_le_bytes())?;
        write_field(wire, pane.blob)?;
        write_field(wire, pane.buffer)?;
    }
    Ok(())
}

fn write_field(wire: &Wire<'_>, bytes: &[u8]) -> io::Result<()> {
    wire.write_all(&(bytes.len() as u64).to_le_bytes())?;
    wire.write_all(bytes)
}

/// A tab id: its length as `u32`, then its text.
fn write_tab(wire: &Wire<'_>, tab: &str) -> io::Result<()> {
    let len = u32::try_from(tab.len()).map_err(|_| io::ErrorKind::InvalidInput)?;
    wire.write_all(&len.to_le_bytes())?;
    wire.write_all(tab.as_bytes())
}

/// Reads one frame: the header first (a version this binary does not know
/// is [`HandoverError::Version`], nothing read past it), then the body.
/// Each call is bounded by [`HAND_WAIT`].
pub fn read_frame(stream: &UnixStream) -> Result<Bundle, HandoverError> {
    let wire = Wire::new(stream, None);
    read_header(&wire)?;
    read_body(&wire)
}

fn read_header(wire: &Wire<'_>) -> Result<(), HandoverError> {
    let mut magic = [0u8; 4];
    wire.read_exact(&mut magic)?;
    if magic != MAGIC {
        return Err(HandoverError::NotAFrame);
    }
    read_version(wire)
}

/// The frame's version, after its magic.
fn read_version(wire: &Wire<'_>) -> Result<(), HandoverError> {
    match wire.u32()? {
        FRAME_VERSION => Ok(()),
        other => Err(HandoverError::Version(other)),
    }
}

fn read_body(wire: &Wire<'_>) -> Result<Bundle, HandoverError> {
    let mut total = 0u64;
    let layout = read_field(wire, wire.u64()?, FIELD_LIMIT, &mut total)?;
    let panes = read_panes(wire, &mut total)?;
    Ok(Bundle { layout, panes })
}

/// The body after the layout: the panes.
fn read_panes(wire: &Wire<'_>, total: &mut u64) -> Result<Vec<HeldPane>, HandoverError> {
    let count = wire.u32()?;
    if count > PANE_LIMIT {
        return Err(HandoverError::Malformed("pane count"));
    }
    let mut panes = Vec::new();
    for position in 0..count {
        wire.arm(false)?;
        let master = recv_fd(wire.stream)?;
        let tab = read_tab(wire, total)?;
        let pid = wire.u32()?;
        let start = wire.u64()?;
        // Bits this binary does not know are ignored: a later one adds
        // meaning a reader without it can do without.
        let flags = wire.u32()?;
        let blob = read_field(wire, wire.u64()?, FIELD_LIMIT, total)?;
        let buffer = read_field(wire, wire.u64()?, FIELD_LIMIT, total)?;
        panes.push(HeldPane {
            tab,
            pid,
            start,
            ended: flags & ENDED != 0,
            cut: flags & CUT != 0,
            blob,
            buffer,
            master,
            position,
        });
    }
    Ok(panes)
}

/// A tab id ([`write_tab`]).
fn read_tab(wire: &Wire<'_>, total: &mut u64) -> Result<TabId, HandoverError> {
    let tab = read_field(wire, u64::from(wire.u32()?), TAB_LIMIT, total)?;
    std::str::from_utf8(&tab)
        .ok()
        .and_then(TabId::parse)
        .ok_or(HandoverError::Malformed("tab id"))
}

/// A field of `len` bytes, refused past `limit` or past [`TOTAL_LIMIT`]
/// for the frame (`total` counts).
fn read_field(
    wire: &Wire<'_>,
    len: u64,
    limit: u64,
    total: &mut u64,
) -> Result<Vec<u8>, HandoverError> {
    *total = total.saturating_add(len);
    if len > limit || *total > TOTAL_LIMIT {
        return Err(HandoverError::Malformed("field length"));
    }
    let len = usize::try_from(len).map_err(|_| HandoverError::Malformed("field length"))?;
    Ok(wire.read_vec(len)?)
}

/// Room for the ancillary data of one `recvmsg`: a few descriptors, so a
/// peer that sends more is seen (and refused) rather than truncated.
const FD_ROOM: usize = 4;

/// Sends `fd` on [`FD_MARK`]: one byte, the descriptor attached to it.
fn send_fd(stream: &UnixStream, fd: RawFd) -> io::Result<()> {
    let mut byte = [FD_MARK];
    let mut iov = libc::iovec {
        iov_base: byte.as_mut_ptr().cast(),
        iov_len: 1,
    };
    // `u64`s: the control buffer must be aligned for `cmsghdr`.
    let mut control = [0u64; 8];
    // SAFETY: a pure size computation.
    let space = unsafe { libc::CMSG_SPACE(size_of::<libc::c_int>() as libc::c_uint) } as usize;
    debug_assert!(space <= size_of_val(&control));
    // SAFETY: an all-zero `msghdr` is a valid empty message.
    let mut msg: libc::msghdr = unsafe { std::mem::zeroed() };
    msg.msg_iov = &raw mut iov;
    msg.msg_iovlen = 1;
    msg.msg_control = control.as_mut_ptr().cast();
    msg.msg_controllen = space as _;
    // SAFETY: the control buffer is `space` bytes long and aligned, so the
    // first header and its data fit; `fd` is copied in, not owned.
    unsafe {
        let header = libc::CMSG_FIRSTHDR(&raw const msg);
        (*header).cmsg_level = libc::SOL_SOCKET;
        (*header).cmsg_type = libc::SCM_RIGHTS;
        (*header).cmsg_len = libc::CMSG_LEN(size_of::<libc::c_int>() as libc::c_uint) as _;
        std::ptr::write_unaligned(libc::CMSG_DATA(header).cast::<libc::c_int>(), fd);
    }
    loop {
        // SAFETY: `msg` points at this frame's buffers for the call.
        let sent = unsafe { libc::sendmsg(stream.as_raw_fd(), &raw const msg, 0) };
        match sent {
            1 => return Ok(()),
            0 => return Err(io::ErrorKind::WriteZero.into()),
            _ => {
                let error = io::Error::last_os_error();
                if error.kind() != io::ErrorKind::Interrupted {
                    return Err(error);
                }
            }
        }
    }
}

/// Receives the descriptor [`send_fd`] sent: exactly one byte (the
/// ancillary data travels with the first byte of its `sendmsg`), exactly one
/// descriptor, nothing truncated. Every descriptor that arrived is owned at
/// once, so a refused message leaks none.
fn recv_fd(stream: &UnixStream) -> Result<OwnedFd, HandoverError> {
    let mut byte = [0u8];
    let mut iov = libc::iovec {
        iov_base: byte.as_mut_ptr().cast(),
        iov_len: 1,
    };
    let mut control = [0u64; 16];
    // SAFETY: a pure size computation.
    let space =
        unsafe { libc::CMSG_SPACE((FD_ROOM * size_of::<libc::c_int>()) as libc::c_uint) } as usize;
    debug_assert!(space <= size_of_val(&control));
    // SAFETY: an all-zero `msghdr` is a valid empty message.
    let mut msg: libc::msghdr = unsafe { std::mem::zeroed() };
    msg.msg_iov = &raw mut iov;
    msg.msg_iovlen = 1;
    msg.msg_control = control.as_mut_ptr().cast();
    msg.msg_controllen = space as _;
    // macOS has no `MSG_CMSG_CLOEXEC` (the module doc's known limit).
    #[cfg(target_os = "linux")]
    let flags = libc::MSG_CMSG_CLOEXEC;
    #[cfg(not(target_os = "linux"))]
    let flags = 0;
    let received = loop {
        // SAFETY: `msg` points at this frame's buffers for the call.
        let received = unsafe { libc::recvmsg(stream.as_raw_fd(), &raw mut msg, flags) };
        if received >= 0 {
            break received;
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error.into());
        }
    };
    let mut fds = Vec::new();
    // SAFETY: the kernel filled `control` up to `msg_controllen`; the
    // `CMSG_*` walk stays inside it, and each descriptor read is a fresh one
    // this process now owns.
    unsafe {
        let mut header = libc::CMSG_FIRSTHDR(&raw const msg);
        while !header.is_null() {
            if (*header).cmsg_level == libc::SOL_SOCKET && (*header).cmsg_type == libc::SCM_RIGHTS {
                let data = libc::CMSG_DATA(header).cast::<libc::c_int>();
                let bytes = (*header).cmsg_len as usize - libc::CMSG_LEN(0) as usize;
                for i in 0..bytes / size_of::<libc::c_int>() {
                    let fd = OwnedFd::from_raw_fd(std::ptr::read_unaligned(data.add(i)));
                    set_cloexec(fd.as_raw_fd());
                    fds.push(fd);
                }
            }
            header = libc::CMSG_NXTHDR(&raw const msg, header);
        }
    }
    if received == 0 {
        return Err(HandoverError::Io(io::ErrorKind::UnexpectedEof.into()));
    }
    if byte[0] != FD_MARK {
        return Err(HandoverError::Malformed("descriptor"));
    }
    if msg.msg_flags & libc::MSG_CTRUNC != 0 {
        return Err(HandoverError::Truncated);
    }
    if fds.len() != 1 {
        return Err(HandoverError::Malformed("descriptor"));
    }
    Ok(fds.remove(0))
}

fn set_cloexec(fd: RawFd) {
    // SAFETY: `fd` is open; `F_SETFD` changes only its descriptor flags.
    unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) };
}

/// The uid and pid of the process at the other end of `stream`.
#[cfg(target_os = "macos")]
fn peer(stream: &UnixStream) -> io::Result<(u32, u32)> {
    // SAFETY: an all-zero `xucred` is valid; the kernel fills it.
    let mut cred: libc::xucred = unsafe { std::mem::zeroed() };
    let mut len = size_of::<libc::xucred>() as libc::socklen_t;
    // SAFETY: `cred` and `len` belong to this frame and `len` is its size.
    let status = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_LOCAL,
            libc::LOCAL_PEERCRED,
            (&raw mut cred).cast(),
            &raw mut len,
        )
    };
    if status != 0 {
        return Err(io::Error::last_os_error());
    }
    if cred.cr_version != libc::XUCRED_VERSION {
        return Err(io::Error::other("unknown xucred version"));
    }
    let mut pid: libc::pid_t = 0;
    let mut len = size_of::<libc::pid_t>() as libc::socklen_t;
    // SAFETY: as above.
    let status = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_LOCAL,
            libc::LOCAL_PEERPID,
            (&raw mut pid).cast(),
            &raw mut len,
        )
    };
    if status != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok((cred.cr_uid, u32::try_from(pid).map_err(io::Error::other)?))
}

/// The uid and pid of the process at the other end of `stream`.
#[cfg(target_os = "linux")]
fn peer(stream: &UnixStream) -> io::Result<(u32, u32)> {
    // SAFETY: an all-zero `ucred` is valid; the kernel fills it.
    let mut cred: libc::ucred = unsafe { std::mem::zeroed() };
    let mut len = size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: `cred` and `len` belong to this frame and `len` is its size.
    let status = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&raw mut cred).cast(),
            &raw mut len,
        )
    };
    if status != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok((cred.uid, u32::try_from(cred.pid).map_err(io::Error::other)?))
}

/// The holder sockets in `dir` ([`is_holder_socket_name`]), each with its
/// modification time (its bind: a younger holder binds later).
fn holder_sockets(dir: &Path) -> Vec<(PathBuf, std::time::SystemTime)> {
    use std::os::unix::fs::FileTypeExt;
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|entry| {
            entry.file_type().is_ok_and(|kind| kind.is_socket())
                && entry
                    .file_name()
                    .to_str()
                    .is_some_and(is_holder_socket_name)
        })
        .map(|entry| {
            let modified = entry
                .metadata()
                .and_then(|meta| meta.modified())
                .unwrap_or(std::time::UNIX_EPOCH);
            (entry.path(), modified)
        })
        .collect()
}

/// Whether a holder listens on any holder socket in `dir`. The connection
/// sends nothing, so the holder closes it without serving (no [`TAKE`]).
pub fn holder_listening(dir: &Path) -> bool {
    holder_sockets(dir)
        .iter()
        .any(|(socket, _)| socket_listening(socket))
}

/// Whether something listens on `socket` — a probe, closed unanswered.
fn socket_listening(socket: &Path) -> bool {
    UnixStream::connect(socket).is_ok()
}

// ─── the client (the new bateri) ─────────────────────────────────────────

/// The connection to a holder after [`take`]: dropping it without
/// [`Link::ack`] is a disconnect without an acknowledgement — the holder
/// waits for the next client.
#[derive(Debug)]
pub struct Link {
    stream: UnixStream,
    holder: u32,
    socket: PathBuf,
}

/// A holder after its frame's layout: [`open`] read the header and the
/// layout, the panes are still on the wire — a caller that finds another
/// bundle's layout lets go before reading them.
#[derive(Debug)]
pub struct Opened {
    link: Link,
    /// The frame's layout field (opaque here, [`layout_of`]).
    pub layout: Vec<u8>,
    total: u64,
}

/// Connects to the holder on `socket`, checks it is user `uid`, asks for the
/// bundle and reads up to its layout. A holder that closes without a byte is
/// [`HandoverError::Declined`] — a bound holder, whose bateri lives. A frame of
/// a version this binary does not know is released at once
/// ([`RELEASE_ALL`]: the holder hangs every shell up) and answered with
/// [`HandoverError::Version`]; the caller falls back.
pub fn open(socket: &Path, uid: u32) -> Result<Opened, HandoverError> {
    if socket.as_os_str().len() >= SUN_PATH {
        return Err(io::Error::from(io::ErrorKind::InvalidInput).into());
    }
    let stream = UnixStream::connect(socket)?;
    let (peer_uid, holder) = peer(&stream)?;
    if peer_uid != uid {
        return Err(HandoverError::Peer);
    }
    let wire = Wire::new(&stream, None);
    wire.write_all(&[TAKE])?;
    let mut magic = [0u8; 4];
    match wire.read_exact(&mut magic[..1]) {
        Ok(()) => {}
        // Linux resets a connection closed with our `TAKE` unread.
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::UnexpectedEof | io::ErrorKind::ConnectionReset
            ) =>
        {
            return Err(HandoverError::Declined);
        }
        Err(error) => return Err(error.into()),
    }
    wire.read_exact(&mut magic[1..])?;
    if magic != MAGIC {
        return Err(HandoverError::NotAFrame);
    }
    match read_version(&wire) {
        Ok(()) => {}
        Err(HandoverError::Version(version)) => {
            let _ = wire.write_all(&[RELEASE_ALL]);
            return Err(HandoverError::Version(version));
        }
        Err(error) => return Err(error),
    }
    let mut total = 0u64;
    let layout = read_field(&wire, wire.u64()?, FIELD_LIMIT, &mut total)?;
    Ok(Opened {
        link: Link {
            stream,
            holder,
            socket: socket.to_owned(),
        },
        layout,
        total,
    })
}

impl Opened {
    /// The rest of the frame: the panes, with the layout read before.
    pub fn body(self) -> Result<(Bundle, Link), (HandoverError, Link)> {
        let Opened {
            link,
            layout,
            mut total,
        } = self;
        match read_panes(&link.wire(), &mut total) {
            Ok(panes) => Ok((Bundle { layout, panes }, link)),
            Err(error) => Err((error, link)),
        }
    }
}

/// Connects to the update's holder in `dir` ([`HANDOVER_SOCKET`]) and takes
/// its bundle ([`open`], then the body); a holder that closes without a byte
/// is the end of the stream.
pub fn take(dir: &Path, uid: u32) -> Result<(Bundle, Link), HandoverError> {
    let opened = match open(&dir.join(HANDOVER_SOCKET), uid) {
        Ok(opened) => opened,
        Err(HandoverError::Declined) => {
            return Err(io::Error::from(io::ErrorKind::UnexpectedEof).into());
        }
        Err(error) => return Err(error),
    };
    opened.body().map_err(|(error, _)| error)
}

impl Link {
    /// The holder's pid: the `from` of [`ssh_route::adopt_instance`].
    pub fn holder_pid(&self) -> u32 {
        self.holder
    }

    /// The socket this link connected to: its name tells which kind of
    /// holder it is ([`is_bound_socket`]).
    pub fn socket(&self) -> &Path {
        &self.socket
    }

    fn wire(&self) -> Wire<'_> {
        Wire::new(&self.stream, None)
    }

    /// Releases one pane: this side's copy of its master is closed **here**
    /// (the argument is consumed), then the holder closes its own. Nobody is
    /// signalled: when no other copy is open that is the hang-up, and when
    /// another holder carries the same program it lives on there.
    pub fn release(&mut self, pane: HeldPane) -> io::Result<()> {
        let position = pane.position;
        drop(pane);
        let mut message = [0u8; 5];
        message[0] = RELEASE;
        message[1..].copy_from_slice(&position.to_le_bytes());
        self.wire().write_all(&message)
    }

    /// Every pane is adopted: the holder closes its copies and exits. Waits
    /// (at most [`HAND_WAIT`]) for the holder to close the connection, so the
    /// caller knows the acknowledgement arrived.
    pub fn ack(self) -> io::Result<()> {
        self.wire().write_all(&[ACK])?;
        self.until_closed()
    }

    /// Gives every pane up: this side's copies are closed here (`bundle` is
    /// consumed), then the holder closes its own and exits.
    pub fn release_all(self, bundle: Bundle) -> io::Result<()> {
        drop(bundle);
        self.wire().write_all(&[RELEASE_ALL])?;
        self.until_closed()
    }

    fn until_closed(self) -> io::Result<()> {
        // Fails once the holder closed (macOS' EINVAL) — then nothing blocks.
        let _ = self.stream.set_read_timeout(Some(HAND_WAIT));
        let mut rest = Vec::new();
        (&self.stream).read_to_end(&mut rest).map(|_| ())
    }
}

/// `SIGHUP` to `pid` if it is still the process that started at `start` and
/// its exit watch (if any) has not fired: a pid that exited may be another
/// process's now. The master copies are closed before — the slave's hang-up
/// is the mechanism, the signal the belt and braces for a copy someone else
/// holds.
fn hang_up(pid: u32, start: u64, exit: Option<&OwnedFd>) {
    if exit.is_some_and(|exit| fd_ready(exit.as_fd())) {
        return;
    }
    if jobs::start_time(pid) != Some(start) {
        return;
    }
    let Ok(pid) = libc::pid_t::try_from(pid) else {
        return;
    };
    // SAFETY: a signal to a pid whose start time says it is still the
    // process the pane was.
    unsafe { libc::kill(pid, libc::SIGHUP) };
}

/// Whether `fd` is readable now (a zero-timeout `select`; one that does not
/// fit an `fd_set` counts as not).
fn fd_ready(fd: BorrowedFd<'_>) -> bool {
    readable(&[fd.as_raw_fd()], Some(Duration::ZERO))[0]
}

// ─── the two bateris' own side ───────────────────────────────────────────
//
// What crosses inside the frame's opaque fields, and the two process steps
// around it: the old bateri spawns the holder and gives it the bundle
// ([`spawn_holder`], [`Spawned::give`]), the new one takes every holder's
// bundle at its sequence point ([`arrive`]).

/// The layout blob's first line: this word, [`LAYOUT_VERSION`] and the
/// bundle id of the bateri that wrote it; the rest is session restore's layout text
/// (`restore::Saved::render`).
const LAYOUT_WORD: &str = "bateri-handover";

/// The layout blob's version. A new bateri reads this one and the one
/// before it; there is none before it yet.
pub const LAYOUT_VERSION: u32 = 1;

/// The layout blob for `layout` (session restore's text) of the bundle `bundle_id`.
pub fn layout_blob(bundle_id: &str, layout: &str) -> Vec<u8> {
    format!("{LAYOUT_WORD} {LAYOUT_VERSION} {bundle_id}\n{layout}").into_bytes()
}

/// A layout blob's bundle id and layout text; `None` if it is not one of a
/// version this binary reads.
pub fn layout_of(blob: &[u8]) -> Option<(&str, &str)> {
    let text = std::str::from_utf8(blob).ok()?;
    let (first, layout) = text.split_once('\n')?;
    let mut words = first.split(' ');
    let (Some(LAYOUT_WORD), Some(version), Some(bundle), None) =
        (words.next(), words.next(), words.next(), words.next())
    else {
        return None;
    };
    (version.parse::<u32>().ok()? == LAYOUT_VERSION && !bundle.is_empty())
        .then_some((bundle, layout))
}

/// The bundle a layout blob's header names, **whatever its version**: a
/// layout this bateri cannot read is released only when it is its own
/// bundle's — another bundle's holder (the real app's, during its update)
/// is let go untouched even when its version is newer.
fn layout_bundle(blob: &[u8]) -> Option<&str> {
    let end = blob.iter().position(|&byte| byte == b'\n')?;
    let first = std::str::from_utf8(&blob[..end]).ok()?;
    let mut words = first.split(' ');
    match (words.next(), words.next(), words.next(), words.next()) {
        (Some(LAYOUT_WORD), Some(_), Some(bundle), None) if !bundle.is_empty() => Some(bundle),
        _ => None,
    }
}

/// A pane's blob: the platform shell's state for one frozen session
/// (`bt_core::Frozen` minus the master and the tail, which cross as the
/// frame's fd and buffer). Versioned on its own: a new bateri reads this
/// version and the one before it; there is none before it yet.
///
/// ```text
/// "BTPS" | version u32 | cols u16 | rows u16 | flags u32 (bit 0: login)
/// vt | core | input | history: each len u64, bytes
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaneState {
    /// The grid's size the VT is laid out for (`bt_core::Frozen::cols`).
    pub cols: u16,
    pub rows: u16,
    /// Who the PTY's child is: the foreground question needs it.
    pub parent: ShellParent,
    /// `bt_core::Frozen::vt`.
    pub vt: Vec<u8>,
    /// `bt_core::Frozen::blob` — `bt-core`'s own state.
    pub core: Vec<u8>,
    /// `bt_core::Frozen::input`.
    pub input: Vec<u8>,
    /// `Session::final_history` — session restore's scrollback, taken before the
    /// freeze: a pane that falls back gets it from here (in memory, so
    /// `restore_windows = "layout"` keeps its promise).
    pub history: Vec<u8>,
}

const PANE_MAGIC: [u8; 4] = *b"BTPS";

/// The pane blob's version.
pub const PANE_VERSION: u32 = 1;

/// The `login` bit of a pane blob's flags.
const LOGIN: u32 = 1;

impl PaneState {
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(
            32 + self.vt.len() + self.core.len() + self.input.len() + self.history.len(),
        );
        out.extend_from_slice(&PANE_MAGIC);
        out.extend_from_slice(&PANE_VERSION.to_le_bytes());
        out.extend_from_slice(&self.cols.to_le_bytes());
        out.extend_from_slice(&self.rows.to_le_bytes());
        let flags = match self.parent {
            ShellParent::Login => LOGIN,
            ShellParent::Direct => 0,
        };
        out.extend_from_slice(&flags.to_le_bytes());
        for field in [&self.vt, &self.core, &self.input, &self.history] {
            out.extend_from_slice(&(field.len() as u64).to_le_bytes());
            out.extend_from_slice(field);
        }
        out
    }

    /// `None` for anything that is not exactly one blob of
    /// [`PANE_VERSION`]: an unknown flag bit, a field past the end, a byte
    /// past the last field.
    pub fn decode(bytes: &[u8]) -> Option<PaneState> {
        let mut rest = bytes;
        let mut take = |len: usize| -> Option<&[u8]> {
            let (head, tail) = rest.split_at_checked(len)?;
            rest = tail;
            Some(head)
        };
        if take(4)? != PANE_MAGIC {
            return None;
        }
        let version = u32::from_le_bytes(take(4)?.try_into().ok()?);
        if version != PANE_VERSION {
            return None;
        }
        let cols = u16::from_le_bytes(take(2)?.try_into().ok()?);
        let rows = u16::from_le_bytes(take(2)?.try_into().ok()?);
        let parent = match u32::from_le_bytes(take(4)?.try_into().ok()?) {
            LOGIN => ShellParent::Login,
            0 => ShellParent::Direct,
            _ => return None,
        };
        let mut field = || -> Option<Vec<u8>> {
            let len = u64::from_le_bytes(take(8)?.try_into().ok()?);
            Some(take(usize::try_from(len).ok()?)?.to_vec())
        };
        let state = PaneState {
            cols,
            rows,
            parent,
            vt: field()?,
            core: field()?,
            input: field()?,
            history: field()?,
        };
        rest.is_empty().then_some(state)
    }
}

/// The spawned holder before it has the bundle: spawning comes
/// first — in `applicationShouldTerminate:`, so a holder that cannot be
/// born leaves today's quit (its question included) — and the panes are
/// frozen only after.
#[derive(Debug)]
pub struct Spawned {
    pub pid: u32,
    stream: UnixStream,
}

/// Spawns `exe hold --fd 3 --dir …` ([`hold_main`]) with one end of a fresh
/// `socketpair` at fd 3 and nothing else of this process
/// ([`spawn_clean`]). `dirs` are the instance's directories, the first
/// holds the socket — so the first whose socket path fits `sun_path` goes
/// first (a long home's cache directory does not; the short `/tmp` root
/// does), and with none that fits there is no holder.
pub fn spawn_holder(exe: &Path, dirs: &[PathBuf]) -> io::Result<Spawned> {
    let dirs = socket_first(dirs).ok_or(io::ErrorKind::InvalidInput)?;
    let (ours, theirs) = UnixStream::pair()?;
    let mut args: Vec<std::ffi::OsString> = vec!["hold".into(), "--fd".into(), "3".into()];
    for dir in &dirs {
        args.push("--dir".into());
        args.push(dir.as_os_str().to_owned());
    }
    let pid = spawn_clean(exe, &args, Some(std::os::fd::AsFd::as_fd(&theirs)))?;
    drop(theirs);
    Ok(Spawned { pid, stream: ours })
}

/// `dirs` with the first whose [`HANDOVER_SOCKET`] fits `sun_path` moved to
/// the front; `None` if none fits.
fn socket_first(dirs: &[PathBuf]) -> Option<Vec<PathBuf>> {
    socket_first_for(dirs, HANDOVER_SOCKET)
}

/// `dirs` with the first where a socket named `name` fits `sun_path` moved
/// to the front; `None` if none fits. A bound holder asks with its longest
/// name, the largest pid's.
fn socket_first_for(dirs: &[PathBuf], name: &str) -> Option<Vec<PathBuf>> {
    let first = dirs
        .iter()
        .position(|dir| dir.join(name).as_os_str().len() < SUN_PATH)?;
    let mut ordered = vec![dirs[first].clone()];
    ordered.extend(
        dirs.iter()
            .enumerate()
            .filter(|(index, _)| *index != first)
            .map(|(_, dir)| dir.clone()),
    );
    Some(ordered)
}

impl Spawned {
    /// Gives `bundle` to the holder and waits (at most [`HAND_WAIT`]) for
    /// its [`READY`]: `Ok` only then — the directories are the holder's and
    /// it listens. This side's copies of the masters close here either way
    /// (`bundle` is consumed); on an `Err` the caller goes on with today's
    /// quit.
    pub fn give(self, bundle: Bundle) -> io::Result<()> {
        write_frame(&self.stream, &bundle)?;
        drop(bundle);
        let wire = Wire::new(&self.stream, None);
        let mut ready = [0u8];
        wire.read_exact(&mut ready)?;
        if ready[0] == READY {
            Ok(())
        } else {
            Err(io::ErrorKind::InvalidData.into())
        }
    }
}

/// Spawns `program` with `args` (its `argv[0]` is `program`) and returns
/// its pid without waiting: standard I/O on `/dev/null`, `keep` (if any) at
/// fd 3, the signal mask empty and `SIGPIPE` back to its default — and **no
/// other descriptor of this process**, close-on-exec or not. On macOS
/// `posix_spawn` with `POSIX_SPAWN_CLOEXEC_DEFAULT`: a frozen
/// session's master is still open here and a copy in the child would defeat
/// the hang-up. Elsewhere a `fork` + `exec` (the holder closes what it
/// inherited itself, [`detach`]). The environment is this process's.
pub fn spawn_clean(
    program: &Path,
    args: &[std::ffi::OsString],
    keep: Option<std::os::fd::BorrowedFd<'_>>,
) -> io::Result<u32> {
    #[cfg(target_os = "macos")]
    {
        posix_spawn_clean(program, args, keep)
    }
    #[cfg(not(target_os = "macos"))]
    {
        use std::os::unix::process::CommandExt;
        let raw = keep.map(|fd| fd.as_raw_fd());
        let mut command = std::process::Command::new(program);
        command
            .args(args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        // SAFETY: only async-signal-safe calls between `fork` and `exec`.
        unsafe {
            command.pre_exec(move || {
                if let Some(raw) = raw {
                    if raw == 3 {
                        libc::fcntl(3, libc::F_SETFD, 0);
                    } else if libc::dup2(raw, 3) < 0 {
                        return Err(io::Error::last_os_error());
                    }
                }
                Ok(())
            });
        }
        let child = command.spawn()?;
        Ok(child.id())
    }
}

#[cfg(target_os = "macos")]
fn posix_spawn_clean(
    program: &Path,
    args: &[std::ffi::OsString],
    keep: Option<std::os::fd::BorrowedFd<'_>>,
) -> io::Result<u32> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    fn invalid<E>(_: E) -> io::Error {
        io::ErrorKind::InvalidInput.into()
    }
    let path = CString::new(program.as_os_str().as_bytes()).map_err(invalid)?;
    let mut owned = vec![path.clone()];
    for arg in args {
        owned.push(CString::new(arg.as_bytes()).map_err(invalid)?);
    }
    let mut argv: Vec<*mut libc::c_char> =
        owned.iter().map(|arg| arg.as_ptr().cast_mut()).collect();
    argv.push(std::ptr::null_mut());
    // `dup2(3, 3)` would not clear close-on-exec everywhere: a `keep` that
    // already is 3 moves out of the way first (the copy closes on return).
    let moved = match keep {
        Some(fd) if fd.as_raw_fd() == 3 => Some(fd.try_clone_to_owned()?),
        _ => None,
    };
    let source = moved
        .as_ref()
        .map(AsRawFd::as_raw_fd)
        .or_else(|| keep.map(|fd| fd.as_raw_fd()));
    let null = c"/dev/null";
    let mut actions: libc::posix_spawn_file_actions_t = std::ptr::null_mut();
    let mut attr: libc::posix_spawnattr_t = std::ptr::null_mut();
    // SAFETY: the two objects are initialised before use and destroyed on
    // every way out below; every pointer handed in lives for the call
    // (`owned` holds the strings `argv` points into, `null` is static), and
    // `_NSGetEnviron` is the process's own environment.
    unsafe {
        let check = |status: libc::c_int| {
            if status == 0 {
                Ok(())
            } else {
                Err(io::Error::from_raw_os_error(status))
            }
        };
        check(libc::posix_spawn_file_actions_init(&raw mut actions))?;
        if let Err(error) = check(libc::posix_spawnattr_init(&raw mut attr)) {
            libc::posix_spawn_file_actions_destroy(&raw mut actions);
            return Err(error);
        }
        let result = (|| {
            check(libc::posix_spawn_file_actions_addopen(
                &raw mut actions,
                0,
                null.as_ptr(),
                libc::O_RDONLY,
                0,
            ))?;
            for fd in [1, 2] {
                check(libc::posix_spawn_file_actions_addopen(
                    &raw mut actions,
                    fd,
                    null.as_ptr(),
                    libc::O_WRONLY,
                    0,
                ))?;
            }
            if let Some(source) = source {
                check(libc::posix_spawn_file_actions_adddup2(
                    &raw mut actions,
                    source,
                    3,
                ))?;
            }
            let mut empty: libc::sigset_t = 0;
            libc::sigemptyset(&raw mut empty);
            let mut defaults: libc::sigset_t = 0;
            libc::sigemptyset(&raw mut defaults);
            libc::sigaddset(&raw mut defaults, libc::SIGPIPE);
            check(libc::posix_spawnattr_setsigmask(
                &raw mut attr,
                &raw const empty,
            ))?;
            check(libc::posix_spawnattr_setsigdefault(
                &raw mut attr,
                &raw const defaults,
            ))?;
            let flags = libc::POSIX_SPAWN_CLOEXEC_DEFAULT
                | libc::POSIX_SPAWN_SETSIGMASK
                | libc::POSIX_SPAWN_SETSIGDEF;
            check(libc::posix_spawnattr_setflags(
                &raw mut attr,
                libc::c_short::try_from(flags).map_err(invalid)?,
            ))?;
            let mut pid: libc::pid_t = 0;
            check(libc::posix_spawn(
                &raw mut pid,
                path.as_ptr(),
                &raw const actions,
                &raw const attr,
                argv.as_ptr(),
                (*libc::_NSGetEnviron()).cast_const(),
            ))?;
            u32::try_from(pid).map_err(invalid)
        })();
        libc::posix_spawnattr_destroy(&raw mut attr);
        libc::posix_spawn_file_actions_destroy(&raw mut actions);
        result
    }
}

/// What the new bateri took at its sequence point ([`arrive`]): the
/// connections, each holder's layout and every pane — each with the index of
/// the connection it came from.
#[derive(Debug)]
pub struct Arrival {
    links: Vec<Link>,
    /// The newest holder's instance name: the new bateri's ssh
    /// registry takes it as its own, so its masters, its focus listener and
    /// the carried shells' `BATERI_SSH_INSTANCE` stay one directory. The
    /// other holders' directories (an earlier unacknowledged attempt, another
    /// instance that crashed) are adopted too but not used; they go with this
    /// process.
    pub instance: String,
    /// Each connection's holder, by the panes' index: its socket (the kind of
    /// holder, [`is_bound_socket`]) and its own layout — every holder's
    /// windows come back, not only the newest's.
    pub holders: Vec<Holder>,
    pub panes: Vec<(usize, HeldPane)>,
    /// The launches before this one that started taking these programs and
    /// never settled — the most any holder's directory counted
    /// ([`restore::bump_attempt`], [`restore::attempt_mode`]).
    pub attempt: u32,
    /// The directories whose attempt marker this launch counted: cleared once
    /// it settles, or at a clean quit ([`restore::clear_attempt`]).
    pub marked: Vec<PathBuf>,
}

/// One holder an [`Arrival`] took from.
#[derive(Debug)]
pub struct Holder {
    pub socket: PathBuf,
    /// Its layout (session restore's text).
    pub layout: String,
}

impl Arrival {
    /// Releases one pane the new bateri could not place: the holder closes
    /// its copy ([`Link::release`]) and the program is sent `SIGHUP` here —
    /// another process (an ssh master spawned before the master was
    /// close-on-exec) may hold a copy, and an older holder does not signal.
    pub fn release(&mut self, link: usize, pane: HeldPane) {
        let (pid, start) = (pane.pid, pane.start);
        if let Some(link) = self.links.get_mut(link) {
            let _ = link.release(pane);
        } else {
            drop(pane);
        }
        hang_up(pid, start, None);
    }

    /// Every pane is placed: what is left is released ([`Arrival::release`]),
    /// then every holder is acknowledged and exits ([`Link::ack`]).
    pub fn finish(mut self) {
        for (link, pane) in std::mem::take(&mut self.panes) {
            self.release(link, pane);
        }
        for link in self.links {
            let _ = link.ack();
        }
    }

    /// Nothing could be placed: every holder hangs every pane up and exits,
    /// and each program is sent `SIGHUP` here too ([`Arrival::release`]'s
    /// reason).
    pub fn release_all(self) {
        let programs: Vec<(u32, u64)> = self
            .panes
            .iter()
            .map(|(_, pane)| (pane.pid, pane.start))
            .collect();
        drop(self.panes);
        for link in self.links {
            let _ = link.release_all(Bundle::default());
        }
        for (pid, start) in programs {
            hang_up(pid, start, None);
        }
    }
}

/// The holders listening in `roots`' instance directories — every holder
/// socket of every instance — with the instance's name, newest socket first.
fn holders(roots: &[PathBuf]) -> Vec<(PathBuf, String)> {
    let mut names: Vec<String> = Vec::new();
    for root in roots {
        let Ok(entries) = std::fs::read_dir(root) else {
            continue;
        };
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str()
                && !names.iter().any(|seen| seen == name)
            {
                names.push(name.to_owned());
            }
        }
    }
    let mut found: Vec<(PathBuf, String, std::time::SystemTime)> = Vec::new();
    for name in names {
        for dir in ssh_route::instance_dirs(roots, &name) {
            for (socket, modified) in holder_sockets(&dir) {
                found.push((socket, name.clone(), modified));
            }
        }
    }
    found.sort_by(|a, b| b.2.cmp(&a.2));
    found
        .into_iter()
        .map(|(socket, name, _)| (socket, name))
        .collect()
}

/// **The new bateri's sequence point**: takes every holder's
/// bundle in `roots`' instance directories and adopts their instance
/// directories (`me` from each holder, [`ssh_route::adopt_instance`]).
///
/// **It must run before this process spawns any child, on any thread** —
/// one caller, at the top of the platform shell's `run`, before the
/// application delegate (whose ssh registry sweeps on a thread of its own
/// and runs `ssh`). macOS' `recvmsg` has no `MSG_CMSG_CLOEXEC`: a received
/// master is close-on-exec only after it arrived, and a child spawned in
/// between keeps a copy that defeats the hang-up. It must also run before
/// the sweep for the directories' sake: a dead owner's directory would be
/// swept with its live sockets.
///
/// Only a holder of `bundle_id` is taken: another bundle's (a dev package
/// next to the real one) is let go after its layout, before its panes, and
/// without an acknowledgement — it waits for its own bateri, its directories
/// untouched. A bound holder (its bateri lives) closes without a byte and is
/// passed over in silence. A holder whose layout cannot be read is released
/// whole, and so is one of this bundle whose frame breaks after the layout
/// (a passing failure — a timeout, a full descriptor table — is left for
/// the next launch). The same program carried by two holders — the one a
/// bateri took from, and that bateri's own holder when it crashed before its
/// acknowledgement — comes once, from the **older** holder: the younger copy
/// can only be an unconfirmed registration, and the older one drained the
/// output; the younger is released without a signal. `None` if nothing was
/// taken.
///
/// Every holder of this bundle that gives a frame counts one attempt in its
/// directory's marker **before its panes are read** — a launch that crashes
/// reading or replaying them counts too — once per directory per launch
/// ([`Arrival::attempt`]). A bound holder (it declines), another bundle's and
/// a socket nobody listens on count nothing: a second instance or a dev
/// package must not push the real one's programs toward being given up. A
/// launch that took nothing in the end clears what it counted — no restore
/// of it can crash.
///
/// Without `bound` the bound holders are not asked at all — a launch with ⇧
/// held leaves their programs waiting for the next one; the update's holder
/// is taken still, since it cannot wait ([`HOLD_LIMIT`]).
pub fn arrive(
    roots: &[PathBuf],
    uid: u32,
    me: u32,
    bundle_id: &str,
    bound: bool,
) -> Option<Arrival> {
    let mut arrival: Option<Arrival> = None;
    let mut attempt = 0;
    let mut marked: Vec<PathBuf> = Vec::new();
    for (socket, instance) in holders(roots) {
        if !bound && is_bound_socket(&socket) {
            continue;
        }
        let opened = match open(&socket, uid) {
            Ok(opened) => opened,
            Err(HandoverError::Declined) => continue,
            // A holder that is gone (`kill -9` leaves its socket behind):
            // nothing to say about it.
            Err(HandoverError::Io(error))
                if matches!(
                    error.kind(),
                    io::ErrorKind::ConnectionRefused | io::ErrorKind::NotFound
                ) =>
            {
                forget_dead_socket(&socket);
                continue;
            }
            Err(error) => {
                eprintln!(
                    "bateri: the holder on {} failed: {error:?}",
                    socket.display()
                );
                continue;
            }
        };
        let layout = match layout_of(&opened.layout) {
            // Another bundle's: dropped before the panes; the holder keeps them.
            Some((owner, _)) if owner != bundle_id => continue,
            Some((_, layout)) => Some(layout.to_owned()),
            // Another bundle's, in a version this one cannot read: untouched.
            None if layout_bundle(&opened.layout).is_some_and(|owner| owner != bundle_id) => {
                continue;
            }
            None => None,
        };
        if layout_bundle(&opened.layout) == Some(bundle_id)
            && let Some(dir) = socket.parent()
            && !marked.iter().any(|seen| seen == dir)
        {
            attempt = attempt.max(restore::bump_attempt(dir));
            marked.push(dir.to_owned());
        }
        let (bundle, link) = match opened.body() {
            Ok(taken) => taken,
            Err((HandoverError::Malformed(why), link)) => {
                eprintln!(
                    "bateri: the holder on {} sent a broken frame ({why}); its programs end",
                    socket.display()
                );
                let _ = link.release_all(Bundle::default());
                continue;
            }
            Err((error, _)) => {
                eprintln!(
                    "bateri: the holder on {} failed: {error:?}",
                    socket.display()
                );
                continue;
            }
        };
        let Some(layout) = layout else {
            let programs: Vec<(u32, u64)> = bundle
                .panes
                .iter()
                .map(|pane| (pane.pid, pane.start))
                .collect();
            let _ = link.release_all(bundle);
            for (pid, start) in programs {
                hang_up(pid, start, None);
            }
            continue;
        };
        for instance_dir in ssh_route::instance_dirs(roots, &instance) {
            if let Err(error) =
                ssh_route::adopt_instance(&instance_dir, Some(link.holder_pid()), me)
            {
                eprintln!(
                    "bateri: could not adopt {}: {error}",
                    instance_dir.display()
                );
            }
        }
        let arrival = arrival.get_or_insert_with(|| Arrival {
            links: Vec::new(),
            instance: instance.clone(),
            holders: Vec::new(),
            panes: Vec::new(),
            attempt: 0,
            marked: Vec::new(),
        });
        let index = arrival.links.len();
        arrival.links.push(link);
        arrival.holders.push(Holder { socket, layout });
        arrival
            .panes
            .extend(bundle.panes.into_iter().map(|pane| (index, pane)));
    }
    match &mut arrival {
        Some(arrival) => {
            drop_duplicates(arrival);
            arrival.attempt = attempt;
            arrival.marked = marked;
        }
        None => {
            for dir in &marked {
                restore::clear_attempt(dir);
            }
        }
    }
    arrival
}

/// Removes a bound holder's socket that nothing listens on, once the pid its
/// name carries is gone — a holder between its `bind` and its `listen`
/// refuses a connection too, and its pid is alive. The update's
/// [`HANDOVER_SOCKET`] is left to the next holder's bind, which removes a
/// dead one.
fn forget_dead_socket(socket: &Path) {
    let Some(pid) = socket
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| name.strip_prefix(BOUND_SOCKET_PREFIX))
        .filter(|_| is_bound_socket(socket))
        .and_then(|pid| pid.parse::<u32>().ok())
    else {
        return;
    };
    // The directory is this user's: a pid that is gone, or another user's,
    // is not the holder that bound here.
    if !ssh_route::alive(pid) {
        let _ = std::fs::remove_file(socket);
    }
}

/// Keeps one copy of each program — `(tab, pid, start)` — from the oldest
/// holder (the largest link index: the holders came newest first); the others
/// are released without a signal ([`Link::release`]).
fn drop_duplicates(arrival: &mut Arrival) {
    let mut kept: Vec<(usize, HeldPane)> = Vec::new();
    for (link, pane) in std::mem::take(&mut arrival.panes) {
        let same = kept.iter().position(|(_, other)| {
            other.tab == pane.tab && other.pid == pane.pid && other.start == pane.start
        });
        let loser = match same {
            None => {
                kept.push((link, pane));
                continue;
            }
            Some(index) if kept[index].0 < link => {
                std::mem::replace(&mut kept[index], (link, pane))
            }
            Some(_) => (link, pane),
        };
        if let Some(holder) = arrival.links.get_mut(loser.0) {
            let _ = holder.release(loser.1);
        }
    }
    arrival.panes = kept;
}

// ─── the bound connection (bateri's end) ─────────────────────────────────
//
// A bound holder is spawned when bateri starts ([`spawn_bound`]) and stays
// connected; [`Bound`] is bateri's end. The messages are an internal detail
// of one build — the handshake carries [`build_id`] and the holder refuses
// another's — so they carry no version of their own beyond it:
//
// ```text
// handshake    "BTHB" | version u32 | build id: len u32, bytes
//              layout: len u64, bytes (the first; a crash bundle always has one)
//              → READY, or REFUSED (another build, or no socket)
// bateri → holder, a tag byte, then:
//   ADD        'F' + master | tab | pid u32 | start u64
//              flags u32 (bit 0: login is the child, bit 1: unconfirmed)
//              taken from: len u32, socket path | blob: len u64, bytes
//              → PANE_REFUSED tab, if the holder cannot keep it
//   DROP tab | CONFIRM tab | LAYOUT len u64, bytes
//   STATE      tab | blob: len u64, bytes
//   PING → PONG | QUIT
//   HANDOVER   then a frame, byte for byte → READY or REFUSED
// ```
//
// A tab is its length as `u32` and its text, as in the frame.

/// The bound handshake's first bytes.
const BOUND_MAGIC: [u8; 4] = *b"BTHB";

/// The bound connection's own version, beside the build's identity.
const BOUND_VERSION: u32 = 1;

/// The sanity limits of a build id and of a socket path in a message.
const ID_LIMIT: u64 = 256;
const PATH_LIMIT: u64 = 1024;

const MSG_ADD: u8 = b'a';
const MSG_DROP: u8 = b'd';
const MSG_CONFIRM: u8 = b'c';
const MSG_LAYOUT: u8 = b'l';
const MSG_STATE: u8 = b's';
const MSG_PING: u8 = b'p';
const MSG_QUIT: u8 = b'q';
const MSG_HANDOVER: u8 = b'h';

/// Holder → bateri: it refuses the handshake or a handover.
const REFUSED: u8 = b'N';
/// Holder → bateri: it cannot keep a pane (tab follows).
const PANE_REFUSED: u8 = b'n';
/// Holder → bateri: the answer to a ping.
const PONG: u8 = b'P';

/// An added pane's flags.
const ADD_LOGIN: u32 = 1;
const ADD_UNCONFIRMED: u32 = 2;

/// This build's identity: the workspace version and the running image's own
/// — the Mach-O `LC_UUID` on macOS (new at every link), the executable's
/// device, inode, size and time on Linux. A bound holder and its spawner must
/// be the same build: the path bateri spawns the holder from may hold a newer
/// binary by then (an install over a running bateri).
pub fn build_id() -> String {
    format!("{} {}", env!("CARGO_PKG_VERSION"), image_identity())
}

#[cfg(target_os = "macos")]
unsafe extern "C" {
    /// dyld's header of loaded image `index`; image 0 is the main executable.
    fn _dyld_get_image_header(index: u32) -> *const u8;
}

/// The main executable's `LC_UUID` as hex; empty if it has none.
#[cfg(target_os = "macos")]
fn image_identity() -> String {
    // A 64-bit Mach-O header: `magic` at 0, `ncmds` at 16, `sizeofcmds` at
    // 20, the load commands from 32; each command `cmd` at 0 and `cmdsize`
    // at 4, `LC_UUID`'s 16 bytes at 8.
    const MH_MAGIC_64: u32 = 0xfeed_facf;
    const HEADER: usize = 32;
    const LC_UUID: u32 = 0x1b;
    // SAFETY: dyld's header of the main executable is mapped for the life of
    // the process, followed by its `sizeofcmds` bytes of load commands; every
    // read below stays inside them and is unaligned.
    unsafe {
        let header = _dyld_get_image_header(0);
        if header.is_null() {
            return String::new();
        }
        let word = |at: *const u8| std::ptr::read_unaligned(at.cast::<u32>());
        if word(header) != MH_MAGIC_64 {
            return String::new();
        }
        let count = word(header.add(16));
        let commands = word(header.add(20)) as usize;
        let mut offset = 0usize;
        for _ in 0..count {
            if offset + 8 > commands {
                break;
            }
            let command = header.add(HEADER + offset);
            let size = word(command.add(4)) as usize;
            if size < 8 || offset + size > commands {
                break;
            }
            if word(command) == LC_UUID && size >= 8 + 16 {
                let uuid = std::ptr::read_unaligned(command.add(8).cast::<[u8; 16]>());
                return uuid.iter().map(|byte| format!("{byte:02x}")).collect();
            }
            offset += size;
        }
    }
    String::new()
}

/// The running executable's file — `/proc/self/exe` names it even after
/// the path was replaced.
#[cfg(target_os = "linux")]
fn image_identity() -> String {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata("/proc/self/exe")
        .map(|meta| {
            format!(
                "{:x}-{:x}-{:x}-{}.{:09}",
                meta.dev(),
                meta.ino(),
                meta.size(),
                meta.mtime(),
                meta.mtime_nsec()
            )
        })
        .unwrap_or_default()
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn image_identity() -> String {
    String::new()
}

/// A pane as bateri registers it with its bound holder ([`Bound::add`]).
#[derive(Debug)]
pub struct BoundPane {
    pub tab: TabId,
    /// The PTY's child and its start time (`jobs::start_time`).
    pub pid: u32,
    pub start: u64,
    pub parent: ShellParent,
    /// `bt-core`'s state blob now: a crash right after the registration
    /// still leaves a bundle to give.
    pub blob: Vec<u8>,
    /// A copy of the PTY master; sent and closed here.
    pub master: OwnedFd,
    /// The socket of the holder this pane was taken from, until that holder
    /// is acknowledged and the pane confirmed ([`Bound::confirm`]): while the
    /// other holder listens it drains the master, and two drains would split
    /// the output between them. `None` for a pane this bateri spawned.
    pub taken_from: Option<PathBuf>,
}

/// bateri's end of a bound holder ([`spawn_bound`]). Every send queues and
/// returns at once: a thread of its own writes, and the layout and each
/// pane's state keep only their newest unsent value. Dropping it closes the
/// connection without a goodbye — to the holder that is a crash, and it
/// holds; [`Bound::quit`] and [`Bound::hand_over`] are the goodbyes.
pub struct Bound {
    pid: u32,
    stream: UnixStream,
    shared: Arc<Shared>,
}

impl std::fmt::Debug for Bound {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Bound").field("pid", &self.pid).finish()
    }
}

/// What the two threads and the handle share.
struct Shared {
    outbox: Mutex<Outbox>,
    wake: Condvar,
    news: Mutex<News>,
    heard: Condvar,
}

/// What waits to be written.
#[derive(Default)]
struct Outbox {
    /// In order.
    orders: VecDeque<Order>,
    /// The newest unsent layout and per-pane states.
    layout: Option<Vec<u8>>,
    states: Vec<(TabId, Vec<u8>)>,
    /// Nothing more is queued: the writer flushes and ends.
    closed: bool,
}

impl Outbox {
    fn is_empty(&self) -> bool {
        self.orders.is_empty() && self.layout.is_none() && self.states.is_empty()
    }
}

enum Order {
    Add(BoundPane),
    Drop(TabId),
    Confirm(TabId),
    Ping,
    Quit,
    Handover(Bundle),
}

/// What was heard from the holder.
#[derive(Default)]
struct News {
    /// The connection ended.
    dead: bool,
    /// Why this side lets go, once it does.
    leaving: Option<Leaving>,
    pongs: u64,
    /// Messages written so far: a goodbye waits while they move.
    progress: u64,
    /// The last batch (behind a goodbye) is written.
    flushed: bool,
    /// The holder's answer to the handover.
    answer: Option<bool>,
    refused: Vec<TabId>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Leaving {
    Quit,
    Handover,
    Dropped,
}

/// A lock that a panicked holder of it does not poison for the others.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Spawns a bound holder: `exe hold --fd 3 --dir …` ([`hold_main`], the same
/// argv as the update's, [`spawn_clean`]), then the handshake. `dirs` are this
/// instance's directories, the socket goes to the first where it fits
/// ([`socket_first_for`]). `layout` is the first layout ([`layout_blob`]): a
/// crash bundle always names its bundle, or the next bateri could not tell
/// whose programs they are. `on_death` runs once, on the handle's reader
/// thread, if the holder goes away while this side did not let go — the
/// spawner then spawns another and registers everything again; it must not
/// wait for a thread that drops the handle. `Err` if no holder runs (none
/// spawned, another build, no socket): the panes are not protected.
pub fn spawn_bound(
    exe: &Path,
    dirs: &[PathBuf],
    layout: Vec<u8>,
    on_death: impl FnOnce() + Send + 'static,
) -> io::Result<Bound> {
    let dirs =
        socket_first_for(dirs, &bound_socket(u32::MAX)).ok_or(io::ErrorKind::InvalidInput)?;
    let (ours, theirs) = UnixStream::pair()?;
    let mut args: Vec<std::ffi::OsString> = vec!["hold".into(), "--fd".into(), "3".into()];
    for dir in &dirs {
        args.push("--dir".into());
        args.push(dir.as_os_str().to_owned());
    }
    let pid = spawn_clean(exe, &args, Some(theirs.as_fd()))?;
    drop(theirs);
    Bound::connect(ours, pid, true, layout, on_death).inspect_err(|_| reap_later(pid))
}

/// Waits for a child of ours on a thread of its own: a zombie would stay
/// until bateri does, and the wait may be long (a holder that went detached
/// lives on).
fn reap_later(pid: u32) {
    let _ = std::thread::Builder::new()
        .name("bateri-holder-reap".to_owned())
        .spawn(move || reap(pid));
}

fn reap(pid: u32) {
    let Ok(pid) = libc::pid_t::try_from(pid) else {
        return;
    };
    let mut status = 0;
    // SAFETY: a wait for our own child; `status` belongs to this frame.
    while unsafe { libc::waitpid(pid, &raw mut status, 0) } < 0
        && io::Error::last_os_error().kind() == io::ErrorKind::Interrupted
    {}
}

impl Bound {
    /// The handshake on `stream`, then the two threads. `reap`: the holder is
    /// this process's child (not in the tests, whose harness owns it).
    fn connect(
        stream: UnixStream,
        pid: u32,
        reap: bool,
        layout: Vec<u8>,
        on_death: impl FnOnce() + Send + 'static,
    ) -> io::Result<Bound> {
        let wire = Wire::new(&stream, None);
        let id = build_id();
        wire.write_all(&BOUND_MAGIC)?;
        wire.write_all(&BOUND_VERSION.to_le_bytes())?;
        let len = u32::try_from(id.len()).map_err(|_| io::ErrorKind::InvalidInput)?;
        wire.write_all(&len.to_le_bytes())?;
        wire.write_all(id.as_bytes())?;
        write_field(&wire, &layout)?;
        let mut answer = [0u8];
        wire.read_exact(&mut answer)?;
        match answer[0] {
            READY => {}
            REFUSED => {
                return Err(io::Error::other(
                    "the holder refused: another build, or no socket",
                ));
            }
            _ => return Err(io::ErrorKind::InvalidData.into()),
        }
        let shared = Arc::new(Shared {
            outbox: Mutex::new(Outbox::default()),
            wake: Condvar::new(),
            news: Mutex::new(News::default()),
            heard: Condvar::new(),
        });
        {
            let stream = stream.try_clone()?;
            let shared = Arc::clone(&shared);
            std::thread::Builder::new()
                .name("bateri-holder-writer".to_owned())
                .spawn(move || write_loop(&stream, &shared))?;
        }
        let bound = Bound {
            pid,
            stream,
            shared,
        };
        {
            let stream = bound.stream.try_clone()?;
            let shared = Arc::clone(&bound.shared);
            std::thread::Builder::new()
                .name("bateri-holder-reader".to_owned())
                .spawn(move || read_loop(&stream, &shared, pid, reap, on_death))?;
        }
        Ok(bound)
    }

    /// The holder's pid.
    pub fn pid(&self) -> u32 {
        self.pid
    }

    fn order(&self, order: Order) {
        let mut outbox = lock(&self.shared.outbox);
        if outbox.closed {
            return;
        }
        outbox.orders.push_back(order);
        drop(outbox);
        self.shared.wake.notify_all();
    }

    /// Registers a pane (its master copy closes here once sent). The holder
    /// refuses one it cannot keep — over [`PANE_CAP`], or a program already
    /// gone — and that is written to stderr ([`Bound::refused`]).
    pub fn add(&self, pane: BoundPane) {
        self.order(Order::Add(pane));
    }

    /// Releases a pane: the holder closes its copy and signals nobody (the
    /// pane's own close does what closing means).
    pub fn release(&self, tab: &TabId) {
        lock(&self.shared.outbox)
            .states
            .retain(|(pending, _)| pending != tab);
        self.order(Order::Drop(tab.clone()));
    }

    /// The holder a pane was taken from is acknowledged: the pane is this
    /// holder's to drain if bateri goes.
    pub fn confirm(&self, tab: &TabId) {
        self.order(Order::Confirm(tab.clone()));
    }

    /// The window layout (opaque, the frame's layout field); only the newest
    /// unsent one is written.
    pub fn layout(&self, layout: Vec<u8>) {
        let mut outbox = lock(&self.shared.outbox);
        if outbox.closed {
            return;
        }
        outbox.layout = Some(layout);
        drop(outbox);
        self.shared.wake.notify_all();
    }

    /// A pane's newer `bt-core` state blob; only the newest unsent one per
    /// pane is written.
    pub fn state(&self, tab: &TabId, blob: Vec<u8>) {
        let mut outbox = lock(&self.shared.outbox);
        if outbox.closed {
            return;
        }
        match outbox.states.iter_mut().find(|(pending, _)| pending == tab) {
            Some(pending) => pending.1 = blob,
            None => outbox.states.push((tab.clone(), blob)),
        }
        drop(outbox);
        self.shared.wake.notify_all();
    }

    /// Whether the connection still stands (the holder neither exited nor
    /// broke it).
    pub fn alive(&self) -> bool {
        !lock(&self.shared.news).dead
    }

    /// Whether the holder answers within `wait`: alive **and** serving, not
    /// merely connected.
    pub fn ping(&self, wait: Duration) -> bool {
        let before = lock(&self.shared.news).pongs;
        self.order(Order::Ping);
        let deadline = Instant::now() + wait;
        let mut news = lock(&self.shared.news);
        loop {
            if news.pongs > before {
                return true;
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if news.dead || remaining.is_zero() {
                return false;
            }
            news = self
                .shared
                .heard
                .wait_timeout(news, remaining)
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .0;
        }
    }

    /// The panes the holder refused since the last call.
    pub fn refused(&self) -> Vec<TabId> {
        std::mem::take(&mut lock(&self.shared.news).refused)
    }

    /// The quiet exit: what is queued is written, then the holder closes its
    /// copies without touching a program and exits. Waits at most
    /// [`HAND_WAIT`] for it to go.
    pub fn quit(self) {
        lock(&self.shared.news).leaving = Some(Leaving::Quit);
        self.close_with(Order::Quit);
        self.until(|news| news.dead);
    }

    /// The deliberate handover (quit, update): what is queued is written,
    /// then `bundle` as a frame over this connection; `Ok` once the holder
    /// says [`READY`] — it owns the instance directories and holds the
    /// bundle, detached, for the next bateri. Its registrations end: a pane
    /// the bundle does not carry is bateri's to close. This side's copies of
    /// the masters close either way (`bundle` is consumed).
    pub fn hand_over(self, bundle: Bundle) -> io::Result<()> {
        lock(&self.shared.news).leaving = Some(Leaving::Handover);
        self.close_with(Order::Handover(bundle));
        let answer = self.until(|news| news.answer.is_some() || news.dead);
        if answer.and_then(|news| news.answer) == Some(true) {
            Ok(())
        } else {
            Err(io::Error::other("the holder did not take the handover"))
        }
    }

    /// Queues the last order and closes the outbox behind it.
    fn close_with(&self, order: Order) {
        let mut outbox = lock(&self.shared.outbox);
        if !outbox.closed {
            outbox.orders.push_back(order);
            outbox.closed = true;
        }
        drop(outbox);
        self.shared.wake.notify_all();
    }

    /// Waits until `done` holds: while the writer moves (each message it
    /// writes gives [`HAND_WAIT`] more — a holder that stopped reading must not
    /// hang a quit behind an earlier batch), then at most [`HAND_WAIT`] for the
    /// holder's answer once the last batch is out. The news, if `done` came.
    fn until(&self, done: impl Fn(&News) -> bool) -> Option<MutexGuard<'_, News>> {
        let mut news = lock(&self.shared.news);
        let mut seen = news.progress;
        let mut flushed = news.flushed;
        let mut deadline = Instant::now() + HAND_WAIT;
        loop {
            if done(&news) {
                return Some(news);
            }
            if news.progress != seen || news.flushed != flushed {
                seen = news.progress;
                flushed = news.flushed;
                deadline = Instant::now() + HAND_WAIT;
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return None;
            }
            news = self
                .shared
                .heard
                .wait_timeout(news, remaining)
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .0;
        }
    }
}

impl Drop for Bound {
    /// Ends both threads without waiting for them: the reader may be the
    /// thread dropping the handle (from `on_death`), or `on_death` may be
    /// waiting for this thread.
    fn drop(&mut self) {
        lock(&self.shared.news)
            .leaving
            .get_or_insert(Leaving::Dropped);
        lock(&self.shared.outbox).closed = true;
        self.shared.wake.notify_all();
        // To the holder, a connection that ends without a goodbye.
        let _ = self.stream.shutdown(std::net::Shutdown::Both);
    }
}

/// The writer thread: everything queued ([`write_batch`]'s order) until the
/// outbox is closed and empty or a write fails. Its writes wait for the holder
/// without a bound — except the last batch, behind a goodbye, whose every
/// write is bounded: a quit must not hang on a holder that stopped reading.
fn write_loop(stream: &UnixStream, shared: &Shared) {
    loop {
        let (orders, layout, states, closing) = {
            let mut outbox = lock(&shared.outbox);
            while outbox.is_empty() && !outbox.closed {
                outbox = shared
                    .wake
                    .wait(outbox)
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
            }
            if outbox.is_empty() {
                return;
            }
            (
                std::mem::take(&mut outbox.orders),
                outbox.layout.take(),
                std::mem::take(&mut outbox.states),
                outbox.closed,
            )
        };
        let wire = if closing {
            Wire::new(stream, None)
        } else {
            Wire::patient(stream)
        };
        if write_batch(&wire, shared, orders, layout, states).is_err() {
            lock(&shared.news).dead = true;
            shared.heard.notify_all();
            return;
        }
        if closing {
            lock(&shared.news).flushed = true;
            shared.heard.notify_all();
        }
    }
}

/// One batch: the registrations in order, then the newest layout and
/// states, then a ping or a goodbye — so a ping answered means everything
/// queued before it arrived, and a goodbye follows everything.
fn write_batch(
    wire: &Wire<'_>,
    shared: &Shared,
    orders: VecDeque<Order>,
    layout: Option<Vec<u8>>,
    states: Vec<(TabId, Vec<u8>)>,
) -> io::Result<()> {
    let moved = || {
        lock(&shared.news).progress += 1;
        shared.heard.notify_all();
    };
    let (last, first): (Vec<Order>, Vec<Order>) = orders
        .into_iter()
        .partition(|order| matches!(order, Order::Ping | Order::Quit | Order::Handover(_)));
    for order in first {
        write_order(wire, order)?;
        moved();
    }
    if let Some(layout) = layout {
        wire.write_all(&[MSG_LAYOUT])?;
        write_field(wire, &layout)?;
        moved();
    }
    for (tab, blob) in states {
        wire.write_all(&[MSG_STATE])?;
        write_tab(wire, tab.as_str())?;
        write_field(wire, &blob)?;
        moved();
    }
    for order in last {
        write_order(wire, order)?;
        moved();
    }
    Ok(())
}

fn write_order(wire: &Wire<'_>, order: Order) -> io::Result<()> {
    match order {
        Order::Add(pane) => {
            wire.write_all(&[MSG_ADD])?;
            wire.arm(true)?;
            send_fd(wire.stream, pane.master.as_raw_fd())?;
            write_tab(wire, pane.tab.as_str())?;
            wire.write_all(&pane.pid.to_le_bytes())?;
            wire.write_all(&pane.start.to_le_bytes())?;
            let mut flags = 0;
            if pane.parent == ShellParent::Login {
                flags |= ADD_LOGIN;
            }
            let from = pane
                .taken_from
                .as_ref()
                .map(|path| {
                    use std::os::unix::ffi::OsStrExt;
                    path.as_os_str().as_bytes().to_vec()
                })
                .unwrap_or_default();
            if !from.is_empty() {
                flags |= ADD_UNCONFIRMED;
            }
            wire.write_all(&flags.to_le_bytes())?;
            let len = u32::try_from(from.len()).map_err(|_| io::ErrorKind::InvalidInput)?;
            wire.write_all(&len.to_le_bytes())?;
            wire.write_all(&from)?;
            write_field(wire, &pane.blob)?;
        }
        Order::Drop(tab) => {
            wire.write_all(&[MSG_DROP])?;
            write_tab(wire, tab.as_str())?;
        }
        Order::Confirm(tab) => {
            wire.write_all(&[MSG_CONFIRM])?;
            write_tab(wire, tab.as_str())?;
        }
        Order::Ping => wire.write_all(&[MSG_PING])?,
        Order::Quit => wire.write_all(&[MSG_QUIT])?,
        Order::Handover(bundle) => {
            wire.write_all(&[MSG_HANDOVER])?;
            write_bundle(wire, &bundle)?;
        }
    }
    Ok(())
}

/// The reader thread: what the holder says, until the connection ends. A
/// holder that went away while this side did not let go is reaped and
/// reported (`on_death`).
fn read_loop(
    stream: &UnixStream,
    shared: &Shared,
    pid: u32,
    reap_child: bool,
    on_death: impl FnOnce(),
) {
    let wire = Wire::patient(stream);
    loop {
        let mut tag = [0u8];
        if wire.read_exact(&mut tag).is_err() {
            break;
        }
        let mut total = 0u64;
        let heard = match tag[0] {
            READY | REFUSED => {
                lock(&shared.news).answer = Some(tag[0] == READY);
                true
            }
            PONG => {
                lock(&shared.news).pongs += 1;
                true
            }
            PANE_REFUSED => match read_tab(&wire, &mut total) {
                Ok(tab) => {
                    eprintln!(
                        "bateri: the holder cannot keep pane {}; its programs end with bateri",
                        tab.as_str()
                    );
                    lock(&shared.news).refused.push(tab);
                    true
                }
                Err(_) => false,
            },
            _ => false,
        };
        if !heard {
            break;
        }
        shared.heard.notify_all();
    }
    let leaving = {
        let mut news = lock(&shared.news);
        news.dead = true;
        news.leaving
    };
    shared.heard.notify_all();
    // Reaped whatever the end — a holder that went detached (handed over,
    // or the handle dropped) exits some day too — on a thread of its own,
    // which may wait as long as it lives.
    if reap_child {
        reap_later(pid);
    }
    if leaving.is_none() {
        on_death();
    }
}

// ─── the holder ──────────────────────────────────────────────────────────

/// What the holder needs besides its spawner's stream.
#[derive(Clone, Debug)]
pub struct HoldConfig {
    /// The instance's directories, one per socket root; the first holds the
    /// socket. All are taken over from `parent` once the holder holds.
    pub dirs: Vec<PathBuf>,
    /// The spawner (bateri): the owner the directories are taken from.
    pub parent: u32,
    /// The only uid a client may have.
    pub uid: u32,
    /// The tests' bound on a holder's life; `None` in production. The
    /// update's holder holds at most [`HOLD_LIMIT`] from its birth (or this,
    /// if shorter); a bound holder that went detached holds while a program
    /// is left (or at most this long after it went detached).
    pub limit: Option<Duration>,
    /// [`BUFFER_LIMIT`], smaller in the tests.
    pub buffer_limit: usize,
}

/// How the holder ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HoldEnd {
    /// A client took everything and acknowledged.
    Acked,
    /// A client released everything, or nothing was left to hold.
    Released,
    /// The limit passed: every copy closed, every program signalled.
    Limit,
    /// Its spawner said goodbye: the copies closed, no program touched.
    Quit,
    /// It never held: no bundle, another build, no socket or no directory —
    /// or a handover failed and its spawner keeps the programs.
    Failed,
}

/// One pane in a holding holder.
struct Held {
    tab: String,
    pid: u32,
    start: u64,
    ended: bool,
    cut: bool,
    blob: Vec<u8>,
    buffer: VecDeque<u8>,
    /// `None` once released.
    master: Option<OwnedFd>,
    /// The program's exit watch — a detached holder's; `None` once it fired.
    exit: Option<OwnedFd>,
    /// The socket of the holder that still drains this pane — an unconfirmed
    /// registration's, while that holder listens.
    beside: Option<PathBuf>,
}

impl Held {
    fn from_frame(pane: HeldPane, exit: Option<OwnedFd>) -> Held {
        Held {
            tab: pane.tab.as_str().to_owned(),
            pid: pane.pid,
            start: pane.start,
            ended: pane.ended,
            cut: pane.cut,
            blob: pane.blob,
            buffer: pane.buffer.into(),
            master: Some(pane.master),
            exit,
            beside: None,
        }
    }
}

/// How a holding holder reads its masters.
#[derive(Clone, Copy, Debug)]
enum Drain {
    /// Up to this many bytes, then not at all: the program blocks, nothing
    /// is lost — the update's holder, for the seconds an update takes.
    Stop(usize),
    /// Always: past this many bytes the oldest go and the pane is cut — a
    /// detached holder, for as long as bateri stays closed.
    Ring(usize),
}

/// A pane registered with a bound holder.
struct Registered {
    tab: TabId,
    pid: u32,
    start: u64,
    parent: ShellParent,
    /// `bt-core`'s newest state blob.
    blob: Vec<u8>,
    master: OwnedFd,
    exit: OwnedFd,
    taken_from: Option<PathBuf>,
}

/// What a bound holder was sent.
struct Registry {
    panes: Vec<Registered>,
    layout: Vec<u8>,
}

/// The listening socket; its file goes with it on every way out.
struct Listening {
    listener: UnixListener,
    path: PathBuf,
}

impl Drop for Listening {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// The holder's body (the process setup is [`hold_main`]'s). The spawner's
/// first bytes decide: a frame is the update's bundle ([`HOLD_LIMIT`] from
/// birth), the bound handshake starts a bound holder. Every way out drops the
/// remaining copies; the ones meaning "nobody took these programs" signal
/// them too. A failure after the directories were taken gives them back to
/// the spawner, which falls back and lives on.
pub fn hold(spawner: UnixStream, config: &HoldConfig) -> HoldEnd {
    let deadline = Instant::now()
        + config
            .limit
            .map_or(HOLD_LIMIT, |limit| limit.min(HOLD_LIMIT));
    let wire = Wire::new(&spawner, Some(deadline));
    let mut magic = [0u8; 4];
    if wire.read_exact(&mut magic).is_err() {
        return HoldEnd::Failed;
    }
    match magic {
        MAGIC => hold_for_update(spawner, config, deadline),
        BOUND_MAGIC => hold_bound(spawner, config),
        _ => HoldEnd::Failed,
    }
}

/// The update's holder: takes the bundle, listens on [`HANDOVER_SOCKET`],
/// owns the directories, says [`READY`], then drains and serves until an
/// acknowledgement, a release or `deadline`.
fn hold_for_update(spawner: UnixStream, config: &HoldConfig, deadline: Instant) -> HoldEnd {
    let wire = Wire::new(&spawner, Some(deadline));
    let bundle = match read_version(&wire).and_then(|()| read_body(&wire)) {
        Ok(bundle) => bundle,
        Err(_) => return HoldEnd::Failed,
    };
    let layout = bundle.layout;
    let held: Vec<Held> = bundle
        .panes
        .into_iter()
        .map(|pane| Held::from_frame(pane, None))
        .collect();
    let Some(first) = config.dirs.first() else {
        return HoldEnd::Failed;
    };
    let Ok(listening) = listen(first, HANDOVER_SOCKET) else {
        return HoldEnd::Failed;
    };
    let me = std::process::id();
    if take_dirs(&config.dirs, config.parent, me).is_err() {
        return HoldEnd::Failed;
    }
    if wire.write_all(&[READY]).is_err() {
        give_back(&config.dirs, me, config.parent);
        return HoldEnd::Failed;
    }
    drop(spawner);
    let holding = Detached {
        listening: &listening,
        layout: &layout,
        deadline: Some(deadline),
        drain: Drain::Stop(config.buffer_limit),
        uid: config.uid,
        heir: None,
    };
    hold_detached(holding, held, None)
}

/// Takes every directory from `from`, or none: what was taken is given back
/// when one cannot be.
fn take_dirs(dirs: &[PathBuf], from: u32, me: u32) -> io::Result<()> {
    let mut taken = Vec::new();
    for dir in dirs {
        if let Err(error) = ssh_route::adopt_instance(dir, Some(from), me) {
            give_back(&taken, me, from);
            return Err(error);
        }
        taken.push(dir.clone());
    }
    Ok(())
}

/// Hands the directories back to the spawner: the holder failed before the
/// spawner left, and a dead owner would have the spawner's directories swept
/// under it.
fn give_back(dirs: &[PathBuf], me: u32, parent: u32) {
    for dir in dirs {
        let _ = ssh_route::adopt_instance(dir, Some(me), parent);
    }
}

/// A bound holder: the handshake, then its spawner's messages, the
/// listener and each pane's exit watch in one `select` — until the
/// connection ends (detached), a goodbye or a handover.
fn hold_bound(spawner: UnixStream, config: &HoldConfig) -> HoldEnd {
    let wire = Wire::new(&spawner, Some(Instant::now() + HAND_WAIT));
    let shaken = (|| -> Result<Option<Vec<u8>>, HandoverError> {
        let version = wire.u32()?;
        let mut total = 0u64;
        let id = read_field(&wire, u64::from(wire.u32()?), ID_LIMIT, &mut total)?;
        if version != BOUND_VERSION || id != build_id().as_bytes() {
            return Ok(None);
        }
        Ok(Some(read_field(
            &wire,
            wire.u64()?,
            FIELD_LIMIT,
            &mut total,
        )?))
    })();
    let Ok(Some(layout)) = shaken else {
        let _ = wire.write_all(&[REFUSED]);
        return HoldEnd::Failed;
    };
    let me = std::process::id();
    let listening = match config
        .dirs
        .first()
        .map(|dir| listen(dir, &bound_socket(me)))
    {
        Some(Ok(listening)) => listening,
        _ => {
            let _ = wire.write_all(&[REFUSED]);
            return HoldEnd::Failed;
        }
    };
    if wire.write_all(&[READY]).is_err() {
        return HoldEnd::Failed;
    }
    // The spawner's exit, watched from the start: it is alive now, and its
    // directories pass to this holder only once it is gone.
    let watch =
        jobs::start_time(config.parent).and_then(|start| jobs::exit_fd(config.parent, start));
    let mut registry = Registry {
        panes: Vec::new(),
        layout,
    };
    loop {
        let mut fds = vec![spawner.as_raw_fd(), listening.listener.as_raw_fd()];
        fds.extend(registry.panes.iter().map(|pane| pane.exit.as_raw_fd()));
        let ready = readable(&fds, None);
        // A program that exited: its copy closes now, whether or not the
        // release ever comes — and before the messages that arrived with it,
        // so a ping answered after an exit was seen answers for it too.
        let mut index = 0;
        registry.panes.retain(|_| {
            index += 1;
            !ready[index + 1]
        });
        if ready[0]
            && let Some(end) = bound_step(&spawner, &mut registry)
        {
            return bound_end(end, spawner, listening, registry, config, None, watch);
        }
        if !ready[1] {
            continue;
        }
        let Some(client) = accept_take(&listening.listener, config.uid, None) else {
            continue;
        };
        // An end of the spawner already waiting comes first: a bateri that
        // starts right after a crash must find this holder detached.
        while fd_ready(spawner.as_fd()) {
            if let Some(end) = bound_step(&spawner, &mut registry) {
                return bound_end(
                    end,
                    spawner,
                    listening,
                    registry,
                    config,
                    Some(client),
                    watch,
                );
            }
        }
        drop(client);
    }
}

/// How a bound holder's connection ends.
enum BoundEnd {
    /// Without a goodbye: bateri crashed, was force-quit or killed.
    Gone,
    Quit,
    Handover,
    /// Not a message this build sends: the stream cannot be followed.
    Broken,
}

/// A bound holder's end; `client` is a [`TAKE`] that waited for it, `watch`
/// the spawner's exit watch.
fn bound_end(
    end: BoundEnd,
    spawner: UnixStream,
    listening: Listening,
    registry: Registry,
    config: &HoldConfig,
    client: Option<UnixStream>,
    watch: Option<OwnedFd>,
) -> HoldEnd {
    match end {
        BoundEnd::Gone => {
            drop(spawner);
            hold_after_crash(&listening, registry, config, client, watch)
        }
        // Bateri keeps every program: the copies close, nobody is signalled.
        BoundEnd::Quit | BoundEnd::Broken => {
            drop(registry);
            drop(spawner);
            if matches!(end, BoundEnd::Quit) {
                HoldEnd::Quit
            } else {
                HoldEnd::Failed
            }
        }
        BoundEnd::Handover => hold_handed_over(spawner, listening, registry, config, watch),
    }
}

/// Whether an I/O error is the other side's end rather than a fault.
fn gone(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::UnexpectedEof | io::ErrorKind::ConnectionReset | io::ErrorKind::BrokenPipe
    )
}

/// Reads and applies one message of the spawner; the end it brings, if
/// any.
fn bound_step(spawner: &UnixStream, registry: &mut Registry) -> Option<BoundEnd> {
    let wire = Wire::patient(spawner);
    let mut tag = [0u8];
    if let Err(error) = wire.read_exact(&mut tag) {
        return Some(if gone(&error) {
            BoundEnd::Gone
        } else {
            BoundEnd::Broken
        });
    }
    let applied = match tag[0] {
        MSG_ADD => registry.add(spawner, &wire),
        MSG_DROP => registry.drop_pane(&wire),
        MSG_CONFIRM => registry.confirm(&wire),
        MSG_LAYOUT => registry.layout(&wire),
        MSG_STATE => registry.state(&wire),
        MSG_PING => {
            let _ = Wire::new(spawner, None).write_all(&[PONG]);
            Ok(())
        }
        MSG_QUIT => return Some(BoundEnd::Quit),
        MSG_HANDOVER => return Some(BoundEnd::Handover),
        _ => return Some(BoundEnd::Broken),
    };
    match applied {
        Ok(()) => None,
        Err(HandoverError::Io(error)) if gone(&error) => Some(BoundEnd::Gone),
        Err(_) => Some(BoundEnd::Broken),
    }
}

impl Registry {
    /// A pane: kept if there is room and its program is still the one it
    /// was, otherwise refused out loud. A tab registered again replaces its
    /// older record.
    fn add(&mut self, spawner: &UnixStream, wire: &Wire<'_>) -> Result<(), HandoverError> {
        wire.arm(false)?;
        // A full descriptor table refuses the pane, the stream goes on (its
        // byte was read); any other failure is not followed.
        let master = match recv_fd(spawner) {
            Ok(master) => Some(master),
            Err(HandoverError::Truncated) => None,
            Err(error) => return Err(error),
        };
        let mut total = 0u64;
        let tab = read_tab(wire, &mut total)?;
        let pid = wire.u32()?;
        let start = wire.u64()?;
        let flags = wire.u32()?;
        let from = read_field(wire, u64::from(wire.u32()?), PATH_LIMIT, &mut total)?;
        let blob = read_field(wire, wire.u64()?, FIELD_LIMIT, &mut total)?;
        self.panes.retain(|pane| pane.tab != tab);
        let room = self.panes.len() < PANE_CAP;
        let kept = master
            .filter(|master| room && selectable(master.as_raw_fd()))
            .and_then(|master| {
                let exit = jobs::exit_fd(pid, start)?;
                selectable(exit.as_raw_fd()).then_some((master, exit))
            });
        let Some((master, exit)) = kept else {
            let answer = Wire::new(spawner, None);
            let _ = answer
                .write_all(&[PANE_REFUSED])
                .and_then(|()| write_tab(&answer, tab.as_str()));
            return Ok(());
        };
        let taken_from = (flags & ADD_UNCONFIRMED != 0 && !from.is_empty()).then(|| {
            use std::os::unix::ffi::OsStringExt;
            PathBuf::from(std::ffi::OsString::from_vec(from))
        });
        self.panes.push(Registered {
            tab,
            pid,
            start,
            parent: if flags & ADD_LOGIN != 0 {
                ShellParent::Login
            } else {
                ShellParent::Direct
            },
            blob,
            master,
            exit,
            taken_from,
        });
        Ok(())
    }

    /// A released pane: its copy closes, nobody is signalled.
    fn drop_pane(&mut self, wire: &Wire<'_>) -> Result<(), HandoverError> {
        let tab = read_tab(wire, &mut 0)?;
        self.panes.retain(|pane| pane.tab != tab);
        Ok(())
    }

    fn confirm(&mut self, wire: &Wire<'_>) -> Result<(), HandoverError> {
        let tab = read_tab(wire, &mut 0)?;
        for pane in self.panes.iter_mut().filter(|pane| pane.tab == tab) {
            pane.taken_from = None;
        }
        Ok(())
    }

    fn layout(&mut self, wire: &Wire<'_>) -> Result<(), HandoverError> {
        self.layout = read_field(wire, wire.u64()?, FIELD_LIMIT, &mut 0)?;
        Ok(())
    }

    /// A pane's newer blob.
    fn state(&mut self, wire: &Wire<'_>) -> Result<(), HandoverError> {
        let mut total = 0u64;
        let tab = read_tab(wire, &mut total)?;
        let blob = read_field(wire, wire.u64()?, FIELD_LIMIT, &mut total)?;
        if let Some(pane) = self.panes.iter_mut().find(|pane| pane.tab == tab) {
            pane.blob = blob;
        }
        Ok(())
    }
}

/// The spawner of a bound holder is gone without a goodbye: the holder builds
/// the bundle from what it was sent — each pane's newest blob, no screen (the
/// program redraws), the size its master has — and holds, detached, until a
/// client takes it. With nothing registered there is nothing to hold. The
/// instance's directories are taken only once the spawner's exit watch
/// (`watch`) fires: a connection may end with its bateri alive (a handle
/// dropped, a failed start), and a living bateri's directories must not pass
/// to a holder that later exits — the sweep would remove them under it.
fn hold_after_crash(
    listening: &Listening,
    registry: Registry,
    config: &HoldConfig,
    client: Option<UnixStream>,
    watch: Option<OwnedFd>,
) -> HoldEnd {
    let Registry { panes, layout } = registry;
    if panes.is_empty() {
        return HoldEnd::Released;
    }
    let held = panes
        .into_iter()
        .map(|pane| {
            let (cols, rows) = window_size(pane.master.as_fd());
            let state = PaneState {
                cols,
                rows,
                parent: pane.parent,
                vt: Vec::new(),
                core: pane.blob,
                input: Vec::new(),
                history: Vec::new(),
            };
            Held {
                tab: pane.tab.as_str().to_owned(),
                pid: pane.pid,
                start: pane.start,
                ended: false,
                cut: false,
                blob: state.encode(),
                buffer: VecDeque::new(),
                master: Some(pane.master),
                exit: Some(pane.exit),
                beside: pane.taken_from,
            }
        })
        .collect();
    let holding = Detached {
        listening,
        layout: &layout,
        deadline: config.limit.map(|limit| Instant::now() + limit),
        drain: Drain::Ring(config.buffer_limit),
        uid: config.uid,
        heir: watch.map(|watch| Heir {
            watch,
            dirs: config.dirs.clone(),
            from: config.parent,
        }),
    };
    hold_detached(holding, held, client)
}

/// A deliberate handover over the live connection: the frame replaces the
/// registrations, the holder owns the directories, says [`READY`] and holds,
/// detached. A spawner that dies in the middle of the frame is a crash; a
/// frame that cannot be read is refused — bateri still has every program.
fn hold_handed_over(
    spawner: UnixStream,
    listening: Listening,
    registry: Registry,
    config: &HoldConfig,
    watch: Option<OwnedFd>,
) -> HoldEnd {
    let wire = Wire::new(&spawner, None);
    let bundle = match read_header(&wire).and_then(|()| read_body(&wire)) {
        Ok(bundle) => bundle,
        Err(HandoverError::Io(error)) if gone(&error) => {
            drop(spawner);
            return hold_after_crash(&listening, registry, config, None, watch);
        }
        Err(_) => {
            let _ = wire.write_all(&[REFUSED]);
            return HoldEnd::Failed;
        }
    };
    // Every registration's copy closes — the frame's own copies replace
    // them, and a pane it does not carry is bateri's to close — before the
    // frame's panes get their exit watches, which keeps the descriptors
    // under `FD_SETSIZE`.
    drop(registry);
    let me = std::process::id();
    if take_dirs(&config.dirs, config.parent, me).is_err() {
        let _ = wire.write_all(&[REFUSED]);
        return HoldEnd::Failed;
    }
    let layout = bundle.layout;
    let held: Vec<Held> = bundle
        .panes
        .into_iter()
        .map(|pane| {
            let exit = jobs::exit_fd(pane.pid, pane.start);
            Held::from_frame(pane, exit)
        })
        .collect();
    if wire.write_all(&[READY]).is_err() {
        give_back(&config.dirs, me, config.parent);
        return HoldEnd::Failed;
    }
    drop(spawner);
    let holding = Detached {
        listening: &listening,
        layout: &layout,
        deadline: config.limit.map(|limit| Instant::now() + limit),
        drain: Drain::Ring(config.buffer_limit),
        uid: config.uid,
        heir: None,
    };
    hold_detached(holding, held, None)
}

/// The PTY's size from its master (`TIOCGWINSZ`); zero if it cannot be read.
fn window_size(master: BorrowedFd<'_>) -> (u16, u16) {
    // SAFETY: an all-zero `winsize` is valid; the ioctl fills it or fails.
    let mut size: libc::winsize = unsafe { std::mem::zeroed() };
    // SAFETY: `master` is open for the call; `size` belongs to this frame.
    let status = unsafe { libc::ioctl(master.as_raw_fd(), libc::TIOCGWINSZ, &raw mut size) };
    if status == 0 {
        (size.ws_col, size.ws_row)
    } else {
        (0, 0)
    }
}

/// Binds `name` in `dir`, a private directory of this user. A file already
/// there is removed only when nobody listens on it (a dead holder's); a live
/// one is another holder's and this one fails. Only this name is asked:
/// another holder's socket beside it is no obstacle.
fn listen(dir: &Path, name: &str) -> io::Result<Listening> {
    if !ssh_route::private_dir(dir) {
        return Err(io::ErrorKind::PermissionDenied.into());
    }
    let path = dir.join(name);
    if path.as_os_str().len() >= SUN_PATH {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    if socket_listening(&path) {
        return Err(io::ErrorKind::AddrInUse.into());
    }
    match std::fs::remove_file(&path) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let listener = UnixListener::bind(&path)?;
    let listening = Listening { listener, path };
    listening.listener.set_nonblocking(true)?;
    Ok(listening)
}

/// What a holder with the bundle and no spawner works with.
struct Detached<'a> {
    listening: &'a Listening,
    layout: &'a [u8],
    deadline: Option<Instant>,
    drain: Drain,
    uid: u32,
    /// The directories to take once the spawner is gone.
    heir: Option<Heir>,
}

/// A spawner's directories, taken when its exit watch fires.
struct Heir {
    watch: OwnedFd,
    dirs: Vec<PathBuf>,
    from: u32,
}

/// A holder with the bundle and no spawner: drains and serves until an
/// acknowledgement, a release, the deadline — or, draining without a stop,
/// until no program is left. `client` is a [`TAKE`] that already waited.
fn hold_detached(
    mut holding: Detached<'_>,
    mut held: Vec<Held>,
    mut client: Option<UnixStream>,
) -> HoldEnd {
    let mut probe_at = Instant::now();
    loop {
        if held.iter().all(|pane| pane.master.is_none()) {
            return HoldEnd::Released;
        }
        // A holder without a time limit holds for the programs: when none is
        // left it ends (their last output goes with it).
        if matches!(holding.drain, Drain::Ring(_))
            && held.iter().all(|pane| pane.master.is_none() || pane.ended)
        {
            return HoldEnd::Released;
        }
        if holding
            .deadline
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            hang_up_all(&mut held);
            return HoldEnd::Limit;
        }
        if Instant::now() >= probe_at {
            probe_beside(&mut held);
            probe_at = Instant::now() + UNCONFIRMED_POLL;
        }
        let taker = match client.take() {
            Some(taker) => Some(taker),
            None => wait(&mut holding, &mut held, probe_at),
        };
        let Some(taker) = taker else {
            continue;
        };
        match serve(&taker, holding.layout, &mut held, holding.deadline) {
            Served::Acked => return HoldEnd::Acked,
            Served::Released => {
                hang_up_all(&mut held);
                return HoldEnd::Released;
            }
            Served::Limit => {
                hang_up_all(&mut held);
                return HoldEnd::Limit;
            }
            Served::Gone => {}
        }
    }
}

/// Closes every copy left and signals each program still running: nobody
/// took them. A program another holder still carries (an unconfirmed
/// registration whose holder listens, asked afresh) is not this holder's to
/// end — only its copy closes.
fn hang_up_all(held: &mut [Held]) {
    probe_beside(held);
    for pane in held.iter_mut() {
        let Some(master) = pane.master.take() else {
            continue;
        };
        drop(master);
        if !pane.ended && pane.beside.is_none() {
            hang_up(pane.pid, pane.start, pane.exit.as_ref());
        }
    }
}

/// Lets go of the panes another holder still drains once that holder no
/// longer listens: the program must not block on a master nobody reads.
fn probe_beside(held: &mut [Held]) {
    let mut sockets: Vec<PathBuf> = Vec::new();
    for socket in held.iter().filter_map(|pane| pane.beside.clone()) {
        if !sockets.contains(&socket) {
            sockets.push(socket);
        }
    }
    for socket in sockets {
        if socket_listening(&socket) {
            continue;
        }
        for pane in held
            .iter_mut()
            .filter(|pane| pane.beside.as_ref() == Some(&socket))
        {
            pane.beside = None;
        }
    }
}

/// One wait while nobody is connected: drains the readable masters, marks
/// the programs whose exit watch fired as ended (after what they left is
/// read), takes the directories of a spawner whose exit watch fired, and
/// returns a client of user `uid` that said [`TAKE`]; `None` on a timeout
/// (the limit, or the next probe), after a drain, or for a connection closed
/// unanswered (another user, a probe).
fn wait(holding: &mut Detached<'_>, held: &mut [Held], probe_at: Instant) -> Option<UnixStream> {
    let now = Instant::now();
    let mut timeout = holding
        .deadline
        .map(|deadline| deadline.saturating_duration_since(now));
    if held.iter().any(|pane| pane.beside.is_some()) {
        let probe = probe_at.saturating_duration_since(now);
        timeout = Some(timeout.map_or(probe, |timeout| timeout.min(probe)));
    }
    let mut fds = vec![holding.listening.listener.as_raw_fd()];
    if let Some(heir) = &holding.heir {
        fds.push(heir.watch.as_raw_fd());
    }
    let first_pane = fds.len();
    // Per descriptor from `first_pane`: the pane and whether it is the exit watch.
    let mut roles: Vec<(usize, bool)> = Vec::new();
    for (index, pane) in held.iter().enumerate() {
        if let Some(master) = &pane.master
            && drainable(pane, holding.drain)
        {
            fds.push(master.as_raw_fd());
            roles.push((index, false));
        }
        if let Some(exit) = &pane.exit {
            fds.push(exit.as_raw_fd());
            roles.push((index, true));
        }
    }
    let ready = readable(&fds, timeout);
    if first_pane > 1
        && ready[1]
        && let Some(heir) = holding.heir.take()
    {
        let me = std::process::id();
        // Not all or none: the spawner is gone, and a directory without a
        // live owner is swept only when no holder listens in it.
        for dir in &heir.dirs {
            let _ = ssh_route::adopt_instance(dir, Some(heir.from), me);
        }
    }
    for (&(index, exit), _) in roles
        .iter()
        .zip(&ready[first_pane..])
        .filter(|(_, ready)| **ready)
    {
        let pane = &mut held[index];
        if exit {
            // It stays readable: watched once.
            pane.exit = None;
            if pane.beside.is_none() {
                // What the program left, whole (its output has an end now).
                drain_pane(pane, holding.drain, BUFFER_LIMIT);
            }
            pane.ended = true;
        } else {
            drain_pane(pane, holding.drain, CHUNK);
        }
    }
    if !ready[0] {
        return None;
    }
    accept_take(&holding.listening.listener, holding.uid, holding.deadline)
}

/// Whether a pane's master is read now: its program not ended, no other
/// holder draining it, and — stopping at the limit — room left.
fn drainable(pane: &Held, drain: Drain) -> bool {
    !pane.ended
        && pane.beside.is_none()
        && match drain {
            Drain::Stop(limit) => pane.buffer.len() < limit,
            Drain::Ring(_) => true,
        }
}

/// Accepts a connection and keeps it if it is user `uid` and says [`TAKE`]
/// within [`TAKE_WAIT`].
fn accept_take(listener: &UnixListener, uid: u32, deadline: Option<Instant>) -> Option<UnixStream> {
    let (client, _) = match listener.accept() {
        Ok(accepted) => accepted,
        Err(error) => {
            // Out of descriptors leaves the connection waiting and the
            // listener readable: a pause, not a spin.
            if !matches!(
                error.kind(),
                io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
            ) {
                std::thread::sleep(ACCEPT_PAUSE);
            }
            return None;
        }
    };
    // The accepted stream inherits nothing from the non-blocking listener
    // on Linux and everything on macOS: make it blocking either way.
    client.set_nonblocking(false).ok()?;
    if peer(&client).ok()?.0 != uid {
        return None;
    }
    let until = Instant::now() + TAKE_WAIT;
    let wire = Wire::new(
        &client,
        Some(deadline.map_or(until, |deadline| deadline.min(until))),
    );
    let mut first = [0u8];
    wire.read_exact(&mut first).ok()?;
    (first[0] == TAKE).then_some(client)
}

/// `select` for reading on `fds`, waiting at most `timeout` (`None`: until
/// one is ready); which are ready, in order. A descriptor that does not fit
/// an `fd_set` is never ready, and an interrupted or failed call reports
/// none — the caller asks again. `select`, not `poll`: macOS' `poll` does not
/// support devices, a PTY master among them.
fn readable(fds: &[RawFd], timeout: Option<Duration>) -> Vec<bool> {
    let mut ready = vec![false; fds.len()];
    // SAFETY: an all-zero `fd_set` is a valid empty set, and every fd added
    // is below `FD_SETSIZE` ([`selectable`]).
    let mut set: libc::fd_set = unsafe { std::mem::zeroed() };
    let mut top = -1;
    for &fd in fds.iter().filter(|&&fd| selectable(fd)) {
        unsafe { libc::FD_SET(fd, &raw mut set) };
        top = top.max(fd);
    }
    let mut time = timeout.map(|timeout| libc::timeval {
        tv_sec: timeout.as_secs().try_into().unwrap_or(libc::time_t::MAX),
        // Below a million: fits `suseconds_t` (`i32` on macOS, `i64` on Linux).
        tv_usec: timeout.subsec_micros() as libc::suseconds_t,
    });
    let time_ptr = time
        .as_mut()
        .map_or(std::ptr::null_mut(), std::ptr::from_mut);
    // SAFETY: the set and the timeout belong to this frame.
    let count = unsafe {
        libc::select(
            top + 1,
            &raw mut set,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            time_ptr,
        )
    };
    if count <= 0 {
        return ready;
    }
    for (slot, &fd) in ready.iter_mut().zip(fds) {
        // SAFETY: `set` is the set `select` filled.
        *slot = selectable(fd) && unsafe { libc::FD_ISSET(fd, &raw const set) };
    }
    ready
}

/// Whether `fd` fits an `fd_set`. Registration refuses a pane past it, so
/// this is a guard, not a limit anyone meets.
fn selectable(fd: RawFd) -> bool {
    usize::try_from(fd).is_ok_and(|fd| fd < libc::FD_SETSIZE)
}

/// Reads what the master has: stopping, up to the limit; never stopping, at
/// most `budget` bytes at a time (a flood must not starve the listener) with
/// the oldest bytes past the limit dropped. EOF or an error other than
/// "nothing yet" is the program's end.
fn drain_pane(pane: &mut Held, drain: Drain, mut budget: usize) {
    let Some(master) = &pane.master else {
        return;
    };
    let mut chunk = [0u8; 16 * 1024];
    loop {
        let room = match drain {
            Drain::Stop(limit) => limit.saturating_sub(pane.buffer.len()),
            Drain::Ring(_) => budget,
        }
        .min(chunk.len());
        if room == 0 {
            return;
        }
        // SAFETY: `chunk` belongs to this frame and `room` fits it.
        let read = unsafe { libc::read(master.as_raw_fd(), chunk.as_mut_ptr().cast(), room) };
        match read {
            0 => {
                pane.ended = true;
                return;
            }
            n if n > 0 => {
                let n = n as usize;
                pane.buffer.extend(&chunk[..n]);
                if let Drain::Ring(limit) = drain {
                    budget -= n;
                    let excess = pane.buffer.len().saturating_sub(limit);
                    if excess > 0 {
                        pane.buffer.drain(..excess);
                        pane.cut = true;
                    }
                }
            }
            _ => {
                let error = io::Error::last_os_error();
                match error.kind() {
                    io::ErrorKind::Interrupted => {}
                    io::ErrorKind::WouldBlock => return,
                    // EIO: the slave side is gone (Linux; macOS gives EOF).
                    _ => {
                        pane.ended = true;
                        return;
                    }
                }
            }
        }
    }
}

/// How serving one client ended.
enum Served {
    Acked,
    Released,
    Limit,
    /// The client went away (or broke the protocol) without an
    /// acknowledgement: wait for the next.
    Gone,
}

/// Gives the panes still held to `client` and follows its messages, all
/// within `deadline` (if any). A failed write still reads what the client
/// already sent — a [`RELEASE_ALL`] may be waiting there.
fn serve(
    client: &UnixStream,
    layout: &[u8],
    held: &mut [Held],
    deadline: Option<Instant>,
) -> Served {
    let given: Vec<usize> = (0..held.len())
        .filter(|&i| held[i].master.is_some())
        .collect();
    for &i in &given {
        held[i].buffer.make_contiguous();
    }
    let views = given
        .iter()
        .filter_map(|&i| {
            let pane = &held[i];
            Some(PaneView {
                tab: &pane.tab,
                pid: pane.pid,
                start: pane.start,
                ended: pane.ended,
                cut: pane.cut,
                blob: &pane.blob,
                buffer: pane.buffer.as_slices().0,
                master: pane.master.as_ref()?.as_raw_fd(),
            })
        })
        .collect();
    let wire = Wire::new(client, deadline);
    let blocking = write_views(&wire, layout, views).is_ok();
    if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
        return Served::Limit;
    }
    if !blocking && client.set_nonblocking(true).is_err() {
        return Served::Gone;
    }
    messages(client, held, &given, deadline, blocking)
}

/// The client's messages until one ends the serving. `blocking`: wait for
/// them (until `deadline`, if any); otherwise only what is already there.
fn messages(
    client: &UnixStream,
    held: &mut [Held],
    given: &[usize],
    deadline: Option<Instant>,
    blocking: bool,
) -> Served {
    let wire = Wire::new(client, deadline);
    loop {
        if blocking {
            let wait = match deadline {
                Some(deadline) => {
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    if remaining.is_zero() {
                        return Served::Limit;
                    }
                    Some(remaining)
                }
                None => None,
            };
            // Fails once the client closed (macOS' EINVAL): the read then
            // sees the end without blocking.
            let _ = client.set_read_timeout(wait);
        }
        let mut tag = [0u8];
        match (&*client).read(&mut tag) {
            Ok(1) => {}
            Ok(_) => return Served::Gone,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) if blocking && deadline.is_some_and(|deadline| Instant::now() >= deadline) => {
                return Served::Limit;
            }
            Err(_) => return Served::Gone,
        }
        match tag[0] {
            ACK => return Served::Acked,
            RELEASE_ALL => return Served::Released,
            RELEASE => {
                let mut position = [0u8; 4];
                if wire.read_exact(&mut position).is_err() {
                    return Served::Gone;
                }
                let position = u32::from_le_bytes(position) as usize;
                let Some(&i) = given.get(position) else {
                    return Served::Gone;
                };
                // Only the copy: the client releases a duplicate this way
                // too, whose program lives on with another holder.
                held[i].master = None;
            }
            _ => return Served::Gone,
        }
    }
}

// ─── the process ─────────────────────────────────────────────────────────

/// `bateri hold --fd FD --dir DIR [--dir DIR]...`: the holder process,
/// started by bateri with one end of a `socketpair` at `FD` and the
/// instance's directories (the first holds the socket) — by the old bateri at
/// an update's moment, or by a bateri at its start for a bound holder; the
/// first bytes on `FD` tell which. Returns the exit code. The body is
/// [`hold`]; here the argv and the process ([`detach`]).
pub fn hold_main(args: &[String]) -> i32 {
    hold_process(args, None, BUFFER_LIMIT)
}

fn hold_process(args: &[String], limit: Option<Duration>, buffer_limit: usize) -> i32 {
    let Some((fd, dirs)) = parse_args(args) else {
        eprintln!("{USAGE}");
        return EXIT_USAGE;
    };
    // Only a socket is taken as the spawner's end: an fd number that is
    // something else (or nothing) is not ours to own.
    // SAFETY: an all-zero `stat` is valid; `fstat` fills it or fails.
    let mut meta: libc::stat = unsafe { std::mem::zeroed() };
    // SAFETY: `meta` belongs to this frame.
    if unsafe { libc::fstat(fd, &raw mut meta) } != 0
        || meta.st_mode & libc::S_IFMT != libc::S_IFSOCK
    {
        eprintln!("bateri hold: fd {fd} is not a socket");
        return EXIT_FAILED;
    }
    // Before `detach`: the spawner is the parent until it exits.
    let parent = std::os::unix::process::parent_id();
    detach(fd);
    raise_descriptor_limit();
    // SAFETY: `fd` is an open socket (checked above) this process inherited
    // for this purpose alone; nothing else owns it.
    let spawner = unsafe { UnixStream::from_raw_fd(fd) };
    set_cloexec(fd);
    // SAFETY: `getuid` has no preconditions and cannot fail.
    let uid = unsafe { libc::getuid() };
    let config = HoldConfig {
        dirs,
        parent,
        uid,
        limit,
        buffer_limit,
    };
    match hold(spawner, &config) {
        HoldEnd::Acked | HoldEnd::Released | HoldEnd::Limit | HoldEnd::Quit => EXIT_DONE,
        HoldEnd::Failed => EXIT_FAILED,
    }
}

/// Raises the soft limit on open descriptors to `FD_SETSIZE` (or the hard
/// limit, if lower): an app launched from the Dock inherits a soft limit of
/// 256, and a bound holder keeps up to three descriptors per pane
/// ([`PANE_CAP`]).
fn raise_descriptor_limit() {
    let mut limit = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    // SAFETY: `limit` belongs to this frame; the calls read or write it only.
    unsafe {
        if libc::getrlimit(libc::RLIMIT_NOFILE, &raw mut limit) != 0 {
            return;
        }
        let wanted = (libc::FD_SETSIZE as libc::rlim_t).min(limit.rlim_max);
        if limit.rlim_cur < wanted {
            limit.rlim_cur = wanted;
            libc::setrlimit(libc::RLIMIT_NOFILE, &raw const limit);
        }
    }
}

/// `--fd FD` then one or more `--dir DIR`, nothing else; `FD` above the
/// standard three, every directory absolute.
fn parse_args(args: &[String]) -> Option<(RawFd, Vec<PathBuf>)> {
    let [fd_flag, fd, rest @ ..] = args else {
        return None;
    };
    if fd_flag != "--fd" || rest.is_empty() || rest.len() % 2 != 0 {
        return None;
    }
    let fd: RawFd = fd.parse().ok().filter(|&fd: &RawFd| fd > 2)?;
    let mut dirs = Vec::new();
    for pair in rest.chunks(2) {
        let dir = PathBuf::from(&pair[1]);
        if pair[0] != "--dir" || !dir.is_absolute() {
            return None;
        }
        dirs.push(dir);
    }
    Some((fd, dirs))
}

/// The holder leaves the old bateri behind: its own session (`setsid`),
/// `SIGHUP` and `SIGPIPE` ignored, `/` as working directory (the old one may
/// be inside the bundle being replaced), standard I/O on `/dev/null` and
/// every inherited descriptor but `keep` closed — an untracked copy of a
/// master would outlive the holder's release and defeat the hang-up.
fn detach(keep: RawFd) {
    // SAFETY: plain process calls without memory arguments (the paths are
    // NUL-terminated literals); a failure leaves the setting as it was —
    // `setsid` fails only for a process group leader, which a spawned
    // child is not.
    unsafe {
        libc::setsid();
        libc::signal(libc::SIGHUP, libc::SIG_IGN);
        libc::signal(libc::SIGPIPE, libc::SIG_IGN);
        libc::chdir(c"/".as_ptr());
        let null = libc::open(c"/dev/null".as_ptr(), libc::O_RDWR);
        if null >= 0 {
            for fd in 0..=2 {
                libc::dup2(null, fd);
            }
            if null > 2 {
                libc::close(null);
            }
        }
    }
    // The open descriptors, listed first and closed after: the listing's
    // own descriptor is among them and closed by then.
    let open: Vec<RawFd> = std::fs::read_dir("/dev/fd")
        .map(|entries| {
            entries
                .flatten()
                .filter_map(|entry| entry.file_name().to_str()?.parse().ok())
                .collect()
        })
        .unwrap_or_default();
    for fd in open.into_iter().filter(|&fd| fd > 2 && fd != keep) {
        // SAFETY: an inherited descriptor nothing in this process owns; one
        // already closed answers EBADF.
        unsafe { libc::close(fd) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::child::wait_until;
    use crate::jobs::start_time;
    use crate::ssh_route::prepare_instance;
    use std::fs::File;
    use std::os::unix::fs::OpenOptionsExt;
    use std::os::unix::process::CommandExt;
    use std::process::{Child, Command, Stdio};
    use std::sync::Mutex;

    const ID: &str = "0F1E2D3C-4B5A-6978-8796-A5B4C3D2E1F0";
    const OTHER: &str = "11111111-2222-3333-4444-555555555555";
    const INSTANCE: &str = "11111111";

    fn tab(id: &str) -> TabId {
        TabId::parse(id).unwrap()
    }

    fn uid() -> u32 {
        // SAFETY: `getuid` has no preconditions and cannot fail.
        unsafe { libc::getuid() }
    }

    /// A private root (short: a socket is bound under it) with this test's
    /// instance directory, owned by this process.
    fn scratch(name: &str) -> (PathBuf, PathBuf) {
        let root = PathBuf::from(format!("/tmp/bt-ho-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let dir = prepare_instance(&root, INSTANCE).expect("instance directory");
        (root, dir)
    }

    fn owner_of(dir: &Path) -> Option<u32> {
        std::fs::read_to_string(dir.join("pid"))
            .ok()?
            .trim()
            .parse()
            .ok()
    }

    /// A descriptor to carry: one end of a pair, the other kept to prove the
    /// carried one still works.
    fn carried() -> (OwnedFd, UnixStream) {
        let (carried, kept) = UnixStream::pair().unwrap();
        (OwnedFd::from(carried), kept)
    }

    fn proves_alive(fd: &OwnedFd, kept: &UnixStream, word: &[u8]) {
        let mut sender = File::from(fd.try_clone().unwrap());
        sender.write_all(word).unwrap();
        let mut got = vec![0u8; word.len()];
        kept.set_read_timeout(Some(HAND_WAIT)).unwrap();
        (&*kept).read_exact(&mut got).unwrap();
        assert_eq!(got, word);
    }

    /// The frame round-trips: layout, every field of every pane, and the
    /// descriptors themselves — a blob larger than the socket's buffer too.
    #[test]
    fn a_frame_round_trips_with_its_descriptors() {
        let (writer, reader) = UnixStream::pair().unwrap();
        let (first_fd, first_kept) = carried();
        let (second_fd, second_kept) = carried();
        let big: Vec<u8> = (0..600_000u32).map(|i| i as u8).collect();
        let mut sent = Bundle {
            layout: b"layout bytes".to_vec(),
            panes: vec![
                HeldPane::new(tab(ID), 42, 7, big.clone(), b"tail".to_vec(), first_fd),
                HeldPane::new(tab(OTHER), 43, u64::MAX, Vec::new(), Vec::new(), second_fd),
            ],
        };
        sent.panes[1].ended = true;
        let writing = std::thread::spawn(move || {
            write_frame(&writer, &sent).unwrap();
            sent
        });
        let got = read_frame(&reader).unwrap();
        drop(writing.join().unwrap());
        assert_eq!(got.layout, b"layout bytes");
        assert_eq!(got.panes.len(), 2);
        let (a, b) = (&got.panes[0], &got.panes[1]);
        assert_eq!(
            (a.tab.as_str(), a.pid, a.start, a.ended),
            (ID, 42, 7, false)
        );
        assert_eq!((a.blob == big, a.buffer.as_slice()), (true, &b"tail"[..]));
        assert_eq!(
            (b.tab.as_str(), b.pid, b.start, b.ended),
            (OTHER, 43, u64::MAX, true)
        );
        assert!(b.blob.is_empty() && b.buffer.is_empty());
        assert_eq!((a.position, b.position), (0, 1));
        // The sender's copies are gone; the received ones still reach the peers.
        proves_alive(&a.master, &first_kept, b"one");
        proves_alive(&b.master, &second_kept, b"two");
    }

    /// A frame of another version is refused before its body, and the
    /// client releases everything; bytes that are not a frame are refused.
    #[test]
    fn an_unknown_frame_version_is_released() {
        let (root, dir) = scratch("version");
        let listener = UnixListener::bind(dir.join(HANDOVER_SOCKET)).unwrap();
        let holder = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut take = [0u8];
            stream.read_exact(&mut take).unwrap();
            assert_eq!(take[0], TAKE);
            stream.write_all(&MAGIC).unwrap();
            stream.write_all(&99u32.to_le_bytes()).unwrap();
            let mut answer = [0u8];
            stream.read_exact(&mut answer).unwrap();
            let (mut second, _) = listener.accept().unwrap();
            // Its `TAKE` is read first: closing with it unread resets the
            // connection on Linux, and the client would see that reset
            // instead of the bytes.
            second.read_exact(&mut take).unwrap();
            second.write_all(b"SSH-2.0").unwrap();
            answer[0]
        });
        assert!(matches!(take(&dir, uid()), Err(HandoverError::Version(99))));
        assert!(matches!(take(&dir, uid()), Err(HandoverError::NotAFrame)));
        assert_eq!(holder.join().unwrap(), RELEASE_ALL);
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// A broken body is refused, not trusted: a length past the limit and a
    /// pane whose descriptor never came.
    #[test]
    fn a_broken_frame_is_refused() {
        let (writer, reader) = UnixStream::pair().unwrap();
        let mut out = &writer;
        out.write_all(&MAGIC).unwrap();
        out.write_all(&FRAME_VERSION.to_le_bytes()).unwrap();
        out.write_all(&(FIELD_LIMIT + 1).to_le_bytes()).unwrap();
        assert!(matches!(
            read_frame(&reader),
            Err(HandoverError::Malformed(_))
        ));

        let (writer, reader) = UnixStream::pair().unwrap();
        let mut out = &writer;
        out.write_all(&MAGIC).unwrap();
        out.write_all(&FRAME_VERSION.to_le_bytes()).unwrap();
        out.write_all(&0u64.to_le_bytes()).unwrap();
        out.write_all(&1u32.to_le_bytes()).unwrap();
        out.write_all(&[FD_MARK]).unwrap();
        assert!(matches!(
            read_frame(&reader),
            Err(HandoverError::Malformed(_))
        ));
    }

    /// The holder (in-process here) closes a peer of another uid without a
    /// byte, and the client refuses a holder of another uid; the limit ends
    /// it, and its socket goes with it.
    #[test]
    fn a_peer_of_another_uid_is_refused() {
        let (root, dir) = scratch("peer");
        let (spawner, theirs) = UnixStream::pair().unwrap();
        let config = HoldConfig {
            dirs: vec![dir.clone()],
            parent: std::process::id(),
            // Stands in for a client of another user.
            uid: uid() ^ 1,
            limit: Some(Duration::from_millis(1500)),
            buffer_limit: BUFFER_LIMIT,
        };
        let holder = std::thread::spawn(move || hold(theirs, &config));
        let (fd, _kept) = carried();
        let bundle = Bundle {
            layout: Vec::new(),
            panes: vec![HeldPane::new(tab(ID), 1, 1, Vec::new(), Vec::new(), fd)],
        };
        write_frame(&spawner, &bundle).unwrap();
        drop(bundle);
        let mut ready = [0u8];
        spawner.set_read_timeout(Some(HAND_WAIT)).unwrap();
        (&spawner).read_exact(&mut ready).unwrap();
        assert_eq!(ready[0], READY);
        assert!(matches!(take(&dir, uid() ^ 1), Err(HandoverError::Peer)));
        match take(&dir, uid()) {
            Err(HandoverError::Io(error)) => {
                // Closed unanswered; Linux resets, the `TAKE` being unread.
                assert!(
                    matches!(
                        error.kind(),
                        io::ErrorKind::UnexpectedEof | io::ErrorKind::ConnectionReset
                    ),
                    "{error:?}"
                );
            }
            other => panic!("the holder answered a stranger: {other:?}"),
        }
        assert_eq!(holder.join().unwrap(), HoldEnd::Limit);
        assert!(!dir.join(HANDOVER_SOCKET).exists(), "the socket stayed");
        std::fs::remove_dir_all(&root).unwrap();
    }

    // ── the real process ─────────────────────────────────────────────────

    /// The environment that turns this test binary into a holder
    /// ([`holder_process`]): `fd`, directory and limit in milliseconds.
    const HOLDER_ENV: &str = "BT_TEST_HOLDER";

    /// Not a test of its own: the entry of the holder the tests spawn (this
    /// binary again, run with `--exact` and [`HOLDER_ENV`]). Without the
    /// variable it does nothing.
    #[test]
    fn holder_process() {
        let Ok(spec) = std::env::var(HOLDER_ENV) else {
            return;
        };
        let parts: Vec<&str> = spec.split('\u{1f}').collect();
        let [fd, limit, dirs @ ..] = &parts[..] else {
            std::process::exit(EXIT_USAGE);
        };
        let mut args = vec!["--fd".to_owned(), (*fd).to_owned()];
        for dir in dirs {
            args.extend(["--dir".to_owned(), (*dir).to_owned()]);
        }
        let limit = Duration::from_millis(limit.parse().unwrap_or(0));
        let buffer = std::env::var(BUFFER_ENV)
            .ok()
            .and_then(|buffer| buffer.parse().ok())
            .unwrap_or(BUFFER_LIMIT);
        std::process::exit(hold_process(&args, Some(limit), buffer));
    }

    /// The buffer limit of a holder the tests spawn ([`holder_process`]).
    const BUFFER_ENV: &str = "BT_TEST_HOLDER_BUFFER";

    /// Spawns a holder process for `dirs` (this test binary,
    /// [`holder_process`]) with `limit` and a buffer limit; returns it and
    /// the spawner's end of its `socketpair`.
    fn spawn_harness(dirs: &[&Path], limit: Duration, buffer: usize) -> (Child, UnixStream) {
        let (ours, theirs) = UnixStream::pair().unwrap();
        let raw = theirs.as_raw_fd();
        let mut spec = format!("3\u{1f}{}", limit.as_millis());
        for dir in dirs {
            spec.push_str(&format!("\u{1f}{}", dir.display()));
        }
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", "handover::tests::holder_process", "--nocapture"])
            .args(["--test-threads", "1", "-q"])
            .env(HOLDER_ENV, spec)
            .env(BUFFER_ENV, buffer.to_string())
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
        let child = command.spawn().expect("the holder did not start");
        drop(theirs);
        ours.set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        (child, ours)
    }

    /// Spawns a holder for `dirs` and gives it `bundle` ("the old side
    /// exits": its copies go); returns the holder and the spawner's end.
    fn start_holder(dirs: &[&Path], limit: Duration, bundle: Bundle) -> (Child, UnixStream) {
        let (child, ours) = spawn_harness(dirs, limit, BUFFER_LIMIT);
        write_frame(&ours, &bundle).unwrap();
        drop(bundle);
        (child, ours)
    }

    /// [`start_holder`] for one directory, returning once it said [`READY`].
    fn spawn_holder(dir: &Path, limit: Duration, bundle: Bundle) -> Child {
        let (child, ours) = start_holder(&[dir], limit, bundle);
        let mut ready = [0u8];
        (&ours)
            .read_exact(&mut ready)
            .expect("the holder never got ready");
        assert_eq!(ready[0], READY);
        child
    }

    /// `ptsname` is not reentrant.
    static PTSNAME: Mutex<()> = Mutex::new(());

    /// A PTY pair, both ends close-on-exec from birth (a copy in another
    /// test's child would defeat the hang-up); the master non-blocking, as
    /// bateri's is.
    fn open_pty() -> (OwnedFd, File) {
        let master = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NOCTTY)
            .open("/dev/ptmx")
            .unwrap();
        let fd = master.as_raw_fd();
        // SAFETY: `fd` is an open PTY master; the name is copied out under
        // the lock before anyone else can call `ptsname`.
        let name = unsafe {
            assert_eq!(libc::grantpt(fd), 0);
            assert_eq!(libc::unlockpt(fd), 0);
            let _guard = PTSNAME.lock().unwrap();
            std::ffi::CStr::from_ptr(libc::ptsname(fd))
                .to_string_lossy()
                .into_owned()
        };
        // SAFETY: as above; only the status flags change.
        unsafe {
            let flags = libc::fcntl(fd, libc::F_GETFL);
            libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
        }
        let slave = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NOCTTY)
            .open(name)
            .unwrap();
        (OwnedFd::from(master), slave)
    }

    /// Numbered lines, twenty a second.
    const TICKS: &str = "i=0; while :; do i=$((i+1)); echo tick $i; sleep 0.05; done";

    /// Numbered lines as fast as the shell prints them.
    const FLOOD: &str = "i=0; while :; do i=$((i+1)); echo tick $i; done";

    /// Nothing printed. A program that ends while output waits unread on a
    /// master someone keeps open (bateri here, which reads in life; a stray
    /// copy) cannot finish exiting — the terminal's close waits for the
    /// output to drain — so the tests that end a program that way keep it
    /// quiet.
    const QUIET: &str = "while :; do sleep 1; done";

    /// A shell on the PTY as its controlling terminal (so the master's last
    /// close hangs it up), printing numbered lines.
    fn spawn_shell(slave: File) -> Child {
        spawn_script(slave, TICKS)
    }

    /// [`spawn_shell`] running `script`, with nothing of this process but
    /// its terminal: the tests run side by side, and a descriptor another
    /// test opened in the moment before it was close-on-exec would stay open
    /// as long as this shell lives.
    fn spawn_script(slave: File, script: &str) -> Child {
        let mut command = Command::new("/bin/sh");
        command
            .args(["-c", script])
            .stdin(slave.try_clone().unwrap())
            .stdout(slave.try_clone().unwrap())
            .stderr(slave);
        // SAFETY: only async-signal-safe calls between fork and exec.
        unsafe {
            command.pre_exec(|| {
                libc::setsid();
                if libc::ioctl(0, libc::TIOCSCTTY as _, 0) != 0 {
                    return Err(io::Error::last_os_error());
                }
                for fd in 3..1024 {
                    libc::close(fd);
                }
                Ok(())
            });
        }
        command.spawn().expect("sh")
    }

    fn shell_pane(id: &str) -> (HeldPane, Child) {
        let (master, slave) = open_pty();
        let shell = spawn_shell(slave);
        let start = start_time(shell.id()).expect("start time");
        let pane = HeldPane::new(
            tab(id),
            shell.id(),
            start,
            b"blob".to_vec(),
            b"tail".to_vec(),
            master,
        );
        (pane, shell)
    }

    fn exited(child: &mut Child) -> bool {
        child.try_wait().unwrap().is_some()
    }

    /// The numbers of the complete `tick N` lines in `bytes`.
    fn ticks(bytes: &[u8]) -> Vec<u32> {
        let text = String::from_utf8_lossy(bytes);
        let mut lines: Vec<&str> = text.split("\r\n").collect();
        // The last piece is a line still being printed.
        lines.pop();
        lines
            .into_iter()
            .filter_map(|line| line.strip_prefix("tick ")?.parse().ok())
            .collect()
    }

    /// The acceptance of R3: the old side gives a live shell and exits; a
    /// client that leaves without an ACK leaves the holder waiting; the next
    /// one takes the master with the lines printed meanwhile, acknowledges,
    /// the holder exits — and the shell lives on, its output unbroken.
    #[test]
    fn a_shell_lives_through_the_holder() {
        let (root, dir) = scratch("live");
        let (pane, mut shell) = shell_pane(ID);
        let (pid, start) = (pane.pid, pane.start);
        let bundle = Bundle {
            layout: b"layout".to_vec(),
            panes: vec![pane],
        };
        let mut holder = spawn_holder(&dir, Duration::from_secs(20), bundle);
        assert_eq!(
            owner_of(&dir),
            Some(holder.id()),
            "the holder does not own the directory"
        );
        // Let the shell print into the holder's buffer.
        std::thread::sleep(Duration::from_millis(300));

        // A client that goes away without acknowledging.
        let (first, link) = take(&dir, uid()).expect("first take");
        assert_eq!(first.panes.len(), 1);
        drop((first, link));

        let (mut taken, link) = take(&dir, uid()).expect("second take");
        assert_eq!(taken.layout, b"layout");
        let pane = taken.panes.remove(0);
        assert_eq!((pane.tab.as_str(), pane.pid, pane.start), (ID, pid, start));
        assert_eq!(pane.blob, b"blob");
        assert!(
            pane.buffer.starts_with(b"tail"),
            "the frozen tail is not first"
        );
        assert!(!pane.ended);
        assert_eq!(
            ticks(&pane.buffer[4..]).first(),
            Some(&1),
            "the lines printed meanwhile are missing"
        );
        link.ack().expect("ack");
        wait_until("the holder did not exit after the ACK", || {
            exited(&mut holder)
        });
        assert!(!dir.join(HANDOVER_SOCKET).exists(), "the socket stayed");

        // The shell lives and its output continues where the buffer ended.
        let mut seen = pane.buffer[4..].to_vec();
        let last = *ticks(&seen).last().unwrap();
        wait_until("the shell stopped printing", || {
            let mut chunk = [0u8; 4096];
            // SAFETY: `chunk` belongs to this frame.
            let read = unsafe {
                libc::read(
                    pane.master.as_raw_fd(),
                    chunk.as_mut_ptr().cast(),
                    chunk.len(),
                )
            };
            if read > 0 {
                seen.extend_from_slice(&chunk[..read as usize]);
            }
            ticks(&seen).last().is_some_and(|&n| n > last + 3)
        });
        assert!(!exited(&mut shell), "the shell died in the handover");
        let numbers = ticks(&seen);
        assert!(
            numbers.windows(2).all(|pair| pair[1] == pair[0] + 1),
            "the output has a gap: {numbers:?}"
        );

        drop(pane);
        wait_until("the shell outlived its last master", || exited(&mut shell));
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// "Release" hangs a pane's shell up at once, "release all" the rest and
    /// ends the holder; the limit does the same with nobody connecting — no
    /// orphan either way.
    #[test]
    fn release_and_the_limit_hang_the_shells_up() {
        let (root, dir) = scratch("release");
        let (first, mut first_shell) = shell_pane(ID);
        let (second, mut second_shell) = shell_pane(OTHER);
        let bundle = Bundle {
            layout: Vec::new(),
            panes: vec![first, second],
        };
        let mut holder = spawn_holder(&dir, Duration::from_secs(20), bundle);
        let (mut taken, mut link) = take(&dir, uid()).expect("take");
        link.release(taken.panes.remove(0)).expect("release");
        wait_until("a released shell lives", || exited(&mut first_shell));
        assert!(
            !exited(&mut second_shell),
            "release took the other pane too"
        );
        link.release_all(taken).expect("release all");
        wait_until("release all left a shell", || exited(&mut second_shell));
        wait_until("the holder outlived release all", || exited(&mut holder));
        std::fs::remove_dir_all(&root).unwrap();

        let (root, dir) = scratch("limit");
        let (pane, mut shell) = shell_pane(ID);
        let bundle = Bundle {
            layout: Vec::new(),
            panes: vec![pane],
        };
        let mut holder = spawn_holder(&dir, Duration::from_millis(500), bundle);
        wait_until("the holder outlived its limit", || exited(&mut holder));
        wait_until("the limit left a shell", || exited(&mut shell));
        assert!(!dir.join(HANDOVER_SOCKET).exists(), "the socket stayed");
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// A connection that never says [`TAKE`] — a liveness probe, the
    /// sweep's — is closed unserved, and the holder serves the next client.
    #[test]
    fn a_probe_is_not_served() {
        let (root, dir) = scratch("probe");
        let (pane, mut shell) = shell_pane(ID);
        let bundle = Bundle {
            layout: Vec::new(),
            panes: vec![pane],
        };
        let mut holder = spawn_holder(&dir, Duration::from_secs(20), bundle);
        assert!(holder_listening(&dir));
        let mut probe = UnixStream::connect(dir.join(HANDOVER_SOCKET)).unwrap();
        probe.set_read_timeout(Some(HAND_WAIT)).unwrap();
        let mut got = Vec::new();
        probe.read_to_end(&mut got).unwrap();
        assert!(got.is_empty(), "a probe was served {} bytes", got.len());
        let (bundle, link) = take(&dir, uid()).expect("take after a probe");
        assert_eq!(bundle.panes.len(), 1);
        link.release_all(bundle).unwrap();
        wait_until("the holder outlived release all", || exited(&mut holder));
        wait_until("release all left a shell", || exited(&mut shell));
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// Every directory of the instance passes to the holder from its
    /// spawner; when one cannot be taken (a living stranger owns it) the
    /// holder gives back what it took, never says READY, and the shell —
    /// still the spawner's — is not hung up.
    #[test]
    fn the_holder_takes_every_directory_or_none() {
        let (root, first) = scratch("dirs");
        let second = prepare_instance(&root.join("b"), INSTANCE).unwrap();
        // Both directories taken.
        let (pane, mut shell) = shell_pane(ID);
        let bundle = Bundle {
            layout: Vec::new(),
            panes: vec![pane],
        };
        let (mut holder, ours) = start_holder(&[&first, &second], Duration::from_secs(20), bundle);
        let mut ready = [0u8];
        (&ours).read_exact(&mut ready).expect("ready");
        assert_eq!(owner_of(&first), Some(holder.id()));
        assert_eq!(owner_of(&second), Some(holder.id()));
        let (bundle, link) = take(&first, uid()).expect("take");
        assert_eq!(link.holder_pid(), holder.id());
        link.release_all(bundle).unwrap();
        wait_until("the holder outlived release all", || exited(&mut holder));
        wait_until("release all left a shell", || exited(&mut shell));

        // The second owned by a living stranger: nothing taken, no READY.
        std::fs::write(first.join("pid"), std::process::id().to_string()).unwrap();
        let mut stranger = Command::new("/bin/sleep").arg("30").spawn().unwrap();
        std::fs::write(second.join("pid"), stranger.id().to_string()).unwrap();
        let (master, slave) = open_pty();
        let mut shell = spawn_shell(slave);
        let start = start_time(shell.id()).unwrap();
        let kept = master.try_clone().unwrap();
        let bundle = Bundle {
            layout: Vec::new(),
            panes: vec![HeldPane::new(
                tab(ID),
                shell.id(),
                start,
                Vec::new(),
                Vec::new(),
                master,
            )],
        };
        let (mut holder, ours) = start_holder(&[&first, &second], Duration::from_secs(20), bundle);
        let mut ready = [0u8];
        assert!(
            (&ours).read_exact(&mut ready).is_err(),
            "a failed holder said READY"
        );
        wait_until("a failed holder lives", || exited(&mut holder));
        assert_eq!(
            owner_of(&first),
            Some(std::process::id()),
            "the first was not given back"
        );
        assert_eq!(owner_of(&second), Some(stranger.id()));
        assert!(!exited(&mut shell), "the spawner's shell was hung up");
        drop(kept);
        wait_until("the shell outlived its last master", || exited(&mut shell));
        let _ = stranger.kill();
        let _ = stranger.wait();
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn the_argv_is_exact() {
        let args = |list: &[&str]| list.iter().map(|&s| s.to_owned()).collect::<Vec<_>>();
        assert_eq!(
            parse_args(&args(&["--fd", "3", "--dir", "/tmp/x"])),
            Some((3, vec![PathBuf::from("/tmp/x")]))
        );
        assert_eq!(
            parse_args(&args(&["--fd", "4", "--dir", "/a", "--dir", "/b"])),
            Some((4, vec![PathBuf::from("/a"), PathBuf::from("/b")]))
        );
        for wrong in [
            &["--fd", "3", "--dir", "/a", "--dir"][..],
            &["--fd", "3", "--dir", "/a", "--fd", "/b"],
            &["--fd", "1", "--dir", "/tmp/x"][..],
            &["--fd", "x", "--dir", "/tmp/x"],
            &["--fd", "3", "--dir", "relative"],
            &["--dir", "/tmp/x", "--fd", "3"],
            &["--fd", "3"],
        ] {
            assert_eq!(parse_args(&args(wrong)), None, "{wrong:?}");
        }
    }

    fn state() -> PaneState {
        PaneState {
            cols: 80,
            rows: 24,
            parent: ShellParent::Login,
            vt: b"\x1b[?1049hvt".to_vec(),
            core: b"core".to_vec(),
            input: b"ls\r".to_vec(),
            history: b"history".to_vec(),
        }
    }

    /// The pane blob round-trips every field and refuses anything else: a
    /// cut at any length, a byte past the end, another version, an unknown
    /// flag — a half-read blob would replay a wrong screen.
    #[test]
    fn a_pane_state_round_trips_and_refuses_damage() {
        let state = state();
        let bytes = state.encode();
        assert_eq!(PaneState::decode(&bytes), Some(state.clone()));
        let direct = PaneState {
            parent: ShellParent::Direct,
            history: Vec::new(),
            ..state
        };
        assert_eq!(PaneState::decode(&direct.encode()), Some(direct));
        for cut in 0..bytes.len() {
            assert_eq!(PaneState::decode(&bytes[..cut]), None, "cut at {cut}");
        }
        let mut longer = bytes.clone();
        longer.push(0);
        assert_eq!(PaneState::decode(&longer), None);
        let mut version = bytes.clone();
        version[4] = 2;
        assert_eq!(PaneState::decode(&version), None);
        let mut flags = bytes.clone();
        flags[12] = 2;
        assert_eq!(PaneState::decode(&flags), None);
    }

    /// The layout blob names the bundle that wrote it: a dev package's
    /// holder is not another bundle's to take — not even when its version
    /// is one this bateri cannot read.
    #[test]
    fn a_layout_blob_names_its_bundle() {
        assert_eq!(
            layout_bundle(b"bateri-handover 2 dev.bateri.bateri\nx"),
            Some("dev.bateri.bateri")
        );
        assert_eq!(layout_bundle(b"bateri-handover 2 \nx"), None);
        assert_eq!(layout_bundle(b"something 1 a\nx"), None);
        let blob = layout_blob("dev.bateri.bateri", "bateri-session 1\nW x\n");
        assert_eq!(
            layout_of(&blob),
            Some(("dev.bateri.bateri", "bateri-session 1\nW x\n"))
        );
        for broken in [
            &b"bateri-handover 2 dev.bateri.bateri\nx"[..],
            b"bateri-handover 1 \nx",
            b"bateri-handover 1 a b\nx",
            b"something 1 a\nx",
            b"bateri-handover 1 a",
            b"\xff\n",
        ] {
            assert_eq!(layout_of(broken), None, "{broken:?}");
        }
    }

    /// The new bateri takes its own bundle's holder and the instance's
    /// directory with it; another bundle's holder is let go untouched — it
    /// still listens and still owns its directory.
    #[test]
    fn arrival_takes_only_its_own_bundles_holder() {
        let (root, dir) = scratch("arrive");
        let (fd, kept) = carried();
        let bundle = Bundle {
            layout: layout_blob("dev.bateri.test", "the layout"),
            panes: vec![HeldPane::new(
                tab(ID),
                1,
                2,
                b"blob".to_vec(),
                b"buf".to_vec(),
                fd,
            )],
        };
        let mut holder = spawn_holder(&dir, Duration::from_secs(20), bundle);
        let me = std::process::id();

        assert!(arrive(&[root.clone()], uid(), me, "dev.bateri.other", true).is_none());
        assert_eq!(
            owner_of(&dir),
            Some(holder.id()),
            "another bundle took the directory"
        );
        assert!(
            holder_listening(&dir),
            "another bundle's arrival ended the holder"
        );

        let mut arrival =
            arrive(&[root.clone()], uid(), me, "dev.bateri.test", true).expect("an arrival");
        assert_eq!(arrival.holders[0].layout, "the layout");
        assert_eq!(owner_of(&dir), Some(me), "the directory was not adopted");
        // The instance's name comes with it: the registry runs on in it.
        assert_eq!(
            Some(arrival.instance.as_str()),
            dir.file_name().and_then(|name| name.to_str())
        );
        let (link, pane) = arrival.panes.remove(0);
        assert_eq!((link, pane.pid, pane.start), (0, 1, 2));
        assert_eq!(
            (pane.blob.as_slice(), pane.buffer.as_slice()),
            (&b"blob"[..], &b"buf"[..])
        );
        proves_alive(&pane.master, &kept, b"after");
        arrival.panes.push((link, pane));
        arrival.finish();
        wait_until("the holder did not exit after the ACK", || {
            exited(&mut holder)
        });
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// A launch that took a holder's programs and never settled is counted
    /// in the holder's directory before the panes are read: the next launch
    /// sees one attempt. Another bundle's arrival counts nothing.
    #[test]
    fn an_unsettled_arrival_is_counted_in_the_holders_directory() {
        let (root, dir) = scratch("attempt");
        let (fd, _kept) = carried();
        let bundle = Bundle {
            layout: layout_blob("dev.bateri.test", "the layout"),
            panes: vec![HeldPane::new(
                tab(ID),
                1,
                2,
                b"blob".to_vec(),
                Vec::new(),
                fd,
            )],
        };
        let mut holder = spawn_holder(&dir, Duration::from_secs(20), bundle);
        let me = std::process::id();
        assert!(arrive(&[root.clone()], uid(), me, "dev.bateri.other", true).is_none());
        let first =
            arrive(&[root.clone()], uid(), me, "dev.bateri.test", true).expect("an arrival");
        assert_eq!(
            (first.attempt, first.marked.as_slice()),
            (0, &[dir.clone()][..])
        );
        // Gone without an acknowledgement: the holder waits for the next.
        drop(first);
        let second =
            arrive(&[root.clone()], uid(), me, "dev.bateri.test", true).expect("an arrival");
        assert_eq!(second.attempt, 1, "the first attempt was not counted");
        second.finish();
        wait_until("the holder did not exit after the ACK", || {
            exited(&mut holder)
        });
        assert_eq!(restore::bump_attempt(&dir), 2);
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// A holder killed with `-9` leaves its socket behind: nothing is said
    /// about it, and a bound one's is removed once its pid is gone — while a
    /// live pid's (a holder between its `bind` and its `listen`) stays.
    #[test]
    fn a_dead_holders_socket_is_passed_over_and_removed() {
        let (root, dir) = scratch("stale");
        let mut gone = std::process::Command::new("/usr/bin/true")
            .spawn()
            .expect("a child");
        let dead = gone.id();
        gone.wait().expect("the child exits");
        let stale = dir.join(bound_socket(dead));
        let live = dir.join(bound_socket(std::process::id()));
        drop(UnixListener::bind(&stale).unwrap());
        drop(UnixListener::bind(&live).unwrap());
        assert!(arrive(&[root.clone()], uid(), 1, "dev.bateri.test", true).is_none());
        assert!(!stale.exists(), "the dead holder's socket stayed");
        assert!(live.exists(), "a live pid's socket was removed");
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// A stranger's frame is let go after its layout, before its panes,
    /// without a word; a frame of this bundle that breaks after the layout is
    /// released — a holder without a time limit would otherwise keep its
    /// programs forever.
    #[test]
    fn arrival_releases_only_its_own_broken_frame() {
        let (root, dir) = scratch("broken");
        let listener = UnixListener::bind(dir.join(HANDOVER_SOCKET)).unwrap();
        let holder = std::thread::spawn(move || {
            let mut answers = Vec::new();
            for owner in ["dev.bateri.other", "dev.bateri.test"] {
                let layout = layout_blob(owner, "the layout");
                let (mut stream, _) = listener.accept().unwrap();
                let mut take = [0u8];
                stream.read_exact(&mut take).unwrap();
                stream.write_all(&MAGIC).unwrap();
                stream.write_all(&FRAME_VERSION.to_le_bytes()).unwrap();
                stream
                    .write_all(&(layout.len() as u64).to_le_bytes())
                    .unwrap();
                stream.write_all(&layout).unwrap();
                // A pane count past the limit.
                stream.write_all(&(PANE_LIMIT + 1).to_le_bytes()).unwrap();
                stream.set_read_timeout(Some(HAND_WAIT)).unwrap();
                let mut answer = [0u8];
                let read = stream.read(&mut answer).unwrap_or(0);
                answers.push(answer[..read].to_vec());
            }
            answers
        });
        let me = std::process::id();
        for _ in 0..2 {
            assert!(
                arrive(
                    std::slice::from_ref(&root),
                    uid(),
                    me,
                    "dev.bateri.test",
                    true
                )
                .is_none()
            );
        }
        assert_eq!(holder.join().unwrap(), [vec![], vec![RELEASE_ALL]]);
        assert_eq!(owner_of(&dir), Some(me));
        // Nothing was taken, so nothing this launch counted can crash a
        // restore: the attempt it counted for its own bundle is gone.
        assert_eq!(restore::bump_attempt(&dir), 0);
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// A launch with ⇧ held does not ask the bound holders — their programs
    /// wait for the next one — but still takes the update's, which cannot.
    #[test]
    fn a_launch_without_the_bound_holders_still_takes_the_updates() {
        let (root, dir) = scratch("shift");
        let bound = UnixListener::bind(dir.join(bound_socket(std::process::id()))).unwrap();
        bound.set_nonblocking(true).unwrap();
        let (fd, _kept) = carried();
        let bundle = Bundle {
            layout: layout_blob("dev.bateri.test", "the layout"),
            panes: vec![HeldPane::new(
                tab(ID),
                1,
                2,
                b"blob".to_vec(),
                Vec::new(),
                fd,
            )],
        };
        let mut holder = spawn_holder(&dir, Duration::from_secs(20), bundle);
        let arrival = arrive(
            std::slice::from_ref(&root),
            uid(),
            std::process::id(),
            "dev.bateri.test",
            false,
        )
        .expect("the update's holder is taken");
        assert!(
            matches!(bound.accept(), Err(error) if error.kind() == io::ErrorKind::WouldBlock),
            "a bound holder was asked"
        );
        arrival.finish();
        wait_until("the holder did not exit after the ACK", || {
            exited(&mut holder)
        });
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// The holder's spawn passes fd 3 and nothing else of this process —
    /// a descriptor without close-on-exec included (macOS: the frozen
    /// masters are such; elsewhere the holder closes them itself).
    #[test]
    fn a_clean_spawn_passes_only_the_kept_descriptor() {
        let (ours, theirs) = UnixStream::pair().unwrap();
        // SAFETY: a plain open of a static path; checked.
        let stray = unsafe { libc::open(c"/dev/null".as_ptr(), libc::O_RDONLY) };
        assert!(stray > 2);
        // SAFETY: `stray` is open; descriptor flags only.
        unsafe { libc::fcntl(stray, libc::F_SETFD, 0) };
        let script =
            format!("if [ -e /dev/fd/{stray} ]; then echo leak >&3; else echo clean >&3; fi");
        let pid = spawn_clean(
            Path::new("/bin/sh"),
            &["-c".into(), script.into()],
            Some(std::os::fd::AsFd::as_fd(&theirs)),
        )
        .expect("spawn");
        drop(theirs);
        ours.set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let mut said = String::new();
        (&ours).read_to_string(&mut said).unwrap();
        // SAFETY: our own descriptor.
        unsafe { libc::close(stray) };
        let mut status = 0;
        // SAFETY: our own child.
        unsafe { libc::waitpid(i32::try_from(pid).unwrap(), &raw mut status, 0) };
        if cfg!(target_os = "macos") {
            assert_eq!(said, "clean\n");
        } else {
            assert!(said == "clean\n" || said == "leak\n", "{said:?}");
        }
    }

    /// The holder's socket goes to the first directory where it fits: a
    /// long home's cache directory is passed over for the short root.
    #[test]
    fn the_socket_goes_where_it_fits() {
        let long = PathBuf::from(format!("/{}", "x".repeat(120)));
        let short = PathBuf::from("/tmp/bateri-501/12345678");
        assert_eq!(
            socket_first(&[long.clone(), short.clone()]),
            Some(vec![short.clone(), long.clone()])
        );
        assert_eq!(
            socket_first(&[short.clone(), long.clone()]),
            Some(vec![short, long.clone()])
        );
        assert_eq!(socket_first(&[long]), None);
        assert_eq!(socket_first(&[]), None);
    }

    // ── the frame, byte for byte ─────────────────────────────────────────

    /// A frame as version 1 writes it — one ended pane — spelled out by hand,
    /// not by the writer.
    fn frame_fixture() -> Vec<u8> {
        [
            &b"BTHO"[..],
            &[1, 0, 0, 0],             // version
            &[1, 0, 0, 0, 0, 0, 0, 0], // layout length
            b"L",                      // layout
            &[1, 0, 0, 0],             // one pane
            b"F",                      // the master rides this byte
            &[36, 0, 0, 0],            // tab length
            ID.as_bytes(),             // tab
            &[42, 0, 0, 0],            // pid
            &[7, 0, 0, 0, 0, 0, 0, 0], // start
            &[1, 0, 0, 0],             // flags: ended
            &[2, 0, 0, 0, 0, 0, 0, 0], // blob length
            b"ab",                     // blob
            &[3, 0, 0, 0, 0, 0, 0, 0], // buffer length
            b"xyz",                    // buffer
        ]
        .concat()
    }

    /// Where the fixture's descriptor byte is: magic, version, the layout's
    /// length and byte, the count.
    const FIXTURE_MARK: usize = 4 + 4 + 8 + 1 + 4;

    /// Where the fixture's flags are: past the mark, the tab, pid and start.
    const FIXTURE_FLAGS: usize = FIXTURE_MARK + 1 + 4 + 36 + 4 + 8;

    /// The frame is version 1, byte for byte: written, a pane gives exactly
    /// the fixture (the descriptor travels beside the bytes); read, the
    /// fixture gives the pane back. The cut bit is bit 1, and a reader skips
    /// a bit it does not know.
    #[test]
    fn the_frame_is_version_one_byte_for_byte() {
        let expected = frame_fixture();
        assert_eq!(expected[FIXTURE_MARK], FD_MARK);
        assert_eq!(expected[FIXTURE_FLAGS], 1);

        let frame_of = |pane: HeldPane| {
            let (writer, reader) = UnixStream::pair().unwrap();
            write_frame(
                &writer,
                &Bundle {
                    layout: b"L".to_vec(),
                    panes: vec![pane],
                },
            )
            .unwrap();
            drop(writer);
            let mut got = Vec::new();
            (&reader).read_to_end(&mut got).unwrap();
            got
        };
        let (fd, _kept) = carried();
        let mut pane = HeldPane::new(tab(ID), 42, 7, b"ab".to_vec(), b"xyz".to_vec(), fd);
        pane.ended = true;
        assert_eq!(frame_of(pane), expected);
        let (fd, _kept) = carried();
        let mut pane = HeldPane::new(tab(ID), 42, 7, b"ab".to_vec(), b"xyz".to_vec(), fd);
        pane.cut = true;
        assert_eq!(
            frame_of(pane)[FIXTURE_FLAGS..FIXTURE_FLAGS + 4],
            [2, 0, 0, 0]
        );

        let read = |bytes: &[u8]| {
            let (writer, reader) = UnixStream::pair().unwrap();
            let (fd, kept) = carried();
            let mut out = &writer;
            out.write_all(&bytes[..FIXTURE_MARK]).unwrap();
            send_fd(&writer, fd.as_raw_fd()).unwrap();
            out.write_all(&bytes[FIXTURE_MARK + 1..]).unwrap();
            drop(fd);
            (read_frame(&reader).unwrap(), kept)
        };
        let (bundle, kept) = read(&expected);
        assert_eq!(bundle.layout, b"L");
        assert_eq!(bundle.panes.len(), 1);
        let pane = &bundle.panes[0];
        assert_eq!(
            (
                pane.tab.as_str(),
                pane.pid,
                pane.start,
                pane.ended,
                pane.cut
            ),
            (ID, 42, 7, true, false)
        );
        assert_eq!(
            (pane.blob.as_slice(), pane.buffer.as_slice()),
            (&b"ab"[..], &b"xyz"[..])
        );
        proves_alive(&pane.master, &kept, b"fixture");
        let mut unknown = expected.clone();
        unknown[FIXTURE_FLAGS..FIXTURE_FLAGS + 4].copy_from_slice(&0x8000_0003u32.to_le_bytes());
        let (bundle, _kept) = read(&unknown);
        assert!(bundle.panes[0].ended && bundle.panes[0].cut);
    }

    // ── the bound holder ─────────────────────────────────────────────────

    /// The build's identity names this image and does not change while it
    /// runs.
    #[test]
    fn the_build_id_names_the_image() {
        let id = build_id();
        assert_eq!(id, build_id());
        let (version, image) = id.split_once(' ').expect("version and image");
        assert_eq!(version, env!("CARGO_PKG_VERSION"));
        assert!(!image.is_empty(), "the image has no identity");
        if cfg!(target_os = "macos") {
            assert!(
                image.len() == 32 && image.bytes().all(|byte| byte.is_ascii_hexdigit()),
                "not a Mach-O UUID: {image}"
            );
        }
    }

    /// A holder of another build holds nothing and leaves no socket; one of
    /// this build shakes hands, and says goodbye quietly.
    #[test]
    fn a_bound_holder_shakes_hands_only_with_its_own_build() {
        let (root, dir) = scratch("build");
        let config = HoldConfig {
            dirs: vec![dir.clone()],
            parent: std::process::id(),
            uid: uid(),
            limit: Some(Duration::from_millis(1500)),
            buffer_limit: BUFFER_LIMIT,
        };
        let (spawner, theirs) = UnixStream::pair().unwrap();
        let refusing = {
            let config = config.clone();
            std::thread::spawn(move || hold(theirs, &config))
        };
        let mut out = &spawner;
        out.write_all(&BOUND_MAGIC).unwrap();
        out.write_all(&BOUND_VERSION.to_le_bytes()).unwrap();
        let other = b"0.0.0 another-build";
        out.write_all(&(other.len() as u32).to_le_bytes()).unwrap();
        out.write_all(other).unwrap();
        let mut answer = [0u8];
        spawner.set_read_timeout(Some(HAND_WAIT)).unwrap();
        (&spawner).read_exact(&mut answer).unwrap();
        assert_eq!(answer[0], REFUSED);
        assert_eq!(refusing.join().unwrap(), HoldEnd::Failed);
        assert!(
            holder_sockets(&dir).is_empty(),
            "a refusing holder left a socket"
        );

        let (ours, theirs) = UnixStream::pair().unwrap();
        let holding = std::thread::spawn(move || hold(theirs, &config));
        let bound =
            Bound::connect(ours, std::process::id(), false, initial(), || {}).expect("handshake");
        assert!(
            dir.join(bound_socket(std::process::id())).exists(),
            "no socket of its own"
        );
        assert!(bound.ping(HAND_WAIT));
        bound.quit();
        assert_eq!(holding.join().unwrap(), HoldEnd::Quit);
        assert!(holder_sockets(&dir).is_empty(), "the socket stayed");
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// A bound holder for `dir` in a process of its own: the harness and
    /// bateri's end.
    fn start_bound(dir: &Path, limit: Duration, buffer: usize) -> (Child, Bound) {
        let (child, ours) = spawn_harness(&[dir], limit, buffer);
        let bound = Bound::connect(ours, child.id(), false, initial(), || {}).expect("handshake");
        (child, bound)
    }

    /// The first layout a bound holder gets.
    fn initial() -> Vec<u8> {
        layout_blob("dev.bateri.test", "initial")
    }

    /// A live pane registered with `bound`: a shell running `script` on a PTY
    /// of 100 × 30, the master kept here (bateri's) and a copy registered.
    fn register(bound: &Bound, id: &str, script: &str, blob: &[u8]) -> (OwnedFd, Child) {
        let (master, slave) = open_pty();
        let size = libc::winsize {
            ws_row: 30,
            ws_col: 100,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        // SAFETY: `master` is open; `size` belongs to this frame.
        assert_eq!(
            unsafe { libc::ioctl(master.as_raw_fd(), libc::TIOCSWINSZ, &raw const size) },
            0
        );
        let shell = spawn_script(slave, script);
        bound.add(BoundPane {
            tab: tab(id),
            pid: shell.id(),
            start: start_time(shell.id()).expect("start time"),
            parent: ShellParent::Direct,
            blob: blob.to_vec(),
            master: master.try_clone().unwrap(),
            taken_from: None,
        });
        (master, shell)
    }

    /// A process that keeps a copy of `master` open — an ssh master that
    /// bateri spawned before the master was close-on-exec — and nothing
    /// else: a copy of another test's master would hold that test's program.
    fn stray_copy(master: &OwnedFd) -> Child {
        let raw = master.as_raw_fd();
        let mut command = Command::new("/bin/sleep");
        command
            .arg("30")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        // SAFETY: only async-signal-safe calls between fork and exec; `dup2`
        // leaves the copy open across the exec.
        unsafe {
            command.pre_exec(move || {
                if libc::dup2(raw, 10) < 0 {
                    return Err(io::Error::last_os_error());
                }
                for fd in (3..1024).filter(|&fd| fd != 10) {
                    libc::close(fd);
                }
                Ok(())
            });
        }
        command.spawn().expect("sleep")
    }

    /// Takes the bundle of the holder on `socket`.
    fn take_from(socket: &Path) -> (Bundle, Link) {
        open(socket, uid())
            .expect("open")
            .body()
            .map_err(|(error, _)| error)
            .expect("body")
    }

    /// The acceptance: bateri lets go without a word (its connection and its
    /// masters close); the shell lives on in the detached holder, which
    /// drains the output and gives the newest blob and layout — no screen —
    /// to the next client; after the ACK the holder is gone and the shell
    /// still lives. The spawner (this test) lives on, so its directory stays
    /// its own.
    #[test]
    fn a_shell_outlives_its_bateri_in_the_bound_holder() {
        let (root, dir) = scratch("bound");
        let (mut holder, bound) = start_bound(&dir, Duration::from_secs(20), BUFFER_LIMIT);
        let socket = dir.join(bound_socket(holder.id()));
        assert!(socket.exists(), "no socket of its own");
        let (master, mut shell) = register(&bound, ID, TICKS, b"first");
        let start = start_time(shell.id()).unwrap();
        bound.layout(b"old layout".to_vec());
        bound.layout(b"layout".to_vec());
        bound.state(&tab(ID), b"newer".to_vec());
        assert!(bound.ping(HAND_WAIT), "the holder does not answer");
        assert_eq!(
            owner_of(&dir),
            Some(std::process::id()),
            "a bound holder took the directory from a living bateri"
        );

        drop(bound);
        drop(master);
        // Let the shell print into the detached holder.
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(
            owner_of(&dir),
            Some(std::process::id()),
            "a living bateri's directory passed to the holder"
        );
        let (mut taken, link) = take_from(&socket);
        assert_eq!(taken.layout, b"layout");
        let pane = taken.panes.remove(0);
        assert_eq!(
            (
                pane.tab.as_str(),
                pane.pid,
                pane.start,
                pane.ended,
                pane.cut
            ),
            (ID, shell.id(), start, false, false)
        );
        let state = PaneState::decode(&pane.blob).expect("a pane state");
        assert_eq!(
            state,
            PaneState {
                cols: 100,
                rows: 30,
                parent: ShellParent::Direct,
                vt: Vec::new(),
                core: b"newer".to_vec(),
                input: Vec::new(),
                history: Vec::new(),
            }
        );
        assert!(
            !ticks(&pane.buffer).is_empty(),
            "the detached holder did not drain"
        );
        link.ack().expect("ack");
        wait_until("the holder did not exit after the ACK", || {
            exited(&mut holder)
        });
        assert!(!socket.exists(), "the socket stayed");
        assert!(!exited(&mut shell), "the shell died with its bateri");
        drop(pane);
        wait_until("the shell outlived its last master", || exited(&mut shell));
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// A bound holder closes a client without a byte while its bateri
    /// lives — and serves the first one after its bateri died, even when the
    /// end of the connection waits unread as the client comes.
    #[test]
    fn a_bound_holder_serves_only_after_its_bateri() {
        let (root, dir) = scratch("decline");
        let (mut holder, bound) = start_bound(&dir, Duration::from_secs(20), BUFFER_LIMIT);
        let socket = dir.join(bound_socket(holder.id()));
        let (master, mut shell) = register(&bound, ID, TICKS, b"blob");
        assert!(bound.ping(HAND_WAIT));
        assert!(
            matches!(open(&socket, uid()), Err(HandoverError::Declined)),
            "a bound holder served a take"
        );
        assert!(holder_listening(&dir));
        assert!(
            bound.ping(HAND_WAIT),
            "a declined take broke the connection"
        );

        // A client the holder accepted while bound, whose `TAKE` comes only
        // after bateri died: the end on the connection is read first.
        let client = UnixStream::connect(&socket).unwrap();
        std::thread::sleep(Duration::from_millis(200));
        drop(bound);
        drop(master);
        let wire = Wire::new(&client, None);
        wire.write_all(&[TAKE]).unwrap();
        let bundle = read_header(&wire)
            .and_then(|()| read_body(&wire))
            .expect("the holder declined right after the crash");
        assert_eq!(bundle.panes.len(), 1);
        let link = Link {
            stream: client,
            holder: holder.id(),
            socket: socket.clone(),
        };
        link.release_all(bundle).unwrap();
        wait_until("the holder outlived release all", || exited(&mut holder));
        wait_until("release all left the shell", || exited(&mut shell));
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// The deliberate handover: the frame over the live connection replaces
    /// the registrations — a registered pane it does not carry loses the
    /// holder's copy (bateri closes it its own way) — and the holder serves
    /// the frame's panes, detached.
    #[test]
    fn a_handover_replaces_the_registrations() {
        let (root, dir) = scratch("handover");
        let (mut holder, bound) = start_bound(&dir, Duration::from_secs(20), BUFFER_LIMIT);
        let socket = dir.join(bound_socket(holder.id()));
        let (carried_master, mut carried_shell) = register(&bound, ID, TICKS, b"a");
        let (left_master, mut left_shell) = register(&bound, OTHER, TICKS, b"b");
        assert!(bound.ping(HAND_WAIT));
        let start = start_time(carried_shell.id()).unwrap();
        let bundle = Bundle {
            layout: b"frozen".to_vec(),
            panes: vec![HeldPane::new(
                tab(ID),
                carried_shell.id(),
                start,
                b"frozen blob".to_vec(),
                b"tail".to_vec(),
                carried_master.try_clone().unwrap(),
            )],
        };
        bound.hand_over(bundle).expect("handover");
        assert_eq!(owner_of(&dir), Some(holder.id()));
        // bateri exits: its masters close.
        drop(carried_master);
        drop(left_master);
        wait_until("the holder kept a pane the frame did not carry", || {
            exited(&mut left_shell)
        });
        let (mut taken, link) = take_from(&socket);
        assert_eq!(taken.layout, b"frozen");
        assert_eq!(taken.panes.len(), 1);
        let pane = taken.panes.remove(0);
        assert_eq!(pane.blob, b"frozen blob");
        assert!(pane.buffer.starts_with(b"tail"));
        link.ack().expect("ack");
        wait_until("the holder did not exit after the ACK", || {
            exited(&mut holder)
        });
        assert!(
            !exited(&mut carried_shell),
            "the handover hung the shell up"
        );
        drop(pane);
        wait_until("the shell outlived its last master", || {
            exited(&mut carried_shell)
        });
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// A program that exits while bound loses the holder's copy at once —
    /// no release needed: a crash after it gives only the survivor.
    #[test]
    fn an_exited_program_leaves_the_bound_holder() {
        let (root, dir) = scratch("exit");
        let (mut holder, bound) = start_bound(&dir, Duration::from_secs(20), BUFFER_LIMIT);
        let socket = dir.join(bound_socket(holder.id()));
        let (gone_master, mut gone_shell) = register(&bound, ID, QUIET, b"a");
        let (live_master, mut live_shell) = register(&bound, OTHER, TICKS, b"b");
        assert!(bound.ping(HAND_WAIT));
        gone_shell.kill().unwrap();
        gone_shell.wait().unwrap();
        // Answered after the exit was seen.
        assert!(bound.ping(HAND_WAIT));
        drop(bound);
        drop(gone_master);
        drop(live_master);
        let (bundle, link) = take_from(&socket);
        let tabs: Vec<&str> = bundle.panes.iter().map(|pane| pane.tab.as_str()).collect();
        assert_eq!(tabs, [OTHER], "the exited program's copy stayed");
        link.release_all(bundle).unwrap();
        wait_until("the holder outlived release all", || exited(&mut holder));
        wait_until("release all left the shell", || exited(&mut live_shell));
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// The quiet exit closes the holder's copies and touches no program.
    #[test]
    fn a_quiet_exit_touches_no_program() {
        let (root, dir) = scratch("quiet");
        let (mut holder, bound) = start_bound(&dir, Duration::from_secs(20), BUFFER_LIMIT);
        let socket = dir.join(bound_socket(holder.id()));
        let (master, mut shell) = register(&bound, ID, QUIET, b"blob");
        let mut stray = stray_copy(&master);
        assert!(bound.ping(HAND_WAIT));
        bound.quit();
        wait_until("the holder outlived its goodbye", || exited(&mut holder));
        assert_eq!(holder.wait().unwrap().code(), Some(EXIT_DONE));
        assert!(!socket.exists(), "the socket stayed");
        // A signal would have ended it by now, the stray copy notwithstanding.
        std::thread::sleep(Duration::from_millis(300));
        assert!(!exited(&mut shell), "a quiet exit hung the shell up");
        drop(master);
        let _ = stray.kill();
        let _ = stray.wait();
        wait_until("the shell outlived its last master", || exited(&mut shell));
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// A holder that dies under a living bateri is reported once.
    #[test]
    fn a_holder_that_dies_is_reported() {
        let (root, dir) = scratch("death");
        let (mut holder, ours) =
            spawn_harness(&[dir.as_path()], Duration::from_secs(20), BUFFER_LIMIT);
        let died = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let bound = {
            let died = Arc::clone(&died);
            Bound::connect(ours, holder.id(), false, initial(), move || {
                died.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            })
            .expect("handshake")
        };
        assert!(bound.alive() && bound.ping(HAND_WAIT));
        holder.kill().unwrap();
        holder.wait().unwrap();
        wait_until("the death was not reported", || {
            died.load(std::sync::atomic::Ordering::SeqCst) == 1
        });
        assert!(!bound.alive() && !bound.ping(Duration::from_millis(200)));
        drop(bound);
        assert_eq!(died.load(std::sync::atomic::Ordering::SeqCst), 1);
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// A pane the holder cannot keep — its program is already gone — is
    /// refused out loud.
    #[test]
    fn a_pane_the_holder_cannot_keep_is_refused() {
        let (root, dir) = scratch("refuse");
        let (mut holder, bound) = start_bound(&dir, Duration::from_secs(20), BUFFER_LIMIT);
        let mut gone = Command::new("/bin/sh")
            .args(["-c", "exit 0"])
            .spawn()
            .unwrap();
        let pid = gone.id();
        gone.wait().unwrap();
        let (master, _slave) = open_pty();
        bound.add(BoundPane {
            tab: tab(OTHER),
            pid,
            start: 1,
            parent: ShellParent::Login,
            blob: Vec::new(),
            master,
            taken_from: None,
        });
        assert!(bound.ping(HAND_WAIT));
        let refused: Vec<String> = bound
            .refused()
            .iter()
            .map(|tab| tab.as_str().to_owned())
            .collect();
        assert_eq!(refused, [OTHER]);
        bound.quit();
        wait_until("the holder outlived its goodbye", || exited(&mut holder));
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// A detached holder never stops reading: past its limit the oldest
    /// bytes go, the pane is marked cut and the newest output is whole — and
    /// the buffer keeps moving on, where stopping at the limit would freeze
    /// it and the program with it.
    #[test]
    fn a_detached_holder_never_stops_reading() {
        let (root, dir) = scratch("ring");
        let (mut holder, bound) = start_bound(&dir, Duration::from_secs(20), 4096);
        let socket = dir.join(bound_socket(holder.id()));
        let (master, mut shell) = register(&bound, ID, FLOOD, b"blob");
        assert!(bound.ping(HAND_WAIT));
        drop(bound);
        drop(master);
        let newest = || {
            std::thread::sleep(Duration::from_millis(300));
            let (mut taken, link) = take_from(&socket);
            let pane = taken.panes.remove(0);
            assert!(pane.cut, "the cut bit is not set");
            assert!(
                pane.buffer.len() <= 4096,
                "{} bytes kept",
                pane.buffer.len()
            );
            let numbers = ticks(&pane.buffer);
            assert!(
                numbers.first().is_some_and(|&first| first > 1),
                "the oldest bytes stayed: {:?}",
                numbers.first()
            );
            assert!(
                numbers.windows(2).all(|pair| pair[1] == pair[0] + 1),
                "the newest output has a gap"
            );
            // Going away without an ACK: the holder holds on.
            drop((pane, taken, link));
            *numbers.last().unwrap()
        };
        let first = newest();
        let second = newest();
        assert!(
            second > first,
            "the holder stopped reading at its limit: {first} then {second}"
        );
        let (bundle, link) = take_from(&socket);
        link.release_all(bundle).unwrap();
        wait_until("the holder outlived release all", || exited(&mut holder));
        wait_until("release all left the shell", || exited(&mut shell));
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// "Nobody took these programs" signals them: release all and the limit
    /// end a shell even while another process keeps a copy of its master.
    #[test]
    fn release_all_and_the_limit_signal_the_programs() {
        let (root, dir) = scratch("signal");
        let (mut holder, bound) = start_bound(&dir, Duration::from_secs(20), BUFFER_LIMIT);
        let socket = dir.join(bound_socket(holder.id()));
        let (master, mut shell) = register(&bound, ID, QUIET, b"blob");
        let mut stray = stray_copy(&master);
        assert!(bound.ping(HAND_WAIT));
        drop(bound);
        drop(master);
        let (bundle, link) = take_from(&socket);
        link.release_all(bundle).unwrap();
        wait_until("release all left a shell beside a stray copy", || {
            exited(&mut shell)
        });
        wait_until("the holder outlived release all", || exited(&mut holder));
        let _ = stray.kill();
        let _ = stray.wait();

        let (mut holder, bound) = start_bound(&dir, Duration::from_millis(600), BUFFER_LIMIT);
        let (master, mut shell) = register(&bound, ID, QUIET, b"blob");
        let mut stray = stray_copy(&master);
        assert!(bound.ping(HAND_WAIT));
        drop(bound);
        drop(master);
        wait_until("the holder outlived its limit", || exited(&mut holder));
        wait_until("the limit left a shell beside a stray copy", || {
            exited(&mut shell)
        });
        let _ = stray.kill();
        let _ = stray.wait();
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// The arrival's two releases: a duplicate's ([`Link::release`]) closes
    /// only the copies — another holder carries the program — and a pane that
    /// cannot be placed ([`Arrival::release`]) is signalled.
    #[test]
    fn a_duplicate_is_released_quietly_and_a_refused_pane_is_signalled() {
        let (root, dir) = scratch("releases");
        let (mut holder, bound) = start_bound(&dir, Duration::from_secs(20), BUFFER_LIMIT);
        bound.layout(layout_blob("dev.bateri.test", "the layout"));
        let (quiet_master, mut quiet_shell) = register(&bound, ID, QUIET, b"a");
        let (loud_master, mut loud_shell) = register(&bound, OTHER, QUIET, b"b");
        let mut strays = [stray_copy(&quiet_master), stray_copy(&loud_master)];
        assert!(bound.ping(HAND_WAIT));
        drop(bound);
        drop(quiet_master);
        drop(loud_master);
        let mut arrival = arrive(
            std::slice::from_ref(&root),
            uid(),
            std::process::id(),
            "dev.bateri.test",
            true,
        )
        .expect("an arrival");
        assert_eq!(arrival.holders.len(), 1);
        assert!(is_bound_socket(&arrival.holders[0].socket));
        assert_eq!(arrival.holders[0].layout, "the layout");
        let take = |arrival: &mut Arrival, id: &str| {
            let index = arrival
                .panes
                .iter()
                .position(|(_, pane)| pane.tab.as_str() == id)
                .unwrap();
            arrival.panes.remove(index)
        };
        let (link, quiet) = take(&mut arrival, ID);
        arrival.links[link].release(quiet).unwrap();
        let (link, loud) = take(&mut arrival, OTHER);
        arrival.release(link, loud);
        wait_until("a refused pane's program lives beside a stray copy", || {
            exited(&mut loud_shell)
        });
        assert!(
            !exited(&mut quiet_shell),
            "a duplicate's release hung its program up"
        );
        arrival.finish();
        wait_until("the holder did not exit after the ACK", || {
            exited(&mut holder)
        });
        let _ = quiet_shell.kill();
        let _ = quiet_shell.wait();
        for stray in &mut strays {
            let _ = stray.kill();
            let _ = stray.wait();
        }
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// Two holders in one directory, as a crash in the moment before an
    /// acknowledgement leaves them: the older (its bateri crashed) carries
    /// `ID`; the younger (the next bateri took `ID`, registered it
    /// unconfirmed beside a pane of its own, `OTHER`, and crashed before the
    /// ACK) carries both.
    struct TwoHolders {
        older: Child,
        older_socket: PathBuf,
        younger: Child,
        younger_socket: PathBuf,
        shared_shell: Child,
        own_shell: Child,
    }

    fn two_holders(dir: &Path) -> TwoHolders {
        let (older, bound) = start_bound(dir, Duration::from_secs(20), BUFFER_LIMIT);
        let older_socket = dir.join(bound_socket(older.id()));
        bound.layout(layout_blob("dev.bateri.test", "older layout"));
        let (master, shared_shell) = register(&bound, ID, TICKS, b"older");
        assert!(bound.ping(HAND_WAIT));
        drop(bound);
        drop(master);

        let (mut bundle, link) = take_from(&older_socket);
        let (younger, bound) = start_bound(dir, Duration::from_secs(20), BUFFER_LIMIT);
        let younger_socket = dir.join(bound_socket(younger.id()));
        bound.layout(layout_blob("dev.bateri.test", "younger layout"));
        let taken = bundle.panes.remove(0);
        bound.add(BoundPane {
            tab: taken.tab.clone(),
            pid: taken.pid,
            start: taken.start,
            parent: ShellParent::Direct,
            blob: b"younger".to_vec(),
            master: taken.master.try_clone().unwrap(),
            taken_from: Some(older_socket.clone()),
        });
        let (own_master, own_shell) = register(&bound, OTHER, TICKS, b"own");
        assert!(bound.ping(HAND_WAIT));
        drop((link, taken, bundle, bound, own_master));
        assert!(
            socket_listening(&older_socket) && socket_listening(&younger_socket),
            "two holders in one directory got in each other's way"
        );
        TwoHolders {
            older,
            older_socket,
            younger,
            younger_socket,
            shared_shell,
            own_shell,
        }
    }

    /// The tab's buffer in `bundle`.
    fn buffer_of(bundle: &Bundle, id: &str) -> Vec<u8> {
        bundle
            .panes
            .iter()
            .find(|pane| pane.tab.as_str() == id)
            .map(|pane| pane.buffer.clone())
            .unwrap()
    }

    /// Neither of two holders in one directory blocks the other; the younger
    /// leaves the program it registered unconfirmed to the older while the
    /// older listens; the arrival gives that program once, from the older.
    #[test]
    fn two_holders_share_a_directory_and_the_older_wins() {
        let (root, dir) = scratch("two");
        let TwoHolders {
            mut older,
            older_socket,
            mut younger,
            younger_socket,
            mut shared_shell,
            mut own_shell,
        } = two_holders(&dir);
        std::thread::sleep(Duration::from_millis(300));

        let (peek, peek_link) = take_from(&younger_socket);
        assert!(
            buffer_of(&peek, ID).is_empty(),
            "two holders drained one master"
        );
        assert!(!ticks(&buffer_of(&peek, OTHER)).is_empty());
        drop((peek, peek_link));

        let mut arrival = arrive(
            std::slice::from_ref(&root),
            uid(),
            std::process::id(),
            "dev.bateri.test",
            true,
        )
        .expect("an arrival");
        assert_eq!(arrival.holders.len(), 2);
        assert_eq!(arrival.holders[0].layout, "younger layout");
        let shared: Vec<usize> = (0..arrival.panes.len())
            .filter(|&i| arrival.panes[i].1.tab.as_str() == ID)
            .collect();
        assert_eq!(shared.len(), 1, "a program came twice");
        let (link, shared_pane) = arrival.panes.remove(shared[0]);
        assert_eq!(
            arrival.holders[link].socket, older_socket,
            "the younger copy won"
        );
        assert!(
            !ticks(&shared_pane.buffer).is_empty(),
            "the older holder's output is missing"
        );
        let (link, own_pane) = arrival.panes.remove(0);
        assert_eq!(own_pane.tab.as_str(), OTHER);
        assert_eq!(arrival.holders[link].socket, younger_socket);
        arrival.finish();
        wait_until("the older holder outlived the ACK", || exited(&mut older));
        wait_until("the younger holder outlived the ACK", || {
            exited(&mut younger)
        });
        assert!(
            !exited(&mut shared_shell),
            "the dropped duplicate hung its program up"
        );
        assert!(!exited(&mut own_shell));
        drop(shared_pane);
        drop(own_pane);
        wait_until("the shell outlived its last master", || {
            exited(&mut shared_shell)
        });
        wait_until("the shell outlived its last master", || {
            exited(&mut own_shell)
        });
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// "Release all" to the younger of two holders (its frame broke, say)
    /// ends only its own program: the one it carries unconfirmed is the
    /// older holder's, which still listens.
    #[test]
    fn a_younger_holders_release_spares_the_older_holders_program() {
        let (root, dir) = scratch("spare");
        let TwoHolders {
            mut older,
            older_socket,
            mut younger,
            younger_socket,
            mut shared_shell,
            mut own_shell,
        } = two_holders(&dir);
        let (bundle, link) = take_from(&younger_socket);
        link.release_all(bundle).unwrap();
        wait_until("the younger holder outlived release all", || {
            exited(&mut younger)
        });
        wait_until("release all left the younger holder's own shell", || {
            exited(&mut own_shell)
        });
        // A signal would have ended it by now, whoever else holds a copy.
        std::thread::sleep(Duration::from_millis(300));
        assert!(
            !exited(&mut shared_shell),
            "the younger holder ended the older holder's program"
        );
        let (bundle, link) = take_from(&older_socket);
        link.release_all(bundle).unwrap();
        wait_until("the older holder outlived release all", || {
            exited(&mut older)
        });
        wait_until("release all left the shell", || exited(&mut shared_shell));
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// The directories pass to a detached holder only once its spawner has
    /// exited — not when the connection ends with the spawner alive — and a
    /// holder with nothing registered holds nothing and touches nothing.
    #[test]
    fn the_directories_pass_only_from_a_dead_spawner() {
        let (root, dir) = scratch("heir");
        let me = std::process::id();
        let start_in_process = |parent: u32| {
            std::fs::write(dir.join("pid"), parent.to_string()).unwrap();
            let config = HoldConfig {
                dirs: vec![dir.clone()],
                parent,
                uid: uid(),
                limit: Some(Duration::from_secs(20)),
                buffer_limit: BUFFER_LIMIT,
            };
            let (ours, theirs) = UnixStream::pair().unwrap();
            let holding = std::thread::spawn(move || hold(theirs, &config));
            let bound = Bound::connect(ours, me, false, initial(), || {}).expect("handshake");
            (holding, bound)
        };

        // Nothing registered: the end is the holder's end too.
        let mut spawner = Command::new("/bin/sleep").arg("30").spawn().unwrap();
        let (holding, bound) = start_in_process(spawner.id());
        assert!(bound.ping(HAND_WAIT));
        drop(bound);
        assert_eq!(holding.join().unwrap(), HoldEnd::Released);
        assert_eq!(
            owner_of(&dir),
            Some(spawner.id()),
            "an empty holder took the directory"
        );

        // The spawner lives: the holder holds, the directory stays.
        let (holding, bound) = start_in_process(spawner.id());
        let (master, mut shell) = register(&bound, ID, QUIET, b"blob");
        assert!(bound.ping(HAND_WAIT));
        drop(bound);
        drop(master);
        let socket = dir.join(bound_socket(me));
        let (bundle, link) = take_from(&socket);
        drop((bundle, link));
        assert_eq!(
            owner_of(&dir),
            Some(spawner.id()),
            "a living spawner's directory passed to the holder"
        );
        // It exits: now the directory passes.
        let _ = spawner.kill();
        let _ = spawner.wait();
        wait_until("a dead spawner's directory did not pass", || {
            owner_of(&dir) == Some(me)
        });
        let (bundle, link) = take_from(&socket);
        link.release_all(bundle).unwrap();
        assert_eq!(holding.join().unwrap(), HoldEnd::Released);
        wait_until("release all left the shell", || exited(&mut shell));
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// An unconfirmed pane is left to the holder it came from while that one
    /// listens — the update's kind here — and drained once it is gone.
    #[test]
    fn an_unconfirmed_pane_is_drained_once_its_holder_goes() {
        let (root, dir) = scratch("beside");
        let (pane, mut shell) = shell_pane(ID);
        let (pid, start) = (pane.pid, pane.start);
        let copy = pane.master.try_clone().unwrap();
        let mut old = spawn_holder(
            &dir,
            Duration::from_secs(20),
            Bundle {
                layout: Vec::new(),
                panes: vec![pane],
            },
        );
        let (mut holder, bound) = start_bound(&dir, Duration::from_secs(20), BUFFER_LIMIT);
        let socket = dir.join(bound_socket(holder.id()));
        bound.add(BoundPane {
            tab: tab(ID),
            pid,
            start,
            parent: ShellParent::Direct,
            blob: Vec::new(),
            master: copy,
            taken_from: Some(dir.join(HANDOVER_SOCKET)),
        });
        assert!(bound.ping(HAND_WAIT));
        drop(bound);
        std::thread::sleep(Duration::from_millis(300));
        let (peek, peek_link) = take_from(&socket);
        assert!(
            peek.panes[0].buffer.is_empty(),
            "two holders drained one master"
        );
        drop((peek, peek_link));

        let (bundle, link) = take(&dir, uid()).expect("take the old holder");
        link.ack().expect("ack");
        wait_until("the old holder did not exit", || exited(&mut old));
        drop(bundle);
        wait_until("the unconfirmed pane was never drained", || {
            let (peek, _link) = take_from(&socket);
            !ticks(&peek.panes[0].buffer).is_empty()
        });
        let (bundle, link) = take_from(&socket);
        link.release_all(bundle).unwrap();
        wait_until("the holder outlived release all", || exited(&mut holder));
        wait_until("release all left the shell", || exited(&mut shell));
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// Sends from many threads at once against the writer's flushes: every
    /// pane's newest state is the one the holder keeps.
    #[test]
    #[ignore = "race stress: make test-race"]
    fn race_bound_sends_and_the_writer() {
        const THREADS: usize = 8;
        const STATES: usize = 200;
        let (root, dir) = scratch("race");
        let config = HoldConfig {
            dirs: vec![dir.clone()],
            parent: std::process::id(),
            uid: uid(),
            limit: Some(Duration::from_secs(20)),
            buffer_limit: BUFFER_LIMIT,
        };
        let (ours, theirs) = UnixStream::pair().unwrap();
        let holding = std::thread::spawn(move || hold(theirs, &config));
        let bound =
            Bound::connect(ours, std::process::id(), false, initial(), || {}).expect("handshake");
        // A program safe to signal stands in for every pane's.
        let mut program = Command::new("/bin/sleep").arg("60").spawn().unwrap();
        let start = start_time(program.id()).unwrap();
        let ids: Vec<String> = (0..THREADS)
            .map(|i| format!("00000000-0000-0000-0000-{i:012}"))
            .collect();
        let mut slaves = Vec::new();
        std::thread::scope(|scope| {
            for id in &ids {
                let (master, slave) = open_pty();
                slaves.push(slave);
                let bound = &bound;
                let pid = program.id();
                scope.spawn(move || {
                    bound.add(BoundPane {
                        tab: tab(id),
                        pid,
                        start,
                        parent: ShellParent::Direct,
                        blob: Vec::new(),
                        master,
                        taken_from: None,
                    });
                    for n in 0..STATES {
                        bound.state(&tab(id), n.to_string().into_bytes());
                        bound.layout(format!("{id} {n}").into_bytes());
                        if n % 50 == 0 {
                            assert!(bound.ping(HAND_WAIT));
                        }
                    }
                });
            }
        });
        assert!(bound.ping(HAND_WAIT));
        drop(bound);
        let socket = dir.join(bound_socket(std::process::id()));
        let (bundle, link) = take_from(&socket);
        assert_eq!(bundle.panes.len(), THREADS);
        for pane in &bundle.panes {
            let state = PaneState::decode(&pane.blob).unwrap();
            assert_eq!(state.core, (STATES - 1).to_string().into_bytes());
        }
        link.ack().unwrap();
        assert_eq!(holding.join().unwrap(), HoldEnd::Acked);
        drop(slaves);
        let _ = program.kill();
        let _ = program.wait();
        std::fs::remove_dir_all(&root).unwrap();
    }
}
