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
//! **A tab's question sits on the tab's own owner.** A sheet on the terminal
//! window would block the whole window — the tab bar too — and the user
//! could no longer leave a tab whose question is up, as they could when
//! every tab was a window of its own. So a pane's sheet goes on its tab's
//! **owner** ([`Owner`]): a borderless, clear child window over the tab's
//! splits container, born with the tab's first sheet and taken apart when
//! its last one ends. The bar is outside it and keeps its clicks (measured);
//! a click on the content reaches the owner, like a click on a window under
//! a sheet, not the pane under the question. When the tab leaves the screen
//! its owner is ordered out and the sheet goes with it, up and answerable;
//! when the tab comes back the owner is attached again and its sheet takes
//! the keyboard ([`show_owner`]). The terminal window is not blocked by a
//! sheet on another window, so whenever it becomes key it hands the keyboard
//! to the question on screen ([`key_to_sheet`]) — typing never reaches the
//! pane under it. The owner does not follow its parent's frame changes
//! (measured), so it is fitted to the container wherever the container is
//! placed and when the window moves ([`fit_owner`]).
//!
//! A **window's** question still sits on the window and blocks it, bar
//! included: the selection does not move under it ([`window_asks`]). A
//! window's question does not open over a tab's question on screen, nor a
//! tab's over the window's: whichever comes second waits.
//!
//! **A question that cannot open now waits.** A pane whose tab is not shown
//! ([`TerminalPane::tab_shown`]), or whose tab or window already shows a
//! sheet, **parks** its question in its own slot ([`TerminalPane::parked`])
//! — the alert or panel with its completion block, as begun — and it opens
//! when the tab comes on screen or the sheet in its way ends
//! ([`open_parked`]). Everything the asker holds stays exactly as when it
//! asked: a parked password question still holds its reply, so the job
//! waits as it would on an open sheet, and an update's wait sees it. That is
//! why the decision has a single place: a call that went around it would
//! open the question over the front tab, silently. AppKit's own queue for a
//! second sheet on one window is not used: it could open a sheet on a tab's
//! owner while the tab is off screen, and a sheet brings its window back on
//! screen.
//!
//! **The seat is resolved from the pane object, not by id.** A pane's
//! close answers its open sheet (`TerminalPane::close_password` inside
//! `begin_close`) after the pane has left its split tree, where an id lookup
//! through the application's lists no longer finds it — the sheet would stay
//! up over a closed pane. The pane knows where it stands — its superview is
//! its tab's container, which holds the owner ([`OwnerSlot`]); the owner's
//! events (`PaneHost`) carry no AppKit type for it.
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

use std::cell::RefCell;
use std::collections::VecDeque;

use block2::{DynBlock, RcBlock};
use dispatch2::{DispatchQueue, MainThreadBound};
use objc2::rc::{Retained, Weak};
use objc2::runtime::ProtocolObject;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send};
use objc2_app_kit::{
    NSAlert, NSAppearanceCustomization, NSBackingStoreType, NSColor, NSModalResponse,
    NSModalResponseCancel, NSSavePanel, NSWindow, NSWindowDelegate, NSWindowOrderingMode,
    NSWindowStyleMask,
};
use objc2_foundation::{NSNotification, NSObject, NSObjectProtocol, NSRect};

use crate::app;
use crate::pane::TerminalPane;
use crate::split_view::SplitView;
use crate::tab::container_of;

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
    /// The terminal window: a window's question sits on it, and its question
    /// stands in the way of every tab's.
    window: Retained<NSWindow>,
    /// The asking pane — its tab decides whether the sheet opens now on the
    /// tab's owner or parks ([`TerminalPane::tab_shown`]); `None` for a
    /// window's question, which never parks.
    pane: Option<Retained<TerminalPane>>,
}

/// A question that could not open when it was begun: what [`Seat::begin`]
/// would have opened, kept until [`open_parked`] opens it or [`Seat::end`] /
/// [`answer_parked`] answers it unopened.
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
/// that tab holds while its question cannot be shown.
fn tab_parked(pane: &TerminalPane) -> bool {
    match container_of(pane) {
        Some(container) => parked_in(&container),
        None => !pane.parked().borrow().is_empty(),
    }
}

