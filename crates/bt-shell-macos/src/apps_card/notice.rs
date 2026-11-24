//! The notices of the card's family: a card that says one thing and has one answer, over the
//! corner of a pane, in the same words, size and layout rules as the cards of programs
//! ([`super::view`]).
//!
//! - **A program's view that waits** ([`Kind::View`]): bateri quit unexpectedly with it open (or a
//!   page of it failed), so it is not opened by itself; the card says so and **Reopen** opens it.
//!   It stays as long as the view waits, takes no keyboard, and goes when the view opens again or
//!   the program is gone ([`crate::pane::TerminalPane::reopen_view`]).
//! - **The windows a launch gave up** ([`Kind::Windows`]): the launch after two that never
//!   settled opens no saved window; the card says they are kept and **Open Them** opens them
//!   ([`crate::app::AppDelegate::open_given_up_windows`]).
//!
//! The pane says what it wants ([`want`]); the card is made on the next turn of the loop, once
//! the pane has a size, and again whenever the room it is made for changes ([`pane_resized`]) — a
//! pane that is being born has no place to put one yet. One card to a pane: the corner is
//! the notice's while it wants it, and the cards of programs and links are not raised over it
//! ([`has`]; a link's address goes to the system's browser then).
//!
//! Its key is the menu's: Shell ▸ Install Programs Here (⌘Return) is the card's answer here as it
//! is there ([`asking_in`]), titled for what it answers.

use std::cell::RefCell;
use std::collections::HashMap;

use bt_shell_common::apps::card::Card;
use dispatch2::DispatchQueue;
use objc2::MainThreadMarker;
use objc2::rc::Retained;

use super::Act;
use super::view::{self, CardView, Target};
use crate::app;
use crate::tab::TerminalTab;

/// What the notice is about, and so what its answer does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    /// A program's view that waits: the answer opens it again.
    View,
    /// The saved windows a launch did not open: the answer opens them.
    Windows,
}

/// What the key of a notice's card is called in [`Target`]: its account is this and the pane's
/// number, which is how [`act`] finds the pane again.
const KEY: &str = "notice:";

/// The account the buttons of the card over pane `id` say they are for.
fn account_of(id: u64) -> String {
    format!("{KEY}{id}")
}

/// The pane a notice account names.
fn pane_of(account: &str) -> Option<u64> {
    account.strip_prefix(KEY)?.parse().ok()
}

/// Whether `account` is a notice's.
pub(super) fn is_notice(account: &str) -> bool {
    account.starts_with(KEY)
}

/// A card on screen.
struct Made {
    view: Retained<CardView>,
    /// The buttons' target: the view holds it, and so does this, for the next view.
    target: Retained<Target>,
}

/// What a pane wants, and the card that shows it once there is one.
struct Entry {
    kind: Kind,
    card: Card,
    made: Option<Made>,
}

thread_local! {
    /// The notices by their pane's number: the main thread's alone, like the views in them.
    static NOTICES: RefCell<HashMap<u64, Entry>> = RefCell::new(HashMap::new());
}

fn with<T>(act: impl FnOnce(&mut HashMap<u64, Entry>) -> T) -> T {
    NOTICES.with(|notices| act(&mut notices.borrow_mut()))
}

/// Whether pane `id` wants a notice, shown or not yet.
pub(crate) fn has(id: u64) -> bool {
    with(|notices| notices.contains_key(&id))
}

/// Whether pane `id` wants a notice that is not made yet: its card is made when the pane has a
/// size.
pub(crate) fn waits_for_its_card(id: u64) -> bool {
    with(|notices| notices.get(&id).is_some_and(|entry| entry.made.is_none()))
}

/// Pane `id` wants this notice: it is made on the next turn of the loop (and again where the
/// room asks), over the corner. What the pane wanted before is replaced.
pub(crate) fn want(id: u64, kind: Kind, card: Card) {
    with(|notices| {
        notices.insert(
            id,
            Entry {
                kind,
                card,
                made: None,
            },
        );
    });
    DispatchQueue::main().exec_async(move || {
        // audit: a block running on the main queue is on the main thread by definition.
        let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
        pane_resized(mtm, id);
    });
}

