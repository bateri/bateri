//! The **AppKit half** of the upload queue (037 Karar 7 → Kullanıcı kararı):
//! the AppKit and dispatch work that goes from the drop to the confirmation
//! sheet, from the sheet to the stream, from the stream to the dock's status
//! line, to the "Show files (N)" popover and to the stop question. The queue
//! is **the pane's** (039 phase-2): sheets attach to the pane view's window,
//! the popover to the pane's `BateriView`; the title's `↑ N%` prefix, the
//! notification and the Dock tile come from the pane's owner
//! ([`crate::pane::PaneHost`] - `title_changed`, `notify`, `uploads_changed`),
//! and the Dock tile itself is the total over all panes
//! ([`refresh_dock_tile`], the owner walks them).
//!
//! The rule and the text are in `upload` (pure, tested); only the binding is
//! here. Every report from the background threads goes to the main queue
//! **with the pane id** and finds the pane through the owner's path
//! ([`PaneLookup`], the alternate-screen messenger's pattern): the report of
//! a closed pane is silently dropped.
//!
//! **Zero frames when idle:** while the stream runs the progress report is at
//! most one per [`upload::TICK`] and at most one on the main queue; when the
//! queue ends the result line stays for [`upload::LINGER`] and is then removed
//! with `None`, and no further frame is requested - that is the stopping condition.

use std::cell::RefCell;
use std::ptr::NonNull;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::thread;
use std::time::Instant;

use block2::RcBlock;
use bt_core::{HostMark, Transfer, TransferAction};
use dispatch2::{DispatchQueue, DispatchTime};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool, ProtocolObject};
use objc2::{MainThreadMarker, MainThreadOnly, sel};
use objc2_app_kit::{
    NSAlert, NSAlertFirstButtonReturn, NSAlertSecondButtonReturn, NSApplication, NSBox, NSBoxType,
    NSButton, NSColor, NSControlSize, NSEvent, NSEventMask, NSFont, NSFontWeightRegular,
    NSImageScaling, NSImageView, NSLineBreakMode, NSModalResponse, NSModalResponseAbort, NSPopover,
    NSPopoverBehavior, NSProgressIndicator, NSProgressIndicatorStyle, NSTextField, NSView,
    NSViewController,
};
use objc2_foundation::{
    NSBundle, NSError, NSPoint, NSRect, NSRectEdge, NSSize, NSString, NSUUID, ns_string,
};
use objc2_user_notifications::{
    UNAuthorizationOptions, UNMutableNotificationContent, UNNotificationRequest,
    UNUserNotificationCenter,
};

use crate::pane::{PaneLookup, TerminalPane};
use crate::upload::{
    self, Ended, Job, Local, Outcome, ProbeReply, RowAction, RowStatus, Shared, Stop, StopQuestion,
    UploadList,
};

/// Everything that returns to the main thread from the background probe.
struct Asked {
    command: u64,
    ssh: Vec<String>,
    host: String,
    mark: HostMark,
    reported: bool,
    result: Result<(Vec<Local>, ProbeReply), String>,
}

/// The confirmed drop: the items that will enter the queue.
struct Confirmed {
    command: u64,
    ssh: Vec<String>,
    host: String,
    mark: HostMark,
    jobs: Vec<Job>,
}

/// Finds the pane with `id` on the main thread and applies `work` to it.
fn on_pane(lookup: PaneLookup, id: u64, work: impl FnOnce(&TerminalPane) + Send + 'static) {
    DispatchQueue::main().exec_async(move || {
        // audit: a block running on the main queue is by definition on the main thread.
        let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
        if let Some(pane) = lookup(mtm, id) {
            work(&pane);
        }
    });
}

impl TerminalPane {
    /// A Finder drop in a remote session (037 Karar 7): local measurement and
    /// remote probe in the background, then the confirmation sheet. `false` →
    /// the drop was refused (local session, or another sheet is in progress -
    /// two sheets cannot open on top of each other).
    pub(crate) fn upload_drop(&self, paths: Vec<String>) -> bool {
        let Some(session) = self.session() else {
            return false;
        };
        let Some((command, target, cwd)) = session.remote_target() else {
            return false;
        };
        if !self.accepts_drop() {
            return false;
        }
        let mark = session
            .remote_mark()
            .map_or(HostMark::None, |(_, mark)| mark);
        let ssh = upload::ssh_argv(&target);
        let host = target.host;
        let reported = !cwd.is_empty();
        let (id, lookup) = (self.id(), self.lookup());
        self.uploads().borrow_mut().set_asking(true);
        let spawned = thread::Builder::new()
            .name("upload probe".into())
            .spawn(move || {
                let dir = reported.then_some(cwd.as_str());
                let result = upload::probe(&ssh, &host, dir, &paths);
                let asked = Asked {
                    command,
                    ssh,
                    host,
                    mark,
                    reported,
                    result,
                };
                on_pane(lookup, id, move |pane| pane.upload_asked(asked));
            });
        if spawned.is_err() {
            self.uploads().borrow_mut().set_asking(false);
            return false;
        }
        true
    }

    /// Whether a drop can be released on this pane - `draggingEntered:`'s
    /// question: no while an upload sheet (probe included) is in progress.
    pub(crate) fn accepts_drop(&self) -> bool {
        self.uploads().borrow().can_accept() && self.upload_stop().borrow().is_none()
    }

