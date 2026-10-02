//! The password sheet of bateri's own ssh master (047 R4): ssh asks through
//! askpass ([`crate::ssh_route`]), the job's thread hands the question to the
//! pane that started the job and blocks on a channel until the sheet closes.
//!
//! - **Title** the host, **text** the job, ssh's own prompt small above a
//!   secure field; a second password prompt in the same attempt says
//!   "Wrong password — try again". "Log In" (default) / "Cancel" (Esc).
//! - **Arbitration**: never on top of another sheet (the window's attached
//!   sheet — another pane's too, the close question); a job that does not
//!   already hold the pane's sheet gate (`Transfers::set_asking`) takes it for
//!   the sheet's lifetime. Refused → beep and "nobody answered".
//! - **Nobody answered** is the dropped sender: the sheet's Cancel, a refused
//!   sheet, a pane that is gone, [`TerminalPane::close_password`] on
//!   `begin_close` (⌘W, ⌘Q). The job's thread then sees `None` and ssh is
//!   stopped before it could try an empty password.
//!
//! The Keychain's "Remember" box is 047 phase-3's.

use std::cell::RefCell;
use std::sync::mpsc::{self, Sender};

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSAlert, NSAlertFirstButtonReturn, NSFont, NSModalResponse, NSModalResponseCancel,
    NSSecureTextField, NSTextField, NSView,
};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString, ns_string};

use crate::pane::TerminalPane;
use crate::preview::beep;
use crate::remote_helper::Dial;
use crate::ssh_route::{Answerer, Ask, Question};
use crate::uploader::on_pane;

/// The job that asks — the sheet's text says what the password is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Job {
    Upload,
    Download,
    Preview,
    Finder,
}

impl Job {
    fn text(self) -> &'static str {
        match self {
            Self::Upload => "Log in to upload the dropped items.",
            Self::Download => "Log in to download the file.",
            Self::Preview => "Log in to preview the file.",
            Self::Finder => "Log in to copy the file to Finder.",
        }
    }
}

/// The open sheet: the alert (kept alive for the sheet's duration), the
/// waiting thread's sender and whether the sheet took the pane's gate itself.
pub(crate) struct PasswordSheet {
    alert: Retained<NSAlert>,
    reply: Sender<Option<String>>,
    releases: bool,
}

/// The accessory's width: NSAlert's text column.
const FIELD_WIDTH: f64 = 260.0;

impl TerminalPane {
    /// A remote job's route gate (047 R5): `job` `Some` — the user started it
    /// and a sheet may ask; `holds` — the job already holds the pane's sheet
    /// gate (the upload's probe, the download's count). `None` — a background
    /// job, which never asks.
    pub(crate) fn dial(&self, target: bt_core::RemoteTarget, job: Option<(Job, bool)>) -> Dial {
        let ask = match job {
            Some((job, holds)) => Ask::Sheet(self.password_asker(target.host.clone(), job, holds)),
            None => Ask::Never,
        };
        Dial::gated(self.masters(), target, ask)
    }

    /// The answerer a job's thread calls for every prompt: the question goes to
    /// this pane's main queue and the thread waits for the sheet.
    fn password_asker(&self, host: String, job: Job, holds: bool) -> Answerer {
        let (id, lookup) = (self.id(), self.lookup());
        Box::new(move |question: &Question| {
            let (tx, rx) = mpsc::channel();
            let (question, host) = (question.clone(), host.clone());
            on_pane(lookup, id, move |pane| {
                pane.ask_password(&host, job, holds, &question, tx);
            });
            // A pane that is gone drops the closure, and the sender with it.
            rx.recv().ok().flatten()
        })
    }

