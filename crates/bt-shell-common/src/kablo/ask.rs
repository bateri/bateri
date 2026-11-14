//! The question a pane asks before bateri puts anything on a server it has not been to:
//! `bateri ssh-ask HOST`.
//!
//! The service runs it on the pane's own terminal, where the connection's password is asked
//! too, **before** the connection is made: an answer of "no" must not leave the person
//! typing the password twice (the plain `ssh` that follows asks for it itself). The program
//! prints the question, takes one key and ends with a status that carries the answer
//! ([`Reply`]); the service reads the status ([`Reply::of_status`]) and the rest is its own.
//!
//! One key and no Enter, except that Enter is "yes": a question that waits for a word invites
//! an answer typed to something else. Whatever was typed before the question appeared is thrown
//! away, so a key pressed ahead of it (an Enter hit while the connection was starting) cannot
//! answer it.

use std::io::{self, Write};
use std::os::fd::{AsRawFd, RawFd};

use super::service::Refusal;

/// The subcommand's name: `bateri ssh-ask HOST`.
pub const SUBCOMMAND: &str = "ssh-ask";

/// Enter or `y`: set the server up.
pub const YES: u8 = 0;
/// `n`: do not; the connection is a plain `ssh`.
pub const NO: u8 = 1;
/// `a`: set the server up, and every server from now on.
pub const ALWAYS: u8 = 2;
/// Control-C: no connection at all, and nothing is kept. The status a shell gives a process
/// the interrupt ended, so a signal and the key come to the same.
pub const INTERRUPTED: u8 = 130;

/// What the person answered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reply {
    /// Set this server up.
    Yes,
    /// Do not set this server up.
    No,
    /// Set every server up from now on.
    Always,
}

impl Reply {
    /// The answer a finished question's status stands for. `Err` is the end of the connection
    /// the status asks for: the person interrupted it (and a signal ends the same), or the
    /// program failed, and then the plain `ssh` is the one thing that cannot be wrong.
    ///
    /// # Errors
    ///
    /// The [`Refusal`] the status stands for.
    pub fn of_status(status: u8) -> Result<Reply, Refusal> {
        match status {
            YES => Ok(Reply::Yes),
            NO => Ok(Reply::No),
            ALWAYS => Ok(Reply::Always),
            // The person's own end, as at a password prompt: what they saw is the answer.
            128..=255 => Err(Refusal::ended("")),
            _ => Err(Refusal::plain()),
        }
    }
}

/// The question, for `host` as the person typed it (`deploy@prod` is `prod` here: the question
/// is about the machine).
pub fn question(host: &str) -> String {
    let name = host.rsplit_once('@').map_or(host, |(_, name)| name);
    format!(
        "Keep this terminal running on {name} if the connection drops? bateri puts one folder \
         there (~/.local/share/bateri). [Y]es / [n]o / [a]lways "
    )
}

/// The status a key stands for, `None` for a key that answers nothing.
pub fn status_of_key(key: u8) -> Option<u8> {
    match key {
        b'\r' | b'\n' | b'y' | b'Y' => Some(YES),
        b'n' | b'N' => Some(NO),
        b'a' | b'A' => Some(ALWAYS),
        // Control-C and Control-D: the person is leaving.
        0x03 | 0x04 => Some(INTERRUPTED),
        _ => None,
    }
}

/// The word that follows the key on the line, so what was chosen can be read in the pane after.
fn word(status: u8) -> &'static str {
    match status {
        YES => "yes",
        NO => "no",
        ALWAYS => "always",
        _ => "",
    }
}

/// The subcommand: asks about `host` on the terminal that is standard input, writing to `out`.
/// The status is the process's exit status.
pub fn run(host: &str, out: &mut dyn Write) -> u8 {
    ask_on(io::stdin().as_raw_fd(), host, out)
}

/// [`run`] on the terminal `fd`.
///
/// Anything that is not a terminal, and an end of input, is [`INTERRUPTED`]: nobody is there to
/// answer, and a connection must not be made for them.
pub fn ask_on(fd: RawFd, host: &str, out: &mut dyn Write) -> u8 {
    let Some(raw) = Raw::enter(fd) else {
        return INTERRUPTED;
    };
    // After the terminal is in the mode that does not echo, so a key typed meanwhile is
    // neither shown nor kept.
    raw.discard_typed_ahead();
    let _ = write!(out, "{}", question(host));
    let _ = out.flush();
    let status = loop {
        let mut key = [0u8; 1];
        match read_key(fd, &mut key) {
            Ok(1) => {
                if let Some(status) = status_of_key(key[0]) {
                    break status;
                }
            }
            // Nothing more will come, or the read failed.
            _ => break INTERRUPTED,
        }
    };
    drop(raw);
    // The terminal is back in its own mode, which ends a line itself.
    let _ = writeln!(out, "{}", word(status));
    let _ = out.flush();
    status
}

fn read_key(fd: RawFd, key: &mut [u8; 1]) -> io::Result<usize> {
    loop {
        // SAFETY: `key` is one writable byte and `fd` stays open for the call.
        let read = unsafe { libc::read(fd, key.as_mut_ptr().cast(), 1) };
        if read >= 0 {
            return Ok(read.unsigned_abs());
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
}

/// The terminal in the mode that gives a key at a time and echoes nothing, put back as it was
/// when this is dropped.
struct Raw {
    fd: RawFd,
    before: libc::termios,
}

impl Raw {
    fn enter(fd: RawFd) -> Option<Raw> {
        // SAFETY: a zeroed `termios` is a plain struct of integers that `tcgetattr` fills in.
        let mut before: libc::termios = unsafe { std::mem::zeroed() };
        // SAFETY: `before` is a valid `termios` and `fd` a descriptor.
        if unsafe { libc::tcgetattr(fd, &raw mut before) } != 0 {
            return None;
        }
        let mut raw = before;
        // Not `ISIG`: Control-C is a key here, read and answered, so that the terminal is put
        // back before the process ends.
        raw.c_lflag &= !(libc::ICANON | libc::ECHO | libc::ISIG);
        raw.c_cc[libc::VMIN] = 1;
        raw.c_cc[libc::VTIME] = 0;
        // SAFETY: `raw` is a valid `termios` derived from the terminal's own.
        if unsafe { libc::tcsetattr(fd, libc::TCSANOW, &raw const raw) } != 0 {
            return None;
        }
        Some(Raw { fd, before })
    }

    fn discard_typed_ahead(&self) {
        // SAFETY: `fd` is an open terminal.
        unsafe { libc::tcflush(self.fd, libc::TCIFLUSH) };
    }
}

impl Drop for Raw {
    fn drop(&mut self) {
        // SAFETY: `before` is what `tcgetattr` gave for this descriptor.
        unsafe { libc::tcsetattr(self.fd, libc::TCSANOW, &raw const self.before) };
    }
}

#[cfg(test)]
mod tests;
