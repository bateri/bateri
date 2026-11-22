//! Where a profile is kept when its program runs on a server: the server's side of it, as far as
//! the browser's code knows.
//!
//! The browser code asks for a connection to the **home** of a profile and speaks the home's
//! protocol on it (`sync`); it knows nothing of what carries the connection, as the proxy knows
//! nothing of what makes the connections of a page ([`super::route::Dialer`]). The shell gives a
//! machine its depot when the way to the machine has been shown, and takes it back when the
//! person keeps no profiles there ([`Depots::remove`]).

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use kablo_proto::v1::Exit;

use super::route::Link;
use crate::kablo::remote::RemoteCommand;
use crate::kablo::served::Served;
use crate::ssh_route::Ask;

/// How long a connection to a depot may take to be made: the first one through a server may have
/// to bring the connection to it up. A design constant: the profile does not wait for it
/// ([`SYNC_WAIT`]).
pub const OPEN_WAIT: Duration = Duration::from_secs(10);

/// How long a browser's first page waits for what the home has to be put in the profile: a depot
/// that has not answered by then is left to catch up on its own (the browser opens with what the
/// Mac has, and the profile joins when the depot answers).
pub const SYNC_WAIT: Duration = Duration::from_secs(3);

/// Something that opens connections to the home of a profile.
pub trait Depot: Send + Sync {
    /// A connection to the home, made within `wait`: what the far end says is `reader`, what is
    /// said to it `writer`. The `Err` is a line for the person: why there is no home to speak to
    /// (the machine has no depot of the right kind, the connection did not come up).
    fn open(&self, wait: Duration) -> Result<Link, String>;

    /// Whether the person keeps profiles at this machine (the switch of its `[remote] hosts`
    /// entry). A machine that does not is not connected to for a profile ([`Depot::open`] is not
    /// called); it is told so once with [`Depot::switch`].
    fn kept(&self) -> bool {
        true
    }

    /// Tells the machine that profiles are kept there (`on`: the mark that none is kept is taken
    /// away) or are not (`off`: what is kept is deleted and the mark is left), within `wait`. The
    /// `Err` is a line for the person. A depot that cannot say so does not take a turn.
    fn switch(&self, on: bool, wait: Duration) -> Result<(), String> {
        let _ = (on, wait);
        Err("this depot has no switch".to_owned())
    }
}

/// What the server runs to be the home of a profile: the newest kablo that is installed there,
/// told to keep profiles (`kablo profile`: it speaks the profile's protocol on its standard input
/// and output). A server that has none (or one that is too old to know the command) ends the
/// command at once and the connection is no connection.
pub fn home_script() -> String {
    kablo_script("profile")
}

/// What the server runs to tell its kablo that profiles are kept (`profile on`) or are not
/// (`profile off`).
fn switch_script(on: bool) -> String {
    kablo_script(if on { "profile on" } else { "profile off" })
}

/// The newest kablo that is installed on the server, run with `args`; 127 where there is none.
fn kablo_script(args: &str) -> String {
    let root = kablo_proto::install::ROOT;
    format!(
        "d=\"$HOME/{root}\"; \
         v=$(ls -1 \"$d\" 2>/dev/null | {{ sort -V 2>/dev/null || sort; }} | tail -n 1); \
         [ -n \"$v\" ] || exit 127; exec \"$d/$v/kablo\" {args}"
    )
}

/// The home of a program that runs on a server: the command the server's `sh` runs, over the
/// connection bateri keeps to it. `keep` is the person's switch for the server.
pub struct ServedDepot {
    pub served: Served,
    pub keep: bool,
}

impl Depot for ServedDepot {
    fn kept(&self) -> bool {
        self.keep
    }

    fn switch(&self, on: bool, wait: Duration) -> Result<(), String> {
        let RemoteCommand {
            stdin,
            stdout,
            stderr,
            end,
        } = self.served.sh(&switch_script(on), Ask::Never, wait)?;
        // The command has nothing to read: its end of input is closed at once.
        drop(stdin);
        let said = |mut from: Box<dyn Read + Send>| {
            std::thread::spawn(move || {
                let mut kept = Vec::new();
                let _ = from.by_ref().take(512).read_to_end(&mut kept);
                let _ = std::io::copy(&mut from, &mut std::io::sink());
                String::from_utf8_lossy(&kept).trim().to_owned()
            })
        };
        let (out, err) = (said(Box::new(stdout)), said(Box::new(stderr)));
        let exit = end.wait_within(wait);
        let (out, err) = (
            out.join().unwrap_or_default(),
            err.join().unwrap_or_default(),
        );
        match exit {
            Ok(Exit::Code(0)) => Ok(()),
            Ok(Exit::Code(127)) => Err("the server has no kablo that keeps profiles".to_owned()),
            Ok(Exit::Code(code)) => Err(format!(
                "the server's kablo ended with {code}: {}",
                if err.is_empty() { out } else { err }
            )),
            Ok(Exit::Signal(_)) => Err("the server's kablo was ended".to_owned()),
            Err(why) => Err(why),
        }
    }

