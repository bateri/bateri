//! Terminal tab: one splits container (`split_view::SplitView`) with its
//! panes (`pane::TerminalPane`) and what a window carries **once per tab** —
//! the focused pane and the pane side of the focus watch, the dim veil, split
//! navigation, resizing, equalizing and zoom, adding and closing panes, the
//! title's read and the remote mark, the saved form, and the panes' share of
//! the theme, the top edge and the host marks.
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
//! **A tab is not a `TabId`.** `bt_core::TabId` (`TERM_SESSION_ID`,
//! `bateri://tab/<id>`) is a **pane's** identity, and a tab holds several
//! panes: a tab and the `TabId`s inside it are 1:N. A lookup by `TabId`
//! finds a pane and the tab around it, never "the tab".
//!
//! **Upward by id, never by reference.** The panes' events reach the tab by
//! its id ([`TabHost`]) and the tab reaches its window by the window's id:
//! the window holds the tab strongly and the tab holds the panes, so a back
//! reference either way would be a cycle. The AppKit facts the tab needs —
//! the first responder, the occlusion state — come from its container's own
//! `window()`.
//!
//! **The focused pane** is the pane of the window's first responder
//! ([`TerminalTab::focused_pane`]): the title, `⇄`, upload percentage and
//! the inheritance of a new tab or split come from it. ⌘W closes it,
//! and in the last pane the tab. The other panes are under the dim veil
//! ([`TerminalTab::refresh_dim`]). Split, navigation, resizing, equalizing
//! and pane closing drop the zoom (⇧⌘↩) — resizing and equalizing because the
//! user asked for a layout change, and silently changing a hidden layout
//! would be an invisible effect.

use std::cell::Cell;

use bt_core::{ContentEdge, HostMark, MarkSubject, Settings, TabId, Theme};
use dispatch2::DispatchQueue;
use objc2::rc::Retained;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{NSView, NSWindow, NSWindowOcclusionState};
use objc2_foundation::{NSObject, NSObjectProtocol, NSPoint, NSRect};

use crate::app;
use crate::notices::Source;
use crate::pane::{PaneHost, PaneLaunch, TerminalPane};
use crate::restore::{SavedTab, Shape};
use crate::sheets;
use crate::split::{Axis, Direction, Removal, Tree};
use crate::split_view::SplitView;
use crate::upload;
use crate::uploader;
use crate::window::{Closing, TerminalWindow, initial_rect};

/// Saved scrollback, `(tab id, VT bytes)` per pane (`restore::save`'s input).
pub(crate) type Histories = Vec<(TabId, Vec<u8>)>;

/// The owner handle the tab gives its panes ([`PaneHost`]).
///
/// **It finds the tab by id**, does not hold it by reference: the tab holds
/// the pane strongly (its container), a back reference would be a cycle. The
/// id is known before the tab is born (`AppDelegate::open_window`'s counter
/// draws first), so the handle can go into the birth package — there is no
/// slot set up afterwards. If the tab's window left the list the event is
/// dropped.
pub(crate) struct TabHost {
    tab: u64,
}

impl TabHost {
    pub(crate) fn new(tab: u64) -> Self {
        Self { tab }
    }

    /// We are on the main thread: all of `PaneHost`'s calls come from the
    /// pane, on the main thread.
    fn mtm() -> MainThreadMarker {
        // audit: `PaneHost` is called only on the main thread (the trait's doc).
        MainThreadMarker::new().expect("PaneHost is called on the main thread")
    }

