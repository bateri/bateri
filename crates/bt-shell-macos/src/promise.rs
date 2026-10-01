//! **⌘-dragging a remote link to Finder** (045 R7, Karar 7, 14): the drag is a
//! file promise (`NSFilePromiseProvider`), and the promise is kept by a
//! download into the transfer queue's **Finder lane**, which never waits.
//!
//! The flow, one hop per thread:
//!
//! 1. **Main** ([`begin_drag`], from the view's `Drag::Link`): the provider (the
//!    remote name's content type, `public.folder` for a folder), a drag image
//!    (the system icon of that type and the name) and the drag session; the view
//!    is the source (`NSDraggingSource`, Copy only). The delegate
//!    ([`FilePromise`]) is weak in the provider, so the pane keeps it
//!    ([`FinderDrops::promises`]) until Finder asks for the file or the drag
//!    ends with no drop.
//! 2. **The delegate's own operation queue** (`writePromiseToURL:`): the
//!    completion handler is copied into a [`Promised`] — called **exactly
//!    once**, by construction: [`Promised::finish`] or, if it is dropped
//!    unfinished on any path (the pane closed, the session ended, the queue
//!    refused), its `Drop` with `NSUserCancelledError`. The callback returns at
//!    once and hops to the main queue. A background queue and not the main one:
//!    AppKit may wait on the delivering thread for the handler, and the work
//!    behind it needs the main thread.
//! 3. **Main** ([`TerminalPane::finder_write`]) → **the helper's worker** (a fresh
//!    `Query::Count`: a folder's files and bytes are the bar's and the
//!    `NSProgress`'s total) → **main** ([`TerminalPane::finder_counted`]): the
//!    download enters the Finder lane under `Conflict::Replace` — Finder chose
//!    the destination and its name, so no conflict rule applies — and its
//!    progress is published on the destination URL (`NSProgress`, kind file,
//!    downloading; Finder's cancel button stops the item).
//! 4. **The stream thread** (`uploader`'s `spawn_transfer`) writes into a hidden
//!    temporary folder next to the destination and renames at the end
//!    (`download::transfer`): a cancel or an error deletes it, so nothing half
//!    written is left where Finder looks. The item's end finishes the promise
//!    ([`TerminalPane::finder_finished`]).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use block2::{DynBlock, RcBlock};
use bt_core::HostMark;
use dispatch2::DispatchQueue;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, ProtocolObject};
use objc2::{
    AllocAnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send,
};
use objc2_app_kit::{
    NSDragOperation, NSDraggingItem, NSDraggingSession, NSEvent, NSFilePromiseProvider,
    NSFilePromiseProviderDelegate, NSFont, NSImage, NSImageView, NSLineBreakMode, NSTextAlignment,
    NSTextField, NSView, NSWorkspace,
};
use objc2_foundation::{
    NSArray, NSCocoaErrorDomain, NSDictionary, NSError, NSErrorUserInfoKey,
    NSLocalizedDescriptionKey, NSObject, NSObjectProtocol, NSOperationQueue, NSPoint, NSProgress,
    NSProgressFileOperationKindDownloading, NSProgressKindFile, NSRect, NSSize, NSString, NSURL,
    NSUserCancelledError,
};

use crate::download::Conflict;
use crate::pane::{PaneLookup, TerminalPane};
use crate::remote_files::{self, RemoteEntry};
use crate::remote_helper::{Answer, Query, Request};
use crate::upload::{self, Job, Lane, Outcome, Shared};
use crate::uploader::Confirmed;
use crate::view::BateriView;

/// `NSFileWriteUnknownError`: a failed download's error code — the text says why.
const WRITE_FAILED: isize = 512;

/// The drag image's icon edge (points): Finder's own list-drag size. A design
/// constant.
const ICON: f64 = 48.0;

/// The drag image's widest label (points); a longer name is shortened in the
/// middle, like Finder's.
const LABEL_MAX: f64 = 220.0;

/// A file promise's completion handler, called **exactly once**: by
/// [`Promised::finish`], or — on every path that drops it unfinished — by its
/// `Drop` with `NSUserCancelledError`, so a promise can never be left hanging
/// in Finder.
pub(crate) struct Promised(Option<RcBlock<dyn Fn(*mut NSError)>>);

