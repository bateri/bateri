//! Terminal pane: the **whole core** of a single terminal session — the
//! session, the display link that drives frames, its own `Renderer`, the
//! `CAMetalLayer` surface, `BateriView`, the shell's wake end (`ShellWake`),
//! the dock reserve, the temporary point-size delta, the tab identity, the
//! scrollback search panel and the upload queue (039 Karar 1–3).
//!
//! `TerminalPane` is an `NSView` subclass and is the very same thing as
//! today's content container (033 → R4.1): `BateriView` is its child that
//! fills it via autoresizing, and the search panel floats inside it as a
//! sibling of the Metal layer. The window (`window::TerminalWindow`) plugs the
//! pane into the splits container (`split_view::SplitView`) and keeps the
//! work that belongs to the **tab**: chrome, title, tab, the close question;
//! geometry, occlusion and focus are distributed from the window to all
//! panes. A tab can hold several panes (039 splits): each with its own
//! session, link and renderer.
//!
//! **The boundary has three parts** (Karar 1, 3): the pane takes its inputs
//! at birth in a single package ([`PaneLaunch`]: settings snapshot, theme,
//! timed-run recipe, measurement ledger, integration environment + dock
//! reserve, identity, start directory and first input, motion flags), hands
//! its events to its owner through [`PaneHost`] (title, the shell's exit,
//! upload status, notification, subtitle notices, OSC 52 copy) and every job
//! the menu fulfils is a named method here — the selector is a line that
//! calls it. Pane-level selectors (point size, find, clear, scroll, upload
//! cancel) live on the pane, because the responder chain is
//! `BateriView` → **pane** → window → delegate: a targetless menu item
//! reaches them from the focused pane, even while the search field has focus
//! (the field is a descendant of the pane). There is no path in this module
//! that reaches `AppDelegate`: main-queue returns find the pane by id through
//! the lookup function the owner supplies ([`PaneLookup`]).
//!
//! **Renderer per pane** (039 Karar 5; 026 → Karar 2a): the atlas key
//! includes scale and point size, and the point-size delta belongs to the pane.

use std::cell::{Cell, OnceCell, RefCell};
use std::collections::HashMap;
use std::ffi::c_void;
use std::path::PathBuf;
use std::ptr::NonNull;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use bt_core::{
    FontOptions, RemoteFiles, RemoteTarget, SearchCover, SearchDirection, SearchReport,
    SearchStatus, Session, SessionOptions, Settings, TabId, Theme, Wake,
};
use bt_core::{load_shell, smoke_shell};
use bt_gpu::{DisplayLink, GpuError, Layout, Pacer, Renderer, Stats, Surface, Waker};
use dispatch2::DispatchQueue;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject, Sel};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAlert, NSApplication, NSAutoresizingMaskOptions, NSBox, NSBoxType, NSButton, NSColor,
    NSControlTextEditingDelegate, NSEventModifierFlags, NSFont, NSLineBreakMode, NSMenuItem,
    NSPasteboard, NSPasteboardNameFind, NSPopoverDelegate, NSSearchFieldDelegate, NSTextField,
    NSTextFieldDelegate, NSTitlePosition, NSView, NSViewFrameDidChangeNotification,
};
use objc2_foundation::{
    NSNotification, NSNotificationCenter, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString,
    NSUUID, ns_string,
};
use objc2_quartz_core::CAMetalLayer;

use crate::app::{self, Grid, split_into_grid};
use crate::clipboard::{self, PendingCopy};
use crate::jobs::{self, Foreground, Probe, ShellParent, SystemTable};
use crate::notices::{Source, font_messages};
use crate::pacer::MacPacer;
use crate::preview::PreviewTicket;
use crate::promise::FinderDrops;
use crate::quote;
use crate::remote_helper::RemoteHelper;
use crate::search_bar::{SearchBar, selection_query};
use crate::upload::Transfers;
use crate::uploader::{StopSheet, UploadPopover};
use crate::view::BateriView;
use crate::window::{Closing, Launch, is_dark_background};
use crate::zoom::Zoom;
use crate::{Run, Workload};
use crate::{child, locale};

/// Events the pane hands to its owner (039 Karar 3) — today
/// `window::WindowHost`, tomorrow an embedding application.
///
/// All of them are called **on the main thread** and with the pane's id
/// ([`TerminalPane::id`]): the owner holds several panes (splits) and must
/// know which one the event came from. The methods carry no AppKit types, so
/// the owner can be tested with a fake application. Sheets and the popover
/// use the pane view's own `window()`; the owner is not asked for them.
pub(crate) trait PaneHost {
    /// Title, working directory, remote state or upload percentage changed:
    /// the window's title and the tab's dot must be re-read from the pane.
    fn title_changed(&self, pane: u64);
    /// The shell exited: the pane has nothing left to stand on and must close
    /// (026 → Karar 5) — only this pane, not the tab (039 Karar 8).
    fn shell_exited(&self, pane: u64);
    /// The keyboard arrived at this pane's terminal (`BateriView` became first
    /// responder): the focused pane is now this one — the title, the tab dot
    /// and the new split's inheritance come from it (039 Karar 11).
    fn focused(&self, pane: u64);
    /// The upload queue's progress or existence changed — the application's
    /// Dock icon is the total of all panes ([`TerminalPane::upload_totals`]).
    fn uploads_changed(&self, pane: u64);
    /// Notification to the user (upload finished, failed, connection lost).
    fn notify(&self, pane: u64, title: &str, body: &str);
    /// Subtitle notices (today only the font's, `sync_geometry`).
    fn post_notices(&self, pane: u64, source: Source, messages: Vec<String>);
    /// Remote copy (OSC 52). The default arm writes to the general pasteboard
    /// — Cmd-C's pasteboard; separating the pasteboard stays open as an owner
    /// decision.
    fn copy_to_clipboard(&self, _pane: u64, text: String) {
        clipboard::copy(&NSPasteboard::generalPasteboard(), Some(text));
    }
}

/// Column count of the smallest pane (039 Karar 14): a split that would drop
/// below it is not made. Not measured, a design constant — room for the
/// prompt's two columns, a short command and the folder name in the dock's
/// context line; narrower makes the shell's own line wrapping meaningless.
/// Tuned by eye.
const MIN_PANE_COLS: u16 = 20;

/// Row count of the smallest pane (039 Karar 14), grid rows **excluding** the
/// dock's reserve. Design constant: one command and a few lines of its
/// output; a full-screen program (vim, htop) can show nothing but a status
/// line below it.
const MIN_PANE_ROWS: u16 = 5;

/// Opacity of the unfocused pane's veil (039 Karar 7): the theme's
/// background overlays the text at this ratio. Not measured, a design
/// constant (like `GUTTER_PT`) — Ghostty's `unfocused-split-opacity` default
/// is `0.7`, i.e. a veil of `0.3`; the same ratio: focus reads at a glance
/// and the dimmed pane's text is still readable. Tuned by eye.
const DIM_ALPHA: f64 = 0.3;

define_class!(
    // SAFETY: NSBox is designed for subclassing; DimOverlay implements no
    // `Drop`, has no ivar and is born with NSBox's constructor (`new`).
    #[unsafe(super(NSBox))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriDimOverlay"]
    pub(crate) struct DimOverlay;

    unsafe impl NSObjectProtocol for DimOverlay {}

    impl DimOverlay {
        /// Never takes part in hit testing: clicks, drags and the wheel fall
        /// through to the `BateriView` underneath — clicking a dimmed pane
        /// focuses it (039 phase-3's click path) and the veil must not cut that.
        #[unsafe(method_id(hitTest:))]
        fn hit_test(&self, _point: NSPoint) -> Option<Retained<NSView>> {
            None
        }
    }
);

impl DimOverlay {
    /// Born hidden; the colour is [`DimOverlay::paint`], the visibility is the
    /// owner's ([`TerminalPane::set_dimmed`]).
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        // SAFETY: `NSBox`'s `init`; the subclass has no ivar.
        let this = Self::alloc(mtm).set_ivars(());
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        this.setBoxType(NSBoxType::Custom);
        this.setTitlePosition(NSTitlePosition::NoTitle);
        this.setBorderWidth(0.0);
        this.setHidden(true);
        this
    }

    /// The theme's background at [`DIM_ALPHA`] opacity. `NSColor` takes sRGB
    /// (`CLAUDE.md` → Renk uzayı; like the separator's `separator_srgb`).
    fn paint(&self, theme: &Theme) {
        let [r, g, b] = theme.background_srgb().map(|byte| f64::from(byte) / 255.0);
        self.setFillColor(&NSColor::colorWithSRGBRed_green_blue_alpha(
            r, g, b, DIM_ALPHA,
        ));
    }
}

/// The target label's distance from the pane's bottom-left corner and the
/// text's inset inside it, in points. Design constants (not measured) — a
/// browser's status bubble.
const LINK_LABEL_MARGIN: f64 = 6.0;
const LINK_LABEL_PAD_X: f64 = 6.0;
const LINK_LABEL_PAD_Y: f64 = 2.0;

define_class!(
    // SAFETY: NSBox is designed for subclassing; LinkLabel implements no
    // `Drop`, has no ivar and is born with NSBox's constructor (`new`).
    #[unsafe(super(NSBox))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriLinkLabel"]
    pub(crate) struct LinkLabel;

    unsafe impl NSObjectProtocol for LinkLabel {}

    impl LinkLabel {
        /// Never takes part in hit testing ([`DimOverlay`]'s rule): the label
        /// sits over the dock's context line and a click there must reach the
        /// `BateriView` underneath.
        #[unsafe(method_id(hitTest:))]
        fn hit_test(&self, _point: NSPoint) -> Option<Retained<NSView>> {
            None
        }
    }
);

impl LinkLabel {
    /// The ⌘-hovered OSC 8 link's target (044 Karar 7): a small box in the
    /// pane's bottom-left corner, born hidden, its single child the text. The
    /// text is the whole target, cut in the **middle** when it does not fit —
    /// the scheme and host on the left and the file name on the right are what
    /// tells a link apart.
    fn new(mtm: MainThreadMarker) -> (Retained<Self>, Retained<NSTextField>) {
        // SAFETY: `NSBox`'s `init`; the subclass has no ivar.
        let this = Self::alloc(mtm).set_ivars(());
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        this.setBoxType(NSBoxType::Custom);
        this.setTitlePosition(NSTitlePosition::NoTitle);
        this.setBorderWidth(1.0);
        this.setCornerRadius(4.0);
        this.setHidden(true);
        this.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewMaxXMargin | NSAutoresizingMaskOptions::ViewMaxYMargin,
        );
        let text = NSTextField::labelWithString(ns_string!(""), mtm);
        text.setFont(Some(&NSFont::systemFontOfSize(
            NSFont::smallSystemFontSize(),
        )));
        text.setTextColor(Some(&NSColor::secondaryLabelColor()));
        text.setLineBreakMode(NSLineBreakMode::ByTruncatingMiddle);
        this.addSubview(&text);
        (this, text)
    }

    /// The theme's background and separator tone (sRGB, `DimOverlay::paint`'s
    /// rule): the label reads as the terminal's own surface.
    fn paint(&self, theme: &Theme) {
        let srgb = |[r, g, b]: [u8; 3]| {
            NSColor::colorWithSRGBRed_green_blue_alpha(
                f64::from(r) / 255.0,
                f64::from(g) / 255.0,
                f64::from(b) / 255.0,
                1.0,
            )
        };
        self.setFillColor(&srgb(theme.background_srgb()));
        self.setBorderColor(&srgb(theme.separator_srgb()));
    }
}

/// The path by which main-queue returns find the pane by id; the owner
/// supplies it (today `app::pane_by_id`). A plain `fn` pointer, not a
/// closure: `Send` and `Copy`, so every job thrown from the reader thread to
/// the main queue can capture it and it opens no reference cycle. It must not
/// find a pane whose close has begun ([`TerminalPane::is_closed`]).
pub(crate) type PaneLookup = fn(MainThreadMarker, u64) -> Option<Retained<TerminalPane>>;

/// The pane's birth package (039 Karar 3): all inputs in a single struct,
/// from the owner. Live changes go a separate way, through the pane's `set_*`
/// methods.
pub(crate) struct PaneLaunch {
    /// In-process identity ([`TerminalPane::id`]).
    pub(crate) id: u64,
    /// Timed-run recipe; `None` → interactive.
    pub(crate) run: Option<Run>,
    /// Owner of the events.
    pub(crate) host: Rc<dyn PaneHost>,
    /// The path by which main-queue returns find the pane.
    pub(crate) lookup: PaneLookup,
    /// Measurement ledger (the timed run's `BT_FRAME_STATS`), goes to the link.
    pub(crate) stats: Option<Arc<Stats>>,
    /// Copy of the settings at birth.
    pub(crate) settings: Settings,
    /// The session's theme.
    pub(crate) theme: Theme,
    /// Start directory and first input.
    pub(crate) launch: Launch,
    /// Shell integration's environment and the dock reserve — from **a single
    /// question** (`AppDelegate::shell_integration`): two separate calls could diverge.
    pub(crate) integration: (Vec<(String, String)>, u16),
    /// Resolved value of Reduce Motion.
    pub(crate) reduce_motion: bool,
    /// Resolved mode of the wheel.
    pub(crate) smooth_scroll: bool,
    /// Inherited temporary point-size delta (026 → Karar 3).
    pub(crate) zoom: Zoom,
}

