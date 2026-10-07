//! The sheet gate: every path that begins a sheet, ends one or asks whether
//! one is open goes through this module.
//!
//! A sheet is window-modal — it attaches to an `NSWindow` and blocks the
//! whole window — so **where a question's sheet sits** is a decision of its
//! own, and it is made here, not by the asker. A pane's question names the
//! pane ([`Asker::Pane`]: the password sheet, the upload and download
//! confirmations, the stop question, the preview and remote-job errors, an
//! uncommon link's confirmation and its "Download To…" panel); a question
//! about a window names the window ([`Asker::Window`]: the close question,
//! the application's report of kept previews on the key window).
//!
//! **A background tab's question waits.** A window carries several tabs and
//! one window holds one sheet: a sheet attached straight to it from a
//! background tab would open that tab's question over the tab on screen. So
//! a pane whose tab is not shown ([`TerminalPane::tab_shown`]) **parks** its
//! question in its own slot ([`TerminalPane::parked`]) — the alert or panel
//! with its completion block, as begun — and it opens when the tab comes on
//! screen or the window's sheet ends ([`open_parked`]). Everything the
//! asker holds stays exactly as when it asked: a parked password question
//! still holds its reply, so the job waits as it would on an open sheet,
//! and an update's wait sees it. That is why the decision has a single
//! place: a call that went around it would open the question over the
//! front tab, silently.
//!
//! **The seat is resolved from the pane object, not by id.** A pane's
//! close answers its open sheet (`TerminalPane::close_password` inside
//! `begin_close`) after the pane has left its split tree, where an id lookup
//! through the application's lists no longer finds it — the sheet would stay
//! up over a closed pane. The pane knows where it stands; the owner's events
//! (`PaneHost`) carry no AppKit type for it.
//!
//! What this module does **not** decide is whether a question may open at
//! all: every asker keeps its own gate (the password's single sheet, the
//! upload queue's `set_asking`, the stop question's slot) and asks
//! [`Seat::is_taken`] where it did before — a parked question counts as a
//! taken seat, the one sheet a tab can hold.
//!
//! `make audit` fails on `beginSheet`, `endSheet` or `attachedSheet` outside
//! this file; the settings window's own panel (a window of its own, never a
//! terminal's) is exempt.

use std::collections::VecDeque;

use block2::{DynBlock, RcBlock};
use objc2::Message;
use objc2::rc::Retained;
use objc2_app_kit::{NSAlert, NSModalResponse, NSModalResponseCancel, NSSavePanel, NSWindow};

use crate::pane::TerminalPane;

