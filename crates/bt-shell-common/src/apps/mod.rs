//! The programs bateri keeps up to date: the sources of `[apps] sources`, followed.
//!
//! A source is the address of an `app.json` — the file a program's author signs and publishes with
//! each release (pencere's `spec/draft/apps.md`). At launch and once a day a worker fetches it and
//! the signature beside it, reads it with `pencere-host`'s reader (which does no I/O and trusts no
//! key), and acts on what it says:
//!
//! 1. **Trust.** The key the release names must be one bateri trusts for that name: the key that
//!    comes in the application ([`BATERI_KEY`]), or the one the person approved ([`store::approve`]).
//!    A program of a key nobody approved waits, with nothing downloaded but its `app.json`; a
//!    release signed with another key than the approved one stops there too, and asks again.
//! 2. **Selection.** The newest release among this one and its `previous` that this bateri opens,
//!    by the rule the handshake uses ([`bt_core::pencere::offered_views`] and the protocol
//!    versions the session speaks). A release that none opens waits for a bateri update.
//! 3. **Landing.** The program's file for this machine's target is downloaded, its SHA-256 sum
//!    checked against the signed one, and the release put in its folder whole ([`store`]).
//!
//! A program that leaves the list is deleted from this Mac at once and its name remembered for 30
//! days ([`store::removed`]); the check that deletes it is also the one that **cannot be wrong
//! about the list** (`authoritative`): a settings file that could not be read at launch is
//! the defaults, and the defaults are not what the person wrote.
//!
//! **Nothing here blocks the window.** [`Tracker`] runs on a thread of its own, the shell asks it
//! for [`Status`]es when it draws something, and every failure is a [`Failure`] in a status, never
//! a panic or a dialog.
//!
//! The network is [`fetch::Curl`] behind the [`Fetch`] seam; the folder is [`store`]; the rules
//! are [`check`]. The shell gives the folder (`~/Library/Application Support/bateri/apps`).

mod check;
pub mod distribute;
pub mod fetch;
pub mod store;
mod worker;

use std::fmt;
use std::path::PathBuf;

use bt_core::pencere::apps::{self, Version};
use bt_core::pencere::{Offer, SUPPORTED_VERSIONS, offered_views};

pub use check::{check, fetch_target};
pub use distribute::Distributor;
pub use fetch::{Curl, Fetch};
pub use worker::{After, Tracker};

/// The public key the programs bateri itself publishes are signed with (`ed25519:` and base64),
/// which comes in the application: a release signed with it is taken without asking.
///
/// The public half of the key its owner made once (the secret half is theirs alone: no program
/// makes it, and the tests here sign with keys they invent). It is `Option` so that a build with no
/// key to name — a fork — has textpen wait for approval like any other program.
pub const BATERI_KEY: Option<&str> = Some("ed25519:59e8IXkv6FP4tD4uJ/fvw9/pSV+1YgEpXtXQzWLwOpU=");

/// Names a program may not have: the commands bateri and its servers already have.
pub const RESERVED_NAMES: &[&str] = &["bateri", "kablo", "pencere"];

/// What a check needs to know about this machine and this bateri.
#[derive(Debug, Clone)]
pub struct Config {
    /// The apps folder.
    pub root: PathBuf,
    /// The target whose file this machine runs: a key of `files` (`aarch64-apple-darwin`).
    pub target: String,
    /// The protocol versions this bateri speaks to a program.
    pub protocols: Vec<u32>,
    /// The kinds of view it opens and the versions of each.
    pub views: Vec<Offer>,
    /// The key that is trusted without asking ([`BATERI_KEY`]).
    pub in_app_key: Option<String>,
}

impl Config {
    /// This machine and this bateri, with the apps folder `root`. `None` where this machine has no
    /// target name ([`this_target`]).
    pub fn for_this_machine(root: PathBuf) -> Option<Self> {
        Some(Self {
            root,
            target: this_target()?.to_owned(),
            protocols: SUPPORTED_VERSIONS.to_vec(),
            views: offered_views(),
            in_app_key: BATERI_KEY.map(str::to_owned),
        })
    }
}

