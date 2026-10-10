//! Terminal tab: one splits container (`split_view::SplitView`) with its
//! panes (`pane::TerminalPane`) and what a window carries **once per tab** —
//! the focused pane and the pane side of the focus watch, the dim veil, split
//! navigation, resizing, equalizing and zoom, adding and closing panes, the
//! title's read and the remote mark, the saved form, the panes' share of
//! the theme, the top edge and the host marks, and where its questions sit
//! while they are up (its sheet owner, `sheets`).
//!
//! **The boundary with the window.** The window (`window::TerminalWindow`)
//! is the `NSWindow` and its delegate: chrome, the tab bar, the title it
//! writes, the order and selection of its tabs, the close question and its
//! scope, and the menu actions — the responder chain reaches the window's
//! delegate, never a tab, so the selectors stay there and call the tab. A
//! window carries one or more tabs; only the selected one's container is
//! shown, the others stay in the hierarchy hidden — so a background tab's
//! panes draw nothing and ask nothing on screen ([`TerminalTab::leave_screen`],
//! [`TerminalTab::shown`]).
//!
//! **A tab has no outward identity.** `bt_core::PaneUuid` (`TERM_SESSION_ID`,
//! `bateri://tab/<id>` — the path says `tab`, the identity is a pane's) names
//! one pane, and a tab holds several: a lookup by `PaneUuid` finds a pane and
//! the tab around it, never "the tab". The tab's own id is the in-process
//! number, which never leaves the process.
//!
//! **Upward by id, never by reference.** The panes' events reach the tab by
//! its id ([`TabHost`]) and the tab reaches its window by the window's id:
//! the window holds the tab strongly and the tab holds the panes, so a back
//! reference either way would be a cycle. The AppKit facts the tab needs —
//! the first responder, the occlusion state — come from its container's own
//! `window()`. A pane that moves to another tab is given the new tab's id
//! ([`PaneHost::rehomed`]); without it its events would go on reaching the tab
//! it left.
//!
//! **The focused pane** is the pane of the window's first responder
//! ([`TerminalTab::focused_pane`]): the title, `⇄`, upload percentage and
//! the inheritance of a new tab or split come from it. ⌘W closes it,
//! and in the last pane the tab. Its card's frame is the stronger one — the
//! other panes sit under the dim veil only with `dim_unfocused_splits`
//! ([`TerminalTab::refresh_look`]). Split, navigation, resizing, equalizing
//! and pane closing drop the zoom (⇧⌘↩) — resizing and equalizing because the
//! user asked for a layout change, and silently changing a hidden layout
//! would be an invisible effect.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use bt_core::{ContentEdge, HostMark, MarkSubject, PaneUuid, Settings, Theme};
use dispatch2::DispatchQueue;
use objc2::rc::Retained;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{NSEvent, NSView, NSWindow, NSWindowOcclusionState};
use objc2_foundation::{NSObject, NSObjectProtocol, NSPoint, NSRect};

use crate::app;
use crate::arrange::{self, Entry, Raised, Scene, Status, Wants};
use crate::embed;
use crate::launch::Closing;
use crate::notices::Source;
use crate::pane::{PaneHost, TerminalPane};
use crate::restore::{SavedTab, Shape};
use crate::sheets;
use crate::split::{Axis, Direction, Placement, Removal, Tree};
use crate::split_view::SplitView;
use crate::tab_bar::Upload;
use crate::tabs::{self, Card, CardCommand, Indicator, Signals, Tally, Unseen};
use crate::upload;
use crate::uploader;
use crate::window::{TerminalWindow, initial_rect};

/// Saved scrollback, `(tab id, VT bytes)` per pane (`restore::save`'s input).
pub(crate) type Histories = Vec<(PaneUuid, Vec<u8>)>;

/// The owner handle the tab gives its panes ([`PaneHost`]).
///
/// **It finds the tab by id**, does not hold it by reference: the tab holds
/// the pane strongly (its container), a back reference would be a cycle. The
/// id is known before the tab is born (`AppDelegate::open_window`'s counter
/// draws first), so the handle can go into the birth package — there is no
/// slot set up afterwards. If the tab's window left the list the event is
/// dropped.
pub(crate) struct TabHost {
    /// The tab the pane is in; it changes when the pane is moved to another
    /// ([`PaneHost::rehomed`]).
    tab: Cell<u64>,
}

impl TabHost {
    pub(crate) fn new(tab: u64) -> Self {
        Self {
            tab: Cell::new(tab),
        }
    }

    /// We are on the main thread: all of `PaneHost`'s calls come from the
    /// pane, on the main thread.
    fn mtm() -> MainThreadMarker {
        // audit: `PaneHost` is called only on the main thread (the trait's doc).
        MainThreadMarker::new().expect("PaneHost is called on the main thread")
    }

    fn tab(&self) -> Option<Retained<TerminalTab>> {
        app::delegate(Self::mtm())?.tab(self.tab.get())
    }
}

impl PaneHost for TabHost {
    fn title_changed(&self, _pane: u64) {
        // The title is from the focused pane; a background pane's news does
        // the same read and rewrites the unchanged title — cheap and branchless.
        if let Some(tab) = self.tab() {
            tab.refresh_title();
            // The news also says a directory moved: a layout edge.
            tab.layout_changed();
        }
    }

    fn shell_exited(&self, pane: u64) {
        // Only that pane; the last pane closes the tab.
        if let Some(tab) = self.tab() {
            tab.close_pane(pane);
        }
    }

    fn focused(&self, pane: u64) {
        if let Some(tab) = self.tab() {
            tab.pane_focused(pane);
        }
    }

    fn uploads_changed(&self, _pane: u64) {
        if let Some(app) = app::delegate(Self::mtm()) {
            app.refresh_dock_tile();
        }
    }

    fn activity_changed(&self, _pane: u64) {
        if let Some(tab) = self.tab() {
            tab.activity_changed();
        }
    }

    fn notify(&self, _pane: u64, title: &str, body: &str) {
        uploader::notify(Self::mtm(), title, body);
    }

    fn post_notices(&self, _pane: u64, source: Source, messages: Vec<String>) {
        if let Some(app) = app::delegate(Self::mtm()) {
            app.post_notices(source, messages);
        }
    }

    fn rehomed(&self, _pane: u64, tab: u64) {
        self.tab.set(tab);
    }

    fn files_dragged(&self, _pane: u64, over: bool) {
        if let Some(app) = app::delegate(Self::mtm()) {
            app.files_dragged(over);
        }
    }

    fn carry_press(&self, pane: u64, (x, y): (f64, f64)) {
        if let Some(app) = app::delegate(Self::mtm()) {
            app.pane_press(pane, NSPoint::new(x, y));
        }
    }

