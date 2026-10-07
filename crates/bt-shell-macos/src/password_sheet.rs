//! The password sheet of bateri's own ssh master and the pane's half
//! of the saved passwords: ssh asks through askpass
//! ([`crate::ssh_route`]), the job's thread hands the question to the pane
//! that started the job and blocks on a channel until the sheet closes.
//!
//! - **Title** the host, **text** the job, ssh's own prompt small above a
//!   secure field; a second password prompt in the same attempt says
//!   "Wrong password — try again", one after the saved password "The saved
//!   password didn't work". "Log In" (default) / "Cancel" (Esc).
//! - **Remember in Keychain** — only for an account password
//!   ([`Prompt::Password`]; a passphrase or a code is never kept) and
//!   **ticked by default** (the user's decision, 2026-10-02). It is written
//!   after the master is up, never before: a wrong password is not saved.
//! - **Arbitration**: never on top of another sheet (one open where the
//!   pane's questions attach, [`crate::sheets`] — another pane's too, the
//!   close question); a job that does not
//!   already hold the pane's sheet gate (`Transfers::set_asking`) takes it for
//!   the sheet's lifetime. Refused → beep and "nobody answered".
//! - **Nobody answered** is the dropped sender: the sheet's Cancel, a refused
//!   sheet, a pane that is gone, [`TerminalPane::close_password`] on
//!   `begin_close` (⌘W, ⌘Q). The job's thread then sees `None` and ssh is
//!   stopped before it could try an empty password.
//! - **Sign In…**: a background job (the link check, the load
//!   indicator) that could not log in by itself says so
//!   ([`ssh_route::SIGN_IN_NEEDED`]); the ssh status bar then shows the
//!   button, whose click opens this sheet. Any user job's successful login
//!   hides it and the background jobs try again at once
//!   ([`TerminalPane::signed_in`]).
//! - **Forget Password**: Shell ▸ Forget Password for “{host}” drops the
//!   saved password and stops our master for it
//!   ([`crate::ssh_route::Masters::forget`]).

use std::cell::RefCell;
use std::sync::mpsc::{self, Sender};
use std::thread;

use block2::RcBlock;
use bt_core::SignIn;
use objc2::rc::Retained;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSAlert, NSAlertFirstButtonReturn, NSButton, NSControlStateValueOn, NSFont, NSModalResponse,
    NSModalResponseCancel, NSSecureTextField, NSTextField, NSView,
};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString, ns_string};

use crate::pane::TerminalPane;
use crate::preview::beep;
use crate::remote_helper::Dial;
use crate::sheets::{self, Asker};
use crate::ssh_route::{self, Answerer, Ask, Denied, Prompt, Question, Route, Typed};
use crate::uploader::on_pane;

/// The job that asks — the sheet's text says what the password is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Job {
    Upload,
    Download,
    Preview,
    Finder,
    /// The ssh status bar's Sign In….
    SignIn,
}

impl Job {
    fn text(self) -> &'static str {
        match self {
            Self::Upload => "Log in to upload the dropped items.",
            Self::Download => "Log in to download the file.",
            Self::Preview => "Log in to preview the file.",
            Self::Finder => "Log in to copy the file to Finder.",
            Self::SignIn => "Log in to use remote files.",
        }
    }
}

/// The open sheet: the alert (kept alive for the sheet's duration), the
/// waiting thread's sender and whether the sheet took the pane's gate itself.
pub(crate) struct PasswordSheet {
    alert: Retained<NSAlert>,
    reply: Sender<Option<Typed>>,
    releases: bool,
}

/// The accessory's width: NSAlert's text column.
const FIELD_WIDTH: f64 = 260.0;

/// The sheet's line above the job's text: why it asks again.
fn lead(question: &Question) -> Option<&'static str> {
    if question.stale {
        Some("The saved password didn't work.")
    } else if question.again {
        Some("Wrong password — try again.")
    } else {
        None
    }
}