// SAFETY: the block is AppKit's completion handler copied to the heap: Apple's
// file promise handlers may be called from any thread (the delegate's operation
// queue is a background one by design), and only ownership moves across threads
// — the block is called once, from one thread, and released after.
unsafe impl Send for Promised {}

impl Promised {
    /// Fulfils the promise (`Ok`) or fails it with the reason Finder shows
    /// (`Err`). A cancel goes through [`Promised::cancel`].
    pub(crate) fn finish(mut self, result: Result<(), String>) {
        if let Some(handler) = self.0.take() {
            match result {
                Ok(()) => handler.call((std::ptr::null_mut(),)),
                Err(reason) => call_with(&handler, &failure(&reason)),
            }
        }
    }

    /// Fails the promise as cancelled by the user (Finder removes its placeholder
    /// quietly). The same as dropping it; named for the call sites that mean it.
    pub(crate) fn cancel(self) {
        drop(self);
    }
}

impl Drop for Promised {
    fn drop(&mut self) {
        if let Some(handler) = self.0.take() {
            call_with(&handler, &cancelled());
        }
    }
}

fn call_with(handler: &RcBlock<dyn Fn(*mut NSError)>, error: &NSError) {
    // The handler takes a nullable `NSError *` it does not consume; `error`
    // outlives the call.
    handler.call((std::ptr::from_ref(error).cast_mut(),));
}

/// `NSUserCancelledError` in the Cocoa domain.
fn cancelled() -> Retained<NSError> {
    // SAFETY: a Foundation domain constant, a Foundation code, no user info.
    unsafe {
        NSError::errorWithDomain_code_userInfo(NSCocoaErrorDomain, NSUserCancelledError, None)
    }
}

/// A write failure whose localized description is `reason`.
fn failure(reason: &str) -> Retained<NSError> {
    let text = NSString::from_str(reason);
    // SAFETY: a Foundation key constant.
    let key: &NSErrorUserInfoKey = unsafe { NSLocalizedDescriptionKey };
    let info =
        NSDictionary::<NSErrorUserInfoKey, AnyObject>::from_slices(&[key], &[&*text as &AnyObject]);
    // SAFETY: `NSLocalizedDescriptionKey`'s documented value is a string.
    unsafe { NSError::errorWithDomain_code_userInfo(NSCocoaErrorDomain, WRITE_FAILED, Some(&info)) }
}

/// What a promise delegate knows: the pane that owns the drag, the remote session
/// it belongs to and the remote item.
pub(crate) struct PromiseIvars {
    pane: u64,
    lookup: PaneLookup,
    /// The remote session's generation at the drag (a later session's helper
    /// must not be asked about this one's path).
    command: u64,
    /// The remote absolute path.
    remote: String,
    /// The remote name — the promised file's name.
    name: String,
    /// The background queue the write callback arrives on.
    queue: Retained<NSOperationQueue>,
    /// Whether Finder asked for the file: the pane stops keeping the delegate.
    asked: AtomicBool,
}

define_class!(
    // SAFETY: NSObject subclassing has no requirements; `FilePromise` does not
    // implement `Drop`. Not `MainThreadOnly`: the write callback arrives on the
    // delegate's own background queue and touches only `Send` data and the
    // atomic flag there.
    #[unsafe(super(NSObject))]
    #[name = "BateriFilePromise"]
    #[ivars = PromiseIvars]
    pub(crate) struct FilePromise;

    unsafe impl NSObjectProtocol for FilePromise {}

    unsafe impl NSFilePromiseProviderDelegate for FilePromise {
        /// The promised item's name: the remote name (Finder may still pick
        /// another one next to an existing item; the write takes its URL).
        #[unsafe(method_id(filePromiseProvider:fileNameForType:))]
        fn file_name(
            &self,
            _provider: &NSFilePromiseProvider,
            _file_type: &NSString,
        ) -> Retained<NSString> {
            NSString::from_str(&self.ivars().name)
        }

        /// Finder wants the item at `url`: the handler is kept (exactly once,
        /// [`Promised`]) and the work hops to the main queue. Returns at once.
        #[unsafe(method(filePromiseProvider:writePromiseToURL:completionHandler:))]
        fn write_promise(
            &self,
            _provider: &NSFilePromiseProvider,
            url: &NSURL,
            completion_handler: &DynBlock<dyn Fn(*mut NSError)>,
        ) {
            let promised = Promised(Some(completion_handler.copy()));
            let ivars = self.ivars();
            ivars.asked.store(true, Ordering::Release);
            let Some(landing) = url.to_file_path() else {
                promised.finish(Err("The destination is not a folder on this Mac.".into()));
                return;
            };
            let (id, lookup, command, remote) = (
                ivars.pane,
                ivars.lookup,
                ivars.command,
                ivars.remote.clone(),
            );
            DispatchQueue::main().exec_async(move || {
                // audit: a block running on the main queue is by definition on the main thread.
                let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
                match lookup(mtm, id) {
                    Some(pane) => pane.finder_write(command, remote, landing, promised),
                    // The pane closed: the promise is cancelled (`Promised`'s drop).
                    None => promised.cancel(),
                }
            });
        }

        /// The write's queue: a background one ([`FilePromise`]'s module doc).
        #[unsafe(method_id(operationQueueForFilePromiseProvider:))]
        fn operation_queue(&self, _provider: &NSFilePromiseProvider) -> Retained<NSOperationQueue> {
            self.ivars().queue.clone()
        }
    }
);

