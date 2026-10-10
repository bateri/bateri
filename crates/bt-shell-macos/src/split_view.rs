//! Container for the splits: a plain `NSView`, one per tab, under the
//! window's tab bar (`window::RootView`; hidden while its tab is not the
//! selected one). It holds the tab's panes and the split tree
//! ([`crate::split`]), applies the tree's frames to the panes and places the
//! dividers' drag handles. It is not on the frame path: it draws no cells,
//! only the `line` hairline's `NSBox` fill.
//!
//! **The tree lives here, not in the window**: the container's own size
//! changes independently of the window (the title row shortens the content)
//! and the notification about it is AppKit's
//! `resizeSubviewsWithOldSize:` call on this view. Were the tree in the
//! window, the view would have to reach back to the window on every size
//! change.
//!
//! **One pane fills the container, two or more are cards** ([`crate::card`]):
//! with a single pane (or a zoomed one) the pane is the container's bounds
//! **unadjusted**; otherwise the tree is laid out with a gap between and
//! around the panes ([`spacing`], one `split::Spacing` for every question —
//! the frames, the split limit, a drag, a resize — so each approves exactly
//! what will be drawn). The gap shows the window's own background, the
//! theme's; no divider line is drawn, each card carries its own frame.
//!
//! **Going from one pane to two (and back) slides** ([`SplitView::insert`],
//! [`SplitView::detach`]): the panes are laid out once, at their final
//! frames, and each pane's layer then plays a transform from where it stood
//! to where it stands ([`crate::card::slide`]). The pane count in between
//! (3 → 2, a restore) just lays out.
//!
//! **A move slides the same way** ([`SplitView::rearrange`], a swap): one
//! layout at the final frames — a program is resized once, at the drop —
//! and each pane that changed place plays its transform ([`card::MOVE_SECS`]).
//! What a carried pane would do at a point is [`SplitView::verdict`], the
//! tree's answer ([`Tree::verdict`]) in this container's room; asking
//! changes nothing.
//!
//! When a pane's frame changes the pane refreshes its own geometry
//! (`TerminalPane::observe_frame`); only `setFrame` happens here, so while a
//! divider is being dragged the PTY resizes by the same path as window
//! resizing.
//!
//! **Drag handles**: the gap is [`crate::card::GAP_PT`] wide, so the hit
//! area is the gap itself — a transparent view ([`DividerHandle`]) that sits
//! **above** the panes (the cards' edges are not covered: a pane's
//! scroll bar lives there). A gap narrower than twice [`HANDLE_PT`] is
//! widened to that. The cursor is `resizeLeftRight`/`resizeUpDown`. Handles
//! are rebuilt only when the **number** of dividers changes (a new pane is
//! added on top of them, so at that moment they must be brought back to the
//! top); the same view stays throughout a drag, because AppKit delivers
//! `mouseDragged:` to the view that received the press.
//!
//! **`line`'s hairline** (`[appearance] content_edge = "line"`): an opaque
//! `NSBox`, one device pixel tall along the container's top edge and
//! **above** the panes, in the theme's `separator` tone. It belongs to the
//! container's edge, not a pane's, and it shows only while the panes touch
//! that edge — in a split tab each card's own frame is that line. Its frame
//! does not depend on the tree: one `setFrame` before the single-pane
//! branch covers one pane and zoom alike, and its height follows the scale
//! (the window lays out again on a scale change). A pane joining the
//! container goes in **below** it. It takes no part in hit testing
//! ([`Hairline`]): a click on that pixel row reaches the pane under it.
//!
//! **Zoom** (⇧⌘↩): the zoomed pane takes the whole area
//! ([`Tree::layout_zoomed_spaced`]), the other panes are **hidden** and their
//! frames (and so their grids) stay as they were; there are no dividers or
//! handles, and the zoomed pane is no card. A hidden pane's link sleeps like
//! that of an occluded window ([`SplitView::apply_visibility`]).

use std::cell::{Cell, RefCell};

use bt_core::{ContentEdge, SplitStyle, Theme};
use objc2::rc::Retained;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send};
use objc2_app_kit::{
    NSBox, NSBoxType, NSColor, NSCursor, NSEvent, NSTitlePosition, NSView, NSWindowOrderingMode,
};
use objc2_foundation::{NSObjectProtocol, NSPoint, NSRect, NSSize};

use crate::card::{self, Change, GAP_PT};
use crate::pane::TerminalPane;
use crate::sheets::{self, OwnerSlot};
use crate::split::{
    self, Axis, Direction, Divider, Placement, Rect, Removal, Room, Size, Spacing, Tree, Verdict,
};
use crate::window::RootView;

/// Half of the least width of a divider's hit area, in points. A design
/// constant, not a measured one: a band under six points is hard to grab with
/// a mouse. The gap between cards is exactly [`GAP_PT`] wide, so in practice
/// the handle is the gap; the number only matters if the gap is ever narrowed.
const HANDLE_PT: f64 = 3.0;

/// The id a split that has not happened yet gives the pane it would add: only
/// the arithmetic of the split limit sees it ([`SplitView::halves`]).
const HYPOTHETICAL: u64 = u64::MAX;