/// The name of the target this machine's programs are made for.
pub fn this_target() -> Option<&'static str> {
    if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        Some("aarch64-apple-darwin")
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        Some("x86_64-apple-darwin")
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        Some("x86_64-unknown-linux-musl")
    } else if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
        Some("aarch64-unknown-linux-musl")
    } else {
        None
    }
}

/// How one source stands, after a check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    /// The entry of `[apps] sources`, as written.
    pub source: String,
    /// The program's name, once its `app.json` was read (or, if it could not be now, from the
    /// release that landed before).
    pub name: Option<String>,
    /// What the program is.
    pub state: State,
    /// Why the last check could not finish, when something usable stays (`state` is `Ready`): the
    /// network was down, the file did not match.
    pub problem: Option<Failure>,
}

/// The states of a source: the rows of the Apps page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    /// A release is in its folder and runs on this machine.
    Ready {
        /// The release that is in use.
        version: Version,
        /// The newest release, when it is newer but this bateri cannot open it: it comes with an
        /// update of bateri.
        newer: Option<Version>,
    },
    /// The release's key is not approved: a source's first key, or a key that changed. Nothing but
    /// the `app.json` was downloaded.
    Awaiting {
        /// The release waiting.
        version: Version,
        /// The key it is signed with (`ed25519:` and base64): what the person is asked to approve.
        key: String,
        /// The program had another key approved: its author changed key, or someone else
        /// publishes at this address.
        changed: bool,
    },
    /// No release of the program opens in this bateri and none landed before: "will come when
    /// bateri updates".
    WaitsForUpdate {
        /// The newest release, which needs more than this bateri has.
        version: Version,
    },
    /// The release has no file for this machine's target: there is nothing to run here.
    NotHere {
        /// The release.
        version: Version,
    },
    /// Nothing could be done and nothing usable is in the folder.
    Error(Failure),
}

/// What went wrong with a source, in words for the Apps page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    /// The entry names no `app.json` address.
    Source,
    /// An address could not be fetched: the first line `curl` said.
    Fetch(String),
    /// The `app.json` or its signature does not hold.
    Manifest(apps::Error),
    /// The name is one of [`RESERVED_NAMES`].
    Reserved,
    /// Another source already has the name.
    NameTaken,
    /// Two files that must agree do not: an entry of `previous` and the `app.json` it names.
    Mismatch(&'static str),
    /// An `https:` source names a `file:` address, which a page on the network may not do.
    Unreachable,
    /// The program's file is not the one the signed `app.json` sums.
    Digest,
    /// The folder could not be written.
    Disk(String),
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Source => f.write_str("not the address of an app.json"),
            Self::Fetch(why) | Self::Disk(why) => f.write_str(why),
            Self::Manifest(why) => write!(f, "app.json: {why}"),
            Self::Mismatch(why) => f.write_str(why),
            Self::Reserved => {
                f.write_str("bateri, kablo and pencere are not names a program may have")
            }
            Self::NameTaken => f.write_str("another program already has this name"),
            Self::Unreachable => {
                f.write_str("a release on the network may not point at a file on this Mac")
            }
            Self::Digest => f.write_str("the file does not match the sum its app.json signs"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_key_that_comes_in_the_application_is_an_ed25519_public_key() {
        let key = BATERI_KEY.expect("the key comes in the application");
        // `pencere-host`'s reader refuses a `key` that is not a point of the curve in the text
        // the specification gives.
        let json = format!(
            r#"{{"name":"demo","version":"1.0.0","protocol":[1],"views":[{{"kind":"lua","versions":["0.1"]}}],"key":"{key}","files":{{"aarch64-apple-darwin":{{"url":"https://example.test/demo","sha256":"{}"}}}},"previous":[]}}"#,
            "0".repeat(64)
        );
        let manifest = apps::parse(json.as_bytes()).expect("a release naming that key");
        assert_eq!(manifest.key, key);
    }

    #[test]
    fn this_machine_trusts_that_key_and_opens_what_a_pane_opens() {
        let Some(config) = Config::for_this_machine(PathBuf::from("/nonexistent")) else {
            return;
        };
        assert_eq!(config.in_app_key.as_deref(), BATERI_KEY);
        assert_eq!(config.protocols, SUPPORTED_VERSIONS);
        assert_eq!(config.views, offered_views());
    }
}