impl TerminalPane {
    /// A remote job's route gate: `job` `Some` — the user started it
    /// and a sheet may ask; `holds` — the job already holds the pane's sheet
    /// gate (the upload's probe, the download's count). `None` — a background
    /// job, which never asks. A user job's successful dial is a login: the
    /// Sign In… button goes and the background jobs try again
    /// ([`Self::signed_in`]).
    pub(crate) fn dial(&self, target: bt_core::RemoteTarget, job: Option<(Job, bool)>) -> Dial {
        let Some((job, holds)) = job else {
            return Dial::gated(self.masters(), target, Ask::Never);
        };
        let ask = Ask::Sheet(self.password_asker(target.host.clone(), job, holds));
        let dial = Dial::gated(self.masters(), target, ask);
        let (id, lookup) = (self.id(), self.lookup());
        let argv = dial.argv;
        Dial {
            argv: Box::new(move || {
                let route = argv();
                if route.is_ok() {
                    on_pane(lookup, id, |pane| pane.signed_in());
                }
                route
            }),
            ..dial
        }
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
        reply: Sender<Option<Typed>>,
    ) {
        if self.is_closed() || self.password().borrow().is_some() {
            return;
        }
        let Some(seat) = sheets::seat(Asker::Pane(self)) else {
            return;
        };
        let gate_taken = {
            let uploads = self.uploads().borrow();
            uploads.asking() || self.upload_alert().borrow().is_some()
        } || self.upload_stop().borrow().is_some();
        if seat.is_taken() || (!holds && gate_taken) {
            beep();
            return;
        }
        let mtm = self.mtm();
        let alert = NSAlert::new(mtm);
        alert.setMessageText(&NSString::from_str(host));
        let text = match lead(question) {
            Some(lead) => format!("{lead}\n{}", job.text()),
            None => job.text().to_owned(),
        };
        alert.setInformativeText(&NSString::from_str(&text));
        let (field, remember) = accessory(
            mtm,
            &alert,
            &question.prompt,
            question.class == Prompt::Password,
        );
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
        // The block is `Fn`: the controls are read once.
        let controls = RefCell::new(Some((field, remember)));
        let answered = RcBlock::new(move |response: NSModalResponse| {
            // audit: the sheet's completion block runs on AppKit's main thread.
            let mtm = MainThreadMarker::new().expect("the sheet block is on the main thread");
            let controls = controls.borrow_mut().take();
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
            let answer = controls
                .filter(|_| response == NSAlertFirstButtonReturn)
                .map(|(field, remember)| Typed {
                    text: field.stringValue().to_string(),
                    remember: remember.is_some_and(|box_| box_.state() == NSControlStateValueOn),
                });
            let _ = sheet.reply.send(answer);
            // A postponed update may have waited for this sheet.
            pane.host().uploads_changed(pane.id());
        });
        seat.begin(&alert, &answered);
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
        if let Some(seat) = sheets::seat(Asker::Pane(self)) {
            seat.end(&alert.window(), NSModalResponseCancel);
        }
    }

    // ─── Sign In… ────────────────────────────────────────────────────────
    //
    // The button's one owner is the session (`Session::sign_in`): it clears it
    // with the remote state, so the pane keeps no copy that could disagree.

    /// A background job of remote generation `command` failed with `text`:
    /// if signing in would fix it, the ssh status bar shows Sign In…. A late
    /// reply of another generation does nothing.
    pub(crate) fn background_failed(&self, command: u64, text: &str) {
        if text != ssh_route::SIGN_IN_NEEDED || self.masters().is_none() {
            return;
        }
        let Some(session) = self.session() else {
            return;
        };
        let current = session.remote_target().map(|(now, ..)| now);
        if current != Some(command) || session.sign_in().is_some() {
            return;
        }
        session.set_sign_in(command, Some(SignIn::default()));
        self.view().sync_cursor_rects();
        // The pointer may already be over the new button.
        let at = self.view().pointer_context_column();
        self.sign_in_hover(at);
    }