/// Whether a pane of `container`'s tab has a parked question.
fn parked_in(container: &SplitView) -> bool {
    container
        .panes()
        .iter()
        .any(|pane| !pane.parked().borrow().is_empty())
}

impl Seat {
    /// The pane this seat's sheets park in: the asker's, while its tab is
    /// not on screen.
    fn parking(&self) -> Option<&TerminalPane> {
        self.pane.as_deref().filter(|pane| !pane.tab_shown())
    }

    /// Whether a sheet is up where this seat's sheet would show: the
    /// window's own question, and for a pane its tab's owner's sheet; for a
    /// window, the owner's of whichever tab is on screen — the only owner
    /// that is the window's child (a hidden tab's owner is ordered out).
    fn sheet_up(&self) -> bool {
        if self.window.attachedSheet().is_some() {
            return true;
        }
        match self.pane.as_deref() {
            Some(pane) => container_of(pane).is_some_and(|container| owner_busy(&container)),
            None => self.window.childWindows().is_some_and(|children| {
                children.iter().any(|child| child.attachedSheet().is_some())
            }),
        }
    }

    /// Whether a sheet is already up here — two sheets cannot open on top
    /// of each other. A background tab's seat is taken by its own parked
    /// question, not by the sheet the window shows for another tab.
    pub(crate) fn is_taken(&self) -> bool {
        let parked = self.pane.as_deref().is_some_and(tab_parked);
        // The window's sheet is read only when it can matter.
        taken(self.parking().is_none(), parked, || self.sheet_up())
    }

    /// Opens `alert` as a sheet here; `answered` gets its response. A
    /// question that cannot open now parks instead ([`Parked`]).
    pub(crate) fn begin(&self, alert: &NSAlert, answered: &DynBlock<dyn Fn(NSModalResponse)>) {
        self.begin_sheet(Sheet::Alert(alert.retain()), answered);
    }

    /// Opens an open or save panel as a sheet here; `answered` gets its
    /// response. A question that cannot open now parks instead.
    pub(crate) fn begin_panel(
        &self,
        panel: &NSSavePanel,
        answered: &DynBlock<dyn Fn(NSModalResponse)>,
    ) {
        self.begin_sheet(Sheet::Panel(panel.retain()), answered);
    }

    fn begin_sheet(&self, sheet: Sheet, answered: &DynBlock<dyn Fn(NSModalResponse)>) {
        let Some(pane) = self.pane.as_deref() else {
            // A window's question: on the window, the caller checked the seat.
            sheet.begin(&self.window, answered);
            return;
        };
        let Some(container) = container_of(pane) else {
            // Not in a tab (never seen: a pane in a window is in its tab's
            // container) — on the window, as before tabs had owners.
            sheet.begin(&self.window, answered);
            return;
        };
        if may_open(pane.tab_shown(), self.sheet_up()) {
            let owner = owner_of(&container, &self.window);
            sheet.begin(&owner, answered);
            return;
        }
        pane.parked().borrow_mut().push_back(Parked {
            sheet,
            answered: answered.copy(),
        });
        marks_changed(&self.window);
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
                marks_changed(&self.window);
                return;
            }
        }
        // Wherever it sits — a tab's owner or the window itself.
        let parent = sheet.sheetParent().unwrap_or_else(|| self.window.clone());
        parent.endSheet_returnCode(sheet, code);
    }
}