    fn tab(&self) -> Option<Retained<TerminalTab>> {
        app::delegate(Self::mtm())?.tab(self.tab)
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

    fn notify(&self, _pane: u64, title: &str, body: &str) {
        uploader::notify(Self::mtm(), title, body);
    }

    fn post_notices(&self, _pane: u64, source: Source, messages: Vec<String>) {
        if let Some(app) = app::delegate(Self::mtm()) {
            app.post_notices(source, messages);
        }
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
    launches: Vec<PaneLaunch>,
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
        .map(|launch| TerminalPane::new(mtm, rect, launch).map_err(|e| e.to_string()))
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
    /// the title and the tab's closing ([`TerminalTab::window`]).
    window: u64,
    /// The splits container; the window's root view holds it strongly
    /// (under the tab bar, hidden while the tab is not selected), this copy
    /// is for typed access ([`TerminalTab::panes`]).
    container: Retained<SplitView>,
    /// The id of the last focused pane — the answer of focus when the first
    /// responder is not inside a pane (the window itself)
    /// ([`TerminalTab::focused_pane`]). Written by the pane's
    /// `PaneHost::focused` event when `BateriView` becomes first responder.
    focused: Cell<u64>,
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
        let this = Self::alloc(mtm).set_ivars(TabIvars {
            id,
            window,
            container,
            focused: Cell::new(pane.id()),
        });
        // SAFETY: NSObject's init takes no arguments and the ivars are set.
        unsafe { msg_send![super(this), init] }
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
        app::delegate(self.mtm())?.window(self.ivars().window)
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
                tab.refresh_dim();
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
        self.refresh_dim();
    }

    /// The dim veil of unfocused panes: with several panes in the tab, those
    /// other than the focused one. No veil with a single pane. AppKit's
    /// work, it asks for no frame.
    pub(crate) fn refresh_dim(&self) {
        let panes = self.panes();
        let focused = self.focused_pane().id();
        let many = panes.len() > 1;
        for pane in &panes {
            pane.set_dimmed(many && pane.id() != focused);
        }
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
        self.refresh_dim();
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
        launch: PaneLaunch,
        target: u64,
        axis: Axis,
    ) -> Result<(), String> {
        let container = &self.ivars().container;
        let (_, half) = container
            .halves(target, axis)
            .ok_or_else(|| "no pane to split".to_owned())?;
        let frame = NSRect::new(NSPoint::new(0.0, 0.0), half);
        let pane = TerminalPane::new(mtm, frame, launch).map_err(|e| e.to_string())?;
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
                self.refresh_dim();
                if let Some(app) = app::delegate(self.mtm()) {
                    app.refresh_dock_tile();
                    app.layout_changed();
                }
            }
        }
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
        Some(upload::titled_as(prefix, &session.title()))
    }

    /// The focused pane's session title alone — a tab's label among
    /// several, where an upload is not a prefix (the bar shows the tab's
    /// own title; the window's title keeps the prefix). `None` while there
    /// is no session yet.
    pub(crate) fn session_title(&self) -> Option<String> {
        Some(self.focused_pane().session()?.title())
    }

    /// The tab leaves the screen (another one is selected): its panes are
    /// no longer the user's — focus off, and what floats over a pane or
    /// follows the pointer goes, since nothing would close it while hidden:
    /// the popovers, the ⌘-hovered link, the scroll bar's hover and the
    /// upload buttons' hover. Called after the container is hidden.
    pub(crate) fn leave_screen(&self) {
        for pane in self.panes() {
            pane.apply_focus(false);
            pane.close_stats_popover();
            pane.close_upload_list();
            pane.unhover_upload();
            pane.view().clear_link();
            pane.view().release_scrollbar_hover();
        }
    }

    /// The tab came on screen (selected): a question one of its panes asked
    /// while it was in the background opens now ([`sheets::open_parked`]).
    /// Called once the container is shown; the window's sheet ending calls
    /// it again for the next one.
    pub(crate) fn shown(&self) {
        for pane in self.panes() {
            sheets::open_parked(&pane);
        }
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
                histories.push((entry.tab_id.clone(), history));
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
    /// Idempotent; the result of a pane whose session never came to be is
    /// `None`.
    pub(crate) fn begin_close(&self) -> Vec<(u64, Option<Closing>)> {
        self.panes()
            .iter()
            .map(|pane| (pane.id(), pane.begin_close()))
            .collect()
    }
}
