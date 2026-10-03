//! The update's live handover (055 phase-3): the holder process (`bateri
//! hold`) and its frame.
//!
//! When Sparkle relaunches bateri, the old process gives every pane's PTY
//! master and opaque state to a holder and exits **without** hanging the
//! shells up; the new process connects, takes them and acknowledges, and the
//! holder exits. Decisions: `.tasks/055-guncellemede-canli-devir/discussion.md`
//! → Karar 1, 2, 5, 6, 8, 10.
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
//!     tab: len u32, UUID text | pid u32 | start u64 | flags u32 (bit 0: ended)
//!     blob: len u64, bytes | buffer: len u64, bytes
//!   ```
//!
//!   Integers are little-endian. The client opens with [`TAKE`] and answers
//!   with one-byte messages; their values are fixed **across every version**
//!   (a newer client must be able to release an older holder's panes):
//!   [`ACK`], [`RELEASE`] + the pane's position as `u32`, [`RELEASE_ALL`]. The
//!   holder tells its spawner [`READY`] once it listens and owns the instance
//!   directories.
//! - **The holder** ([`hold`], [`hold_main`]): detached (`setsid`, `SIGHUP`
//!   ignored, `chdir /`, standard I/O on `/dev/null`, every other inherited
//!   descriptor closed — a stray copy of a master would defeat the hang-up),
//!   no AppKit. It reads the bundle from the `socketpair` end it inherited,
//!   binds [`HANDOVER_SOCKET`] in the old instance's first directory (`0700`)
//!   and takes every directory of the instance over from its spawner
//!   ([`ssh_route::adopt_instance`]). While nobody is connected it **drains**
//!   the masters into each pane's buffer up to [`BUFFER_LIMIT`] (then stops
//!   reading: the program blocks on back-pressure, nothing is lost) and
//!   records a master that reports the end (EOF/EIO). A peer of another uid
//!   is closed without a byte, and so is one that does not say [`TAKE`] (a
//!   liveness probe — the sweep's — costs nothing). Once the bundle is given
//!   it stops draining — the client reads the masters now — and waits:
//!   [`ACK`] closes its copies and exits, [`RELEASE`] closes one copy at once,
//!   [`RELEASE_ALL`] closes all and exits, a disconnect without an ACK sends
//!   it back to waiting (the buffers kept: the next client needs them).
//!   [`HOLD_LIMIT`] after its birth it closes everything **in any state**
//!   (every read and write is bounded by it, [`Wire`]) and exits: closing the
//!   last copy of a master is the hang-up (`context.md`, measured), so no
//!   shell outlives an update nobody completed.
//!
//! **Known limits:** bytes a client read from a master before it went away
//! without an ACK are lost to the next one. On macOS `recvmsg` has no
//! `MSG_CMSG_CLOEXEC`: a received master is close-on-exec only after it
//! arrives, so a child spawned by another thread of the receiving process in
//! that window inherits a copy — the new bateri takes before it spawns
//! anything (phase-4).
//!
//! **Order for the new bateri** (phase-4): [`take`] → `adopt_instance` (from
//! [`Link::holder_pid`]) → adopt the panes ([`Link::release`] those that fall
//! back) → [`Link::ack`].

use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use bt_core::TabId;

use crate::ssh_route::{self, SUN_PATH};

/// The holder's socket in the instance directory.
pub const HANDOVER_SOCKET: &str = "handover";

/// The frame's first bytes.
const MAGIC: [u8; 4] = *b"BTHO";

/// The frame's version: **frozen** — the layout in the module doc is
/// version 1, byte for byte.
pub const FRAME_VERSION: u32 = 1;

/// How long a holder lives without an acknowledgement — a **design
/// constant** (055 R3.3). Long enough for Sparkle to install and start the
/// new bateri on a slow disk and for that bateri to come up; short enough
/// that an update nobody completes does not keep the user's programs
/// blocked out of sight for long. After it every shell is hung up.
pub const HOLD_LIMIT: Duration = Duration::from_secs(120);

