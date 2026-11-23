//! The first browser a person opens: the engine is not on this Mac, so it is asked for ("Download
//! the browser engine?", with the size the release notice gives), downloaded with the progress told
//! to the page as the browser's own words, and the browsers that waited are started. Nothing is
//! downloaded before the person says yes; once an engine is installed that yes covers its updates
//! ([`update_in_background`]).

use std::cell::RefCell;
use std::sync::Once;
use std::sync::atomic::AtomicBool;

use block2::RcBlock;
use bt_shell_common::browser::notice::Offer;
use bt_shell_common::native_view::{BrowserState, Id};
use objc2::MainThreadOnly;
use objc2_app_kit::{NSAlert, NSAlertFirstButtonReturn, NSModalResponse};
use objc2_foundation::{NSString, ns_string};

use super::{PageBrowsers, on_main, status};
use crate::browser_install::{Check, Installer};
use crate::pane::TerminalPane;
use crate::sheets::{self, Asker};

/// Words the page shows in a browser's room while its engine is got ready.
const LOOKING: &str = "Looking for the browser engine…";

impl PageBrowsers {
    /// A room's engine is not installed: look for what is on offer and ask. One question at a
    /// time in a pane; a second room waits for the first one's answer and starts with it.
    pub(super) fn not_installed(&self, pane: &TerminalPane, id: Id) {
        if self.installing.replace(true) {
            self.tell(
                pane,
                id,
                status(BrowserState::STARTING, "Waiting for the browser engine…"),
            );
            return;
        }
        let Some(installer) = crate::browser_install::installer() else {
            self.installing.set(false);
            self.tell(
                pane,
                id,
                status(
                    BrowserState::CRASHED,
                    "This copy of bateri has no browser engine to download.",
                ),
            );
            return;
        };
        self.tell(pane, id, status(BrowserState::STARTING, LOOKING));
        let pane_id = pane.id();
        let _ = std::thread::Builder::new()
            .name("browser-notice".into())
            .spawn(move || {
                let check = installer.check();
                on_main(pane_id, move |pane| {
                    if let Some(browsers) = pane.page_browsers() {
                        browsers.checked(pane, id, installer, check);
                    }
                });
            });
    }

    /// The release notice was read.
    fn checked(&self, pane: &TerminalPane, id: Id, installer: Installer, check: Check) {
        match check {
            Check::Offer(offer) => self.offer(pane, id, installer, offer),
            Check::Current => {
                // It is there after all (another pane installed it meanwhile): start again.
                self.installed(pane, id);
            }
            Check::Refused(why) => {
                self.installing.set(false);
                self.tell(
                    pane,
                    id,
                    status(
                        BrowserState::CRASHED,
                        &format!("The browser engine on offer is not one bateri uses: {why:?}."),
                    ),
                );
            }
            Check::Unavailable(why) | Check::Absent(why) => {
                self.installing.set(false);
                self.tell(
                    pane,
                    id,
                    status(
                        BrowserState::CRASHED,
                        &format!("The browser engine cannot be found now: {why}."),
                    ),
                );
            }
        }
    }