/// The space the tree is laid out with when it has `leaves` panes in
/// `style`: none of the card gap for one pane (the pane fills the area, with
/// the one-pixel divider the tree never draws), the card gap between and
/// around otherwise — except at the top, where the cards start right under
/// the title row. Under [`SplitStyle::Lines`] the panes always touch, one
/// pixel apart.
fn spacing(leaves: usize, scale: f64, style: SplitStyle) -> Spacing {
    if leaves > 1 && style == SplitStyle::Cards {
        Spacing::gapped(GAP_PT, GAP_PT, scale).with_top(0.0, scale)
    } else {
        Spacing::DIVIDED
    }
}

pub(crate) struct HandleIvars {
    /// Its index in [`split::Layout::dividers`] - [`Tree::drag`]'s index.
    index: Cell<usize>,
    /// The axis of the split it separates: the handle of a side-by-side split
    /// is dragged horizontally.
    axis: Cell<Axis>,
    /// The line's position along the axis, in the container's coordinates
    /// (points).
    line: Cell<f64>,
    /// The offset between the pointer and the line at press: the line must not
    /// jump under the pointer, it moves from where it was grabbed.
    grab: Cell<f64>,
}

define_class!(
    // SAFETY: NSView is designed for subclassing; DividerHandle does not
    // implement `Drop` and offers no initializer other than `initWithFrame:`.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriDividerHandle"]
    #[ivars = HandleIvars]
    pub(crate) struct DividerHandle;

    unsafe impl NSObjectProtocol for DividerHandle {}

    impl DividerHandle {
        /// `resizeLeftRightCursor`/`resizeUpDownCursor` are deprecated but
        /// their replacement `columnResizeCursorInDirections:` arrives in
        /// macOS 15; the floor is macOS 14.
        #[unsafe(method(resetCursorRects))]
        #[allow(deprecated)]
        fn reset_cursor_rects(&self) {
            let cursor = match self.ivars().axis.get() {
                Axis::Horizontal => NSCursor::resizeLeftRightCursor(),
                Axis::Vertical => NSCursor::resizeUpDownCursor(),
            };
            self.addCursorRect_cursor(self.bounds(), &cursor);
        }

        /// The press is swallowed (it must not climb the responder chain to
        /// the window) and the grabbed position is recorded.
        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            if let Some(along) = self.along(event) {
                self.ivars().grab.set(along - self.ivars().line.get());
            }
        }

        #[unsafe(method(mouseDragged:))]
        fn mouse_dragged(&self, event: &NSEvent) {
            let Some(along) = self.along(event) else {
                return;
            };
            if let Some(container) = self.container() {
                container.drag_divider(self.ivars().index.get(), along - self.ivars().grab.get());
            }
        }

        /// The drag ended: the split's ratio is part of the layout the
        /// bound holder keeps (once per drag, not per step).
        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, _event: &NSEvent) {
            if let Some(app) = crate::app::delegate(self.mtm()) {
                app.layout_changed();
            }
        }
    }
);

impl DividerHandle {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(HandleIvars {
            index: Cell::new(0),
            axis: Cell::new(Axis::Horizontal),
            line: Cell::new(0.0),
            grab: Cell::new(0.0),
        });
        // SAFETY: `initWithFrame:` is NSView's designated initializer and the
        // ivars are set.
        unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] }
    }

    fn container(&self) -> Option<Retained<SplitView>> {
        // SAFETY: reading the superview; we are on the main thread
        // (`MainThreadOnly`).
        unsafe { self.superview() }?.downcast::<SplitView>().ok()
    }

    /// The event's position in the container's (top-down) coordinates, along
    /// the handle's axis.
    fn along(&self, event: &NSEvent) -> Option<f64> {
        let container = self.container()?;
        let point = container.convertPoint_fromView(event.locationInWindow(), None);
        Some(match self.ivars().axis.get() {
            Axis::Horizontal => point.x,
            Axis::Vertical => point.y,
        })
    }

    /// Sits on the divider: its index, axis, the gap's leading edge and its
    /// frame — the gap itself, widened to twice [`HANDLE_PT`] if narrower
    /// (clipped to the container's bounds).
    fn place(&self, index: usize, divider: Divider, bounds: NSSize) {
        let iv = self.ivars();
        iv.index.set(index);
        iv.axis.set(divider.axis);
        let rect = divider.rect;
        let frame = match divider.axis {
            Axis::Horizontal => {
                iv.line.set(rect.x);
                let reach = (HANDLE_PT - rect.width / 2.0).max(0.0);
                let x = (rect.x - reach).max(0.0);
                let right = (rect.x + rect.width + reach).min(bounds.width);
                NSRect::new(NSPoint::new(x, rect.y), NSSize::new(right - x, rect.height))
            }
            Axis::Vertical => {
                iv.line.set(rect.y);
                let reach = (HANDLE_PT - rect.height / 2.0).max(0.0);
                let y = (rect.y - reach).max(0.0);
                let bottom = (rect.y + rect.height + reach).min(bounds.height);
                NSRect::new(NSPoint::new(rect.x, y), NSSize::new(rect.width, bottom - y))
            }
        };
        self.setFrame(frame);
        if let Some(window) = self.window() {
            window.invalidateCursorRectsForView(self);
        }
    }
}

define_class!(
    // SAFETY: NSBox is designed for subclassing; Hairline implements no
    // `Drop`, has no ivar and is born with NSBox's constructor (`new`).
    #[unsafe(super(NSBox))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriHairline"]
    pub(crate) struct Hairline;

    unsafe impl NSObjectProtocol for Hairline {}

    impl Hairline {
        /// Never takes part in hit testing (the pane's veil's rule): the line
        /// lies over the panes' first pixel row, and a click, a drag or a
        /// mouse report there belongs to the pane underneath.
        #[unsafe(method_id(hitTest:))]
        fn hit_test(&self, _point: NSPoint) -> Option<Retained<NSView>> {
            None
        }
    }
);