    /// The probe returned: the confirmation or error sheet.
    fn upload_asked(&self, asked: Asked) {
        // If the session ended in the meantime (ssh closed) there is nothing to ask.
        let alive = self
            .session()
            .and_then(|session| session.remote_target())
            .is_some_and(|(command, ..)| command == asked.command);
        // The sheet's window is the pane view's; if the pane was detached from
        // the window there is nowhere to ask either.
        let Some(window) = self.window().filter(|_| alive) else {
            self.uploads().borrow_mut().set_asking(false);
            return;
        };
        let mtm = self.mtm();
        let alert = NSAlert::new(mtm);
        let confirmed = match asked.result {
            Err(text) => {
                alert.setMessageText(&NSString::from_str(&format!(
                    "Can't upload to {}",
                    asked.host
                )));
                alert.setInformativeText(&NSString::from_str(&text));
                alert.addButtonWithTitle(ns_string!("OK"));
                None
            }
            Ok((items, reply)) => {
                let busy = self.uploads().borrow().busy();
                let sheet = upload::sheet(&asked.host, asked.reported, &items, &reply, busy);
                alert.setMessageText(&NSString::from_str(&sheet.message));
                alert.setInformativeText(&NSString::from_str(&sheet.informative));
                let confirm = alert.addButtonWithTitle(&NSString::from_str(sheet.button));
                confirm.setEnabled(sheet.enabled);
                let cancel = alert.addButtonWithTitle(ns_string!("Cancel"));
                // Esc by hand (the rationale of `window::alert`).
                cancel.setKeyEquivalent(ns_string!("\u{1b}"));
                sheet.enabled.then(|| Confirmed {
                    command: asked.command,
                    ssh: asked.ssh,
                    host: asked.host,
                    mark: asked.mark,
                    jobs: items
                        .into_iter()
                        .map(|local| Job {
                            local,
                            dir: reply.dir.clone(),
                        })
                        .collect(),
                })
            }
        };
        let (id, lookup) = (self.id(), self.lookup());
        // The block is `Fn`: the payload is taken once.
        let confirmed = RefCell::new(confirmed);
        let answered = RcBlock::new(move |response: NSModalResponse| {
            // audit: the sheet's completion block runs on AppKit's main thread.
            let mtm = MainThreadMarker::new().expect("the sheet block is on the main thread");
            let Some(pane) = lookup(mtm, id) else {
                return;
            };
            drop(pane.upload_alert().take());
            pane.uploads().borrow_mut().set_asking(false);
            if response != NSAlertFirstButtonReturn {
                return;
            }
            if let Some(confirmed) = confirmed.borrow_mut().take() {
                pane.upload_confirmed(confirmed);
            }
        });
        self.upload_alert().replace(Some(alert.clone()));
        alert.beginSheetModalForWindow_completionHandler(&window, Some(&answered));
    }

    /// Confirmation: the items go to the end of the queue, the first starts if the queue is idle.
    fn upload_confirmed(&self, confirmed: Confirmed) {
        let alive = self
            .session()
            .and_then(|session| session.remote_target())
            .is_some_and(|(command, ..)| command == confirmed.command);
        if !alive {
            return;
        }
        let queued = self.uploads().borrow_mut().enqueue(
            confirmed.command,
            confirmed.ssh,
            confirmed.host,
            confirmed.mark,
            confirmed.jobs,
        );
        if queued {
            self.upload_next();
            // If the queue was already flowing, the new items are in the list and the count.
            self.upload_refresh();
        }
    }

    /// Starts the next item on a background thread (no-op while an item flows).
    fn upload_next(&self) {
        let Some((ssh, job, shared)) = self.uploads().borrow_mut().start_next(Instant::now())
        else {
            return;
        };
        self.upload_refresh();
        let (id, lookup) = (self.id(), self.lookup());
        let spawned = thread::Builder::new().name("upload".into()).spawn({
            let shared = Arc::clone(&shared);
            move || {
                let outcome = upload::transfer(&ssh, &job.local, &job.dir, &shared, || {
                    tick(lookup, id, &shared);
                });
                on_pane(lookup, id, move |pane| pane.upload_finished(outcome));
            }
        });
        if let Err(error) = spawned {
            self.upload_finished(Outcome::Failed(error.to_string()));
        }
    }

    /// Progress report: refreshes the status line, the popover, the title and
    /// the Dock tile.
    fn upload_refresh(&self) {
        let status = self.uploads().borrow_mut().status(Instant::now());
        if let Some(status) = status {
            self.show_transfer(Some(status));
            self.rehover_upload();
        }
        self.refresh_upload_list();
        self.refresh_upload_title();
        self.uploads_changed();
    }

    /// The owner's Dock tile (the total over all panes) should refresh.
    fn uploads_changed(&self) {
        self.host().uploads_changed(self.id());
    }

    /// The flowing item ended: move on to the next or show the result. No path
    /// is pasted (037 phase-7): the result line says where it went.
    fn upload_finished(&self, outcome: Outcome) {
        let ended = self.uploads().borrow_mut().finish(outcome);
        // If the stop question was about the ended item, the sheet closes by itself.
        self.dismiss_stale_stop();
        match ended {
            Some(ended) => self.show_end(ended),
            None => self.upload_next(),
        }
        self.uploads_changed();
    }