impl FilePromise {
    fn new(
        pane: u64,
        lookup: PaneLookup,
        command: u64,
        remote: String,
        name: String,
    ) -> Retained<Self> {
        let queue = NSOperationQueue::new();
        queue.setName(Some(&NSString::from_str("bateri file promise")));
        let this = Self::alloc().set_ivars(PromiseIvars {
            pane,
            lookup,
            command,
            remote,
            name,
            queue,
            asked: AtomicBool::new(false),
        });
        // SAFETY: NSObject's `init` takes no arguments and the ivars are set.
        unsafe { msg_send![super(this), init] }
    }

    fn asked(&self) -> bool {
        self.ivars().asked.load(Ordering::Acquire)
    }
}

/// A Finder download in flight: the promise to fulfil and its published
/// progress.
struct FinderDrop {
    promised: Option<Promised>,
    progress: Retained<NSProgress>,
    /// The queue item, once it started (`spawn_transfer`).
    item: Option<u64>,
    shared: Option<Arc<Shared>>,
}

impl Drop for FinderDrop {
    fn drop(&mut self) {
        self.progress.unpublish();
        // An unfinished promise is cancelled by `Promised`'s own drop.
    }
}

/// The pane's file promises: the delegates it keeps alive (the provider holds
/// them weakly) and the downloads that will fulfil them.
#[derive(Default)]
pub(crate) struct FinderDrops {
    /// One per drag session until Finder asks for the item or the drag ends with
    /// no drop. A list, not one slot: two quick drags must both survive until
    /// asked.
    promises: Vec<(Retained<NSDraggingSession>, Retained<FilePromise>)>,
    /// By destination path (the item's `job.landing()`).
    drops: HashMap<PathBuf, FinderDrop>,
}

/// The remote name's content type identifier: `public.folder` for a folder,
/// the extension's type for a file (`public.data` when it has none known).
/// Read through the runtime (`AnyClass::get`, `hyperlink::extension_content`'s
/// precedent) — no `UTType` flag.
fn content_type(name: &str, dir: bool) -> Retained<NSString> {
    let fallback = || NSString::from_str(if dir { "public.folder" } else { "public.data" });
    if dir {
        return fallback();
    }
    let (Some(ext), Some(class)) = (
        Path::new(name).extension().and_then(|ext| ext.to_str()),
        AnyClass::get(c"UTType"),
    ) else {
        return fallback();
    };
    // SAFETY: `+[UTType typeWithFilenameExtension:]` takes an `NSString` and
    // returns a nullable `UTType`; `-identifier` returns an `NSString`.
    unsafe {
        let ty: Option<Retained<AnyObject>> =
            msg_send![class, typeWithFilenameExtension: &*NSString::from_str(ext)];
        match ty {
            Some(ty) => msg_send![&*ty, identifier],
            None => fallback(),
        }
    }
}

/// The system icon of the content type `identifier`.
fn icon(identifier: &NSString) -> Option<Retained<NSImage>> {
    let class = AnyClass::get(c"UTType")?;
    // SAFETY: `+[UTType typeWithIdentifier:]` takes an `NSString` and returns a
    // nullable `UTType`; `-[NSWorkspace iconForContentType:]` takes a `UTType`
    // and returns an `NSImage`.
    unsafe {
        let ty: Option<Retained<AnyObject>> = msg_send![class, typeWithIdentifier: identifier];
        let ty = ty?;
        msg_send![&*NSWorkspace::sharedWorkspace(), iconForContentType: &*ty]
    }
}

