//! The card a server asks with, once: *Programs you follow aren't on rasp*.
//!
//! A server that bateri is connected to tells its watch's `package state` which programs it has and
//! which it was told it need not have ([`bt_shell_common::apps::package`]); what the person
//! follows that it lacks ([`card::missing`]) waits here, per server, until a pane of that server is
//! ready for a question: it has come to a prompt, so the card is never the first thing a shell
//! shows. The card then hangs over the top right corner of that pane
//! ([`TerminalPane::set_corner_card`]): where a terminal's oldest lines are and not its input, so
//! it takes no key and the person keeps typing. It waits until it is answered. The first pane (of
//! the tabs, in order) that shows the server and is ready is the one; a server that has a card or
//! was answered one is not asked again while bateri runs.
//!
//! *Install* has the server's own kablo bring each program that is checked, one after the other
//! (`bateri package install`, run on the server's account over the connection bateri keeps to
//! it), and says each as it goes; a card that did not bring them all stays, with *Try Again*.
//! *Not Here* writes the names into the server's `not-here` and goes: that server does not ask
//! about them again. ⌘Return and ⌘Escape are those two buttons, from the Shell menu, and only
//! while a card asks (the menu greys them otherwise).
//!
//! What the card says and when it may be raised are decided in
//! [`bt_shell_common::apps::card`]; this file is the application's side of it — the panes, the
//! connection, the clock.

pub(crate) mod layout_check;
mod view;

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::thread;
use std::time::Duration;

use bt_core::{HostMark, Settings, host_mark};
use bt_shell_common::apps::card::{self, Candidate, Card, Ground, Row, Stage};
use bt_shell_common::apps::followed;
use dispatch2::{DispatchQueue, DispatchTime};
use objc2::MainThreadMarker;
use objc2::rc::Retained;

use crate::app;
use crate::hold_service;
use crate::pane::TerminalPane;
use crate::ssh_route::Ask;
use crate::tab::TerminalTab;
use view::{CardView, Target};

/// How long a card that has brought everything stays to say so.
const READY_SECONDS: u64 = 3;

/// What the person did on the card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Act {
    /// The box of this line was pressed.
    Toggle(usize),
    Install,
    NotHere,
    Retry,
    Close,
}

/// What a server lacks, waiting for a pane.
#[derive(Clone)]
struct Pending {
    host: String,
    /// The `ssh` arguments that reach the server.
    ssh: Vec<String>,
    rows: Vec<Row>,
}

/// A card on screen.
struct Open {
    pane: u64,
    /// The `ssh` arguments that reach the server.
    ssh: Vec<String>,
    /// What the card was made from, to ask again if its pane goes before it was answered.
    pending: Pending,
    card: Card,
    view: Retained<CardView>,
    /// The buttons' target: the view holds it, and so does this, for the next view.
    target: Retained<Target>,
}

#[derive(Default)]
struct Cards {
    /// By the server's account.
    pending: HashMap<String, Pending>,
    /// The panes that have come to a prompt.
    prompted: HashSet<u64>,
    /// The servers whose card was answered.
    answered: HashSet<String>,
    open: HashMap<String, Open>,
}

impl Cards {
    /// The pane of the card of `account` is gone, and the card with it. A card that had not been
    /// answered asks again over another pane; one that was cut off while it brought programs lets
    /// its server be asked at its next `state`, which says what is still missing.
    fn pane_closed(&mut self, account: &str) {
        let Some(open) = self.open.remove(account) else {
            return;
        };
        match open.card.stage() {
            Stage::Ask => {
                self.pending
                    .entry(account.to_owned())
                    .or_insert(open.pending);
            }
            Stage::Done => {}
            Stage::Installing | Stage::Failed => {
                self.answered.remove(account);
            }
        }
    }
}

thread_local! {
    /// The application's cards: the main thread's alone, like the views in them.
    static CARDS: RefCell<Cards> = RefCell::new(Cards::default());
}

fn with<T>(act: impl FnOnce(&mut Cards) -> T) -> T {
    CARDS.with(|cards| act(&mut cards.borrow_mut()))
}

// ─── what makes a card ────────────────────────────────────────────────────────

