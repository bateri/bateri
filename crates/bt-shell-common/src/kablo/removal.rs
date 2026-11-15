//! Shell ▸ Remove bateri from “{host}”: the steps, apart from what shows them.
//!
//! The command takes **everything bateri put on a server** off it, once the person has said so:
//! the terminals this and other computers keep there end, the folders bateri keeps its things in
//! are deleted ([`install::remove`]), what this Mac remembers of the server is forgotten, and the
//! host's entry in the settings says `integration = false` — without that line a Mac that sets
//! every server up (`"auto"`) would put it all back on the next `ssh`.
//!
//! What it goes through is [`World`], so that the order of the steps and what each refusal says
//! are tested without a window or a server: the application's one asks the server's `ssh`, shows
//! the question in the pane and writes the settings; a test's answers from a table. [`run`] is
//! called on a thread of its own: it waits for a login and for the answer to the question.
//!
//! The order is the rule: nothing is asked of a server bateri has not put its files on from this
//! Mac (no login is asked for it either), nothing is removed from one whose terminals could not
//! be listed, and nothing is forgotten here before the server is clean — a removal that failed
//! half way can be made again, because what it comes back to is still known.
//!
//! **What is done after the server is clean does not depend on the pane that asked.** Ending the
//! terminals of the server may end the pane the person asked from (it showed one of them), and a
//! tab can be closed while the removal waits for a login: the service's letting go of the account
//! and the settings' entry go through the owner of the application ([`World::account_gone`],
//! [`World::entry_off`]), never through a pane, and a removal that cannot say it in the end says
//! so in [`Outcome::Removed`].

use std::path::Path;

use bt_core::SettingsEdit;

use super::install::{self, Channel, remove};
use crate::ssh_wrap::{self, Fact};

/// Why a way to the server could not be made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reach {
    /// The login was asked for and the person did not give it: nothing to say.
    Cancelled,
    /// The connection could not be made; the line says why.
    Failed(String),
}

/// What the removal does not decide itself.
pub trait World {
    /// The key of the server's account (`user@host:port`, [`ssh_wrap::server_key`]); `None` when
    /// `ssh` cannot say it.
    fn key(&self) -> Option<String>;

    /// A way to run scripts on the server: a connection of bateri's own over its master, which
    /// asks for the login when there is none. Never the bridge of a terminal's account — the
    /// terminals the script ends are what that connection is made of. Waits for it.
    fn channel(&self) -> Result<Box<dyn Channel>, Reach>;

    /// The version of the kablo this bateri brings ([`remove::script`] prefers it to find kablo).
    fn version(&self) -> Option<String>;

    /// The question: `terminals` is how many end with it. `true` is a yes. Waits for the answer.
    fn confirm(&self, terminals: usize) -> bool;

    /// The question was answered yes and the server is about to be taken off: the service writes
    /// nothing to its account until [`World::account_gone`] or [`World::removal_failed`]
    /// ([`super::service::Service::removing`]).
    fn removing(&self, key: &str);

    /// The removal did not finish: the service serves the account again
    /// ([`super::service::Service::removal_failed`]).
    fn removal_failed(&self, key: &str);

    /// The server has nothing of bateri's any more: the service lets go of its account
    /// ([`super::service::Service::removed`]).
    fn account_gone(&self, key: &str);

    /// Writes `integration = false` in the host's entry of the settings ([`entry_off_edit`]).
    /// `Err` is the line the settings could not be written for.
    fn entry_off(&self) -> Result<(), String>;
}

/// The edit that turns bateri off for the server `host` (as the person typed it): the host's own
/// entry says `integration = false`, which wins over `[remote] integration = "auto"`, so no later
/// `ssh` sets the server up again.
pub fn entry_off_edit(host: &str) -> SettingsEdit {
    SettingsEdit::RemoteHostIntegration {
        host: host.to_owned(),
        on: false,
    }
}

/// How a removal ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Everything of bateri's is off the server; this many terminals ended with it.
    /// `entry_error` is why the settings could not say `integration = false` for the host, if
    /// they could not: the server is clean all the same, and `"auto"` would set it up again.
    Removed {
        terminals: usize,
        entry_error: Option<String>,
    },
    /// The person said no.
    Declined,
    /// The person gave no login: nothing was done and nothing is said.
    Cancelled,
    /// Nothing was removed (or not all of it); the line says why, for the person.
    Refused(String),
}

/// Removes bateri from the server of `host` (as the person typed it): see the module. `state` is
/// the file of what this Mac knows of servers (`remote-hosts`).
pub fn run(host: &str, state: &Path, world: &dyn World) -> Outcome {
    let Some(key) = world.key() else {
        return Outcome::Refused(format!(
            "bateri cannot tell from your ssh settings how to reach {host}."
        ));
    };
    // A server bateri never wrote to from this Mac has nothing of ours that this Mac may take off
    // (what another computer put there is that computer's), and it is not asked for a login.
    if !ssh_wrap::load(state).knows(Fact::Touched, &key) {
        return Outcome::Refused(format!(
            "bateri has not put anything on {host} from this Mac."
        ));
    }
    let channel = match world.channel() {
        Ok(channel) => channel,
        Err(Reach::Cancelled) => return Outcome::Cancelled,
        Err(Reach::Failed(why)) => {
            return Outcome::Refused(format!("bateri could not reach {host}: {why}"));
        }
    };
    let version = world.version().unwrap_or_default();
    // The machine's terminals, all of them: kablo's folder is what they run from.
    let Some(census) = remove::look(&*channel, &version) else {
        return Outcome::Refused(format!(
            "bateri could not list the terminals on {host}, so nothing was removed."
        ));
    };
    let ids = remove::running(&census);
    if !world.confirm(ids.len()) {
        return Outcome::Declined;
    }
    world.removing(&key);
    if let Err(why) = remove::remove(&*channel, &version, &ids) {
        world.removal_failed(&key);
        return Outcome::Refused(format!(
            "bateri could not finish removing itself from {host}: {why}. Nothing was forgotten here, so the command can be run again."
        ));
    }
    // The server is clean: what this Mac believed of it goes, and the entry says no more.
    let _ = ssh_wrap::forget_server(state, &key);
    world.account_gone(&key);
    let entry_error = world.entry_off().err();
    Outcome::Removed {
        terminals: ids.len(),
        entry_error,
    }
}

/// How long the steps over a channel of the removal's own may take: [`remove::LIMIT`], the
/// longest of them.
pub const LIMIT: std::time::Duration = remove::LIMIT;

/// The server's way in for [`World::channel`] when there is a connection of ours to ride on or
/// to make: `argv` is [`crate::upload::ssh_argv_for`] of the route the connection was given.
pub fn ssh_channel(argv: Vec<String>) -> Box<dyn Channel> {
    Box::new(install::SshChannel { argv, limit: LIMIT })
}

#[cfg(test)]
mod tests;