/// The half of the birth package that only [`TerminalPane::start`] consumes.
struct Birth {
    stats: Option<Arc<Stats>>,
    settings: Settings,
    theme: Theme,
    launch: Launch,
    integration: (Vec<(String, String)>, u16),
}

/// Main-queue half of the title notification: the flag drops **before every
/// read** (a change arriving after the read wants a new job and is not
/// missed), then the pane's own edge (`edge`: the upload queue's connection)
/// and the owner re-read the title. The edge must also come after the flag:
/// an ssh that ends in between spawns no job and the queue would stay on a
/// dead connection (`/code-review`). `swap`, because the read-modify-write
/// pairs with the writer's `swap` and makes what it wrote to the slot visible.
fn announce_title(pending: &AtomicBool, edge: impl FnOnce(), host: &dyn PaneHost, pane: u64) {
    pending.swap(false, Ordering::AcqRel);
    edge();
    host.title_changed(pane);
}

/// Main-queue half of the OSC 52 copy: the text in the slot goes to the owner
/// ([`PaneHost::copy_to_clipboard`]); if the slot is empty (another job took
/// it in a race) there is no event.
fn announce_copy(pending: &PendingCopy, host: &dyn PaneHost, pane: u64) {
    if let Some(text) = pending.take() {
        host.copy_to_clipboard(pane, text);
    }
}

/// Text on the system's find pasteboard (033 Karar 6: ⌘E's cross-application
/// norm), through the same filter as ⌘E's query: its first line, `None` if
/// empty or only whitespace ([`selection_query`]).
fn find_pasteboard_text() -> Option<String> {
    // SAFETY: a constant name AppKit exposes, lives for the whole process.
    let name = unsafe { NSPasteboardNameFind };
    clipboard::read(&NSPasteboard::pasteboardWithName(name))
        .and_then(|text| selection_query(&text, false))
}

/// The six items that are greyed out on the alternate screen (034 Karar 2):
/// the two clear modes and the four scrolls — all touch the primary
/// scrollback and that scrollback is unreachable on the alternate screen.
fn is_scrollback_action(action: Sel) -> bool {
    [
        sel!(clearToStart:),
        sel!(clearScrollback:),
        sel!(scrollToTop:),
        sel!(scrollToBottom:),
        sel!(scrollPageUp:),
        sel!(scrollPageDown:),
    ]
    .contains(&action)
}

/// `bt-core`'s wake end — one per pane, together with its session.
///
/// `Session::spawn` asks for the `Wake` **before** the link, while the
/// `Waker` is born after the link; the slot's `None` closes the gap. No
/// frame is lost: the opening frame is requested by hand anyway and every
/// byte read up to that point has accumulated in the damage flag.
struct ShellWake {
    /// The pane's id: main-queue jobs find the pane with it
    /// ([`PaneLookup`], the pattern of the alternate-screen notifier) —
    /// holding a reference to the `Session` or the pane would close
    /// `wake.rs`'s ownership cycle.
    id: u64,
    /// The path from id to pane, from the owner ([`PaneLaunch::lookup`]).
    lookup: PaneLookup,
    /// Whether this is a timed run: `child_exit` splits into two paths
    /// ([`Wake::child_exit`]'s body). A digest of the recipe in the birth
    /// package (`PaneLaunch::run`); the reader thread cannot reach the pane.
    timed: bool,
    /// The link's `Waker` — under a **leaf lock** and **detachable**.
    ///
    /// When the pane closes it is `take()`n on the main thread
    /// ([`ShellWake::detach`]): the last copy of this object can drop on the
    /// `"PTY teardown"` thread (`wake.rs` → ownership) and if the `Waker`'s
    /// `MainThreadBound` dropped there, its `Drop` would throw a synchronous
    /// job at the main queue. A detached slot pins that `Drop` to the main
    /// thread structurally; before, the only protection was the window list
    /// outliving `app.run()`.
    ///
    /// The lock is a leaf: `wake()` takes it under the `Term` lock and
    /// releases it, and no other lock is taken under it (like `Theme`'s leaf
    /// lock).
    waker: Mutex<Option<Waker>>,
    /// Text OSC 52 has pending for the main queue. `Arc`, because the main
    /// queue's job wants `'static` and `Wake`'s call only gives `&self`; the
    /// job holds not `ShellWake` but only the slot.
    pending_copy: Arc<PendingCopy>,
    /// Whether the title job is waiting on the main queue — **at most one**
    /// job in the queue (`PendingCopy`'s pattern, a flag instead of a
    /// payload: the title itself is in the session, the job reads it).
    title_pending: Arc<AtomicBool>,
    /// Whether the search count's scrollback news is waiting on the main
    /// queue — `title_pending`'s twin (033).
    search_pending: Arc<AtomicBool>,
    /// The remote-session probe's arm and pending job (036); `Arc`, because
    /// the main queue's job holds it.
    remote_probe: Arc<RemoteProbe>,
    /// Whether the stale-link news is waiting on the main queue (044 R4.1) —
    /// `search_pending`'s twin: at most one job.
    link_pending: Arc<AtomicBool>,
}

/// The two bits of the remote-session probe (036 Karar 2): the **arm** (no
/// definitive answer yet for this command) and the **pending job** (a probe is
/// in the main queue — at most one, `title_pending`'s pattern).
///
/// The arm is set on the `C` edge; while set, every `wake` (output from the
/// PTY) throws a job, a definitive answer drops it and later output does not
/// probe. The cost of a running `cat` is a single probe.
///
/// **The job drops the arm before probing**, not after, and re-arms on an
/// undecided answer: while a probe runs, a new `C` on the reader thread can
/// set the arm and throw a new job, and if the ending old command's definitive
/// answer dropped it the new command would never be probed.
#[derive(Debug, Default)]
struct RemoteProbe {
    armed: AtomicBool,
    pending: AtomicBool,
}

impl RemoteProbe {
    /// The `C` edge: sets the arm; `true` if a job is to be thrown at the main queue.
    fn command_started(&self) -> bool {
        self.armed.store(true, Ordering::Release);
        self.claim()
    }

    /// The output edge (reader thread, possibly under the `Term` lock): `true`
    /// if the arm is set and no job is waiting. A single atomic read when unarmed.
    fn output(&self) -> bool {
        self.armed.load(Ordering::Acquire) && self.claim()
    }

    /// Takes the pending job's slot; `false` if one is already pending.
    fn claim(&self) -> bool {
        !self.pending.swap(true, Ordering::AcqRel)
    }

    /// The head of the main-queue job: releases the slot and drops the arm; if
    /// the arm is not set (a definitive answer was given) there is no probe.
    fn begin(&self) -> bool {
        self.pending.store(false, Ordering::Release);
        self.armed.swap(false, Ordering::AcqRel)
    }

    /// Undecided answer: the arm is set back, no job is thrown — the next
    /// output throws one.
    fn rearm(&self) {
        self.armed.store(true, Ordering::Release);
    }
}

impl ShellWake {
    /// Throws the remote-session probe to the main queue ([`RemoteProbe`]).
    fn dispatch_remote_probe(&self) {
        let probe = Arc::clone(&self.remote_probe);
        let (id, lookup) = (self.id, self.lookup);
        DispatchQueue::main().exec_async(move || {
            if !probe.begin() {
                return;
            }
            // audit: a block running on the main queue is on the main thread by definition.
            let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
            // If the pane closed in the meantime there is no shell to probe.
            let Some(pane) = lookup(mtm, id) else {
                return;
            };
            let outcome = pane.probe_remote();
            // The remote state's edge is the edge of the upload queue, the
            // window title and the tab's dot ([`TerminalPane::remote_or_title_changed`]).
            if outcome.changed {
                pane.remote_or_title_changed();
            }
            if outcome.undecided {
                probe.rearm();
            }
        });
    }

    /// Takes the leaf lock; if poisoned it continues with what is inside — the
    /// slot's only invariant is "either a `Waker` exists or not" and it cannot
    /// have a half-written state.
    fn slot(&self) -> MutexGuard<'_, Option<Waker>> {
        self.waker.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Detaches the `Waker` from the slot and hands it to the caller — so that
    /// it drops on the main thread ([`ShellWake::waker`]). The second call is `None`.
    fn detach(&self) -> Option<Waker> {
        self.slot().take()
    }
}

impl Wake for ShellWake {
    fn wake(&self) {
        // Reader thread; the `Term` lock may be held. One job: throw the
        // "open the link" job to the main queue and return at once.
        // The waker is **not copied**, it is called under the lock: a copy
        // could end up as the last reference here and drop on the reader
        // thread — the very path detaching closed.
        if let Some(waker) = self.slot().as_ref() {
            waker.wake();
        }
        // If the remote-session probe stayed undecided this output re-triggers
        // it (036 Karar 2); when unarmed the cost is one atomic read.
        if self.remote_probe.output() {
            self.dispatch_remote_probe();
        }
    }

    fn child_exit(&self, _code: Option<i32>) {
        // The shell is gone, the pane has nothing to stand on: **that window**
        // closes (026 → Karar 5), not the application. Closing goes through the
        // window's own `windowWillClose:` — the red button, ⌘W and `exit` reach
        // the same sequence.
        //
        // **In a timed run** the old path: directly `terminate:`. The report
        // reads the window's counters and if the window dropped off the list
        // before the report, when the smoke recipe ended shorter than the
        // deadline the report would run with an empty list (`will_terminate`'s doc).
        //
        // It is thrown to the main queue for two reasons and both are required:
        // AppKit wants the main thread, and this call arrives **on the reader
        // thread** — a synchronous path to closing would make the reader thread
        // wait on its own closing (`wake.rs` → ownership).
        //
        // **Known limit:** the shell's last output may not reach the screen.
        // alacritty orders it `ChildExit` → `Wakeup`, so when we get here the
        // last byte may not have been drawn yet; and closing runs without a
        // vsync in between. Guaranteeing it would be either a magic delay or
        // adding "damage exhausted, exit now" semantics to the display link —
        // the second would put terminal knowledge into the renderer. When the
        // `bateri -e cmd` path arrives it will be designed together with
        // `drain_on_exit` (`.tasks/002-vt-motoru/phase-4.md` → Uygulama Notları).
        let (timed, id, lookup) = (self.timed, self.id, self.lookup);
        DispatchQueue::main().exec_async(move || {
            // audit: a block running on the main queue is on the main thread by definition.
            let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
            if timed {
                NSApplication::sharedApplication(mtm).terminate(None);
                return;
            }
            // If the pane closed in the meantime (⌘W's `SIGHUP` killed the
            // shell and the news came later) there is nothing to close. Closing
            // is the owner's job ([`PaneHost::shell_exited`]).
            if let Some(pane) = lookup(mtm, id) {
                pane.host().shell_exited(id);
            }
        });
    }

    fn copy_to_clipboard(&self, text: String) {
        // Reader thread, the `Term` lock may be held: the text goes to the
        // lock-free slot, at most **one** job to the main queue (`PendingCopy`'s
        // doc). If there is text waiting in the slot the job that will take it
        // is already in the queue.
        //
        // The owner chooses the pasteboard ([`PaneHost::copy_to_clipboard`];
        // default the general pasteboard, the same as Cmd-C's); the job's order
        // is for the same reason as `child_exit`'s: the main queue. If the pane
        // closed in the meantime the text is dropped.
        if self.pending_copy.put(text) {
            let pending = Arc::clone(&self.pending_copy);
            let (id, lookup) = (self.id, self.lookup);
            DispatchQueue::main().exec_async(move || {
                // audit: a block running on the main queue is on the main thread by definition.
                let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
                if let Some(pane) = lookup(mtm, id) {
                    announce_copy(&pending, pane.host(), id);
                }
            });
        }
    }

    fn title_changed(&self) {
        // Reader thread (or the settings watcher's thread), the `Term` lock
        // may be held: set the flag, return if a job is already waiting.
        if self.title_pending.swap(true, Ordering::AcqRel) {
            return;
        }
        let pending = Arc::clone(&self.title_pending);
        let (id, lookup) = (self.id, self.lookup);
        DispatchQueue::main().exec_async(move || {
            // audit: a block running on the main queue is on the main thread by definition.
            let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
            // If the pane closed in the meantime there is no title to write either
            // (the flag stays set; a closed pane's news is dropped anyway).
            if let Some(pane) = lookup(mtm, id) {
                // The remote state's edge (`D`/`A`'s deletion) is also the
                // upload queue's edge — it first, then the owner reads the title.
                announce_title(&pending, || pane.check_upload_connection(), pane.host(), id);
            }
        });
    }

    fn search_changed(&self) {
        // Reader thread, the `Term` lock may be held (or the main thread's
        // `resize`): `title_changed`'s pattern — at most one job in the main
        // queue. The core already reports on the edge; this flag folds the
        // second of two reports while the job waits in the queue.
        if self.search_pending.swap(true, Ordering::AcqRel) {
            return;
        }
        let pending = Arc::clone(&self.search_pending);
        let (id, lookup) = (self.id, self.lookup);
        DispatchQueue::main().exec_async(move || {
            pending.swap(false, Ordering::AcqRel);
            // audit: a block running on the main queue is on the main thread by definition.
            let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
            // Also works in a background tab: the news is not tied to the frame
            // path. If the pane closed in the meantime there is nothing to count.
            if let Some(pane) = lookup(mtm, id) {
                pane.kick_search();
            }
        });
    }

