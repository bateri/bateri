//! ⌘-hover and ⌘-click on a link in the grid, the fill band (044 phase-4) and
//! the dock's input line (phase-5); the ⌘-less OSC 8 hover, the target label
//! and the right-click menu (phase-6).
//!
//! `bt-core` finds the link (`Session::link_at`) and draws its underline from the
//! hover slot (`Session::set_link_hover`); `bt-shell-common::links` says what a
//! path is on disk and what a click does. This module is the AppKit half in
//! between, an `impl BateriView` block next to `view.rs` (the `uploader`
//! precedent):
//!
//! - **⌘ is read on every event** (`mouseMoved:`'s and `flagsChanged:`'s
//!   `modifierFlags`, `NSEvent.modifierFlags` on the asynchronous returns), so
//!   a ⌘ released in another application cannot leave the underline hanging —
//!   the window resigning key clears it too (`windowDidResignKey:`, which also
//!   fires when the application deactivates).
//! - **The hit test runs once per cell and ⌘ state**: motion inside the same
//!   cell does not touch the `Term` lock, motion inside the same link is a
//!   no-op `set_link_hover` (no frame). With ⌘ any link counts (solid
//!   underline, hand cursor); without it only an OSC 8 link does (a **dashed**
//!   underline, no hand — the text does not name its target, a plain-text link
//!   is its own target and every `ls` word would light up; Karar 3).
//! - **⌘ over an OSC 8 link shows its target** in the pane's bottom-left label
//!   ([`crate::pane::TerminalPane::set_link_target`], Karar 7): AppKit's, outside
//!   the frame path.
//! - **A path is verified on a background queue** — a serial queue per view, so
//!   a `stat` hanging on a network disk stalls only this pane's next
//!   verifications, never the main thread or the frame. The answer comes back
//!   to the main queue by pane id ([`crate::pane::PaneLookup`]) and is taken
//!   only if the pointer is still on the same candidate. Until it is back the
//!   path is not underlined and a ⌘-press takes today's route (report or
//!   selection) — the known limit `discussion.md` → Muhakeme names.
//! - **A stale stamp re-finds**: when output, a scroll or a clear moves the
//!   scrollback the frame drops the hover and says so (`Wake::link_hover_lost`);
//!   while the window is key the same point is asked again and, if the link is the
//!   same one, its earlier verification is reused (no second `stat` per output
//!   round). In the dock the stamp is the input line's text: a key that changes
//!   `BUFFER` drops it the same way (`Session::dock`).
//! - **The press is the link's in every mode** (`Gesture::pressed_link`): the
//!   verified hover is locked at the press and the release opens it if the
//!   pointer is still over the locked range and the click count is one.
//!
//! - **A right click on a link opens a menu** (mouse mode off; the band and the
//!   dock are never the application's): "Open Link"/"Copy Link", on a path
//!   "Open"/"Reveal in Finder"/"Copy Path". The path is verified on the same
//!   background queue first — a missing one is no link and gets no menu.
//!
//! Opening — a click and the menu's "Open" alike — follows `links::action`'s
//! white list: a URL and a document go to
//! their default application, a directory opens in Finder, everything else is
//! revealed in Finder, an uncommon OSC 8 scheme asks first and `bateri://` is
//! swallowed. "Is this a known document" is UTType's answer, read through the
//! runtime (`AnyClass::get`, the `updater` precedent) — no new crate.

use std::cell::Cell;
use std::path::Path;
use std::rc::Rc;

use block2::RcBlock;
use bt_core::{CellHalf, LinkHit, LinkKind, LinkPoint, LinkSpan, SelectionPoint, UnderlineStyle};
use bt_gpu::{CellMetrics, Origin};
use dispatch2::{DispatchQueue, DispatchRetained};
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject};
use objc2::{MainThreadMarker, MainThreadOnly, msg_send, sel};
use objc2_app_kit::{
    NSAlert, NSAlertSecondButtonReturn, NSEvent, NSEventModifierFlags, NSMenu, NSMenuItem,
    NSModalResponse, NSPasteboard, NSWorkspace,
};
use objc2_foundation::{NSArray, NSPoint, NSRect, NSSize, NSString, NSURL, ns_string};

use crate::child;
use crate::clipboard;
use crate::links::{self, Content, LinkAction, Resolved};
use crate::uploader::{ESCAPE, add_key_monitor, remove_monitor};
use crate::view::{BateriView, OutOfGrid};

/// A link whose target is known to be one: a URL or OSC 8 link as found, a path
/// after the background `stat` found it.
#[derive(Clone, Debug)]
pub(crate) struct Verified {
    hit: LinkHit,
    /// What the path is on disk; `None` for a link that names no local path.
    resolved: Option<Resolved>,
}