    fn questions_changed(&self, _pane: u64) {
        // The tab's "waiting for an answer" mark: its window's bar draws its
        // chips again.
        if let Some(window) = self.tab().and_then(|tab| tab.window()) {
            window.refresh_bar();
        }
    }

    fn cover(&self, pane: &TerminalPane) -> Option<Rc<dyn sheets::Cover>> {
        // The container the pane stands in — read from the pane, not by id: a
        // closing pane has left the lists an id lookup walks.
        container_of(pane).map(|container| Rc::new(container) as Rc<dyn sheets::Cover>)
    }
}

/// The splits container `pane` sits in — its tab's ([`TerminalPane::tab_shown`]
/// reads the same superview). `None` before the pane is placed.
pub(crate) fn container_of(pane: &TerminalPane) -> Option<Retained<SplitView>> {
    // SAFETY: reading the superview; we are on the main thread (`MainThreadOnly`).
    unsafe { pane.superview() }?.downcast::<SplitView>().ok()
}

/// The pane of `view` or one of its ancestors — from the first responder to
/// the focused pane (`BateriView`, the search field's field editor).
fn pane_containing(view: Retained<NSView>) -> Option<Retained<TerminalPane>> {
    let mut current = Some(view);
    while let Some(view) = current {
        match view.downcast::<TerminalPane>() {
            Ok(pane) => return Some(pane),
            // SAFETY: reading the parent view; we are on the main thread (`MainThreadOnly`).
            Err(view) => current = unsafe { view.superview() },
        }
    }
    None
}

/// Session restore's first tab half, before the window exists: the saved
/// `shape` matched against the panes' ids and every pane born, in the
/// launches' order. The error returns before anything is shown: a tree that
/// does not match its panes, a pane whose renderer cannot be built.
pub(crate) fn restored_panes(
    mtm: MainThreadMarker,
    shape: &Shape,
    launches: Vec<embed::Config>,
) -> Result<(Tree, Vec<Retained<TerminalPane>>), String> {
    let ids: Vec<u64> = launches.iter().map(|launch| launch.id).collect();
    let tree = shape
        .to_tree(&ids)
        .filter(|tree| {
            let mut leaves = tree.leaves();
            leaves.sort_unstable();
            let mut wanted = ids.clone();
            wanted.sort_unstable();
            leaves == wanted
        })
        .ok_or_else(|| "the saved split tree does not match its panes".to_owned())?;
    let rect = initial_rect();
    let panes = launches
        .into_iter()
        .map(|launch| embed::open(mtm, rect, launch).map_err(|e| e.to_string()))
        .collect::<Result<Vec<_>, _>>()?;
    Ok((tree, panes))
}

/// The tab's state: the splits container and the focused pane. The
/// session's core (session, link, renderer, surface, view, dock reserve,
/// point size, identity, search, upload) is in the panes
/// ([`TerminalPane`]), the split tree in the container ([`SplitView`]).
pub(crate) struct TabIvars {
    /// From the application's one counter (windows, tabs and panes share
    /// it): the key by which the panes' events find the tab ([`TabHost`]).
    id: u64,
    /// The id of the window this tab is in — the way up to the tab bar,
    /// the title and the tab's closing ([`TerminalTab::window`]). It changes
    /// when the tab moves to another window ([`TerminalTab::moved_to`]).
    window: Cell<u64>,
    /// The name the user gave the tab; `None` shows the title its focused
    /// pane's session reports ([`TerminalTab::title`]).
    name: RefCell<Option<String>>,
    /// The splits container; the window's root view holds it strongly
    /// (under the tab bar, hidden while the tab is not selected), this copy
    /// is for typed access ([`TerminalTab::panes`]).
    container: Retained<SplitView>,
    /// The id of the last focused pane — the answer of focus when the first
    /// responder is not inside a pane (the window itself)
    /// ([`TerminalTab::focused_pane`]). Written by the pane's
    /// `PaneHost::focused` event when `BateriView` becomes first responder.
    focused: Cell<u64>,
    /// Commands that ended while the tab was not selected — the chip's tick
    /// and dot until the tab is selected ([`TerminalTab::activity_changed`],
    /// [`TerminalTab::look`]).
    unseen: Cell<Unseen>,
    /// Each pane's ended-command counts as last looked at (pane id, counts):
    /// the difference to `Session::activity`'s is what ended since.
    tallies: RefCell<Vec<(u64, Tally)>>,
    /// What the arrangement moment lifted, while it is up
    /// ([`TerminalTab::arrange`]).
    raised: RefCell<Option<Raised>>,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirement; TerminalTab implements no Drop.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriTerminalTab"]
    #[ivars = TabIvars]
    pub(crate) struct TerminalTab;

    unsafe impl NSObjectProtocol for TerminalTab {}
);

impl TerminalTab {
    /// A tab around its first pane, in window `window`: the container is born
    /// with the pane filling it; splits come afterwards
    /// ([`TerminalTab::add_pane`]), a restored tree through
    /// [`TerminalTab::adopt`].
    pub(crate) fn new(
        mtm: MainThreadMarker,
        id: u64,
        window: u64,
        pane: &TerminalPane,
    ) -> Retained<Self> {
        let container = SplitView::new(mtm, initial_rect(), pane);
        // A pane that came from another tab (split off, moved here) reaches
        // this one from now on.
        pane.host().rehomed(pane.id(), id);
        let this = Self::alloc(mtm).set_ivars(TabIvars {
            id,
            window: Cell::new(window),
            name: RefCell::new(None),
            container,
            focused: Cell::new(pane.id()),
            unseen: Cell::new(Unseen::default()),
            tallies: RefCell::new(Vec::new()),
            raised: RefCell::new(None),
        });
        // SAFETY: NSObject's init takes no arguments and the ivars are set.
        unsafe { msg_send![super(this), init] }
    }

    /// A tab around `first`, a pane that came from another tab, dressed as the tabs of the
    /// window it goes to: the pane's theme, and the application's top edge when it is known
    /// (`edge`). [`Self::new`] and nothing else a window's list needs.
    pub(crate) fn around(
        mtm: MainThreadMarker,
        id: u64,
        window: u64,
        first: &TerminalPane,
        edge: Option<ContentEdge>,
    ) -> Retained<Self> {
        let tab = Self::new(mtm, id, window, first);
        if let Some(session) = first.session() {
            tab.set_theme(session.theme());
        }
        if let Some(edge) = edge {
            tab.set_content_edge(edge);
        }
        tab
    }

    pub(crate) fn id(&self) -> u64 {
        self.ivars().id
    }

