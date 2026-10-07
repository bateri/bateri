//! The **AppKit half** of the remote preview: a
//! ⌘-click (or the menu's "Open Preview") on a remote file downloads a
//! temporary, read-only copy into the preview folder and opens it.
//!
//! The flow, one hop per thread:
//!
//! 1. **Main** ([`TerminalPane::preview_remote`]): the copy's place
//!    (`remote_files::preview_path`) and a fresh question to the pane's helper
//!    session (`Query::Count` — the hover's answer may be cached, and the cache compares
//!    the remote size and mtime **now**).
//! 2. **The helper's worker**: the cache's answer (`preview_cache::state`): an
//!    unchanged copy opens as it is (its age restarts), a changed remote file
//!    downloads, a copy the user edited is moved to the download folder first —
//!    a re-download never destroys it.
//! 3. **Main** ([`TerminalPane::preview_decided`]): the open policy
//!    (`remote_files::preview_open`: scripts, programs and unknown types as plain
//!    text), the size limit's question, and the download into the
//!    transfer queue's preview lane, which never waits.
//! 4. **The stream thread** (`uploader`'s `spawn_transfer`): the landed copy is
//!    sealed — `0444` if `preview_read_only`, recorded in the index
//!    (`preview_cache::seal`) — and the main thread opens it.
//!
//! A folder does nothing. A refused action is never silent: a beep, or
//! a sheet that says why.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::thread;

use block2::RcBlock;
use bt_core::HostMark;
use dispatch2::DispatchQueue;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject};
use objc2::{MainThreadMarker, MainThreadOnly, msg_send};
use objc2_app_kit::{
    NSAlert, NSAlertFirstButtonReturn, NSAlertSecondButtonReturn, NSAlertThirdButtonReturn,
    NSApplication, NSModalResponse, NSWorkspace,
};
use objc2_foundation::{NSArray, NSError, NSString, NSURL, ns_string};

use crate::child;
use crate::download::Conflict;
use crate::hyperlink::extension_content;
use crate::links::Content;
use crate::pane::TerminalPane;
use crate::password_sheet::Job as SheetJob;
use crate::preview_cache;
use crate::remote_files::{self, CacheState, PreviewOpen, RemoteEntry};
use crate::remote_helper::{Answer, Query, Request};
use crate::sheets::{self, Asker, Seat};
use crate::upload::{Job, Lane, format_bytes};
use crate::uploader::{self, Confirmed, on_pane};

/// How a preview in flight finishes: kept by the pane by its landing path (the
/// preview lane replaces, so the copy lands exactly there) until the stream
/// thread seals it and the main thread opens it; the popover's "Open" reads it
/// again.
#[derive(Clone, Debug)]
pub(crate) struct PreviewTicket {
    pub(crate) open: PreviewOpen,
    /// The preview folder the copy was made in (its index).
    pub(crate) dir: PathBuf,
    pub(crate) read_only: bool,
}

/// What the helper's worker decided about a ⌘-clicked remote path.
enum Decided {
    /// An unchanged copy: open it.
    Cached(RemoteEntry),
    /// Download it; `rescued`: where the user's edited copy was moved first.
    Fetch {
        entry: RemoteEntry,
        rescued: Option<PathBuf>,
    },
    /// A folder: nothing.
    Folder,
    /// The sheet's text.
    Failed(String),
}

/// The worker's answer, back on the main thread.
struct Asked {
    command: u64,
    ssh: Vec<String>,
    host: String,
    mark: HostMark,
    remote: String,
    local: PathBuf,
    dir: PathBuf,
    decided: Decided,
}

/// A preview that will download: everything the queue needs.
struct Fetch {
    command: u64,
    ssh: Vec<String>,
    host: String,
    mark: HostMark,
    remote: String,
    local: PathBuf,
    size: Option<u64>,
    ticket: PreviewTicket,
}

/// The system's beep: the answer to an action that cannot run now (another
/// sheet is open, the remote session ended) — never silence.
pub(crate) fn beep() {
    // SAFETY: `NSBeep` is AppKit's `void NSBeep(void)`; AppKit is linked through
    // `objc2-app-kit` and the call has no preconditions.
    unsafe extern "C" {
        fn NSBeep();
    }
    unsafe { NSBeep() };
}