    fn command_started(&self) {
        // Reader thread, lock-free. A timed run does not detect: its tokens
        // must stay as today (and the fixed script has no integration either).
        if self.timed {
            return;
        }
        if self.remote_probe.command_started() {
            self.dispatch_remote_probe();
        }
    }

    fn link_hover_lost(&self) {
        // The frame path (main thread, after the `Term` lock): the hover's stamp
        // went stale and the slot was dropped. The view re-finds the link if ⌘
        // is still down (`BateriView::link_lost`) — on the next main-queue turn,
        // not inside the frame; at most one job (`search_changed`'s pattern).
        if self.link_pending.swap(true, Ordering::AcqRel) {
            return;
        }
        let pending = Arc::clone(&self.link_pending);
        let (id, lookup) = (self.id, self.lookup);
        DispatchQueue::main().exec_async(move || {
            pending.swap(false, Ordering::AcqRel);
            // audit: a block running on the main queue is on the main thread by definition.
            let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
            if let Some(pane) = lookup(mtm, id) {
                pane.view().link_lost();
            }
        });
    }
}

/// `bt-gpu`'s alternate-screen notifier: throws the work **to the main queue**.
///
/// Its call comes from the frame path, i.e. already from the main thread —
/// the queue is not for a thread hop but to **defer by one turn**: at the
/// moment of the call the frame has been drawn and changing the window
/// geometry (drawable size, grid, `DisplayLink` layout) there would pull the
/// rug from under the drawn frame.
///
/// **Pane id, not a targetless action.** The responder chain goes to the key
/// window: exiting vim in a background tab would resize the wrong pane. The
/// job captures the id (`id`), finds the pane through the owner's path
/// ([`PaneLookup`]) and drops if it cannot — if the pane closed in the
/// meantime there is nothing to resize either.
///
/// The only thing it captures is an integer. The reason for the old "captures
/// nothing" rule was a **reference cycle** (`DisplayLink` sits in the pane's
/// ivar, a closure holding the pane would tie it to itself); an integer opens
/// no cycle. Besides, `exec_async` wants `Send` and the pane object is pinned
/// to the main thread — there was nothing else it could hold anyway.
///
/// It carries no payload: the receiver re-reads the truth
/// ([`TerminalPane::alt_screen_did_change`]), so two transitions chasing each
/// other (vim open-close) cannot act on a stale value.
fn alt_screen_notifier(id: u64, lookup: PaneLookup) -> Box<dyn Fn()> {
    Box::new(move || {
        DispatchQueue::main().exec_async(move || {
            // audit: a block running on the main queue is on the main thread by definition.
            let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
            if let Some(pane) = lookup(mtm, id) {
                pane.alt_screen_did_change();
            }
        });
    })
}

/// Result of the remote-session probe ([`TerminalPane::probe_remote`]): two
/// separate answers, because their consumers are separate — undecidedness
/// re-arms (the pane's job), a change in the remote state refreshes the title
/// and the tab's dot (the window's job).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct RemoteProbeOutcome {
    /// The answer is undecided: the arm stays set, the next output probes again.
    pub(crate) undecided: bool,
    /// The session's remote state changed (the return of `Session::set_remote`).
    pub(crate) changed: bool,
}

/// The pane's state. `OnceCell`: the session and link are born once inside
/// `start`, then only read. The view, surface and renderer are born in the
/// constructor.
///
/// **The window is not held here**: the pane is the window's `contentView`,
/// i.e. the window holds it strongly and a back reference would be a cycle.
/// The window is looked at with `NSView::window` whenever needed (scale, key bit).
pub(crate) struct PaneIvars {
    /// Our own counter ([`AppDelegate`] hands it out, from the same counter as
    /// the windows'): the key by which jobs returning from the reader thread
    /// to the main queue find the pane (`AppDelegate::pane`).
    id: u64,
    /// The timed run's recipe, from the birth package (`Copy`): the focus path
    /// asks it on every application switch ([`TerminalPane::apply_focus`]).
    run: Option<Run>,
    /// Owner of the events ([`PaneHost`]).
    host: Rc<dyn PaneHost>,
    /// The path by which main-queue returns find the pane ([`PaneLookup`]).
    lookup: PaneLookup,
    /// The half of the birth package that `start` consumes; `None` after `start`.
    birth: RefCell<Option<Birth>>,
    /// The settings' font — the point-size delta is applied to it
    /// ([`TerminalPane::change_zoom`]); its live change is [`TerminalPane::set_font`].
    font: RefCell<FontOptions>,
    /// Resolved value of Reduce Motion — the search panel's animation looks
    /// at it too; its live change is [`TerminalPane::set_reduce_motion`].
    reduce_motion: Cell<bool>,
    /// Resolved mode of the wheel — the search's trip to a match looks at it
    /// too; its live change is [`TerminalPane::set_smooth_scroll`].
    smooth_scroll: Cell<bool>,
    /// `Rc`: the renderer is pinned to the main thread (see `bt_gpu::DisplayLink`)
    /// and the link holds a copy too.
    renderer: Rc<Renderer>,
    /// The terminal view's layer — this pane owns it (040 → Karar 8): it is
    /// hung on the view here and its scale is set from the window
    /// (`sync_geometry`); `bt-gpu` draws into it through [`Surface`].
    layer: Retained<CAMetalLayer>,
    /// The wgpu surface over `layer`; shared with the link, which acquires
    /// each frame's texture from it.
    surface: Rc<Surface>,
    /// The inputs of the mouse translation are refreshed with the pane's size
    /// (`set_metrics`); this view is also the source of the geometry (`sync_geometry`).
    view: Retained<BateriView>,
    /// The unfocused pane's dim veil (039 Karar 7): the pane's topmost child,
    /// a sibling of the Metal layer — it is not in the frame path, its
    /// composition is CoreAnimation's. The owner determines its visibility.
    dim: Retained<DimOverlay>,
    /// The ⌘-hovered OSC 8 link's target (044 Karar 7) and its text: above the
    /// terminal and the search panel, below the dim veil. AppKit's, outside the
    /// frame path; shown by [`TerminalPane::set_link_target`].
    link_label: (Retained<LinkLabel>, Retained<NSTextField>),
    link: OnceCell<DisplayLink>,
    /// The second step of the closing sequence is called from here; `DisplayLink`
    /// holds a copy too but reaching there after `stop()` would be wrong.
    session: OnceCell<Arc<Session>>,
    /// Whether the shell is the PTY's child or its child's child — written at
    /// the same moment as the session, **from the command**
    /// ([`TerminalPane::start_session`]); detecting the running job finds the
    /// shell with it ([`TerminalPane::foreground`]).
    shell_parent: OnceCell<ShellParent>,
    wake: Arc<ShellWake>,
    /// Cmd +/−/0's temporary point-size delta — **this pane's**: the font that
    /// goes to the renderer is `zoom.apply(&settings.font)` ([`TerminalPane::apply_font`]).
    /// Reset when `size` in the file changes ([`TerminalPane::zoom_after_reload`]).
    zoom: Cell<Zoom>,
    /// How many rows the dock has; `0` → this pane has no dock.
    ///
    /// **Decided when the session is born** (012 → R5.1): the source is
    /// whether the integration was installed and it is asked **once** in
    /// [`TerminalPane::start`]. The slot exists for that reason: `sync_geometry`
    /// runs on every geometry event and must know the answer when computing the
    /// grid height; asking a second time would mean, in a future where the two
    /// calls can diverge, "the pane lost two rows but there is no dock".
    ///
    /// Consequence: the smoke recipe running `/bin/sh` **does not get** a dock,
    /// so `smoke_shell` and the `cells=8 glyphs=6 rules=15` contract tied to it
    /// stay untouched.
    ///
    /// `Cell`, not `OnceCell`: its pre-launch value is `0` and that is the
    /// **right** answer (no session yet, no first frame either); `OnceCell`
    /// would close this path with an `unwrap`.
    ///
    /// **This field is the current reserve**, not the birth value: it drops to
    /// zero on the alternate screen and comes back on exit (R5.2). The birth
    /// value is in a separate field ([`PaneIvars::dock_rows_at_birth`]) and
    /// keeping the two apart is required — otherwise leaving the alternate
    /// screen would conjure a dock in a pane that never had one.
    dock_rows: Cell<u16>,
    /// The dock reserve decided when the session is born: `DOCK_ROWS` if the
    /// integration was installed, `0` if not (R5.1).
    ///
    /// It does **not move** during the run; this is the value the alternate
    /// screen will bring back and its only writer is the session's birth.
    dock_rows_at_birth: Cell<u16>,
    /// The session's persistent identity (038, 039 Karar 10): goes to the
    /// shell as `TERM_SESSION_ID` and `BATERI_TAB_URL`, `bateri://tab/<id>`
    /// finds the pane with it. Constant for the pane's lifetime; the
    /// in-process [`id`] is a separate thing (the key of main-queue returns).
    tab_id: TabId,
    /// Closing has begun ([`TerminalPane::begin_close`]): the pane's window
    /// leaves the list one turn later (`forget_window`) and in the meantime
    /// `AppDelegate::pane` must not find it — so that a stale report from the
    /// reader thread does not act on a closed session and `bateri://tab/`
    /// does not bring a sessionless window to the screen (`/code-review`, 038).
    closed: Cell<bool>,
    /// The scrollback search's panel (033) — born on the first ⌘F: a pane
    /// that never searches carries no views. The query and keys are in the
    /// panel, i.e. **per pane**, and are not forgotten on close (Karar 6).
    search: OnceCell<SearchBar>,
    /// The state of the last query given to the session — the label's input.
    search_status: Cell<SearchStatus>,
    /// Whether the count index's driver is waiting one turn in the main queue
    /// ([`TerminalPane::kick_search`]): so that a second driver is not set up.
    search_driving: Cell<bool>,
    /// Upload of a Finder drop to the remote directory (037 Karar 7): queue,
    /// progress and result line ([`crate::upload::Transfers`]). The queue is
    /// **this pane's ssh connection's** — switching to another tab does not stop it.
    uploads: RefCell<Transfers>,
    /// The open upload sheet (confirmation or error): lives for the sheet's duration.
    upload_alert: RefCell<Option<Retained<NSAlert>>>,
    /// The open stop question (037 phase-7, [`crate::uploader`]).
    upload_stop: RefCell<Option<StopSheet>>,
    /// The open "Show files (N)" popover (037 phase-7).
    upload_list: RefCell<Option<UploadPopover>>,
    /// Time of the event that closed the popover (`popoverWillClose:`): so that
    /// pressing the button again does not reopen the popover.
    list_closed_at: Cell<Option<f64>>,
    /// The helper ssh session that verifies remote links and counts a download
    /// (045 Karar 10): its worker is born at the first question, its session
    /// closes on another generation, when idle and with the pane.
    remote_helper: RefCell<RemoteHelper>,
    /// `[remote]`'s preview and download keys (045 R8): from the birth package,
    /// refreshed live with the host marks ([`TerminalPane::set_host_marks`]).
    remote_files: RefCell<RemoteFiles>,
    /// The previews this pane downloaded, by landing path (045 phase-4): how each
    /// opens and where its index is ([`crate::preview::PreviewTicket`]).
    previews: RefCell<HashMap<PathBuf, PreviewTicket>>,
    /// The file promises of ⌘-dragged remote links (045 phase-5): the delegates
    /// kept alive and the Finder downloads that fulfil them.
    finder: RefCell<FinderDrops>,
}