    /// A background job logged in (a sample, a link check): the button goes
    /// and the load indicator samples again — the login works now, whoever
    /// made it (another pane's sign-in, `ssh-add`, the user's own master).
    pub(crate) fn background_succeeded(&self) {
        if self.hide_sign_in() {
            self.remote_helper().borrow_mut().retry();
            self.retry_stats();
        }
    }

    /// Hides the button; `true` if one was shown. The load indicator's own
    /// sample calls it directly — sampling already runs.
    pub(crate) fn hide_sign_in(&self) -> bool {
        let Some(session) = self.session() else {
            return false;
        };
        let Some((command, _)) = session.sign_in() else {
            return false;
        };
        session.set_sign_in(command, None);
        self.view().sync_cursor_rects();
        true
    }

    /// A user job's dial succeeded while the button is shown: the background
    /// jobs try again at once (the helper forgets its held failure, the load
    /// indicator samples) and their success hides it
    /// ([`Self::background_succeeded`]) — the dial alone does not say a login
    /// happened (today's argv is a success too).
    pub(crate) fn signed_in(&self) {
        if self
            .session()
            .and_then(|session| session.sign_in())
            .is_none()
        {
            return;
        }
        self.remote_helper().borrow_mut().retry();
        self.retry_stats();
    }

    /// The mouse is on the context row at dock-local column `col` (`None` →
    /// off it): the button's hover tone, a frame only on its edge.
    pub(crate) fn sign_in_hover(&self, at: Option<(u16, u16)>) {
        let Some(session) = self.session() else {
            return;
        };
        let Some((command, shown)) = session.sign_in() else {
            return;
        };
        let hover = at.is_some_and(|(col, context)| self.sign_in_hit(col, context));
        if hover != shown.hover {
            session.set_sign_in(command, Some(SignIn { hover }));
        }
    }

    /// Whether dock-local column `col` of the context row (`context` is the
    /// row's budget) is on the drawn button — the drawing's layout
    /// (`Session::sign_in_span`).
    fn sign_in_hit(&self, col: u16, context: u16) -> bool {
        self.session()
            .and_then(|session| session.sign_in_span(context))
            .is_some_and(|(start, end)| (start..end).contains(&col))
    }

    /// A click on the context row: on the button it signs in and `true`.
    pub(crate) fn sign_in_click(&self, col: u16, context: u16) -> bool {
        if !self.sign_in_hit(col, context) {
            return false;
        }
        self.sign_in();
        true
    }

    /// The button's job: the route gate with the sheet, on its own thread.
    /// Our master up is a login: the button goes and the background tries
    /// again. Today's argv (no socket of ours possible, the user's own master)
    /// only retries — the background's own success or failure decides. A
    /// cancel says nothing more; a failure (ssh's reason, an unknown host key)
    /// is a sheet.
    fn sign_in(&self) {
        let (Some(masters), Some((_, target, _))) = (
            self.masters(),
            self.session().and_then(|session| session.remote_target()),
        ) else {
            return;
        };
        let asker = self.password_asker(target.host.clone(), Job::SignIn, false);
        let (id, lookup) = (self.id(), self.lookup());
        let _ = thread::Builder::new()
            .name("ssh sign in".into())
            .spawn(move || {
                let outcome = masters.ensure(&target, Ask::Sheet(asker));
                let host = target.host;
                on_pane(lookup, id, move |pane| match outcome {
                    Ok(Route::Ours(_)) => pane.background_succeeded(),
                    Ok(Route::Direct) => pane.signed_in(),
                    Err(Denied::Cancelled) => {}
                    Err(denied) => {
                        pane.failure_sheet(&format!("Can't sign in to {host}"), &denied.text());
                    }
                });
            });
    }

    // ─── Forget Password ─────────────────────────────────────────────────

