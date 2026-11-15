//! Shell ▸ Remove bateri from “{host}”: the pane's half.
//!
//! The steps and their order are [`bt_shell_common::kablo::removal`]'s; this file is what shows
//! them and reaches the server for them: the question as a sheet of the pane, the login as the
//! pane's password sheet ([`Job::Remove`]), the connection as bateri's own master for the host
//! ([`crate::ssh_route::Masters::ensure`]) and the settings as the host's entry.
//!
//! **The connection is never a kept terminal's bridge**, even for a pane that shows one: the
//! script ends the terminals of the machine, the bridge is made of them, and what the script
//! does next (delete the folders, check that they are gone) would go over a connection that has
//! just been cut. What happens after the server is clean — the service's letting go of the
//! account, the settings' entry — is the application's ([`crate::pane::Removals`]), not the
//! pane's, which the script may have ended.
//!
//! **This is the one exception to `server_target`** (`rules/kablo.md`): the command is offered in
//! a pane that has an `ssh` running ([`Session::remote_target`]), plain or kept, and opens a
//! connection of bateri's own to the host when no kept one is up. The rule's reason is the
//! connection nobody asked for; this one is a command the person chose, and they confirm it —
//! and a server that was turned off, or reached by a plain `ssh` later, is one they must be able
//! to clean. It is grey unless this Mac put bateri on the server ([`TerminalPane::removal_state`]),
//! and nothing is asked of a server it did not ([`removal::run`]).
//!
//! The menu asks whether the item is usable on every opening, which cannot wait for `ssh -G`
//! (the key every record of a server is under, resolved from the user's ssh settings): a kept
//! pane knows its account's key, and a plain one's is asked of `ssh` once, on a thread of its own,
//! and remembered ([`KEYS`]). Until it is known the item is grey.

use std::collections::HashMap;
use std::sync::mpsc;
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;

use block2::RcBlock;
use bt_core::{RemoteKind, RemoteTarget};
use bt_shell_common::kablo::install;
use bt_shell_common::kablo::removal::{self, Outcome, Reach, World};
use bt_shell_common::kablo::service::Backend;
use bt_shell_common::ssh_route::{Denied, Masters};
use bt_shell_common::ssh_wrap::{self, Fact, KabloHost};
use objc2::MainThreadOnly;
use objc2_app_kit::{NSAlert, NSAlertFirstButtonReturn, NSModalResponse};
use objc2_foundation::{NSString, ns_string};

use crate::pane::{PaneLookup, Removals, TerminalPane};
use crate::password_sheet::Job;
use crate::sheets::{self, Asker, beep};
use crate::ssh_route::{Ask, SystemSsh};
use crate::uploader::on_pane;

/// The keys of the servers a plain pane's `ssh` reaches ([`ssh_wrap::server_key`]), by the
/// arguments of the `ssh` (joined): `None` while it is being asked, or when `ssh` could not say.
static KEYS: OnceLock<Mutex<HashMap<String, Option<String>>>> = OnceLock::new();

fn keys() -> &'static Mutex<HashMap<String, Option<String>>> {
    KEYS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// The arguments of an `ssh` as one word, the key of [`KEYS`].
fn argv_key(argv: &[String]) -> String {
    argv.join("\u{1}")
}

/// What the removal needs of the pane's `ssh`.
struct Target {
    /// As the person typed it (`rasp`, `deploy@10.0.0.5`): the name of the host's entry.
    host: String,
    /// What the question and the lines call it.
    label: String,
    /// The `ssh` for the connection of bateri's own.
    remote: RemoteTarget,
    /// The server as a kept terminal's account knows it, when the pane shows one.
    kept: Option<KabloHost>,
}

impl Target {
    /// The server's key: a kept terminal's account has it; a plain `ssh`'s is what `ssh -G` says,
    /// and is asked of `ssh` here (a call that waits: on a thread that may).
    fn key_waiting(&self) -> Option<String> {
        if let Some(kept) = &self.kept {
            return Some(kept.key.clone());
        }
        let word = argv_key(&self.remote.argv);
        if let Some(known) = lock_keys().get(&word).cloned().flatten() {
            return Some(known);
        }
        let key = ssh_wrap::server_key(&SystemSsh, &self.remote.argv);
        lock_keys().insert(word, key.clone());
        key
    }
}