define_class!(
    // SAFETY: NSView is designed for subclassing; TerminalPane implements no `Drop`
    // and offers no constructor besides `initWithFrame:`.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriTerminalPane"]
    #[ivars = PaneIvars]
    pub(crate) struct TerminalPane;

    unsafe impl NSObjectProtocol for TerminalPane {}

    impl TerminalPane {
        /// The terminal view's frame changed (`NSViewFrameDidChangeNotification`,
        /// the observer is set up in [`TerminalPane::observe_frame`]).
        ///
        /// The source is **not** `windowDidResize:`, because the content changes
        /// without the window size changing too: when a second tab opens the tab
        /// bar enters the title area and the content shortens, when only one tab
        /// is left the bar goes away and the content lengthens — the window's
        /// frame is the same in both. Bound to the window notification the
        /// drawable stayed at the old size, the layer **stretched** it to the new
        /// size and the text blurred vertically (measured, 026 phase-4
        /// Uygulama Notları). The view's notification also covers window
        /// resizing, so a single source.
        #[unsafe(method(viewFrameDidChange:))]
        fn view_frame_did_change(&self, _n: &NSNotification) {
            self.refresh_geometry();
        }
    }

    /// Closing of the "Show files (N)" popover (037 phase-7): AppKit also
    /// closes a `transient` popover (click outside) and the button's pressed
    /// tone and the Esc monitor must go away then too.
    unsafe impl NSPopoverDelegate for TerminalPane {
        #[unsafe(method(popoverWillClose:))]
        fn popover_will_close(&self, _n: &NSNotification) {
            self.upload_list_will_close();
        }

        #[unsafe(method(popoverDidClose:))]
        fn popover_did_close(&self, _n: &NSNotification) {
            self.close_upload_list();
        }
    }

    // The search field's delegate (033): all methods of all three protocols
    // are optional; the ones used are in the `impl` below.
    unsafe impl NSControlTextEditingDelegate for TerminalPane {}
    unsafe impl NSTextFieldDelegate for TerminalPane {}
    unsafe impl NSSearchFieldDelegate for TerminalPane {}

    // **Pane-level menu selectors** (039 Karar 2): each is a one-line wrapper
    // around a named method (R2.3) — an owner without a menu can call the
    // same method directly. The responder chain of a targetless action is
    // `BateriView` → pane → window → delegate, and while the search field has
    // focus field editor → field → … → pane; so the item reaches the focused
    // pane and 033's "while the field has focus the chain does not pass
    // through `BateriView`" reason is moot. Application-wide ones
    // (`settingsDidChange:`, theme) are in `AppDelegate`, tab jobs
    // (`closeTab:`, `selectTab:`) in the window.
    impl TerminalPane {
        /// View ▸ Bigger (Cmd +).
        #[unsafe(method(makeFontBigger:))]
        fn make_font_bigger(&self, _sender: Option<&AnyObject>) {
            self.zoom_in();
        }

        /// View ▸ Smaller (Cmd −).
        #[unsafe(method(makeFontSmaller:))]
        fn make_font_smaller(&self, _sender: Option<&AnyObject>) {
            self.zoom_out();
        }

        /// View ▸ Actual Size (Cmd 0).
        #[unsafe(method(resetFontSize:))]
        fn reset_font_size(&self, _sender: Option<&AnyObject>) {
            self.zoom_reset();
        }

        /// Edit ▸ Find ▸ Find… (⌘F).
        ///
        /// The selectors are **our own names**, not `performFindPanelAction:`
        /// (033 Karar 10): while the field has focus the first responder is
        /// AppKit's field editor and it would implement that selector itself and swallow it.
        #[unsafe(method(findInScrollback:))]
        fn find_in_scrollback(&self, _sender: Option<&AnyObject>) {
            self.find();
        }

        /// Edit ▸ Find ▸ Find Next (⌘G) and the panel's up arrow.
        #[unsafe(method(findNextMatch:))]
        fn find_next_match(&self, _sender: Option<&AnyObject>) {
            self.find_next();
        }

        /// Edit ▸ Find ▸ Find Previous (⇧⌘G) and the panel's down arrow.
        #[unsafe(method(findPreviousMatch:))]
        fn find_previous_match(&self, _sender: Option<&AnyObject>) {
            self.find_previous();
        }

        /// Edit ▸ Find ▸ Use Selection for Find (⌘E).
        #[unsafe(method(useSelectionForFind:))]
        fn use_selection_for_find_action(&self, _sender: Option<&AnyObject>) {
            self.use_selection_for_find();
        }

        /// Edit ▸ Clear to Start (⌘K).
        #[unsafe(method(clearToStart:))]
        fn clear_to_start_action(&self, _sender: Option<&AnyObject>) {
            self.clear_to_start();
        }

        /// Edit ▸ Clear Scrollback (⌥⌘K).
        #[unsafe(method(clearScrollback:))]
        fn clear_scrollback_action(&self, _sender: Option<&AnyObject>) {
            self.clear_scrollback();
        }

        /// View ▸ Scroll to Top (⌘Home).
        #[unsafe(method(scrollToTop:))]
        fn scroll_to_top_action(&self, _sender: Option<&AnyObject>) {
            self.scroll_to_top();
        }

        /// View ▸ Scroll to Bottom (⌘End).
        #[unsafe(method(scrollToBottom:))]
        fn scroll_to_bottom_action(&self, _sender: Option<&AnyObject>) {
            self.scroll_to_bottom();
        }

        /// View ▸ Page Up (⌘PgUp).
        #[unsafe(method(scrollPageUp:))]
        fn scroll_page_up_action(&self, _sender: Option<&AnyObject>) {
            self.page_up();
        }

        /// View ▸ Page Down (⌘PgDn).
        #[unsafe(method(scrollPageDown:))]
        fn scroll_page_down_action(&self, _sender: Option<&AnyObject>) {
            self.page_down();
        }

        /// The panel's close button — the same path as Esc (033 Karar 5).
        #[unsafe(method(closeSearch:))]
        fn close_search_action(&self, _sender: Option<&AnyObject>) {
            self.close_search();
        }

        /// The field's action: every text change (`sendsSearchStringImmediately`)
        /// and the ⊗ button.
        #[unsafe(method(searchFieldChanged:))]
        fn search_field_changed(&self, _sender: Option<&AnyObject>) {
            self.apply_search();
        }

        /// The `Aa` or `.*` switch changed.
        #[unsafe(method(searchOptionsChanged:))]
        fn search_options_changed(&self, _sender: Option<&AnyObject>) {
            self.apply_search();
        }

        /// The field's command hook (033 Karar 10): ⏎ the previous (older), ⇧⏎
        /// the next (newer) match; Esc closes the panel — instead of
        /// `NSSearchField`'s "clear the text" default. The remaining commands
        /// go to the field itself (`false`).
        ///
        /// Shift cannot be read from the selector — both keys are
        /// `insertNewline:` — so it is read from the event itself.
        #[unsafe(method(control:textView:doCommandBySelector:))]
        fn control_do_command(
            &self,
            _control: &AnyObject,
            _text_view: &AnyObject,
            command: Sel,
        ) -> bool {
            if command == sel!(insertNewline:) {
                let shift = NSApplication::sharedApplication(self.mtm())
                    .currentEvent()
                    .is_some_and(|event| event.modifierFlags().contains(NSEventModifierFlags::Shift));
                self.search_step(if shift {
                    SearchDirection::Newer
                } else {
                    SearchDirection::Older
                });
                true
            } else if command == sel!(cancelOperation:) {
                self.close_search();
                true
            } else {
                false
            }
        }

        /// Enabled state of the Find items, clearing, scrolling and upload
        /// cancel; **an unknown item is `true`** — point size is always enabled.
        /// Clearing and scrolling are greyed out on the alternate screen (034
        /// Karar 2): the primary scrollback is unreachable there, a grey item is
        /// an honest "not here"; they are grey without a session too.
        #[unsafe(method(validateMenuItem:))]
        fn validate_menu_item(&self, item: &NSMenuItem) -> bool {
            let action = item.action();
            // No `return`: `define_class!` converts the `bool` at the end of the body.
            if action.is_some_and(is_scrollback_action) {
                self.session()
                    .is_some_and(|session| !session.alt_screen())
            } else if action == Some(sel!(findNextMatch:)) || action == Some(sel!(findPreviousMatch:)) {
                self.has_query()
            } else if action == Some(sel!(useSelectionForFind:)) {
                self.session()
                    .is_some_and(|session| session.has_selection())
            } else if action == Some(sel!(cancelUpload:)) {
                // ⌘. only while this pane has a queue (037 Karar 7); the grey
                // item's shortcut falls to `keyDown:` and is swallowed there.
                self.ivars().uploads.borrow().active()
            } else {
                true
            }
        }

        /// Shell ▸ Cancel Upload (⌘.) and the popover's `Cancel all ⌘.`: this
        /// pane's **whole** upload queue (037 Karar 7 → Kullanıcı kararı 5);
        /// if the flowing item has gone past 30 seconds it asks first
        /// (phase-7). Not Esc, because the keyboard goes to the remote shell at
        /// that moment. The menu shortcut is caught before `keyDown:`, so it
        /// also works on the alternate screen (vim) — its only gate is the queue
        /// (`validateMenuItem:`).
        #[unsafe(method(cancelUpload:))]
        fn cancel_upload(&self, _sender: Option<&AnyObject>) {
            self.cancel_uploads();
        }

        /// The popover row's button (`Cancel`/`Remove`): `tag` is the item's
        /// id, not its position — positions shift with finished and removed items.
        #[unsafe(method(uploadRowAction:))]
        fn upload_row_action_sent(&self, sender: Option<&AnyObject>) {
            let Some(button) = sender.and_then(|sender| sender.downcast_ref::<NSButton>()) else {
                return;
            };
            if let Ok(id) = u64::try_from(button.tag()) {
                self.upload_row_action(id);
            }
        }
    }
);