/// `pane`'s first parked question opens on its tab's owner — when its tab
/// is on screen and neither the tab nor the window shows a sheet. The tab
/// coming on screen and a sheet ending on the window or the owner call this;
/// the next parked question opens when this one ends, so several open one
/// after another. A closing pane's questions are answered instead
/// ([`answer_parked`]), never opened.
pub(crate) fn open_parked(pane: &TerminalPane) {
    if pane.is_closed() {
        return;
    }
    let (Some(seat), Some(container)) = (seat(Asker::Pane(pane)), container_of(pane)) else {
        return;
    };
    if !may_open(pane.tab_shown(), seat.sheet_up()) {
        return;
    }
    let parked = pane.parked().borrow_mut().pop_front();
    if let Some(parked) = parked {
        let owner = owner_of(&container, &seat.window);
        parked.sheet.begin(&owner, &parked.answered);
        marks_changed(&seat.window);
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

/// Whether `window` holds a question of its own — the close question or
/// the application's report, which block the whole window, its bar too.
/// The selection waits for it; a tab's question does not hold the selection
/// (it sits on the tab's owner and leaves the screen with its tab).
pub(crate) fn window_asks(window: &NSWindow) -> bool {
    window.attachedSheet().is_some()
}

/// Whether `container`'s tab has a question the user cannot see: one
/// parked in a pane's slot, or one up on its owner while the tab is off
/// screen — the tab's "waiting for an answer" mark.
pub(crate) fn question_waiting(container: &SplitView) -> bool {
    parked_in(container) || (container.isHiddenOrHasHiddenAncestor() && owner_busy(container))
}

// ─── The tab's owner ─────────────────────────────────────────────────────

/// A tab's sheet owner: the child window its panes' sheets sit on, and the
/// delegate that hears them end (a window's delegate property is weak, so
/// the owner keeps it).
pub(crate) struct Owner {
    window: Retained<NSWindow>,
    _watch: Retained<OwnerWatch>,
}

/// A tab's slot for its owner — on the tab's splits container, which the
/// asking pane reaches through its superview even while it closes. Empty
/// while the tab holds no sheet.
pub(crate) type OwnerSlot = RefCell<Option<Owner>>;

/// The owner's window, if the tab has one.
fn owner_window(container: &SplitView) -> Option<Retained<NSWindow>> {
    container
        .sheet_owner()
        .borrow()
        .as_ref()
        .map(|owner| owner.window.clone())
}

/// Whether the tab's owner holds a sheet.
fn owner_busy(container: &SplitView) -> bool {
    owner_window(container).is_some_and(|owner| owner.attachedSheet().is_some())
}

/// The container's rect on screen: the owner covers exactly the tab's
/// content, never the bar, whose clicks must still reach the window. From
/// the container rather than the window's content layout rect, which in
/// full screen reaches over the bar.
fn frame_of(container: &SplitView, window: &NSWindow) -> NSRect {
    let in_window = container.convertRect_toView(container.bounds(), None);
    window.convertRectToScreen(in_window)
}

/// The tab's owner, born on its first sheet: a borderless, clear,
/// shadowless window over the container, attached above `window`. It takes
/// the mouse — a click on the content stays on the question's side, the
/// way a window under a sheet keeps it (`ignoresMouseEvents` would let it
/// through to the pane, measured) — and never becomes key itself
/// (borderless); its sheet takes the window's appearance from it.
fn owner_of(container: &SplitView, window: &NSWindow) -> Retained<NSWindow> {
    if let Some(owner) = owner_window(container) {
        return owner;
    }
    let mtm = container.mtm();
    // SAFETY: with defer=false the window is created at once; it is not
    // released on close (below) — the slot's `Retained` is its owner.
    let owner = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm),
            frame_of(container, window),
            NSWindowStyleMask::Borderless,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    // SAFETY: only changes the ownership semantics; we are the Retained's owner.
    unsafe { owner.setReleasedWhenClosed(false) };
    owner.setRestorable(false);
    owner.setOpaque(false);
    owner.setBackgroundColor(Some(&NSColor::clearColor()));
    owner.setHasShadow(false);
    owner.setIgnoresMouseEvents(false);
    owner.setExcludedFromWindowsMenu(true);
    owner.setAppearance(window.appearance().as_deref());
    let watch = OwnerWatch::new(mtm, container);
    owner.setDelegate(Some(ProtocolObject::from_ref(&*watch)));
    // SAFETY: both windows are alive and the owner is not a child of
    // another window; above, so it covers the content.
    unsafe { window.addChildWindow_ordered(&owner, NSWindowOrderingMode::Above) };
    container.sheet_owner().replace(Some(Owner {
        window: owner.clone(),
        _watch: watch,
    }));
    owner
}

/// The tab's container moved or changed size: its owner follows. A no-op
/// without an owner or while it is ordered out — it is fitted when it
/// comes back ([`show_owner`]).
pub(crate) fn fit_owner(container: &SplitView) {
    let (Some(owner), Some(window)) = (owner_window(container), container.window()) else {
        return;
    };
    if owner.isVisible() {
        owner.setFrame_display(frame_of(container, &window), false);
    }
}

/// The tab came on screen: its owner (if a question of it is up) is
/// attached again — ordering out took it off its parent (measured) —
/// fitted, and its sheet takes the keyboard when the window has it (a sheet
/// shown again is not key by itself, measured).
pub(crate) fn show_owner(container: &SplitView) {
    let (Some(owner), Some(window)) = (owner_window(container), container.window()) else {
        return;
    };
    owner.setFrame_display(frame_of(container, &window), false);
    owner.setAppearance(window.appearance().as_deref());
    // SAFETY: both windows are alive; the owner was ordered out, which
    // detached it from its parent.
    unsafe { window.addChildWindow_ordered(&owner, NSWindowOrderingMode::Above) };
    if window.isKeyWindow()
        && let Some(sheet) = owner.attachedSheet()
    {
        sheet.makeKeyWindow();
    }
}

/// The tab left the screen: its owner is ordered out and its sheet with it,
/// still up and answerable when the tab comes back. A sheet that had the
/// keyboard leaves no key window behind (measured), so the window takes it
/// back.
pub(crate) fn hide_owner(container: &SplitView) {
    let Some(owner) = owner_window(container) else {
        return;
    };
    let had_key = owner
        .attachedSheet()
        .is_some_and(|sheet| sheet.isKeyWindow());
    let parent = owner.parentWindow();
    owner.orderOut(None);
    if had_key && let Some(parent) = parent {
        parent.makeKeyWindow();
    }
}

/// The window became key while its tab on screen shows a question: the
/// question takes the keyboard, the way a sheet on the window itself would
/// keep it; `true` if it did. A sheet on its way out is still attached but
/// no longer visible (measured) and keeps nothing.
pub(crate) fn key_to_sheet(container: &SplitView) -> bool {
    let sheet = owner_window(container)
        .filter(|owner| owner.isVisible())
        .and_then(|owner| owner.attachedSheet())
        .filter(|sheet| sheet.isVisible());
    match sheet {
        Some(sheet) => {
            sheet.makeKeyWindow();
            true
        }
        None => false,
    }
}

/// Whether `window` is `container`'s tab's owner — the key sheet's parent
/// then stands for the terminal window it covers (`AppDelegate::key_window`).
pub(crate) fn is_owner(container: &SplitView, window: &NSWindow) -> bool {
    owner_window(container).is_some_and(|owner| std::ptr::eq(&*owner, window))
}

/// The window's appearance changed (the theme): the tab's owner, and the
/// sheet on it, follow.
pub(crate) fn follow_appearance(container: &SplitView, window: &NSWindow) {
    if let Some(owner) = owner_window(container) {
        owner.setAppearance(window.appearance().as_deref());
    }
}

/// Takes the tab's owner apart — its last sheet ended, or the tab is
/// closing. A sheet still up on it ends `Cancel`, the way a closing
/// window's sheets go, each asker's block clearing its own slot and gate; a
/// child window does not go with its parent's closing by itself (measured:
/// it is only hidden), so the tab's closing calls this. The delegate goes
/// first, so the end is not heard as a reason to open the next question.
/// Idempotent.
pub(crate) fn dismantle(container: &SplitView) {
    let owner = container.sheet_owner().take();
    let Some(Owner { window: owner, .. }) = owner else {
        return;
    };
    owner.setDelegate(None);
    if let Some(sheet) = owner.attachedSheet() {
        owner.endSheet_returnCode(&sheet, NSModalResponseCancel);
    }
    if let Some(parent) = owner.parentWindow() {
        parent.removeChildWindow(&owner);
    }
    owner.orderOut(None);
}

/// A sheet on the tab's owner ended ([`OwnerWatch`], a turn later): the
/// tab's next parked question opens on it; if nothing is left on it the
/// owner is taken apart. A turn later because the end comes from inside
/// AppKit's sheet teardown and, when a pane closes, before its other parked
/// questions are answered — by then they are, and the pane is closed.
fn owner_sheet_ended(container: &SplitView) {
    for pane in container.panes() {
        open_parked(&pane);
    }
    if !owner_kept(owner_busy(container)) {
        // No sheet is left on it to end.
        dismantle(container);
    }
    if let Some(window) = container.window() {
        marks_changed(&window);
    }
}

/// A tab's "waiting for an answer" mark may have changed: the bar of
/// `window` draws its chips again.
fn marks_changed(window: &NSWindow) {
    let Some(app) = app::delegate(window.mtm()) else {
        return;
    };
    if let Some(window) = app.window_owning(window) {
        window.refresh_bar();
    }
}

pub(crate) struct WatchIvars {
    /// The tab the owner covers — weakly: the container holds the owner,
    /// and the owner holds this delegate.
    container: Weak<SplitView>,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirement; OwnerWatch implements no Drop.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriSheetOwnerWatch"]
    #[ivars = WatchIvars]
    pub(crate) struct OwnerWatch;

    unsafe impl NSObjectProtocol for OwnerWatch {}

    unsafe impl NSWindowDelegate for OwnerWatch {
        /// A sheet on the owner ended — answered, or ended by its asker: the
        /// rest is done a main-queue turn later ([`owner_sheet_ended`]).
        #[unsafe(method(windowDidEndSheet:))]
        fn window_did_end_sheet(&self, _n: &NSNotification) {
            let Some(container) = self.ivars().container.load() else {
                return;
            };
            let container = MainThreadBound::new(container, self.mtm());
            DispatchQueue::main().exec_async(move || {
                // audit: a block running on the main queue is on the main thread by definition.
                let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
                owner_sheet_ended(container.get(mtm));
            });
        }
    }
);

impl OwnerWatch {
    fn new(mtm: MainThreadMarker, container: &SplitView) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(WatchIvars {
            container: Weak::new(container),
        });
        // SAFETY: NSObject's init takes no arguments and the ivars are set.
        unsafe { msg_send![super(this), init] }
    }
}