fn lock_keys() -> std::sync::MutexGuard<'static, HashMap<String, Option<String>>> {
    keys()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The question before bateri is removed: its title and its text. `terminals` end with it.
pub(crate) fn question(label: &str, terminals: usize) -> (String, String) {
    let ending = match terminals {
        0 => "No terminal is running there.".to_owned(),
        1 => format!("The terminal running on {label} ends, with what runs in it."),
        many => format!("The {many} terminals running on {label} end, with what runs in them."),
    };
    (
        format!("Remove bateri from \u{201c}{label}\u{201d}?"),
        format!(
            "{ending} bateri's folder there (~/.local/share/bateri), with the programs it \
             installed on the server, is deleted. Nothing else on the server is touched. bateri \
             does not set the server up again unless you turn Shell Integration on for it."
        ),
    )
}

/// What the sheet after a removal says: what ended, and — when the settings could not be written —
/// that bateri may set the server up again.
pub(crate) fn removed_text(terminals: usize, entry_error: Option<&str>) -> String {
    let mut text = match terminals {
        0 => "Its folder there is deleted.".to_owned(),
        1 => "Its folder there is deleted and the terminal that ran there ended.".to_owned(),
        many => {
            format!("Its folder there is deleted and the {many} terminals that ran there ended.")
        }
    };
    if let Some(why) = entry_error {
        text.push_str(&format!(
            " The settings could not be written ({why}), so Shell Integration may set the server up \
             again: turn it off for this server in settings.toml."
        ));
    }
    text
}

/// The world [`removal::run`] goes through, for a pane: used on the thread that waits for the
/// login and for the answer.
struct PaneWorld {
    target: Target,
    host: String,
    backend: Arc<dyn Backend>,
    masters: Option<Arc<Masters>>,
    /// The application's side of the steps after the server is clean.
    removals: Option<Arc<dyn Removals>>,
    /// How the login is asked for; taken once, by the connection.
    ask: std::cell::Cell<Option<Ask>>,
    lookup: PaneLookup,
    pane: u64,
}

impl World for PaneWorld {
    fn key(&self) -> Option<String> {
        self.target.key_waiting()
    }

    fn channel(&self) -> Result<Box<dyn install::Channel>, Reach> {
        let Some(masters) = &self.masters else {
            return Err(Reach::Failed("this run keeps no connections".to_owned()));
        };
        let ask = self.ask.take().unwrap_or(Ask::Never);
        match masters.ensure(&self.target.remote, ask) {
            Ok(route) => Ok(removal::ssh_channel(bt_shell_common::upload::ssh_argv_for(
                &self.target.remote,
                &route,
            ))),
            Err(Denied::Cancelled) => Err(Reach::Cancelled),
            Err(denied) => Err(Reach::Failed(denied.text())),
        }
    }

    fn version(&self) -> Option<String> {
        self.backend.version()
    }

    fn confirm(&self, terminals: usize) -> bool {
        let (answer, answered) = mpsc::channel();
        let label = self.target.label.clone();
        on_pane(self.lookup, self.pane, move |pane| {
            pane.removal_question(&label, terminals, answer);
        });
        // A pane that is gone, a sheet that could not open: the sender is dropped, and that is no.
        answered.recv().unwrap_or(false)
    }

    fn removing(&self, key: &str) {
        if let Some(removals) = &self.removals {
            removals.removing(key);
        }
    }

    fn removal_failed(&self, key: &str) {
        if let Some(removals) = &self.removals {
            removals.removal_failed(key);
        }
    }

    fn account_gone(&self, key: &str) {
        if let Some(removals) = &self.removals {
            removals.account_removed(key);
        }
    }

    fn entry_off(&self) -> Result<(), String> {
        match &self.removals {
            Some(removals) => removals.entry_off(&self.host),
            None => Err("this run writes no settings".to_owned()),
        }
    }
}