    /// Shows the result line, closes the popover and the stale question,
    /// restores the title, sends a notification if bateri is in the
    /// background, and removes the line after [`upload::LINGER`].
    fn show_end(&self, ended: Ended) {
        self.close_upload_list();
        self.dismiss_stale_stop();
        self.show_transfer(Some(ended.line));
        self.refresh_upload_title();
        if let Some((title, body)) = ended.notice {
            self.host().notify(self.id(), &title, &body);
        }
        let (id, lookup) = (self.id(), self.lookup());
        let serial = ended.serial;
        let Ok(when) = DispatchTime::try_from(upload::LINGER) else {
            return;
        };
        // The error arm is not represented today (same rationale as the link's
        // clock); if it drops, the line stays until the next upload.
        let _ = DispatchQueue::main().after(when, move || {
            // audit: a block running on the main queue is by definition on the main thread.
            let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
            if let Some(pane) = lookup(mtm, id)
                && pane.uploads().borrow().linger_over(serial)
            {
                pane.show_transfer(None);
            }
        });
    }

    /// The title's `↑ N% · ` prefix (037 phase-7): if the percentage changed
    /// the owner rewrites the title - at most once per percent. There is no
    /// dock on the alternate screen and the title and tab are the only place that shows progress.
    ///
    /// It only notifies the owner and does not ask about the remote state's
    /// edge ([`TerminalPane::remote_or_title_changed`] is not used): that edge
    /// would go `check_upload_connection` → `show_end` → back here.
    fn refresh_upload_title(&self) {
        let changed = self.uploads().borrow_mut().title_percent_changed();
        if changed {
            self.host().title_changed(self.id());
        }
    }

    /// Writes the status line to the session and stores it for the mouse's
    /// button question. The hand cursor is set up by AppKit's cursor rect
    /// (`BateriView::hand_cursor_rects`); if buttons appeared or went away
    /// the rects are refreshed here, so when a button goes the hand does not
    /// hang under the mouse.
    fn show_transfer(&self, transfer: Option<Transfer>) {
        if let Some(session) = self.session() {
            session.set_transfer(transfer.as_ref());
        }
        self.uploads().borrow_mut().set_shown(transfer);
        // `borrow_mut` is done: computing the rects borrows `uploads` again.
        self.view().sync_cursor_rects();
    }

    /// The dock-local column ranges of the shown line's buttons (`context` is
    /// the context line's budget) - the cursor rects' input, from the same
    /// layout as click and hover (`bt_core::transfer_button_span`).
    pub(crate) fn upload_button_spans(&self, context: u16) -> Vec<(u16, u16)> {
        let uploads = self.uploads().borrow();
        let Some(shown) = uploads.shown() else {
            return Vec::new();
        };
        [TransferAction::List, TransferAction::Cancel]
            .into_iter()
            .filter_map(|action| bt_core::transfer_button_span(shown, context, action))
            .collect()
    }

    /// The mouse is on the context line at dock-local column `col` (`None` →
    /// outside the line; `context` is the context line's budget): if the
    /// button under it **changed**, rewrites the line - the hover tone, a
    /// frame only on that edge (037 phase-6). It does not set the cursor: the
    /// hand comes from the cursor rect, only refreshed here if stale. Without
    /// an upload it exits on the first question - every motion of an idle
    /// window would cost a borrow.
    pub(crate) fn upload_hover(&self, at: Option<(u16, u16)>) {
        let fresh = {
            let mut uploads = self.uploads().borrow_mut();
            let Some(shown) = uploads.shown() else {
                return;
            };
            let hover =
                at.and_then(|(col, context)| bt_core::transfer_button_at(shown, context, col));
            uploads.set_hover(hover)
        };
        match fresh {
            Some(fresh) => self.show_transfer(Some(fresh)),
            // If the dock's drawn place moved (point size, window size, band) the
            // hand cursor's rectangle must move too.
            None => self.view().sync_cursor_rects(),
        }
    }

    /// Recomputes the hover from the pointer's **current** place: the buttons
    /// are right-aligned and their widths come from the state (item count), so
    /// the line can change under a motionless pointer - `/code-review`.
    pub(crate) fn rehover_upload(&self) {
        let at = self.view().pointer_context_column();
        self.upload_hover(at);
    }

    /// The window stopped being key: `mouseMoved:` no longer arrives, the
    /// button must not hang in hover.
    pub(crate) fn unhover_upload(&self) {
        self.upload_hover(None);
    }

    /// Stop request (037 phase-7): ⌘., the line's `Cancel`/`Cancel all` and
    /// the popover's `Cancel all` are `all`, the popover row's `Cancel` is
    /// not. If the flowing item has been flowing longer than
    /// [`upload::STOP_ASK_AFTER`] it asks first; while an upload sheet is open
    /// (confirmation or question) the request is dropped - two sheets cannot
    /// open on top of each other.
    pub(crate) fn request_stop(&self, all: bool) {
        if self.uploads().borrow().asking()
            || self.upload_alert().borrow().is_some()
            || self.upload_stop().borrow().is_some()
        {
            return;
        }
        let request = self.uploads().borrow().stop_request(all, Instant::now());
        match request {
            None => {}
            Some(Stop::Now { id, all }) => self.apply_stop(id, all),
            Some(Stop::Ask(question)) => {
                self.close_upload_list();
                self.ask_stop(question);
            }
        }
    }

    /// Applies the stop ([`Uploads::stop`]); shows the result if it is known immediately.
    fn apply_stop(&self, id: Option<u64>, all: bool) {
        let ended = self.uploads().borrow_mut().stop(id, all);
        match ended {
            Some(ended) => self.show_end(ended),
            None => self.upload_refresh(),
        }
        self.uploads_changed();
    }

