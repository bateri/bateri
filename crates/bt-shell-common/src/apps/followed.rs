//! The programs the person follows, on this computer's own account.
//!
//! The programs on a machine are kablo's, this computer included: what [`super::check`] lands in
//! bateri's own folder is a copy to look at and to answer from ([`super::package`]), and a program
//! is on this computer when this computer's kablo has put it in use. So the app asks it to, the
//! way a person would: `bateri package install SOURCE --trust KEY`, run on the account of this
//! computer, where `KEY` is the key the person approved for the program on the Apps page. kablo
//! asks the app back for the release, judges it, and fetches it; a program that is there at that
//! version is left as it is.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use super::store;
use crate::agent::LOCAL_SERVER;
use crate::agent::relay;
use crate::kablo::served::Served;
use crate::kablo::service::{Backend, Gone};
use crate::ssh_wrap::KabloHost;

/// How long a machine is let take to answer when `bateri package install` (or another of the
/// words) is asked of it: the connection and the start. The command is let run as long as it does.
const INSTALL_WAIT: Duration = Duration::from_secs(30);

/// What this computer's kablo is asked to install for the followed program `name` that landed
/// from `address`: the words of the `bateri` command.
pub(super) fn install_words(root: &Path, name: &str, address: &str) -> Vec<String> {
    let mut words = vec![
        "package".to_owned(),
        "install".to_owned(),
        address.to_owned(),
    ];
    if let Some(key) = store::approved_key(root, name) {
        words.push("--trust".to_owned());
        words.push(key);
    }
    words
}

/// Asks this computer's kablo to install each program that has landed in `root`, one after the
/// other, and says each one that could not be. A program that is already there at its version is
/// not an error; one whose key changed since the person approved it (the command's exit 77) waits
/// for the person on the Apps page and is not reported here.
pub fn install(backend: &Arc<dyn Backend>, root: &Path) -> Vec<(String, String)> {
    install_each(backend, root, store::followed_names(root))
}

/// The same for the followed programs this computer does not have yet. `here` is what its kablo
/// said it has (`package state`): a program that is there is left to the machine's own updating,
/// which the person may have turned off (`bateri package auto off`), so that a kablo telling its
/// state again every day does not bring a newer version in through this door.
pub fn install_missing(
    backend: &Arc<dyn Backend>,
    root: &Path,
    here: &[String],
) -> Vec<(String, String)> {
    install_each(backend, root, missing(root, here))
}

/// The followed programs of `root` that are not among the names in `here`.
fn missing(root: &Path, here: &[String]) -> Vec<(String, String)> {
    let mut wanted = store::followed_names(root);
    wanted.retain(|(name, _)| !here.contains(name));
    wanted
}

fn install_each(
    backend: &Arc<dyn Backend>,
    root: &Path,
    wanted: Vec<(String, String)>,
) -> Vec<(String, String)> {
    let served = Served::new(Arc::clone(backend), crate::kablo::local::host());
    let mut problems = Vec::new();
    for (name, address) in wanted {
        let words = install_words(root, &name, &address);
        match relay::run_for(&served, LOCAL_SERVER, &words, &Gone::new(), INSTALL_WAIT) {
            Ok(done) if done.status == 0 || done.status == 77 => {}
            Ok(done) => problems.push((
                name,
                done.stderr
                    .first()
                    .cloned()
                    .unwrap_or_else(|| format!("it ended with {}", done.status)),
            )),
            Err(why) => problems.push((name, why)),
        }
    }
    problems
}

/// Asks this computer's kablo to take `name` off this computer (`bateri package remove`).
pub fn remove(backend: &Arc<dyn Backend>, name: &str) -> Result<(), String> {
    let words = ["package".to_owned(), "remove".to_owned(), name.to_owned()];
    on(backend, crate::kablo::local::host(), LOCAL_SERVER, &words)
}

/// Runs `bateri WORDS` on a machine (`host`, as the connection to it is kept; `server` is the name
/// it is told by) and says how it ended: `Err` is a line for the person when it did not end well.
/// A machine's kablo is asked what a person would type there; this waits as long as the command
/// does, so it is called on a thread of its own.
pub fn on(
    backend: &Arc<dyn Backend>,
    host: KabloHost,
    server: &str,
    words: &[String],
) -> Result<(), String> {
    let served = Served::new(Arc::clone(backend), host);
    let done = relay::run_for(&served, server, words, &Gone::new(), INSTALL_WAIT)?;
    if done.status == 0 {
        Ok(())
    } else {
        Err(done
            .stderr
            .first()
            .cloned()
            .unwrap_or_else(|| format!("it ended with {}", done.status)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::TempRoot;

    #[test]
    fn a_followed_program_is_installed_by_its_address_with_the_key_that_was_approved() {
        let root = TempRoot::new("followed-words");
        let folder = &root.0;
        store::follow(folder, "textpen", "https://example.org/textpen/app.json").expect("followed");
        assert_eq!(
            install_words(folder, "textpen", "https://example.org/textpen/app.json"),
            ["package", "install", "https://example.org/textpen/app.json"]
        );
        store::approve(folder, "textpen", "ed25519:KEY=").expect("approved");
        assert_eq!(
            install_words(folder, "textpen", "https://example.org/textpen/app.json"),
            [
                "package",
                "install",
                "https://example.org/textpen/app.json",
                "--trust",
                "ed25519:KEY="
            ]
        );
    }

    #[test]
    fn only_the_followed_programs_the_computer_does_not_have_are_asked_for_again() {
        let root = TempRoot::new("followed-missing");
        let folder = &root.0;
        store::follow(folder, "textpen", "https://example.org/textpen/app.json").expect("followed");
        store::follow(folder, "pencil", "https://example.org/pencil/app.json").expect("followed");
        let names = |here: &[&str]| -> Vec<String> {
            let here: Vec<String> = here.iter().map(|n| (*n).to_owned()).collect();
            missing(folder, &here)
                .into_iter()
                .map(|(name, _)| name)
                .collect()
        };
        assert_eq!(names(&[]), ["pencil", "textpen"]);
        assert_eq!(names(&["textpen"]), ["pencil"]);
        assert_eq!(names(&["textpen", "pencil", "other"]), Vec::<String>::new());
    }
}