impl TerminalPane {
    /// The server the pane's `ssh` is on, kept or plain; `None` locally and for `mosh`.
    /// **The named exception to `server_target`**, see the module.
    fn removal_target(&self) -> Option<Target> {
        let (_, remote, _) = self.session()?.remote_target()?;
        if remote.kind != RemoteKind::Ssh {
            return None;
        }
        let kept = self
            .host()
            .kept_terminal(self.uuid())
            .filter(|shown| shown.serves(&remote))
            .map(|shown| shown.server(remote.port));
        Some(Target {
            host: remote.host.clone(),
            label: remote.label().into_owned(),
            remote,
            kept,
        })
    }

    /// The menu's question: the server of the pane's `ssh` and whether this Mac put bateri on it.
    /// `None` locally. A server whose key is not known yet is not one: it is being asked of
    /// `ssh` and the menu is brought up to date when it is.
    pub(crate) fn removal_state(&self) -> Option<(String, bool)> {
        let target = self.removal_target()?;
        let key = match &target.kept {
            Some(kept) => Some(kept.key.clone()),
            None => self.plain_key(&target),
        };
        let touched = key.is_some_and(|key| {
            crate::child::home().is_some_and(|home| {
                ssh_wrap::load(&crate::remote_hosts_path(&home)).knows(Fact::Touched, &key)
            })
        });
        Some((target.label, touched))
    }

    /// A plain `ssh` has been found in the pane: its server's key is asked of `ssh` now, so that
    /// Shell ▸ Remove bateri from “{host}” is not grey the first time the menu is opened. A kept
    /// terminal's account has it already. Asks `ssh -G` only: no connection is made.
    pub(crate) fn warm_removal_key(&self) {
        if let Some(target) = self.removal_target()
            && target.kept.is_none()
        {
            let _ = self.plain_key(&target);
        }
    }

    /// A plain `ssh`'s key, if it is known; else it is asked of `ssh` on a thread of its own, once
    /// (an argv `ssh -G` cannot read is not asked again), and the menu is updated when it is.
    fn plain_key(&self, target: &Target) -> Option<String> {
        let word = argv_key(&target.remote.argv);
        {
            let mut known = lock_keys();
            if let Some(known) = known.get(&word) {
                return known.clone();
            }
            known.insert(word.clone(), None);
        }
        let argv = target.remote.argv.clone();
        let (id, lookup) = (self.id(), self.lookup());
        let _ = thread::Builder::new()
            .name("remote key".into())
            .spawn(move || {
                let key = ssh_wrap::server_key(&SystemSsh, &argv);
                lock_keys().insert(word, key);
                on_pane(lookup, id, |pane| pane.host().menu_changed());
            });
        None
    }

    /// Shell ▸ Remove bateri from “{host}”. A beep when there is nothing to remove from here or
    /// a sheet is up; else the steps of [`removal::run`] on a thread of their own, which asks the
    /// login and the question through this pane.
    pub(crate) fn remove_bateri_from_host(&self) {
        let (Some(target), Some(backend)) = (self.removal_target(), self.host().connections())
        else {
            beep();
            return;
        };
        if sheets::seat(Asker::Pane(self)).is_none_or(|seat| seat.is_taken()) {
            beep();
            return;
        }
        let ask = self.login_ask(&target.host, Some((Job::Remove, false)));
        let (pane, lookup) = (self.id(), self.lookup());
        let host = target.host.clone();
        let label = target.label.clone();
        let world = PaneWorld {
            host: target.host.clone(),
            target,
            backend,
            masters: self.masters(),
            removals: self.host().removals(),
            ask: std::cell::Cell::new(Some(ask)),
            lookup,
            pane,
        };
        let _ = thread::Builder::new()
            .name("remove bateri".into())
            .spawn(move || {
                let state = crate::child::home().map(|home| crate::remote_hosts_path(&home));
                let outcome = match &state {
                    Some(state) => removal::run(&host, state, &world),
                    None => Outcome::Refused("bateri cannot find its own folder.".to_owned()),
                };
                let (title, text) = match outcome {
                    Outcome::Removed {
                        terminals,
                        entry_error,
                    } => (
                        format!("bateri was removed from \u{201c}{label}\u{201d}"),
                        removed_text(terminals, entry_error.as_deref()),
                    ),
                    Outcome::Refused(why) => (
                        format!("bateri was not removed from \u{201c}{label}\u{201d}"),
                        why,
                    ),
                    Outcome::Declined | Outcome::Cancelled => return,
                };
                on_pane(lookup, pane, move |pane| pane.removal_notice(&title, &text));
            });
    }