    /// The "Stop uploading?" sheet: `Keep uploading` is the default (Return)
    /// and Esc, `Stop` is destructive. The upload keeps running while the sheet
    /// is open; if the item ends meanwhile the sheet closes by itself
    /// ([`Self::dismiss_stale_stop`]).
    ///
    /// **Esc by hand**: `NSAlert` gives a button only one key equivalent - the
    /// first button carries Return and giving Esc to it too would take Return
    /// away. For the sheet's duration a local event monitor turns Esc in the
    /// sheet's window into the `Keep uploading` answer.
    fn ask_stop(&self, question: StopQuestion) {
        let mtm = self.mtm();
        let alert = NSAlert::new(mtm);
        alert.setMessageText(&NSString::from_str(&question.title));
        alert.setInformativeText(&NSString::from_str(&question.text));
        alert.addButtonWithTitle(ns_string!("Keep uploading"));
        let stop = alert.addButtonWithTitle(ns_string!("Stop"));
        stop.setHasDestructiveAction(true);
        let Some(window) = self.window() else {
            return;
        };
        let (id, lookup) = (self.id(), self.lookup());
        let (item, all) = (question.id, question.all);
        let answered = RcBlock::new(move |response: NSModalResponse| {
            // audit: the sheet's completion block runs on AppKit's main thread.
            let mtm = MainThreadMarker::new().expect("the sheet block is on the main thread");
            let Some(pane) = lookup(mtm, id) else {
                return;
            };
            let sheet = pane.upload_stop().borrow_mut().take();
            if let Some(sheet) = sheet {
                remove_monitor(sheet.monitor);
            }
            if response == NSAlertSecondButtonReturn {
                pane.apply_stop(Some(item), all);
            }
        });
        let sheet_window = alert.window();
        let monitor = add_key_monitor(move |event| {
            if event.keyCode() != ESCAPE {
                return false;
            }
            // audit: the local event monitor runs on the main thread.
            let mtm = MainThreadMarker::new().expect("the event monitor is on the main thread");
            // The window is compared at event time: the sheet's number is only
            // certain once it is shown (`/code-review`).
            let on_sheet = event
                .window(mtm)
                .is_some_and(|window| Retained::as_ptr(&window) == Retained::as_ptr(&sheet_window));
            if !on_sheet {
                return false;
            }
            if let Some(window) = lookup(mtm, id).and_then(|pane| pane.window()) {
                window.endSheet_returnCode(&sheet_window, NSAlertFirstButtonReturn);
            }
            true
        });
        self.upload_stop().replace(Some(StopSheet {
            alert: alert.clone(),
            id: item,
            monitor,
        }));
        alert.beginSheetModalForWindow_completionHandler(&window, Some(&answered));
    }

    /// If the stop question was about an item that no longer flows, closes the
    /// sheet (the answer counts as "never mind"): the question had named that item's loss.
    fn dismiss_stale_stop(&self) {
        let running = self.uploads().borrow().running_id();
        let stale = self
            .upload_stop()
            .borrow()
            .as_ref()
            .filter(|sheet| running != Some(sheet.id))
            .map(|sheet| sheet.alert.window());
        if let (Some(sheet_window), Some(window)) = (stale, self.window()) {
            window.endSheet_returnCode(&sheet_window, NSModalResponseAbort);
        }
    }

    /// The pane is closing (`begin_close`): the queue is cancelled and
    /// **abandoned** - no dock is left to show the result and the stream
    /// thread's report will no longer find this pane; the Dock tile is
    /// refreshed now, otherwise it would freeze on a half bar.
    pub(crate) fn abandon_uploads(&self) {
        self.close_upload_list();
        self.uploads().borrow_mut().abandon();
        self.dismiss_stale_stop();
        self.uploads_changed();
    }

    /// A popover row's button (`tag` is the item's id): `Cancel` on the
    /// flowing item (asks first if past 30 s), `Remove` on a waiting one (does not ask).
    pub(crate) fn upload_row_action(&self, id: u64) {
        let action = self.uploads().borrow().list().and_then(|list| {
            list.rows
                .into_iter()
                .find(|row| row.id == id)
                .and_then(|row| row.status.action())
        });
        match action {
            Some(RowAction::Cancel) => self.request_stop(false),
            Some(RowAction::Remove) => {
                self.uploads().borrow_mut().remove(id);
                self.upload_refresh();
            }
            None => {}
        }
    }

    /// Whether the remote session ended (the edge of the title and remote
    /// state report, [`TerminalPane::remote_or_title_changed`]): if it ended
    /// the waiting ones are cancelled, the flowing item ends with its own connection.
    pub(crate) fn check_upload_connection(&self) {
        let Some(command) = self.uploads().borrow().command() else {
            return;
        };
        let current = self
            .session()
            .and_then(|session| session.remote_target())
            .map(|(command, ..)| command);
        if current == Some(command) {
            return;
        }
        let ended = self.uploads().borrow_mut().close();
        match ended {
            Some(ended) => self.show_end(ended),
            // The waiting ones will never start: the popover closes.
            None => self.refresh_upload_list(),
        }
        self.uploads_changed();
    }