impl TerminalPane {
    /// Builds the view, the surface and the renderer; the session and link are
    /// **not there yet** ([`TerminalPane::start`]). The frame observer is not
    /// there yet either: the window sets it up after making the pane the
    /// `contentView` ([`TerminalPane::observe_frame`]).
    ///
    /// The renderer is born here and its error returns to the caller: if the
    /// GPU device or the pipelines cannot be built the pane has nothing to
    /// draw. The font is given to the renderer here as a **request**, with
    /// the inherited point-size delta ([`TerminalPane::request_font`]): the
    /// first atlas opens in `start`'s geometry with the enlarged point size.
    pub(crate) fn new(
        mtm: MainThreadMarker,
        frame: NSRect,
        launch: PaneLaunch,
    ) -> Result<Retained<Self>, GpuError> {
        let PaneLaunch {
            id,
            run,
            host,
            lookup,
            stats,
            settings,
            theme,
            launch,
            integration,
            reduce_motion,
            smooth_scroll,
            zoom,
        } = launch;
        let renderer = Rc::new(Renderer::system_default()?);
        // The layer is ours (040 → Karar 8): wgpu configures its device,
        // format and drawable size, the scale stays with its owner.
        let layer = CAMetalLayer::new();
        // SAFETY: `layer` is a live `CAMetalLayer`; wgpu retains it.
        let surface = Rc::new(unsafe {
            Surface::from_layer(&renderer, NonNull::from(&*layer).cast::<c_void>())
        }?);
        let view = BateriView::new(mtm, frame);
        // Order matters: layer first, then wantsLayer — the reverse makes
        // AppKit build its own layer and the CAMetalLayer is dropped.
        view.setLayer(Some(&layer));
        view.setWantsLayer(true);
        let font = settings.font.clone();
        let remote_files = settings.remote_files.clone();
        let dim = DimOverlay::new(mtm);
        dim.paint(&theme);
        let link_label = LinkLabel::new(mtm);
        link_label.0.paint(&theme);
        let this = Self::alloc(mtm).set_ivars(PaneIvars {
            id,
            run,
            host,
            lookup,
            birth: RefCell::new(Some(Birth {
                stats,
                settings,
                theme,
                launch,
                integration,
            })),
            font: RefCell::new(font),
            reduce_motion: Cell::new(reduce_motion),
            smooth_scroll: Cell::new(smooth_scroll),
            renderer,
            layer,
            surface,
            view: view.clone(),
            dim: dim.clone(),
            link_label: link_label.clone(),
            link: OnceCell::new(),
            session: OnceCell::new(),
            shell_parent: OnceCell::new(),
            wake: Arc::new(ShellWake {
                id,
                lookup,
                timed: run.is_some(),
                waker: Mutex::new(None),
                pending_copy: Arc::default(),
                title_pending: Arc::default(),
                search_pending: Arc::default(),
                remote_probe: Arc::default(),
                link_pending: Arc::default(),
            }),
            zoom: Cell::new(zoom),
            // No dock at launch: `start` decides and computes the geometry
            // after that.
            dock_rows: Cell::new(0),
            dock_rows_at_birth: Cell::new(0),
            tab_id: new_tab_id(),
            closed: Cell::new(false),
            search: OnceCell::new(),
            search_status: Cell::new(SearchStatus::Empty),
            search_driving: Cell::new(false),
            uploads: RefCell::new(Transfers::default()),
            upload_alert: RefCell::new(None),
            upload_stop: RefCell::new(None),
            upload_list: RefCell::new(None),
            list_closed_at: Cell::new(None),
            remote_helper: RefCell::new(RemoteHelper::default()),
            remote_files: RefCell::new(remote_files),
            previews: RefCell::new(HashMap::new()),
            finder: RefCell::new(FinderDrops::default()),
        });
        // SAFETY: `initWithFrame:` is NSView's designated initializer and the
        // ivars are set.
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: frame] };
        // The pane is a plain **container**, `BateriView` is its child (033 →
        // R4.1): the search panel will float above the terminal and must be a
        // sibling of the Metal layer, not its child — the subviews of a
        // layer-hosting view are outside AppKit's contract. The pane is
        // layer-backed, otherwise the sibling panel could end up **below** the
        // Metal layer. It draws nothing itself and receives no events:
        // `BateriView` fills it completely, hit testing falls to the topmost child.
        this.setWantsLayer(true);
        // The child is fitted to the pane and follows its size by
        // autoresizing. The source of the geometry is still `BateriView`
        // (`sync_geometry`), and so is the notification's frame.
        view.setFrame(this.bounds());
        view.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable
                | NSAutoresizingMaskOptions::ViewHeightSizable,
        );
        this.addSubview(&view);
        // The link label under the veil (an unfocused pane gets no hover anyway).
        this.addSubview(&link_label.0);
        // The veil is on top: the search panel goes right above `view`
        // (`SearchBar::new`), so it too stays under the veil and the dimmed
        // pane's panel is dimmed too.
        dim.setFrame(this.bounds());
        dim.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable
                | NSAutoresizingMaskOptions::ViewHeightSizable,
        );
        this.addSubview(&dim);
        let font = this.ivars().font.borrow().clone();
        this.request_font(&font);
        Ok(this)
    }

    /// Subscribes to the terminal view's frame notification
    /// (`viewFrameDidChange:`). The content's size changes independently of the
    /// window too (the tab bar); so the geometry comes from the view's own
    /// notification. `postsFrameChangedNotifications` is on by default.
    ///
    /// The **last** step of the window's constructor: a notification arriving
    /// before the pane becomes `contentView` would try to build the geometry
    /// without a window. The observer is removed when the pane closes
    /// ([`TerminalPane::begin_close`]), without waiting for the window's closing.
    pub(crate) fn observe_frame(&self) {
        // SAFETY: the selector is defined on this class and takes a single
        // `&NSNotification`; the name is a constant AppKit exposes, the object
        // is this pane's view.
        unsafe {
            NSNotificationCenter::defaultCenter().addObserver_selector_name_object(
                self,
                sel!(viewFrameDidChange:),
                Some(NSViewFrameDidChangeNotification),
                Some(&self.ivars().view),
            );
        }
    }

    pub(crate) fn id(&self) -> u64 {
        self.ivars().id
    }

    /// The session's persistent identity (`TERM_SESSION_ID`, `bateri://tab/<id>`;
    /// 038). Separate from the in-process [`TerminalPane::id`]: that one is
    /// the key of main-queue returns, this is the name given outward.
    pub(crate) fn tab_id(&self) -> &TabId {
        &self.ivars().tab_id
    }

    /// Whether closing has begun ([`PaneIvars::closed`]).
    pub(crate) fn is_closed(&self) -> bool {
        self.ivars().closed.get()
    }

    /// This pane's temporary point-size delta — a new tab inherits it
    /// (026 → Karar 3).
    pub(crate) fn zoom(&self) -> Zoom {
        self.ivars().zoom.get()
    }

    /// Owner of the events ([`PaneHost`]).
    pub(crate) fn host(&self) -> &dyn PaneHost {
        &*self.ivars().host
    }

    /// The path by which main-queue returns find the pane (`uploader`'s jobs
    /// capture it too).
    pub(crate) fn lookup(&self) -> PaneLookup {
        self.ivars().lookup
    }

    /// The report path reads the counters from here (`AppDelegate::report_and_exit`).
    pub(crate) fn renderer(&self) -> &Renderer {
        &self.ivars().renderer
    }

    pub(crate) fn link(&self) -> Option<&DisplayLink> {
        self.ivars().link.get()
    }

    /// The terminal view.
    pub(crate) fn view(&self) -> &BateriView {
        &self.ivars().view
    }

    pub(crate) fn session(&self) -> Option<&Arc<Session>> {
        self.ivars().session.get()
    }

    /// Gives the renderer the settings' font with this pane's point-size
    /// delta as a **request** — the launch path ([`TerminalPane::new`]): the
    /// atlas opens in `start`'s `sync_geometry` and that also writes the font
    /// slot. The return value (whether it changed) is not a question here: the
    /// geometry has not been built at all yet.
    fn request_font(&self, font: &FontOptions) {
        let _ = self
            .ivars()
            .renderer
            .set_font(&self.ivars().zoom.get().apply(font));
    }

    /// The font in the file changed: the point-size delta is updated by
    /// [`Zoom::after_reload`]'s rule. It does not apply the font; after the
    /// settings are written [`TerminalPane::apply_font`] applies it.
    pub(crate) fn zoom_after_reload(&self, old: &FontOptions, new: &FontOptions) {
        let zoom = self.ivars().zoom.get().after_reload(old, new);
        self.ivars().zoom.set(zoom);
    }

    /// Consumes the rest of the birth package: decides the dock reserve,
    /// builds the first geometry and opens the session.
    ///
    /// The integration was asked **once** on the owner's side and gave both
    /// answers together: the child's environment and the dock's existence
    /// (`AppDelegate::shell_integration`, [`PaneLaunch::integration`]).
    /// With two separate calls the two could diverge — a session that loses
    /// two rows from the window but has no dock (or the reverse), and the
    /// symptom would be silent. **Before** the geometry: the grid height must
    /// see the dock reserve, otherwise the shell is born at launch with one
    /// row too many and the first frame eats a `TIOCSWINSZ` for the correction.
    ///
    /// `working_directory` is the caller's decision (026 → Karar 4: the active
    /// tab's directory, else home). The error returns to the caller: in the
    /// first window the process exits, in ⌘T/⌘N only that window closes —
    /// the other tabs' shells must not die because a new one could not be born.
    ///
    /// `launch.initial_input` is the shell's first input (037 Karar 6: ⌘T in
    /// a remote tab, `AppDelegate::open_window`'s decision); `None` → an
    /// ordinary local shell.
    ///
    /// The pane must be attached to a window (`contentView`): the scale is
    /// read from it ([`TerminalPane::sync_geometry`]); if not, an error. A
    /// second call is an error too: the package is consumed once.
    pub(crate) fn start(&self, mtm: MainThreadMarker) -> std::io::Result<()> {
        let Some(birth) = self.ivars().birth.take() else {
            return Err(std::io::Error::other("pane started a second time"));
        };
        let (_, rows) = birth.integration;
        self.ivars().dock_rows_at_birth.set(rows);
        self.ivars().dock_rows.set(rows);
        // The grid size derives from the window; the session is born with its
        // first size so the shell sees the right `TIOCSWINSZ` at launch.
        let Some(grid) = self.sync_geometry() else {
            return Err(std::io::Error::other("pane is not attached to a window"));
        };
        self.start_session(mtm, grid, birth)
    }

    /// Opens the session and attaches the link that drives frames. The order
    /// is required: `Session` wants `Wake`, the link wants `Session`, `Waker`
    /// is born from the link.
    fn start_session(
        &self,
        mtm: MainThreadMarker,
        grid: Grid,
        birth: Birth,
    ) -> std::io::Result<()> {
        let Birth {
            stats,
            settings,
            theme,
            launch,
            integration: (integration, _),
        } = birth;
        let Launch {
            working_directory,
            initial_input,
        } = launch;
        // In smoke and measurement runs the shell is fixed: the result must not
        // depend on the user's `$SHELL` and rc file. The owner of the scripts
        // is `bt-core`; that `smoke_shell` gives eight cells and six glyphs is
        // tested there — so the `cells=8` and `glyphs=6` expectations are not a
        // documentation sentence but a tested claim.
        //
        // The branch asks for the **load**, not the duration: the same `Run`
        // sets up both the deadline and the guard, and the load is independent
        // of them.
        //
        // An untimed session's command and the shell's parent come from
        // one call ([`child::shell_command`]): on macOS `login(1)` with
        // `-q`, so its `Last login:` banner never lands on the grid (an
        // unresolved user or shell falls back to `None`, alacritty's own
        // `login` path). The timed run's scripts are the shell itself, so
        // they are `Direct`; deciding the parent in the same branch as the
        // command is what keeps the two from disagreeing.
        let (command, shell_parent) = match self.ivars().run {
            None => child::shell_command(),
            Some(run) => (
                Some(match run.workload {
                    Workload::Smoke => smoke_shell(),
                    // The load's duration is the same as the deadline: if it
                    // falls short the window idles at the tail of the run and
                    // the measurement samples idle frames. A load without a
                    // duration is now **not representable** — `Run` carries the
                    // duration next to the load, so the old `unwrap_or(0)` and
                    // the `debug_assert` defending it are gone.
                    Workload::Load => load_shell(run.seconds),
                }),
                ShellParent::Direct,
            ),
        };
        // Whether the wrapper was installed: if the integration's environment
        // is non-empty the shell will print our identity (the `blocks` tier has
        // no dock but has marks, i.e. it cannot be derived from `dock`). Before
        // the environment is moved into `env` below.
        let shell_marks = !integration.is_empty();
        let session = Session::spawn(
            SessionOptions {
                command,
                // Directory and locale follow the same rule in **every**
                // session, timed run included: the decision has a single arm
                // (`discussion.md` → Karar 6 eki, "istisnasız") and neither of
                // the two fixed scripts depends on directory or locale —
                // `printf` with `sleep`, `date` with `printf`; paths absolute
                // or from `PATH`, output ASCII.
                //
                // The directory now comes from the caller: a new tab is in the
                // active tab's OSC 7 directory (026 → Karar 4); in a timed run
                // and the first window `child::working_directory()`.
                working_directory,
                // The title's `~` rule; **the same resolution** as the directory (`child::home`).
                home: child::home(),
                // Shell integration sits beside the locale, in the same map:
                // both are environment **added** to the child and both go only
                // to the child. Their keys are disjoint (`LANG` ↔ `ZDOTDIR`),
                // so order does not matter.
                // The integration's environment comes **from the caller**: the
                // same answer also determines the dock's existence (`start`)
                // and if it were asked a second time here the two decisions
                // could diverge.
                env: child::locale_env(locale::system_locale())
                    .into_iter()
                    .chain(integration)
                    .collect(),
                cols: grid.cols,
                rows: grid.rows,
                cell_px: grid.cell.cell_px(),
                terminal: settings.terminal(),
                theme,
                // The dock's **existence**, not its reserve: `bt-core` hands
                // the caret over accordingly. Its source is the birth-reserve
                // slot (`start` wrote it a line earlier) and the alternate-screen
                // notifier's gate reads the same slot too, so they cannot diverge.
                dock: self.ivars().dock_rows_at_birth.get() > 0,
                // Clustering (035) is on in all windows, timed run included.
                // Not a settings key (035 Karar 2): rolling back is this one line.
                cluster: true,
                // A timed run always gets `None` from `open_window` (single
                // window, no ⌘T), so its fixed scripts are unaffected by this.
                initial_input,
                shell_marks,
                // The identity is in every window, timed run included (038
                // Karar 8): the variables read no file and do not move the tokens.
                tab_id: Some(self.ivars().tab_id.clone()),
                // The machine's name (044): `file://$HOST/…` (GNU `ls --hyperlink`)
                // and OSC 7's named authority count as local. One `gethostname`
                // per pane; the timed run's tokens do not depend on it.
                hostname: crate::links::hostname(),
            },
            Arc::clone(&self.ivars().wake) as Arc<dyn Wake>,
        );
        // A terminal window without a shell is an empty box; what to do is the
        // caller's call (first window: the process exits; later ones: that window closes).
        let session = Arc::new(session?);
        // The closing sequence reaches the session from here, not through the
        // link, and the keyboard holds its own copy; all three live on the main
        // thread, so where the last reference drops is clear (see `shutdown`).
        let _ = self.ivars().session.set(Arc::clone(&session));
        let _ = self.ivars().shell_parent.set(shell_parent);
        // The host marks' list at birth (037 Karar 2); its live change comes
        // from `AppDelegate::reload_settings` ([`Self::set_host_marks`]).
        session.set_host_marks(&settings.remote_hosts);
        // A title notification that arrived before the session entered the slot
        // may have found an empty slot and dropped; the window's `start` closes
        // that (`TerminalWindow::start` → `refresh_title`), right after this call returns.
        let view = &self.ivars().view;
        view.attach(Arc::clone(&session));
        // The mouse translation must see the same grid as the session: the
        // size and count are the same ones that went to the `SessionOptions`
        // above. On the `resize` path the same triple is also written together
        // (`refresh_geometry`).
        view.set_metrics(grid, self.ivars().dock_rows.get());
        // The rhythm is the view's display link, as a timer (040 → Karar 7,
        // path (b)); it gets the frame loop to tick right below.
        let pacer = MacPacer::new(mtm, view);
        let link = DisplayLink::new(
            Arc::clone(&pacer) as Arc<dyn Pacer>,
            Rc::clone(&self.ivars().surface),
            Rc::clone(&self.ivars().renderer),
            session,
            Layout {
                cols: grid.cols,
                dock_rows: self.ivars().dock_rows.get(),
                cell: grid.cell,
            },
            stats,
            // **The path is set up only in a window that has a dock** and this
            // is structural (R5.1): in a dockless session an alternate-screen
            // transition cannot change anything, so there is no watch either.
            // Had it been shut off by a condition, the claim "no resize at all"
            // would depend on the correctness of a branch.
            (self.ivars().dock_rows_at_birth.get() > 0)
                .then(|| alt_screen_notifier(self.ivars().id, self.ivars().lookup)),
        );
        pacer.attach(mtm, link.ticker());
        // We do not want frames before the wake path is closed: a `Wakeup`
        // in between would be dropped silently.
        //
        // audit: `start_session` is called once per window, from `start`.
        // A silently swallowed `Err` here would produce the most insidious bug:
        // the old link's `Waker` stays, the window never wakes to shell output
        // again and not a single line of trace remains.
        assert!(
            self.ivars().wake.slot().replace(link.waker()).is_none(),
            "waker set a second time"
        );
        // The mouse mapping's vertical origin: like `set_metrics` it comes not
        // from the window but from the **frame** path, so once, after the link
        // is born. The mouse thus reads the drawn offset; a second computation
        // would mean "the click is off by a row" (`bt_gpu::Origin`).
        view.attach_origin(link.origin());
        // The cursor's style is the setting's too: the link is born with
        // `CursorMotion::default()` and the call here pulls it to the file's
        // value (in a hermetic run `Settings::default()`'s). `set_font`'s place
        // is `load_settings` but the style's cannot be: the link does not exist yet.
        // The dock's typing effects are here for the same reason.
        link.set_cursor_motion(settings.cursor_motion);
        link.set_glyph_fx(settings.keypress, settings.erase);
        // Opening frame: `Session` is born dirty, we open the link once by hand.
        link.request_frame();
        let _ = self.ivars().link.set(link);
        // Reduce Motion **after** the link enters the slot: the first value
        // (the birth package's resolved value) lands on this pane's link from
        // here, the system's notification and the settings write later reach
        // all panes through `AppDelegate::apply_reduce_motion`. In a hermetic
        // run the resolved value is `false` and the link is born with that
        // value, so the call is a no-op (`DisplayLink::set_reduce_motion`).
        self.set_reduce_motion(self.ivars().reduce_motion.get());
        // The wheel's mode comes from the same resolved input (Reduce Motion is
        // the setting's third input) and the same later path (`apply_reduce_motion`).
        self.set_smooth_scroll(self.ivars().smooth_scroll.get());
        // The cursor's drawing numbers also land once at launch and are read
        // **from the slot**, not from the `link` in hand: the link was moved
        // into the slot in that call. `set_caret_style` is a no-op on the same
        // value, so it does not collide with the save-time path.
        self.apply_caret(&settings);
        // **The focus is seeded too** and the reason is the same ordering: the
        // window becomes key with `makeKeyAndOrderFront`, so
        // `windowDidBecomeKey:` fires **before** the link enters the slot and
        // that call is silently dropped. Without the seeding, in a window
        // opened in the background (`open -g`, a login item, a script-launched
        // open while another application is in front) no notification would
        // arrive and `focused` would stay `true`: an unfocused window would
        // draw a filled caret and set up the blink clock (`/code-review`).
        self.apply_focus(self.window().is_some_and(|window| window.isKeyWindow()));
        Ok(())
    }

    /// The alternate screen changed: the dock goes away or comes back.
    ///
    /// The sender is the frame path's notifier ([`alt_screen_notifier`]) and
    /// this method runs **on the next main-queue turn** — so as not to pull
    /// the rug from under a drawn frame.
    ///
    /// **It re-reads the truth**, ignoring what the notification carried: if
    /// two transitions chase each other (vim open-close) both jobs waiting in
    /// the queue see the same, current answer. If nothing changed it **does
    /// nothing** — this gate upholds the "one resize per transition" (R5.3)
    /// claim.
    pub(crate) fn alt_screen_did_change(&self) {
        let Some(session) = self.ivars().session.get() else {
            return;
        };
        let wanted =
            app::dock_rows_for(session.alt_screen(), self.ivars().dock_rows_at_birth.get());
        if self.ivars().dock_rows.replace(wanted) == wanted {
            return;
        }
        // Reserve, grid and link **in a single block**: the frame path is on the
        // main thread too, so no frame can slip in between and no half state is drawn.
        self.refresh_geometry();
    }

    /// View ▸ Bigger (Cmd +): the point-size delta grows one step.
    pub(crate) fn zoom_in(&self) {
        self.change_zoom(Zoom::bigger);
    }

    /// View ▸ Smaller (Cmd −): the point-size delta shrinks one step.
    pub(crate) fn zoom_out(&self) {
        self.change_zoom(Zoom::smaller);
    }

    /// View ▸ Actual Size (Cmd 0): the delta is reset, the setting's point size.
    pub(crate) fn zoom_reset(&self) {
        self.change_zoom(|_, _| Zoom::default());
    }

    /// Bigger, Smaller, Actual Size: changes this pane's temporary point-size
    /// delta with `step` and applies the font. It does not touch the file and
    /// also works in a timed run — it reads nothing from the user's world.
    fn change_zoom(&self, step: impl FnOnce(Zoom, &FontOptions) -> Zoom) {
        let zoom = step(self.ivars().zoom.get(), &self.ivars().font.borrow());
        self.ivars().zoom.set(zoom);
        self.apply_font();
    }

    /// The setting's font changed (`AppDelegate::reload_settings`, after the
    /// delta was updated by [`TerminalPane::zoom_after_reload`]): it is
    /// stored and applied.
    pub(crate) fn set_font(&self, font: &FontOptions) {
        self.ivars().font.replace(font.clone());
        self.apply_font();
    }

    /// Gives the renderer the settings' font with this pane's temporary
    /// point-size delta; if the request changed the geometry is rebuilt
    /// ([`TerminalPane::refresh_geometry`]: atlas, grid, PTY size, font slot).
    ///
    /// Two gates, both needed: the caller's gate (delta, press) says something
    /// changed, `set_font` says whether the renderer already wants that font —
    /// a press at the limit or a save that writes the same point size as the
    /// delta does not rebuild the atlas.
    fn apply_font(&self) {
        let font = self.ivars().zoom.get().apply(&self.ivars().font.borrow());
        if self.ivars().renderer.set_font(&font) {
            self.refresh_geometry();
        }
    }

    /// `[remote]` changed — the `hosts` pattern list goes to the session; the
    /// active remote host's mark is re-resolved there (037 Karar 2). The tab's
    /// dot is the window's job (`TerminalWindow::set_host_marks`). The preview
    /// and download keys (045 R8) are kept here for the next download.
    pub(crate) fn set_host_marks(&self, settings: &Settings) {
        if let Some(session) = self.ivars().session.get() {
            session.set_host_marks(&settings.remote_hosts);
        }
        self.ivars()
            .remote_files
            .replace(settings.remote_files.clone());
    }

    /// Terminal options changed — to the session, **in full**.
    pub(crate) fn set_terminal_options(&self, settings: &Settings) {
        if let Some(session) = self.ivars().session.get() {
            session.set_terminal_options(settings.terminal());
        }
    }

    /// Swaps the theme into the session (no-op on the same theme,
    /// `Session::set_theme`) and paints the search panel with it; if the panel
    /// is not born yet it is painted with the session's theme on the first ⌘F.
    /// The window paints the chrome and the tab's dot (`TerminalWindow::set_theme`,
    /// the only caller of this call).
    pub(crate) fn set_theme(&self, theme: Theme) {
        if let Some(session) = self.ivars().session.get() {
            session.set_theme(theme);
        }
        if let Some(bar) = self.ivars().search.get() {
            bar.paint(&theme, is_dark_background(&theme));
        }
        self.ivars().dim.paint(&theme);
        self.ivars().link_label.0.paint(&theme);
    }

    /// Shows or hides the dim veil (039 Karar 7, R4.4). The decision is the
    /// owner's ("not focused and more than one pane in the window",
    /// `TerminalWindow::refresh_dim`); it asks for no frame — the veil is AppKit's.
    pub(crate) fn set_dimmed(&self, dimmed: bool) {
        self.ivars().dim.setHidden(!dimmed);
    }

    /// Shows the ⌘-hovered OSC 8 link's target in the bottom-left label, or
    /// hides it (`None`; 044 Karar 7). The caller is `hyperlink`'s hover: only
    /// with ⌘ and only for an OSC 8 link — a plain-text link is its own target.
    /// The width is the text's, at most the pane's minus the margins; asks for
    /// no frame.
    pub(crate) fn set_link_target(&self, target: Option<&str>) {
        let (label, text) = &self.ivars().link_label;
        let Some(target) = target else {
            label.setHidden(true);
            return;
        };
        text.setStringValue(&NSString::from_str(target));
        let fit = text.fittingSize();
        let room = self.bounds().size.width - 2.0 * (LINK_LABEL_MARGIN + LINK_LABEL_PAD_X);
        let width = fit.width.min(room).max(0.0);
        text.setFrame(NSRect::new(
            NSPoint::new(LINK_LABEL_PAD_X, LINK_LABEL_PAD_Y),
            NSSize::new(width, fit.height),
        ));
        label.setFrame(NSRect::new(
            NSPoint::new(LINK_LABEL_MARGIN, LINK_LABEL_MARGIN),
            NSSize::new(
                width + 2.0 * LINK_LABEL_PAD_X,
                fit.height + 2.0 * LINK_LABEL_PAD_Y,
            ),
        ));
        label.setHidden(false);
    }

    /// The cursor's style and the dock's typing effects go to the link, not to
    /// the session: they change **how** we draw, not which frame we draw.
    /// The effects descend **raw**; the reduction for `snap` and Reduce Motion
    /// is in `bt-gpu` (`DisplayLink::set_glyph_fx`).
    pub(crate) fn set_cursor_motion(&self, settings: &Settings) {
        if let Some(link) = self.ivars().link.get() {
            link.set_cursor_motion(settings.cursor_motion);
            link.set_glyph_fx(settings.keypress, settings.erase);
        }
    }

    /// Gives the link the cursor's values that descend from the settings.
    ///
    /// **Launch and save time go through the same code** and the reason is a
    /// class of defect (`/code-review`, 016): had the two lists been written
    /// separately they could drift — a key seeded only at launch would not
    /// apply at save time, a key only reloaded would stay at the default at
    /// launch. Both are silent and `plan.md` names that class ("a key that
    /// descends halfway shows up in no gate").
    ///
    /// A single `Changes::caret` field, two calls: the destinations are
    /// separate (drawing numbers to `Frame`, the period to `bt_gpu::blink`)
    /// but both are the result of the same save — the precedent is the two
    /// keys of `Changes::motion`.
    ///
    /// It returns silently if there is no link: the launch call will give the
    /// same value anyway.
    pub(crate) fn apply_caret(&self, settings: &Settings) {
        let Some(link) = self.ivars().link.get() else {
            return;
        };
        link.set_caret_style(settings.caret);
        link.set_blink_interval(settings.blink_interval);
    }

    /// Gives the link Reduce Motion's **resolved** value
    /// (`AppDelegate::reduce_motion`). A no-op if the value did not change
    /// (`bt_gpu::DisplayLink::set_reduce_motion`); returns silently if there is no link.
    pub(crate) fn set_reduce_motion(&self, reduce: bool) {
        self.ivars().reduce_motion.set(reduce);
        if let Some(link) = self.ivars().link.get() {
            link.set_reduce_motion(reduce);
        }
    }

    /// Gives the view the scrolling's **resolved** mode
    /// (`AppDelegate::smooth_scroll`). To the view, not the link: the decision
    /// is made in the event's classification, in `scrollWheel:`, and the
    /// `false` arm is the very same as today's line path (027 Karar 5).
    pub(crate) fn set_smooth_scroll(&self, smooth: bool) {
        self.ivars().smooth_scroll.set(smooth);
        self.ivars().view.set_smooth_scroll(smooth);
    }

    /// The keyboard came to the terminal (`here`) or went to the search field
    /// — `BateriView`'s first-responder hooks supply it (033 R7). The focus's
    /// second bit; the combination of the two bits is in `bt-gpu`
    /// (`DisplayLink::set_keyboard_in_terminal`). In a timed run it stays
    /// silent through [`TerminalPane::apply_focus`]'s gate.
    ///
    /// The keyboard's arrival also goes to the owner as a focus event
    /// ([`PaneHost::focused`]); its departure does not, because a keyboard
    /// moving to the search field stays in the same pane.
    pub(crate) fn keyboard_moved(&self, here: bool) {
        if self.ivars().run.is_some() {
            return;
        }
        if let Some(link) = self.ivars().link.get() {
            link.set_keyboard_in_terminal(here);
        }
        if here {
            self.host().focused(self.ivars().id);
        }
    }

    /// The smallest pane's size, in points (039 Karar 14): a pane whose grid
    /// is exactly [`MIN_PANE_COLS`] × [`MIN_PANE_ROWS`] — the inverse of
    /// [`split_into_grid`] (left gutter + columns, dock reserve + rows). The
    /// measure is this pane's cell and dock reserve: the point-size delta is
    /// per pane. The split's gate ([`TerminalPane::grid_fits`]) and the
    /// resizing's limit (`SplitView::resize`) come from here. `None` if not
    /// attached to a window.
    pub(crate) fn min_size(&self) -> Option<NSSize> {
        let scale = self.window()?.backingScaleFactor();
        let cell = self.ivars().renderer.cell_metrics(scale);
        let (cell_w, cell_h) = cell.cell_px();
        let width = f64::from(cell.gutter_px()) + f64::from(MIN_PANE_COLS) * f64::from(cell_w);
        let height = f64::from(bt_gpu::dock_px(self.ivars().dock_rows.get(), cell))
            + f64::from(MIN_PANE_ROWS) * f64::from(cell_h);
        Some(NSSize::new(width / scale, height / scale))
    }

    /// A cell's size, in points — the step of keyboard resizing
    /// (`TerminalWindow::resize_split`). `None` if not attached to a window.
    pub(crate) fn cell_size(&self) -> Option<NSSize> {
        let scale = self.window()?.backingScaleFactor();
        let (cell_w, cell_h) = self.ivars().renderer.cell_metrics(scale).cell_px();
        Some(NSSize::new(
            f64::from(cell_w) / scale,
            f64::from(cell_h) / scale,
        ))
    }

    /// Whether a pane of `size` (points) has a grid that passes the smallest
    /// pane limit (039 Karar 14) — the split's gate. The new split inherits
    /// the cell and the dock reserve from this pane ([`TerminalPane::min_size`]).
    /// `false` if not attached to a window.
    pub(crate) fn grid_fits(&self, size: NSSize) -> bool {
        self.min_size()
            .is_some_and(|min| size.width >= min.width && size.height >= min.height)
    }

    /// The job running in the foreground outside the shell (028 → Karar 1).
    /// Idle if there is no session or the reader thread has finished: the
    /// shell is gone and `child_pid` may be stale, a stale pid is not asked.
    pub(crate) fn foreground(&self) -> Foreground {
        let (Some(session), Some(&parent)) =
            (self.ivars().session.get(), self.ivars().shell_parent.get())
        else {
            return Foreground::Idle;
        };
        if !session.reader_alive() {
            return Foreground::Idle;
        }
        jobs::foreground(parent, session.child_pid(), &SystemTable)
    }

    /// The remote-session probe (036 Karar 2): takes the running command's
    /// generation, probes the foreground group and reports to the session if
    /// it found ssh/mosh. The return is two bits ([`RemoteProbeOutcome`]):
    /// **whether undecided** — if so the arm stays set and the next output
    /// probes again ([`RemoteProbe`]) — and whether the remote state changed
    /// (so the window title refreshes).
    ///
    /// The generation **before** the probe: `Session::set_remote` rejects the
    /// answer of a command that ended in between. No probe if the reader has
    /// finished ([`Self::foreground`]'s rule: a stale pid is not asked).
    pub(crate) fn probe_remote(&self) -> RemoteProbeOutcome {
        let settled = RemoteProbeOutcome::default();
        let (Some(session), Some(&parent)) =
            (self.ivars().session.get(), self.ivars().shell_parent.get())
        else {
            return settled;
        };
        if !session.reader_alive() {
            return settled;
        }
        let Some(command) = session.running_command() else {
            return settled;
        };
        match jobs::remote(parent, session.child_pid(), &SystemTable) {
            Probe::Undecided => RemoteProbeOutcome {
                undecided: true,
                changed: false,
            },
            Probe::Local => settled,
            Probe::Remote(target) => {
                // The line is per argument, with readable quoting (037 Karar 1);
                // `bt-core` does not write the rule a second time, it stores the string.
                let line = quote::command_line(&target.argv);
                let target = RemoteTarget {
                    host: target.host,
                    kind: target.kind,
                    argv: target.argv,
                    line,
                };
                RemoteProbeOutcome {
                    undecided: false,
                    changed: session.set_remote(command, Some(&target)),
                }
            }
        }
    }

    /// The focus changed — forwards it to `bt-gpu`.
    ///
    /// **Never called in a hermetic run** (R7.1) and the gate is here, not in
    /// `bt-gpu`'s default: `DisplayLink`'s `focused` is born `true` anyway but
    /// that alone is not enough — a Spotlight opening during `make smoke`
    /// produces `windowDidResignKey:`, which asks for a frame, and the gate
    /// would be green on one machine and red on another. The precedent is
    /// `app::resolve_reduce_motion` looking at `Inputs`.
    ///
    /// It returns silently if there is no link: the key event can also fire
    /// before `start_session` and in that state the default (`true`) is
    /// already right.
    pub(crate) fn apply_focus(&self, focused: bool) {
        // The gate looks at the `run` **flag**, not `inputs()`: `inputs()`
        // resolves `child::home()` as an argument (may go as far as the passwd
        // record) and focus changes on every application switch. The flag's
        // copy is therefore in the pane itself: no need to reach the
        // application delegate either.
        if self.ivars().run.is_some() {
            return;
        }
        if let Some(link) = self.ivars().link.get() {
            link.set_focused(focused);
        }
    }

    /// The pane's share of the closing sequence — **starts, does not wait**.
    /// Its callers are the window's `begin_close` — the window's closing
    /// (`windowWillClose:`, the handle drops) and the application's closing
    /// (`AppDelegate::shutdown`, all handles waited on until a single
    /// deadline) — and a single pane's closing (`TerminalWindow::close_pane`,
    /// the handle drops; if the split could not be born, `add_pane`'s rollback).
    ///
    /// The order is required: first the upload queue is released (processes are
    /// killed and the half file is deleted; there is no dock left to show the
    /// result), **then** the rhythm, the `Waker` and `SIGHUP` — in the reverse
    /// order the cancellation would go after the shell's `SIGHUP`.
    ///
    /// 0. Count the pane as closed ([`PaneIvars::closed`]) and remove the frame
    ///    observer: while the tab bar closes AppKit can re-lay-out the content
    ///    and if the observer stayed the dying session would receive a resize
    ///    (and a `Msg::Resize` that cannot be written to a dropped reader)
    ///    (`/code-review`).
    /// 1. Cut the rhythm (`DisplayLink::stop`): the link stops, leaves the
    ///    run loop and the wake gate closes. No new frame is asked after this.
    /// 2. **Detach** the `Waker` from `ShellWake` and drop it here, on the
    ///    main thread ([`ShellWake::detach`]): `ShellWake`'s last copy can
    ///    drop on the reader or the `"PTY teardown"` thread and must not carry a `Waker` there.
    /// 3. Start the session's closing (`Session::begin_shutdown`: `SIGHUP` +
    ///    the reader thread's finish in the background).
    ///
    /// `DisplayLink` is now **droppable** and drops on the main thread with
    /// the pane object: after detaching, the last `Waker` copy is either in
    /// this object or in Metal's completion block; the second throws a
    /// synchronous job at the main queue but the main thread is not waiting
    /// at that time — the window's closing does not wait, and ⌘Q keeps the
    /// windows in the list until the wait ends.
    ///
    /// Idempotent: the second call returns [`Closing::AlreadyDone`] (`stop` is
    /// latched, `detach` is `take`, `begin_shutdown` is an `Option`, removing
    /// the observer is a no-op if unregistered). `None` if the session was
    /// never born — there is nothing to close.
    pub(crate) fn begin_close(&self) -> Option<Closing> {
        self.abandon_uploads();
        // Finder's pending promises fail now (cancelled), not with the last reference.
        self.finder_abandon();
        // The helper's ssh goes now, not when the last reference drops.
        self.remote_helper().borrow_mut().close();
        self.ivars().closed.set(true);
        // SAFETY: the observer is this object, registered in `observe_frame`;
        // a no-op if it is not registered.
        unsafe { NSNotificationCenter::defaultCenter().removeObserver(self) };
        if let Some(link) = self.ivars().link.get() {
            link.stop();
        }
        drop(self.ivars().wake.detach());
        let session = self.ivars().session.get()?;
        Some(match session.begin_shutdown() {
            Some(handle) => Closing::Started(handle),
            None => Closing::AlreadyDone,
        })
    }

    /// Pane geometry or font moved: match the layer, update the grid, ask for
    /// a frame. A font change that lands on the same grid is redrawn too:
    /// `DisplayLink::resize` asks for the frame unconditionally and asking for
    /// a frame also sets the damage flag.
    ///
    /// The mouse inputs are refreshed here too: the view sits in
    /// `PaneIvars.view` as a `Retained<BateriView>` and goes with the pane
    /// object. If the pane is not attached to a window there is no scale and
    /// nothing is done ([`TerminalPane::sync_geometry`]).
    pub(crate) fn refresh_geometry(&self) {
        let Some(grid) = self.sync_geometry() else {
            return;
        };
        self.ivars()
            .view
            .set_metrics(grid, self.ivars().dock_rows.get());
        if let Some(link) = self.ivars().link.get() {
            link.resize(
                grid.cols,
                grid.rows,
                grid.cell,
                self.ivars().dock_rows.get(),
            );
        }
    }

    /// Matches the layer's drawable size to the view's backing geometry **and**
    /// returns the grid size — the name says both because the caller needs
    /// both and deriving the size without writing the dimensions would give a
    /// wrong result. The scale is read from a single source and the pixel size
    /// is multiplied from it; if `drawableSize` and `contentsScale` diverge
    /// there is blur.
    ///
    /// **The scale's two gates** (`Surface::set_size`, `Renderer::cell_metrics`;
    /// debt since 003) are not merged: this function is the only caller of the
    /// two, the scale is read once here and goes to both from the same local;
    /// the font setting does not touch the scale
    /// (`.tasks/007-ayarlar-ve-tema/discussion.md` → Karar 6).
    ///
    /// The font slot is written here too, at the end: the only path that
    /// (re)builds the atlas is `cell_metrics` and the font notice is current
    /// only after it. Even if a screen change rebuilds the atlas the family is
    /// the same, the slot does not move.
    ///
    /// The scale is from the pane's **window** (`NSView::window`); `None` if the
    /// pane is not attached to a window — an invented scale would build the atlas wrong.
    fn sync_geometry(&self) -> Option<Grid> {
        let scale = self.window()?.backingScaleFactor();
        // The terminal view, not the pane: the two are the same size today but
        // the surface drawn is this view's layer and the size must be its.
        let view = &self.ivars().view;
        let bounds = view.bounds().size;
        let (width_px, height_px) = (bounds.width * scale, bounds.height * scale);
        // The scale is the layer owner's; the pixel size is the surface's
        // configuration (040 → Karar 8).
        self.ivars().layer.setContentsScale(scale);
        self.ivars().surface.set_size(width_px, height_px);

        // The cell size comes from `bt-atlas`'s font metrics through `bt-gpu`
        // and the multiplication by the scale is there too. There is **no**
        // second rounding rule here: the old `.round()` block was deleted on
        // purpose. Had two rules sat side by side, which one wins would depend
        // on call order and the symptom, a one-pixel cell shift, would be silent.
        let renderer = &self.ivars().renderer;
        let cell = renderer.cell_metrics(scale);
        self.host().post_notices(
            self.ivars().id,
            Source::Font,
            font_messages(renderer.font_notice()),
        );
        Some(split_into_grid(
            width_px,
            height_px,
            cell,
            self.ivars().dock_rows.get(),
        ))
    }
}

