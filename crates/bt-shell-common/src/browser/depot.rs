//! Where a profile is kept when its program runs on a server: the server's side of it, as far as
//! the browser's code knows.
//!
//! The browser code asks for a connection to the **home** of a profile and speaks the home's
//! protocol on it (`sync`); it knows nothing of what carries the connection, as the proxy knows
//! nothing of what makes the connections of a page ([`super::route::Dialer`]). The shell gives a
//! machine its depot when the way to the machine has been shown, and takes it back when the
//! person keeps no profiles there ([`Depots::remove`]).

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

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
}

/// What the server runs to be the home of a profile: the newest kablo that is installed there,
/// told to keep profiles (`kablo profile`: it speaks the profile's protocol on its standard input
/// and output). A server that has none (or one that is too old to know the command) ends the
/// command at once and the connection is no connection.
pub fn home_script() -> String {
    let root = kablo_proto::install::ROOT;
    format!(
        "d=\"$HOME/{root}\"; \
         v=$(ls -1 \"$d\" 2>/dev/null | {{ sort -V 2>/dev/null || sort; }} | tail -n 1); \
         [ -n \"$v\" ] || exit 127; exec \"$d/$v/kablo\" profile"
    )
}

/// The home of a program that runs on a server: the command the server's `sh` runs, over the
/// connection bateri keeps to it.
pub struct ServedDepot(pub Served);

impl Depot for ServedDepot {
    fn open(&self, wait: Duration) -> Result<Link, String> {
        let RemoteCommand {
            stdin,
            stdout,
            stderr,
            end,
        } = self.0.sh(&home_script(), Ask::Never, wait)?;
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