    /// The splits container — the window puts it under its tab bar.
    pub(crate) fn container(&self) -> &SplitView {
        &self.ivars().container
    }

    /// The tab's window from the list; `None` once it has left it.
    fn window(&self) -> Option<Retained<TerminalWindow>> {
        app::delegate(self.mtm())?.window(self.ivars().window.get())
    }

    /// The tab now belongs to window `window`: its panes' news and its
    /// questions' marks find that window's bar from here on. Called by the
    /// window that takes the tab in ([`TerminalWindow::adopt_tab`]).
    pub(crate) fn moved_to(&self, window: u64) {
        self.ivars().window.set(window);
    }

    /// The name the user gave the tab, if any.
    pub(crate) fn name(&self) -> Option<String> {
        self.ivars().name.borrow().clone()
    }

    /// Names the tab, or takes the name away (`None`: its own title shows
    /// again). The window writes the titles after ([`TerminalWindow::rename_tab`]).
    pub(crate) fn set_name(&self, name: Option<String>) {
        self.ivars().name.replace(name);
    }

    /// The `NSWindow` the container is in — the first responder and the
    /// occlusion state are its facts.
    fn ns_window(&self) -> Option<Retained<NSWindow>> {
        self.ivars().container.window()
    }

    /// Whether the window is visible (`occlusionState`): a hidden pane's
    /// link sleeps like an occluded window's (`SplitView::apply_visibility`).
    fn window_visible(&self) -> bool {
        self.ns_window().is_some_and(|window| {
            window
                .occlusionState()
                .contains(NSWindowOcclusionState::Visible)
        })
    }

    /// The tab's panes, in tree order (left to right, top to bottom). Never
    /// empty: when the last pane closes the tab closes. A single pane in a
    /// timed run.
    pub(crate) fn panes(&self) -> Vec<Retained<TerminalPane>> {
        self.ivars().container.panes()
    }

    /// The focused pane: the pane of the window's first responder —
    /// `BateriView` or the search field's field editor, both descendants of
    /// the pane. If the first responder is not inside a pane (the window
    /// itself) the last focused pane ([`TabIvars::focused`]), and if there is
    /// none the first pane.
    pub(crate) fn focused_pane(&self) -> Retained<TerminalPane> {
        let panes = self.panes();
        let focused = self.ivars().focused.get();
        self.responder_pane()
            .or_else(|| panes.iter().find(|pane| pane.id() == focused).cloned())
            .or_else(|| panes.first().cloned())
            // audit: the container never empties (`SplitIvars::panes`): closing
            // the last pane closes the tab and the tab is born with a pane.
            .expect("the tab has at least one pane")
    }

    /// The first responder's pane — if it is one of this tab's panes
    /// (`BateriView` or the search field's field editor); otherwise `None`.
    /// The window's focus watch asks this on every first-responder change.
    pub(crate) fn responder_pane(&self) -> Option<Retained<TerminalPane>> {
        let pane = self
            .ns_window()?
            .firstResponder()
            .and_then(|responder| responder.downcast::<NSView>().ok())
            .and_then(pane_containing)?;
        self.panes()
            .into_iter()
            .find(|candidate| candidate.id() == pane.id())
    }

    /// The pane's `BateriView` became first responder (`PaneHost::focused`,
    /// and the window's focus watch): the focus moved to it, the title is
    /// its.
    ///
    /// The title is read **one main-queue turn later**: the event comes from
    /// inside `becomeFirstResponder`, the window's `firstResponder` may still
    /// be the old view at that moment and [`Self::focused_pane`] asks it
    /// before the ivar — the title would be written from the old pane. The
    /// job captures the id (the pattern of the window's `windowWillClose:`).
    pub(crate) fn pane_focused(&self, id: u64) {
        if self.ivars().focused.replace(id) == id {
            return;
        }
        self.layout_changed();
        let tab = self.id();
        DispatchQueue::main().exec_async(move || {
            // audit: a block running on the main queue is on the main thread by definition.
            let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
            if let Some(tab) = app::delegate(mtm).and_then(|app| app.tab(tab)) {
                tab.refresh_title();
                tab.refresh_look();
            }
        });
    }

    /// Gives the keyboard to `pane` (first responder) and moves the focus to
    /// it. If another pane is zoomed the zoom is dropped first: the keyboard
    /// is never given to a hidden pane (navigation, the closing's neighbour,
    /// `bateri://tab/`).
    pub(crate) fn focus_pane(&self, pane: &TerminalPane) {
        if self
            .ivars()
            .container
            .zoomed()
            .is_some_and(|zoomed| zoomed != pane.id())
        {
            self.set_zoom(None);
        }
        if let Some(window) = self.ns_window() {
            let _ = window.makeFirstResponder(Some(pane.view()));
        }
        self.pane_focused(pane.id());
        self.refresh_look();
    }

    /// What tells the focused pane from the others: its card's frame is
    /// a step stronger, and — only with `[appearance] dim_unfocused_splits` — the
    /// others sit under the dim veil. No veil with a single pane. AppKit's
    /// work, it asks for no frame.
    pub(crate) fn refresh_look(&self) {
        let panes = self.panes();
        let focused = self.focused_pane().id();
        let dim = panes.len() > 1
            && app::delegate(self.mtm()).is_some_and(|app| app.settings().dim_unfocused_splits);
        for pane in &panes {
            let own = pane.id() == focused;
            pane.set_focus_look(own, dim && !own);
        }
        self.ivars().container.ground_follows();
    }

    /// The zoomed pane; `None` → the splits are visible.
    pub(crate) fn zoomed(&self) -> Option<u64> {
        self.ivars().container.zoomed()
    }

    /// Sets or drops the zoom; the links of hidden panes sleep, those of
    /// returning ones ask for a frame (if the window is visible).
    fn set_zoom(&self, zoomed: Option<u64>) {
        let container = &self.ivars().container;
        if container.zoomed() == zoomed {
            return;
        }
        container.set_zoomed(zoomed);
        container.apply_visibility(self.window_visible());
        self.refresh_look();
    }

    /// The window's visibility reached the tab (occlusion, miniaturizing):
    /// every pane's link sleeps or wakes; a hidden pane behind the zoom is
    /// counted as occluded (`SplitView::apply_visibility`).
    pub(crate) fn apply_visibility(&self, visible: bool) {
        self.ivars().container.apply_visibility(visible);
    }

    /// The scale changed (a move between screens): first the re-layout —
    /// the split boundaries sit on the device pixel — then **every** pane's
    /// geometry, because the notification of a pane whose frame did not
    /// change does not arrive.
    pub(crate) fn refresh_geometry(&self) {
        self.ivars().container.layout_panes();
        for pane in self.panes() {
            pane.refresh_geometry();
        }
        // The panes moved under a lift: it is raised again where they stand.
        if self.ivars().raised.borrow().is_some() {
            self.arrange(false, false);
            self.arrange(true, false);
        }
    }