/// A server's watch said what it has, and `rows` is what it lacks of what the person follows.
/// Empty: nothing waits for it.
pub(crate) fn state_arrived(
    mtm: MainThreadMarker,
    account: &str,
    host: &str,
    ssh: Vec<String>,
    rows: Vec<Row>,
) {
    with(|cards| {
        if rows.is_empty() {
            cards.pending.remove(account);
        } else {
            cards.pending.insert(
                account.to_owned(),
                Pending {
                    host: host.to_owned(),
                    ssh,
                    rows,
                },
            );
        }
    });
    consider(mtm);
}

/// A pane's shell spoke (a prompt, a command): it is past its opening moment.
pub(crate) fn prompted(mtm: MainThreadMarker, pane: u64) {
    // A card whose pane closed asks again over another, so an open card is a reason to look too.
    let waiting = with(|cards| {
        cards.prompted.insert(pane);
        !cards.pending.is_empty() || !cards.open.is_empty()
    });
    if waiting {
        consider(mtm);
    }
}

/// Raises the card of each server that waits for one, over the pane that is ready.
fn consider(mtm: MainThreadMarker) {
    let Some(app) = app::delegate(mtm) else {
        return;
    };
    let panes = app.all_panes();
    let live: HashSet<u64> = panes.iter().map(|pane| pane.id()).collect();
    let waiting: Vec<(String, String)> = with(|cards| {
        cards.prompted.retain(|pane| live.contains(pane));
        let gone: Vec<String> = cards
            .open
            .iter()
            .filter(|(_, open)| !live.contains(&open.pane))
            .map(|(account, _)| account.clone())
            .collect();
        for account in gone {
            cards.pane_closed(&account);
        }
        cards
            .pending
            .iter()
            .map(|(account, pending)| (account.clone(), pending.host.clone()))
            .collect()
    });
    for (account, host) in waiting {
        let ground = {
            let settings = app.settings();
            let (answered, open) = with(|cards| {
                (
                    cards.answered.contains(&account),
                    cards.open.contains_key(&account),
                )
            });
            ground(&settings, &account, &host, answered, open)
        };
        if !ground.may_ask() {
            continue;
        }
        let candidates: Vec<Candidate> = with(|cards| {
            panes
                .iter()
                .map(|pane| Candidate {
                    on_server: hold_service::shown(pane.uuid())
                        .is_some_and(|shown| shown.account == account),
                    prompted: cards.prompted.contains(&pane.id()),
                })
                .collect()
        });
        let Some(at) = card::pane_for(&candidates) else {
            continue;
        };
        let Some(pending) = with(|cards| cards.pending.remove(&account)) else {
            continue;
        };
        let Some(card) = Card::new(&pending.host, pending.rows.clone()) else {
            continue;
        };
        let Some(pane) = panes.get(at) else {
            continue;
        };
        raise(mtm, pane, &account, pending, card);
    }
}

/// What the settings and the cards say about the server `host` (the account `account`).
fn ground(settings: &Settings, account: &str, host: &str, answered: bool, open: bool) -> Ground {
    Ground {
        local: bt_shell_common::kablo::local::is_account(account),
        integration: settings.integration_for(host),
        production: host_mark(&settings.remote_hosts, host) == HostMark::Production,
        answered,
        open,
    }
}

fn raise(mtm: MainThreadMarker, pane: &TerminalPane, account: &str, pending: Pending, card: Card) {
    let target = Target::new(mtm, account);
    let view = view::build(mtm, &card, &target, pane.bounds().size.height);
    pane.set_corner_card(Some(&view));
    with(|cards| {
        cards.open.insert(
            account.to_owned(),
            Open {
                pane: pane.id(),
                ssh: pending.ssh.clone(),
                pending,
                card,
                view,
                target,
            },
        );
    });
}

/// The card of `account` drawn again from its model, in the same pane.
fn redraw(mtm: MainThreadMarker, account: &str) {
    let Some(app) = app::delegate(mtm) else {
        return;
    };
    let Some((pane, card, target)) = with(|cards| {
        cards
            .open
            .get(account)
            .map(|open| (open.pane, open.card.clone(), Retained::clone(&open.target)))
    }) else {
        return;
    };
    let Some(pane) = app.all_panes().into_iter().find(|p| p.id() == pane) else {
        with(|cards| cards.pane_closed(account));
        return;
    };
    // A long list stays where it was scrolled to when a box is checked or a line settles.
    let offset = with(|cards| {
        cards
            .open
            .get(account)
            .and_then(|open| view::scrolled(&open.view))
    });
    let view = view::build(mtm, &card, &target, pane.bounds().size.height);
    if let Some(offset) = offset {
        view::scroll_to(&view, offset);
    }
    pane.set_corner_card(Some(&view));
    with(|cards| {
        if let Some(open) = cards.open.get_mut(account) {
            open.view = view;
        }
    });
}