    /// A click on dock-local column `col` of the status line (on the context
    /// line, at the small class's pitch): if it lands on a button, does its
    /// work and `true`. `context` is the context line's budget (`bt_gpu::context_cols`).
    pub(crate) fn upload_click(&self, col: u16, context: u16) -> bool {
        let (action, span) = {
            let uploads = self.uploads().borrow();
            let Some(shown) = uploads.shown() else {
                return false;
            };
            (
                bt_core::transfer_button_at(shown, context, col),
                bt_core::transfer_button_span(shown, context, TransferAction::List),
            )
        };
        match action {
            Some(TransferAction::Cancel) => self.request_stop(true),
            Some(TransferAction::List) => self.toggle_upload_list(span),
            None => return false,
        }
        true
    }

    /// "Show files (N)": the queue's popover (037 phase-7) - attached to the
    /// button, `transient`: a click outside, Esc or pressing the button again closes it.
    ///
    /// **Pressing the button again**: a `transient` popover closes itself on a
    /// press outside and that same press reaches here too - so as not to
    /// reopen a popover that looks open, the time of the event that triggered
    /// the close is stored (`popoverWillClose:`) and nothing is done for that event.
    fn toggle_upload_list(&self, span: Option<(u16, u16)>) {
        let shown = self
            .upload_list()
            .borrow()
            .as_ref()
            .map(|list| list.popover.clone());
        if let Some(popover) = shown {
            // A popover that is closing (or that AppKit closed) is cleaned up
            // too: its monitor must not stay on top of the second's.
            let was_shown = popover.isShown();
            self.close_upload_list();
            if was_shown {
                return;
            }
        }
        if self.closed_by_current_event() {
            return;
        }
        let Some(list) = self.uploads().borrow().list() else {
            return;
        };
        if list.rows.len() <= 1 {
            return;
        }
        let Some(rect) = span.and_then(|(start, end)| self.view().context_span_rect(start, end))
        else {
            return;
        };
        let Some(window) = self.window() else {
            return;
        };
        let mtm = self.mtm();
        let popover = NSPopover::new(mtm);
        popover.setBehavior(NSPopoverBehavior::Transient);
        popover.setDelegate(Some(ProtocolObject::from_ref(self)));
        let controller = NSViewController::new(mtm);
        let content = NSView::new(mtm);
        let (size, live) = self.fill_list_view(&content, &list);
        controller.setView(&content);
        popover.setContentViewController(Some(&controller));
        popover.setContentSize(size);
        let (id, lookup) = (self.id(), self.lookup());
        let number = window.windowNumber();
        // Esc: the terminal window stays key, so Esc would go not to the
        // popover but to `keyDown:` - and from there to the remote shell. While
        // the popover is open the monitor swallows Esc in the pane's window and closes the popover.
        let monitor = add_key_monitor(move |event| {
            if event.keyCode() != ESCAPE || event.windowNumber() != number {
                return false;
            }
            // audit: the local event monitor runs on the main thread.
            let mtm = MainThreadMarker::new().expect("the event monitor is on the main thread");
            let Some(pane) = lookup(mtm, id) else {
                return false;
            };
            if pane.upload_list().borrow().is_some() {
                pane.close_upload_list();
                return true;
            }
            // Even if the popover closed Esc itself, the key must not go to the shell.
            pane.closed_by_current_event()
        });
        self.upload_list().replace(Some(UploadPopover {
            popover: popover.clone(),
            shape: shape_of(&list),
            live,
            monitor,
        }));
        let view: &NSView = self.view();
        popover.showRelativeToRect_ofView_preferredEdge(rect, view, NSRectEdge::MinY);
        let opened = self.uploads().borrow_mut().set_list_open(true);
        if let Some(line) = opened {
            self.show_transfer(Some(line));
        }
    }

    /// Whether the event that triggered the close is the current event ([`Self::toggle_upload_list`]).
    fn closed_by_current_event(&self) -> bool {
        let now = NSApplication::sharedApplication(self.mtm())
            .currentEvent()
            .map(|event| event.timestamp());
        now.is_some() && self.list_closed_at().get() == now
    }

    /// `popoverWillClose:`: the time of the event that triggered the close.
    pub(crate) fn upload_list_will_close(&self) {
        let now = NSApplication::sharedApplication(self.mtm())
            .currentEvent()
            .map(|event| event.timestamp());
        self.list_closed_at().set(now);
    }

    /// Closes the popover and takes the button out of its pressed tone; no-op
    /// if closed. `popoverDidClose:` lands here too (AppKit closed it itself).
    pub(crate) fn close_upload_list(&self) {
        let Some(list) = self.upload_list().borrow_mut().take() else {
            return;
        };
        remove_monitor(list.monitor);
        if list.popover.isShown() {
            list.popover.close();
        }
        let closed = self.uploads().borrow_mut().set_list_open(false);
        if let Some(line) = closed {
            self.show_transfer(Some(line));
        }
        // While the popover was open motion went to it: the mouse may be elsewhere now.
        self.rehover_upload();
    }