/// The drag image: the type's icon with the name below it (Finder's look). A
/// throwaway view rendered to PDF — only headers already on.
fn drag_image(
    mtm: MainThreadMarker,
    identifier: &NSString,
    name: &str,
) -> Option<Retained<NSImage>> {
    let icon = icon(identifier)?;
    let label = NSTextField::labelWithString(&NSString::from_str(name), mtm);
    label.setFont(Some(&NSFont::systemFontOfSize(12.0)));
    label.setAlignment(NSTextAlignment::Center);
    label.setLineBreakMode(NSLineBreakMode::ByTruncatingMiddle);
    let fit = label.fittingSize();
    let width = fit.width.clamp(ICON, LABEL_MAX).ceil();
    let label_h = fit.height.ceil();
    let gap = 2.0;
    let size = NSSize::new(width, ICON + gap + label_h);
    let frame = NSRect::new(NSPoint::new(0.0, 0.0), size);
    let view = NSView::initWithFrame(NSView::alloc(mtm), frame);
    let picture = NSImageView::imageViewWithImage(&icon, mtm);
    picture.setFrame(NSRect::new(
        NSPoint::new((width - ICON) / 2.0, label_h + gap),
        NSSize::new(ICON, ICON),
    ));
    label.setFrame(NSRect::new(
        NSPoint::new(0.0, 0.0),
        NSSize::new(width, label_h),
    ));
    view.addSubview(&picture);
    view.addSubview(&label);
    let data = view.dataWithPDFInsideRect(frame);
    NSImage::initWithData(NSImage::alloc(), &data).or(Some(icon))
}

/// `Drag::Link` on the remote path `remote` (`entry`: what the hover found): the
/// file promise drag from `view` (045 R7). A beep-free no-op if the pane or the
/// remote session is gone — the gesture simply does nothing.
pub(crate) fn begin_drag(view: &BateriView, event: &NSEvent, remote: String, entry: &RemoteEntry) {
    let Some(pane) = view.pane() else {
        return;
    };
    let Some((command, ..)) = pane.session().and_then(|session| session.remote_target()) else {
        return;
    };
    let Some(name) = remote_files::split_remote(&remote).map(|(_, name)| name.to_owned()) else {
        return;
    };
    let dir = matches!(entry, RemoteEntry::Dir(_));
    let identifier = content_type(&name, dir);
    let delegate = FilePromise::new(pane.id(), pane.lookup(), command, remote, name.clone());
    let provider = NSFilePromiseProvider::initWithFileType_delegate(
        NSFilePromiseProvider::alloc(),
        &identifier,
        ProtocolObject::from_ref(&*delegate),
    );
    let item = NSDraggingItem::initWithPasteboardWriter(
        NSDraggingItem::alloc(),
        ProtocolObject::from_ref(&*provider),
    );
    let mtm = view.mtm();
    let at = view.convertPoint_fromView(event.locationInWindow(), None);
    let image = drag_image(mtm, &identifier, &name);
    let size = image
        .as_ref()
        .map_or(NSSize::new(ICON, ICON), |image| image.size());
    // Centred on the pointer, the icon's middle under it. The frame is set even
    // without an image: an item with no dragging frame has no place to start.
    let frame = NSRect::new(
        NSPoint::new(at.x - size.width / 2.0, at.y - ICON / 2.0),
        size,
    );
    // SAFETY: an `NSImage` (or nothing) is a documented dragging frame content.
    unsafe { item.setDraggingFrame_contents(frame, image.as_deref().map(|image| image.as_ref())) };
    let session = view.beginDraggingSessionWithItems_event_source(
        &NSArray::from_retained_slice(&[item]),
        event,
        ProtocolObject::from_ref(view),
    );
    pane.finder_drops()
        .borrow_mut()
        .promises
        .push((session, delegate));
}

