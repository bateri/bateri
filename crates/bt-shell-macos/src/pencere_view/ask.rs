//! What a program's view asks of the person: the access it declared, and the links it asks
//! to open. The words are `bt_shell_common::view_ask`'s; this is the sheet they are put in,
//! and what the answers do.
//!
//! **Both go through the sheet gate** ([`crate::sheets`]): a pane's question sits on its
//! tab, is parked while the tab is behind another and opens when it comes forward, so a
//! program in a tab that is not shown does not put a sheet over the one that is. Nothing here
//! calls AppKit's `beginSheet…` itself (`make audit`).
//!
//! **The question about access** ([`consent`]) is the protocol's `Consent`, asked while the
//! conversation is `Opening`; the view does not start before the answer. Three things follow
//! from that:
//!
//! * the deadline of an opening is not running while the person is asked (the pane leaves
//!   the state out of what it sets deadlines for while [`Asking::is_up`]), because a person
//!   who has gone for coffee has not made the program's open late;
//! * an answer applies only to **the open it was asked for** ([`Asking::finish`]): a program
//!   that gave up and opened again while the first sheet was up has a new question, and the
//!   first is taken away ([`withdraw`]) — its answer must not allow what the second
//!   declared;
//! * an answer the person did not give — Escape, a pane that closed under the sheet, a sheet
//!   lowered for good — is a refusal and is not remembered: Escape is "not now", and a stray
//!   key must not leave a view without its access for as long as the application runs, with
//!   no question to give it back.
//!
//! **What is remembered** is [`Memory`]'s rule, kept here for the whole process on the main
//! thread and written nowhere: an answer is for a program in a place with a tool and a set.
//! A question whose tool could not be read is asked every time.
//!
//! **Questions that ask the same are one question** ([`Waiting`]): panes restored together
//! that open the same tool each put the question up before anyone has answered the first, so
//! the memory has nothing yet when they ask. The person's answer to one is then the answer to
//! the others that are up, and their sheets go with it (`answer_same`): asking twice for one
//! answer is what the person would call a bug. Only an answer the person gave does this —
//! Escape is "not now" for the pane it was said in, and the others stay asked.
//!
//! **A link** ([`link`]) is opened in the person's browser on this Mac, after the person
//! agrees, and nothing else is: the question names where the program runs, so a link a
//! program on a server asked for is not taken to open there. One question at a time per pane;
//! another request while it is up is refused, so a view cannot stack sheets.

use std::cell::{Cell, RefCell};
use std::sync::Arc;

use block2::RcBlock;
use bt_core::Session;
use bt_core::pencere::{Consent, Grant, State};
use bt_shell_common::view_ask::{self, Memory, Origin, Question, Waiting};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSAlert, NSAlertFirstButtonReturn, NSAlertSecondButtonReturn, NSModalResponse,
    NSModalResponseCancel, NSWindow,
};
use objc2_foundation::{NSString, NSURL};

use crate::hyperlink::open_url;
use crate::pane::{PaneLookup, TerminalPane};
use crate::sheets::{self, Asker};
use crate::uploader::{ESCAPE, add_key_monitor, remove_monitor};

thread_local! {
    /// The answers the person has given since the application started: the main
    /// thread's, which is where every sheet's answer arrives.
    static MEMORY: RefCell<Memory> = const { RefCell::new(Memory::new()) };
    /// The questions about access that are up in any pane, by what they ask.
    static WAITING: RefCell<Waiting<Up>> = const { RefCell::new(Waiting::new()) };
}

/// A question about access that is up: whose it is (the pane, and the number it gave the
/// question), and what the pane's view declared, so that another's answer can be applied to it.
struct Up {
    pane: u64,
    number: u64,
    consent: Consent,
}

impl Up {
    /// Whether this is question `number` of `pane`.
    fn is(&self, pane: u64, number: u64) -> bool {
        self.pane == pane && self.number == number
    }
}

