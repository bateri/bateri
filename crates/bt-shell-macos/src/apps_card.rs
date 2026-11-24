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
//! it), says each as it goes, and first tells the server the rows left unchecked are not wanted
//! (`bateri package not-here`); a card that did not bring them all stays, with *Try Again*.
//! *Not Here* writes the names into the server's `not-here` and goes: that server does not ask
//! about them again. ⌘Return and ⌘Escape are those two buttons, from the Shell menu, and only
//! while a card asks (the menu greys them otherwise).
//!
//! The same card, in another shape, asks the first ⌘-click on a web address, before anything is
//! decided: *Open links beside your terminal with browserpen?*, over the pane clicked in, whatever
//! machine it shows (`Card::for_here`, kept under this Mac's own account). Its yes writes the choice
//! and brings what is not here (browserpen, the browser engine), says how far it has come, and opens
//! the address in browserpen when it is there; its other answer opens the address in the system's
//! browser. The address is not opened before it is answered.
//!
//! What the card says and when it may be raised are decided in
//! [`bt_shell_common::apps::card`]; this file is the application's side of it — the panes, the
//! connection, the clock.

pub(crate) mod layout_check;
pub(crate) mod notice;
mod view;

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::thread;
use std::time::Duration;

use bt_core::Settings;
use bt_shell_common::apps::card::{self, Candidate, Card, Coming, Ground, Row, Stage, Yes};
use bt_shell_common::apps::followed;
use bt_shell_common::kablo::local;
use bt_shell_common::web_links;
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

/// The key of the card of the first click's question in [`Cards::open`]: this Mac's own account.
/// There is one such card, over the pane that was clicked in.
const HERE: &str = local::ACCOUNT;

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
    /// What `view` shows: `card` as it was when the view was made or last brought up to date.
    drawn: Card,
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
    /// The programs a click's card brought to a server, by the server's account: the server says
    /// it has them at its next `state` (which clears them), and until then the next click must not
    /// ask again.
    brought: HashSet<(String, String)>,
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
            // A click's card is for that click and goes with it: the server is asked about the
            // programs it lacks only as it always is, and the address is not opened anywhere (the
            // pane it was clicked in is gone).
            Stage::Ask if open.card.link().is_some() => {}
            Stage::Ask => {
                self.pending
                    .entry(account.to_owned())
                    .or_insert(open.pending);
            }
            Stage::Done => {}
            // A click's card never marked its server answered: what the person said to the card of
            // the programs they follow stands.
            Stage::Installing | Stage::Failed if open.card.link().is_some() => {}
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
        // The server has said what it has: that is what the next click goes by, not what a card
        // of a click brought before it said.
        cards
            .brought
            .retain(|(brought_to, _)| brought_to != account);
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
                    // A pane that holds a card has no room for a second.
                    prompted: cards.prompted.contains(&pane.id())
                        && !cards.open.values().any(|open| open.pane == pane.id())
                        && !notice::has(pane.id()),
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
        integration: settings.integration_for(host).is_on(),
        answered,
        open,
    }
}