/// A cell the link hit test can be asked about: a **signed** screen row
/// (negative is the fill band, [`link_cell_at`]) or a row of the dock's input
/// block inside its drawn vertical window (`window_point_dock`). Two variants,
/// so a dock row `0` and a screen row `0` never compare equal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LinkCell {
    Screen(i32, u16),
    Dock(u16, u16),
}

impl LinkCell {
    fn point(self) -> LinkPoint {
        match self {
            LinkCell::Screen(row, col) => LinkPoint::Screen { row, col },
            LinkCell::Dock(row, col) => LinkPoint::Dock(SelectionPoint {
                col,
                row,
                half: CellHalf::Left,
            }),
        }
    }
}

/// The view's link state ([`BateriView::link_state`]); main thread only.
#[derive(Default)]
pub(crate) struct LinkState {
    /// The link cell the last motion asked about — the hit test's notch, with
    /// [`LinkState::command`].
    cell: Option<LinkCell>,
    /// Whether ⌘ was down when [`LinkState::cell`] was asked: pressing or
    /// releasing ⌘ over the same cell asks again, another modifier does not.
    command: bool,
    /// The drawn (verified) hover: a ⌘-press is matched against it.
    hover: Option<Verified>,
    /// Whether [`LinkState::hover`] is drawn the ⌘ way (solid, hand cursor) —
    /// `false` for the ⌘-less OSC 8 hover (dashed, no hand).
    command_hover: bool,
    /// A path candidate whose `stat` is in flight.
    pending: Option<LinkHit>,
    /// The last candidate the `stat` did not find: moving inside it does not ask again.
    missing: Option<LinkHit>,
    /// The hover locked at a ⌘-press (044 R6); the release compares with it.
    pressed: Option<Verified>,
    /// A right click's path candidate whose `stat` is in flight: the menu pops
    /// only if it is still this one (a later press or a cleared hover forgets it,
    /// so a `stat` hanging on a network disk cannot pop a menu much later).
    menu_pending: Option<LinkHit>,
    /// The link the open context menu acts on; its items take it.
    menu: Option<Verified>,
    /// The serial queue of the path verifications, born at the first one.
    queue: Option<DispatchRetained<DispatchQueue>>,
}

/// Whether two hits are the same link — the same cells, target and kind; the
/// stamp is left out (it moves with every output round while the link stays).
fn same_link(a: &LinkHit, b: &LinkHit) -> bool {
    a.spans == b.spans && a.target == b.target && a.kind == b.kind
}

/// Whether `cell` is one of the link's cells — on the surface the link was
/// found on ([`LinkHit::in_dock`]).
fn on_link(hit: &LinkHit, cell: LinkCell) -> bool {
    spans_contain(&hit.spans, hit.in_dock(), cell)
}

/// How a hit is drawn with ⌘ down or up (Karar 3): with ⌘ every link solid;
/// without it only an OSC 8 link, dashed — the plain-text link is not
/// highlighted at all. `None` → nothing to show.
fn hover_style(hit: &LinkHit, command: bool) -> Option<UnderlineStyle> {
    if command {
        Some(UnderlineStyle::Single)
    } else if hit.kind == LinkKind::Osc8 {
        Some(UnderlineStyle::Dashed)
    } else {
        None
    }
}

/// What a verification is for: the hover, or the context menu popping at a
/// point of the view.
#[derive(Clone, Copy)]
enum Then {
    Hover,
    Menu(NSPoint),
}

/// Whether `cell` is one of `spans`' cells, `in_dock` saying which surface's.
fn spans_contain(spans: &[LinkSpan], in_dock: bool, cell: LinkCell) -> bool {
    let (row, col) = match cell {
        LinkCell::Screen(row, col) if !in_dock => (row, col),
        LinkCell::Dock(row, col) if in_dock => (i32::from(row), col),
        _ => return false,
    };
    spans
        .iter()
        .any(|span| span.row == row && (span.first..=span.last).contains(&col))
}

/// Mouse point → the link hit test's cell ([`LinkPoint::Screen`]): a **signed**
/// screen row (negative is the fill band above the origin, `-1` its bottom row —
/// [`crate::view::cover_of`]'s arithmetic, floor instead of ceiling) and a column.
/// Pure, the inputs of [`crate::view::point_to_cell`].
///
/// `None` outside the grid's columns, in the left padding and below the grid
/// (the dock band). Above the origin nothing is rejected here: whether a band
/// row stands on screen is `bt-core`'s question (`drawn_lines`), asked under the
/// `Term` lock with the frame's own numbers.
pub(crate) fn link_cell_at(
    view_px: (f64, f64),
    metrics: CellMetrics,
    origin_px: f64,
    scale: f64,
    cols: u16,
    rows: u16,
) -> Option<(i32, u16)> {
    let (cell_w, cell_h) = metrics.cell_px();
    let (cell_w, cell_h) = (f64::from(cell_w.max(1)), f64::from(cell_h.max(1)));
    let x = view_px.0 * scale - f64::from(metrics.gutter_px());
    let y = view_px.1 * scale - origin_px;
    if x < 0.0 || x >= cell_w * f64::from(cols) || !y.is_finite() {
        return None;
    }
    // `as` saturates: a giant negative y is a row no band has.
    let row = (y / cell_h).floor() as i32;
    (row < i32::from(rows)).then_some((row, (x / cell_w) as u16))
}