/// The card goes.
fn remove(mtm: MainThreadMarker, account: &str) {
    let Some(open) = with(|cards| cards.open.remove(account)) else {
        return;
    };
    if let Some(app) = app::delegate(mtm)
        && let Some(pane) = app.all_panes().into_iter().find(|p| p.id() == open.pane)
    {
        pane.set_corner_card(None);
    }
}

// ─── what the person does ─────────────────────────────────────────────────────

/// The person pressed something on the card of `account`.
pub(super) fn act(mtm: MainThreadMarker, account: &str, act: Act) {
    match act {
        Act::Toggle(line) => {
            with(|cards| {
                if let Some(open) = cards.open.get_mut(account) {
                    open.card.toggle(line);
                }
            });
            redraw(mtm, account);
        }
        Act::Install => {
            let first = with(|cards| cards.open.get_mut(account).and_then(|o| o.card.begin()));
            if first.is_some() {
                with(|cards| cards.answered.insert(account.to_owned()));
            }
            redraw(mtm, account);
            if let Some(line) = first {
                bring(mtm, account, line);
            }
        }
        Act::Retry => {
            let first = with(|cards| cards.open.get_mut(account).and_then(|o| o.card.retry()));
            redraw(mtm, account);
            if let Some(line) = first {
                bring(mtm, account, line);
            }
        }
        Act::NotHere => {
            let Some((host, ssh, words)) = with(|cards| {
                cards.open.get(account).and_then(|open| {
                    (open.card.stage() == Stage::Ask).then(|| {
                        (
                            open.card.host().to_owned(),
                            open.ssh.clone(),
                            open.card.not_here_words(),
                        )
                    })
                })
            }) else {
                return;
            };
            with(|cards| cards.answered.insert(account.to_owned()));
            remove(mtm, account);
            // Nothing waits for the answer: the card is gone, and a server that could not be told
            // asks again when bateri next starts.
            let spawned = thread::Builder::new()
                .name("bateri programs".into())
                .spawn(move || {
                    if let Err(why) = on_server(&ssh, &host, &words) {
                        eprintln!("bateri: could not tell {host} that the programs are not wanted there: {why}");
                    }
                });
            if let Err(error) = spawned {
                eprintln!("bateri: no thread to tell a server about its programs: {error}");
            }
        }
        Act::Close => remove(mtm, account),
    }
}

/// Has the server bring line `line` of the card of `account`; what came of it is
/// [`finished`].
fn bring(mtm: MainThreadMarker, account: &str, line: usize) {
    let Some((host, ssh, words)) = with(|cards| {
        let open = cards.open.get(account)?;
        Some((
            open.card.host().to_owned(),
            open.ssh.clone(),
            open.card.words(line)?.to_vec(),
        ))
    }) else {
        return;
    };
    let account = account.to_owned();
    let account_again = account.clone();
    let spawned = thread::Builder::new()
        .name("bateri programs".into())
        .spawn(move || {
            let outcome = on_server(&ssh, &host, &words);
            DispatchQueue::main().exec_async(move || {
                // audit: a block running on the main queue is on the main thread by definition.
                let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
                finished(mtm, &account, line, outcome);
            });
        });
    if let Err(error) = spawned {
        eprintln!("bateri: no thread to bring a program to a server: {error}");
        // The line must not stay busy for ever: it failed, and the card says so.
        finished(
            mtm,
            &account_again,
            line,
            Err("bateri could not start the work".to_owned()),
        );
    }
}

/// Line `line` came, or did not.
fn finished(mtm: MainThreadMarker, account: &str, line: usize, outcome: Result<(), String>) {
    let Some((next, stage)) = with(|cards| {
        let open = cards.open.get_mut(account)?;
        let next = open.card.settle(line, outcome);
        Some((next, open.card.stage()))
    }) else {
        return;
    };
    redraw(mtm, account);
    match (next, stage) {
        (Some(line), _) => bring(mtm, account, line),
        (None, Stage::Done) => {
            // Ready: it says so for a moment and goes.
            let account = account.to_owned();
            if let Ok(when) = DispatchTime::try_from(Duration::from_secs(READY_SECONDS)) {
                let _ = DispatchQueue::main().after(when, move || {
                    // audit: a block running on the main queue is on the main thread by definition.
                    let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
                    let ready = with(|cards| {
                        cards
                            .open
                            .get(&account)
                            .is_some_and(|open| open.card.stage() == Stage::Done)
                    });
                    if ready {
                        remove(mtm, &account);
                    }
                });
            }
        }
        _ => {}
    }
}

