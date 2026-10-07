//! Container for the splits: a plain `NSView`, one per tab, under the
//! window's tab bar (`window::RootView`; hidden while its tab is not the
//! selected one). It holds the tab's panes and the split tree
//! ([`crate::split`]), applies the tree's frames to the panes and shows the
//! dividers. It is not on the frame path: it draws no cells, only
//! `NSBox` fills — the dividers' and `line`'s hairline.
//!
//! **The tree lives here, not in the window**: the container's own size
//! changes independently of the window (the title row shortens the content)
//! and the notification about it is AppKit's
//! `resizeSubviewsWithOldSize:` call on this view. Were the tree in the
//! window, the view would have to reach back to the window on every size
//! change.
//!
//! **The divider is a gap**: the panes are opaque and one device pixel is
//! left open between their frames; what shows through is the fill, in the
//! theme's `separator` tone, of a single `NSBox` that sits behind the panes
//! and fills the container. There is no `drawRect:` and
//! no layer path that would need a `CGColor` (same precedent as the tab
//! dot). With a single pane the box is hidden and the pane fills the
//! container **unadjusted**, exactly the layout from before splitting.
//!
//! When a pane's frame changes the pane refreshes its own geometry
//! (`TerminalPane::observe_frame`); only `setFrame` happens here, so while a
//! divider is being dragged the PTY resizes by the same path as window
//! resizing.
//!
//! **Drag handles**: the drawn line is one pixel, but the hit
//! area is a transparent view ([`DividerHandle`]) [`HANDLE_PT`] wide on
//! every side that sits **above** the panes: the panes are opaque and cover
//! every point outside the line, so the area could not live in the fill
//! behind them. The cursor is `resizeLeftRight`/`resizeUpDown`. Handles are
//! rebuilt only when the **number** of dividers changes (a new pane is added
//! on top of them, so at that moment they must be brought back to the top);
//! the same view stays throughout a drag, because AppKit delivers
//! `mouseDragged:` to the view that received the press.
//!
//! **`line`'s hairline** (`[appearance] content_edge = "line"`): a second
//! opaque `NSBox`, separate from the dividers' fill, one device pixel tall
//! along the container's top edge and **above** the panes, in the same
//! `separator` tone. It belongs to the container's edge, not a pane's, so
//! only the panes touching the window's top run under it — a pane below a
//! divider already has the divider's gap there, and no two-pixel line is
//! born. Its frame does not depend on the tree: one `setFrame` before the
//! single-pane branch covers one pane, splits and zoom alike, and its height
//! follows the scale where the dividers' does (the window lays out again on
//! a scale change). A pane joining the container goes in **below** it. It
//! takes no part in hit testing ([`Hairline`]): a click on that pixel row
//! reaches the pane under it.
//!
//! **Zoom** (⇧⌘↩): the zoomed pane takes the whole area
//! ([`Tree::layout_zoomed`]), the other panes are **hidden** and their
//! frames (and so their grids) stay as they were; there are no dividers or
//! handles. A hidden pane's link sleeps like that of an occluded window
//! ([`SplitView::apply_visibility`]).

use std::cell::{Cell, RefCell};

use bt_core::{ContentEdge, Theme};
use objc2::rc::Retained;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send};
use objc2_app_kit::{
    NSBox, NSBoxType, NSColor, NSCursor, NSEvent, NSTitlePosition, NSView, NSWindowOrderingMode,
};
use objc2_foundation::{NSObjectProtocol, NSPoint, NSRect, NSSize};

use crate::pane::TerminalPane;
use crate::split::{self, Axis, Direction, Divider, Rect, Removal, Size, Tree};