/// The link's cells as rectangles in **physical pixels** of the flipped view —
/// `(x, y, width, height)`, one per span: the inverse of [`link_cell_at`], so the
/// hand cursor covers exactly the cells the hit test answers for.
fn span_rects_px(spans: &[LinkSpan], metrics: CellMetrics, origin_px: f64) -> Vec<[f64; 4]> {
    let (cell_w, cell_h) = metrics.cell_px();
    let (cell_w, cell_h) = (f64::from(cell_w), f64::from(cell_h));
    spans
        .iter()
        .map(|span| {
            let width = f64::from(span.last.saturating_sub(span.first) + 1) * cell_w;
            [
                f64::from(metrics.gutter_px()) + f64::from(span.first) * cell_w,
                origin_px + f64::from(span.row) * cell_h,
                width,
                cell_h,
            ]
        })
        .collect()
}

/// The UTTypes a file conforms to to be opened in its default application
/// (`links::Content::Document`): text and source code, image, PDF, audio/video
/// (`plan.md` → R5.1).
const DOCUMENT_TYPES: [&str; 6] = [
    "public.plain-text",
    "public.source-code",
    "public.json",
    "public.image",
    "com.adobe.pdf",
    "public.audiovisual-content",
];

/// The UTTypes that are **never** a document, even when they also conform to a
/// text type: `public.shell-script` is `public.source-code`, and "opening" a
/// `.command` or a `.py` runs it (Terminal, Python Launcher). Asked first.
const NEVER_DOCUMENT: [&str; 2] = ["public.script", "public.executable"];

/// The content class of a file name's extension, from UTType (044 R5.1).
/// `Other` without an extension, for an unknown one and if the runtime has no
/// `UTType` class — the white list's safe side (the file is revealed).
fn extension_content(ext: &str) -> Content {
    let Some(class) = AnyClass::get(c"UTType") else {
        return Content::Other;
    };
    // SAFETY: `+[UTType typeWithFilenameExtension:]` takes an `NSString` and
    // returns a nullable `UTType`.
    let ty: Option<Retained<AnyObject>> =
        unsafe { msg_send![class, typeWithFilenameExtension: &*NSString::from_str(ext)] };
    let Some(ty) = ty else {
        return Content::Other;
    };
    let conforms = |identifier: &str| {
        // SAFETY: `+[UTType typeWithIdentifier:]` takes an `NSString` and returns
        // a nullable `UTType`; `-conformsToType:` takes a `UTType`, returns `BOOL`.
        let target: Option<Retained<AnyObject>> =
            unsafe { msg_send![class, typeWithIdentifier: &*NSString::from_str(identifier)] };
        target.is_some_and(|target| unsafe { msg_send![&*ty, conformsToType: &*target] })
    };
    if NEVER_DOCUMENT.iter().any(|id| conforms(id)) {
        Content::Other
    } else if DOCUMENT_TYPES.iter().any(|id| conforms(id)) {
        Content::Document
    } else {
        Content::Other
    }
}

/// `links::action`'s content oracle: a package (`.app`, `.pkg`, a bundle) first —
/// opening it would launch or install it —, then the extension's type.
///
/// On the main thread at click time: `isFilePackageAtPath` reads the disk, but
/// only once per click on a path the background `stat` already found.
fn content_of(path: &Path) -> Content {
    let text = NSString::from_str(&path.to_string_lossy());
    if NSWorkspace::sharedWorkspace().isFilePackageAtPath(&text) {
        return Content::Package;
    }
    path.extension()
        .and_then(|ext| ext.to_str())
        .map_or(Content::Other, extension_content)
}

fn file_url(path: &Path) -> Retained<NSURL> {
    NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()))
}

/// Hands a URL string to its default application; a string `NSURL` rejects does nothing.
fn open_url(url: &str) {
    if let Some(url) = NSURL::URLWithString(&NSString::from_str(url)) {
        NSWorkspace::sharedWorkspace().openURL(&url);
    }
}

/// Whether ⌘ is down **now** — the asynchronous returns have no event.
fn command_down() -> bool {
    NSEvent::modifierFlags_class().contains(NSEventModifierFlags::Command)
}