    /// Lifts this tab for arranging, or sets it down (`animate`: with the
    /// fade) — [`crate::arrange`]. A tab that is not on screen is not lifted.
    pub(crate) fn arrange(&self, on: bool, animate: bool) {
        let container = &self.ivars().container;
        if !on {
            let raised = self.ivars().raised.take();
            if let Some(raised) = raised {
                raised.lower(container, &self.panes(), animate);
                container.lifted(false);
            }
            return;
        }
        if self.ivars().raised.borrow().is_some()
            || self.ns_window().is_none()
            || container.isHiddenOrHasHiddenAncestor()
        {
            return;
        }
        let Some(app) = app::delegate(self.mtm()) else {
            return;
        };
        let panes = self.panes();
        let Some(theme) = self.focused_pane().session().map(|session| session.theme()) else {
            return;
        };
        let window = self.ivars().window.get();
        let windows = app.windows();
        let tabs = windows
            .iter()
            .find(|candidate| candidate.id() == window)
            .map_or(1, |candidate| candidate.tab_count());
        let wants = Wants {
            grip: tabs > 1 || panes.len() > 1 || windows.len() > 1,
            new_tab: panes.len() > 1,
        };
        let home = std::env::var("HOME").ok();
        let entries = panes
            .iter()
            .map(|pane| {
                let session = pane.session();
                let dir = session
                    .and_then(|session| session.working_directory())
                    .map(|dir| arrange::abbreviate(&dir.to_string_lossy(), home.as_deref()))
                    .unwrap_or_default();
                Entry {
                    pane: pane.id(),
                    name: session.map_or_else(String::new, |session| session.title()),
                    dir,
                    status: pane_status(pane),
                }
            })
            .collect();
        let pointer = self
            .ns_window()
            .map(|window| window.mouseLocationOutsideOfEventStream())
            .map(|at| container.convertPoint_fromView(at, None))
            .filter(|at| {
                let bounds = container.bounds();
                at.x >= 0.0 && at.y >= 0.0 && at.x < bounds.size.width && at.y < bounds.size.height
            });
        let raised = Raised::raise(
            self.mtm(),
            Scene {
                window,
                tab: self.ivars().id,
                container,
                panes: &panes,
                entries,
                theme,
                wants,
                still: app.reduce_motion(),
                pointer,
            },
        );
        *self.ivars().raised.borrow_mut() = Some(raised);
        container.lifted(true);
    }

    /// The pointer moved while this tab is lifted: the pane under it lifts a
    /// little less than the others.
    pub(crate) fn arrange_pointer(&self, event: &NSEvent) {
        let Some(window) = self.ns_window() else {
            return;
        };
        if !event.window(self.mtm()).is_some_and(|own| own == window) {
            return;
        }
        let at = self
            .ivars()
            .container
            .convertPoint_fromView(event.locationInWindow(), None);
        if let Some(raised) = self.ivars().raised.borrow().as_ref() {
            raised.point(&self.panes(), at);
        }
    }

    /// ⌘] / ⌘[: the next or previous pane in tree order.
    pub(crate) fn select_split(&self, forward: bool) {
        let from = self.focused_pane();
        if let Some(next) = self
            .ivars()
            .container
            .cycle(from.id(), forward)
            .and_then(|id| self.ivars().container.pane(id))
        {
            self.focus_pane(&next);
        }
    }

    /// ⌥⌘ + arrow: the pane in that direction; a no-op at the edge.
    /// The neighbour is from the layout without zoom — the panes' real places
    /// even while zoomed.
    pub(crate) fn select_split_toward(&self, direction: Direction) {
        let from = self.focused_pane();
        if let Some(next) = self
            .ivars()
            .container
            .neighbour(from.id(), direction)
            .and_then(|id| self.ivars().container.pane(id))
        {
            self.focus_pane(&next);
        }
    }

    /// ⌃⌘ + arrow: moves the focused pane's divider on that axis by one cell.
    /// The step is the focused pane's **one cell** — every press
    /// changes the grid by one column or row; a design decision, the number
    /// from the font. Stops at the smallest pane limit.
    pub(crate) fn resize_split(&self, direction: Direction) {
        self.set_zoom(None);
        let pane = self.focused_pane();
        let Some(cell) = pane.cell_size() else {
            return;
        };
        let step = match direction {
            Direction::Left | Direction::Right => cell.width,
            Direction::Up | Direction::Down => cell.height,
        };
        self.ivars().container.resize(pane.id(), direction, step);
        self.layout_changed();
    }

    /// ⌃⌘=: the panes on the same axis are equal.
    pub(crate) fn equalize_splits(&self) {
        self.set_zoom(None);
        self.ivars().container.equalize();
        self.layout_changed();
    }

    /// ⇧⌘↩: zooms the focused pane or undoes the zoom. A no-op
    /// with a single pane.
    pub(crate) fn toggle_split_zoom(&self) {
        if self.panes().len() < 2 {
            return;
        }
        let zoomed = match self.ivars().container.zoomed() {
            Some(_) => None,
            None => Some(self.focused_pane().id()),
        };
        self.set_zoom(zoomed);
        self.layout_changed();
    }

    /// A layout edge for the bound holder (`AppDelegate::layout_changed`).
    pub(crate) fn layout_changed(&self) {
        if let Some(app) = app::delegate(self.mtm()) {
            app.layout_changed();
        }
    }

    /// Whether the focused pane can be split on `axis`: both
    /// halves' grids must pass the smallest pane limit
    /// ([`TerminalPane::grid_fits`]). Since the new pane inherits the focused
    /// one's point-size delta the measure is from the focused one's cell.
    pub(crate) fn can_split(&self, axis: Axis) -> bool {
        let pane = self.focused_pane();
        self.ivars()
            .container
            .halves(pane.id(), axis)
            .is_some_and(|(first, second)| pane.grid_fits(first) && pane.grid_fits(second))
    }

    /// ⌘D / ⇧⌘D: a new split from the focused pane — the application builds
    /// the birth package (`AppDelegate::open_split`: directory, point-size
    /// delta, theme and remote line from the focused one). A
    /// no-op if it would drop below the limit.
    pub(crate) fn split(&self, axis: Axis) {
        // The zoom is dropped first: the split's limit is asked from
        // the layout without zoom (`can_split`) and the new pane must be visible.
        self.set_zoom(None);
        if !self.can_split(axis) {
            return;
        }
        let Some(app) = app::delegate(self.mtm()) else {
            return;
        };
        let from = self.focused_pane();
        if let Some(this) = app.tab(self.id()) {
            app.open_split(&this, &from, axis);
        }
    }