    fn open(&self, wait: Duration) -> Result<Link, String> {
        let RemoteCommand {
            stdin,
            stdout,
            stderr,
            end,
        } = self.served.sh(&home_script(), Ask::Never, wait)?;
        let stdin = stdin.ok_or_else(|| "the server gave the command no input".to_owned())?;
        let ender = end.clone();
        Ok(Link::new(
            stdout,
            stdin,
            move || ender.kill(),
            // What the command says on its error output is kept as long as the command is.
            Some(Box::new((stderr, end))),
        ))
    }
}

/// What the person's switch for a machine was when this Mac last told the machine: it is kept so
/// that a turn of the switch is seen, whichever Mac's turn it was (`off` to `on` takes the mark
/// away at the machine and shares the profile again; `on` to `off` takes the machine's last
/// state and deletes it there).
#[derive(Clone, Debug)]
pub struct Mark {
    path: PathBuf,
}

impl Mark {
    /// The note for `machine`, in the folder `data` (the browser's data folder on this Mac).
    pub fn new(data: &Path, machine: &str) -> Mark {
        let name = &crate::digest::sha256_hex(machine.as_bytes())[..16];
        Mark {
            path: data.join("machines").join(name),
        }
    }

    /// The switch as the machine was last told it; `None` before it ever was.
    pub fn recall(&self) -> Option<bool> {
        match std::fs::read_to_string(&self.path).ok()?.trim() {
            "on" => Some(true),
            "off" => Some(false),
            _ => None,
        }
    }

    /// Notes the switch the machine has been told. A note that cannot be written is a turn seen
    /// again at the next browser, and told again.
    pub fn remember(&self, on: bool) {
        let Some(folder) = self.path.parent() else {
            return;
        };
        if std::fs::create_dir_all(folder).is_err() {
            return;
        }
        let tmp = self.path.with_extension("tmp");
        let text = if on { "on\n" } else { "off\n" };
        if std::fs::write(&tmp, text).is_ok() {
            let _ = std::fs::rename(&tmp, &self.path);
        }
    }
}

/// A turn of the switch that the machine is yet to be told, as part of the sync of a profile.
#[derive(Clone, Debug)]
pub enum Turn {
    /// `off` to `on`: the machine's mark is taken away before the first connection, and the Mac
    /// then gives the profile it has.
    Lift(Mark),
    /// `on` to `off`: the Mac takes what the home has first, then the machine is told to keep
    /// none, and the sync ends.
    Close(Mark),
}

/// The depots that have been given, by machine: what a profile's sync looks its home up in. A
/// machine with none here keeps its profiles to itself.
#[derive(Default)]
pub struct Depots {
    homes: Mutex<HashMap<String, Arc<dyn Depot>>>,
}

impl Depots {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// `machine` keeps its profiles at `depot` from now on.
    pub fn set(&self, machine: &str, depot: Arc<dyn Depot>) {
        self.homes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(machine.to_owned(), depot);
    }

    pub fn get(&self, machine: &str) -> Option<Arc<dyn Depot>> {
        self.homes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(machine)
            .cloned()
    }

    pub fn remove(&self, machine: &str) {
        self.homes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(machine);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct None_;

    impl Depot for None_ {
        fn open(&self, _wait: Duration) -> Result<Link, String> {
            Err("no home".into())
        }
    }

    #[test]
    fn a_machine_has_the_depot_it_was_given_until_it_is_taken_back() {
        let depots = Depots::new();
        assert!(depots.get("server:a").is_none());
        depots.set("server:a", Arc::new(None_));
        assert!(depots.get("server:a").is_some());
        assert!(depots.get("server:b").is_none());
        depots.remove("server:a");
        assert!(depots.get("server:a").is_none());
    }
}