fn file_url(path: &Path) -> Retained<NSURL> {
    NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()))
}

/// The remote open policy for `remote`'s name ([`remote_files::preview_open`]):
/// the content class from the name's extension (the link opener's UTType white list).
fn policy(entry: &RemoteEntry, remote: &str) -> Option<PreviewOpen> {
    remote_files::preview_open(entry, || {
        Path::new(remote)
            .extension()
            .and_then(|ext| ext.to_str())
            .map_or(Content::Other, extension_content)
    })
}

/// The default plain-text application (`public.plain-text`), through the
/// runtime: `URLForApplicationToOpenContentType:` takes a `UTType`, whose header
/// is not among the crate's opened flags (the `extension_content` precedent).
fn plain_text_app() -> Option<Retained<NSURL>> {
    let class = AnyClass::get(c"UTType")?;
    // SAFETY: `+[UTType typeWithIdentifier:]` takes an `NSString` and returns a
    // nullable `UTType`.
    let ty: Option<Retained<AnyObject>> =
        unsafe { msg_send![class, typeWithIdentifier: &*ns_string!("public.plain-text")] };
    let ty = ty?;
    let workspace = NSWorkspace::sharedWorkspace();
    // SAFETY: `-[NSWorkspace URLForApplicationToOpenContentType:]` (macOS 12) takes
    // a `UTType` and returns a nullable `NSURL`.
    unsafe { msg_send![&*workspace, URLForApplicationToOpenContentType: &*ty] }
}

/// Opens a preview copy by the policy: a known document in its default
/// application, anything else in the default plain-text application — a
/// preview is read, never run. Without a plain-text application the
/// copy is revealed in Finder, the white list's safe side.
pub(crate) fn open_copy(path: &Path, open: PreviewOpen) {
    let workspace = NSWorkspace::sharedWorkspace();
    let url = file_url(path);
    if open == PreviewOpen::Default {
        workspace.openURL(&url);
        return;
    }
    let (Some(app), Some(config)) = (
        plain_text_app(),
        AnyClass::get(c"NSWorkspaceOpenConfiguration"),
    ) else {
        workspace.activateFileViewerSelectingURLs(&NSArray::from_retained_slice(&[url]));
        return;
    };
    // SAFETY: `+[NSWorkspaceOpenConfiguration configuration]` returns a new
    // configuration; `-openURLs:withApplicationAtURL:configuration:completionHandler:`
    // (macOS 10.15) takes an array of file URLs, the application's URL, the
    // configuration and a nullable block (nil: no answer wanted).
    unsafe {
        let config: Retained<AnyObject> = msg_send![config, configuration];
        let urls = NSArray::from_retained_slice(&[url]);
        // Typed as the block it is, so the message's encoding (`@?`) matches.
        let none: Option<&block2::Block<dyn Fn(*mut AnyObject, *mut NSError)>> = None;
        let () = msg_send![
            &*workspace,
            openURLs: &*urls,
            withApplicationAtURL: &*app,
            configuration: &*config,
            completionHandler: none
        ];
    }
}

