//! The terminal a program shows a view on: its modes while the view is up and the way back, the
//! stream in and out, and the signals that end the program.
//!
//! What the protocol asks of a program (specification §4.9, §6.5, §10.4): before it speaks, the
//! tty is non-canonical with echo off and **signals on** (so ⌃C reaches the program as SIGINT);
//! when it is done, on every path it can run code on, the modes it changed come back and the
//! alternate screen is left.
//!
//! **The input is read without ever blocking.** A signal character (⌃C) flushes the tty's input
//! queue (specification Appendix B), so a read that `poll` promised bytes for can find the
//! queue emptied under it; with the usual `VMIN` of 1 such a read waits for a byte that may
//! never come, and the signal that should end the program is no use to a program stuck in it.
//! The terminal is given `VMIN` 0 instead — a read returns what there is, even nothing — and the
//! signals are installed without restarting an interrupted call.

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};

/// A signal that ends the program arrived.
static ENDED_BY_SIGNAL: AtomicBool = AtomicBool::new(false);

extern "C" fn on_signal(_: libc::c_int) {
    ENDED_BY_SIGNAL.store(true, Ordering::SeqCst);
}

/// Whether a signal asked the program to end (SIGINT, SIGTERM, SIGHUP).
pub fn signalled() -> bool {
    ENDED_BY_SIGNAL.load(Ordering::SeqCst)
}

/// Whether the input and the output are both a terminal.
pub fn is_terminal() -> bool {
    // SAFETY: isatty takes a file descriptor and touches nothing else.
    unsafe { libc::isatty(0) == 1 && libc::isatty(1) == 1 }
}

/// The terminal while the program uses it. Dropping it gives it back.
pub struct Tty {
    saved: libc::termios,
    alternate: bool,
    given_back: bool,
}

impl Tty {
    /// Puts the terminal in the modes the protocol asks and listens for the signals that end
    /// the program (a handler that sets a flag; the loops look at it).
    pub fn take() -> io::Result<Self> {
        // SAFETY: tcgetattr/tcsetattr read and write a termios of ours; the handlers only set
        // an atomic.
        unsafe {
            let mut saved: libc::termios = std::mem::zeroed();
            if libc::tcgetattr(0, &mut saved) != 0 {
                return Err(io::Error::last_os_error());
            }
            let mut raw = saved;
            raw.c_lflag &= !(libc::ICANON | libc::ECHO);
            // ISIG stays as it was: ⌃C is a SIGINT for the foreground group.
            raw.c_lflag |= libc::ISIG;
            raw.c_cc[libc::VMIN] = 0;
            raw.c_cc[libc::VTIME] = 0;
            if libc::tcsetattr(0, libc::TCSANOW, &raw) != 0 {
                return Err(io::Error::last_os_error());
            }
            // Without `SA_RESTART`: a call a signal interrupts comes back, and the loops look
            // at the flag.
            let mut action: libc::sigaction = std::mem::zeroed();
            action.sa_sigaction = on_signal as *const () as libc::sighandler_t;
            action.sa_flags = 0;
            libc::sigemptyset(&mut action.sa_mask);
            for signal in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP] {
                libc::sigaction(signal, &action, std::ptr::null_mut());
            }
            Ok(Self {
                saved,
                alternate: false,
                given_back: false,
            })
        }
    }

    /// Switches to the alternate screen (`CSI ? 1049 h`).
    pub fn enter_alternate(&mut self) -> io::Result<()> {
        write_all(b"\x1b[?1049h")?;
        self.alternate = true;
        Ok(())
    }

    /// Gives the terminal back as it was found: the modes first, then the screen. Once.
    pub fn give_back(&mut self) {
        if self.given_back {
            return;
        }
        self.given_back = true;
        // SAFETY: restores the termios saved by `take`.
        unsafe {
            libc::tcsetattr(0, libc::TCSANOW, &self.saved);
        }
        if self.alternate {
            let _ = write_all(b"\x1b[?1049l");
        }
    }
}

impl Drop for Tty {
    fn drop(&mut self) {
        self.give_back();
    }
}

/// Writes all of `bytes` to the terminal.
pub fn write_all(mut bytes: &[u8]) -> io::Result<()> {
    while !bytes.is_empty() {
        // SAFETY: writes from a slice we own to the output descriptor.
        let wrote = unsafe { libc::write(1, bytes.as_ptr().cast(), bytes.len()) };
        if wrote < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                // A program that is asked to end does not wait for a terminal that has stopped
                // taking what it writes.
                if signalled() {
                    return Err(error);
                }
                continue;
            }
            if error.kind() == io::ErrorKind::WouldBlock {
                std::thread::sleep(std::time::Duration::from_millis(2));
                continue;
            }
            return Err(error);
        }
        bytes = &bytes[wrote as usize..];
    }
    Ok(())
}

/// Waits up to `milliseconds` for input, and reads what there is into `buffer`: `Ok(Some(n))`
/// bytes (0 when the input has ended), `Ok(None)` when nothing came in time, a signal woke it or
/// the bytes `poll` promised were flushed away before they could be read.
pub fn read(buffer: &mut [u8], milliseconds: i32) -> io::Result<Option<usize>> {
    let mut poll = libc::pollfd {
        fd: 0,
        events: libc::POLLIN,
        revents: 0,
    };
    // SAFETY: one pollfd, owned here.
    let ready = unsafe { libc::poll(&mut poll, 1, milliseconds) };
    if ready < 0 {
        let error = io::Error::last_os_error();
        return if error.kind() == io::ErrorKind::Interrupted {
            Ok(None)
        } else {
            Err(error)
        };
    }
    if ready == 0 {
        return Ok(None);
    }
    // SAFETY: reads into a buffer we own. The terminal's `VMIN` is 0: this does not wait.
    let got = unsafe { libc::read(0, buffer.as_mut_ptr().cast(), buffer.len()) };
    if got < 0 {
        let error = io::Error::last_os_error();
        return if matches!(
            error.kind(),
            io::ErrorKind::Interrupted | io::ErrorKind::WouldBlock
        ) {
            Ok(None)
        } else {
            Err(error)
        };
    }
    if got == 0 {
        // Nothing to read after all, or the terminal is gone: only a hang-up says the second.
        return Ok(
            if poll.revents & (libc::POLLHUP | libc::POLLERR | libc::POLLNVAL) != 0 {
                Some(0)
            } else {
                None
            },
        );
    }
    Ok(Some(got as usize))
}
