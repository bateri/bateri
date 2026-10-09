//! The **AppKit half** of the transfer queue (both directions): the AppKit and dispatch work that
//! goes from the drop to the confirmation sheet, from the sheet to the stream
//! (an upload's or a download's — the latter quarantined before it lands), from
//! the stream to the dock's status line, to the "Show transfers (N)" popover
//! and to the stop question. The queue
//! is **the pane's**: its sheets sit where the pane's questions do
//! ([`crate::sheets`]), the popover on the pane's `BateriView`; the title's `↑ N%` prefix, the
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
use std::path::Path;
use std::ptr::NonNull;
use std::sync::atomic::Ordering;
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use block2::RcBlock;
use bt_core::{HostMark, Transfer};
use dispatch2::{DispatchQueue, DispatchTime};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool, ProtocolObject};
use objc2::{MainThreadMarker, MainThreadOnly, sel};
use objc2_app_kit::{
    NSAlert, NSAlertFirstButtonReturn, NSAlertSecondButtonReturn, NSApplication, NSBox, NSBoxType,
    NSButton, NSColor, NSControlSize, NSEvent, NSEventMask, NSFont, NSFontWeightRegular,
    NSImageScaling, NSImageView, NSLineBreakMode, NSModalResponse, NSModalResponseAbort, NSPopover,
    NSPopoverBehavior, NSProgressIndicator, NSProgressIndicatorStyle, NSTextField, NSView,
    NSViewController, NSWorkspace,
};
use objc2_foundation::{
    NSArray, NSBundle, NSDictionary, NSError, NSPoint, NSRect, NSRectEdge, NSSize, NSString, NSURL,
    NSURLQuarantinePropertiesKey, NSUUID, ns_string,
};
use objc2_user_notifications::{
    UNAuthorizationOptions, UNMutableNotificationContent, UNNotificationRequest,
    UNNotificationTrigger, UNTimeIntervalNotificationTrigger, UNUserNotificationCenter,
};