    /// Puts the new pane in `target`'s second half on `axis`, opens its
    /// session and gives it the keyboard. The order is required: the pane's
    /// `start` reads the scale from the window, so it is first attached to
    /// the container; the frame observer after layout. If the session cannot
    /// be born the pane is taken apart again — no sessionless leaf remains —
    /// and the error goes to the caller.
    pub(crate) fn add_pane(
        &self,
        mtm: MainThreadMarker,
        launch: embed::Config,
        target: u64,
        axis: Axis,
    ) -> Result<(), String> {
        let container = &self.ivars().container;
        let (_, half) = container
            .halves(target, axis)
            .ok_or_else(|| "no pane to split".to_owned())?;
        let frame = NSRect::new(NSPoint::new(0.0, 0.0), half);
        let pane = embed::open(mtm, frame, launch).map_err(|e| e.to_string())?;
        if !container.insert(target, axis, &pane) {
            return Err("no pane to split".to_owned());
        }
        pane.observe_frame();
        if let Err(e) = pane.start(mtm) {
            drop(pane.begin_close());
            let _ = container.remove_leaf(pane.id());
            drop(container.detach(pane.id()));
            return Err(format!("could not start the shell: {e}"));
        }
        self.focus_pane(&pane);
        Ok(())
    }

    /// Closes only the pane `id`, **without asking** (the shell's exit, a
    /// confirmed question). If it is the last pane, the tab's closing
    /// ([`TerminalWindow::close_tab_now`]; the window's own in its last tab).
    ///
    /// If the focused pane is closing the focus goes to the neighbour in the
    /// tree ([`Removal::Removed`]) and **before the teardown**: taking apart
    /// the view that carries the first responder would leave the window
    /// without a responder. The closing is the pane's own sequence
    /// ([`TerminalPane::begin_close`], not waited on); the Dock icon's total
    /// again, because the closed pane's queue is gone.
    pub(crate) fn close_pane(&self, id: u64) {
        let container = &self.ivars().container;
        let Some(pane) = container.pane(id) else {
            return;
        };
        let was_focused = self.focused_pane().id() == id;
        match container.remove_leaf(id) {
            Removal::Missing => {}
            Removal::Last => {
                if let Some(window) = self.window() {
                    window.close_tab_now(self.id());
                }
            }
            Removal::Removed { focus } => {
                self.set_zoom(None);
                if was_focused && let Some(next) = container.pane(focus) {
                    self.focus_pane(&next);
                }
                drop(pane.begin_close());
                drop(container.detach(id));
                self.refresh_title();
                self.refresh_look();
                if let Some(app) = app::delegate(self.mtm()) {
                    app.refresh_dock_tile();
                    app.layout_changed();
                }
            }
        }
    }

    /// The focused pane and its neighbour toward `direction`, the two a
    /// swap trades; `None` at the edge.
    pub(crate) fn swap_toward(&self, direction: Direction) -> Option<(u64, u64)> {
        let from = self.focused_pane().id();
        let to = self.ivars().container.neighbour(from, direction)?;
        Some((from, to))
    }

    /// Panes `a` and `b` trade places ([`SplitView::swap`]); the zoom is
    /// dropped first, a swap is a layout change the user should see. `false`
    /// if a pane would have to be smaller than its smallest in the other's
    /// place.
    pub(crate) fn swap(&self, a: u64, b: u64) -> bool {
        self.set_zoom(None);
        self.ivars().container.swap(a, b)
    }

    /// The panes take the places of `tree`, which holds exactly this tab's
    /// panes — one let go beside another or at the window's edge
    /// ([`SplitView::rearrange`]). The zoom is dropped first: a move is a
    /// layout change the user should see. `false` and nothing changes if the
    /// tree is not these panes or a pane would be left below its smallest.
    pub(crate) fn rearrange(&self, tree: Tree) -> bool {
        self.set_zoom(None);
        self.ivars().container.rearrange(tree)
    }

    /// Where `incoming` (the tree of `moving`) would land if it joined this
    /// tab beside its focused pane, on `side` ([`SplitView::plan_beside`]).
    /// `None` when not even the whole area has room. The panes are asked
    /// about **before** they leave their tab.
    pub(crate) fn plan_beside(
        &self,
        side: Direction,
        incoming: &Tree,
        moving: &[Retained<TerminalPane>],
    ) -> Option<Placement> {
        let leaf = self.focused_pane().id();
        self.ivars()
            .container
            .plan_beside(leaf, side, incoming, moving)
    }

    /// Pane `id` leaves the tab for another (not its last: a tab's only pane
    /// is the tab, [`Self::drain`]) — **moved, not closed**: its shell, its
    /// questions and its programs go on. Its questions are lowered while it
    /// still stands here ([`sheets::lift`]), the focus goes to its neighbour
    /// first when it had it (the window must not be left without a
    /// responder, as in [`Self::close_pane`]) and the zoom drops. `None` if
    /// it is not here or is the last.
    pub(crate) fn release_pane(&self, id: u64) -> Option<Retained<TerminalPane>> {
        let container = &self.ivars().container;
        let pane = container.pane(id)?;
        if container.panes().len() < 2 {
            return None;
        }
        let was_focused = self.focused_pane().id() == id;
        self.set_zoom(None);
        let Removal::Removed { focus } = container.remove_leaf(id) else {
            return None;
        };
        sheets::lift(&pane);
        if was_focused && let Some(next) = container.pane(focus) {
            self.focus_pane(&next);
        }
        let released = container.detach(id)?;
        self.refresh_title();
        self.refresh_look();
        Some(released)
    }

    /// Every pane leaves — the tab has left the window's order and is
    /// thrown away, its panes carried on elsewhere ([`Self::release_pane`]'s
    /// way for all of them): questions lowered, then the panes out in tree
    /// order, then the tab's question owner taken apart. The tab must not be
    /// asked for its focused pane afterwards.
    pub(crate) fn drain(&self) -> Vec<Retained<TerminalPane>> {
        let container = &self.ivars().container;
        for pane in container.panes() {
            sheets::lift(&pane);
        }
        let panes = container.drain();
        sheets::dismantle(container);
        panes
    }