    /// The question before the removal, as a sheet of the pane: its answer goes to `answer` — a
    /// yes only when the person pressed *Remove*. A beep, and the sender dropped (a no), when
    /// another sheet is up.
    pub(crate) fn removal_question(
        &self,
        label: &str,
        terminals: usize,
        answer: mpsc::Sender<bool>,
    ) {
        let Some(seat) = sheets::seat(Asker::Pane(self)).filter(|seat| !seat.is_taken()) else {
            beep();
            return;
        };
        let (title, message) = question(label, terminals);
        let alert = NSAlert::new(self.mtm());
        alert.setMessageText(&NSString::from_str(&title));
        alert.setInformativeText(&NSString::from_str(&message));
        alert.addButtonWithTitle(ns_string!("Remove"));
        let cancel = alert.addButtonWithTitle(ns_string!("Cancel"));
        // Esc by hand (the rationale of `window::alert`).
        cancel.setKeyEquivalent(ns_string!("\u{1b}"));
        let answered = RcBlock::new(move |response: NSModalResponse| {
            let _ = answer.send(response == NSAlertFirstButtonReturn);
        });
        seat.begin(&alert, &answered);
    }

    /// What came of the removal, as a sheet of the pane with one button; a beep when another
    /// sheet is up.
    fn removal_notice(&self, title: &str, text: &str) {
        let Some(seat) = sheets::seat(Asker::Pane(self)).filter(|seat| !seat.is_taken()) else {
            beep();
            return;
        };
        let alert = NSAlert::new(self.mtm());
        alert.setMessageText(&NSString::from_str(title));
        alert.setInformativeText(&NSString::from_str(text));
        alert.addButtonWithTitle(ns_string!("OK"));
        let answered = RcBlock::new(|_: NSModalResponse| {});
        seat.begin(&alert, &answered);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_question_counts_what_ends_and_says_what_goes_and_what_stays() {
        let (title, text) = question("rasp", 0);
        assert_eq!(title, "Remove bateri from \u{201c}rasp\u{201d}?");
        assert!(text.starts_with("No terminal is running there."), "{text}");
        let (_, one) = question("rasp", 1);
        assert!(
            one.starts_with("The terminal running on rasp ends"),
            "{one}"
        );
        let (_, three) = question("deploy@rasp:2222", 3);
        assert!(
            three.starts_with("The 3 terminals running on deploy@rasp:2222 end"),
            "{three}"
        );
        for text in [&text, &one, &three] {
            assert!(text.contains("~/.local/share/bateri"), "{text}");
            assert!(text.contains("programs it installed"), "{text}");
            assert!(
                text.contains("Nothing else on the server is touched"),
                "{text}"
            );
            assert!(text.contains("Shell Integration"), "{text}");
        }
    }

    #[test]
    fn the_sheet_after_a_removal_says_what_ended_and_when_the_settings_were_not_written() {
        assert_eq!(removed_text(0, None), "Its folder there is deleted.");
        assert!(removed_text(3, None).contains("the 3 terminals that ran there ended"));
        let text = removed_text(1, Some("settings.toml is not available"));
        assert!(text.contains("settings.toml is not available"), "{text}");
        assert!(text.contains("set the server up again"), "{text}");
    }

    #[test]
    fn an_ssh_is_told_from_another_by_all_its_arguments() {
        let a = argv_key(&[
            "ssh".to_owned(),
            "-p".to_owned(),
            "22".to_owned(),
            "rasp".to_owned(),
        ]);
        let b = argv_key(&["ssh".to_owned(), "-p2".to_owned(), "2 rasp".to_owned()]);
        assert_ne!(a, b);
    }
}