    /// While the popover is open refreshes progress in place; if the items or
    /// their states changed rebuilds the content. If the queue ended or the
    /// item count dropped to one the popover closes (the button goes away too).
    ///
    /// In place, because a button rebuilt every 200 ms would vanish while
    /// pressed and the click would be lost.
    fn refresh_upload_list(&self) {
        if self.upload_list().borrow().is_none() {
            return;
        }
        let list = self.uploads().borrow().list();
        let Some(list) = list.filter(|list| list.rows.len() > 1) else {
            self.close_upload_list();
            return;
        };
        let rebuild = {
            let open = self.upload_list().borrow();
            let Some(open) = open.as_ref() else {
                return;
            };
            if open.shape == shape_of(&list) {
                let running = list.rows.iter().filter_map(|row| match &row.status {
                    RowStatus::Running { fraction, detail } => Some((*fraction, detail)),
                    _ => None,
                });
                for ((bar, label), (fraction, detail)) in open.live.iter().zip(running) {
                    bar.setDoubleValue(fraction);
                    label.setStringValue(&NSString::from_str(detail));
                }
                None
            } else {
                open.popover
                    .contentViewController()
                    .map(|c| (open.popover.clone(), c.view()))
            }
        };
        if let Some((popover, content)) = rebuild {
            for sub in content.subviews().iter() {
                sub.removeFromSuperview();
            }
            let (size, live) = self.fill_list_view(&content, &list);
            popover.setContentSize(size);
            if let Some(open) = self.upload_list().borrow_mut().as_mut() {
                open.shape = shape_of(&list);
                open.live = live;
            }
        }
    }

    /// Lays the popover's content into `content`: the title, one row per
    /// item, a separator and `Cancel all ⌘.`. Returns: the content's size and
    /// the views of the flowing items to be refreshed (bar, detail).
    ///
    /// The layout is by hand and top-down (the view is not flipped; y is
    /// computed from the bottom at the end): the row count is small and the
    /// height is known from the state.
    fn fill_list_view(&self, content: &NSView, list: &UploadList) -> (NSSize, Vec<Live>) {
        let mtm = self.mtm();
        let target: &AnyObject = self.as_ref();
        let left = LIST_WIDTH - 2.0 * LIST_PAD - ROW_BUTTON_WIDTH - LIST_PAD;
        let mut placed: Vec<(Retained<NSView>, NSRect)> = Vec::new();
        let mut live = Vec::new();
        let mut top = LIST_PAD;
        let mut place = |view: Retained<NSView>, x: f64, top: f64, w: f64, h: f64| {
            placed.push((view, NSRect::new(NSPoint::new(x, top), NSSize::new(w, h))));
        };

        let title = label(mtm, &list.title, 12.0, &NSColor::secondaryLabelColor());
        title.setFont(Some(&NSFont::boldSystemFontOfSize(12.0)));
        place(
            Retained::into_super(Retained::into_super(title)),
            LIST_PAD,
            top,
            LIST_WIDTH - 2.0 * LIST_PAD,
            LINE_HEIGHT,
        );
        top += LINE_HEIGHT + ROW_GAP;

        for row in &list.rows {
            let row_top = top;
            let name = label(mtm, &row.name, 13.0, &NSColor::labelColor());
            name.setLineBreakMode(NSLineBreakMode::ByTruncatingMiddle);
            place(
                Retained::into_super(Retained::into_super(name)),
                LIST_PAD,
                top,
                left,
                NAME_HEIGHT,
            );
            top += NAME_HEIGHT;
            let detail = match &row.status {
                RowStatus::Running { fraction, detail } => {
                    let bar = NSProgressIndicator::initWithFrame(
                        NSProgressIndicator::alloc(mtm),
                        NSRect::ZERO,
                    );
                    bar.setStyle(NSProgressIndicatorStyle::Bar);
                    bar.setIndeterminate(false);
                    bar.setControlSize(NSControlSize::Small);
                    bar.setMinValue(0.0);
                    bar.setMaxValue(1.0);
                    bar.setDoubleValue(*fraction);
                    place(
                        Retained::into_super(bar.clone()),
                        LIST_PAD,
                        top + 2.0,
                        left,
                        BAR_HEIGHT,
                    );
                    top += BAR_HEIGHT + 4.0;
                    let text = label(mtm, detail, 11.0, &NSColor::secondaryLabelColor());
                    live.push((bar, text.clone()));
                    text
                }
                RowStatus::Waiting(detail) => {
                    label(mtm, detail, 11.0, &NSColor::secondaryLabelColor())
                }
                RowStatus::Done(detail) => label(mtm, detail, 11.0, &NSColor::systemGreenColor()),
            };
            place(
                Retained::into_super(Retained::into_super(detail)),
                LIST_PAD,
                top,
                left,
                LINE_HEIGHT,
            );
            top += LINE_HEIGHT;
            let dest = label(mtm, &row.dest, 11.0, &NSColor::tertiaryLabelColor());
            dest.setLineBreakMode(NSLineBreakMode::ByTruncatingMiddle);
            place(
                Retained::into_super(Retained::into_super(dest)),
                LIST_PAD,
                top,
                left,
                LINE_HEIGHT,
            );
            top += LINE_HEIGHT;
            if let Some(action) = row.status.action() {
                let title = match action {
                    RowAction::Cancel => ns_string!("Cancel"),
                    RowAction::Remove => ns_string!("Remove"),
                };
                // SAFETY: the selector is this class's `uploadRowAction:` and
                // takes a single `Option<&AnyObject>`; the target is this pane
                // and the pane keeps the popover alive (the target is weak).
                let button = unsafe {
                    NSButton::buttonWithTitle_target_action(
                        title,
                        Some(target),
                        Some(sel!(uploadRowAction:)),
                        mtm,
                    )
                };
                button.setControlSize(NSControlSize::Small);
                // audit: the id is a counter; it fits in `isize` (the item count is small).
                button.setTag(row.id as isize);
                button.sizeToFit();
                let size = button.frame().size;
                let width = size.width.max(ROW_BUTTON_WIDTH);
                // Vertically centred in the row.
                let middle = row_top + (top - row_top - size.height) / 2.0;
                place(
                    Retained::into_super(Retained::into_super(button)),
                    LIST_WIDTH - LIST_PAD - width,
                    middle,
                    width,
                    size.height,
                );
            }
            top += ROW_GAP;
        }

        let rule = NSBox::new(mtm);
        rule.setBoxType(NSBoxType::Separator);
        place(
            Retained::into_super(rule),
            LIST_PAD,
            top,
            LIST_WIDTH - 2.0 * LIST_PAD,
            1.0,
        );
        top += 1.0 + ROW_GAP;
        // SAFETY: the selector is this class's `cancelUpload:`; the target's
        // rationale is the one above.
        let all = unsafe {
            NSButton::buttonWithTitle_target_action(
                ns_string!("Cancel all ⌘."),
                Some(target),
                Some(sel!(cancelUpload:)),
                mtm,
            )
        };
        all.setControlSize(NSControlSize::Small);
        all.sizeToFit();
        let size = all.frame().size;
        place(
            Retained::into_super(Retained::into_super(all)),
            LIST_WIDTH - LIST_PAD - size.width,
            top,
            size.width,
            size.height,
        );
        top += size.height + LIST_PAD;

        // The view is not flipped: `top` measured from above becomes `y` from the bottom.
        let height = top;
        for (view, frame) in placed {
            view.setFrame(NSRect::new(
                NSPoint::new(frame.origin.x, height - frame.origin.y - frame.size.height),
                frame.size,
            ));
            content.addSubview(&view);
        }
        let size = NSSize::new(LIST_WIDTH, height);
        content.setFrameSize(size);
        (size, live)
    }
}

