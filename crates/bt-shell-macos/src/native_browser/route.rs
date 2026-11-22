//! The way from this Mac to the program's machine, found before the first browser starts: the program is asked
//! for a listener on its machine's loopback that gives a token to one connection ([`bt_core::Session::probe`], §4.10 of the protocol), and the way that would be
//! used ([`TerminalPane::browser_way`]) connects to it and must read the token back
//! ([`route::prove`]). What is shown is what the pane's browsers go by; what is not leaves them
//! with a line that says there is no route, and no engine is started for them.

use std::sync::Arc;
use std::time::Duration;

use bt_shell_common::browser::depot::Depot;
use bt_shell_common::browser::route::{self, Dialer, PROOF_WAIT, no_way};

use super::{PageBrowsers, on_main};
use crate::pane::TerminalPane;

/// How long the listener the program gives stays up (seconds): long enough for the proof, short
/// enough that it is not left lying about.
const LISTENER_TTL: u8 = 20;

/// How long the program may take to answer the probe before the way is called not shown.
const ANSWER_WAIT: Duration = Duration::from_secs(25);

/// The probe that is out.
pub(super) struct Probing {
    id: u32,
    machine: String,
    way: Arc<dyn Dialer>,
    /// Where the machine keeps the profiles of the pane's browsers, if it is to.
    depot: Option<Arc<dyn Depot>>,
}

impl PageBrowsers {
    /// The rooms want the way to the program's machine: the program is asked for its listener.
    pub(super) fn find_route(&self, pane: &TerminalPane) {
        let (machine, way) = match pane.browser_way() {
            Ok(found) => found,
            Err(why) => return self.routed(Err(why)),
        };
        let Some(id) = pane
            .session()
            .and_then(|session| session.probe(LISTENER_TTL))
        else {
            return self.routed(Err(no_way(
                "the program does not offer a listener to show it where the way ends",
            )));
        };
        let depot = pane.browser_depot();
        *self.probing.borrow_mut() = Some(Probing {
            id,
            machine,
            way,
            depot,
        });
        // A program that says nothing leaves the rooms waiting for the way: it is not shown.
        let (pane_id, serial) = (pane.id(), self.serial);
        std::thread::spawn(move || {
            std::thread::sleep(ANSWER_WAIT);
            on_main(pane_id, move |pane| {
                if let Some(browsers) = pane.page_browsers().filter(|b| b.serial == serial) {
                    let waiting = browsers
                        .probing
                        .borrow()
                        .as_ref()
                        .is_some_and(|probing| probing.id == id);
                    if waiting {
                        browsers.probing.replace(None);
                        browsers.routed(Err(no_way("the program did not answer")));
                    }
                }
            });
        });
    }

    /// The program answered the probe: `Ok` is the port of its listener and the token to read
    /// from it, `Err` the word it gave for having none.
    pub(crate) fn probed(
        &self,
        pane: &TerminalPane,
        id: u32,
        answer: Result<(u16, String), String>,
    ) {
        let Some(probing) = self.probing.borrow_mut().take_if(|p| p.id == id) else {
            return;
        };
        let (port, token) = match answer {
            Ok(found) => found,
            Err(code) => {
                return self.routed(Err(no_way(&format!(
                    "the program has no listener to show it ({code})"
                ))));
            }
        };
        // The connection waits for the far end and so is not made on the main queue.
        let (pane_id, serial) = (pane.id(), self.serial);
        let Probing {
            machine,
            way,
            depot,
            ..
        } = probing;
        std::thread::spawn(move || {
            let proof = route::prove(&*way, port, &token, PROOF_WAIT).map(|()| (machine, way));
            on_main(pane_id, move |pane| {
                if let Some(browsers) = pane.page_browsers().filter(|b| b.serial == serial) {
                    browsers.rooms.borrow_mut().set_depot(depot);
                    browsers.routed(proof);
                }
            });
        });
    }

    /// The way is shown, or is not: the rooms are told.
    fn routed(&self, proof: Result<(String, Arc<dyn Dialer>), String>) {
        let mut told = Vec::new();
        self.rooms.borrow_mut().route(proof, &mut told);
        self.effects.borrow_mut().extend(told);
        // Not inline: this may be called while the page drains its replies.
        (self.wake)();
    }
}