    /// Panes arrive in this tab as `tree` lays them out ([`Placement::tree`]
    /// of a plan, [`Self::plan_beside`]): they join the container at their
    /// final size, are told they are this tab's ([`PaneHost::rehomed`]) and
    /// their ended-command counts are taken as seen, so what ended before
    /// the move is not news here. A tab not on screen puts them off screen
    /// like its own; one on screen shows them. The keyboard stays where it
    /// is: giving it to a pane here is the caller's ([`Self::focus_pane`] on
    /// screen, [`Self::pane_focused`] off it).
    pub(crate) fn receive(&self, panes: &[Retained<TerminalPane>], tree: Tree) -> bool {
        self.set_zoom(None);
        if !self.ivars().container.receive(tree, panes) {
            return false;
        }
        for pane in panes {
            pane.host().rehomed(pane.id(), self.id());
            self.take_as_seen(pane);
        }
        self.apply_visibility(self.window_visible());
        let key = self.ns_window().is_some_and(|window| window.isKeyWindow());
        let shown = !self.ivars().container.isHiddenOrHasHiddenAncestor();
        for pane in panes {
            if shown {
                pane.apply_focus(key);
            } else {
                pane.leave_screen();
            }
            pane.refresh_geometry();
        }
        self.refresh_look();
        true
    }

    /// `pane`'s ended-command counts so far are seen: the baseline the next
    /// read ([`Self::observe_ends`]) compares with.
    fn take_as_seen(&self, pane: &TerminalPane) {
        let Some(activity) = pane.session().map(|session| session.activity()) else {
            return;
        };
        self.ivars().tallies.borrow_mut().push((
            pane.id(),
            Tally {
                finished: activity.finished,
                failed: activity.failed,
            },
        ));
    }

    /// The window writes the titles again — this tab's in the bar and, if it
    /// is the selected one, the window's ([`TerminalWindow::refresh_title`])
    /// — the pane's title news and the focus change.
    pub(crate) fn refresh_title(&self) {
        if let Some(window) = self.window() {
            window.refresh_title();
        }
    }

    /// The title the window shows for this tab: the **focused** pane's
    /// session title; while an upload flows `↑ N% · ` in front
    /// (`upload::titled_as`; `↓` while only downloads flow; the arrow and
    /// percentage from the pane's queue). `None` while there is no session
    /// yet — the window keeps what it shows. The window's own title and a
    /// single tab's bar read this.
    pub(crate) fn title(&self) -> Option<String> {
        let pane = self.focused_pane();
        let session = pane.session()?;
        let prefix = pane.upload_title_prefix();
        let title = self.name().unwrap_or_else(|| session.title());
        Some(upload::titled_as(prefix, &title))
    }

    /// The title a tab shows among several — its name, else the focused
    /// pane's session title — where an upload is not a prefix (the bar shows
    /// the tab's own title; the window's title keeps the prefix). The card,
    /// the Show All Tabs list and VoiceOver read this too. `None` while there
    /// is no session yet.
    pub(crate) fn session_title(&self) -> Option<String> {
        let title = self.automatic_title()?;
        Some(self.name().unwrap_or(title))
    }

    /// The title the focused pane's session reports, whatever the tab is
    /// called: what a name is measured against ([`tabs::custom_name`]) and
    /// what comes back when the name goes. `None` while there is no session.
    pub(crate) fn automatic_title(&self) -> Option<String> {
        Some(self.focused_pane().session()?.title())
    }

    /// The tab leaves the screen (another one is selected): its panes are
    /// no longer the user's — focus off, and what floats over a pane or
    /// follows the pointer goes, since nothing would close it while hidden:
    /// the popovers, the ⌘-hovered link, the scroll bar's hover and the
    /// upload buttons' hover. A question up in the tab leaves with it, still
    /// open ([`sheets::hide_owner`]). Called after the container is hidden.
    pub(crate) fn leave_screen(&self) {
        for pane in self.panes() {
            pane.leave_screen();
        }
        sheets::hide_owner(self.container());
    }

    /// The tab came on screen (selected): a question left up when it went
    /// comes back ([`sheets::show_owner`]), and one its panes asked while it
    /// was in the background opens now ([`Self::open_parked`]). Called once
    /// the container is shown.
    pub(crate) fn shown(&self) {
        sheets::show_owner(self.container());
        self.open_parked();
    }

    /// A question one of the panes parked opens, if nothing is in its way
    /// ([`sheets::open_parked`]) — the tab coming on screen, and the
    /// window's own question ending.
    pub(crate) fn open_parked(&self) {
        for pane in self.panes() {
            sheets::open_parked(&pane);
        }
    }

    /// What the tab's chip shows left of its title — the most urgent of its
    /// signals (`tabs::indicator`): a question of the tab that waits for an
    /// answer the user cannot see ([`sheets::question_waiting`], or a program
    /// that reported itself blocked, [`Self::program_blocked`]), a command
    /// running in any pane ([`Self::running_for`]), a command that ended
    /// while the tab was away ([`TabIvars::unseen`]) and a transfer flowing
    /// ([`Self::upload`]).
    pub(crate) fn indicator(&self) -> Option<Indicator> {
        let Unseen { finished, failed } = self.ivars().unseen.get();
        tabs::indicator(Signals {
            question: sheets::question_waiting(self.container()) || self.program_blocked(),
            running: self.running_for().is_some(),
            failed,
            finished,
            uploading: self.upload().is_some(),
        })
    }

    /// A program in one of the tab's panes reported itself blocked on the user
    /// (`OSC 7501`) while the tab is **not** the one on screen: the question
    /// the user cannot see. On screen the program's own prompt is in front of
    /// them and the chip stays quiet. A leaf lock per pane
    /// (`Session::activity`).
    fn program_blocked(&self) -> bool {
        let Some(window) = self.window() else {
            return false;
        };
        !window.is_selected(self.id())
            && self.panes().iter().any(|pane| {
                pane.session().is_some_and(|session| {
                    session
                        .activity()
                        .program
                        .is_some_and(|program| program.blocked)
                })
            })
    }

    /// How long the tab's running command has run — the focused pane's if
    /// one runs there, else the first running pane's in tree order; `None`
    /// when no pane runs one (`Session::activity`). A pane on the alternate
    /// screen counts as running none (`tabs::ring_running`): a full-screen
    /// program gets no ring. The ring, its step and the bar's clock come
    /// from it; the summary card reads the pane itself. A leaf lock and an
    /// atomic per pane, no `Term` — the screen is the one the pane last
    /// drew (`Session::alt_screen`).
    pub(crate) fn running_for(&self) -> Option<Duration> {
        let running = |pane: &TerminalPane| {
            let session = pane.session()?;
            let activity = session.activity();
            tabs::ring_running(activity.running, activity.program, session.alt_screen())
        };
        running(&self.focused_pane()).or_else(|| self.panes().iter().find_map(|pane| running(pane)))
    }