    /// Opens the sheet, or drops `reply` (beep) when it cannot.
    fn ask_password(
        &self,
        host: &str,
        job: Job,
        holds: bool,
        question: &Question,
        reply: Sender<Option<String>>,
    ) {
        if self.is_closed() || self.password().borrow().is_some() {
            return;
        }
        let Some(window) = self.window() else {
            return;
        };
        let gate_taken = {
            let uploads = self.uploads().borrow();
            uploads.asking() || self.upload_alert().borrow().is_some()
        } || self.upload_stop().borrow().is_some();
        if window.attachedSheet().is_some() || (!holds && gate_taken) {
            beep();
            return;
        }
        let mtm = self.mtm();
        let alert = NSAlert::new(mtm);
        alert.setMessageText(&NSString::from_str(host));
        let text = if question.again {
            format!("Wrong password — try again.\n{}", job.text())
        } else {
            job.text().to_owned()
        };
        alert.setInformativeText(&NSString::from_str(&text));
        let field = accessory(mtm, &alert, &question.prompt);
        alert.addButtonWithTitle(ns_string!("Log In"));
        let cancel = alert.addButtonWithTitle(ns_string!("Cancel"));
        // Esc by hand (the rationale of `window::alert`).
        cancel.setKeyEquivalent(ns_string!("\u{1b}"));
        alert.window().setInitialFirstResponder(Some(&field));
        if !holds {
            self.uploads().borrow_mut().set_asking(true);
        }
        self.password().replace(Some(PasswordSheet {
            alert: alert.clone(),
            reply,
            releases: !holds,
        }));
        let (id, lookup) = (self.id(), self.lookup());
        // The block is `Fn`: the field is read once.
        let field = RefCell::new(Some(field));
        let answered = RcBlock::new(move |response: NSModalResponse| {
            // audit: the sheet's completion block runs on AppKit's main thread.
            let mtm = MainThreadMarker::new().expect("the sheet block is on the main thread");
            let field = field.borrow_mut().take();
            let Some(pane) = lookup(mtm, id) else {
                return;
            };
            // Empty: `close_password` already answered and released.
            let Some(sheet) = pane.password().take() else {
                return;
            };
            if sheet.releases {
                pane.uploads().borrow_mut().set_asking(false);
            }
            let answer = field
                .filter(|_| response == NSAlertFirstButtonReturn)
                .map(|field| field.stringValue().to_string());
            let _ = sheet.reply.send(answer);
        });
        alert.beginSheetModalForWindow_completionHandler(&window, Some(&answered));
    }

    /// The pane closes (or the application quits): the waiting job is answered
    /// "nobody" and the sheet goes.
    pub(crate) fn close_password(&self) {
        let Some(sheet) = self.password().take() else {
            return;
        };
        let PasswordSheet {
            alert,
            reply,
            releases,
        } = sheet;
        drop(reply);
        if releases {
            self.uploads().borrow_mut().set_asking(false);
        }
        if let Some(window) = self.window() {
            window.endSheet_returnCode(&alert.window(), NSModalResponseCancel);
        }
    }
}

/// ssh's prompt in the small font above the secure field, as the alert's
/// accessory; the field is returned for the first responder and the answer.
fn accessory(mtm: MainThreadMarker, alert: &NSAlert, prompt: &str) -> Retained<NSSecureTextField> {
    let label = NSTextField::labelWithString(&NSString::from_str(prompt.trim()), mtm);
    label.setFont(Some(&NSFont::systemFontOfSize(
        NSFont::smallSystemFontSize(),
    )));
    let label_h = label.fittingSize().height.ceil();
    let field_h = 22.0;
    let gap = 4.0;
    let view = NSView::initWithFrame(
        NSView::alloc(mtm),
        NSRect::new(
            NSPoint::new(0.0, 0.0),
            NSSize::new(FIELD_WIDTH, field_h + gap + label_h),
        ),
    );
    label.setFrame(NSRect::new(
        NSPoint::new(0.0, field_h + gap),
        NSSize::new(FIELD_WIDTH, label_h),
    ));
    let field = NSSecureTextField::initWithFrame(
        NSSecureTextField::alloc(mtm),
        NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(FIELD_WIDTH, field_h)),
    );
    view.addSubview(&label);
    view.addSubview(&field);
    alert.setAccessoryView(Some(&view));
    field
}