/// How many bytes the holder keeps per pane before it stops reading — a
/// **design constant**. More than a full default scrollback of output
/// (10 000 lines of a wide grid), so a program that prints during the
/// handover is not held back by an ordinary burst; a pane past it blocks
/// on the PTY until the new bateri reads, nothing is dropped.
pub const BUFFER_LIMIT: usize = 4 << 20;

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
}

impl From<io::Error> for HandoverError {
    fn from(error: io::Error) -> HandoverError {
        HandoverError::Io(error)
    }
}

/// One side of a connection: every read and write is bounded by
/// [`HAND_WAIT`] and, with a deadline, by the deadline as a whole — a peer
/// that trickles a byte now and then cannot stretch an operation past it.
struct Wire<'a> {
    stream: &'a UnixStream,
    deadline: Option<Instant>,
}

impl Wire<'_> {
    /// Sets the next call's timeout; past the deadline nothing is tried.
    fn arm(&self, write: bool) -> io::Result<()> {
        let mut wait = HAND_WAIT;
        if let Some(deadline) = self.deadline {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(io::ErrorKind::TimedOut.into());
            }
            wait = wait.min(remaining);
        }
        // A failure is not one: macOS refuses `setsockopt` with EINVAL once
        // the peer closed, and then the call cannot block anyway — the
        // buffered bytes or the end are there.
        let _ = if write {
            self.stream.set_write_timeout(Some(wait))
        } else {
            self.stream.set_read_timeout(Some(wait))
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
    blob: &'a [u8],
    buffer: &'a [u8],
    master: RawFd,
}