fn raise(mtm: MainThreadMarker, pane: &TerminalPane, account: &str, pending: Pending, card: Card) {
    let target = Target::new(mtm, account);
    let view = view::build(
        mtm,
        &card,
        &target,
        pane.bounds().size,
        pane.corner_card_top(),
    );
    pane.set_corner_card(Some(&view));
    with(|cards| {
        cards.open.insert(
            account.to_owned(),
            Open {
                pane: pane.id(),
                ssh: pending.ssh.clone(),
                pending,
                drawn: card.clone(),
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
    // Where only how far a download has come changed, the number under the name is written in
    // place: the view is not made again, so the ring on its row does not start over at every step.
    let (room, top) = (pane.bounds().size, pane.corner_card_top());
    let in_place = with(|cards| {
        cards.open.get_mut(account).is_some_and(|open| {
            // The room is the one the card was made for, and only a percent moved.
            let same = view::made_for(mtm, &open.view, &open.drawn, &open.target, room, top)
                && view::refresh_notes(&open.view, &open.drawn, &card);
            if same {
                open.drawn = card.clone();
            }
            same
        })
    });
    if in_place {
        return;
    }
    // A long list stays where it was scrolled to when a box is checked or a line settles.
    let offset = with(|cards| {
        cards
            .open
            .get(account)
            .and_then(|open| view::scrolled(&open.view))
    });
    let view = view::build(mtm, &card, &target, room, top);
    if let Some(offset) = offset {
        view::scroll_to(&view, offset);
    }
    pane.set_corner_card(Some(&view));
    with(|cards| {
        if let Some(open) = cards.open.get_mut(account) {
            open.view = view;
            open.drawn = card;
        }
    });
}

/// A pane's size changed, or the find panel took its corner or left it: its card is as wide as the
/// pane lets it be and its list as high as the room under the top leaves ([`view::made_for`]), so a
/// card that would be another size in the pane now is made again, whether the pane became
/// narrower, wider, lower or higher, and a card the new room does not touch is left as it is. Told
/// by the pane on the next turn of the loop, so that nothing here runs inside its layout.
pub(crate) fn pane_resized(mtm: MainThreadMarker, id: u64) {
    notice::pane_resized(mtm, id);
    let Some(app) = app::delegate(mtm) else {
        return;
    };
    let Some(pane) = app.all_panes().into_iter().find(|pane| pane.id() == id) else {
        return;
    };
    let (now, top) = (pane.bounds().size, pane.corner_card_top());
    // Every card the pane holds, not the first only.
    let stale: Vec<String> = with(|cards| {
        cards
            .open
            .iter()
            .filter(|(_, open)| {
                open.pane == id
                    && !view::made_for(mtm, &open.view, &open.drawn, &open.target, now, top)
            })
            .map(|(account, _)| account.clone())
            .collect()
    });
    for account in stale {
        redraw(mtm, &account);
    }
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
    if notice::is_notice(account) {
        return notice::act(mtm, account, act);
    }
    if local::is_account(account) {
        return act_here(mtm, act);
    }
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
            // The boxes left unchecked are said to the server before the first program comes, so
            // that it does not ask about them again; begin takes those rows off the card.
            let (first, declined) = with(|cards| match cards.open.get_mut(account) {
                Some(open) => {
                    let declined = open.card.declined_words();
                    (open.card.begin(), declined)
                }
                None => (None, None),
            });
            // A card of a click answers that click, not the question the server is asked once.
            let link = with(|cards| {
                cards
                    .open
                    .get(account)
                    .is_some_and(|open| open.card.link().is_some())
            });
            if first.is_some() && !link {
                with(|cards| cards.answered.insert(account.to_owned()));
            }
            redraw(mtm, account);
            if let Some(line) = first {
                bring(mtm, account, line, declined);
            }
        }
        Act::Retry => {
            let first = with(|cards| cards.open.get_mut(account).and_then(|o| o.card.retry()));
            redraw(mtm, account);
            if let Some(line) = first {
                bring(mtm, account, line, None);
            }
        }
        Act::NotHere => {
            // The card of a click: the other answer is the system's browser, and the server is
            // told nothing.
            if let Some(url) = with(|cards| {
                cards.open.get(account).and_then(|open| {
                    (open.card.stage() == Stage::Ask)
                        .then(|| open.card.link().map(str::to_owned))
                        .flatten()
                })
            }) {
                remove(mtm, account);
                crate::hyperlink::open_url(&url);
                return;
            }
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
        Act::Close => {
            // The click that raised a card of a program that did not come is not lost: its
            // address opens in the system's browser. Only a card that failed has this button (one
            // that is ready has opened its address in the program already).
            let url = with(|cards| {
                cards.open.get(account).and_then(|open| {
                    (open.card.stage() == Stage::Failed)
                        .then(|| open.card.link().map(str::to_owned))
                        .flatten()
                })
            });
            remove(mtm, account);
            if let Some(url) = url {
                crate::hyperlink::open_url(&url);
            }
        }
    }
}

/// Has the server bring line `line` of the card of `account`; what came of it is
/// [`finished`]. `before` is a command to run on the server first (the programs that were not
/// wanted): the one that writes `not-here` and the one that installs do not run at once, for both
/// write notes in the same folder.
fn bring(mtm: MainThreadMarker, account: &str, line: usize, before: Option<Vec<String>>) {
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
            if let Some(before) = before
                && let Err(why) = on_server(&ssh, &host, &before)
            {
                eprintln!(
                    "bateri: could not tell {host} which programs are not wanted there: {why}"
                );
            }
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
        (Some(line), _) => bring(mtm, account, line, None),
        (None, Stage::Done) => {
            // The program is there: the address a click asked to open opens in it, once.
            let opening = with(|cards| {
                cards
                    .open
                    .get(account)
                    .and_then(|open| open.card.link().map(|url| (open.pane, url.to_owned())))
            });
            if let Some((pane, url)) = opening
                && let Some(app) = app::delegate(mtm)
            {
                with(|cards| {
                    if let Some(name) = cards.open.get(account).and_then(|open| {
                        open.card.lines().first().map(|line| line.row.name.clone())
                    }) {
                        cards.brought.insert((account.to_owned(), name));
                    }
                });
                app.open_address_in_installed(pane, &url);
            }
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
    // A notice has its one answer, which is what ⌘Return is; it has no second one for ⌘Escape.
    if install && let Some(account) = notice::asking_in(tab) {
        return Some(account);
    }
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

/// The programs a card of a click brought to the server of `account` in this run: what the server
/// has, to the next click, before it has said so itself.
pub(crate) fn brought_to(account: &str) -> Vec<String> {
    with(|cards| {
        cards
            .brought
            .iter()
            .filter(|(brought_to, _)| brought_to == account)
            .map(|(_, name)| name.clone())
            .collect()
    })
}

/// A menu item of the card was chosen for the card of `account`.
pub(crate) fn chosen(mtm: MainThreadMarker, account: &str, act_on: Act) {
    act(mtm, account, act_on);
}

/// A web address ⌘-clicked in `from`, a pane of a server that lacks the program `name` that opens
/// it: the card that offers to bring the program and open the address in it, or to open the
/// address in the system's browser. `false` when the server cannot be offered the program (it is
/// not followed here, has no file for the server's kind of computer, its release has not landed, or
/// the server has a card up already): the caller opens the address in the system's browser.
pub(crate) fn offer_link(
    mtm: MainThreadMarker,
    from: &TerminalPane,
    name: &str,
    url: &str,
) -> bool {
    let crate::pane::PaneMachine::Server(account) = from.machine() else {
        return false;
    };
    // One card to a pane: a pane that holds one (the programs this server lacks, or the first
    // click's question) has none for this click.
    if notice::has(from.id())
        || with(|cards| {
            cards.open.contains_key(&account)
                || cards.open.values().any(|open| open.pane == from.id())
        })
    {
        return false;
    }
    let (Some(root), Some(answerer)) = (crate::apps_root(), crate::apps_answerer()) else {
        return false;
    };
    let Some(row) = answerer
        .machine(&account)
        .and_then(|machine| card::row_for(&root, &machine, name))
    else {
        return false;
    };
    let Some((_, ssh, _)) = from.server_target() else {
        return false;
    };
    let Some((program, args)) = ssh.argv.split_first() else {
        return false;
    };
    if program != "ssh" || args.is_empty() {
        return false;
    }
    let host = from.kept_host().unwrap_or_else(|| ssh.label().into_owned());
    let card = Card::for_link(&host, row, url);
    let pending = Pending {
        host,
        ssh: args.to_vec(),
        rows: Vec::new(),
    };
    raise(mtm, from, &account, pending, card);
    true
}

// ─── the first click's question ───────────────────────────────────────────────

/// Where the card of the first click's question is in its life, when it is up.
pub(crate) fn here_stage() -> Option<Stage> {
    with(|cards| cards.open.get(HERE).map(|open| open.card.stage()))
}

/// Whether the card of the first click's question is up.
pub(crate) fn here_open() -> bool {
    with(|cards| cards.open.contains_key(HERE))
}

/// A web address ⌘-clicked in `from` before anything was decided about links: the card that asks
/// whether to open links beside the terminal with browserpen, over that pane, with a row for each
/// thing that is not on this Mac. `false` when it cannot be put there (the pane holds a card, or
/// the question is up in another): the caller opens the address in the system's browser.
pub(crate) fn offer_here(
    mtm: MainThreadMarker,
    from: &TerminalPane,
    url: &str,
    program_here: bool,
    engine_here: bool,
    engine_bytes: Option<u64>,
) -> bool {
    if notice::has(from.id())
        || with(|cards| {
            cards.open.contains_key(HERE) || cards.open.values().any(|open| open.pane == from.id())
        })
    {
        return false;
    }
    let card = Card::for_here(url, program_here, engine_here, engine_bytes);
    let pending = Pending {
        host: String::new(),
        ssh: Vec::new(),
        rows: Vec::new(),
    };
    raise(mtm, from, HERE, pending, card);
    true
}

/// What the person pressed on the card of the first click's question.
fn act_here(mtm: MainThreadMarker, act: Act) {
    let Some(app) = app::delegate(mtm) else {
        return;
    };
    match act {
        Act::Install => {
            let Some(yes) = with(|cards| {
                cards
                    .open
                    .get_mut(HERE)
                    .and_then(|open| open.card.say_yes())
            }) else {
                return;
            };
            // Where everything is here the yes has nothing to bring: the card is not drawn as
            // installing for the moment the choice takes to be written, it goes with the address
            // opening ([`here_sync`]).
            if yes == Yes::Download {
                redraw(mtm, HERE);
            }
            app.said_yes_to_links();
        }
        Act::Retry => {
            if !with(|cards| {
                cards
                    .open
                    .get_mut(HERE)
                    .is_some_and(|open| open.card.try_again())
            }) {
                return;
            }
            redraw(mtm, HERE);
            app.said_yes_to_links();
        }
        Act::NotHere => {
            let Some(no) = with(|cards| cards.open.get(HERE).and_then(|open| open.card.say_no()))
            else {
                return;
            };
            remove(mtm, HERE);
            // The question answered no is not asked again; a way out of a download that goes on, or
            // of one that failed, changes nothing.
            if no.writes {
                app.save_edit(&web_links::no_edit());
            }
            crate::hyperlink::open_url(&no.url);
        }
        Act::Toggle(_) | Act::Close => {}
    }
}

/// Where what the card's yes brought stands, as the shell found it ([`Coming`]): the rows follow it,
/// and the card is ready when each thing is here, which opens the address.
pub(crate) fn here_sync(mtm: MainThreadMarker, program: &Coming, engine: &Coming) {
    let Some((changed, arrived, stays, pane, url)) = with(|cards| {
        let open = cards.open.get_mut(HERE)?;
        let before = open.card.clone();
        let stage = open.card.sync_here(program, engine);
        Some((
            before != open.card,
            before.stage() != Stage::Done && stage == Stage::Done,
            open.card.stays_when_ready(),
            open.pane,
            open.card.link().map(str::to_owned),
        ))
    }) else {
        return;
    };
    // Where nothing had to come the card was never drawn as ready: it goes and the address opens.
    if arrived && !stays {
        remove(mtm, HERE);
        if let (Some(app), Some(url)) = (app::delegate(mtm), url) {
            app.open_address_here(pane, &url);
        }
        return;
    }
    if changed {
        redraw(mtm, HERE);
    }
    // A card that failed can be on its way again without a *Try Again* (another click started the
    // download again): nothing tells when the link is in place, so it is looked at again.
    if here_stage() == Some(Stage::Installing) {
        here_watch();
    }
    if !arrived {
        return;
    }
    // Everything is here: the address opens as a click on it would now, and the card says so for a
    // moment, as a server's does, and goes.
    if let (Some(app), Some(url)) = (app::delegate(mtm), url) {
        app.open_address_here(pane, &url);
    }
    if let Ok(when) = DispatchTime::try_from(Duration::from_secs(READY_SECONDS)) {
        let _ = DispatchQueue::main().after(when, move || {
            // audit: a block running on the main queue is on the main thread by definition.
            let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
            if here_stage() == Some(Stage::Done) {
                remove(mtm, HERE);
            }
        });
    }
}

/// The card's yes could not be carried out at all (the choice could not be written): it says why.
pub(crate) fn here_failed(mtm: MainThreadMarker, why: &str) {
    let failed = with(|cards| {
        cards.open.get_mut(HERE).map(|open| {
            open.card.fail(why);
            open.card.stage() == Stage::Failed
        })
    });
    if failed == Some(true) {
        redraw(mtm, HERE);
    }
}

/// The first click's card goes at once: the corner is wanted for another card (a server's, which
/// offers browserpen where it has none).
pub(crate) fn remove_here(mtm: MainThreadMarker) {
    remove(mtm, HERE);
}

thread_local! {
    /// Whether the card's once-a-second look at what its yes is bringing is running.
    static WATCHING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Looks, once a second while the card waits for its yes to come, at whether it has: a program
/// that landed is put in place by kablo behind the check of the programs, which tells nobody when
/// it is done, and a link on the disk is read in a moment.
pub(crate) fn here_watch() {
    if WATCHING.replace(true) {
        return;
    }
    here_tick();
}

fn here_tick() {
    let Ok(when) = DispatchTime::try_from(Duration::from_secs(1)) else {
        WATCHING.set(false);
        return;
    };
    let _ = DispatchQueue::main().after(when, move || {
        // audit: a block running on the main queue is on the main thread by definition.
        let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
        if here_stage() != Some(Stage::Installing) {
            WATCHING.set(false);
            return;
        }
        if let Some(app) = app::delegate(mtm) {
            app.links_progress();
        }
        here_tick();
    });
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
        let off = settings("[remote]\nintegration = \"off\"\n");
        assert!(!asks(&off, "u@dev-1:22", "dev-1"));
        let host = settings("[remote]\nhosts = [{ host = \"dev-1\", integration = false }]\n");
        assert!(!asks(&host, "u@dev-1:22", "dev-1"));
        assert!(asks(&host, "u@dev-2:22", "dev-2"));
    }

    #[test]
    fn a_server_marked_production_is_asked_like_any_other() {
        // The mark colors a server and decides nothing else.
        let production =
            settings("[remote]\nhosts = [{ host = \"prod-*\", mark = \"production\" }]\n");
        assert!(asks(&production, "u@prod-1:22", "prod-1"));
        let off = settings(
            "[remote]\nintegration = \"off\"\nhosts = [{ host = \"prod-*\", mark = \"production\" }]\n",
        );
        assert!(!asks(&off, "u@prod-1:22", "prod-1"));
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