impl BateriView {
    /// The pointer's link cell at a window point: the grid and the band
    /// ([`link_cell_at`]), below them the dock's input block (the dock
    /// selection's geometry, `window_point_dock`; the context line, the band's
    /// padding and the mark's columns are `bt-core`'s rejection).
    fn link_cell(&self, in_window: NSPoint) -> Option<LinkCell> {
        let (metrics, (cols, rows)) = self.metrics()?;
        let point = self.convertPoint_fromView(in_window, None);
        let scale = self.window()?.backingScaleFactor();
        let origin = self.origin().map_or(0.0, Origin::px);
        link_cell_at(
            (point.x, point.y),
            metrics,
            f64::from(origin),
            scale,
            cols,
            rows,
        )
        .map(|(row, col)| LinkCell::Screen(row, col))
        .or_else(|| {
            self.window_point_dock(in_window, OutOfGrid::Reject)
                .map(|point| LinkCell::Dock(point.row, point.col))
        })
    }

    /// The pointer's link cell now, without an event.
    fn pointer_link_cell(&self) -> Option<LinkCell> {
        self.link_cell(self.window()?.mouseLocationOutsideOfEventStream())
    }

    /// `mouseMoved:`: asks for the link under a **new** cell or a new ⌘ state
    /// ([`hover_style`] — any link with ⌘, an OSC 8 link without). Motion inside
    /// the same cell costs one flag test and one borrow — no `Term` lock.
    pub(crate) fn link_motion(&self, event: &NSEvent) {
        let command = event
            .modifierFlags()
            .contains(NSEventModifierFlags::Command);
        let at = self.link_cell(event.locationInWindow());
        self.ask_link(at, command);
    }

    /// `flagsChanged:`: ⌘ went down or up → the link under the pointer asked
    /// again with the new state (solid ↔ dashed, or cleared). A path candidate
    /// the `stat` did not find is forgotten when ⌘ goes up (it is asked again
    /// at the next ⌘ — a file created meanwhile is found).
    pub(crate) fn link_flags(&self, event: &NSEvent) {
        let command = event
            .modifierFlags()
            .contains(NSEventModifierFlags::Command);
        if !command {
            self.link_state().borrow_mut().missing = None;
        }
        self.ask_link(self.pointer_link_cell(), command);
    }

    /// The notch: the hit test runs only if the cell or the ⌘ state changed.
    fn ask_link(&self, at: Option<LinkCell>, command: bool) {
        {
            let mut state = self.link_state().borrow_mut();
            if state.cell == at && state.command == command {
                return;
            }
            state.cell = at;
            state.command = command;
        }
        self.find_link(at, command);
    }

    /// The frame dropped a stale hover (`Wake::link_hover_lost`): if the window
    /// is key the same point is asked again with ⌘'s state now, otherwise
    /// everything clears — the only thing that stops the drop → re-find cycle
    /// when the window is not the user's.
    pub(crate) fn link_lost(&self) {
        let key = self.window().is_some_and(|window| window.isKeyWindow());
        if key {
            let (at, command) = (self.pointer_link_cell(), command_down());
            {
                let mut state = self.link_state().borrow_mut();
                state.cell = at;
                state.command = command;
            }
            self.find_link(at, command);
        } else {
            self.clear_link();
        }
    }

    /// Removes the hover and forgets the in-flight candidates; a press already
    /// locked stays (⌘ is read at the press). Idempotent and cheap when nothing is shown.
    pub(crate) fn clear_link(&self) {
        let shown = {
            let mut state = self.link_state().borrow_mut();
            state.cell = None;
            state.command = false;
            state.pending = None;
            state.missing = None;
            state.menu_pending = None;
            state.hover.take().is_some()
        };
        if shown {
            self.hide_hover();
        }
    }

    /// The hit test at `at` and what follows from it, `command` saying which
    /// links count and how they are drawn ([`hover_style`]).
    fn find_link(&self, at: Option<LinkCell>, command: bool) {
        let Some(session) = self.session() else {
            return;
        };
        let hit = at
            .and_then(|cell| session.link_at(cell.point()))
            .filter(|hit| hover_style(hit, command).is_some());
        let Some(hit) = hit else {
            let shown = {
                let mut state = self.link_state().borrow_mut();
                state.pending = None;
                state.hover.take().is_some()
            };
            if shown {
                self.hide_hover();
            }
            return;
        };
        // The same link as the shown one (re-found after output, the pointer
        // moved inside it or ⌘ changed its style): only the stamp may be new, the
        // verification stands.
        let reuse = {
            let state = self.link_state().borrow();
            state
                .hover
                .as_ref()
                .filter(|shown| same_link(&shown.hit, &hit))
                .map(|shown| shown.resolved.clone())
        };
        if let Some(resolved) = reuse {
            self.show_link(Verified { hit, resolved }, command);
            return;
        }
        let Some(path) = links::local_path(&hit.target, &hit.kind) else {
            self.show_link(
                Verified {
                    hit,
                    resolved: None,
                },
                command,
            );
            return;
        };
        let (in_flight, missing, shown) = {
            let mut state = self.link_state().borrow_mut();
            let in_flight = state.pending.as_ref().is_some_and(|p| same_link(p, &hit));
            let missing = state.missing.as_ref().is_some_and(|m| same_link(m, &hit));
            // Whatever is shown is another link (the same one returned above).
            let shown = state.hover.take().is_some();
            // A new candidate, or the in-flight one with a fresher stamp; a
            // candidate known to be missing waits for nothing.
            state.pending = (!missing).then(|| hit.clone());
            (in_flight, missing, shown)
        };
        if shown {
            self.hide_hover();
        }
        if in_flight || missing {
            return;
        }
        self.verify_path(path, hit, Then::Hover);
    }