/// Writes `bundle` as one frame — the old bateri's side of the
/// `socketpair`. Each call is bounded by [`HAND_WAIT`].
pub fn write_frame(stream: &UnixStream, bundle: &Bundle) -> io::Result<()> {
    let views = bundle.panes.iter().map(|pane| PaneView {
        tab: pane.tab.as_str(),
        pid: pane.pid,
        start: pane.start,
        ended: pane.ended,
        blob: &pane.blob,
        buffer: &pane.buffer,
        master: pane.master.as_raw_fd(),
    });
    let wire = Wire {
        stream,
        deadline: None,
    };
    write_views(&wire, &bundle.layout, views.collect())
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
        let tab = pane.tab.as_bytes();
        let tab_len = u32::try_from(tab.len()).map_err(|_| io::ErrorKind::InvalidInput)?;
        wire.write_all(&tab_len.to_le_bytes())?;
        wire.write_all(tab)?;
        wire.write_all(&pane.pid.to_le_bytes())?;
        wire.write_all(&pane.start.to_le_bytes())?;
        let flags = if pane.ended { ENDED } else { 0 };
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

/// Reads one frame: the header first (a version this binary does not know
/// is [`HandoverError::Version`], nothing read past it), then the body.
/// Each call is bounded by [`HAND_WAIT`].
pub fn read_frame(stream: &UnixStream) -> Result<Bundle, HandoverError> {
    let wire = Wire {
        stream,
        deadline: None,
    };
    read_header(&wire)?;
    read_body(&wire)
}

fn read_header(wire: &Wire<'_>) -> Result<(), HandoverError> {
    let mut magic = [0u8; 4];
    wire.read_exact(&mut magic)?;
    if magic != MAGIC {
        return Err(HandoverError::NotAFrame);
    }
    match wire.u32()? {
        FRAME_VERSION => Ok(()),
        other => Err(HandoverError::Version(other)),
    }
}

fn read_body(wire: &Wire<'_>) -> Result<Bundle, HandoverError> {
    let mut total = 0u64;
    let layout = read_field(wire, wire.u64()?, FIELD_LIMIT, &mut total)?;
    let count = wire.u32()?;
    if count > PANE_LIMIT {
        return Err(HandoverError::Malformed("pane count"));
    }
    let mut panes = Vec::new();
    for position in 0..count {
        wire.arm(false)?;
        let master = recv_fd(wire.stream)?;
        let tab = read_field(wire, u64::from(wire.u32()?), TAB_LIMIT, &mut total)?;
        let tab = std::str::from_utf8(&tab)
            .ok()
            .and_then(TabId::parse)
            .ok_or(HandoverError::Malformed("tab id"))?;
        let pid = wire.u32()?;
        let start = wire.u64()?;
        let flags = wire.u32()?;
        let blob = read_field(wire, wire.u64()?, FIELD_LIMIT, &mut total)?;
        let buffer = read_field(wire, wire.u64()?, FIELD_LIMIT, &mut total)?;
        panes.push(HeldPane {
            tab,
            pid,
            start,
            ended: flags & ENDED != 0,
            blob,
            buffer,
            master,
            position,
        });
    }
    Ok(Bundle { layout, panes })
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
    if byte[0] != FD_MARK || msg.msg_flags & libc::MSG_CTRUNC != 0 || fds.len() != 1 {
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

/// Whether a holder listens on `dir`'s [`HANDOVER_SOCKET`]. The connection
/// sends nothing, so the holder closes it without serving (no [`TAKE`]).
pub fn holder_listening(dir: &Path) -> bool {
    UnixStream::connect(dir.join(HANDOVER_SOCKET)).is_ok()
}

// ─── the client (the new bateri) ─────────────────────────────────────────

/// The connection to a holder after [`take`]: dropping it without
/// [`Link::ack`] is a disconnect without an acknowledgement — the holder
/// waits for the next client.
#[derive(Debug)]
pub struct Link {
    stream: UnixStream,
    holder: u32,
}

/// Connects to the holder in `dir`, checks it is user `uid`, and takes the
/// bundle. A frame of a version this binary does not know is released at
/// once ([`RELEASE_ALL`]: the holder hangs every shell up — Karar 8) and
/// answered with [`HandoverError::Version`]; the caller falls back.
pub fn take(dir: &Path, uid: u32) -> Result<(Bundle, Link), HandoverError> {
    let path = dir.join(HANDOVER_SOCKET);
    if path.as_os_str().len() >= SUN_PATH {
        return Err(io::Error::from(io::ErrorKind::InvalidInput).into());
    }
    let stream = UnixStream::connect(&path)?;
    let (peer_uid, holder) = peer(&stream)?;
    if peer_uid != uid {
        return Err(HandoverError::Peer);
    }
    let wire = Wire {
        stream: &stream,
        deadline: None,
    };
    wire.write_all(&[TAKE])?;
    match read_header(&wire) {
        Ok(()) => {}
        Err(HandoverError::Version(version)) => {
            let _ = wire.write_all(&[RELEASE_ALL]);
            return Err(HandoverError::Version(version));
        }
        Err(error) => return Err(error),
    }
    let bundle = read_body(&wire)?;
    Ok((bundle, Link { stream, holder }))
}

impl Link {
    /// The holder's pid: the `from` of [`ssh_route::adopt_instance`].
    pub fn holder_pid(&self) -> u32 {
        self.holder
    }

    fn wire(&self) -> Wire<'_> {
        Wire {
            stream: &self.stream,
            deadline: None,
        }
    }

    /// Releases one pane: this side's copy of its master is closed **here**
    /// (the argument is consumed), then the holder closes its own — the last
    /// copy, so the pane's shell is hung up.
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

// ─── the holder ──────────────────────────────────────────────────────────

/// What the holder needs besides its spawner's stream.
#[derive(Clone, Debug)]
pub struct HoldConfig {
    /// The old instance's directories, one per socket root; the first holds
    /// the socket. All are taken over from `parent`.
    pub dirs: Vec<PathBuf>,
    /// The spawner (the old bateri): the owner the directories are taken from.
    pub parent: u32,
    /// The only uid a client may have.
    pub uid: u32,
    /// [`HOLD_LIMIT`], shorter in the tests.
    pub limit: Duration,
}

/// How the holder ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HoldEnd {
    /// A client took everything and acknowledged.
    Acked,
    /// A client released everything, or nothing was left to hold.
    Released,
    /// The limit passed: every copy closed.
    Limit,
    /// It never held: no bundle, no socket or no directory.
    Failed,
}

/// One pane in the holder.
struct Held {
    tab: String,
    pid: u32,
    start: u64,
    ended: bool,
    blob: Vec<u8>,
    buffer: Vec<u8>,
    /// `None` once released.
    master: Option<OwnedFd>,
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

/// The holder's body (the process setup is [`hold_main`]'s): takes the
/// bundle from `spawner`, listens, owns the directories, says [`READY`],
/// then drains and serves until an acknowledgement, a release or the limit.
/// Every way out drops the remaining copies — the hang-up of whatever no
/// client holds. A failure after the directories were taken gives them
/// back to the spawner, which falls back and lives on.
pub fn hold(spawner: UnixStream, config: &HoldConfig) -> HoldEnd {
    let deadline = Instant::now() + config.limit;
    let wire = Wire {
        stream: &spawner,
        deadline: Some(deadline),
    };
    let bundle = match read_header(&wire).and_then(|()| read_body(&wire)) {
        Ok(bundle) => bundle,
        Err(_) => return HoldEnd::Failed,
    };
    let layout = bundle.layout;
    let mut held: Vec<Held> = bundle
        .panes
        .into_iter()
        .map(|pane| Held {
            tab: pane.tab.as_str().to_owned(),
            pid: pane.pid,
            start: pane.start,
            ended: pane.ended,
            blob: pane.blob,
            buffer: pane.buffer,
            master: Some(pane.master),
        })
        .collect();
    let Some(first) = config.dirs.first() else {
        return HoldEnd::Failed;
    };
    let Ok(listening) = listen(first) else {
        return HoldEnd::Failed;
    };
    let me = std::process::id();
    let mut taken = Vec::new();
    for dir in &config.dirs {
        if ssh_route::adopt_instance(dir, Some(config.parent), me).is_err() {
            give_back(&taken, me, config.parent);
            return HoldEnd::Failed;
        }
        taken.push(dir.clone());
    }
    if wire.write_all(&[READY]).is_err() {
        give_back(&taken, me, config.parent);
        return HoldEnd::Failed;
    }
    drop(spawner);
    loop {
        if held.iter().all(|pane| pane.master.is_none()) {
            return HoldEnd::Released;
        }
        let Some(client) = wait(&listening.listener, &mut held, deadline, config.uid) else {
            if Instant::now() >= deadline {
                return HoldEnd::Limit;
            }
            continue;
        };
        match serve(&client, &layout, &mut held, deadline) {
            Served::Acked => return HoldEnd::Acked,
            Served::Released => return HoldEnd::Released,
            Served::Limit => return HoldEnd::Limit,
            Served::Gone => {}
        }
    }
}

/// Hands the directories back to the spawner: the holder failed before the
/// spawner left, and a dead owner would have the spawner's directories swept
/// under it.
fn give_back(dirs: &[PathBuf], me: u32, parent: u32) {
    for dir in dirs {
        let _ = ssh_route::adopt_instance(dir, Some(me), parent);
    }
}

/// Binds [`HANDOVER_SOCKET`] in `dir`, a private directory of this user. A
/// file already there is removed only when nobody listens on it (a dead
/// holder's); a live one is another holder's and this one fails.
fn listen(dir: &Path) -> io::Result<Listening> {
    if !ssh_route::private_dir(dir) {
        return Err(io::ErrorKind::PermissionDenied.into());
    }
    let path = dir.join(HANDOVER_SOCKET);
    if path.as_os_str().len() >= SUN_PATH {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    if holder_listening(dir) {
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

/// One wait while nobody is connected: drains the readable masters and
/// returns a client of user `uid` that said [`TAKE`]; `None` on a timeout,
/// after a drain, or for a connection closed unanswered (another user, a
/// probe).
fn wait(
    listener: &UnixListener,
    held: &mut [Held],
    deadline: Instant,
    uid: u32,
) -> Option<UnixStream> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        return None;
    }
    let drainable: Vec<usize> = (0..held.len())
        .filter(|&i| {
            let pane = &held[i];
            !pane.ended
                && pane.buffer.len() < BUFFER_LIMIT
                && pane
                    .master
                    .as_ref()
                    .is_some_and(|fd| selectable(fd.as_raw_fd()))
        })
        .collect();
    let listener_fd = listener.as_raw_fd();
    if !selectable(listener_fd) {
        return None;
    }
    // SAFETY: an all-zero `fd_set` is a valid empty set, and every fd added
    // is below `FD_SETSIZE` ([`selectable`]).
    let mut readable: libc::fd_set = unsafe { std::mem::zeroed() };
    let mut top = listener_fd;
    unsafe { libc::FD_SET(listener_fd, &raw mut readable) };
    for &i in &drainable {
        if let Some(fd) = &held[i].master {
            unsafe { libc::FD_SET(fd.as_raw_fd(), &raw mut readable) };
            top = top.max(fd.as_raw_fd());
        }
    }
    let mut timeout = libc::timeval {
        tv_sec: remaining.as_secs().try_into().unwrap_or(libc::time_t::MAX),
        // Below a million: fits `suseconds_t` (`i32` on macOS, `i64` on Linux).
        tv_usec: remaining.subsec_micros() as libc::suseconds_t,
    };
    // SAFETY: the set and the timeout belong to this frame. `select`, not
    // `poll`: macOS' `poll` does not support devices, a PTY master among them.
    let ready = unsafe {
        libc::select(
            top + 1,
            &raw mut readable,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &raw mut timeout,
        )
    };
    if ready <= 0 {
        return None;
    }
    for &i in &drainable {
        let is_set = held[i]
            .master
            .as_ref()
            // SAFETY: `readable` is the set `select` filled.
            .is_some_and(|fd| unsafe { libc::FD_ISSET(fd.as_raw_fd(), &raw const readable) });
        if is_set {
            drain(&mut held[i]);
        }
    }
    // SAFETY: as above.
    if !unsafe { libc::FD_ISSET(listener_fd, &raw const readable) } {
        return None;
    }
    let (client, _) = listener.accept().ok()?;
    // The accepted stream inherits nothing from the non-blocking listener
    // on Linux and everything on macOS: make it blocking either way.
    client.set_nonblocking(false).ok()?;
    if peer(&client).ok()?.0 != uid {
        return None;
    }
    let wire = Wire {
        stream: &client,
        deadline: Some(deadline.min(Instant::now() + TAKE_WAIT)),
    };
    let mut first = [0u8];
    wire.read_exact(&mut first).ok()?;
    (first[0] == TAKE).then_some(client)
}

/// Whether `fd` fits an `fd_set`. A holder starts with a handful of
/// descriptors, so this is a guard, not a limit anyone meets.
fn selectable(fd: RawFd) -> bool {
    usize::try_from(fd).is_ok_and(|fd| fd < libc::FD_SETSIZE)
}

/// Reads what the master has, up to [`BUFFER_LIMIT`]; EOF or an error other
/// than "nothing yet" is the program's end.
fn drain(pane: &mut Held) {
    let Some(master) = &pane.master else {
        return;
    };
    let mut chunk = [0u8; 16 * 1024];
    while pane.buffer.len() < BUFFER_LIMIT {
        let room = (BUFFER_LIMIT - pane.buffer.len()).min(chunk.len());
        // SAFETY: `chunk` belongs to this frame and `room` fits it.
        let read = unsafe { libc::read(master.as_raw_fd(), chunk.as_mut_ptr().cast(), room) };
        match read {
            0 => {
                pane.ended = true;
                return;
            }
            n if n > 0 => pane.buffer.extend_from_slice(&chunk[..n as usize]),
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
/// within `deadline`. A failed write still reads what the client already
/// sent — a [`RELEASE_ALL`] may be waiting there.
fn serve(client: &UnixStream, layout: &[u8], held: &mut [Held], deadline: Instant) -> Served {
    let given: Vec<usize> = (0..held.len())
        .filter(|&i| held[i].master.is_some())
        .collect();
    let views = given
        .iter()
        .filter_map(|&i| {
            let pane = &held[i];
            Some(PaneView {
                tab: &pane.tab,
                pid: pane.pid,
                start: pane.start,
                ended: pane.ended,
                blob: &pane.blob,
                buffer: &pane.buffer,
                master: pane.master.as_ref()?.as_raw_fd(),
            })
        })
        .collect();
    let wire = Wire {
        stream: client,
        deadline: Some(deadline),
    };
    let blocking = write_views(&wire, layout, views).is_ok();
    if Instant::now() >= deadline {
        return Served::Limit;
    }
    if !blocking && client.set_nonblocking(true).is_err() {
        return Served::Gone;
    }
    messages(client, held, &given, deadline, blocking)
}

/// The client's messages until one ends the serving. `blocking`: wait for
/// them until `deadline`; otherwise only what is already there.
fn messages(
    client: &UnixStream,
    held: &mut [Held],
    given: &[usize],
    deadline: Instant,
    blocking: bool,
) -> Served {
    let wire = Wire {
        stream: client,
        deadline: Some(deadline),
    };
    loop {
        if blocking {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Served::Limit;
            }
            // Fails once the client closed (macOS' EINVAL): the read then
            // sees the end without blocking.
            let _ = client.set_read_timeout(Some(remaining));
        }
        let mut tag = [0u8];
        match (&*client).read(&mut tag) {
            Ok(1) => {}
            Ok(_) => return Served::Gone,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) if blocking && Instant::now() >= deadline => return Served::Limit,
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
                held[i].master = None;
            }
            _ => return Served::Gone,
        }
    }
}

// ─── the process ─────────────────────────────────────────────────────────

/// `bateri hold --fd FD --dir DIR [--dir DIR]...` (055 R3.2): the update's
/// holder process, started by the old bateri with one end of a `socketpair`
/// at `FD` and the instance's directories (the first holds the socket).
/// Returns the exit code. The body is [`hold`]; here the argv and the
/// process ([`detach`]).
pub fn hold_main(args: &[String]) -> i32 {
    hold_process(args, HOLD_LIMIT)
}

fn hold_process(args: &[String], limit: Duration) -> i32 {
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
    };
    match hold(spawner, &config) {
        HoldEnd::Acked | HoldEnd::Released | HoldEnd::Limit => EXIT_DONE,
        HoldEnd::Failed => EXIT_FAILED,
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
            limit: Duration::from_millis(1500),
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
        std::process::exit(hold_process(&args, limit));
    }

    /// Spawns a holder for `dirs` and gives it `bundle` ("the old side
    /// exits": its copies go); returns the holder and the spawner's end.
    fn start_holder(dirs: &[&Path], limit: Duration, bundle: Bundle) -> (Child, UnixStream) {
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
        write_frame(&ours, &bundle).unwrap();
        drop(bundle);
        ours.set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
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

    /// A shell on the PTY as its controlling terminal (so the master's last
    /// close hangs it up), printing numbered lines.
    fn spawn_shell(slave: File) -> Child {
        let mut command = Command::new("/bin/sh");
        command
            .args([
                "-c",
                "i=0; while :; do i=$((i+1)); echo tick $i; sleep 0.05; done",
            ])
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
}