impl Hairline {
    /// Born hidden, without a border; the colour is the container's
    /// ([`SplitView::set_theme`]).
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        // SAFETY: `NSBox`'s `init`; the subclass has no ivar.
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        this.setBoxType(NSBoxType::Custom);
        this.setTitlePosition(NSTitlePosition::NoTitle);
        this.setBorderWidth(0.0);
        this.setHidden(true);
        this
    }
}

pub(crate) struct SplitIvars {
    /// The split tree; its leaves are the ids of [`SplitIvars::panes`].
    tree: RefCell<Tree>,
    /// The tab's panes. The container also holds them as subviews; this list
    /// is for typed access. **Never becomes empty**: removing the last pane
    /// means closing the window ([`Removal::Last`]).
    panes: RefCell<Vec<Retained<TerminalPane>>>,
    /// The divider's fill under [`SplitStyle::Lines`]: a box in the theme's
    /// `separator` tone behind the panes, filling the container, which shows
    /// through the one pixel left open between two panes.
    backdrop: Retained<NSBox>,
    /// The tab is lifted for arranging: the backdrop would show round the
    /// shrunk panes.
    lifted: Cell<bool>,
    /// `line`'s hairline: the fill above the panes along the top edge, shown
    /// only while the mode is `line` and the panes touch that edge (the
    /// module header).
    hairline: Retained<Hairline>,
    /// What the content does at the top edge (`[appearance] content_edge`):
    /// the hairline's half of it.
    edge: Cell<ContentEdge>,
    /// The zoomed pane (⇧⌘↩); `None` → the splits are visible.
    zoomed: Cell<Option<u64>>,
    /// The dividers' drag handles, in the order of
    /// [`split::Layout::dividers`].
    handles: RefCell<Vec<Retained<DividerHandle>>>,
    /// The tab's sheet owner while one of its questions is up
    /// ([`crate::sheets`]): here because the container is the tab's view,
    /// which an asking pane reaches through its superview even while it
    /// closes.
    sheet_owner: OwnerSlot,
}

define_class!(
    // SAFETY: NSView is designed for subclassing; SplitView does not
    // implement `Drop` and offers no initializer other than `initWithFrame:`.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriSplitView"]
    #[ivars = SplitIvars]
    pub(crate) struct SplitView;

    unsafe impl NSObjectProtocol for SplitView {}

    impl SplitView {
        /// Top-down coordinates: the tree's "second leaf is below" without
        /// flipping signs. It only affects the panes' **frames**; each pane's
        /// interior is in its own coordinates.
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        /// The container's size changed (window, tab bar): the panes are laid
        /// out again, keeping their proportions.
        #[unsafe(method(resizeSubviewsWithOldSize:))]
        fn resize_subviews(&self, _old: NSSize) {
            self.layout_panes();
        }
    }
);