    /// Throws the `stat` to the view's serial queue; the answer returns to the
    /// main queue by pane id — to the hover ([`BateriView::link_verified`]) or to
    /// the context menu ([`BateriView::link_menu_verified`]).
    fn verify_path(&self, path: std::path::PathBuf, hit: LinkHit, then: Then) {
        let (Some(pane), Some(session)) = (self.pane(), self.session()) else {
            return;
        };
        let (id, lookup) = (pane.id(), pane.lookup());
        let cwd = session.working_directory();
        let mut state = self.link_state().borrow_mut();
        let queue = state
            .queue
            .get_or_insert_with(|| DispatchQueue::new("dev.bateri.link", None));
        queue.exec_async(move || {
            // `home` may read the passwd entry: here, off the main thread.
            let home = child::home();
            let resolved = links::resolve(&path, cwd.as_deref(), home.as_deref(), links::stat);
            DispatchQueue::main().exec_async(move || {
                // audit: a block running on the main queue is on the main thread by definition.
                let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
                if let Some(pane) = lookup(mtm, id) {
                    match then {
                        Then::Hover => pane.view().link_verified(&hit, resolved),
                        Then::Menu(at) => pane.view().link_menu_verified(&hit, resolved, at),
                    }
                }
            });
        });
    }

    /// The `stat`'s answer: taken only if the pointer is still on the same
    /// candidate (the view's pending one) and the window is key; drawn with ⌘'s
    /// state **now** ([`hover_style`]).
    pub(crate) fn link_verified(&self, hit: &LinkHit, resolved: Option<Resolved>) {
        let pending = {
            let mut state = self.link_state().borrow_mut();
            if !state.pending.as_ref().is_some_and(|p| same_link(p, hit)) {
                return;
            }
            let pending = state.pending.take();
            if resolved.is_none() {
                state.missing = pending;
                return;
            }
            pending
        };
        let key = self.window().is_some_and(|window| window.isKeyWindow());
        let command = command_down();
        match pending {
            Some(hit) if key && hover_style(&hit, command).is_some() => {
                self.show_link(Verified { hit, resolved }, command);
            }
            _ => self.clear_link(),
        }
    }

    /// Draws `link` underlined in `command`'s style ([`hover_style`]); with ⌘
    /// its cells are the hand cursor's and an OSC 8 target shows in the pane's
    /// label.
    fn show_link(&self, link: Verified, command: bool) {
        let (Some(session), Some(style)) = (self.session(), hover_style(&link.hit, command)) else {
            return;
        };
        session.set_link_hover(Some(link.hit.hover(style)));
        let label = (command && link.hit.kind == LinkKind::Osc8).then(|| link.hit.target.clone());
        {
            let mut state = self.link_state().borrow_mut();
            state.pending = None;
            state.hover = Some(link);
            state.command_hover = command;
        }
        if let Some(pane) = self.pane() {
            pane.set_link_target(label.as_deref());
        }
        self.sync_cursor_rects();
    }

    fn hide_hover(&self) {
        if let Some(session) = self.session() {
            session.set_link_hover(None);
        }
        if let Some(pane) = self.pane() {
            pane.set_link_target(None);
        }
        self.sync_cursor_rects();
    }

    /// The hand cursor's rectangles over the shown link, in view points — only
    /// with ⌘ (the dashed ⌘-less hover is a hint, a click there selects).
    pub(crate) fn link_rects(&self) -> Vec<NSRect> {
        let state = self.link_state().borrow();
        let Some(shown) = state.hover.as_ref().filter(|_| state.command_hover) else {
            return Vec::new();
        };
        let (Some((metrics, _)), Some(window)) = (self.metrics(), self.window()) else {
            return Vec::new();
        };
        let scale = window.backingScaleFactor();
        // A dock link's rows are the input block's, from its drawn top
        // (`Origin::dock`, the geometry `window_point_dock` reads).
        let origin = if shown.hit.in_dock() {
            match self.origin().and_then(Origin::dock) {
                Some((top, _)) => top,
                None => return Vec::new(),
            }
        } else {
            self.origin().map_or(0.0, Origin::px)
        };
        span_rects_px(&shown.hit.spans, metrics, f64::from(origin))
            .into_iter()
            .map(|[x, y, width, height]| {
                NSRect::new(
                    NSPoint::new(x / scale, y / scale),
                    NSSize::new(width / scale, height / scale),
                )
            })
            .collect()
    }