/// Pane `id` wants no notice: the card goes, if it is up.
pub(crate) fn drop_notice(mtm: MainThreadMarker, id: u64) {
    let Some(entry) = with(|notices| notices.remove(&id)) else {
        return;
    };
    let Some(made) = entry.made else {
        return;
    };
    if let Some(app) = app::delegate(mtm)
        && let Some(pane) = app.all_panes().into_iter().find(|pane| pane.id() == id)
        && pane.corner_card_view().is_some_and(|card| {
            Retained::as_ptr(&card).cast::<()>() == Retained::as_ptr(&made.view).cast::<()>()
        })
    {
        pane.set_corner_card(None);
    }
}

/// The pane is closed: what it wanted goes with it (its card is its view).
pub(crate) fn forget(id: u64) {
    with(|notices| notices.remove(&id));
}

/// The pane's room changed (or its card was asked for): the card is made if it is not, and made
/// again if the room is another size ([`view::made_for`]); a card the room does not touch is left
/// as it is. Told by the pane on the next turn of the loop, so that nothing here runs inside its
/// layout.
pub(crate) fn pane_resized(mtm: MainThreadMarker, id: u64) {
    let Some((card, made)) = with(|notices| {
        notices.get(&id).map(|entry| {
            (
                entry.card.clone(),
                entry
                    .made
                    .as_ref()
                    .map(|made| (Retained::clone(&made.view), Retained::clone(&made.target))),
            )
        })
    }) else {
        return;
    };
    let Some(app) = app::delegate(mtm) else {
        return;
    };
    let Some(pane) = app.all_panes().into_iter().find(|pane| pane.id() == id) else {
        return;
    };
    let (room, top) = (pane.bounds().size, pane.corner_card_top());
    // A pane that has no room yet is told of it again when it has some.
    if room.width < 1.0 || room.height < 1.0 {
        return;
    }
    if let Some((view, target)) = &made
        && view::made_for(mtm, view, &card, target, room, top)
    {
        return;
    }
    let target = made
        .map(|(_, target)| target)
        .unwrap_or_else(|| Target::new(mtm, &account_of(id)));
    let view = view::build(mtm, &card, &target, room, top);
    pane.set_corner_card(Some(&view));
    with(|notices| {
        if let Some(entry) = notices.get_mut(&id) {
            entry.made = Some(Made { view, target });
        }
    });
}

/// The account of the notice over a pane of `tab`, the pane the person is in first: Shell ▸
/// Install Programs Here (⌘Return) answers it.
pub(crate) fn asking_in(tab: &TerminalTab) -> Option<String> {
    let focused = tab.focused_pane().id();
    let mut order = vec![focused];
    order.extend(
        tab.panes()
            .iter()
            .map(|pane| pane.id())
            .filter(|id| *id != focused),
    );
    with(|notices| {
        order
            .iter()
            .find(|id| notices.get(id).is_some_and(|entry| entry.made.is_some()))
            .map(|id| account_of(*id))
    })
}

/// What the answer of the notice of `account` is called, for the menu's item: `None` when it is
/// not a notice's.
pub(crate) fn answer_of(account: &str) -> Option<&'static str> {
    let id = pane_of(account)?;
    with(|notices| {
        notices
            .get(&id)
            .and_then(|entry| entry.card.notice_answer())
    })
}

/// The person pressed the answer of the notice of `account` (the button, or ⌘Return).
pub(super) fn act(mtm: MainThreadMarker, account: &str, act: Act) {
    // The notice has one button, and Install is how the card's yes is called everywhere.
    if act != Act::Install {
        return;
    }
    let Some(id) = pane_of(account) else {
        return;
    };
    let Some(kind) = with(|notices| notices.get(&id).map(|entry| entry.kind)) else {
        return;
    };
    let Some(app) = app::delegate(mtm) else {
        return;
    };
    match kind {
        Kind::View => {
            if let Some(pane) = app.all_panes().into_iter().find(|pane| pane.id() == id) {
                pane.reopen_view();
            }
            drop_notice(mtm, id);
        }
        Kind::Windows => {
            drop_notice(mtm, id);
            app.open_given_up_windows();
        }
    }
}