    /// "Download the browser engine?" as the pane's question: a sheet, as the other questions of
    /// a pane are, with the size from the notice.
    fn offer(&self, pane: &TerminalPane, id: Id, installer: Installer, offer: Offer) {
        let Some(seat) = sheets::seat(Asker::Pane(pane)).filter(|seat| !seat.is_taken()) else {
            self.installing.set(false);
            self.tell(
                pane,
                id,
                status(
                    BrowserState::CRASHED,
                    "A question is open in this tab. Reload to be asked about the browser engine.",
                ),
            );
            return;
        };
        let mtm = pane.mtm();
        let alert = NSAlert::new(mtm);
        alert.setMessageText(ns_string!("Download the browser engine?"));
        let megabytes = offer.asset.size.div_ceil(1_000_000);
        alert.setInformativeText(&NSString::from_str(&format!(
            "A browser in a bateri pane runs on Chromium {}, which is downloaded once (about {megabytes} MB) \
             and kept up to date. It is signed by bateri's developer and checked before it is used.",
            offer.chromium
        )));
        alert.addButtonWithTitle(ns_string!("Download"));
        let cancel = alert.addButtonWithTitle(ns_string!("Not Now"));
        cancel.setKeyEquivalent(ns_string!("\u{1b}"));
        let (pane_id, lookup) = (pane.id(), pane.lookup());
        let held = RefCell::new(Some((installer, offer)));
        let answered = RcBlock::new(move |response: NSModalResponse| {
            // audit: the sheet's completion block runs on AppKit's main thread.
            let mtm =
                objc2::MainThreadMarker::new().expect("the sheet block is on the main thread");
            let Some((installer, offer)) = held.borrow_mut().take() else {
                return;
            };
            let Some(pane) = lookup(mtm, pane_id) else {
                return;
            };
            let Some(browsers) = pane.page_browsers() else {
                return;
            };
            if response == NSAlertFirstButtonReturn {
                browsers.download(&pane, id, installer, offer);
            } else {
                browsers.installing.set(false);
                browsers.tell(
                    &pane,
                    id,
                    status(
                        BrowserState::CRASHED,
                        "The browser engine is not installed. Reload to be asked again.",
                    ),
                );
            }
        });
        seat.begin(&alert, &answered);
    }

    /// The person said yes: downloaded on a thread of its own, its progress told to the room as
    /// the page's own words.
    fn download(&self, pane: &TerminalPane, id: Id, installer: Installer, offer: Offer) {
        self.tell(
            pane,
            id,
            status(BrowserState::STARTING, "Downloading the browser engine…"),
        );
        let pane_id = pane.id();
        let _ = std::thread::Builder::new()
            .name("browser-download".into())
            .spawn(move || {
                let last = std::sync::atomic::AtomicU64::new(u64::MAX);
                let progress = |got: u64, total: Option<u64>| {
                    let Some(total) = total.filter(|total| *total > 0) else {
                        return;
                    };
                    let percent = got.saturating_mul(100) / total;
                    // Told when the figure changes, not for every piece that arrives.
                    if last.swap(percent, std::sync::atomic::Ordering::Relaxed) != percent {
                        on_main(pane_id, move |pane| {
                            if let Some(browsers) = pane.page_browsers() {
                                browsers.tell(
                                    pane,
                                    id,
                                    status(
                                        BrowserState::STARTING,
                                        &format!("Downloading the browser engine… {percent}%"),
                                    ),
                                );
                            }
                        });
                    }
                };
                let result = installer.install(&offer, &progress, &AtomicBool::new(false));
                on_main(pane_id, move |pane| {
                    let Some(browsers) = pane.page_browsers() else {
                        return;
                    };
                    match result {
                        Ok(_) => browsers.installed(pane, id),
                        Err(e) => {
                            browsers.installing.set(false);
                            browsers.tell(
                                pane,
                                id,
                                status(
                                    BrowserState::CRASHED,
                                    &format!("The browser engine was not installed: {e}. Reload to try again."),
                                ),
                            );
                        }
                    }
                });
            });
    }

    /// The engine is on this Mac: every room of the page that waited for it starts.
    fn installed(&self, pane: &TerminalPane, _id: Id) {
        self.installing.set(false);
        let mut told = Vec::new();
        {
            let mut rooms = self.rooms.borrow_mut();
            let ids: Vec<Id> = rooms.ids().collect();
            for id in ids {
                rooms.retry(id, &mut told);
            }
        }
        self.effects.borrow_mut().extend(told);
        self.drain(pane);
    }

    /// Tells the page what a room's browser is.
    pub(super) fn tell(&self, pane: &TerminalPane, id: Id, state: BrowserState) {
        if let Some(handle) = pane.page_handle() {
            handle.page().browser(id, state);
            pane.drain_page();
        }
    }
}

/// An engine that is installed looks once a day for a newer one and installs it without asking
/// (the first yes covers it); the next start of the helper runs it. Once in the process, on a
/// thread of its own.
pub(crate) fn update_in_background() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let _ = std::thread::Builder::new()
            .name("browser-update".into())
            .spawn(|| {
                if let Some(installer) = crate::browser_install::installer() {
                    let _ = installer.update_if_due(&|_, _| {}, &AtomicBool::new(false));
                }
            });
    });
}