    /// A left press: `true` if it is a ⌘-press on the shown link's cells — the
    /// hover is locked for the release and the caller routes the gesture to the
    /// link (`Gesture::pressed_link`), calling neither the report nor the selection.
    pub(crate) fn link_press(&self, event: &NSEvent) -> bool {
        if !event
            .modifierFlags()
            .contains(NSEventModifierFlags::Command)
        {
            return false;
        }
        let Some(at) = self.link_cell(event.locationInWindow()) else {
            return false;
        };
        let mut state = self.link_state().borrow_mut();
        let Some(shown) = state.hover.clone() else {
            return false;
        };
        if !on_link(&shown.hit, at) {
            return false;
        }
        state.pressed = Some(shown);
        true
    }

    /// `Release::Link`: opens the locked link if the pointer is still over its
    /// range (the macOS button rule — slide off to cancel) and this is the first
    /// click (a double click opens once). The hit test does not run again.
    pub(crate) fn link_release(&self, event: &NSEvent) {
        let Some(pressed) = self.link_state().borrow_mut().pressed.take() else {
            return;
        };
        if event.clickCount() != 1 {
            return;
        }
        let over = self
            .link_cell(event.locationInWindow())
            .is_some_and(|at| on_link(&pressed.hit, at));
        if over {
            self.open_link(&pressed);
        }
    }

    /// The click's action (`links::action`, the white list).
    fn open_link(&self, link: &Verified) {
        let action = links::action(
            &link.hit.target,
            &link.hit.kind,
            link.resolved.as_ref(),
            content_of,
        );
        let workspace = NSWorkspace::sharedWorkspace();
        match action {
            Some(LinkAction::OpenUrl(url)) => open_url(&url),
            Some(LinkAction::OpenFile(path) | LinkAction::OpenDir(path)) => {
                workspace.openURL(&file_url(&path));
            }
            Some(LinkAction::Reveal(path)) => {
                workspace.activateFileViewerSelectingURLs(&NSArray::from_retained_slice(&[
                    file_url(&path),
                ]));
            }
            Some(LinkAction::Confirm(target)) => self.confirm_open(target),
            Some(LinkAction::Swallow) | None => {}
        }
    }

    /// A right press the terminal owns — mouse mode off (or Shift, its escape)
    /// in the grid, always on the fill band and the dock's input line (never the
    /// application's screen): the context menu if a link is under it (Karar 7).
    /// No ⌘ needed. A link the hover already verified pops at once; a path
    /// candidate is `stat`ed on the background queue first and pops on its
    /// return ([`BateriView::link_menu_verified`]) — a missing path is no link and
    /// gets no menu. Without a link nothing happens, as before.
    pub(crate) fn link_menu(&self, event: &NSEvent) {
        let in_window = event.locationInWindow();
        let (Some(at), Some(session)) = (self.link_cell(in_window), self.session()) else {
            return;
        };
        let Some(hit) = session.link_at(at.point()) else {
            return;
        };
        let point = self.convertPoint_fromView(in_window, None);
        let reuse = {
            let state = self.link_state().borrow();
            state
                .hover
                .as_ref()
                .filter(|shown| same_link(&shown.hit, &hit))
                .map(|shown| shown.resolved.clone())
        };
        if let Some(resolved) = reuse {
            self.pop_link_menu(Verified { hit, resolved }, point);
            return;
        }
        match links::local_path(&hit.target, &hit.kind) {
            None => self.pop_link_menu(
                Verified {
                    hit,
                    resolved: None,
                },
                point,
            ),
            Some(path) => {
                self.link_state().borrow_mut().menu_pending = Some(hit.clone());
                self.verify_path(path, hit, Then::Menu(point));
            }
        }
    }

    /// A press forgets the menu still waiting for its `stat` — it would pop
    /// after the user moved on.
    pub(crate) fn forget_link_menu(&self) {
        self.link_state().borrow_mut().menu_pending = None;
    }

    /// The right click's `stat` came back: the menu pops at the click's point
    /// if this is still the waited-for candidate, the path exists and the
    /// window is still key.
    pub(crate) fn link_menu_verified(
        &self,
        hit: &LinkHit,
        resolved: Option<Resolved>,
        at: NSPoint,
    ) {
        let pending = {
            let mut state = self.link_state().borrow_mut();
            if !state
                .menu_pending
                .as_ref()
                .is_some_and(|p| same_link(p, hit))
            {
                return;
            }
            state.menu_pending.take()
        };
        let key = self.window().is_some_and(|window| window.isKeyWindow());
        if let (Some(hit), true, true) = (pending, resolved.is_some(), key) {
            self.pop_link_menu(Verified { hit, resolved }, at);
        }
    }