    /// A transfer flowing in any of the tab's panes: its direction (the
    /// focused pane's, else the first flowing one's — the title prefix's
    /// arrow), the percentage the title shows and the fraction of all of
    /// them together for the chip's underline; `None` when nothing flows.
    pub(crate) fn upload(&self) -> Option<Upload> {
        let panes = self.panes();
        let focused = self.focused_pane();
        let (arrow, percent) = focused
            .upload_title_prefix()
            .or_else(|| panes.iter().find_map(|pane| pane.upload_title_prefix()))?;
        let (sent, total) = panes
            .iter()
            .filter(|pane| pane.upload_title_prefix().is_some())
            .filter_map(|pane| pane.upload_totals())
            .fold((0u64, 0u64), |(sent, total), (s, t)| {
                (sent.saturating_add(s), total.saturating_add(t))
            });
        let fraction = if total == 0 {
            f64::from(percent) / 100.0
        } else {
            sent as f64 / total as f64
        };
        Some(Upload {
            down: arrow == "↓",
            percent,
            fraction: fraction.clamp(0.0, 1.0),
        })
    }

    /// The focused pane's marked host, for the chip's top line and the
    /// card; `HostMark::None` locally and for an unmarked host
    /// (`Session::remote_mark`, the dock's mapping).
    pub(crate) fn host_mark(&self) -> HostMark {
        self.remote_mark().map_or(HostMark::None, |(_, mark)| mark)
    }

    /// A command started or ended in one of the panes
    /// ([`PaneHost::activity_changed`]): what ended while the tab was not
    /// selected marks it ([`Unseen::observe`]), and the bar draws the tab's
    /// indicator again. The selection is the window's
    /// (`TerminalWindow::is_selected`) — a selected tab in a window behind
    /// another gets no mark.
    pub(crate) fn activity_changed(&self) {
        let Some(window) = self.window() else {
            return;
        };
        self.observe_ends(window.is_selected(self.id()));
        window.refresh_bar();
    }

    /// The user saw the tab: its ends so far are seen and its tick or dot
    /// goes. The window's switch calls it for the tab coming on screen —
    /// before the bar is drawn — and for the one leaving it: an end whose
    /// news is still in the main queue behind the switch happened on screen.
    pub(crate) fn look(&self) {
        self.observe_ends(true);
        let mut unseen = self.ivars().unseen.get();
        unseen.looked();
        self.ivars().unseen.set(unseen);
    }

    /// Reads every pane's ended-command counts against the ones last seen,
    /// marks what ended unseen and keeps the new counts — a closed pane's
    /// entry goes with it.
    fn observe_ends(&self, selected: bool) {
        let mut unseen = self.ivars().unseen.get();
        let mut tallies = self.ivars().tallies.borrow_mut();
        let mut next = Vec::new();
        for pane in self.panes() {
            let Some(activity) = pane.session().map(|session| session.activity()) else {
                continue;
            };
            let now = Tally {
                finished: activity.finished,
                failed: activity.failed,
            };
            let seen = tallies
                .iter()
                .find(|(id, _)| *id == pane.id())
                .map_or(Tally::default(), |(_, tally)| *tally);
            unseen.observe(seen, now, selected);
            next.push((pane.id(), now));
        }
        *tallies = next;
        self.ivars().unseen.set(unseen);
    }

    /// What the tab's summary card tells, from the focused pane: its title,
    /// directory (`host:path` remotely), newest command, transfer, marked
    /// host and the pane count (`tabs::card_lines` lays them out). The
    /// newest command's row is read from the grid — a `Term` round, never
    /// on the frame path: when the card opens and, while it is open, when
    /// what the tab's chip shows changes (`tab_bar::TabBar::show`;
    /// `Session::last_block_info`).
    pub(crate) fn card(&self) -> Card {
        let pane = self.focused_pane();
        let session = pane.session();
        let remote = session.and_then(|session| {
            session
                .remote_mark()
                .map(|(host, _)| (host, session.remote_link_directory()))
        });
        let running = session.and_then(|session| session.activity().running);
        let block = session.and_then(|session| session.last_block_info());
        let command = match (running, block) {
            (Some(elapsed), block) => Some(CardCommand::Running {
                command: block
                    .filter(|block| block.running)
                    .map(|block| block.command)
                    .unwrap_or_default(),
                elapsed,
            }),
            (None, Some(block)) if block.running => None,
            (None, Some(block)) => match block.exit {
                Some(0) => Some(CardCommand::Finished {
                    command: block.command,
                    duration: block.duration,
                }),
                Some(exit) => Some(CardCommand::Failed {
                    command: block.command,
                    exit,
                }),
                None => None,
            },
            (None, None) => None,
        };
        let upload = pane.upload_title_prefix().map(|(arrow, percent)| {
            (
                pane.upload_flowing_name().unwrap_or_default(),
                percent,
                arrow == "↓",
            )
        });
        Card {
            title: self.session_title().unwrap_or_else(|| "bateri".to_owned()),
            directory: session.and_then(|session| session.working_directory()),
            home: crate::child::home(),
            remote,
            command,
            upload,
            mark: self.host_mark(),
            panes: self.panes().len(),
        }
    }

    /// The window became key: a question up in this tab takes the keyboard
    /// instead ([`sheets::key_to_sheet`]); `true` if it did.
    pub(crate) fn key_to_sheet(&self) -> bool {
        sheets::key_to_sheet(self.container())
    }

    /// The focused pane's remote host and resolved mark; `None` locally
    /// (`Session::remote_mark`).
    pub(crate) fn remote_mark(&self) -> Option<(String, HostMark)> {
        self.focused_pane().session()?.remote_mark()
    }

    /// The host Shell ▸ Mark … as ▸ marks in the focused pane: the remote
    /// host, else the server or Kubernetes context a guide bar names
    /// (`Session::program_mark`); with its resolved mark and how the
    /// patterns meet it. Only the marks read this — "Forget Password" and
    /// "Shell Integration on" are ssh's and stay on [`Self::remote_mark`].
    pub(crate) fn mark_target(&self) -> Option<(String, HostMark, MarkSubject)> {
        let pane = self.focused_pane();
        let session = pane.session()?;
        session
            .remote_mark()
            .map(|(host, mark)| (host, mark, MarkSubject::Host))
            .or_else(|| session.program_mark())
    }

