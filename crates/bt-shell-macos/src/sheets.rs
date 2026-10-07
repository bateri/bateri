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
//! the application's report of kept previews on the key window). Today a
//! pane's sheet sits on the pane's own window: a window carries one tab, so
//! the window and the tab are the same thing. Once a window carries several
//! tabs a sheet attached straight to it would open a background tab's
//! question over the front one — that is why the decision has a single
//! place: a call that went around it would keep doing exactly that,
//! silently.
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
//! [`Seat::is_taken`] where it did before.
//!
//! `make audit` fails on `beginSheet`, `endSheet` or `attachedSheet` outside
//! this file; the settings window's own panel (a window of its own, never a
//! terminal's) is exempt.

use block2::DynBlock;
use objc2::Message;
use objc2::rc::Retained;
use objc2_app_kit::{NSAlert, NSModalResponse, NSSavePanel, NSWindow};

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
pub(crate) struct Seat(Retained<NSWindow>);

/// The seat of `asker`'s sheets; `None` when the pane is not in a window
/// (detached, or not placed yet).
pub(crate) fn seat(asker: Asker<'_>) -> Option<Seat> {
    match asker {
        Asker::Pane(pane) => pane.window().map(Seat),
        Asker::Window(window) => Some(Seat(window.retain())),
    }
}

impl Seat {
    /// Whether a sheet is already up here — two sheets cannot open on top
    /// of each other.
    pub(crate) fn is_taken(&self) -> bool {
        self.0.attachedSheet().is_some()
    }

    /// Opens `alert` as a sheet here; `answered` gets its response.
    pub(crate) fn begin(&self, alert: &NSAlert, answered: &DynBlock<dyn Fn(NSModalResponse)>) {
        alert.beginSheetModalForWindow_completionHandler(&self.0, Some(answered));
    }

    /// Opens an open or save panel as a sheet here; `answered` gets its
    /// response.
    pub(crate) fn begin_panel(
        &self,
        panel: &NSSavePanel,
        answered: &DynBlock<dyn Fn(NSModalResponse)>,
    ) {
        panel.beginSheetModalForWindow_completionHandler(&self.0, answered);
    }

    /// Ends the sheet whose window is `sheet` with `code` — its completion
    /// block runs with that response.
    pub(crate) fn end(&self, sheet: &NSWindow, code: NSModalResponse) {
        self.0.endSheet_returnCode(sheet, code);
    }
}