    /// The context menu at `at` (view points): on a path that exists "Open",
    /// "Reveal in Finder", "Copy Path", on any other link "Open Link", "Copy
    /// Link". A link the policy has nothing for (`bateri://`) gets no menu.
    fn pop_link_menu(&self, link: Verified, at: NSPoint) {
        let action = links::action(
            &link.hit.target,
            &link.hit.kind,
            link.resolved.as_ref(),
            |_| Content::Other,
        );
        if matches!(action, None | Some(LinkAction::Swallow)) {
            return;
        }
        let mtm = self.mtm();
        let items: &[(&NSString, objc2::runtime::Sel)] = if link.resolved.is_some() {
            &[
                (ns_string!("Open"), sel!(openLinkFromMenu:)),
                (ns_string!("Reveal in Finder"), sel!(revealLinkFromMenu:)),
                (ns_string!("Copy Path"), sel!(copyLinkFromMenu:)),
            ]
        } else {
            &[
                (ns_string!("Open Link"), sel!(openLinkFromMenu:)),
                (ns_string!("Copy Link"), sel!(copyLinkFromMenu:)),
            ]
        };
        let menu = NSMenu::new(mtm);
        for &(title, action) in items {
            // SAFETY: `initWithTitle:action:keyEquivalent:` takes two `NSString`s
            // and a selector the target implements (`BateriView`'s link menu
            // selectors); `setTarget:` keeps the target weakly and the view
            // outlives the menu's modal tracking.
            let item = unsafe {
                let item = NSMenuItem::initWithTitle_action_keyEquivalent(
                    NSMenuItem::alloc(mtm),
                    title,
                    Some(action),
                    ns_string!(""),
                );
                item.setTarget(Some(self));
                item
            };
            menu.addItem(&item);
        }
        self.link_state().borrow_mut().menu = Some(link);
        menu.popUpMenuPositioningItem_atLocation_inView(None, at, Some(self));
    }

    /// "Open" / "Open Link": the click's own path ([`BateriView::open_link`]) —
    /// the menu does not skip the policy, an uncommon scheme asks here too.
    pub(crate) fn menu_open_link(&self) {
        let link = self.link_state().borrow_mut().menu.take();
        if let Some(link) = link {
            self.open_link(&link);
        }
    }

    /// "Reveal in Finder": always safe, whatever the file is.
    pub(crate) fn menu_reveal_link(&self) {
        let link = self.link_state().borrow_mut().menu.take();
        if let Some(Resolved { path, .. }) = link.and_then(|link| link.resolved) {
            NSWorkspace::sharedWorkspace()
                .activateFileViewerSelectingURLs(&NSArray::from_retained_slice(&[file_url(&path)]));
        }
    }

    /// "Copy Path" (the resolved absolute path) / "Copy Link" (the target).
    pub(crate) fn menu_copy_link(&self) {
        let link = self.link_state().borrow_mut().menu.take();
        let text = link.map(|link| match link.resolved {
            Some(Resolved { path, .. }) => path.to_string_lossy().into_owned(),
            None => link.hit.target,
        });
        clipboard::copy(&NSPasteboard::generalPasteboard(), text);
    }