impl TerminalPane {
    /// The view's drag session ended (`draggingSession:endedAtPoint:operation:`):
    /// with no drop its delegate is let go — Finder will never ask.
    pub(crate) fn finder_drag_ended(
        &self,
        session: &NSDraggingSession,
        operation: NSDragOperation,
    ) {
        if operation != NSDragOperation::None {
            return;
        }
        let gone: Vec<_> = {
            let mut drops = self.finder_drops().borrow_mut();
            let (gone, kept) = std::mem::take(&mut drops.promises)
                .into_iter()
                .partition(|(owner, promise)| std::ptr::eq(&**owner, session) && !promise.asked());
            drops.promises = kept;
            gone
        };
        drop(gone);
    }

    /// Finder asked for the item at `landing` (main thread): the remote item is
    /// counted afresh, then [`TerminalPane::finder_counted`]. A session that
    /// ended (or changed) fails the promise.
    pub(crate) fn finder_write(
        &self,
        command: u64,
        remote: String,
        landing: PathBuf,
        promised: Promised,
    ) {
        // The asked delegates are no longer needed (the handler is in `promised`).
        let asked: Vec<_> = {
            let mut drops = self.finder_drops().borrow_mut();
            let (asked, kept) = std::mem::take(&mut drops.promises)
                .into_iter()
                .partition(|(_, promise)| promise.asked());
            drops.promises = kept;
            asked
        };
        drop(asked);
        let Some((now, target, _)) = self.session().and_then(|session| session.remote_target())
        else {
            promised.finish(Err("The remote session ended.".into()));
            return;
        };
        if now != command {
            promised.finish(Err("The remote session ended.".into()));
            return;
        }
        let mark = self
            .session()
            .and_then(|session| session.remote_mark())
            .map_or(HostMark::None, |(_, mark)| mark);
        let ssh = upload::ssh_argv(&target);
        let host = target.host;
        let (id, lookup) = (self.id(), self.lookup());
        let request = Request {
            command,
            ssh: ssh.clone(),
            host: host.clone(),
            query: Query::Count(remote.clone()),
            reply: Box::new(move |answer| {
                let counted = match answer {
                    Ok(Answer::Counted(Some(entry))) => Ok(entry),
                    Ok(_) => Err(format!("{remote} no longer exists on {host}.")),
                    Err(text) => Err(text),
                };
                let finder = Finder {
                    command,
                    ssh,
                    host,
                    mark,
                    remote,
                    landing,
                };
                DispatchQueue::main().exec_async(move || {
                    // audit: a block running on the main queue is by definition on the main thread.
                    let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
                    match lookup(mtm, id) {
                        Some(pane) => pane.finder_counted(finder, counted, promised),
                        None => promised.cancel(),
                    }
                });
            }),
        };
        self.remote_helper().borrow_mut().ask(request);
    }

    /// The count came back: the download enters the Finder lane (it starts at
    /// once, whatever the queue holds) with its progress published.
    fn finder_counted(
        &self,
        finder: Finder,
        counted: Result<RemoteEntry, String>,
        promised: Promised,
    ) {
        let entry = match counted {
            Ok(entry) => entry,
            Err(text) => {
                promised.finish(Err(text));
                return;
            }
        };
        let Finder {
            command,
            ssh,
            host,
            mark,
            remote,
            landing,
        } = finder;
        let (dir, files, bytes) = match entry {
            RemoteEntry::File { size, .. } => (false, 1, size.unwrap_or(0)),
            RemoteEntry::Dir(size) => (
                true,
                size.map_or(0, |s| s.files),
                size.map_or(0, |s| s.bytes),
            ),
        };
        let Some(job) = Job::download(
            &remote,
            landing.clone(),
            dir,
            files,
            bytes,
            Lane::Finder,
            Conflict::Replace,
        ) else {
            promised.finish(Err(format!("{remote} can't be downloaded safely.")));
            return;
        };
        let progress = self.finder_progress_for(&landing, bytes);
        let stale = self.finder_drops().borrow_mut().drops.insert(
            landing.clone(),
            FinderDrop {
                promised: Some(promised),
                progress,
                item: None,
                shared: None,
            },
        );
        drop(stale);
        let queued = self.upload_confirmed(Confirmed {
            command,
            ssh,
            host,
            mark,
            jobs: vec![job],
        });
        if !queued {
            let refused = self.finder_drops().borrow_mut().drops.remove(&landing);
            if let Some(mut refused) = refused
                && let Some(promised) = refused.promised.take()
            {
                promised.finish(Err(
                    "The transfer could not start: another one is still stopping.".into(),
                ));
            }
        }
    }

