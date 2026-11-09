//! The programs the person follows, on this computer's own account.
//!
//! The programs on a machine are kablo's, this computer included: what [`super::check`] lands in
//! bateri's own folder is a copy to look at and to answer from ([`super::package`]), and a program
//! is on this computer when this computer's kablo has put it in use. So the app asks it to, the
//! way a person would: `bateri package install SOURCE --trust KEY`, run on the account of this
//! computer, where `KEY` is the key the person approved for the program on the Apps page. kablo
//! asks the app back for the release, judges it, and fetches it; a program that is there at that
//! version is left as it is.
//!
//! **Only what is not here.** The app asks for the followed programs this computer lacks, and
//! only those: a program that is there is the machine's own to update, and its updating may be
//! off (`bateri package auto off`, the rule of kablo on every machine, this one too). So the ask
//! is made with the names the computer said it has in its `package state`, which comes when its
//! watch starts and again each day while updating is off, and a computer that has not said yet is
//! asked for nothing: its state comes and asks for the same.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use super::store;
use crate::agent::LOCAL_SERVER;
use crate::agent::message::Relayed;
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

/// Asks this computer's kablo to install the followed programs it does not have yet, one after the
/// other, and says each one that could not be. `here` is the names it said it has; `None` is a
/// computer that has not said, which is asked for nothing. A program that is already there at its
/// version is not an error; one whose key changed since the person approved it (the command's
/// exit 77) waits for the person on the Apps page and is not reported here.
pub fn install_missing(
    backend: &Arc<dyn Backend>,
    root: &Path,
    here: Option<&[String]>,
) -> Vec<(String, String)> {
    let served = Served::new(Arc::clone(backend), crate::kablo::local::host());
    install_each(root, missing(root, here), &mut |words| {
        relay::run_for(&served, LOCAL_SERVER, words, &Gone::new(), INSTALL_WAIT)
    })
}

/// The followed programs of `root` that are not among the names in `here`; none when the computer
/// has not said what it has.
fn missing(root: &Path, here: Option<&[String]>) -> Vec<(String, String)> {
    let Some(here) = here else {
        return Vec::new();
    };
    let mut wanted = store::followed_names(root);
    wanted.retain(|(name, _)| !here.contains(name));
    wanted
}

/// Runs the install of each of `wanted` with `run`, and says each one that could not be.
fn install_each(
    root: &Path,
    wanted: Vec<(String, String)>,
    run: &mut dyn FnMut(&[String]) -> Result<Relayed, String>,
) -> Vec<(String, String)> {
    let mut problems = Vec::new();
    for (name, address) in wanted {
        match run(&install_words(root, &name, &address)) {
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

    /// Follows `textpen` (approved) and `pencil`, and says the words of every install that
    /// `install_each` runs for a computer that has `here`.
    fn asked_when_it_has(
        here: Option<&[&str]>,
        status: u8,
    ) -> (Vec<Vec<String>>, Vec<(String, String)>) {
        // One folder for each call: the tests run side by side.
        static CALLS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let call = CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let root = TempRoot::new(&format!("followed-missing-{call}"));
        let folder = &root.0;
        store::follow(folder, "textpen", "https://example.org/textpen/app.json").expect("followed");
        store::follow(folder, "pencil", "https://example.org/pencil/app.json").expect("followed");
        store::approve(folder, "textpen", "ed25519:KEY=").expect("approved");
        let here: Option<Vec<String>> =
            here.map(|names| names.iter().map(|name| (*name).to_owned()).collect());
        let mut ran = Vec::new();
        let problems = install_each(folder, missing(folder, here.as_deref()), &mut |words| {
            ran.push(words.to_vec());
            Ok(Relayed::of(status, "it did not work\n", ""))
        });
        (ran, problems)
    }

    #[test]
    fn a_program_the_computer_has_is_not_asked_for_again_and_one_it_lacks_is() {
        let (ran, _) = asked_when_it_has(Some(&["textpen"]), 0);
        assert_eq!(
            ran,
            [["package", "install", "https://example.org/pencil/app.json"]]
        );
        let (ran, _) = asked_when_it_has(Some(&[]), 0);
        assert_eq!(
            ran,
            [
                vec!["package", "install", "https://example.org/pencil/app.json"],
                vec![
                    "package",
                    "install",
                    "https://example.org/textpen/app.json",
                    "--trust",
                    "ed25519:KEY="
                ],
            ]
        );
        let (ran, _) = asked_when_it_has(Some(&["textpen", "pencil", "other"]), 0);
        assert!(ran.is_empty(), "everything is there: {ran:?}");
    }

    #[test]
    fn a_computer_that_has_not_said_what_it_has_is_asked_for_nothing() {
        let (ran, problems) = asked_when_it_has(None, 0);
        assert!(ran.is_empty(), "its state asks for the same: {ran:?}");
        assert!(problems.is_empty());
    }

    #[test]
    fn a_program_that_could_not_be_installed_is_told_but_a_key_waiting_for_the_person_is_not() {
        let (_, problems) = asked_when_it_has(Some(&[]), 1);
        assert_eq!(
            problems,
            [
                ("pencil".to_owned(), "it did not work".to_owned()),
                ("textpen".to_owned(), "it did not work".to_owned()),
            ]
        );
        let (_, problems) = asked_when_it_has(Some(&[]), 77);
        assert!(problems.is_empty(), "{problems:?}");
    }
}