    /// Shell ▸ Forget Password's enablement: a remote tab whose account has a
    /// saved password. The account is the one a job already resolved — the
    /// main thread starts no `ssh -G` — so it is grey until the first remote
    /// job; the Keychain is asked attributes only (no consent prompt).
    pub(crate) fn can_forget_password(&self) -> bool {
        let (Some(masters), Some((_, target, _))) = (
            self.masters(),
            self.session().and_then(|session| session.remote_target()),
        ) else {
            return false;
        };
        masters.has_saved(&target)
    }

    /// Shell ▸ Forget Password for “{host}”: the saved password goes and our
    /// master stops taking jobs; the pane's helper session closes too, so the
    /// next background job asks the gate afresh and finds no login — the
    /// status bar's Sign In….
    pub(crate) fn forget_password(&self) {
        let (Some(masters), Some((_, target, _))) = (
            self.masters(),
            self.session().and_then(|session| session.remote_target()),
        ) else {
            return;
        };
        let (id, lookup) = (self.id(), self.lookup());
        let _ = thread::Builder::new()
            .name("ssh forget".into())
            .spawn(move || {
                masters.forget(&target);
                on_pane(lookup, id, |pane| {
                    pane.remote_helper().borrow_mut().close();
                    pane.retry_stats();
                });
            });
    }
}

/// ssh's prompt in the small font above the secure field, and under it the
/// Remember in Keychain box (ticked) for an account password, as the alert's
/// accessory; the field and the box are returned for the answer.
fn accessory(
    mtm: MainThreadMarker,
    alert: &NSAlert,
    prompt: &str,
    password: bool,
) -> (Retained<NSSecureTextField>, Option<Retained<NSButton>>) {
    let label = NSTextField::labelWithString(&NSString::from_str(prompt.trim()), mtm);
    label.setFont(Some(&NSFont::systemFontOfSize(
        NSFont::smallSystemFontSize(),
    )));
    let label_h = label.fittingSize().height.ceil();
    let field_h = 22.0;
    let gap = 4.0;
    let remember = password.then(|| {
        // SAFETY: a checkbox without a target or action; its state is read
        // when the sheet closes.
        let check = unsafe {
            NSButton::checkboxWithTitle_target_action(
                ns_string!("Remember in Keychain"),
                None,
                None,
                mtm,
            )
        };
        check.setState(NSControlStateValueOn);
        check
    });
    let check_h = remember
        .as_ref()
        .map_or(0.0, |check| check.fittingSize().height.ceil() + gap);
    let view = NSView::initWithFrame(
        NSView::alloc(mtm),
        NSRect::new(
            NSPoint::new(0.0, 0.0),
            NSSize::new(FIELD_WIDTH, check_h + field_h + gap + label_h),
        ),
    );
    label.setFrame(NSRect::new(
        NSPoint::new(0.0, check_h + field_h + gap),
        NSSize::new(FIELD_WIDTH, label_h),
    ));
    let field = NSSecureTextField::initWithFrame(
        NSSecureTextField::alloc(mtm),
        NSRect::new(
            NSPoint::new(0.0, check_h),
            NSSize::new(FIELD_WIDTH, field_h),
        ),
    );
    view.addSubview(&label);
    view.addSubview(&field);
    if let Some(check) = &remember {
        check.setFrame(NSRect::new(
            NSPoint::new(0.0, 0.0),
            NSSize::new(FIELD_WIDTH, check_h - gap),
        ));
        view.addSubview(check);
    }
    alert.setAccessoryView(Some(&view));
    (field, remember)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn question(again: bool, stale: bool) -> Question {
        Question {
            prompt: "u@prod's password: ".to_owned(),
            class: Prompt::Password,
            again,
            stale,
        }
    }

    #[test]
    fn the_sheet_says_why_it_asks_again() {
        assert_eq!(lead(&question(false, false)), None);
        assert_eq!(
            lead(&question(true, false)),
            Some("Wrong password — try again.")
        );
        assert_eq!(
            lead(&question(false, true)),
            Some("The saved password didn't work.")
        );
    }
}