/// The question `number` of `pane` is not up any more, however it ended.
fn gone(pane: u64, number: u64) {
    WAITING.with(|waiting| waiting.borrow_mut().gone(|up| up.is(pane, number)));
}

/// The questions a pane has up for its program's view.
#[derive(Default)]
pub(crate) struct Asking {
    /// The number of the question about access that is up; `0` for none.
    consent: Cell<u64>,
    /// The number of the last question about access.
    issued: Cell<u64>,
    /// The window of the sheet of that question, to take it away.
    sheet: RefCell<Option<Retained<NSWindow>>>,
    /// A question about a link is up.
    link: Cell<bool>,
}

impl Asking {
    /// Whether the person is being asked about access, in which case the conversation
    /// waits for the person and no deadline of the pane's runs.
    pub(crate) fn is_up(&self) -> bool {
        self.consent.get() != 0
    }

    /// The state the conversation's deadlines are set for ([`super::Waits::observe`]):
    /// `state`, except while the person is asked about access, when the open waits for
    /// them however long and the conversation waits for nothing the pane ends.
    pub(crate) fn deadline_state(&self, state: Option<State>) -> Option<State> {
        state.filter(|_| !self.is_up())
    }

    /// The question numbered `number` was answered or gone: whether it is still the one
    /// that is up, and so whether its answer is for the open that is. Either way nothing
    /// is up after.
    fn finish(&self, number: u64) -> bool {
        let current = self.consent.get() == number;
        if current {
            self.consent.set(0);
            self.sheet.borrow_mut().take();
        }
        current
    }
}

/// The sheet of `question` on `pane`'s seat; `answered` gets the response. The refusing
/// button is the first, which AppKit makes the one Return presses. Escape is "not now": it
/// ends the sheet with `Cancel`, which is a refusal the person did not choose a button for, so
/// nothing is remembered and the next open asks again (AppKit makes a button of its own title
/// the Escape one only when it is "Cancel", so the key is read here).
/// `None` when the pane is in no window and nothing was asked.
fn put(
    pane: &TerminalPane,
    question: &Question,
    answered: impl Fn(NSModalResponse) + 'static,
) -> Option<Retained<NSWindow>> {
    let seat = sheets::seat(Asker::Pane(pane))?;
    let alert = NSAlert::new(pane.mtm());
    alert.setMessageText(&NSString::from_str(&question.title));
    alert.setInformativeText(&NSString::from_str(&question.text));
    alert.addButtonWithTitle(&NSString::from_str(question.no));
    alert.addButtonWithTitle(&NSString::from_str(question.yes));
    let sheet_window = alert.window();
    let escape_window = sheet_window.clone();
    let monitor: std::rc::Rc<Cell<Option<Retained<AnyObject>>>> = std::rc::Rc::default();
    // The pane by id, not the window: a monitor that outlived its sheet must not pin
    // the window (`hyperlink`'s question is asked the same way).
    let (id, lookup) = (pane.id(), pane.lookup());
    monitor.set(add_key_monitor(move |event| {
        if event.keyCode() != ESCAPE {
            return false;
        }
        // audit: the local event monitor runs on the main thread.
        let mtm = MainThreadMarker::new().expect("the event monitor is on the main thread");
        let on_sheet = event
            .window(mtm)
            .is_some_and(|window| Retained::as_ptr(&window) == Retained::as_ptr(&escape_window));
        if on_sheet
            && let Some(seat) = lookup(mtm, id).and_then(|pane| sheets::seat(Asker::Pane(&pane)))
        {
            seat.end(&escape_window, NSModalResponseCancel);
        }
        on_sheet
    }));
    let slot = std::rc::Rc::clone(&monitor);
    let block = RcBlock::new(move |response: NSModalResponse| {
        remove_monitor(slot.take());
        answered(response);
    });
    seat.begin(&alert, &block);
    Some(sheet_window)
}