/// Whether a seat is taken — the rule, without AppKit. `shown`: the
/// asker's tab is on screen (a window's own question always is);
/// `parked`: a question of that tab waits; `sheet_up`: a sheet is up where
/// this one would show. A background tab's seat is its own: the window's
/// sheet and the tab on screen's belong to others and do not take it.
fn taken(shown: bool, parked: bool, sheet_up: impl FnOnce() -> bool) -> bool {
    parked || (shown && sheet_up())
}

/// Whether a pane's question opens now — begun or parked: its tab on
/// screen and no sheet in the way (the window's own, or one already on the
/// tab's owner). Otherwise it waits in the pane's slot: never two sheets
/// at once, never a sheet on a hidden tab.
fn may_open(shown: bool, sheet_up: bool) -> bool {
    shown && !sheet_up
}

/// Whether a tab's owner stays once a sheet on it ended — only while
/// another sheet is up on it (a parked question that just opened). With
/// none it is taken apart: an owner lives only while its tab shows a sheet.
fn owner_kept(sheet_up: bool) -> bool {
    sheet_up
}

#[cfg(test)]
mod tests {
    use super::{may_open, owner_kept, taken};

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

    /// A question opens on screen over nothing; in the background, or with
    /// the window's question or another of its tab's up, it waits — begun
    /// or parked alike.
    #[test]
    fn a_question_opens_on_screen_over_nothing() {
        assert!(may_open(true, false));
        assert!(!may_open(true, true), "a sheet is up: it waits its turn");
        assert!(!may_open(false, false), "still in the background");
        assert!(!may_open(false, true));
    }

    /// The owner's life: born with the tab's first sheet (`owner_of`), kept
    /// while a sheet is on it, taken apart when its last one ended.
    #[test]
    fn an_owner_lives_only_while_its_tab_shows_a_sheet() {
        assert!(owner_kept(true), "the next parked question opened on it");
        assert!(!owner_kept(false), "nothing left: taken apart");
    }
}