    /// The sheet for an OSC 8 link with an uncommon scheme: the whole target,
    /// "Cancel" (the default — Return — and Esc) and "Open". Dropped if the window
    /// already has a sheet (two sheets cannot open on top of each other).
    ///
    /// Esc by hand, for the reason `uploader`'s stop question gives: the first
    /// button carries Return and a button has one key equivalent.
    fn confirm_open(&self, target: String) {
        let (Some(window), Some(pane)) = (self.window(), self.pane()) else {
            return;
        };
        if window.attachedSheet().is_some() {
            return;
        }
        let mtm = self.mtm();
        let alert = NSAlert::new(mtm);
        alert.setMessageText(ns_string!("Open this link?"));
        alert.setInformativeText(&NSString::from_str(&target));
        alert.addButtonWithTitle(ns_string!("Cancel"));
        alert.addButtonWithTitle(ns_string!("Open"));
        let monitor: Rc<Cell<Option<Retained<AnyObject>>>> = Rc::default();
        let sheet_window = alert.window();
        // The pane by id, not the window: a monitor that outlived its sheet must
        // not pin the window (`uploader`'s precedent).
        let (id, lookup) = (pane.id(), pane.lookup());
        let installed = add_key_monitor(move |event| {
            if event.keyCode() != ESCAPE {
                return false;
            }
            // audit: the local event monitor runs on the main thread.
            let mtm = MainThreadMarker::new().expect("the event monitor is on the main thread");
            let on_sheet = event
                .window(mtm)
                .is_some_and(|window| Retained::as_ptr(&window) == Retained::as_ptr(&sheet_window));
            if on_sheet && let Some(parent) = lookup(mtm, id).and_then(|pane| pane.window()) {
                parent.endSheet_returnCode(&sheet_window, objc2_app_kit::NSAlertFirstButtonReturn);
            }
            on_sheet
        });
        monitor.set(installed);
        let slot = Rc::clone(&monitor);
        let answered = RcBlock::new(move |response: NSModalResponse| {
            remove_monitor(slot.take());
            if response == NSAlertSecondButtonReturn {
                open_url(&target);
            }
        });
        alert.beginSheetModalForWindow_completionHandler(&window, Some(&answered));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bt_gpu::CellMetrics;

    /// A 10×20 px cell with an 8 px left padding.
    fn metrics() -> CellMetrics {
        CellMetrics::new(10, 20, 15, 8, 1).expect("non-zero cell")
    }

    #[test]
    fn the_link_cell_is_signed_above_the_origin_and_rejects_the_dock_and_the_padding() {
        let m = metrics();
        // Origin 100 px, scale 1: row 0 starts at y = 100.
        let at = |x: f64, y: f64| link_cell_at((x, y), m, 100.0, 1.0, 80, 24);
        assert_eq!(at(8.0, 100.0), Some((0, 0)));
        assert_eq!(at(27.9, 139.9), Some((1, 1)));
        // Above the origin: the fill band's rows, `-1` the bottom one (floor, not ceiling).
        assert_eq!(at(18.0, 99.0), Some((-1, 1)));
        assert_eq!(at(18.0, 80.0), Some((-1, 1)));
        assert_eq!(at(18.0, 79.0), Some((-2, 1)));
        // The left padding, past the last column and below the grid (the dock).
        assert_eq!(at(7.0, 110.0), None);
        assert_eq!(at(8.0 + 800.0, 110.0), None);
        assert_eq!(at(8.0, 100.0 + 24.0 * 20.0), None);
        // Retina: points go up to pixels first.
        assert_eq!(
            link_cell_at((9.0, 60.0), m, 100.0, 2.0, 80, 24),
            Some((1, 1))
        );
    }

    #[test]
    fn the_hand_rects_are_the_inverse_of_the_link_cell() {
        let m = metrics();
        let spans = [
            LinkSpan {
                row: -1,
                first: 3,
                last: 79,
            },
            LinkSpan {
                row: 0,
                first: 0,
                last: 4,
            },
        ];
        let rects = span_rects_px(&spans, m, 100.0);
        assert_eq!(rects[0], [38.0, 80.0, 770.0, 20.0]);
        assert_eq!(rects[1], [8.0, 100.0, 50.0, 20.0]);
        for (span, [x, y, w, h]) in spans.iter().zip(&rects) {
            // The rectangle's corners fall in the span's first and last cells.
            let first = link_cell_at((*x, *y), m, 100.0, 1.0, 80, 24);
            let last = link_cell_at((x + w - 0.5, y + h - 0.5), m, 100.0, 1.0, 80, 24);
            assert_eq!(first, Some((span.row, span.first)));
            assert_eq!(last, Some((span.row, span.last)));
        }
        assert!(spans_contain(&spans, false, LinkCell::Screen(0, 4)));
        assert!(!spans_contain(&spans, false, LinkCell::Screen(0, 5)));
        assert!(!spans_contain(&spans, false, LinkCell::Screen(-1, 2)));
        // A dock row 0 is not the screen's row 0, and the reverse.
        assert!(!spans_contain(&spans, false, LinkCell::Dock(0, 4)));
        assert!(spans_contain(&spans, true, LinkCell::Dock(0, 4)));
        assert!(!spans_contain(&spans, true, LinkCell::Screen(0, 4)));
    }

    /// Reads this Mac's Launch Services database (an application declaring an
    /// extension can change UTType's answer): a red here is the environment's
    /// first, then the code's — the `child` tests' real-zsh precedent.
    #[test]
    fn documents_open_and_scripts_and_executables_never_do() {
        for ext in ["txt", "md", "json", "png", "jpg", "pdf", "mp4", "c"] {
            assert_eq!(extension_content(ext), Content::Document, ".{ext}");
        }
        // A script is source code too — and opening it runs it.
        for ext in [
            "command", "sh", "py", "tool", "app", "pkg", "terminal", "webloc",
        ] {
            assert_eq!(extension_content(ext), Content::Other, ".{ext}");
        }
        assert_eq!(extension_content("no-such-extension-xyz"), Content::Other);
    }
}