/// The question about the access a view declared. `origin` is who asks and `app` the
/// application's name. Answered at once when the person has answered the same before.
pub(crate) fn consent(
    pane: &TerminalPane,
    session: &Arc<Session>,
    consent: Consent,
    origin: &Origin,
    app: &str,
) {
    // A question still up is about an open that has been replaced.
    withdraw(pane);
    if session.view_state() != Some(State::Opening) {
        // The program has gone on without waiting: nobody is waiting for the answer.
        return;
    }
    let key = view_ask::key(origin, &consent);
    if let Some(allowed) = key
        .as_ref()
        .and_then(|key| MEMORY.with(|memory| memory.borrow().recall(key)))
    {
        session.view_consent(if allowed {
            consent.grant_all()
        } else {
            Grant::none()
        });
        return;
    }
    let question = view_ask::consent(origin, &consent, app);
    let asking = pane.view_asking();
    let number = asking.issued.get() + 1;
    asking.issued.set(number);
    asking.consent.set(number);
    let (id, lookup) = (pane.id(), pane.lookup());
    let answering = Arc::clone(session);
    let waits = consent.clone();
    let asked = key.clone();
    let sheet = put(pane, &question, move |response| {
        // audit: the sheet's completion block runs on the main thread.
        let mtm = MainThreadMarker::new().expect("a sheet's block is on the main thread");
        // Whatever happened to the sheet, this question is not up any more.
        gone(id, number);
        let Some(pane) = lookup(mtm, id) else {
            return;
        };
        if !pane.view_asking().finish(number) {
            return;
        }
        let allowed = response == NSAlertSecondButtonReturn;
        // The person's own answer, as against a sheet that was taken away.
        let given = allowed || response == NSAlertFirstButtonReturn;
        let mut same = Vec::new();
        if given
            && answering.view_state() == Some(State::Opening)
            && let Some(key) = key.clone()
        {
            MEMORY.with(|memory| memory.borrow_mut().remember(key.clone(), allowed));
            // The others that ask the same are answered by this: taken out before any sheet
            // is ended, which runs their blocks.
            same = WAITING.with(|waiting| waiting.borrow_mut().answered(&key));
        }
        answering.view_consent(if allowed {
            consent.grant_all()
        } else {
            Grant::none()
        });
        for other in same {
            answer_same(mtm, lookup, &other, allowed);
        }
    });
    match sheet {
        Some(window) => {
            asking.sheet.replace(Some(window));
            if let Some(key) = asked {
                WAITING.with(|waiting| {
                    waiting.borrow_mut().up(
                        key,
                        Up {
                            pane: id,
                            number,
                            consent: waits,
                        },
                    );
                });
            }
        }
        None => {
            // The pane is in no window: nobody can be asked, and the view starts with
            // nothing allowed.
            asking.consent.set(0);
            session.view_consent(Grant::none());
        }
    }
}

/// The person answered a question that `other` asks the same as: it is answered with the same,
/// and its sheet goes. `other` is up in another pane, which may be in a tab behind (its sheet is
/// parked) or have closed (nothing to do).
fn answer_same(mtm: MainThreadMarker, lookup: PaneLookup, other: &Up, allowed: bool) {
    let Some(pane) = lookup(mtm, other.pane) else {
        return;
    };
    let asking = pane.view_asking();
    let sheet = asking.sheet.borrow().clone();
    // Finished first, so that the sheet's own block, which ending it runs, finds nothing to do.
    if !asking.finish(other.number) {
        return;
    }
    if let Some(sheet) = sheet
        && let Some(seat) = sheets::seat(Asker::Pane(&pane))
    {
        seat.end(&sheet, NSModalResponseCancel);
    }
    let Some(session) = pane.session().cloned() else {
        return;
    };
    if session.view_state() == Some(State::Opening) {
        session.view_consent(if allowed {
            other.consent.grant_all()
        } else {
            Grant::none()
        });
    }
}