/// Pane-level actions (R2.3) and scrollback search (033) — the menu
/// selectors and the search panel's controls land here.
impl TerminalPane {
    /// Remote state or title changed: first the upload queue's connection edge
    /// ([`TerminalPane::check_upload_connection`]; if ssh closed the waiting
    /// ones are cancelled — 037 Karar 7 → Kullanıcı kararı 6), then the owner
    /// re-reads the title and the tab's dot ([`PaneHost::title_changed`]).
    pub(crate) fn remote_or_title_changed(&self) {
        self.check_upload_connection();
        // The remote session ended: its helper ssh is not held open until idle.
        if self
            .session()
            .is_some_and(|session| session.remote_target().is_none())
        {
            self.remote_helper().borrow_mut().close();
        }
        self.host().title_changed(self.ivars().id);
    }

    /// Edit ▸ Find ▸ Find… (⌘F): opens the panel, focuses the field and selects
    /// its text (033 Karar 5); if the panel is open, only focus and selection.
    /// If the pane has no query the field fills with the find pasteboard's text (Karar 6).
    pub(crate) fn find(&self) {
        self.open_search(true);
    }

    /// Edit ▸ Find ▸ Find Next (⌘G): the previous, **older** match
    /// (033 Karar 3).
    pub(crate) fn find_next(&self) {
        self.search_step(SearchDirection::Older);
    }