    /// What session restore saves of this tab and, with
    /// `with_history`, its panes' scrollback (`(tab id, bytes)`). `None` if a
    /// pane has nothing live to save ([`TerminalPane::saved`]): a tab whose
    /// tree would not match its panes is not saved at all.
    pub(crate) fn saved_tab(&self, with_history: bool) -> Option<(SavedTab, Histories)> {
        let panes = self.panes();
        let mut saved = Vec::with_capacity(panes.len());
        let mut histories = Vec::new();
        for pane in &panes {
            let (entry, history) = pane.saved(with_history)?;
            if let Some(history) = history {
                histories.push((entry.uuid.clone(), history));
            }
            saved.push(entry);
        }
        let ids: Vec<u64> = panes.iter().map(|pane| pane.id()).collect();
        let shape = Shape::from_tree(&self.ivars().container.tree(), &ids)?;
        let position = |id: u64| ids.iter().position(|candidate| *candidate == id);
        let tab = SavedTab {
            shape,
            panes: saved,
            focused: position(self.focused_pane().id()).unwrap_or(0),
            zoomed: self.ivars().container.zoomed().and_then(position),
            name: self.name(),
        };
        Some((tab, histories))
    }

    /// Session restore's second tab half, once the window exists: the saved
    /// `tree` replaces the single-pane one and `extra` (every pane but the
    /// one the tab was born with) join, laid out with the saved ratios at
    /// once ([`SplitView::adopt`]); their frame observers after.
    pub(crate) fn adopt(&self, tree: Tree, extra: &[Retained<TerminalPane>]) {
        // Cannot fail: the leaves were matched against the panes
        // (`restored_panes`).
        let adopted = self.ivars().container.adopt(tree, extra);
        debug_assert!(adopted, "the checked tree must be adopted");
        for pane in extra {
            pane.observe_frame();
        }
    }

    /// Session restore's last tab half, after the window is placed — so each
    /// shell sees its final size in its first `TIOCSWINSZ` and the replayed
    /// history wraps once. `ids` are the panes' ids in the saved order.
    ///
    /// A tree that does not fit the panes' smallest size on this screen is
    /// equalized; the zoom comes before the focus, so a focus on another
    /// pane drops the zoom (the keyboard is never given to a hidden pane,
    /// [`Self::focus_pane`]). A pane whose shell cannot start leaves the tree
    /// (the precedent of [`Self::add_pane`]); if none starts the error
    /// returns at once and the caller closes the window.
    pub(crate) fn start_restored(
        &self,
        mtm: MainThreadMarker,
        ids: &[u64],
        focused: usize,
        zoomed: Option<usize>,
    ) -> Result<(), String> {
        let container = &self.ivars().container;
        if !container.fits() {
            container.equalize();
        }
        // The zoom after the plain layout: the hidden panes keep real frames.
        let zoomed = zoomed.and_then(|index| ids.get(index).copied());
        if ids.len() > 1 {
            container.set_zoomed(zoomed);
        }
        for pane in self.panes() {
            if let Err(e) = pane.start(mtm) {
                eprintln!("bateri: could not start a restored pane's shell: {e}");
                if let Removal::Last = container.remove_leaf(pane.id()) {
                    return Err(format!("could not start the shell: {e}"));
                }
                drop(pane.begin_close());
                drop(container.detach(pane.id()));
            }
        }
        if container
            .zoomed()
            .is_some_and(|zoomed| container.pane(zoomed).is_none())
        {
            container.set_zoomed(None);
        }
        let focus = ids
            .get(focused)
            .and_then(|id| container.pane(*id))
            .or_else(|| self.panes().into_iter().next());
        if let Some(focus) = focus {
            self.focus_pane(&focus);
        }
        // The links were born after the zoom hid its panes; they learn it now.
        container.apply_visibility(self.window_visible());
        Ok(())
    }

    /// Opens the first pane's session ([`TerminalPane::start`], from the
    /// birth package).
    pub(crate) fn start(&self, mtm: MainThreadMarker) -> std::io::Result<()> {
        self.focused_pane().start(mtm)
    }

    /// `[remote] hosts` changed — the pattern list goes to every pane's
    /// session ([`TerminalPane::set_host_marks`]).
    pub(crate) fn set_host_marks(&self, settings: &Settings) {
        for pane in self.panes() {
            pane.set_host_marks(settings);
        }
    }

    /// Gives the theme to the panes ([`TerminalPane::set_theme`]: session and
    /// search panel) and the separator ([`SplitView::set_theme`]); the chrome
    /// is the window's (`TerminalWindow::set_theme`, the single caller).
    pub(crate) fn set_theme(&self, theme: Theme) {
        for pane in self.panes() {
            pane.set_theme(theme);
        }
        self.ivars().container.set_theme(&theme);
    }

    /// Gives what the content does at the panes' top edge (`[appearance]
    /// content_edge`) to the panes ([`TerminalPane::set_content_edge`]: the
    /// rows and the fade) and to the container ([`SplitView::set_content_edge`]:
    /// `line`'s line) — one call for both: two callers (the window's birth,
    /// the settings' save) and a half applied mode would put a line over a
    /// fading pane.
    ///
    /// A pane born later in this tab (a split) takes the mode from its
    /// birth settings, the same source as this call's.
    pub(crate) fn set_content_edge(&self, edge: ContentEdge) {
        for pane in self.panes() {
            pane.set_content_edge(edge);
        }
        self.ivars().container.set_content_edge(edge);
    }

    /// The closing sequence's steps that fall to the tab — **starts, does
    /// not wait**: the pane's order ([`TerminalPane::begin_close`]: the
    /// upload queue, rhythm, `Waker`, `SIGHUP`) for **every** pane; one
    /// result per pane in tree order, beside the pane's id — whoever wants
    /// one pane's result finds it by id, not by a second walk's position.
    /// Then the tab's sheet owner goes, a question still up on it answered
    /// `Cancel` ([`sheets::dismantle`]): it would outlive its tab otherwise.
    /// Idempotent; the result of a pane whose session never came to be is
    /// `None`.
    pub(crate) fn begin_close(&self) -> Vec<(u64, Option<Closing>)> {
        let closing = self
            .panes()
            .iter()
            .map(|pane| (pane.id(), pane.begin_close()))
            .collect();
        sheets::dismantle(self.container());
        closing
    }
}

/// What one pane is doing, for its capsule: a question waiting, an ssh
/// session, a running command, or a shell at rest — the chip's indicator for
/// a single pane ([`TerminalTab::indicator`] looks at the tab as a whole).
fn pane_status(pane: &TerminalPane) -> Status {
    if pane.asking() {
        return Status::Question;
    }
    let Some(session) = pane.session() else {
        return Status::Shell;
    };
    // The ssh process runs for the whole session: the remote mark comes
    // first, or the ring would hide it.
    if session.remote_target().is_some() {
        return Status::Remote;
    }
    let activity = session.activity();
    if tabs::ring_running(activity.running, activity.program, session.alt_screen()).is_some() {
        Status::Running
    } else {
        Status::Shell
    }
}