impl SplitView {
    /// A single-pane container; the pane fills it.
    pub(crate) fn new(
        mtm: MainThreadMarker,
        frame: NSRect,
        first: &TerminalPane,
    ) -> Retained<Self> {
        let hairline = Hairline::new(mtm);
        let backdrop = NSBox::new(mtm);
        backdrop.setBoxType(NSBoxType::Custom);
        backdrop.setTitlePosition(NSTitlePosition::NoTitle);
        backdrop.setBorderWidth(0.0);
        backdrop.setHidden(true);
        let this = Self::alloc(mtm).set_ivars(SplitIvars {
            tree: RefCell::new(Tree::Leaf(first.id())),
            panes: RefCell::new(vec![first.retain()]),
            backdrop: backdrop.clone(),
            lifted: Cell::new(false),
            hairline: hairline.clone(),
            edge: Cell::new(ContentEdge::default()),
            zoomed: Cell::new(None),
            handles: RefCell::new(Vec::new()),
            sheet_owner: OwnerSlot::default(),
        });
        // SAFETY: `initWithFrame:` is NSView's designated initializer and the
        // ivars are set.
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: frame] };
        // The previous `contentView` (the pane) was layer-backed; the Metal
        // layer's compositing mode must not change.
        this.setWantsLayer(true);
        this.addSubview(&backdrop);
        this.addSubview(first);
        this.addSubview(&hairline);
        this.layout_panes();
        this
    }

    /// A pane joins the container **below** the hairline, so the line stays
    /// above every pane without being taken out and put back.
    fn add_pane(&self, pane: &TerminalPane) {
        self.addSubview_positioned_relativeTo(
            pane,
            NSWindowOrderingMode::Below,
            Some(&self.ivars().hairline),
        );
    }

    /// The panes, in tree order (left to right, top to bottom).
    pub(crate) fn panes(&self) -> Vec<Retained<TerminalPane>> {
        let panes = self.ivars().panes.borrow();
        self.ivars()
            .tree
            .borrow()
            .leaves()
            .into_iter()
            .filter_map(|id| panes.iter().find(|pane| pane.id() == id).cloned())
            .collect()
    }

    /// The pane whose id is `id`.
    pub(crate) fn pane(&self, id: u64) -> Option<Retained<TerminalPane>> {
        self.ivars()
            .panes
            .borrow()
            .iter()
            .find(|pane| pane.id() == id)
            .cloned()
    }

    /// The window's scale: bounds snap to the device pixel. If the container
    /// is not yet attached to a window (the constructor's first layout) it is
    /// `1` - once the window is attached AppKit reports the size again and the
    /// layout repeats, and on a scale change the window lays out again too
    /// (`TerminalWindow`'s `windowDidChangeBackingProperties:`).
    pub(crate) fn scale(&self) -> f64 {
        self.window()
            .map_or(1.0, |window| window.backingScaleFactor())
    }

    fn bounds_rect(&self) -> Rect {
        let size = self.bounds().size;
        Rect::new(0.0, 0.0, size.width, size.height)
    }

    /// `id`'s two halves were it split (in points): the frames the pane and
    /// its newcomer would have **after** the split. The tree is split
    /// in a copy and laid out by the very function that will lay out the real
    /// one ([`Tree::layout_spaced`], with the spacing the two panes will
    /// have), so the half the split limit approves is exactly the half that
    /// will be drawn — whether the pane is the only one (a card's margin is
    /// then still to come) or one of several. `None` if there is no such
    /// pane.
    pub(crate) fn halves(&self, id: u64, axis: Axis) -> Option<(NSSize, NSSize)> {
        let scale = self.scale();
        let mut tree = self.ivars().tree.borrow().clone();
        if !tree.split(id, axis, HYPOTHETICAL) {
            return None;
        }
        let spacing = spacing(tree.leaves().len(), scale, self.style());
        let layout = tree.layout_spaced(self.bounds_rect(), scale, spacing);
        let frame_of = |wanted: u64| {
            let (_, rect) = layout.panes.iter().find(|(pane, _)| *pane == wanted)?;
            Some(NSSize::new(rect.width, rect.height))
        };
        Some((frame_of(id)?, frame_of(HYPOTHETICAL)?))
    }

    /// Splits `target` along `axis` and puts `pane` in the second half (right
    /// or below). If the target is not in the tree, `false` and nothing changes.
    pub(crate) fn insert(&self, target: u64, axis: Axis, pane: &TerminalPane) -> bool {
        let before = self.snapshot();
        if !self
            .ivars()
            .tree
            .borrow_mut()
            .split(target, axis, pane.id())
        {
            return false;
        }
        self.ivars().panes.borrow_mut().push(pane.retain());
        self.add_pane(pane);
        self.layout_panes();
        // One pane became two: the first slides into its card, the newcomer
        // grows from the edge it was split off.
        if before.len() == 1 {
            self.slide(&before, Some((pane.id(), axis)), card::SLIDE_SECS);
        }
        true
    }

    /// Session restore's bulk placement: the saved `tree` replaces the
    /// single-pane one, `extra` (every pane but the one the container was
    /// born with) join as subviews and all are laid out with the saved
    /// ratios at once — no pane passes through an intermediate size. `false`
    /// and nothing changes unless the tree's leaves are exactly the born pane
    /// and `extra`.
    pub(crate) fn adopt(&self, tree: Tree, extra: &[Retained<TerminalPane>]) -> bool {
        let mut wanted: Vec<u64> = self.ivars().panes.borrow().iter().map(|p| p.id()).collect();
        wanted.extend(extra.iter().map(|pane| pane.id()));
        wanted.sort_unstable();
        let mut leaves = tree.leaves();
        leaves.sort_unstable();
        if leaves != wanted {
            return false;
        }
        self.ivars().tree.replace(tree);
        for pane in extra {
            self.ivars().panes.borrow_mut().push(pane.retain());
            self.add_pane(pane);
        }
        self.layout_panes();
        true
    }

    /// Panes join the tab as it moves them in: `tree` is the new tree
    /// (every pane of the container and every one of `panes`, nothing else)
    /// and the panes are laid out in it at once — a pane meets only its final
    /// size, so its program is resized once. A single pane becoming several
    /// slides into its card as it does when it splits ([`SplitView::insert`]).
    /// `false` and nothing changes if the leaves are not exactly those.
    pub(crate) fn receive(&self, tree: Tree, panes: &[Retained<TerminalPane>]) -> bool {
        let before = self.snapshot();
        if !self.adopt(tree, panes) {
            return false;
        }
        if before.len() == 1 {
            self.slide(&before, None, card::SLIDE_SECS);
        }
        true
    }

    /// Where `incoming` (the tree of `moving`, panes that may be in another
    /// tab) would land if let go on `side` of `leaf` — the placement the
    /// drop makes ([`Tree::plan_beside`]), in this container's room with the
    /// cards' spacing of the tree it would become. The smallest pane
    /// each of `moving` is held to is its own, measured where it is now: ask
    /// **before** the pane leaves its window.
    pub(crate) fn plan_beside(
        &self,
        leaf: u64,
        side: Direction,
        incoming: &Tree,
        moving: &[Retained<TerminalPane>],
    ) -> Option<Placement> {
        let scale = self.scale();
        let own = self.ivars().panes.borrow().clone();
        let min = move |id: u64| {
            own.iter()
                .chain(moving.iter())
                .find(|pane| pane.id() == id)
                .and_then(|pane| pane.min_size())
                .map_or(Size::new(0.0, 0.0), |min| Size::new(min.width, min.height))
        };
        let tree = self.ivars().tree.borrow();
        let room = Room {
            bounds: self.bounds_rect(),
            scale,
            spacing: spacing(
                tree.leaves().len() + incoming.leaves().len(),
                scale,
                self.style(),
            ),
            min: &min,
        };
        tree.plan_beside(leaf, side, incoming, &room)
    }

    /// Panes `a` and `b` trade places ([`Tree::swap`]): each takes the
    /// other's frame and the ratios stay, so the grids change size only if
    /// the two frames differ.
    ///
    /// `false` and nothing changes if a pane would have to be smaller than
    /// its smallest in the other's place (a pane at a larger point size has
    /// a larger smallest).
    pub(crate) fn swap(&self, a: u64, b: u64) -> bool {
        let before = self.snapshot();
        if !self.ivars().tree.borrow_mut().swap(a, b) {
            return false;
        }
        if !self.fits() {
            self.ivars().tree.borrow_mut().swap(a, b);
            return false;
        }
        self.layout_panes();
        self.slide(&before, None, card::MOVE_SECS);
        true
    }

    /// The panes take the places of `tree`, which holds exactly the panes of
    /// this container (a pane let go beside another or at the window's edge,
    /// [`Tree::verdict`]): the panes are laid out **once**, so each program
    /// is resized once, and every pane whose place changed slides there
    /// ([`card::MOVE_SECS`]). `false` and nothing changes if the leaves are
    /// not these panes, or a pane would be left below its smallest.
    pub(crate) fn rearrange(&self, tree: Tree) -> bool {
        let mut wanted = tree.leaves();
        wanted.sort_unstable();
        let mut have = self.ivars().tree.borrow().leaves();
        have.sort_unstable();
        if wanted != have {
            return false;
        }
        let before = self.snapshot();
        let old = self.ivars().tree.replace(tree);
        if !self.fits() {
            self.ivars().tree.replace(old);
            return false;
        }
        self.layout_panes();
        self.slide(&before, None, card::MOVE_SECS);
        true
    }

    /// What carrying pane `carried` with the pointer at `at` (window
    /// coordinates) shows and does: [`Tree::verdict`] in this container's
    /// room, read against the layout as it stands — nothing moves while a
    /// pane is carried. Nothing while a pane is zoomed: the splits are not
    /// on screen to be let go beside.
    pub(crate) fn verdict(&self, carried: u64, at: NSPoint) -> Verdict {
        self.verdict_of(&Tree::Leaf(carried), &[], at)
    }

    /// The same question for a block `carried` that may come from another
    /// tab: `moving` are its panes, whose own smallest sizes count where the
    /// block would stand, and the spacing is that of the tree it would make
    /// (a block landing in a lone pane makes cards).
    pub(crate) fn verdict_of(
        &self,
        carried: &Tree,
        moving: &[Retained<TerminalPane>],
        at: NSPoint,
    ) -> Verdict {
        if self.ivars().zoomed.get().is_some() {
            return Verdict::Nothing;
        }
        let at = self.convertPoint_fromView(at, None);
        let own = self.ivars().panes.borrow().clone();
        let min = |id: u64| {
            own.iter()
                .chain(moving.iter())
                .find(|pane| pane.id() == id)
                .and_then(|pane| pane.min_size())
                .map_or(Size::new(0.0, 0.0), |min| Size::new(min.width, min.height))
        };
        let tree = self.ivars().tree.borrow();
        let here = tree.leaves();
        let foreign = carried
            .leaves()
            .iter()
            .filter(|id| !here.contains(id))
            .count();
        let scale = self.scale();
        let room = Room {
            bounds: self.bounds_rect(),
            scale,
            spacing: spacing(here.len() + foreign, scale, self.style()),
            min: &min,
        };
        tree.verdict(carried, &room, (at.x, at.y))
    }

    /// The frames panes `ids` of `tree` would have in this container, in
    /// points — where a block's panes stand once it has landed
    /// (`tree` a placement's), for the preview to outline.
    pub(crate) fn frames_of(&self, tree: &Tree, ids: &[u64]) -> Vec<Rect> {
        let scale = self.scale();
        tree.layout_spaced(
            self.bounds_rect(),
            scale,
            spacing(tree.leaves().len(), scale, self.style()),
        )
        .panes
        .into_iter()
        .filter(|(id, _)| ids.contains(id))
        .map(|(_, rect)| rect)
        .collect()
    }

    /// Takes every pane out of the container, in tree order, and leaves it
    /// without any: a tab that is about to be thrown away, its panes carried
    /// on elsewhere. Nothing is laid out again; the container is on its way
    /// out of the window and must not be asked about its panes afterwards.
    pub(crate) fn drain(&self) -> Vec<Retained<TerminalPane>> {
        let panes = self.panes();
        self.ivars().panes.borrow_mut().clear();
        for pane in &panes {
            pane.removeFromSuperview();
        }
        panes
    }

    /// A copy of the split tree — what session restore saves.
    pub(crate) fn tree(&self) -> Tree {
        self.ivars().tree.borrow().clone()
    }

    /// Whether every pane's plain (unzoomed) frame passes its smallest-pane
    /// limit ([`TerminalPane::min_size`]) — a restored tree from a larger
    /// screen or a smaller font may not.
    pub(crate) fn fits(&self) -> bool {
        self.plain_layout().panes.iter().all(|(id, rect)| {
            self.pane(*id)
                .and_then(|pane| pane.min_size())
                .is_none_or(|min| rect.width >= min.width && rect.height >= min.height)
        })
    }

    /// Removes `id` from the tree ([`Tree::remove`]); the pane stays in the
    /// view and the list - the caller first moves focus, then calls
    /// [`SplitView::detach`] (a window whose view carrying the first
    /// responder is removed would be left without a responder).
    pub(crate) fn remove_leaf(&self, id: u64) -> Removal {
        self.ivars().tree.borrow_mut().remove(id)
    }

    /// Removes a pane that left the tree from the view and the list, and lays
    /// out the rest again. The pane's last strong reference drops at the caller.
    pub(crate) fn detach(&self, id: u64) -> Option<Retained<TerminalPane>> {
        let before = self.snapshot();
        let removed = {
            let mut panes = self.ivars().panes.borrow_mut();
            let index = panes.iter().position(|pane| pane.id() == id)?;
            panes.remove(index)
        };
        removed.removeFromSuperview();
        self.layout_panes();
        // Two panes became one: the survivor opens out to the whole area (the
        // closed one is already gone, closing does not wait for a slide).
        if before.len() == 2 {
            self.slide(&before, None, card::SLIDE_SECS);
        }
        Some(removed)
    }

    /// The zoomed pane; `None` → the splits are visible.
    pub(crate) fn zoomed(&self) -> Option<u64> {
        self.ivars().zoomed.get()
    }

    /// The tab's sheet owner slot — [`crate::sheets`]' alone.
    pub(crate) fn sheet_owner(&self) -> &OwnerSlot {
        &self.ivars().sheet_owner
    }

    /// Sets or releases the zoom and lays the panes out again. The links'
    /// visibility is the caller's ([`SplitView::apply_visibility`]): it knows
    /// the window's occlusion state.
    pub(crate) fn set_zoomed(&self, zoomed: Option<u64>) {
        self.ivars().zoomed.set(zoomed);
        self.layout_panes();
    }

    /// The panes' visibility: the window is visible **and** the pane is not
    /// hidden — neither itself (left behind the zoom) nor through an
    /// ancestor (this container, when its tab is not the selected one). One
    /// AppKit read answers both, so a background tab needs no signal of its
    /// own. A hidden pane draws zero frames like an occluded window; when it
    /// returns it asks for a frame (`DisplayLink::set_visible`). The same
    /// answer pauses the remote load indicator's sampling
    /// (`TerminalPane::set_visible`). Called after any `setHidden` it must
    /// see: the read is of the hierarchy as it stands.
    pub(crate) fn apply_visibility(&self, window_visible: bool) {
        let panes = self.ivars().panes.borrow().clone();
        for pane in &panes {
            let visible = window_visible && !pane.isHiddenOrHasHiddenAncestor();
            if let Some(link) = pane.link() {
                link.set_visible(visible);
            }
            pane.set_visible(visible);
        }
    }

    /// The tree's plain (unzoomed) layout: navigation asks for it.
    fn plain_layout(&self) -> split::Layout {
        let scale = self.scale();
        let tree = self.ivars().tree.borrow();
        tree.layout_spaced(
            self.bounds_rect(),
            scale,
            spacing(tree.leaves().len(), scale, self.style()),
        )
    }

    /// `from`'s neighbour in the direction (⌥⌘ + arrow; [`split::Layout::neighbour`]).
    pub(crate) fn neighbour(&self, from: u64, direction: Direction) -> Option<u64> {
        self.plain_layout().neighbour(from, direction)
    }

    /// The next or previous pane (⌘] / ⌘[; [`Tree::cycle`]).
    pub(crate) fn cycle(&self, from: u64, forward: bool) -> Option<u64> {
        self.ivars().tree.borrow().cycle(from, forward)
    }

    /// The smallest-pane limit, per leaf: from the pane's own
    /// cell (`TerminalPane::min_size`). A pane that cannot measure is unlimited.
    fn limits(&self) -> impl Fn(u64) -> Size + use<> {
        let panes = self.ivars().panes.borrow().clone();
        move |id| {
            panes
                .iter()
                .find(|pane| pane.id() == id)
                .and_then(|pane| pane.min_size())
                .map_or(Size::new(0.0, 0.0), |min| Size::new(min.width, min.height))
        }
    }

    /// The ground a resize or a drag stands on: the container's area, the
    /// spacing the tree is laid out with and the panes' smallest sizes —
    /// the layout's own, so a divider moves exactly as far as the frames go.
    fn room<'a>(&self, min: &'a dyn Fn(u64) -> Size) -> Room<'a> {
        let scale = self.scale();
        Room {
            bounds: self.bounds_rect(),
            scale,
            spacing: spacing(
                self.ivars().tree.borrow().leaves().len(),
                scale,
                self.style(),
            ),
            min,
        }
    }

    /// ⌃⌘ + arrow: moves `target`'s nearest divider on that axis by `step`
    /// points ([`Tree::resize`]), clamping at the limit. `true` if it moved.
    pub(crate) fn resize(&self, target: u64, direction: Direction, step: f64) -> bool {
        let limits = self.limits();
        let room = self.room(&limits);
        let moved = self
            .ivars()
            .tree
            .borrow_mut()
            .resize_within(target, direction, step, &room);
        if moved {
            self.layout_panes();
        }
        moved
    }

    /// ⌃⌘=: the panes on the same axis become equal ([`Tree::equalize`]).
    pub(crate) fn equalize(&self) {
        self.ivars().tree.borrow_mut().equalize();
        self.layout_panes();
    }

    /// The handle's drag: the `index`th divider to `position`
    /// ([`Tree::drag`]).
    fn drag_divider(&self, index: usize, position: f64) {
        let limits = self.limits();
        let room = self.room(&limits);
        let moved = self
            .ivars()
            .tree
            .borrow_mut()
            .drag_within(index, position, &room);
        if moved {
            self.layout_panes();
        }
    }

    /// Fits the handles to the dividers; if the count changed, rebuilds them
    /// all and adds them on top (the module header).
    fn sync_handles(&self, dividers: &[Divider]) {
        let mut handles = self.ivars().handles.borrow_mut();
        if handles.len() != dividers.len() {
            for handle in handles.drain(..) {
                handle.removeFromSuperview();
            }
            for _ in dividers {
                let handle = DividerHandle::new(self.mtm());
                self.addSubview(&handle);
                handles.push(handle);
            }
        }
        let size = self.bounds().size;
        for (index, (handle, divider)) in handles.iter().zip(dividers).enumerate() {
            handle.place(index, *divider, size);
        }
    }

    /// `line`'s hairline takes the theme's dividers' tone
    /// (`Theme::separator_srgb` - the same tier as the dock's hairlines, and
    /// the cards' frames). `NSColor` takes sRGB; the linear value is the
    /// GPU's.
    pub(crate) fn set_theme(&self, theme: &Theme) {
        let [r, g, b] = theme.separator_srgb().map(|byte| f64::from(byte) / 255.0);
        let color = NSColor::colorWithSRGBRed_green_blue_alpha(r, g, b, 1.0);
        self.ivars().hairline.setFillColor(&color);
        self.ivars().backdrop.setFillColor(&color);
    }

    /// What the content does at the panes' top edge: the hairline shows only
    /// in `line`, and only while the panes touch the edge (a card's frame is
    /// its own line). The panes' own part — the rows and the fade — is theirs
    /// (`TerminalPane::set_content_edge`); this asks for no frame, the line
    /// is AppKit's.
    pub(crate) fn set_content_edge(&self, edge: ContentEdge) {
        self.ivars().edge.set(edge);
        self.show_hairline(!self.carded());
    }

    fn show_hairline(&self, panes_touch_the_edge: bool) {
        let line = self.ivars().edge.get() == ContentEdge::Line;
        self.ivars()
            .hairline
            .setHidden(!(line && panes_touch_the_edge));
    }

    /// Whether the panes are cards now: two or more and none zoomed.
    pub(crate) fn carded(&self) -> bool {
        self.split() && self.style() == SplitStyle::Cards
    }

    /// Two or more panes in view: not one pane, not a zoomed one.
    fn split(&self) -> bool {
        let zoomed = self.ivars().zoomed.get();
        let tree = self.ivars().tree.borrow();
        let leaves = tree.leaves();
        leaves.len() > 1 && !zoomed.is_some_and(|id| leaves.contains(&id))
    }

    /// `[appearance] split_style`: cards on a ground or panes divided by a
    /// line. Read at every layout; a change lays every container out again.
    fn style(&self) -> SplitStyle {
        crate::app::delegate(self.mtm())
            .map_or(SplitStyle::default(), |app| app.settings().split_style)
    }

    /// The divider's fill shows under [`SplitStyle::Lines`] while two panes
    /// or more are in view and the tab is not lifted.
    fn sync_backdrop(&self) {
        let backdrop = &self.ivars().backdrop;
        backdrop.setFrame(self.bounds());
        let shown = self.split() && self.style() == SplitStyle::Lines && !self.ivars().lifted.get();
        backdrop.setHidden(!shown);
    }

    /// Where every pane stands and whether it is a card, **after** settling
    /// the slide that may still be running: the model values are the end of
    /// every slide, so they are where the next one starts.
    fn snapshot(&self) -> Vec<Before> {
        let panes = self.ivars().panes.borrow().clone();
        let mut out = Vec::with_capacity(panes.len());
        for pane in &panes {
            card::settle(pane, pane.frame_view());
            out.push(Before {
                id: pane.id(),
                frame: pane.frame(),
                card: pane.is_card(),
            });
        }
        out
    }

    /// Plays the slide of one pane count to the other: every pane's layer from
    /// where it stood in `before` to where it stands now. The pane in `grows`
    /// did not exist: it begins as a line along the far edge of the split it
    /// was made by. Honors Reduce Motion by not playing.
    fn slide(&self, before: &[Before], grows: Option<(u64, Axis)>, secs: f64) {
        let still = crate::app::delegate(self.mtm()).is_some_and(|app| app.reduce_motion());
        if still {
            return;
        }
        // The shade already stands at the cards' final places: it waits out
        // the slide.
        if let Some(root) = self.root()
            && !self.isHidden()
        {
            root.hold_shade(secs);
        }
        let panes = self.ivars().panes.borrow().clone();
        for pane in &panes {
            if pane.isHidden() {
                continue;
            }
            let frame = pane.frame();
            let to = Rect::new(
                frame.origin.x,
                frame.origin.y,
                frame.size.width,
                frame.size.height,
            );
            let (from, was_card) = match before.iter().find(|b| b.id == pane.id()) {
                Some(b) => (
                    Rect::new(
                        b.frame.origin.x,
                        b.frame.origin.y,
                        b.frame.size.width,
                        b.frame.size.height,
                    ),
                    b.card,
                ),
                None => match grows {
                    Some((id, axis)) if id == pane.id() => (split::sliver(to, axis), false),
                    _ => continue,
                },
            };
            // A pane that stands where it stood has nothing to play.
            if from == to && was_card == pane.is_card() {
                continue;
            }
            let change = Change {
                from,
                to,
                was_card,
                is_card: pane.is_card(),
            };
            card::slide(pane, pane.frame_view(), &change, secs);
        }
    }

    /// Applies the tree's frames to the panes. No fitting with a single pane:
    /// the pane is the container's bounds themselves, as before splitting.
    /// While zoomed only the zoomed pane is visible, the others are hidden
    /// with their frames in place. Two or more panes, none zoomed, are laid
    /// out as cards ([`spacing`]); every pane is told so, which also gives it
    /// the scale its one-pixel frame needs.
    pub(crate) fn layout_panes(&self) {
        let panes = self.ivars().panes.borrow().clone();
        let scale = self.scale();
        let (leaves, zoomed) = {
            let tree = self.ivars().tree.borrow();
            let leaves = tree.leaves();
            let zoomed = self.ivars().zoomed.get().filter(|id| leaves.contains(id));
            (leaves.len(), zoomed)
        };
        let carded = leaves > 1 && zoomed.is_none() && self.style() == SplitStyle::Cards;
        self.sync_backdrop();
        // The hairline's frame is the container's top edge, whatever the
        // tree: set **before** the single-pane branch, so one pane, splits
        // and zoom all get it. One device pixel, from the same scale the
        // frames snap to.
        let width = self.bounds().size.width;
        self.ivars().hairline.setFrame(NSRect::new(
            NSPoint::new(0.0, 0.0),
            NSSize::new(width, 1.0 / scale),
        ));
        self.show_hairline(!carded);
        if let [only] = panes.as_slice() {
            only.setHidden(false);
            only.set_card(false);
            only.setFrame(self.bounds());
            self.sync_handles(&[]);
            self.ground_follows();
            return;
        }
        let layout = self.ivars().tree.borrow().layout_zoomed_spaced(
            self.bounds_rect(),
            scale,
            zoomed,
            spacing(leaves, scale, self.style()),
        );
        for pane in &panes {
            match layout.panes.iter().find(|(id, _)| *id == pane.id()) {
                Some((_, rect)) => {
                    pane.setHidden(false);
                    pane.set_card(carded);
                    pane.setFrame(NSRect::new(
                        NSPoint::new(rect.x, rect.y),
                        NSSize::new(rect.width, rect.height),
                    ));
                }
                None => pane.setHidden(true),
            }
        }
        self.sync_handles(&layout.dividers);
        self.ground_follows();
    }

    /// The window this container stands in, if it is laid out in one.
    fn root(&self) -> Option<Retained<RootView>> {
        // SAFETY: reading the superview; we are on the main thread.
        unsafe { self.superview() }.and_then(|view| view.downcast::<RootView>().ok())
    }

    /// The window's ground and shade follow this container if it is the tab
    /// on screen: one pane ↔ cards, the cards' places and focus.
    pub(crate) fn ground_follows(&self) {
        if let Some(root) = self.root()
            && !self.isHidden()
        {
            root.sync_ground();
        }
    }

    /// The tab is lifted for arranging, or set down: the lift's own plates
    /// stand under the shrunk cards, the window's shade steps aside.
    pub(crate) fn lifted(&self, lifted: bool) {
        self.ivars().lifted.set(lifted);
        self.sync_backdrop();
        if let Some(root) = self.root() {
            root.shade_lifted(lifted);
        }
    }
}