    /// Edit ▸ Find ▸ Find Previous (⇧⌘G): the newer match.
    pub(crate) fn find_previous(&self) {
        self.search_step(SearchDirection::Newer);
    }

    /// Edit ▸ Clear to Start (⌘K; 034 Karar 1): deletes the screen and the
    /// scrollback, the current block stays — `Session::clear_to_start`. No
    /// byte goes to the shell; on the alternate screen the item is grey and
    /// the call is a no-op anyway.
    pub(crate) fn clear_to_start(&self) {
        if let Some(session) = self.session() {
            session.clear_to_start();
        }
    }

    /// Edit ▸ Clear Scrollback (⌥⌘K): scrollback only —
    /// `Session::clear_scrollback`.
    pub(crate) fn clear_scrollback(&self) {
        if let Some(session) = self.session() {
            session.clear_scrollback();
        }
    }

    /// View ▸ Scroll to Top (⌘Home): the start of the scrollback. There is no
    /// new scroll API in `bt-core` (034 Muhakeme): `scroll_page`'s
    /// `saturating_mul` clamps `i32::MAX` pages to the end of the scrollback.
    pub(crate) fn scroll_to_top(&self) {
        self.scroll_pages(i32::MAX);
    }

    /// View ▸ Scroll to Bottom (⌘End): the bottom — `scroll_locked` goes down
    /// to the bottom by the fill-band rule.
    pub(crate) fn scroll_to_bottom(&self) {
        self.scroll_pages(-i32::MAX);
    }

    /// View ▸ Page Up (⌘PgUp): Shift+PgUp's path.
    pub(crate) fn page_up(&self) {
        self.scroll_pages(1);
    }

    /// View ▸ Page Down (⌘PgDn): Shift+PgDn's path.
    pub(crate) fn page_down(&self) {
        self.scroll_pages(-1);
    }

    /// Shell ▸ Cancel Upload (⌘.): this pane's whole upload queue, asking first
    /// if the flowing item has run long ([`TerminalPane::request_stop`]).
    pub(crate) fn cancel_uploads(&self) {
        self.request_stop(true);
    }

    /// View ▸'s four scrolls: `Session::scroll_page`'s path (the very same as
    /// Shift+PgUp/PgDn) — the fraction is reset, the glide generation goes up,
    /// the fill-band rule is in `scroll_locked`. `None` on the alternate screen
    /// and the items are grey anyway; the answer is not read here.
    fn scroll_pages(&self, pages: i32) {
        if let Some(session) = self.session() {
            session.scroll_page(pages);
        }
    }

    /// The search panel — built on the first call, painted with the theme.
    fn search_bar(&self) -> &SearchBar {
        self.ivars().search.get_or_init(|| {
            // The panel is inside the pane, a sibling of the view that carries
            // the Metal layer (033 → R4.1; the pane is that very container,
            // 039 Karar 2). The field's delegate and the controls' target are
            // the pane — both weak, the pane holds the panel.
            let bar = SearchBar::new(
                self.mtm(),
                self,
                self.view(),
                self,
                ProtocolObject::from_ref(self),
            );
            if let Some(session) = self.session() {
                let theme = session.theme();
                bar.paint(&theme, is_dark_background(&theme));
            }
            bar
        })
    }

