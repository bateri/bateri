//! This Mac as a kablo account: the steps the service takes for a server, taken here.
//!
//! A terminal bateri keeps for a pane of this Mac is a terminal of a kablo that runs on this
//! Mac, and the account that reaches it is the Mac itself ([`ACCOUNT`]). Everything a server's
//! account has, this one has by the same code: kablo is put in a directory of its version below
//! the home directory ([`super::install::ensure_here`], with the rules of
//! `kablo_proto::install`), never over a version that is there; the terminals bateri keeps are
//! renewed one by one when the package brings another release, and the version left behind goes
//! when no terminal of the machine runs from it; the account is in the ledger and is dialed by
//! the service like any other. What differs is the transport, and it is the whole of this module:
//! a step is `sh` in the home directory instead of a command on an ssh channel
//! ([`LocalChannel`]), and the bridge is `kablo attach` started here, not at the far end of an
//! ssh ([`carry`]).
//!
//! **Why not the package's own file.** The bridge's relay starts its warden from its own
//! executable and a renewal `execve`s into a path it is given; an updated application replaces
//! the file inside the package under both. The copy in the home directory is the version's, as a
//! server's is.
//!
//! **Whose it is.** The account is the Mac's, and it is *not* a server: none of what bateri does
//! for a server pane (file drops, the load indicator, the server's ports, a server's files and
//! counter, the notes a program left there) is done for it. The one place that says which is
//! which is the account's name ([`is_account`], [`Shown::is_local`](super::service::Shown)).
//!
//! **What a pane gets.** A pane's shell runs in a terminal of this account, and the pane's session
//! reads its bytes from a stream the service holds as a raw holder
//! ([`super::service::Service::open_local`], [`super::hub::Hub::open_raw`]): the terminal is
//! started with the command, the environment and the directory the session would have started its
//! own child with ([`new_session`]), so the shell cannot tell. For now such a pane is not carried
//! over an update or a quit — its terminal is ended with its session — and it is not in
//! [`super::service::Service::shown`]: the surfaces that ask for a server's terminal of a pane do
//! not find one.

use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use kablo_proto::install;

use super::install::{Channel, Installed, Reply, run_command_in};
use super::service::Carried;
use super::{Cause, NewSession, Size};
use crate::ssh_wrap::KabloHost;

/// The account of this Mac. Not a shape any server's key has (`user@host:port`, or the digits of
/// a hash).
pub const ACCOUNT: &str = "local";

/// How long one step on this Mac may take. The copy of a binary of a few megabytes is the long
/// one; nothing here waits for a network, so the bound is for a disk that does not answer.
pub const STEP_LIMIT: Duration = Duration::from_secs(30);

/// Whether `account` (an account's key) is this Mac's.
pub fn is_account(account: &str) -> bool {
    account == ACCOUNT
}

/// This Mac as the service holds a server: no address, no ssh arguments.
pub fn host() -> KabloHost {
    KabloHost {
        host: String::new(),
        port: None,
        argv: Vec::new(),
        config: String::new(),
        key: ACCOUNT.to_owned(),
    }
}

/// A [`Channel`] on this Mac: the script is run by `sh` in `home`, where the commands of an ssh
/// start on a server, so the relative paths of [`super::install`]'s scripts mean the same.
pub struct LocalChannel {
    /// The home directory kablo is installed below.
    pub home: PathBuf,
    /// How long one step may take.
    pub limit: Duration,
}

impl LocalChannel {
    /// A channel in `home` with the usual bound.
    pub fn new(home: PathBuf) -> LocalChannel {
        LocalChannel {
            home,
            limit: STEP_LIMIT,
        }
    }
}

impl Channel for LocalChannel {
    fn run(&self, script: &str, input: &[u8]) -> io::Result<Reply> {
        run_command_in(
            &["/bin/sh".to_owned(), "-c".to_owned(), script.to_owned()],
            input,
            self.limit,
            Some(&self.home),
        )
    }
}

/// The command that starts the bridge: the binary `installed` names, below `home`, run as
/// `attach`. A caller that has to set its own environment on it (a test pointing kablo at a
/// socket directory of its own) does so before [`start`]ing it.
pub fn bridge_command(home: &Path, installed: &Installed) -> io::Result<Command> {
    let program = install::install_path_in(&home.to_string_lossy(), &installed.version)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error.to_string()))?;
    let mut command = Command::new(program);
    command.arg("attach");
    Ok(command)
}

/// Starts the bridge `command` and hands its two ends to the service. It leads a process group of
/// its own (a `^C` at the terminal bateri was started from is not its business) and says nothing
/// anyone reads: what ends it ends the stream, and there is no network to tell a login from.
pub fn start(mut command: Command) -> io::Result<Carried> {
    use std::os::unix::process::CommandExt as _;
    let mut child = command
        .process_group(0)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
        let _ = child.kill();
        let _ = child.wait();
        return Err(io::Error::other("the bridge has no pipes"));
    };
    Ok(Carried {
        reader: Box::new(stdout),
        writer: Box::new(stdin),
        // Only the bridge: the terminals it serves are other processes, started apart from it,
        // and they are the point.
        close: Box::new(move || {
            let _ = child.kill();
            let _ = child.wait();
        }),
        ended: Box::new(|| Cause::Dropped),
    })
}

/// The bridge to this Mac's terminals: `kablo attach` from the version `installed` names.
pub fn carry(home: &Path, installed: &Installed) -> io::Result<Carried> {
    start(bridge_command(home, installed)?)
}

/// The request that starts a pane's terminal here: the pane's own command, environment and
/// directory as `launch` says them ([`bt_core::SessionOptions::child_launch`]), and `history`
/// rows of scrollback kept for it; with the size the terminal starts at, in the cells and the
/// pixels the program is told.
pub fn new_session(launch: &bt_core::ChildLaunch, history: u32) -> (NewSession, Size) {
    use std::os::unix::ffi::OsStrExt as _;
    // `NewSession` is not exhaustive: it is started from its default.
    let mut spec = NewSession::default();
    spec.argv = launch
        .argv
        .iter()
        .map(|arg| arg.as_bytes().to_vec())
        .collect();
    spec.env = launch
        .env
        .iter()
        .map(|(name, value)| (name.as_bytes().to_vec(), value.as_bytes().to_vec()))
        .collect();
    spec.cwd = launch
        .cwd
        .as_ref()
        .map(|dir| dir.as_os_str().as_bytes().to_vec());
    spec.history = Some(history);
    let pixels =
        |cells: u16, each: u16| u16::try_from(u32::from(cells) * u32::from(each)).unwrap_or(0);
    let size = Size {
        cols: launch.size.cols,
        rows: launch.size.rows,
        xpixel: pixels(launch.size.cols, launch.size.cell_width),
        ypixel: pixels(launch.size.rows, launch.size.cell_height),
    };
    (spec, size)
}

#[cfg(test)]
mod tests;