/// Takes away the question about access that is up, without an answer: the open it was
/// for is over, or another has come.
pub(crate) fn withdraw(pane: &TerminalPane) {
    let asking = pane.view_asking();
    let number = asking.consent.replace(0);
    if number == 0 {
        return;
    }
    gone(pane.id(), number);
    let sheet = asking.sheet.borrow_mut().take();
    if let Some(sheet) = sheet
        && let Some(seat) = sheets::seat(Asker::Pane(pane))
    {
        seat.end(&sheet, NSModalResponseCancel);
    }
}

/// The conversation is in `state`: a question about access that is up is for an open
/// that is `Opening`, and goes when it is not.
pub(crate) fn settle(pane: &TerminalPane, state: Option<State>) {
    if pane.view_asking().is_up() && state != Some(State::Opening) {
        withdraw(pane);
    }
}

/// A link the view asked to open, numbered `request` in the conversation. It is asked about
/// when it is a plain web link ([`view_ask::web_target`]) and nothing else is being asked;
/// the answer goes to the conversation, and a link the person agrees to opens in their
/// browser.
pub(crate) fn link(
    pane: &TerminalPane,
    session: &Arc<Session>,
    request: u32,
    url: &str,
    origin: &Origin,
) {
    let asking = pane.view_asking();
    let refuse = || session.view_link_decision(request, false);
    let Some(link) = view_ask::link(origin, url) else {
        refuse();
        return;
    };
    if NSURL::URLWithString(&NSString::from_str(&link.target)).is_none() || asking.link.get() {
        refuse();
        return;
    }
    asking.link.set(true);
    let (id, lookup) = (pane.id(), pane.lookup());
    let answering = Arc::clone(session);
    let target = link.target;
    let sheet = put(pane, &link.question, move |response| {
        // audit: the sheet's completion block runs on the main thread.
        let mtm = MainThreadMarker::new().expect("a sheet's block is on the main thread");
        if let Some(pane) = lookup(mtm, id) {
            pane.view_asking().link.set(false);
        }
        let allowed = response == NSAlertSecondButtonReturn;
        answering.view_link_decision(request, allowed);
        if allowed {
            open_url(&target);
        }
    });
    if sheet.is_none() {
        asking.link.set(false);
        refuse();
    }
}

#[cfg(test)]
mod tests {
    use super::super::wait::{OPENING, Waits};
    use super::*;

    #[test]
    fn a_question_answers_only_the_open_it_was_asked_for() {
        let asking = Asking::default();
        assert!(!asking.is_up());
        asking.consent.set(1);
        assert!(asking.is_up());
        // A newer question took its place: the first one's answer is for an open that is
        // gone, and the newer one is still up.
        asking.consent.set(2);
        assert!(!asking.finish(1));
        assert!(asking.is_up());
        assert!(asking.finish(2));
        assert!(!asking.is_up());
        // Nothing is up, so nothing is anyone's answer.
        assert!(!asking.finish(2));
    }

    #[test]
    fn the_opening_has_no_deadline_while_the_person_is_asked() {
        let asking = Asking::default();
        let mut waits = Waits::default();
        // The open started: a deadline is set.
        let armed = waits.observe(asking.deadline_state(Some(State::Opening)));
        let (number, delay) = armed.expect("an opening waits");
        assert_eq!(delay, OPENING);
        // The question comes up: what was set is cancelled, and none is set while it is up.
        asking.consent.set(1);
        assert_eq!(
            waits.observe(asking.deadline_state(Some(State::Opening))),
            None
        );
        assert!(
            !waits.expired(number),
            "the deadline that was running does not end the open under the sheet"
        );
        // The sheet went and the open is still going: it waits again, with a new deadline.
        asking.finish(1);
        let again = waits.observe(asking.deadline_state(Some(State::Opening)));
        assert!(again.is_some_and(|(next, _)| next != number));
        // A state that waits for nothing never had one.
        assert_eq!(
            waits.observe(asking.deadline_state(Some(State::Open))),
            None
        );
    }
}