/// How far the divider's hit area extends past each side of the line, in
/// points. A design constant, not a measured one: a one-pixel line cannot be
/// grabbed with a mouse, a six-point band can, and it eats very little of the
/// text at the pane's edge (a click inside the band goes to the divider, not
/// the pane).
const HANDLE_PT: f64 = 3.0;

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

    /// Sits on the divider: its index, axis, line and its frame extending
    /// [`HANDLE_PT`] past the line (clipped to the container's bounds).
    fn place(&self, index: usize, divider: Divider, bounds: NSSize) {
        let iv = self.ivars();
        iv.index.set(index);
        iv.axis.set(divider.axis);
        let rect = divider.rect;
        let frame = match divider.axis {
            Axis::Horizontal => {
                iv.line.set(rect.x);
                let x = (rect.x - HANDLE_PT).max(0.0);
                let right = (rect.x + rect.width + HANDLE_PT).min(bounds.width);
                NSRect::new(NSPoint::new(x, rect.y), NSSize::new(right - x, rect.height))
            }
            Axis::Vertical => {
                iv.line.set(rect.y);
                let y = (rect.y - HANDLE_PT).max(0.0);
                let bottom = (rect.y + rect.height + HANDLE_PT).min(bounds.height);
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
    /// The dividers' colour: the fill behind the panes.
    backdrop: Retained<NSBox>,
    /// `line`'s hairline: the fill above the panes along the top edge, shown
    /// only while the mode is `line` (the module header).
    hairline: Retained<Hairline>,
    /// The zoomed pane (⇧⌘↩); `None` → the splits are visible.
    zoomed: Cell<Option<u64>>,
    /// The dividers' drag handles, in the order of
    /// [`split::Layout::dividers`].
    handles: RefCell<Vec<Retained<DividerHandle>>>,
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
        let backdrop = NSBox::new(mtm);
        backdrop.setBoxType(NSBoxType::Custom);
        backdrop.setTitlePosition(NSTitlePosition::NoTitle);
        backdrop.setBorderWidth(0.0);
        backdrop.setHidden(true);
        let hairline = Hairline::new(mtm);
        let this = Self::alloc(mtm).set_ivars(SplitIvars {
            tree: RefCell::new(Tree::Leaf(first.id())),
            panes: RefCell::new(vec![first.retain()]),
            backdrop: backdrop.clone(),
            hairline: hairline.clone(),
            zoomed: Cell::new(None),
            handles: RefCell::new(Vec::new()),
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
    fn scale(&self) -> f64 {
        self.window()
            .map_or(1.0, |window| window.backingScaleFactor())
    }

    fn bounds_rect(&self) -> Rect {
        let size = self.bounds().size;
        Rect::new(0.0, 0.0, size.width, size.height)
    }

    /// `id`'s frame, with its two halves were it split (in points, the same
    /// arithmetic as the frame computation - [`split::split_halves`]).
    /// `None` if there is no such pane.
    pub(crate) fn halves(&self, id: u64, axis: Axis) -> Option<(NSSize, NSSize)> {
        let scale = self.scale();
        let layout = self.ivars().tree.borrow().layout(self.bounds_rect(), scale);
        let frame = layout.panes.iter().find(|(pane, _)| *pane == id)?.1;
        let (first, second) = split::split_halves(frame, axis, scale);
        Some((
            NSSize::new(first.width, first.height),
            NSSize::new(second.width, second.height),
        ))
    }

    /// Splits `target` along `axis` and puts `pane` in the second half (right
    /// or below). If the target is not in the tree, `false` and nothing changes.
    pub(crate) fn insert(&self, target: u64, axis: Axis, pane: &TerminalPane) -> bool {
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
        let removed = {
            let mut panes = self.ivars().panes.borrow_mut();
            let index = panes.iter().position(|pane| pane.id() == id)?;
            panes.remove(index)
        };
        removed.removeFromSuperview();
        self.layout_panes();
        Some(removed)
    }

    /// The zoomed pane; `None` → the splits are visible.
    pub(crate) fn zoomed(&self) -> Option<u64> {
        self.ivars().zoomed.get()
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
        self.ivars()
            .tree
            .borrow()
            .layout(self.bounds_rect(), self.scale())
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

    /// ⌃⌘ + arrow: moves `target`'s nearest divider on that axis by `step`
    /// points ([`Tree::resize`]), clamping at the limit. `true` if it moved.
    pub(crate) fn resize(&self, target: u64, direction: Direction, step: f64) -> bool {
        let limits = self.limits();
        let moved = self.ivars().tree.borrow_mut().resize(
            target,
            direction,
            step,
            self.bounds_rect(),
            self.scale(),
            &limits,
        );
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
        let moved = self.ivars().tree.borrow_mut().drag(
            index,
            position,
            self.bounds_rect(),
            self.scale(),
            &limits,
        );
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

    /// The divider's colour comes from the theme:
    /// `Theme::separator_srgb` - the same tier as the dock's hairlines.
    /// `NSColor` takes sRGB; the linear value is the GPU's.
    ///
    /// `line`'s hairline takes the same colour: it is the dividers' line
    /// drawn along the top edge.
    pub(crate) fn set_theme(&self, theme: &Theme) {
        let [r, g, b] = theme.separator_srgb().map(|byte| f64::from(byte) / 255.0);
        let color = NSColor::colorWithSRGBRed_green_blue_alpha(r, g, b, 1.0);
        self.ivars().backdrop.setFillColor(&color);
        self.ivars().hairline.setFillColor(&color);
    }

    /// What the content does at the panes' top edge: the hairline shows only
    /// in `line`. The panes' own part — the rows and the fade — is theirs
    /// (`TerminalPane::set_content_edge`); this asks for no frame, the line
    /// is AppKit's.
    pub(crate) fn set_content_edge(&self, edge: ContentEdge) {
        self.ivars().hairline.setHidden(edge != ContentEdge::Line);
    }

    /// Applies the tree's frames to the panes. No fitting with a single pane:
    /// the pane is the container's bounds themselves, as before splitting.
    /// While zoomed only the zoomed pane is visible, the others are hidden
    /// with their frames in place.
    pub(crate) fn layout_panes(&self) {
        let panes = self.ivars().panes.borrow().clone();
        let zoomed = self.ivars().zoomed.get();
        // We answer `resizeSubviewsWithOldSize:` ourselves, so AppKit's
        // autoresizing is not applied to this view's children: the fill too by hand.
        let backdrop = &self.ivars().backdrop;
        backdrop.setFrame(self.bounds());
        backdrop.setHidden(panes.len() <= 1 || zoomed.is_some());
        // The hairline's frame is the container's top edge, whatever the
        // tree: set **before** the single-pane branch, so one pane, splits
        // and zoom all get it. One device pixel, from the same scale the
        // dividers snap to.
        let width = self.bounds().size.width;
        self.ivars().hairline.setFrame(NSRect::new(
            NSPoint::new(0.0, 0.0),
            NSSize::new(width, 1.0 / self.scale()),
        ));
        if let [only] = panes.as_slice() {
            only.setHidden(false);
            only.setFrame(self.bounds());
            self.sync_handles(&[]);
            return;
        }
        let layout =
            self.ivars()
                .tree
                .borrow()
                .layout_zoomed(self.bounds_rect(), self.scale(), zoomed);
        for pane in &panes {
            match layout.panes.iter().find(|(id, _)| *id == pane.id()) {
                Some((_, rect)) => {
                    pane.setHidden(false);
                    pane.setFrame(NSRect::new(
                        NSPoint::new(rect.x, rect.y),
                        NSSize::new(rect.width, rect.height),
                    ));
                }
                None => pane.setHidden(true),
            }
        }
        self.sync_handles(&layout.dividers);
    }
}