/// `bateri WORDS` on the server `host` (reached with the `ssh` arguments `ssh`), which kablo
/// there runs for the person: over the connection bateri keeps to it, as a person would type it
/// there.
fn on_server(ssh: &[String], host: &str, words: &[String]) -> Result<(), String> {
    let backend = hold_service::backend()
        .ok_or_else(|| "this bateri keeps no connection to servers".to_owned())?;
    let server = backend
        .reach(ssh, Ask::Never, &hold_service::terminals_of)?
        .server;
    followed::on(&backend, server, host, words)
}

// ─── the menu ─────────────────────────────────────────────────────────────────

/// The account of the card over a pane of `tab`, when it asks: Shell ▸ Install Programs Here
/// (⌘Return) and Don't Install Programs Here (⌘Escape) act on it. `install` wants a box checked.
pub(crate) fn asking_in(tab: &TerminalTab, install: bool) -> Option<String> {
    // The card over the pane the person is in comes first, then the others in the tab's order: two
    // servers in one tab each have a card, and the keys are for the one they look at.
    let focused = tab.focused_pane().id();
    let mut order: Vec<u64> = vec![focused];
    order.extend(
        tab.panes()
            .iter()
            .map(|pane| pane.id())
            .filter(|id| *id != focused),
    );
    with(|cards| {
        order.iter().find_map(|id| {
            cards.open.iter().find_map(|(account, open)| {
                (open.pane == *id
                    && open.card.stage() == Stage::Ask
                    && (!install || open.card.can_install()))
                .then(|| account.clone())
            })
        })
    })
}

/// A menu item of the card was chosen for the card of `account`.
pub(crate) fn chosen(mtm: MainThreadMarker, account: &str, act_on: Act) {
    act(mtm, account, act_on);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(text: &str) -> Settings {
        Settings::parse(text).expect("valid settings").settings
    }

    fn asks(settings: &Settings, account: &str, host: &str) -> bool {
        ground(settings, account, host, false, false).may_ask()
    }

    #[test]
    fn a_server_with_integration_is_asked_and_this_computer_never_is() {
        let settings = settings("");
        assert!(asks(&settings, "u@dev-1:22", "dev-1"));
        assert!(!asks(&settings, "local", "dev-1"));
    }

    #[test]
    fn a_server_that_gets_no_integration_is_not_asked() {
        let off = settings("[remote]\nintegration = false\n");
        assert!(!asks(&off, "u@dev-1:22", "dev-1"));
        let host = settings("[remote]\nhosts = [{ host = \"dev-1\", integration = false }]\n");
        assert!(!asks(&host, "u@dev-1:22", "dev-1"));
        assert!(asks(&host, "u@dev-2:22", "dev-2"));
    }

    #[test]
    fn a_server_marked_production_is_not_asked_even_where_its_integration_is_turned_on() {
        let production =
            settings("[remote]\nhosts = [{ host = \"prod-*\", mark = \"production\" }]\n");
        assert!(!asks(&production, "u@prod-1:22", "prod-1"));
        let turned_on = settings(
            "[remote]\nhosts = [{ host = \"prod-*\", mark = \"production\", integration = true }]\n",
        );
        assert!(turned_on.integration_for("prod-1"));
        assert!(!asks(&turned_on, "u@prod-1:22", "prod-1"));
        let staging = settings("[remote]\nhosts = [{ host = \"dev-*\", mark = \"staging\" }]\n");
        assert!(asks(&staging, "u@dev-1:22", "dev-1"));
    }

    #[test]
    fn a_server_that_has_a_card_or_answered_one_is_not_asked_again() {
        let settings = settings("");
        assert!(!ground(&settings, "u@dev-1:22", "dev-1", true, false).may_ask());
        assert!(!ground(&settings, "u@dev-1:22", "dev-1", false, true).may_ask());
    }
}