/// Esc's key code (independent of the ANSI layout, a hardware code).
pub(crate) const ESCAPE: u16 = 53;

/// The popover's width, inner padding, the gap between rows and the row
/// button's minimum width - **design constants**, the measures of the
/// approved demo (pt).
const LIST_WIDTH: f64 = 340.0;
const LIST_PAD: f64 = 12.0;
const ROW_GAP: f64 = 10.0;
const ROW_BUTTON_WIDTH: f64 = 70.0;
/// The rows' heights: the name (13 pt), the small lines (11-12 pt) and the bar.
const NAME_HEIGHT: f64 = 18.0;
const LINE_HEIGHT: f64 = 15.0;
const BAR_HEIGHT: f64 = 10.0;

/// The refreshed views of a flowing row: the bar and the detail.
type Live = (Retained<NSProgressIndicator>, Retained<NSTextField>);

/// The open "Show files (N)" popover and what it holds for refreshing.
pub(crate) struct UploadPopover {
    popover: Retained<NSPopover>,
    /// The items' ids and states: if they change the content is rebuilt.
    shape: Vec<(u64, u8)>,
    /// The flowing items' bar and detail, in row order.
    live: Vec<Live>,
    /// The Esc monitor (removed on close).
    monitor: Option<Retained<AnyObject>>,
}

/// The open stop question: the sheet, which item it was asked for, and the
/// Esc monitor.
pub(crate) struct StopSheet {
    alert: Retained<NSAlert>,
    id: u64,
    monitor: Option<Retained<AnyObject>>,
}

/// The list's shape: the items' ids and states (flowing 0, waiting 1, finished 2).
fn shape_of(list: &UploadList) -> Vec<(u64, u8)> {
    list.rows
        .iter()
        .map(|row| {
            let kind = match row.status {
                RowStatus::Running { .. } => 0,
                RowStatus::Waiting(_) => 1,
                RowStatus::Done(_) => 2,
            };
            (row.id, kind)
        })
        .collect()
}

/// A plain label: a single line, not selectable.
fn label(mtm: MainThreadMarker, text: &str, size: f64, color: &NSColor) -> Retained<NSTextField> {
    let field = NSTextField::labelWithString(&NSString::from_str(text), mtm);
    // SAFETY: `NSFontWeightRegular` is an AppKit constant global.
    let weight = unsafe { NSFontWeightRegular };
    field.setFont(Some(&NSFont::monospacedDigitSystemFontOfSize_weight(
        size, weight,
    )));
    field.setTextColor(Some(color));
    field
}

/// Installs a local key monitor: if `swallow` returns `true` the event is
/// swallowed. It runs on the main thread, before `NSApp.sendEvent:`.
pub(crate) fn add_key_monitor(
    swallow: impl Fn(&NSEvent) -> bool + 'static,
) -> Option<Retained<AnyObject>> {
    let block = RcBlock::new(move |event: NonNull<NSEvent>| -> *mut NSEvent {
        // SAFETY: AppKit gives the monitor a valid event.
        let event_ref = unsafe { event.as_ref() };
        if swallow(event_ref) {
            std::ptr::null_mut()
        } else {
            event.as_ptr()
        }
    });
    // SAFETY: the block returns a valid event pointer or null.
    unsafe { NSEvent::addLocalMonitorForEventsMatchingMask_handler(NSEventMask::KeyDown, &block) }
}

/// Removes [`add_key_monitor`]'s monitor.
pub(crate) fn remove_monitor(monitor: Option<Retained<AnyObject>>) {
    if let Some(monitor) = monitor {
        // SAFETY: the object is the monitor `addLocalMonitor…` returned and it
        // is removed once (its owner is taken from the slot).
        unsafe { NSEvent::removeMonitor(&monitor) };
    }
}