/// Whose question a sheet asks.
pub(crate) enum Asker<'a> {
    /// A pane's own question — its sheet belongs where the pane is shown.
    Pane(&'a TerminalPane),
    /// A question about a whole window (closing it), or the application's
    /// report shown on the key window.
    Window(&'a NSWindow),
}

/// Where an asker's sheets sit. Opaque: the asker begins, ends and asks
/// through it and never holds the window itself.
pub(crate) struct Seat {
    window: Retained<NSWindow>,
    /// The asking pane — its tab decides whether the sheet opens now or
    /// parks ([`TerminalPane::tab_shown`]); `None` for a window's question,
    /// which never parks.
    pane: Option<Retained<TerminalPane>>,
}

/// A question begun while its pane's tab was not on screen: what
/// [`Seat::begin`] would have opened, kept until [`open_parked`] opens it or
/// [`Seat::end`] / [`answer_parked`] answers it unopened.
pub(crate) struct Parked {
    sheet: Sheet,
    answered: RcBlock<dyn Fn(NSModalResponse)>,
}

enum Sheet {
    Alert(Retained<NSAlert>),
    Panel(Retained<NSSavePanel>),
}

impl Sheet {
    /// Whether this is the sheet whose window is `window` (the asker ends a
    /// sheet by its window).
    fn is(&self, window: &NSWindow) -> bool {
        match self {
            Sheet::Alert(alert) => std::ptr::eq(&*alert.window(), window),
            Sheet::Panel(panel) => {
                let panel: &NSWindow = panel;
                std::ptr::eq(panel, window)
            }
        }
    }

    fn begin(&self, window: &NSWindow, answered: &DynBlock<dyn Fn(NSModalResponse)>) {
        match self {
            Sheet::Alert(alert) => {
                alert.beginSheetModalForWindow_completionHandler(window, Some(answered));
            }
            Sheet::Panel(panel) => {
                panel.beginSheetModalForWindow_completionHandler(window, answered);
            }
        }
    }
}

/// The seat of `asker`'s sheets; `None` when the pane is not in a window
/// (detached, or not placed yet).
pub(crate) fn seat(asker: Asker<'_>) -> Option<Seat> {
    match asker {
        Asker::Pane(pane) => pane.window().map(|window| Seat {
            window,
            pane: Some(pane.retain()),
        }),
        Asker::Window(window) => Some(Seat {
            window: window.retain(),
            pane: None,
        }),
    }
}

/// Whether a pane of `pane`'s tab has a parked question — the one sheet
/// that tab holds while it is not on screen.
fn tab_parked(pane: &TerminalPane) -> bool {
    let container = crate::tab::container_of(pane);
    match container {
        Some(container) => container
            .panes()
            .iter()
            .any(|sibling| !sibling.parked().borrow().is_empty()),
        None => !pane.parked().borrow().is_empty(),
    }
}

impl Seat {
    /// The pane this seat's sheets park in: the asker's, while its tab is
    /// not on screen.
    fn parking(&self) -> Option<&TerminalPane> {
        self.pane.as_deref().filter(|pane| !pane.tab_shown())
    }

    /// Whether a sheet is already up here — two sheets cannot open on top
    /// of each other. A background tab's seat is taken by its own parked
    /// question, not by the sheet the window shows for another tab.
    pub(crate) fn is_taken(&self) -> bool {
        let parked = self.pane.as_deref().is_some_and(tab_parked);
        // The window's sheet is read only when it can matter.
        taken(self.parking().is_none(), parked, || {
            self.window.attachedSheet().is_some()
        })
    }

    /// Opens `alert` as a sheet here; `answered` gets its response. A
    /// background tab's question parks instead ([`Parked`]).
    pub(crate) fn begin(&self, alert: &NSAlert, answered: &DynBlock<dyn Fn(NSModalResponse)>) {
        self.begin_sheet(Sheet::Alert(alert.retain()), answered);
    }

    /// Opens an open or save panel as a sheet here; `answered` gets its
    /// response. A background tab's question parks instead.
    pub(crate) fn begin_panel(
        &self,
        panel: &NSSavePanel,
        answered: &DynBlock<dyn Fn(NSModalResponse)>,
    ) {
        self.begin_sheet(Sheet::Panel(panel.retain()), answered);
    }

    fn begin_sheet(&self, sheet: Sheet, answered: &DynBlock<dyn Fn(NSModalResponse)>) {
        match self.parking() {
            Some(pane) => pane.parked().borrow_mut().push_back(Parked {
                sheet,
                answered: answered.copy(),
            }),
            None => sheet.begin(&self.window, answered),
        }
    }

    /// Ends the sheet whose window is `sheet` with `code` — its completion
    /// block runs with that response. A parked question is answered the same
    /// way without ever opening: the asker's own bookkeeping runs as for a
    /// sheet that was up.
    pub(crate) fn end(&self, sheet: &NSWindow, code: NSModalResponse) {
        if let Some(pane) = self.pane.as_deref() {
            let parked = {
                let mut slot = pane.parked().borrow_mut();
                slot.iter()
                    .position(|parked| parked.sheet.is(sheet))
                    .and_then(|index| slot.remove(index))
            };
            if let Some(parked) = parked {
                parked.answered.call((code,));
                return;
            }
        }
        self.window.endSheet_returnCode(sheet, code);
    }
}

/// `pane`'s first parked question opens — when its tab is on screen and
/// its window holds no sheet. The tab coming on screen and a sheet ending
/// on the window call this; the next parked question opens when this one
/// ends, so several open one after another.
pub(crate) fn open_parked(pane: &TerminalPane) {
    let Some(window) = pane.window() else {
        return;
    };
    if !may_open(pane.tab_shown(), window.attachedSheet().is_some()) {
        return;
    }
    let parked = pane.parked().borrow_mut().pop_front();
    if let Some(parked) = parked {
        parked.sheet.begin(&window, &parked.answered);
    }
}

/// The pane is closing: every parked question is answered `Cancel`
/// unopened, the way a closing window's sheets go — each asker's block
/// clears its own slot and gate. Each is taken out of the slot before its
/// block runs, so a block that reaches the slot finds it unborrowed.
pub(crate) fn answer_parked(pane: &TerminalPane) {
    loop {
        let parked = pane.parked().borrow_mut().pop_front();
        let Some(parked) = parked else {
            return;
        };
        parked.answered.call((NSModalResponseCancel,));
    }
}

/// A pane's parked questions, oldest first.
pub(crate) type ParkedQueue = VecDeque<Parked>;

/// Whether a seat is taken — the rule, without AppKit. `shown`: the
/// asker's tab is on screen (a window's own question always is);
/// `parked`: a question of that tab waits; `sheet_up`: the window holds a
/// sheet. A background tab's seat is its own: the window's sheet belongs to
/// the tab on screen (or to the window) and does not take it.
fn taken(shown: bool, parked: bool, sheet_up: impl FnOnce() -> bool) -> bool {
    parked || (shown && sheet_up())
}

/// Whether a parked question may open now — its tab on screen and the
/// window free.
fn may_open(shown: bool, sheet_up: bool) -> bool {
    shown && !sheet_up
}

#[cfg(test)]
mod tests {
    use super::{may_open, taken};

    /// A background tab parks, so the front tab's sheet does not take its
    /// seat — but its own parked question does, and on screen both do.
    #[test]
    fn a_background_tab_is_taken_only_by_its_own_question() {
        assert!(!taken(false, false, || true), "the front tab's sheet");
        assert!(taken(false, true, || false), "its own parked question");
        assert!(taken(true, false, || true), "on screen: the window's sheet");
        assert!(taken(true, true, || false), "on screen: still parked");
        assert!(!taken(true, false, || false));
        // In the background the window is not even asked.
        assert!(!taken(false, false, || unreachable!("the window was read")));
    }

    /// A parked question opens once its tab is on screen and the window is
    /// free; never over another sheet, never in the background.
    #[test]
    fn a_parked_question_opens_on_screen_over_nothing() {
        assert!(may_open(true, false));
        assert!(!may_open(true, true), "the window holds a sheet: next turn");
        assert!(!may_open(false, false), "still in the background");
        assert!(!may_open(false, true));
    }
}