/// Tells the user that edited previews were kept, not removed: a sheet
/// on `seat` when bateri is in front (a notification would not show), a
/// notification otherwise; `then` runs when the sheet is dismissed (at once
/// without one). "Show in Finder" reveals the moved files.
pub(crate) fn report_rescued(
    mtm: MainThreadMarker,
    seat: Option<Seat>,
    rescued: &[PathBuf],
    then: impl FnOnce() + 'static,
) {
    let Some(first) = rescued.first() else {
        then();
        return;
    };
    let folder = first
        .parent()
        .map_or_else(String::new, |parent| parent.display().to_string());
    let (title, body) = if rescued.len() == 1 {
        let name = first
            .file_name()
            .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
        (
            "Your edited preview was kept".to_owned(),
            format!(
                "{name} was changed after bateri downloaded it, so it was moved to {folder} instead of being removed."
            ),
        )
    } else {
        (
            format!("{} edited previews were kept", rescued.len()),
            format!(
                "They were changed after bateri downloaded them, so they were moved to {folder} instead of being removed."
            ),
        )
    };
    let active = NSApplication::sharedApplication(mtm).isActive();
    let Some(seat) = seat.filter(|seat| active && !seat.is_taken()) else {
        uploader::deliver_notification(&title, &body);
        then();
        return;
    };
    let alert = NSAlert::new(mtm);
    alert.setMessageText(&NSString::from_str(&title));
    alert.setInformativeText(&NSString::from_str(&body));
    alert.addButtonWithTitle(ns_string!("OK"));
    alert.addButtonWithTitle(ns_string!("Show in Finder"));
    let urls: Vec<String> = rescued
        .iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect();
    // The block is `Fn`: `then` is taken once.
    let then = RefCell::new(Some(then));
    let answered = RcBlock::new(move |response: NSModalResponse| {
        if response == NSAlertSecondButtonReturn {
            let urls: Vec<Retained<NSURL>> =
                urls.iter().map(|path| file_url(Path::new(path))).collect();
            NSWorkspace::sharedWorkspace()
                .activateFileViewerSelectingURLs(&NSArray::from_retained_slice(&urls));
        }
        if let Some(then) = then.borrow_mut().take() {
            then();
        }
    });
    seat.begin(&alert, &answered);
}

impl TerminalPane {
    /// The ⌘-click (or "Open Preview") on the remote file `remote`: asks
    /// the helper what it is now, then [`TerminalPane::preview_decided`].
    pub(crate) fn preview_remote(&self, remote: String) {
        let Some((command, target, _)) = self.session().and_then(|session| session.remote_target())
        else {
            beep();
            return;
        };
        let mark = self
            .session()
            .and_then(|session| session.remote_mark())
            .map_or(HostMark::None, |(_, mark)| mark);
        let (preview_dir, download_dir) = {
            let files = self.remote_files().borrow();
            (files.preview_dir.clone(), files.download_dir.clone())
        };
        let home = child::home();
        let dir = bt_core::expand_home(&preview_dir, home.as_deref());
        let downloads = bt_core::expand_home(&download_dir, home.as_deref());
        let (Some(dir), Some(downloads)) = (dir, downloads) else {
            self.preview_failed(
                &target.host,
                &format!("The preview folder {preview_dir} can't be found."),
            );
            return;
        };
        let host = target.host.clone();
        let Some(local) = remote_files::preview_path(&dir, &host, &remote) else {
            self.preview_failed(&host, &format!("{remote} can't be previewed."));
            return;
        };
        // Already streaming: that copy opens when it lands.
        if self.uploads().borrow().previewing(&local) {
            return;
        }
        // The job does not hold the sheet gate: the password sheet takes it if free.
        let dial = self.dial(target, Some((SheetJob::Preview, false)));
        let (id, lookup) = (self.id(), self.lookup());
        let request = Request {
            command,
            dial,
            host: host.clone(),
            query: Query::Count(remote.clone()),
            reply: Box::new(move |answer, ssh| {
                let ssh = ssh.to_vec();
                let decided = match answer {
                    Ok(Answer::Counted(Some(entry @ RemoteEntry::File { size, mtime, .. }))) => {
                        decide(&dir, &local, &downloads, entry, (size, mtime))
                    }
                    Ok(Answer::Counted(Some(RemoteEntry::Dir(_)))) => Decided::Folder,
                    Ok(_) => Decided::Failed(format!("{remote} no longer exists on {host}.")),
                    Err(text) => Decided::Failed(text),
                };
                let asked = Asked {
                    command,
                    ssh,
                    host,
                    mark,
                    remote,
                    local,
                    dir,
                    decided,
                };
                on_pane(lookup, id, move |pane| pane.preview_decided(asked));
            }),
        };
        self.remote_helper().borrow_mut().ask(request);
    }