    /// Opens the panel (leaves it in place if open) and applies the query; if
    /// `focus`, focuses the field and selects its text (⌘F).
    /// `true` if the query was given to the session in this call ([`TerminalPane::apply_search`]).
    fn open_search(&self, focus: bool) -> bool {
        let bar = self.search_bar();
        if bar.query().text.is_empty()
            && let Some(text) = find_pasteboard_text()
        {
            bar.set_text(&text);
        }
        bar.show(!self.ivars().reduce_motion.get());
        if focus && let Some(window) = self.window() {
            window.makeFirstResponder(Some(bar.field()));
            // SAFETY: the sender is optional; the field's own action.
            unsafe { bar.field().selectText(None) };
        }
        self.apply_search()
    }

    /// If the query of the field and switches changed, gives it to the
    /// session, reveals the current match and writes the label; `true` if it
    /// gave. The same query is a no-op.
    fn apply_search(&self) -> bool {
        let (Some(bar), Some(session)) = (self.ivars().search.get(), self.session()) else {
            return false;
        };
        if !bar.is_shown() {
            return false;
        }
        let query = bar.query();
        if !bar.take_change(&query) {
            return false;
        }
        let status = session.set_search(&query);
        self.ivars().search_status.set(status);
        let report = if status == SearchStatus::Ready {
            session.search_reveal(self.search_cover(), self.ivars().smooth_scroll.get())
        } else {
            SearchReport::default()
        };
        bar.set_count(status, report);
        self.kick_search();
        true
    }

    /// Sets up the count index's driver (033 phase-5, Karar 2-B): one chunk on
    /// the next turn of the main queue. A no-op if already set up, if the
    /// panel is closed or if the query is not a pattern to count.
    ///
    /// Its callers: a query change, navigation (a new match whose order is
    /// unknown may want another pass) and the scrollback news
    /// ([`Wake::search_changed`]) — the last one in a background tab too.
    pub(crate) fn kick_search(&self) {
        let shown = self.ivars().search.get().is_some_and(SearchBar::is_shown);
        if !shown
            || self.ivars().search_status.get() != SearchStatus::Ready
            || self.ivars().search_driving.replace(true)
        {
            return;
        }
        self.schedule_search_chunk();
    }

    /// One turn of the driver onto the main queue: the pane is found by id
    /// (`ShellWake`'s pattern), the job drops for a pane that closed.
    fn schedule_search_chunk(&self) {
        let (id, lookup) = (self.ivars().id, self.ivars().lookup);
        DispatchQueue::main().exec_async(move || {
            // audit: a block running on the main queue is on the main thread by definition.
            let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
            if let Some(pane) = lookup(mtm, id) {
                pane.search_chunk();
            }
        });
    }

    /// A chunk of the index and the label; if the count is not finished it is
    /// set up again for the next turn — key events slip in between turns. The
    /// stop condition is the core's `complete` (the pass is done **and** no
    /// pending scrollback news), the panel closing or the search being dropped.
    fn search_chunk(&self) {
        let (Some(bar), Some(session)) = (self.ivars().search.get(), self.session()) else {
            self.ivars().search_driving.set(false);
            return;
        };
        let status = self.ivars().search_status.get();
        if !bar.is_shown() || status != SearchStatus::Ready {
            self.ivars().search_driving.set(false);
            return;
        }
        let Some(report) = session.search_step() else {
            self.ivars().search_driving.set(false);
            return;
        };
        bar.set_count(status, report);
        if report.complete {
            self.ivars().search_driving.set(false);
        } else {
            self.schedule_search_chunk();
        }
    }

    /// ⏎ / ⌘G / ⇧⏎ / ⇧⌘G: if the panel is closed it is opened first (focus
    /// stays in place), then the next match.
    ///
    /// If the opening gave the query **again** (Esc had closed the search) the
    /// step is that selection itself: `set_search` chose and revealed the
    /// nearest match and one more step on top would make ⇧⌘G wrap to the
    /// oldest (`/code-review`).
    fn search_step(&self, direction: SearchDirection) {
        if self.open_search(false) {
            return;
        }
        let (Some(bar), Some(session)) = (self.ivars().search.get(), self.session()) else {
            return;
        };
        let status = self.ivars().search_status.get();
        if status != SearchStatus::Ready {
            return;
        }
        let report = session.search_next(
            direction,
            self.search_cover(),
            self.ivars().smooth_scroll.get(),
        );
        bar.set_count(status, report);
        if !report.complete {
            self.kick_search();
        }
    }

    /// Esc and the close button (033 Karar 5): the panel goes away, **the
    /// window stays in place**, the current match becomes the grid's selection
    /// and the keyboard returns to the terminal. The query stays in the field (Karar 6).
    pub(crate) fn close_search(&self) {
        let Some(bar) = self.ivars().search.get() else {
            return;
        };
        bar.hide(!self.ivars().reduce_motion.get());
        bar.forget_applied();
        if let Some(session) = self.session() {
            session.select_search_match();
            session.clear_search();
        }
        self.ivars().search_status.set(SearchStatus::Empty);
        if let Some(window) = self.window() {
            window.makeFirstResponder(Some(self.view()));
        }
    }

    /// Edit ▸ Find ▸ Use Selection for Find (⌘E; 033 Karar 6): the selection's
    /// first line (grid or dock) becomes the query — escaped in regex mode —,
    /// is written to the find pasteboard and the panel opens with the field focused.
    pub(crate) fn use_selection_for_find(&self) {
        let Some(text) = self.session().and_then(|session| session.selection_text()) else {
            return;
        };
        let bar = self.search_bar();
        let Some(query) = selection_query(&text, bar.regex()) else {
            return;
        };
        bar.set_text(&query);
        // The pasteboard carries **plain** text: other applications do not know regex mode.
        if let Some(plain) = selection_query(&text, false) {
            // SAFETY: a constant name AppKit exposes, lives for the whole process.
            let name = unsafe { NSPasteboardNameFind };
            clipboard::copy(&NSPasteboard::pasteboardWithName(name), Some(plain));
        }
        self.open_search(true);
    }

    /// Gate of Find Next/Previous: whether the pane has a query or the find
    /// pasteboard has text.
    fn has_query(&self) -> bool {
        self.ivars()
            .search
            .get()
            .is_some_and(|bar| !bar.query().text.is_empty())
            || find_pasteboard_text().is_some()
    }

    /// The cells the panel covers; none if the panel is closed. The panel's
    /// coordinates are the pane's (its container is that).
    fn search_cover(&self) -> SearchCover {
        let Some(bar) = self.ivars().search.get().filter(|bar| bar.is_shown()) else {
            return SearchCover::default();
        };
        let view = self.view();
        view.search_cover(view.convertRect_fromView(bar.resting_frame(), Some(self)))
    }

    /// The upload queue (half of `uploader`).
    pub(crate) fn uploads(&self) -> &RefCell<Transfers> {
        &self.ivars().uploads
    }

    /// The helper ssh session's handle (045 Karar 10).
    pub(crate) fn remote_helper(&self) -> &RefCell<RemoteHelper> {
        &self.ivars().remote_helper
    }

    /// `[remote]`'s preview and download keys as last read.
    pub(crate) fn remote_files(&self) -> &RefCell<RemoteFiles> {
        &self.ivars().remote_files
    }

    /// The previews this pane downloaded, by landing path.
    pub(crate) fn previews(&self) -> &RefCell<HashMap<PathBuf, PreviewTicket>> {
        &self.ivars().previews
    }

    /// The file promises of ⌘-dragged remote links ([`crate::promise`]).
    pub(crate) fn finder_drops(&self) -> &RefCell<FinderDrops> {
        &self.ivars().finder
    }

    /// The open upload sheet's slot.
    pub(crate) fn upload_alert(&self) -> &RefCell<Option<Retained<NSAlert>>> {
        &self.ivars().upload_alert
    }

    /// The open stop question's slot.
    pub(crate) fn upload_stop(&self) -> &RefCell<Option<StopSheet>> {
        &self.ivars().upload_stop
    }

    /// The open "Show transfers (N)" popover's slot.
    pub(crate) fn upload_list(&self) -> &RefCell<Option<UploadPopover>> {
        &self.ivars().upload_list
    }

    /// The time of the event that closed the popover.
    pub(crate) fn list_closed_at(&self) -> &Cell<Option<f64>> {
        &self.ivars().list_closed_at
    }

    /// The queue's sent and total bytes; `None` if there is no queue — the
    /// input of the owner's Dock icon total ([`PaneHost::uploads_changed`]).
    pub(crate) fn upload_totals(&self) -> Option<(u64, u64)> {
        self.ivars().uploads.borrow().totals()
    }

    /// Whether the queue is running (Dock icon, `cancelUpload:`'s gate).
    pub(crate) fn upload_active(&self) -> bool {
        self.ivars().uploads.borrow().active()
    }

    /// The arrow and percentage of the title's `↑ N% · ` prefix (`↓` while
    /// only downloads flow); `None` if nothing is flowing (`upload::titled_as`).
    pub(crate) fn upload_title_prefix(&self) -> Option<(&'static str, u8)> {
        self.ivars().uploads.borrow().title_prefix()
    }
}

/// A new tab identity, from `NSUUID` (038 Karar 2).
fn new_tab_id() -> TabId {
    // `UUIDString` gives the canonical 8-4-4-4-12 form; if `parse` rejects it
    // the defect is in `bt-core`'s contract, not on this line.
    TabId::parse(&NSUUID::new().UUIDString().to_string())
        .expect("NSUUID's UUIDString must be a canonical UUID")
}

#[cfg(test)]
mod tests {
    #[test]
    fn tab_ids_are_canonical_and_distinct() {
        let (a, b) = (super::new_tab_id(), super::new_tab_id());
        assert_ne!(a, b, "two NSUUID identities must differ");
        assert_eq!(bt_core::TabId::from_url(&a.url()), Some(a));
    }

    /// Fake owner: records the events with their ids — no window, no
    /// pasteboard (039 phase-2).
    #[derive(Default)]
    struct FakeHost(std::cell::RefCell<Vec<(u64, String)>>);

    impl super::PaneHost for FakeHost {
        fn title_changed(&self, pane: u64) {
            self.0.borrow_mut().push((pane, "title".into()));
        }
        fn focused(&self, pane: u64) {
            self.0.borrow_mut().push((pane, "focused".into()));
        }
        fn shell_exited(&self, pane: u64) {
            self.0.borrow_mut().push((pane, "exit".into()));
        }
        fn uploads_changed(&self, pane: u64) {
            self.0.borrow_mut().push((pane, "uploads".into()));
        }
        fn notify(&self, pane: u64, title: &str, _body: &str) {
            self.0.borrow_mut().push((pane, format!("notify {title}")));
        }
        fn post_notices(&self, pane: u64, _source: crate::notices::Source, _messages: Vec<String>) {
            self.0.borrow_mut().push((pane, "notices".into()));
        }
        fn copy_to_clipboard(&self, pane: u64, text: String) {
            self.0.borrow_mut().push((pane, format!("copy {text}")));
        }
    }

    #[test]
    fn title_and_copy_events_reach_the_host_with_the_pane_id() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let host = FakeHost::default();
        // Title: the flag drops, the event goes to the owner with the pane's id.
        // The pane's edge runs after the flag dropped.
        let pending = AtomicBool::new(true);
        let edge_saw = std::cell::Cell::new(true);
        super::announce_title(
            &pending,
            || edge_saw.set(pending.load(Ordering::Acquire)),
            &host,
            7,
        );
        assert!(!pending.load(Ordering::Acquire));
        assert!(!edge_saw.get(), "the edge ran before the flag dropped");
        // Copy: the text in the slot goes to the owner, not to the general
        // pasteboard; an empty slot produces no event.
        let copy = crate::clipboard::PendingCopy::default();
        assert!(copy.put("osc52".into()));
        super::announce_copy(&copy, &host, 7);
        super::announce_copy(&copy, &host, 7);
        assert_eq!(
            *host.0.borrow(),
            vec![(7, "title".to_owned()), (7, "copy osc52".to_owned())]
        );
    }

    #[test]
    fn remote_probe_repeats_only_while_undecided() {
        use super::RemoteProbe;
        let probe = RemoteProbe::default();
        // When unarmed, output throws no probe.
        assert!(!probe.output());
        // The `C` edge sets the arm and throws a single job; output arriving
        // while a job waits does not throw a second.
        assert!(probe.command_started());
        assert!(!probe.output());
        // The job starts, the probe is undecided: the arm is set back, the
        // next output throws again.
        assert!(probe.begin());
        probe.rearm();
        assert!(probe.output());
        // The job starts, the answer is definitive: the arm stays down, output throws nothing.
        assert!(probe.begin());
        assert!(!probe.output());
        // A new `C` arriving while the definitive answer is being probed is not overwritten.
        assert!(probe.command_started());
        assert!(probe.begin());
        assert!(probe.command_started());
        assert!(probe.begin(), "the new command must be probed");
        // When the arm is down a job that fell into the queue does not probe.
        assert!(!probe.begin());
    }
}