/// A macOS notification while bateri is in the background (037 phase-7): the
/// queue finished, failed or the connection dropped. None while in the
/// foreground - the result is in the dock and the title. The pane's request
/// goes through the owner ([`crate::pane::PaneHost::notify`]); today's owner
/// lands here (`window::WindowHost`).
///
/// `UNUserNotificationCenter` (a user-approved dependency, decision record
/// `.tasks/037-ssh-ikinci-tur/phase-7.md` → Uygulama Notları). Three rules:
///
/// - **Permission is requested at the first notification**, not at launch: a
///   user who has never finished an upload in the background never sees the
///   question. Requesting on every call is free - once the answer was given
///   the system returns the stored answer without asking. The notification is
///   built in the answer's completion block, so the first notification is not
///   lost; if denied only the notification is missing, the upload never waits.
/// - **No foreground presentation and no delegate is set:** a center that
///   does not implement `willPresentNotification` silences a notification
///   arriving in the foreground (the default Apple documents), so a
///   notification queued in the background and delivered after the user
///   returns also obeys the "none while in the foreground" rule.
/// - **Never called without a bundle identifier:** `currentNotificationCenter`
///   throws an exception in a bundle-less process (`cargo run`, tests, timed run).
pub(crate) fn notify(mtm: MainThreadMarker, title: &str, body: &str) {
    if NSApplication::sharedApplication(mtm).isActive() {
        return;
    }
    if NSBundle::mainBundle().bundleIdentifier().is_none() {
        return;
    }
    let (title, body) = (title.to_owned(), body.to_owned());
    // The block runs on a background queue: only owned strings are carried,
    // the center is fetched again and AppKit is not touched.
    let deliver = RcBlock::new(move |granted: Bool, _error: *mut NSError| {
        if !granted.as_bool() {
            return;
        }
        let content = UNMutableNotificationContent::new();
        content.setTitle(&NSString::from_str(&title));
        content.setBody(&NSString::from_str(&body));
        // Unique identifier: the same identifier would replace the previous one,
        // and results must stack up.
        let request = UNNotificationRequest::requestWithIdentifier_content_trigger(
            &NSUUID::new().UUIDString(),
            &content,
            None,
        );
        UNUserNotificationCenter::currentNotificationCenter()
            .addNotificationRequest_withCompletionHandler(&request, None);
    });
    UNUserNotificationCenter::currentNotificationCenter()
        .requestAuthorizationWithOptions_completionHandler(UNAuthorizationOptions::Alert, &deliver);
}

/// The stream thread's progress report: at most one on the main queue.
fn tick(lookup: PaneLookup, id: u64, shared: &Arc<Shared>) {
    if shared.tick_pending.swap(true, Ordering::AcqRel) {
        return;
    }
    let shared = Arc::clone(shared);
    on_pane(lookup, id, move |pane| {
        shared.tick_pending.store(false, Ordering::Release);
        pane.upload_refresh();
    });
}

/// The progress of all panes' uploads on the application's Dock tile (Kullanıcı
/// kararı 4); the owner walks the panes (`AppDelegate::refresh_dock_tile`).
/// When there is no upload the tile returns to its own state. It is not
/// touched at all until the first upload - launch (and the timed run) does
/// not visit the Dock tile.
pub(crate) fn refresh_dock_tile(mtm: MainThreadMarker, panes: &[&TerminalPane]) {
    let (sent, total) = panes
        .iter()
        .filter_map(|pane| pane.upload_totals())
        .fold((0u64, 0u64), |(a, b), (sent, total)| (a + sent, b + total));
    let tile = NSApplication::sharedApplication(mtm).dockTile();
    let active = panes.iter().any(|pane| pane.upload_active());
    if !active {
        if tile.contentView(mtm).is_some() {
            tile.setContentView(None);
            tile.display();
        }
        return;
    }
    let fraction = if total == 0 {
        0.0
    } else {
        sent.min(total) as f64 / total as f64
    };
    let view = match tile.contentView(mtm) {
        Some(view) => view,
        None => {
            let view = dock_tile_view(mtm);
            tile.setContentView(Some(&view));
            view
        }
    };
    if let Some(bar) = view
        .subviews()
        .iter()
        .find_map(|sub| sub.downcast::<NSProgressIndicator>().ok())
    {
        bar.setDoubleValue(fraction);
    }
    tile.display();
}

/// The Dock tile's content: the application's icon and a bar beneath it.
fn dock_tile_view(mtm: MainThreadMarker) -> Retained<NSView> {
    const SIDE: f64 = 128.0;
    let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(SIDE, SIDE));
    let view = NSView::initWithFrame(NSView::alloc(mtm), frame);
    let icon = NSImageView::initWithFrame(NSImageView::alloc(mtm), frame);
    if let Some(image) = NSApplication::sharedApplication(mtm).applicationIconImage() {
        icon.setImage(Some(&image));
    }
    icon.setImageScaling(NSImageScaling::ScaleProportionallyUpOrDown);
    view.addSubview(&icon);
    let bar = NSProgressIndicator::initWithFrame(
        NSProgressIndicator::alloc(mtm),
        NSRect::new(NSPoint::new(12.0, 6.0), NSSize::new(SIDE - 24.0, 20.0)),
    );
    bar.setStyle(NSProgressIndicatorStyle::Bar);
    bar.setIndeterminate(false);
    bar.setMinValue(0.0);
    bar.setMaxValue(1.0);
    view.addSubview(&bar);
    view
}