/// A pane as it stood before a layout: the slide starts from it.
struct Before {
    id: u64,
    frame: NSRect,
    card: bool,
}

/// A tab's questions cover its splits container: a sheet over it blocks the
/// tab, not the window or the bar ([`sheets::Cover`]).
impl sheets::Cover for SplitView {
    fn view(&self) -> &NSView {
        self
    }

    fn panes(&self) -> Vec<Retained<TerminalPane>> {
        SplitView::panes(self)
    }

    fn owner_slot(&self) -> &OwnerSlot {
        self.sheet_owner()
    }

    fn weak(&self) -> sheets::WeakCover {
        let weak = objc2::rc::Weak::new(self);
        sheets::WeakCover::new(move || {
            weak.load()
                .map(|container| std::rc::Rc::new(container) as std::rc::Rc<dyn sheets::Cover>)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_pane_fills_and_several_are_cards() {
        // The one-pane layout is the tree's own, bit for bit: the pane is the
        // area. From two panes on, the gap is the card gap at the scale, and
        // none at the top: the cards start right under the title row.
        assert_eq!(spacing(1, 2.0, SplitStyle::Cards), Spacing::DIVIDED);
        let cards = spacing(2, 2.0, SplitStyle::Cards);
        assert_eq!(cards.between_px(), GAP_PT * 2.0);
        assert_eq!(cards.around_px(), GAP_PT * 2.0);
        assert_eq!(cards.top_px(), 0.0);
        assert_eq!(
            spacing(5, 1.0, SplitStyle::Cards),
            Spacing::gapped(GAP_PT, GAP_PT, 1.0).with_top(0.0, 1.0)
        );
    }

    #[test]
    fn lines_let_the_panes_touch_however_many() {
        // Under `lines` there are no cards: every pane count is laid out with
        // the one-pixel divider, the layout from before the cards.
        for leaves in [1, 2, 5] {
            assert_eq!(spacing(leaves, 2.0, SplitStyle::Lines), Spacing::DIVIDED);
        }
    }
}