    /// The helper's answer on the main thread: open, download (after the size
    /// question if it is over `preview_max_size`), nothing for a folder, or the
    /// error sheet.
    fn preview_decided(&self, asked: Asked) {
        let alive = self
            .session()
            .and_then(|session| session.remote_target())
            .is_some_and(|(command, ..)| command == asked.command);
        if !alive {
            beep();
            return;
        }
        let Asked {
            command,
            ssh,
            host,
            mark,
            remote,
            local,
            dir,
            decided,
        } = asked;
        let (entry, rescued) = match decided {
            Decided::Folder => return,
            // The password sheet was cancelled: nothing more to say.
            Decided::Failed(text) if text == crate::ssh_route::CANCELLED => return,
            Decided::Failed(text) => {
                self.preview_failed(&host, &text);
                return;
            }
            Decided::Cached(entry) => {
                if let Some(open) = policy(&entry, &remote) {
                    open_copy(&local, open);
                }
                return;
            }
            Decided::Fetch { entry, rescued } => (entry, rescued),
        };
        let Some(open) = policy(&entry, &remote) else {
            return;
        };
        let (limit, read_only) = {
            let files = self.remote_files().borrow();
            (files.preview_max_size, files.preview_read_only)
        };
        let size = match entry {
            RemoteEntry::File { size, .. } => size,
            RemoteEntry::Dir(_) => None,
        };
        let fetch = Fetch {
            command,
            ssh,
            host,
            mark,
            remote,
            local,
            size,
            ticket: PreviewTicket {
                open,
                dir,
                read_only,
            },
        };
        let (id, lookup) = (self.id(), self.lookup());
        let go = move || {
            // audit: `then` runs on the main thread (the sheet's block or inline).
            let mtm = MainThreadMarker::new().expect("the rescue report is on the main thread");
            let Some(pane) = lookup(mtm, id) else {
                return;
            };
            if fetch.size.is_some_and(|size| size > limit) {
                pane.ask_preview_limit(fetch);
            } else {
                pane.fetch_preview(fetch);
            }
        };
        match rescued {
            Some(path) => {
                // One main-queue turn later: the rescue sheet has detached by
                // then and the size question can attach.
                let seat = sheets::seat(Asker::Pane(self));
                report_rescued(self.mtm(), seat, &[path], move || {
                    DispatchQueue::main().exec_async(go);
                });
            }
            None => go(),
        }
    }

    /// The preview goes into the queue's preview lane, which starts at once.
    fn fetch_preview(&self, fetch: Fetch) {
        let Fetch {
            command,
            ssh,
            host,
            mark,
            remote,
            local,
            size,
            ticket,
        } = fetch;
        // A second ⌘-click answered while the first one's stream started.
        if self.uploads().borrow().previewing(&local) {
            return;
        }
        let Some(job) = Job::download(
            &remote,
            local.clone(),
            false,
            1,
            size.unwrap_or(0),
            Lane::Preview,
            Conflict::Replace,
        ) else {
            beep();
            return;
        };
        self.previews().borrow_mut().insert(local, ticket);
        let queued = self.upload_confirmed(Confirmed {
            command,
            ssh,
            host,
            mark,
            jobs: vec![job],
        });
        if !queued {
            beep();
        }
    }

    /// A file over `preview_max_size` asks first — "Open Preview" (the
    /// default), "Cancel" (Esc) and, on the left, "Save to Downloads instead"
    /// (the right-click download's path). The text says the copy is temporary
    /// (and read-only) and removed at a later launch.
    fn ask_preview_limit(&self, fetch: Fetch) {
        let Some(seat) =
            sheets::seat(Asker::Pane(self)).filter(|seat| !seat.is_taken() && self.accepts_drop())
        else {
            beep();
            return;
        };
        let name = remote_files::split_remote(&fetch.remote)
            .map_or_else(String::new, |(_, name)| name.to_owned());
        let size = fetch.size.map(format_bytes).unwrap_or_default();
        let alert = NSAlert::new(self.mtm());
        alert.setMessageText(&NSString::from_str(&format!("{name} is {size}")));
        let kind = if fetch.ticket.read_only {
            "a temporary, read-only copy"
        } else {
            "a temporary copy"
        };
        alert.setInformativeText(&NSString::from_str(&format!(
            "The preview downloads {kind} to {}. bateri removes it when it cleans up at launch.",
            fetch.ticket.dir.display()
        )));
        alert.addButtonWithTitle(ns_string!("Open Preview"));
        let cancel = alert.addButtonWithTitle(ns_string!("Cancel"));
        // Esc by hand (the rationale of `window::alert`).
        cancel.setKeyEquivalent(ns_string!("\u{1b}"));
        alert.addButtonWithTitle(ns_string!("Save to Downloads instead"));
        self.uploads().borrow_mut().set_asking(true);
        let (id, lookup) = (self.id(), self.lookup());
        // The block is `Fn`: the payload is taken once.
        let payload = RefCell::new(Some(fetch));
        let answered = RcBlock::new(move |response: NSModalResponse| {
            // audit: the sheet's completion block runs on AppKit's main thread.
            let mtm = MainThreadMarker::new().expect("the sheet block is on the main thread");
            let Some(pane) = lookup(mtm, id) else {
                return;
            };
            drop(pane.upload_alert().take());
            pane.uploads().borrow_mut().set_asking(false);
            let Some(fetch) = payload.borrow_mut().take() else {
                return;
            };
            if response == NSAlertFirstButtonReturn {
                pane.fetch_preview(fetch);
            } else if response == NSAlertThirdButtonReturn {
                pane.download_remote(fetch.remote, None);
            }
        });
        self.upload_alert().replace(Some(alert.clone()));
        seat.begin(&alert, &answered);
    }