    /// The `NSProgress` Finder reads on `landing` (Karar 7): kind file,
    /// downloading, cancellable — its cancel stops the item.
    fn finder_progress_for(&self, landing: &Path, bytes: u64) -> Retained<NSProgress> {
        let progress = NSProgress::discreteProgressWithTotalUnitCount(
            i64::try_from(bytes).unwrap_or(i64::MAX),
        );
        // SAFETY: Foundation's constants for a file progress.
        unsafe {
            progress.setKind(Some(NSProgressKindFile));
            progress.setFileOperationKind(Some(NSProgressFileOperationKindDownloading));
        }
        if let Some(url) = NSURL::from_file_path(landing) {
            progress.setFileURL(Some(&url));
        }
        progress.setCancellable(true);
        let (id, lookup, at) = (self.id(), self.lookup(), landing.to_owned());
        let cancel = RcBlock::new(move || {
            let at = at.clone();
            // Any thread: the stop is the main thread's.
            DispatchQueue::main().exec_async(move || {
                // audit: a block running on the main queue is by definition on the main thread.
                let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
                if let Some(pane) = lookup(mtm, id) {
                    pane.finder_cancel(&at);
                }
            });
        });
        // SAFETY: the handler captures only `Send` data and touches the pane
        // only through the main queue.
        unsafe { progress.setCancellationHandler(Some(&cancel)) };
        progress.publish();
        progress
    }

    /// `spawn_transfer` started the Finder item `item` landing at `landing`.
    pub(crate) fn finder_started(&self, landing: &Path, item: u64, shared: &Arc<Shared>) {
        if let Some(found) = self.finder_drops().borrow_mut().drops.get_mut(landing) {
            found.item = Some(item);
            found.shared = Some(Arc::clone(shared));
        }
    }

    /// A progress tick: every published Finder progress follows its bytes.
    pub(crate) fn finder_tick(&self) {
        for found in self.finder_drops().borrow().drops.values() {
            if let Some(shared) = &found.shared {
                let (bytes, _) = shared.progress();
                found
                    .progress
                    .setCompletedUnitCount(i64::try_from(bytes).unwrap_or(i64::MAX));
            }
        }
    }

    /// The item `item` ended: if it is a Finder download, its promise is kept
    /// (`Done`) or failed, and its progress unpublished.
    pub(crate) fn finder_finished(&self, item: u64, outcome: &Outcome) {
        let ended = {
            let mut drops = self.finder_drops().borrow_mut();
            let key = drops
                .drops
                .iter()
                .find(|(_, found)| found.item == Some(item))
                .map(|(key, _)| key.clone());
            key.and_then(|key| drops.drops.remove(&key))
        };
        let Some(mut ended) = ended else {
            return;
        };
        if let Some(promised) = ended.promised.take() {
            match outcome {
                Outcome::Done => {
                    // The bar ends full before it is unpublished.
                    ended
                        .progress
                        .setCompletedUnitCount(ended.progress.totalUnitCount());
                    promised.finish(Ok(()));
                }
                Outcome::Cancelled => promised.cancel(),
                Outcome::DiskFull => promised.finish(Err("The disk is full on this Mac.".into())),
                Outcome::Failed(reason) => promised.finish(Err(reason.clone())),
            }
        }
    }

    /// Finder's cancel on the progress at `landing`: the item stops, without
    /// asking — the user pressed stop on that very item.
    fn finder_cancel(&self, landing: &Path) {
        let item = self
            .finder_drops()
            .borrow()
            .drops
            .get(landing)
            .and_then(|found| found.item);
        if let Some(item) = item {
            self.apply_stop(Some(item), false);
        }
    }

    /// The pane closes: every pending promise is cancelled and every progress
    /// unpublished (`FinderDrop`'s and `Promised`'s drops).
    pub(crate) fn finder_abandon(&self) {
        let drops = std::mem::take(&mut *self.finder_drops().borrow_mut());
        drop(drops);
    }
}

/// A Finder write between the main thread and the helper's answer.
struct Finder {
    command: u64,
    ssh: Vec<String>,
    host: String,
    mark: HostMark,
    remote: String,
    landing: PathBuf,
}