use crate::child;
use crate::download::{self, Conflict};
use crate::pane::{PaneLookup, TerminalPane};
use crate::password_sheet::Job as SheetJob;
use crate::preview::beep;
use crate::preview_cache;
use crate::remote_files::{self, RemoteEntry};
use crate::remote_helper::{Answer, Query, Request};
use crate::sheets::{self, Asker};
use crate::ssh_route;
use crate::upload::{
    self, Direction, Ended, Job, Lane, Local, Outcome, ProbeReply, RowAction, RowStatus, Shared,
    Started, Stop, StopQuestion, TransferList, Way,
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

/// The confirmed drop (or download, or preview): the items that will enter the
/// queue.
pub(crate) struct Confirmed {
    pub(crate) command: u64,
    pub(crate) ssh: Vec<String>,
    pub(crate) host: String,
    pub(crate) mark: HostMark,
    pub(crate) jobs: Vec<Job>,
}

/// A download's question answered: what the remote item is and what
/// its destination holds — everything the sheet decides on, gathered on the
/// helper's thread (the remote count, then the local folder: a network volume
/// must not stall the main thread).
struct Prepared {
    command: u64,
    ssh: Vec<String>,
    host: String,
    mark: HostMark,
    /// The remote absolute path.
    remote: String,
    /// `Err` → the error sheet's text.
    result: Result<Landing, String>,
}

/// Where a download lands and what is there.
struct Landing {
    entry: RemoteEntry,
    /// The destination folder (it may not exist yet).
    folder: std::path::PathBuf,
    free: Option<u64>,
    /// Whether the remote name is already taken in `folder`.
    clash: bool,
}

/// Finds the pane with `id` on the main thread and applies `work` to it.
pub(crate) fn on_pane(
    lookup: PaneLookup,
    id: u64,
    work: impl FnOnce(&TerminalPane) + Send + 'static,
) {
    DispatchQueue::main().exec_async(move || {
        // audit: a block running on the main queue is by definition on the main thread.
        let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
        if let Some(pane) = lookup(mtm, id) {
            work(&pane);
        }
    });
}

impl TerminalPane {
    /// A Finder drop in a remote session: local measurement and
    /// remote probe in the background, then the confirmation sheet. `false` →
    /// the drop was refused (local session, or another sheet is in progress -
    /// two sheets cannot open on top of each other).
    pub(crate) fn upload_drop(&self, paths: Vec<String>) -> bool {
        let Some(session) = self.session() else {
            return false;
        };
        let Some((command, target, _)) = session.remote_target() else {
            return false;
        };
        // OSC 7's directory, or the title's when the server sends none.
        let cwd = session.remote_link_directory();
        if !self.accepts_drop() {
            return false;
        }
        let mark = session
            .remote_mark()
            .map_or(HostMark::None, |(_, mark)| mark);
        let host = target.host.clone();
        // The job holds the sheet gate from here: the password sheet opens within it.
        let dial = self.dial(target, Some((SheetJob::Upload, true)));
        let reported = !cwd.is_empty();
        let (id, lookup) = (self.id(), self.lookup());
        self.uploads().borrow_mut().set_asking(true);
        let spawned = thread::Builder::new()
            .name("upload probe".into())
            .spawn(move || {
                let dir = reported.then_some(cwd.as_str());
                // The route first: the probe and the stream ride it.
                let (ssh, result) = match (dial.argv)() {
                    Ok(ssh) => {
                        let result = upload::probe(&ssh, &host, dir, &paths);
                        (ssh, result)
                    }
                    Err(text) => (Vec::new(), Err(text)),
                };
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
        // The sheet's seat is the pane's ([`sheets`]); if the pane was
        // detached from the window there is nowhere to ask either.
        let Some(seat) = alive.then(|| sheets::seat(Asker::Pane(self))).flatten() else {
            self.uploads().borrow_mut().set_asking(false);
            return;
        };
        // The password sheet was cancelled: nothing more to say.
        if asked
            .result
            .as_ref()
            .is_err_and(|text| text == ssh_route::CANCELLED)
        {
            self.uploads().borrow_mut().set_asking(false);
            return;
        }
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
                            way: Way::Up,
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
            if let Some(confirmed) = confirmed.borrow_mut().take()
                && !pane.upload_confirmed(confirmed)
            {
                beep();
            }
        });
        self.upload_alert().replace(Some(alert.clone()));
        seat.begin(&alert, &answered);
    }

    /// Confirmation (an upload's, a download's or a preview's): the items go to
    /// the end of the queue, the first starts if the queue is idle — a lingering
    /// result line ends with it. `false` if they could not enter:
    /// the remote session ended, or the queue is still stopping (its half-written
    /// file is being deleted); the caller beeps, an action is never dropped in
    /// silence.
    pub(crate) fn upload_confirmed(&self, confirmed: Confirmed) -> bool {
        let alive = self
            .session()
            .and_then(|session| session.remote_target())
            .is_some_and(|(command, ..)| command == confirmed.command);
        if !alive {
            return false;
        }
        let queued = self.uploads().borrow_mut().enqueue(
            confirmed.command,
            confirmed.ssh,
            confirmed.host,
            confirmed.mark,
            confirmed.jobs,
        );
        if queued {
            let _ = self.start_transfers();
            // If the queue was already flowing, the new items are in the list and the count.
            self.upload_refresh();
        }
        queued
    }

    /// A remote link's "Download to Downloads" (`folder` `None`: `[remote]
    /// download_dir`) or "Download To…": the helper counts the item
    /// and the destination is looked at on its thread, then
    /// [`TerminalPane::download_prepared`] asks or starts. Refused with a beep
    /// while another sheet is in progress (two sheets cannot open on top of each
    /// other) or the remote session ended.
    pub(crate) fn download_remote(&self, remote: String, folder: Option<std::path::PathBuf>) {
        let Some(session) = self.session() else {
            beep();
            return;
        };
        let Some((command, target, _)) = session.remote_target() else {
            beep();
            return;
        };
        if !self.accepts_drop() {
            // Another sheet is open (two cannot stack) or the queue is still
            // stopping: refused, but audibly.
            beep();
            return;
        }
        let mark = session
            .remote_mark()
            .map_or(HostMark::None, |(_, mark)| mark);
        let host = target.host.clone();
        // The job holds the sheet gate from here: the password sheet opens within it.
        let dial = self.dial(target, Some((SheetJob::Download, true)));
        let download_dir = self.remote_files().borrow().download_dir.clone();
        let (id, lookup) = (self.id(), self.lookup());
        self.uploads().borrow_mut().set_asking(true);
        let request = Request {
            command,
            dial,
            host: host.clone(),
            query: Query::Count(remote.clone()),
            reply: Box::new(move |answer, ssh| {
                let ssh = ssh.to_vec();
                let result = match answer {
                    Ok(Answer::Counted(Some(entry))) => {
                        landing(entry, &remote, folder, &download_dir)
                    }
                    Ok(_) => Err(format!("{remote} no longer exists on {host}.")),
                    Err(text) => Err(text),
                };
                let prepared = Prepared {
                    command,
                    ssh,
                    host,
                    mark,
                    remote,
                    result,
                };
                on_pane(lookup, id, move |pane| pane.download_prepared(prepared));
            }),
        };
        self.remote_helper().borrow_mut().ask(request);
    }

    /// The download's question came back: straight into the queue, or the
    /// confirmation sheet first ([`remote_files::download_sheet`]), or the error
    /// sheet.
    fn download_prepared(&self, prepared: Prepared) {
        let alive = self
            .session()
            .and_then(|session| session.remote_target())
            .is_some_and(|(command, ..)| command == prepared.command);
        let Some(seat) = alive.then(|| sheets::seat(Asker::Pane(self))).flatten() else {
            self.uploads().borrow_mut().set_asking(false);
            beep();
            return;
        };
        let Prepared {
            command,
            ssh,
            host,
            mark,
            remote,
            result,
        } = prepared;
        // The password sheet was cancelled: nothing more to say.
        if result
            .as_ref()
            .is_err_and(|text| text == ssh_route::CANCELLED)
        {
            self.uploads().borrow_mut().set_asking(false);
            return;
        }
        let name = remote_files::split_remote(&remote)
            .map_or_else(String::new, |(_, name)| name.to_owned());
        let mtm = self.mtm();
        let alert = NSAlert::new(mtm);
        // The confirm buttons' conflict rules, in order; empty for the error sheet.
        let (buttons, landing) = match result {
            Err(text) => {
                alert.setMessageText(&NSString::from_str(&format!("Can't download from {host}")));
                alert.setInformativeText(&NSString::from_str(&text));
                alert.addButtonWithTitle(ns_string!("OK"));
                (Vec::new(), None)
            }
            Ok(landing) => {
                let setting = self.remote_files().borrow().download_conflict;
                let dest = landing.folder.display().to_string();
                match remote_files::download_sheet(
                    &host,
                    &name,
                    &landing.entry,
                    &dest,
                    landing.free,
                    landing.clash,
                    setting,
                ) {
                    Ok(conflict) => {
                        self.uploads().borrow_mut().set_asking(false);
                        self.download_confirmed(
                            command, ssh, host, mark, &remote, &landing, conflict,
                        );
                        return;
                    }
                    Err(sheet) => {
                        alert.setMessageText(&NSString::from_str(&sheet.message));
                        alert.setInformativeText(&NSString::from_str(&sheet.informative));
                        for (title, _) in &sheet.buttons {
                            let button = alert.addButtonWithTitle(&NSString::from_str(title));
                            button.setEnabled(sheet.enabled);
                        }
                        let cancel = alert.addButtonWithTitle(ns_string!("Cancel"));
                        // Esc by hand (the rationale of `window::alert`).
                        cancel.setKeyEquivalent(ns_string!("\u{1b}"));
                        let rules: Vec<Conflict> =
                            sheet.buttons.iter().map(|(_, rule)| *rule).collect();
                        (rules, Some(landing))
                    }
                }
            }
        };
        let (id, lookup) = (self.id(), self.lookup());
        // The block is `Fn`: the payload is taken once.
        let payload = RefCell::new(landing.map(|landing| (ssh, host, mark, remote, landing)));
        let answered = RcBlock::new(move |response: NSModalResponse| {
            // audit: the sheet's completion block runs on AppKit's main thread.
            let mtm = MainThreadMarker::new().expect("the sheet block is on the main thread");
            let Some(pane) = lookup(mtm, id) else {
                return;
            };
            drop(pane.upload_alert().take());
            pane.uploads().borrow_mut().set_asking(false);
            let chosen = if response == NSAlertFirstButtonReturn {
                buttons.first()
            } else if response == NSAlertSecondButtonReturn {
                buttons.get(1)
            } else {
                None
            };
            if let (Some(&conflict), Some((ssh, host, mark, remote, landing))) =
                (chosen, payload.borrow_mut().take())
            {
                pane.download_confirmed(command, ssh, host, mark, &remote, &landing, conflict);
            }
        });
        self.upload_alert().replace(Some(alert.clone()));
        seat.begin(&alert, &answered);
    }

    /// The download goes to the queue's end under `conflict` (a
    /// right-click download waits its turn like an upload).
    #[allow(clippy::too_many_arguments)]
    fn download_confirmed(
        &self,
        command: u64,
        ssh: Vec<String>,
        host: String,
        mark: HostMark,
        remote: &str,
        landing: &Landing,
        conflict: Conflict,
    ) {
        let (files, bytes) = match landing.entry {
            RemoteEntry::File { size, .. } => (1, size.unwrap_or(0)),
            RemoteEntry::Dir(size) => size.map_or((0, 0), |size| (size.files, size.bytes)),
        };
        let job = remote_files::split_remote(remote).and_then(|(_, name)| {
            Job::download(
                remote,
                landing.folder.join(name),
                matches!(landing.entry, RemoteEntry::Dir(_)),
                files,
                bytes,
                Lane::Queue,
                conflict,
            )
        });
        let queued = job.is_some_and(|job| {
            self.upload_confirmed(Confirmed {
                command,
                ssh,
                host,
                mark,
                jobs: vec![job],
            })
        });
        if !queued {
            beep();
        }
    }

    /// Starts every item that may start on its own background thread: the
    /// queue's next item (no-op while one flows) and every preview and Finder
    /// item, which never wait.
    /// Whether anything started (the line was refreshed then).
    fn start_transfers(&self) -> bool {
        let started = self.uploads().borrow_mut().start(Instant::now());
        if started.is_empty() {
            return false;
        }
        self.upload_refresh();
        for started in started {
            self.spawn_transfer(started);
        }
        true
    }

    /// One item's stream on a background thread: an upload's, or a download's
    /// that is quarantined while still hidden and then lands
    /// ([`download::transfer`]).
    fn spawn_transfer(&self, started: Started) {
        let Started {
            id: item,
            ssh,
            job,
            shared,
        } = started;
        // A preview's ticket: sealed on this thread when it lands,
        // opened on the main thread after its row is updated.
        let ticket = match job.way {
            Way::Down {
                lane: Lane::Preview,
                ..
            } => self.previews().borrow().get(job.landing()).cloned(),
            _ => None,
        };
        // A Finder drop's promise follows this item from now on.
        if let Way::Down {
            lane: Lane::Finder, ..
        } = job.way
        {
            self.finder_started(job.landing(), item, &shared);
        }
        let (id, lookup) = (self.id(), self.lookup());
        let spawned = thread::Builder::new().name("transfer".into()).spawn({
            let shared = Arc::clone(&shared);
            move || {
                let progress = || tick(lookup, id, &shared);
                let (outcome, landed) = match job.way {
                    Way::Up => (
                        upload::transfer(&ssh, &job.local, &job.dir, &shared, progress),
                        None,
                    ),
                    Way::Down { conflict, .. } => download::transfer(
                        &ssh,
                        &job.remote_path(),
                        job.landing(),
                        conflict,
                        &shared,
                        progress,
                        quarantine,
                    ),
                };
                let open = match (&outcome, &landed, ticket) {
                    (Outcome::Done, Some(landed), Some(ticket)) => {
                        preview_cache::seal(
                            &ticket.dir,
                            landed,
                            ticket.read_only,
                            preview_cache::now(),
                        );
                        Some(landed.clone())
                    }
                    _ => None,
                };
                on_pane(lookup, id, move |pane| {
                    pane.upload_finished(item, outcome, landed);
                    if let Some(path) = open {
                        pane.open_preview(&path);
                    }
                });
            }
        });
        if let Err(error) = spawned {
            self.upload_finished(item, Outcome::Failed(error.to_string()), None);
        }
    }

    /// Progress report: refreshes the status line, the popover, the title and
    /// the Dock tile.
    fn upload_refresh(&self) {
        self.finder_tick();
        let status = self.uploads().borrow_mut().status(Instant::now());
        if let Some(status) = status {
            self.show_transfer(Some(status));
            self.rehover_footer();
        }
        self.refresh_upload_list();
        self.refresh_upload_title();
        self.uploads_changed();
    }

    /// The owner's Dock tile (the total over all panes) should refresh — and a
    /// postponed update may go.
    fn uploads_changed(&self) {
        self.host().uploads_changed(self.id());
    }

    /// The queue's items not finished yet — what an update waits for.
    pub(crate) fn upload_unfinished(&self) -> usize {
        self.uploads().borrow().unfinished()
    }

    /// An update waits for `left` transfers of the application (`None`: it
    /// does not wait any more): the line leads with it, redrawn at once if
    /// this pane streams.
    pub(crate) fn set_update_waits(&self, left: Option<usize>) {
        let changed = self.uploads().borrow_mut().set_update_waits(left);
        if !changed {
            return;
        }
        let status = self.uploads().borrow_mut().status(Instant::now());
        if let Some(status) = status {
            self.show_transfer(Some(status));
        }
    }

    /// The flowing item `item` ended (`landed`: where a download landed): move
    /// on to the next or show the result. No path is pasted: the
    /// result line says where it went.
    fn upload_finished(&self, item: u64, outcome: Outcome, landed: Option<std::path::PathBuf>) {
        // A Finder drop's promise is kept or failed first: when a stop already
        // ended the queue, `finish_item` knows nothing of the item any more.
        self.finder_finished(item, &outcome);
        let ended = self
            .uploads()
            .borrow_mut()
            .finish_item(item, outcome, landed);
        // If the stop question was about the ended item, the sheet closes by itself.
        self.dismiss_stale_stop();
        match ended {
            Some(ended) => self.show_end(ended),
            // A lane item ended while others flow: its row and the line change.
            None if !self.start_transfers() => self.upload_refresh(),
            None => {}
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
        // `[remote] download_notify`: a transfer ending in the
        // background notifies only if the user wants it — either direction.
        if let Some((title, body)) = ended.notice
            && self.remote_files().borrow().download_notify
        {
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

    /// The title's `↑ N% · ` prefix: if the percentage changed
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
        // An upload row takes the load indicator's place: its popover closes
        // and the hand cursor's rects are refreshed there.
        self.stats_gauge_changed();
    }

    /// Stop request: ⌘., the line's `Cancel`/`Cancel all` and
    /// the popover's `Cancel all` are `all`, the popover row's `Cancel` is
    /// not. If the flowing item has been flowing longer than
    /// [`upload::STOP_ASK_AFTER`] it asks first; while an upload sheet is open
    /// (confirmation or question) the request is dropped - two sheets cannot
    /// open on top of each other.
    pub(crate) fn request_stop(&self, all: bool) {
        self.request_stop_item(None, all);
    }

    /// [`Self::request_stop`] for the flowing item `id` (a popover row's
    /// `Cancel`; `None` → the queue's item).
    fn request_stop_item(&self, id: Option<u64>, all: bool) {
        if self.uploads().borrow().asking()
            || self.upload_alert().borrow().is_some()
            || self.upload_stop().borrow().is_some()
        {
            return;
        }
        let request = self
            .uploads()
            .borrow()
            .stop_request_item(id, all, Instant::now());
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
    pub(crate) fn apply_stop(&self, id: Option<u64>, all: bool) {
        let ended = self.uploads().borrow_mut().stop(id, all);
        match ended {
            Some(ended) => self.show_end(ended),
            None => self.upload_refresh(),
        }
        self.uploads_changed();
    }

    /// The "Stop uploading?" sheet (or "Stop downloading?" and their kin, in
    /// the question's direction): `Keep uploading` is the default (Return) and
    /// Esc, `Stop` is destructive. The upload keeps running while the sheet
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
        let keep = self.uploads().borrow().keep_label(&question);
        alert.addButtonWithTitle(&NSString::from_str(keep));
        let stop = alert.addButtonWithTitle(ns_string!("Stop"));
        stop.setHasDestructiveAction(true);
        let Some(seat) = sheets::seat(Asker::Pane(self)) else {
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
            // certain once it is shown.
            let on_sheet = event
                .window(mtm)
                .is_some_and(|window| Retained::as_ptr(&window) == Retained::as_ptr(&sheet_window));
            if !on_sheet {
                return false;
            }
            if let Some(seat) = lookup(mtm, id).and_then(|pane| sheets::seat(Asker::Pane(&pane))) {
                seat.end(&sheet_window, NSAlertFirstButtonReturn);
            }
            true
        });
        self.upload_stop().replace(Some(StopSheet {
            alert: alert.clone(),
            id: item,
            monitor,
        }));
        seat.begin(&alert, &answered);
    }

    /// If the stop question was about an item that no longer flows, closes the
    /// sheet (the answer counts as "never mind"): the question had named that item's loss.
    fn dismiss_stale_stop(&self) {
        let stale = self
            .upload_stop()
            .borrow()
            .as_ref()
            .filter(|sheet| !self.uploads().borrow().is_running(sheet.id))
            .map(|sheet| sheet.alert.window());
        if let (Some(sheet_window), Some(seat)) = (stale, sheets::seat(Asker::Pane(self))) {
            seat.end(&sheet_window, NSModalResponseAbort);
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

    /// A popover row's button (`tag` is the item's id): `Cancel` on a flowing
    /// item (asks first if past 30 s), `Remove` on a waiting one (does not
    /// ask), `Show in Finder` on a finished download, `Open` on a finished
    /// preview.
    pub(crate) fn upload_row_action(&self, id: u64) {
        let action = self.uploads().borrow().list().and_then(|list| {
            list.rows
                .into_iter()
                .find(|row| row.id == id)
                .and_then(|row| row.action)
        });
        let landed = || self.uploads().borrow().landed(id);
        match action {
            Some(RowAction::Cancel) => self.request_stop_item(Some(id), false),
            Some(RowAction::Remove) => {
                self.uploads().borrow_mut().remove(id);
                self.upload_refresh();
            }
            Some(RowAction::ShowInFinder) => {
                if let Some(path) = landed() {
                    NSWorkspace::sharedWorkspace().activateFileViewerSelectingURLs(
                        &NSArray::from_retained_slice(&[file_url(&path)]),
                    );
                }
            }
            // Through the preview's open policy: a script opens as text.
            Some(RowAction::Open) => {
                if let Some(path) = landed() {
                    self.open_preview(&path);
                }
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

    /// "Show transfers (N)": the queue's popover - attached to the
    /// button, `transient`: a click outside, Esc or pressing the button again closes it.
    ///
    /// **Pressing the button again**: a `transient` popover closes itself on a
    /// press outside and that same press reaches here too - so as not to
    /// reopen a popover that looks open, the time of the event that triggered
    /// the close is stored (`popoverWillClose:`) and nothing is done for that event.
    pub(crate) fn toggle_upload_list(&self, span: Option<(u16, u16)>) {
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
        self.rehover_footer();
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
    fn fill_list_view(&self, content: &NSView, list: &TransferList) -> (NSSize, Vec<Live>) {
        let mtm = self.mtm();
        let target: &AnyObject = self.as_ref();
        // The buttons first: the widest one ("Show in Finder") sets the text column.
        let buttons: Vec<Option<Retained<NSButton>>> = list
            .rows
            .iter()
            .map(|row| {
                let action = row.action?;
                let title = match action {
                    RowAction::Cancel => ns_string!("Cancel"),
                    RowAction::Remove => ns_string!("Remove"),
                    RowAction::ShowInFinder => ns_string!("Show in Finder"),
                    RowAction::Open => ns_string!("Open"),
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
                Some(button)
            })
            .collect();
        let button_width = buttons
            .iter()
            .flatten()
            .map(|button| button.frame().size.width)
            .fold(ROW_BUTTON_WIDTH, f64::max);
        let left = LIST_WIDTH - 2.0 * LIST_PAD - button_width - LIST_PAD;
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

        for (row, button) in list.rows.iter().zip(buttons) {
            let row_top = top;
            // The row's arrow: ↑ to the server, ↓ to this Mac.
            let arrow = match row.direction {
                Direction::Up => "↑",
                Direction::Down => "↓",
            };
            let name = label(
                mtm,
                &format!("{arrow} {}", row.name),
                13.0,
                &NSColor::labelColor(),
            );
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
            if let Some(button) = button {
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

/// The open "Show transfers (N)" popover and what it holds for refreshing.
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
fn shape_of(list: &TransferList) -> Vec<(u64, u8)> {
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
pub(crate) fn label(
    mtm: MainThreadMarker,
    text: &str,
    size: f64,
    color: &NSColor,
) -> Retained<NSTextField> {
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

/// A macOS notification while bateri is in the background: the
/// queue finished, failed or the connection dropped. None while in the
/// foreground - the result is in the dock and the title. The pane's request
/// goes through the owner ([`crate::pane::PaneHost::notify`]); today's owner
/// lands here (`tab::TabHost`).
///
/// `UNUserNotificationCenter` (a user-approved dependency). Three rules:
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
    deliver_notification(title, body);
}

/// [`notify`] without the background gate: the caller decided a notification is
/// the right channel (an edited preview kept while no window can show a
/// sheet). Never in an unbundled process.
pub(crate) fn deliver_notification(title: &str, body: &str) {
    if NSBundle::mainBundle().bundleIdentifier().is_none() {
        return;
    }
    let (title, body) = (title.to_owned(), body.to_owned());
    // The block runs on a background queue: only owned strings are carried,
    // the center is fetched again and AppKit is not touched.
    let deliver = RcBlock::new(move |granted: Bool, _error: *mut NSError| {
        if granted.as_bool() {
            add_notification(&title, &body, None, None);
        }
    });
    UNUserNotificationCenter::currentNotificationCenter()
        .requestAuthorizationWithOptions_completionHandler(UNAuthorizationOptions::Alert, &deliver);
}

/// Asks for the permission to notify, and nothing else: the system asks the
/// user once and answers from its record afterwards. For a notification
/// whose moment cannot ask ([`schedule_notification`]): the question comes
/// when the user chose the setting that needs it, while bateri is in front.
/// Never in an unbundled process.
pub(crate) fn request_notification_permission() {
    if NSBundle::mainBundle().bundleIdentifier().is_none() {
        return;
    }
    let ignore = RcBlock::new(|_granted: Bool, _error: *mut NSError| {});
    UNUserNotificationCenter::currentNotificationCenter()
        .requestAuthorizationWithOptions_completionHandler(UNAuthorizationOptions::Alert, &ignore);
}

/// A notification shown `after` from now — after bateri has quit: a
/// notification delivered while the app is in front is silenced (no
/// delegate, [`notify`]), and one that waits on a timer is delivered by the
/// system once the process is gone.
///
/// **No permission call**: this runs while quitting, where a question must not
/// appear; the request is only added, and without permission the system
/// drops it in the completion (its error, said on stderr). No answer is cached
/// in the process either — a permission given later in System Settings works.
///
/// The receiver gets one message once the system took the request or refused
/// it; the caller waits on it, bounded, so the request leaves the process
/// before it exits. `None` in an unbundled process.
pub(crate) fn schedule_notification(
    title: &str,
    body: &str,
    after: Duration,
) -> Option<mpsc::Receiver<()>> {
    // An unbundled process has no center (it throws).
    NSBundle::mainBundle().bundleIdentifier()?;
    let trigger = UNTimeIntervalNotificationTrigger::triggerWithTimeInterval_repeats(
        after.as_secs_f64(),
        false,
    );
    let (done, taken) = mpsc::channel();
    add_notification(title, body, Some(&trigger), Some(done));
    Some(taken)
}

/// The one way a notification is added: its content, a unique identifier (the
/// same identifier would replace the previous one, and results must stack
/// up), the trigger (`None` — at once) and, if asked, the completion's
/// message. Any thread: the center is thread-safe and AppKit is not touched.
fn add_notification(
    title: &str,
    body: &str,
    trigger: Option<&UNNotificationTrigger>,
    done: Option<mpsc::Sender<()>>,
) {
    let content = UNMutableNotificationContent::new();
    content.setTitle(&NSString::from_str(title));
    content.setBody(&NSString::from_str(body));
    let request = UNNotificationRequest::requestWithIdentifier_content_trigger(
        &NSUUID::new().UUIDString(),
        &content,
        trigger,
    );
    let center = UNUserNotificationCenter::currentNotificationCenter();
    let Some(done) = done else {
        center.addNotificationRequest_withCompletionHandler(&request, None);
        return;
    };
    // The block runs on a background queue; it carries only the sender.
    let finished = RcBlock::new(move |error: *mut NSError| {
        // SAFETY: the completion's argument is null or an `NSError` that lives
        // for the call (UserNotifications' contract); it is only read here.
        if let Some(error) = unsafe { error.as_ref() } {
            eprintln!(
                "bateri: the notification was not added: {}",
                error.localizedDescription()
            );
        }
        let _ = done.send(());
    });
    center.addNotificationRequest_withCompletionHandler(&request, Some(&finished));
}

/// A file URL for a local path.
fn file_url(path: &Path) -> Retained<NSURL> {
    NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()))
}

/// The quarantine mark of a downloaded item: Gatekeeper asks
/// before a downloaded program first runs, as for a browser's download. A
/// folder's every entry is marked too (a browser's unpacked archive is). The
/// download's `seal` hook: it runs on the stream thread while the item is still
/// in its hidden temporary folder, so the item never has its name unmarked.
/// Foundation's resource values are thread-safe. A failure leaves the item
/// unmarked and is not an error: the bytes arrived.
fn quarantine(path: &Path) {
    let properties = NSDictionary::<NSString, NSString>::from_slices(
        &[
            ns_string!("LSQuarantineType"),
            ns_string!("LSQuarantineAgentName"),
        ],
        &[
            ns_string!("LSQuarantineTypeOtherDownload"),
            ns_string!("bateri"),
        ],
    );
    let mark = |path: &Path| {
        // SAFETY: `NSURLQuarantinePropertiesKey` is a Foundation constant; its
        // value is a dictionary of LaunchServices' quarantine keys with string
        // values — the type the key documents.
        let _ = unsafe {
            file_url(path)
                .setResourceValue_forKey_error(Some(&properties), NSURLQuarantinePropertiesKey)
        };
    };
    let mut pending = vec![path.to_owned()];
    while let Some(path) = pending.pop() {
        mark(&path);
        if std::fs::symlink_metadata(&path).is_ok_and(|meta| meta.is_dir())
            && let Ok(entries) = std::fs::read_dir(&path)
        {
            pending.extend(entries.flatten().map(|entry| entry.path()));
        }
    }
}

/// The download's destination on the helper's thread: `folder` or the expanded
/// `download_dir`, its free space and whether the remote name is already taken
/// there. Nothing is created here — a missing folder is made when the stream
/// starts (`download::transfer`), after the user confirmed.
fn landing(
    entry: RemoteEntry,
    remote: &str,
    folder: Option<std::path::PathBuf>,
    download_dir: &str,
) -> Result<Landing, String> {
    let folder = folder
        .or_else(|| bt_core::expand_home(download_dir, child::home().as_deref()))
        .ok_or_else(|| format!("The download folder {download_dir} can't be found."))?;
    let Some((_, name)) = remote_files::split_remote(remote) else {
        return Err(format!("{remote} can't be downloaded."));
    };
    Ok(Landing {
        entry,
        clash: folder.join(name).symlink_metadata().is_ok(),
        free: download::free_space(&folder),
        folder,
    })
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

/// The progress of all panes' uploads on the application's Dock tile (the
/// user's decision); the owner walks the panes (`AppDelegate::refresh_dock_tile`).
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