    /// "Can't preview from {host}" with the reason; a beep if another sheet is open.
    fn preview_failed(&self, host: &str, text: &str) {
        self.failure_sheet(&format!("Can't preview from {host}"), text);
    }

    /// A remote job's error sheet — title, ssh's reason, OK — through the
    /// pane's sheet gate; a beep when another sheet is up.
    pub(crate) fn failure_sheet(&self, title: &str, text: &str) {
        let Some(seat) =
            sheets::seat(Asker::Pane(self)).filter(|seat| !seat.is_taken() && self.accepts_drop())
        else {
            beep();
            return;
        };
        let alert = NSAlert::new(self.mtm());
        alert.setMessageText(&NSString::from_str(title));
        alert.setInformativeText(&NSString::from_str(text));
        alert.addButtonWithTitle(ns_string!("OK"));
        self.uploads().borrow_mut().set_asking(true);
        let (id, lookup) = (self.id(), self.lookup());
        let answered = RcBlock::new(move |_: NSModalResponse| {
            // audit: the sheet's completion block runs on AppKit's main thread.
            let mtm = MainThreadMarker::new().expect("the sheet block is on the main thread");
            if let Some(pane) = lookup(mtm, id) {
                drop(pane.upload_alert().take());
                pane.uploads().borrow_mut().set_asking(false);
            }
        });
        self.upload_alert().replace(Some(alert.clone()));
        seat.begin(&alert, &answered);
    }

    /// A finished preview's copy opens by its ticket's policy (the popover's
    /// "Open" and the stream's end); without a ticket as plain text — the safe
    /// side, it is never run. Its age restarts (off the main thread).
    pub(crate) fn open_preview(&self, path: &Path) {
        let ticket = self.previews().borrow().get(path).cloned();
        open_copy(
            path,
            ticket.as_ref().map_or(PreviewOpen::PlainText, |t| t.open),
        );
        if let Some(ticket) = ticket {
            let path = path.to_owned();
            let _ = thread::Builder::new()
                .name("preview touch".into())
                .spawn(move || preview_cache::touch(&ticket.dir, &path, preview_cache::now()));
        }
    }
}

/// The cache's answer for a remote file, on the helper's worker.
fn decide(
    dir: &Path,
    local: &Path,
    downloads: &Path,
    entry: RemoteEntry,
    remote: (Option<u64>, Option<u64>),
) -> Decided {
    match preview_cache::state(dir, local, remote) {
        CacheState::Fresh => {
            preview_cache::touch(dir, local, preview_cache::now());
            Decided::Cached(entry)
        }
        CacheState::Stale => Decided::Fetch {
            entry,
            rescued: None,
        },
        CacheState::Diverged => match preview_cache::rescue(dir, local, downloads) {
            Ok(path) => Decided::Fetch {
                entry,
                rescued: Some(path),
            },
            // The copy that may hold the user's edits stays where it is and
            // nothing overwrites it.
            Err(error) => Decided::Failed(format!(
                "The preview on this Mac was changed and could not be kept aside: {error}"
            )),
        },
    }
}
