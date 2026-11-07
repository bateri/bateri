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
//! **Under cards every pane is a card** ([`crate::card`]), a tab's only one
//! too: the tree is laid out with a gap between and around the panes
//! ([`spacing`], one `split::Spacing` for every question — the frames, the
//! split limit, a drag, a resize — so each approves exactly what will be
//! drawn). The gap shows the window's ground; no divider line is drawn, each
//! card carries its own frame. Under lines a single pane (or a zoomed one) is
//! the container's bounds **unadjusted**, several touch one pixel apart.
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
//! that edge (in a split tab each card's own frame is that line) and only with the tabs in
//! the title row: the side column has no bar for it to stand under. Its frame
//! does not depend on the tree: one `setFrame` before the single-pane
//! branch covers one pane and zoom alike, and its height follows the scale
//! (the window lays out again on a scale change). A pane joining the
//! container goes in **below** it. It takes no part in hit testing
//! ([`Hairline`]): a click on that pixel row reaches the pane under it.
//!
//! **Zoom** (⇧⌘↩): the zoomed pane takes the whole area
//! ([`Tree::layout_zoomed_spaced`]), the other panes are **hidden** and their
//! frames (and so their grids) stay as they were; there are no dividers or
//! handles, and the zoomed pane stands like a tab's only one — a card under
//! cards, the whole area under lines. A hidden pane's link sleeps like
//! that of an occluded window ([`SplitView::apply_visibility`]).

use std::cell::{Cell, RefCell};
use std::ptr::NonNull;

use block2::RcBlock;

use bt_core::{ContentEdge, MinimizedPanes, SplitStyle, Theme};
use bt_shell_common::floating::{
    self, Area, CapsulePlace, Card, Gone, Grip, Ground, Layers, Margins, Pull, capsule,
};
use objc2::rc::Retained;
use objc2::{
    AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send,
};
use objc2_app_kit::{
    NSBezierPath, NSBox, NSBoxType, NSColor, NSCursor, NSEvent, NSImage, NSTitlePosition, NSView,
    NSWindowOcclusionState, NSWindowOrderingMode,
};
use objc2_foundation::{NSObjectProtocol, NSPoint, NSRect, NSSize, NSTimer};
use objc2_quartz_core::CACurrentMediaTime;

use crate::card::{self, Change, GAP_PT};
use crate::pane::TerminalPane;
use crate::sheets::{self, OwnerSlot};
use crate::split::{
    self, Axis, Direction, Divider, Placement, Rect, Removal, Room, Size, Spacing, Tree, Verdict,
};
use crate::tab_bar::{Capsule, HeadLook, PaneHead};
use crate::window::RootView;

/// Half of the least width of a divider's hit area, in points. A design
/// constant, not a measured one: a band under six points is hard to grab with
/// a mouse. The gap between cards is exactly [`GAP_PT`] wide, so in practice
/// the handle is the gap; the number only matters if the gap is ever narrowed.
const HANDLE_PT: f64 = 3.0;

/// How far a floating card's grip reaches either side of its border, points: the handle
/// straddles the border, and a corner is where two handles meet.
const GRIP_PT: f64 = 5.0;

/// The id a split that has not happened yet gives the pane it would add: only
/// the arithmetic of the split limit sees it ([`SplitView::halves`]).
const HYPOTHETICAL: u64 = u64::MAX;

/// The space the tree is laid out with in `style`, however many panes it
/// has: under [`SplitStyle::Cards`] the card gap between and around — a tab's
/// only pane is a card too — except at the edge the tabs stand along: the
/// cards start right under the title row when the tabs are in it, and right at
/// the side column when they are in that (`beside_tabs`), whose own inner
/// margin is the cards' gap on that side — a second one would stand the cards
/// twice as far from the rows as from the window's other edges. Under
/// [`SplitStyle::Lines`] the panes touch, one pixel apart, and one pane fills
/// the area.
fn spacing(scale: f64, style: SplitStyle, beside_tabs: bool) -> Spacing {
    spaced(scale, style, beside_tabs).with_head(card::HEAD_PT, scale)
}

/// The panes that float in `cards`, in id order: two sets differ only when a pane floated or
/// docked (or a card was split or lost a pane), not when a card came to the front.
fn floating_panes(cards: &[Card]) -> Vec<u64> {
    let mut out: Vec<u64> = cards.iter().flat_map(|card| card.tree.leaves()).collect();
    out.sort_unstable();
    out
}

/// [`spacing`] without the panes' headers.
fn spaced(scale: f64, style: SplitStyle, beside_tabs: bool) -> Spacing {
    if style == SplitStyle::Cards {
        let gapped = Spacing::gapped(GAP_PT, GAP_PT, scale);
        if beside_tabs {
            gapped.with_left(0.0, scale)
        } else {
            gapped.with_top(0.0, scale)
        }
    } else {
        Spacing::DIVIDED
    }
}

/// The space inside one floating card: its panes touch, one line between them as lines' do,
/// and no margin round them (the card's edge is its own frame), each pane's header above it.
/// A floating pane is a card under either split style, rounded where the card is and square
/// where its panes meet ([`card_corners`]).
fn card_spacing(scale: f64) -> Spacing {
    Spacing::DIVIDED.with_head(card::HEAD_PT, scale)
}

/// Which corners of a pane of a floating card are the card's own, and so round: the header's
/// top left and right where it stands at the card's top, the pane's bottom left and right where
/// it stands at the card's bottom — each on the side the pane meets the card's edge. `frame` is
/// the pane's, under its header; `card` the card's outline, headers included.
fn card_corners(frame: Rect, card: Rect) -> CardCorners {
    let on = |edge: f64, of: f64| (edge - of).abs() <= EDGE_EPS;
    let left = on(frame.x, card.x);
    let right = on(frame.x + frame.width, card.x + card.width);
    let top = on(frame.y - card::HEAD_PT, card.y);
    let bottom = on(frame.y + frame.height, card.y + card.height);
    CardCorners {
        top: (top && left, top && right),
        bottom: (bottom && left, bottom && right),
    }
}

/// [`card_corners`]: left and right, at the header's top and at the pane's bottom.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CardCorners {
    top: (bool, bool),
    bottom: (bool, bool),
}

/// The parts of a divider's handle at `rect` (the container's space) that none of `covers`
/// stands over, along its length: a handle is a thin strip, so a card over it cuts it in two at
/// most. `axis` is the split's: a side-by-side split's handle runs top to bottom.
fn uncovered(rect: Rect, axis: Axis, covers: &[Rect]) -> Vec<Rect> {
    let (start, end) = match axis {
        Axis::Horizontal => (rect.y, rect.y + rect.height),
        Axis::Vertical => (rect.x, rect.x + rect.width),
    };
    let mut open = vec![(start, end)];
    for cover in covers {
        let across = match axis {
            Axis::Horizontal => cover.x < rect.x + rect.width && rect.x < cover.x + cover.width,
            Axis::Vertical => cover.y < rect.y + rect.height && rect.y < cover.y + cover.height,
        };
        if !across {
            continue;
        }
        let (from, to) = match axis {
            Axis::Horizontal => (cover.y, cover.y + cover.height),
            Axis::Vertical => (cover.x, cover.x + cover.width),
        };
        open = open
            .into_iter()
            .flat_map(|(a, b)| {
                let before = (a, b.min(from));
                let after = (a.max(to), b);
                [before, after]
                    .into_iter()
                    .filter(|(a, b)| b - a > 0.0)
                    .collect::<Vec<_>>()
            })
            .collect();
    }
    open.into_iter()
        .map(|(a, b)| match axis {
            Axis::Horizontal => Rect::new(rect.x, a, rect.width, b - a),
            Axis::Vertical => Rect::new(a, rect.y, b - a, rect.height),
        })
        .collect()
}

/// A card growing out of its capsule ([`card::fly`]): its panes, its frame and its capsule's (in
/// the container's space) and when it began (`CACurrentMediaTime`).
struct Growth {
    leaves: Vec<u64>,
    card: Rect,
    capsule: Rect,
    begin: f64,
}

/// How far a floating card keeps from the window's edges under lines, where the flat panes keep
/// none, once it is let go: the cards' gap, so it reads as standing over the panes.
const FLOAT_MARGIN_PT: f64 = GAP_PT;

/// How near the window's edge a carried card may be pushed into its margin: never nearer
/// than this, so it never lies on the edge.
const EDGE_GAP_PT: f64 = 3.0;

/// How far from a corner of the margins a carried card is pulled toward it, as by a magnet
/// ([`Area::carried`]); it holds fast in the last quarter of that.
const MAGNET_PT: f64 = 24.0;

/// How long a card let go past its margin takes to come back inside it: the motions' base.
const SETTLE_SECS: f64 = 0.24;

/// How far a pane's edge may stand from the container's and still be on it,
/// in points: the frames' sub-pixel rounding, not a gap. The smallest pane is
/// far larger than this, so no pane stands this close to an edge it is not on.
const EDGE_EPS: f64 = 0.5;

/// The radii, in points, of a lined pane's top left and right corners where its frame (header
/// included) stands in the container (`bounds`): the window's corner where the pane meets the
/// window's top edge and its right edge — only with the tabs in the side column, the
/// container's top edge being the window's then —, square everywhere else. The bottom's twin
/// ([`bottom_corners`]), for the focus frame. Pure.
fn top_corners(rect: Rect, bounds: Rect, beside_tabs: bool, window: f64) -> (f64, f64) {
    // With the tabs in the title row the container starts under it: its top edge is not the
    // window's. In the side column it is, but the left corner is the column's.
    if !beside_tabs {
        return (0.0, 0.0);
    }
    let on = |edge: f64, of: f64| (edge - of).abs() <= EDGE_EPS;
    let top = on(rect.y, bounds.y);
    let right = top && on(rect.x + rect.width, bounds.x + bounds.width);
    (0.0, if right { window } else { 0.0 })
}

/// The radii, in points, of a pane's bottom left and right corners, given
/// where it stands in the container (`bounds`, which is the window's content
/// to its edge): a card has its own round corners on both sides; a pane that
/// is not a card has the **window's** corner exactly where it touches the
/// window's bottom edge and that side edge, and a square one everywhere else —
/// the lone pane, the zoomed pane and the bottom panes of a lined split. With
/// the tabs in the side column (`beside_tabs`) the container's left edge is the
/// column's, not the window's: the left corner is the column's foot, never a
/// pane's. The text beside a corner starts where the curve lets it
/// (`CellMetrics::with_corners`), and a corner the pane does not own is not
/// the pane's to keep clear of. Pure.
fn bottom_corners(
    rect: Rect,
    bounds: Rect,
    carded: bool,
    beside_tabs: bool,
    window: f64,
    card: f64,
) -> (f64, f64) {
    if carded {
        return (card, card);
    }
    let on = |edge: f64, of: f64| (edge - of).abs() <= EDGE_EPS;
    let bottom = on(rect.y + rect.height, bounds.y + bounds.height);
    let left = bottom && !beside_tabs && on(rect.x, bounds.x);
    let right = bottom && on(rect.x + rect.width, bounds.x + bounds.width);
    (
        if left { window } else { 0.0 },
        if right { window } else { 0.0 },
    )
}

pub(crate) struct HandleIvars {
    /// The floating card whose tree it divides (its place in the cards, back to front); `None`
    /// for the flat area.
    card: Cell<Option<usize>>,
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
        ///
        /// Only where no card in front of the line covers it: AppKit gives a cursor rect its
        /// cursor whatever stands over the view, so a line under a floating card would turn the
        /// pointer on the card into a resize arrow.
        #[unsafe(method(resetCursorRects))]
        #[allow(deprecated)]
        fn reset_cursor_rects(&self) {
            let cursor = match self.ivars().axis.get() {
                Axis::Horizontal => NSCursor::resizeLeftRightCursor(),
                Axis::Vertical => NSCursor::resizeUpDownCursor(),
            };
            let Some(container) = self.container() else {
                return;
            };
            let own = self.ivars().card.get();
            let covers: Vec<Rect> = container
                .outlines_with_cards()
                .into_iter()
                .filter(|(card, _)| own.is_none_or(|own| *card > own))
                .map(|(_, rect)| rect)
                .collect();
            let frame = self.frame();
            for open in uncovered(
                Rect::new(
                    frame.origin.x,
                    frame.origin.y,
                    frame.size.width,
                    frame.size.height,
                ),
                self.ivars().axis.get(),
                &covers,
            ) {
                // From the container's space to the handle's own: the handle is not flipped.
                let local = self.convertRect_fromView(
                    NSRect::new(
                        NSPoint::new(open.x, open.y),
                        NSSize::new(open.width, open.height),
                    ),
                    Some(&*container),
                );
                if local.size.width > 0.0 && local.size.height > 0.0 {
                    self.addCursorRect_cursor(local, &cursor);
                }
            }
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
                container.drag_divider(
                    self.ivars().card.get(),
                    self.ivars().index.get(),
                    along - self.ivars().grab.get(),
                );
            }
        }

        /// The drag ended: the split's ratio is part of the layout on disk
        /// (once per drag, not per step).
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
            card: Cell::new(None),
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
    fn place(&self, card: Option<usize>, index: usize, divider: Divider, bounds: NSSize) {
        let iv = self.ivars();
        iv.card.set(card);
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

/// A press-and-drag on a floating card's border or header in progress
/// ([`SplitView::begin_card_drag`]).
#[derive(Clone, Copy, Debug)]
struct CardDrag {
    /// A pane of the card.
    pane: u64,
    /// The edges it moves: `None` moves the whole card (the header).
    grip: Option<Grip>,
    /// The card's shown share when the press came, and where it came (container coordinates):
    /// the drag is measured from here, so a card that was stopped at the tab's edge comes back
    /// the way it went.
    start: Area,
    origin: NSPoint,
    /// The card has changed since the press: a click that moved nothing is no layout change.
    moved: bool,
}

define_class!(
    /// The edges and corners of the floating cards, one view over every card: it takes a press
    /// only on a grip ([`floating::pick`]) and lets everything else through to the card under it.
    // SAFETY: NSView is designed for subclassing; CardGrips has no ivar and implements no
    // `Drop`; it is born with `initWithFrame:`.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriCardGrips"]
    pub(crate) struct CardGrips;

    unsafe impl NSObjectProtocol for CardGrips {}

    impl CardGrips {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        /// A grip is this view's; the rest is the card's or the flat area's.
        #[unsafe(method_id(hitTest:))]
        fn hit_test(&self, point: NSPoint) -> Option<Retained<NSView>> {
            let on_a_grip = self.container().is_some_and(|container| {
                matches!(container.card_at((point.x, point.y)), Some((_, Some(_))))
            });
            let view: &NSView = self;
            on_a_grip.then(|| view.retain())
        }

        /// The cursors of the grips, back to front so that a card in front has the last word.
        #[unsafe(method(resetCursorRects))]
        fn reset_cursor_rects(&self) {
            let Some(container) = self.container() else {
                return;
            };
            for (_, outline) in container.outlines() {
                for (grip, rect) in grip_rects(outline, GRIP_PT) {
                    self.addCursorRect_cursor(rect, &crate::pointer::grip_cursor(grip));
                }
            }
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            let Some(container) = self.container() else {
                return;
            };
            let point = container.convertPoint_fromView(event.locationInWindow(), None);
            if let Some((pane, Some(grip))) = container.card_at((point.x, point.y)) {
                container.begin_card_drag(pane, Some(grip), event.locationInWindow());
            }
        }

        #[unsafe(method(mouseDragged:))]
        fn mouse_dragged(&self, event: &NSEvent) {
            if let Some(container) = self.container() {
                container.drag_card(event.locationInWindow());
            }
        }

        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, _event: &NSEvent) {
            if let Some(container) = self.container() {
                container.end_card_drag();
            }
        }
    }
);

impl CardGrips {
    fn new(mtm: MainThreadMarker, frame: NSRect) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        // SAFETY: `initWithFrame:` is NSView's designated initializer; there is no ivar.
        unsafe { msg_send![super(this), initWithFrame: frame] }
    }

    fn container(&self) -> Option<Retained<SplitView>> {
        // SAFETY: reading the superview; we are on the main thread (`MainThreadOnly`).
        unsafe { self.superview() }?.downcast::<SplitView>().ok()
    }
}

/// The rectangles of a card's eight grips in the card's outline `card`: the four corners, `reach`
/// either side of the corner point both ways, and the four edges between them, `reach` either
/// side of the border — where [`floating::grip_at`] says the grip is.
fn grip_rects(card: Rect, reach: f64) -> Vec<(Grip, NSRect)> {
    let (x, y, right, bottom) = (card.x, card.y, card.x + card.width, card.y + card.height);
    let wide = 2.0 * reach;
    let rect = |x: f64, y: f64, width: f64, height: f64| {
        NSRect::new(
            NSPoint::new(x, y),
            NSSize::new(width.max(0.0), height.max(0.0)),
        )
    };
    vec![
        (
            Grip::Left,
            rect(x - reach, y + reach, wide, card.height - wide),
        ),
        (
            Grip::Right,
            rect(right - reach, y + reach, wide, card.height - wide),
        ),
        (
            Grip::Top,
            rect(x + reach, y - reach, card.width - wide, wide),
        ),
        (
            Grip::Bottom,
            rect(x + reach, bottom - reach, card.width - wide, wide),
        ),
        (Grip::TopLeft, rect(x - reach, y - reach, wide, wide)),
        (Grip::TopRight, rect(right - reach, y - reach, wide, wide)),
        (
            Grip::BottomLeft,
            rect(x - reach, bottom - reach, wide, wide),
        ),
        (
            Grip::BottomRight,
            rect(right - reach, bottom - reach, wide, wide),
        ),
    ]
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
    /// ([`SplitView::set_theme`]) or the root view's ([`RootView::paint_rule`]).
    pub(crate) fn new(mtm: MainThreadMarker) -> Retained<Self> {
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
    /// The flat area's split tree; its leaves are ids of [`SplitIvars::panes`], as are the
    /// leaves of every card's.
    tree: RefCell<Tree>,
    /// The floating cards over the flat area, back to front (the model is
    /// [`bt_shell_common::floating`]).
    cards: RefCell<Vec<Card>>,
    /// Each card's shadow, in the order of `cards`: a subview of this container standing just
    /// under the card's panes, so a card's shadow falls on the cards behind it and not on its own.
    shades: RefCell<Vec<Retained<card::Shade>>>,
    /// The floating cards' headers and which of their top corners are round ([`card_corners`]).
    head_corners: RefCell<Vec<(u64, (bool, bool))>>,
    /// The dividers' drag handles of the cards' own splits, per card, in the order of
    /// `cards`.
    card_handles: RefCell<Vec<Vec<Retained<DividerHandle>>>>,
    /// The frames the shown cards have now, back to front, with a pane of each: what a pointer is
    /// asked against ([`SplitView::card_at`]), kept from the last layout.
    outlines: RefCell<Vec<(u64, Rect)>>,
    /// The cards' edges and corners (made while a card is shown, [`CardGrips`]).
    grips: RefCell<Option<Retained<CardGrips>>>,
    /// The press on a card's border or header that is being dragged.
    card_drag: Cell<Option<CardDrag>>,
    /// Counts the cards let go past their margins: a card on its way back stops when a newer
    /// drag (or way back) begins ([`SplitView::settle_card`]).
    settling: Cell<u64>,
    /// The pane of the card a pointer carries, or that is on its way back inside its margins:
    /// shown where its area says ([`Ground::loose`]).
    loose: Cell<Option<u64>>,
    /// The cards growing out of their capsules now: the focus frame that comes round one of
    /// them grows with it.
    growing: RefCell<Vec<Growth>>,
    /// The capsules of the minimized cards, oldest first, the newest at the right end
    /// ([`Layers::capsules`]): subviews over everything of the container.
    capsules: RefCell<Vec<Retained<Capsule>>>,
    /// Each pane's last look ([`SplitView::set_head_looks`]): what a capsule born between two
    /// refreshes shows at once.
    looks: RefCell<Vec<(u64, HeadLook)>>,
    /// The strip's ground and the line along its top, under [`SplitStyle::Lines`] while the
    /// strip stands (cards stand on the window's ground, which the strip shows).
    floor: Retained<Hairline>,
    floor_line: Retained<Hairline>,
    /// The tab's panes, the flat area's and the cards'. The container also holds them as
    /// subviews; this list is for typed access. **Never becomes empty**: removing the last pane
    /// means closing the window ([`Removal::Last`]).
    panes: RefCell<Vec<Retained<TerminalPane>>>,
    /// The divider's fill under [`SplitStyle::Lines`]: a box in the theme's
    /// `separator` tone behind the panes, filling the container, which shows
    /// through the one pixel left open between two panes.
    backdrop: Retained<NSBox>,
    /// `line`'s hairline: the fill above the panes along the top edge, shown
    /// only while the mode is `line` and the panes touch that edge (the
    /// module header).
    hairline: Retained<Hairline>,
    /// What the content does at the top edge (`[appearance] content_edge`):
    /// the hairline's half of it.
    edge: Cell<ContentEdge>,
    /// The zoomed pane (⇧⌘↩); `None` → the splits are visible.
    zoomed: Cell<Option<u64>>,
    /// The panes' headers (`pane_head`): one over each pane in view,
    /// above the panes, laid out with them.
    heads: RefCell<Vec<Retained<PaneHead>>>,
    /// The dividers' drag handles, in the order of
    /// [`split::Layout::dividers`].
    handles: RefCell<Vec<Retained<DividerHandle>>>,
    /// The focus frame (`[appearance] focus_frame`): above the panes and their headers, round
    /// the card of [`Self::framed`] while the tab shows a split ([`Self::set_focus_frame`]).
    focus_ring: Retained<card::FocusRing>,
    /// The pane the focus frame stands round, and the theme it is drawn in; `None`: no frame.
    framed: Cell<Option<u64>>,
    ring_theme: Cell<Option<Theme>>,
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
        let floor = Hairline::new(mtm);
        let floor_line = Hairline::new(mtm);
        let focus_ring = card::FocusRing::new(mtm);
        let backdrop = NSBox::new(mtm);
        backdrop.setBoxType(NSBoxType::Custom);
        backdrop.setTitlePosition(NSTitlePosition::NoTitle);
        backdrop.setBorderWidth(0.0);
        backdrop.setHidden(true);
        let this = Self::alloc(mtm).set_ivars(SplitIvars {
            tree: RefCell::new(Tree::Leaf(first.id())),
            cards: RefCell::new(Vec::new()),
            shades: RefCell::new(Vec::new()),
            head_corners: RefCell::new(Vec::new()),
            card_handles: RefCell::new(Vec::new()),
            outlines: RefCell::new(Vec::new()),
            grips: RefCell::new(None),
            card_drag: Cell::new(None),
            settling: Cell::new(0),
            loose: Cell::new(None),
            growing: RefCell::new(Vec::new()),
            capsules: RefCell::new(Vec::new()),
            looks: RefCell::new(Vec::new()),
            floor: floor.clone(),
            floor_line: floor_line.clone(),
            panes: RefCell::new(vec![first.retain()]),
            backdrop: backdrop.clone(),
            hairline: hairline.clone(),
            edge: Cell::new(ContentEdge::default()),
            zoomed: Cell::new(None),
            heads: RefCell::new(Vec::new()),
            handles: RefCell::new(Vec::new()),
            focus_ring: focus_ring.clone(),
            framed: Cell::new(None),
            ring_theme: Cell::new(None),
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
        this.addSubview(&focus_ring);
        this.addSubview(&floor);
        this.addSubview(&floor_line);
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

    /// Every pane of the tab: the flat area's in tree order (left to right, top to bottom),
    /// then each card's, back to front.
    pub(crate) fn panes(&self) -> Vec<Retained<TerminalPane>> {
        let panes = self.ivars().panes.borrow();
        self.layers()
            .panes()
            .into_iter()
            .filter_map(|id| panes.iter().find(|pane| pane.id() == id).cloned())
            .collect()
    }

    /// The panes of the flat area, in tree order. What the window's ground stands under, and
    /// what a zoom is among.
    pub(crate) fn flat_panes(&self) -> Vec<Retained<TerminalPane>> {
        let panes = self.ivars().panes.borrow();
        self.ivars()
            .tree
            .borrow()
            .leaves()
            .into_iter()
            .filter_map(|id| panes.iter().find(|pane| pane.id() == id).cloned())
            .collect()
    }

    /// The tab's two layers as the model sees them ([`Layers`]): a copy.
    pub(crate) fn layers(&self) -> Layers {
        Layers {
            flat: self.ivars().tree.borrow().clone(),
            cards: self.ivars().cards.borrow().clone(),
        }
    }

    /// Whether pane `id` floats.
    pub(crate) fn floating(&self, id: u64) -> bool {
        self.ivars()
            .cards
            .borrow()
            .iter()
            .any(|card| card.tree.leaves().contains(&id))
    }

    /// The ground a question about the layers stands on: the container's area, the flat
    /// area's spacing, a card's and the panes' smallest sizes (`min`, from [`Self::limits`]).
    fn ground<'a>(&self, min: &'a dyn Fn(u64) -> Size) -> Ground<'a> {
        let scale = self.scale();
        Ground {
            bounds: self.bounds_rect(),
            scale,
            flat: spacing(scale, self.style(), self.beside_tabs()),
            card: card_spacing(scale),
            min,
            margins: self.card_margins(),
            loose: self.ivars().loose.get(),
        }
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

    /// The room the splits and the cards share, in points: the container less the strip the
    /// capsules stand in ([`SplitView::strip_height`]). What every layout is asked in.
    fn bounds_rect(&self) -> Rect {
        let size = self.bounds().size;
        Rect::new(
            0.0,
            0.0,
            size.width,
            (size.height - self.strip_height()).max(0.0),
        )
    }

    /// Where the flat area's panes stand: [`SplitView::bounds_rect`] less the margins the panes
    /// keep from its edges (under cards, the gap round them; under lines, none). The capsules
    /// line up with it: the strip's row under its bottom edge and flush with its right one, the
    /// corner's row in from its top right corner.
    fn panes_rect(&self) -> Rect {
        let scale = self.scale();
        let margins = spaced(scale, self.style(), self.beside_tabs());
        let (left, top, around) = (
            margins.left_px() / scale,
            margins.top_px() / scale,
            margins.around_px() / scale,
        );
        let rect = self.bounds_rect();
        Rect::new(
            rect.x + left,
            rect.y + top,
            (rect.width - left - around).max(0.0),
            (rect.height - top - around).max(0.0),
        )
    }

    /// The whole container, strip included: what the window's corners are measured against.
    fn full_rect(&self) -> Rect {
        let size = self.bounds().size;
        Rect::new(0.0, 0.0, size.width, size.height)
    }

    /// `[appearance] minimized_panes`, as the capsules' model says it. Read at every layout; a
    /// change lays every container out again.
    fn capsule_place(&self) -> CapsulePlace {
        let place = crate::app::delegate(self.mtm()).map_or(MinimizedPanes::default(), |app| {
            app.settings().minimized_panes
        });
        match place {
            MinimizedPanes::Strip => CapsulePlace::Strip,
            MinimizedPanes::TopRight => CapsulePlace::TopRight,
        }
    }

    /// The height the splits give up for the capsules' strip under `cards`: a capsule and 8
    /// points while a card is minimized and the capsules are set to the strip, nothing otherwise.
    fn strip_of(&self, cards: &[Card]) -> f64 {
        capsule::strip(
            self.capsule_place(),
            cards.iter().filter(|card| card.minimized).count(),
        )
    }

    /// [`SplitView::strip_of`] for the cards now.
    fn strip_height(&self) -> f64 {
        self.strip_of(&self.ivars().cards.borrow())
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
        let layout = if let Some(card) = self
            .ivars()
            .cards
            .borrow()
            .iter()
            .find(|card| card.tree.leaves().contains(&id))
        {
            // A floating pane is split inside its card, in the card's own room.
            let min = self.limits();
            let ground = self.ground(&min);
            let mut tree = card.tree.clone();
            if !tree.split(id, axis, HYPOTHETICAL) {
                return None;
            }
            tree.layout_spaced(ground.rect_of(card), scale, ground.card)
        } else {
            let mut tree = self.ivars().tree.borrow().clone();
            if !tree.split(id, axis, HYPOTHETICAL) {
                return None;
            }
            let spacing = spacing(scale, self.style(), self.beside_tabs());
            tree.layout_spaced(self.bounds_rect(), scale, spacing)
        };
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
        let split = self
            .ivars()
            .tree
            .borrow_mut()
            .split(target, axis, pane.id())
            || self
                .ivars()
                .cards
                .borrow_mut()
                .iter_mut()
                .any(|card| card.tree.split(target, axis, pane.id()));
        if !split {
            return false;
        }
        self.ivars().panes.borrow_mut().push(pane.retain());
        self.add_pane(pane);
        self.layout_panes();
        // One pane became two: the first slides into its card, the newcomer
        // grows from the edge it was split off.
        if before.len() == 1 && !self.floating(target) {
            self.slide(&before, Some((pane.id(), axis)), card::SLIDE_SECS);
        }
        true
    }

    /// Session restore's bulk placement: the saved `layers` replace the
    /// single-pane tree, `extra` (every pane but the one the container was
    /// born with) join as subviews and all are laid out with the saved
    /// ratios at once — no pane passes through an intermediate size. `false`
    /// and nothing changes unless the panes of `layers` are exactly the born pane
    /// and `extra`.
    pub(crate) fn adopt_layers(&self, layers: Layers, extra: &[Retained<TerminalPane>]) -> bool {
        let mut wanted: Vec<u64> = self.ivars().panes.borrow().iter().map(|p| p.id()).collect();
        wanted.extend(extra.iter().map(|pane| pane.id()));
        wanted.sort_unstable();
        let mut leaves = layers.panes();
        leaves.sort_unstable();
        if leaves != wanted {
            return false;
        }
        self.ivars().tree.replace(layers.flat);
        self.ivars().cards.replace(layers.cards);
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
    ///
    /// The cards stay as they are: `tree` is the flat area's.
    pub(crate) fn receive(&self, tree: Tree, panes: &[Retained<TerminalPane>]) -> bool {
        let cards = self.ivars().cards.borrow().clone();
        self.receive_layers(Layers { flat: tree, cards }, panes)
    }

    /// [`SplitView::receive`] for layers: a pane that floats or docks (the card structure
    /// changed) slides from where it stood to where it stands.
    pub(crate) fn receive_layers(&self, layers: Layers, panes: &[Retained<TerminalPane>]) -> bool {
        let before = self.snapshot();
        let floating_before = floating_panes(&self.ivars().cards.borrow());
        let floating_after = floating_panes(&layers.cards);
        if !self.adopt_layers(layers, panes) {
            return false;
        }
        if before.len() == 1 {
            self.slide(&before, None, card::SLIDE_SECS);
        } else if floating_before != floating_after {
            // Only a float or a dock slides; a pane moved into a tab that merely has a card
            // takes its place at once, as it does in a tab without one.
            self.slide(&before, None, card::MOVE_SECS);
            self.hide_backdrop_while_sliding(card::MOVE_SECS);
        }
        true
    }

    /// Under lines the backdrop (the dividers' tone behind the flat panes) would show as a light
    /// frame round a pane that slides into the flat area or out of it, in the part of its place
    /// it has not reached yet: it is hidden for the slide and comes back with it.
    fn hide_backdrop_while_sliding(&self, secs: f64) {
        let backdrop = &self.ivars().backdrop;
        if backdrop.isHidden() {
            return;
        }
        backdrop.setHidden(true);
        let this = self.retain();
        let back = RcBlock::new(move |_timer: NonNull<NSTimer>| this.sync_backdrop());
        // SAFETY: a one-shot timer on the main run loop with a block that takes it.
        let _ =
            unsafe { NSTimer::scheduledTimerWithTimeInterval_repeats_block(secs, false, &back) };
    }

    /// A new pane floats in front, where and as big as the wish says (`None`: the opening place
    /// and size, [`floating::NEW_SHARE`]). `false` and nothing changes when the model refuses.
    pub(crate) fn open_card(
        &self,
        pane: &TerminalPane,
        at: Option<(f64, f64)>,
        size: Option<(f64, f64)>,
    ) -> bool {
        let mut layers = self.layers();
        let opened = {
            let min = self.limits();
            let ground = self.ground(&min);
            layers.open(pane.id(), at, size, &ground)
        };
        if !opened {
            return false;
        }
        self.ivars().cards.replace(layers.cards);
        self.ivars().panes.borrow_mut().push(pane.retain());
        self.add_pane(pane);
        self.layout_panes();
        true
    }

    /// The layers as they would be were flat pane `pane` floated ([`Layers::float_out`]): in its
    /// own card, `at` and `size` as the wish says (`None`: the middle of the place it stands in,
    /// four fifths of it). `None` when the model refuses — the flat area's only pane, or a pane
    /// that does not stand in it. Nothing changes here: [`Move::Float`] carries the layers.
    pub(crate) fn floated(
        &self,
        pane: u64,
        at: Option<(f64, f64)>,
        size: Option<(f64, f64)>,
    ) -> Option<Layers> {
        let mut layers = self.layers();
        let min = self.limits();
        let ground = self.ground(&min);
        layers.float_out(pane, at, size, &ground).then_some(layers)
    }

    /// The layers as they would be were floating pane `pane` docked ([`Layers::dock`]),
    /// `beside` the flat pane the keyboard was in. `None` when there is no room or `pane` does
    /// not float.
    pub(crate) fn docked(&self, pane: u64, beside: u64) -> Option<Layers> {
        let mut layers = self.layers();
        let min = self.limits();
        let ground = self.ground(&min);
        layers.dock(pane, beside, &ground).then_some(layers)
    }

    /// [`SplitView::docked`] for a place a command names: beside flat pane `leaf`, on `side`
    /// ([`Layers::dock_beside`]). `None` when there is no room there, `pane` does not float or
    /// `leaf` is not in the flat area.
    pub(crate) fn docked_beside(&self, pane: u64, leaf: u64, side: Direction) -> Option<Layers> {
        let mut layers = self.layers();
        let min = self.limits();
        let ground = self.ground(&min);
        layers
            .dock_beside(pane, leaf, side, &ground)
            .then_some(layers)
    }

    /// Floating pane `pane`'s card moves or changes its size as the wish says (`--at`, `--size`;
    /// fractions of the tab: [`Layers::reframe`]). `true` if the card changed. Not a move: the
    /// card's place is not undone, like a drag's.
    pub(crate) fn reframe_card(
        &self,
        pane: u64,
        at: Option<(f64, f64)>,
        size: Option<(f64, f64)>,
    ) -> bool {
        let mut layers = self.layers();
        let changed = {
            let min = self.limits();
            let ground = self.ground(&min);
            layers.reframe(pane, at, size, &ground)
        };
        if changed {
            self.ivars().cards.replace(layers.cards);
            self.layout_panes();
        }
        changed
    }

    /// The card holding `pane` comes to the front and opens if it was a capsule: a click on it,
    /// the keyboard arriving in it. `true` when something changed (a card already in front and
    /// shown changes nothing); the pane views were taken out and put back, so the keyboard
    /// is the caller's to give again.
    pub(crate) fn reveal(&self, pane: u64) -> bool {
        let mut layers = self.layers();
        let Some(index) = layers.card_of(pane) else {
            return false;
        };
        if index + 1 == layers.cards.len() && !layers.cards[index].minimized {
            return false;
        }
        layers.reveal(pane);
        self.set_cards(layers.cards);
        true
    }

    /// Takes `cards` as the cards — a change of which are minimized or of their order, the
    /// panes the same. The splits give up the strip's height or take it back as the capsules
    /// come and go, sliding there as they slide from one pane to two. The pane views may be
    /// taken out and put back ([`SplitView::restack`]): the keyboard is the caller's to give
    /// again.
    pub(crate) fn set_cards(&self, cards: Vec<Card>) {
        let before = (self.strip_of(&cards) != self.strip_height()).then(|| self.snapshot());
        // The cards that go into their capsules and the ones that come out: the first fly from
        // their pictures as they are now, the second from their capsules as they are now.
        let visible = self.window().is_some_and(|window| {
            window
                .occlusionState()
                .contains(NSWindowOcclusionState::Visible)
        });
        let still = !visible
            || self.isHidden()
            || crate::app::delegate(self.mtm()).is_some_and(|app| app.reduce_motion());
        let (going, coming) = if still {
            (Vec::new(), Vec::new())
        } else {
            let was: Vec<(u64, bool)> = self
                .ivars()
                .cards
                .borrow()
                .iter()
                .map(|card| (card.tree.leaves()[0], card.minimized))
                .collect();
            let turned = |minimized: bool| -> Vec<u64> {
                cards
                    .iter()
                    .filter(|card| card.minimized == minimized)
                    .map(|card| card.tree.leaves()[0])
                    .filter(|id| was.contains(&(*id, !minimized)))
                    .collect()
            };
            let going: Vec<(u64, NSRect, Retained<NSImage>)> = turned(true)
                .into_iter()
                .filter_map(|id| {
                    let (frame, picture) = self.card_picture(id)?;
                    Some((id, frame, picture))
                })
                .collect();
            let coming: Vec<(u64, NSRect)> = turned(false)
                .into_iter()
                .filter_map(|id| Some((id, self.capsule_frame(id)?)))
                .collect();
            (going, coming)
        };
        self.ivars().cards.replace(cards);
        self.layout_panes();
        // A minimized card's panes are hidden now — their frames stop, the programs run on — and
        // an opened one's ask for a frame again.
        self.apply_visibility(visible);
        if let Some(before) = before {
            self.slide(&before, None, card::MOVE_SECS);
        }
        let begin = CACurrentMediaTime();
        for (id, frame, picture) in going {
            let Some(capsule) = self.capsule_frame(id) else {
                continue;
            };
            card::fly(
                self,
                &picture,
                frame,
                capsule,
                card::Flight::Into,
                begin,
                || {},
            );
            if let Some(view) = self.capsule_view(id) {
                card::arrive(&view);
            }
        }
        for (id, capsule) in coming {
            let Some((frame, picture)) = self.card_picture(id) else {
                continue;
            };
            // The card's own views wait unseen under the picture until it has grown; the focus
            // frame, when it comes round the card, grows with the picture ([`SplitView::place_ring`]).
            let views = self.card_views(id);
            for view in &views {
                view.setAlphaValue(0.0);
            }
            let leaves = self.card_leaves(id);
            let to_rect =
                |at: NSRect| Rect::new(at.origin.x, at.origin.y, at.size.width, at.size.height);
            self.ivars().growing.borrow_mut().push(Growth {
                leaves: leaves.clone(),
                card: to_rect(frame),
                capsule: to_rect(capsule),
                begin,
            });
            let this = self.retain();
            card::fly(
                self,
                &picture,
                frame,
                capsule,
                card::Flight::OutOf,
                begin,
                move || {
                    for view in &views {
                        view.setAlphaValue(1.0);
                    }
                    this.ivars()
                        .growing
                        .borrow_mut()
                        .retain(|growth| growth.leaves != leaves);
                },
            );
        }
        // The focus frame may already stand round the card that opens: it grows with it.
        if !self.ivars().growing.borrow().is_empty() {
            self.place_ring();
        }
    }

    /// The panes of the card holding `pane`.
    fn card_leaves(&self, pane: u64) -> Vec<u64> {
        self.ivars()
            .cards
            .borrow()
            .iter()
            .find(|card| card.tree.leaves().contains(&pane))
            .map(|card| card.tree.leaves())
            .unwrap_or_default()
    }

    /// A picture of the card holding `pane` as it stands now, and its frame: the card's ground
    /// with its corners, each pane's picture (`TerminalPane::snapshot`, the terminal drawn off
    /// screen) and each header as AppKit draws it, where they stand in the card. What a card
    /// flies into its capsule and out of it as ([`card::fly`]). `None` for a card not shown.
    fn card_picture(&self, pane: u64) -> Option<(NSRect, Retained<NSImage>)> {
        let iv = self.ivars();
        let leaves = {
            let cards = iv.cards.borrow();
            cards
                .iter()
                .find(|card| card.tree.leaves().contains(&pane))?
                .tree
                .leaves()
        };
        let outline = iv
            .outlines
            .borrow()
            .iter()
            .find(|(id, _)| leaves.contains(id))
            .map(|(_, rect)| *rect)?;
        let theme = iv.ring_theme.get()?;
        let scale = self.scale();
        let frame = NSRect::new(
            NSPoint::new(outline.x, outline.y),
            NSSize::new(outline.width, outline.height),
        );
        let mut pieces: Vec<(NSRect, Retained<NSImage>)> = Vec::new();
        for id in &leaves {
            let Some(view) = self.pane(*id) else {
                continue;
            };
            let at = view.frame();
            if let Some(still) = view.snapshot((at.size.width, at.size.height), scale) {
                pieces.push((
                    at,
                    NSImage::initWithCGImage_size(NSImage::alloc(), &still, at.size),
                ));
            }
            view.release_snapshot();
            if let Some(head) = iv.heads.borrow().iter().find(|head| head.pane() == *id) {
                let bounds = head.bounds();
                if let Some(rep) = head.bitmapImageRepForCachingDisplayInRect(bounds) {
                    head.cacheDisplayInRect_toBitmapImageRep(bounds, &rep);
                    let image = NSImage::initWithSize(NSImage::alloc(), bounds.size);
                    image.addRepresentation(&rep);
                    pieces.push((head.frame(), image));
                }
            }
        }
        let [r, g, b] = [
            (theme.background >> 16) & 0xff,
            (theme.background >> 8) & 0xff,
            theme.background & 0xff,
        ]
        .map(|byte| f64::from(byte) / 255.0);
        let origin = frame.origin;
        let draw = RcBlock::new(move |bounds: NSRect| -> objc2::runtime::Bool {
            NSColor::colorWithSRGBRed_green_blue_alpha(r, g, b, 1.0).setFill();
            NSBezierPath::fillRect(bounds);
            for (at, image) in &pieces {
                image.drawInRect(NSRect::new(
                    NSPoint::new(at.origin.x - origin.x, at.origin.y - origin.y),
                    at.size,
                ));
            }
            objc2::runtime::Bool::YES
        });
        let picture = NSImage::imageWithSize_flipped_drawingHandler(frame.size, true, &draw);
        Some((frame, picture))
    }

    /// The pill of the capsule of the card holding `pane`, in this view's space; `None` when
    /// the card is not minimized.
    fn capsule_frame(&self, pane: u64) -> Option<NSRect> {
        self.capsule_view(pane).map(|view| view.pill_frame())
    }

    /// The capsule of the card whose first pane is `pane`.
    fn capsule_view(&self, pane: u64) -> Option<Retained<Capsule>> {
        self.ivars()
            .capsules
            .borrow()
            .iter()
            .find(|view| view.pane() == pane)
            .cloned()
    }

    /// The views the card holding `pane` is drawn with: its shadow, its panes and their headers.
    fn card_views(&self, pane: u64) -> Vec<Retained<NSView>> {
        let iv = self.ivars();
        let cards = iv.cards.borrow();
        let Some(index) = cards
            .iter()
            .position(|card| card.tree.leaves().contains(&pane))
        else {
            return Vec::new();
        };
        let mut views: Vec<Retained<NSView>> = Vec::new();
        if let Some(shade) = iv.shades.borrow().get(index) {
            let view: &NSView = shade;
            views.push(view.retain());
        }
        for id in cards[index].tree.leaves() {
            if let Some(view) = self.pane(id) {
                let view: &NSView = &view;
                views.push(view.retain());
            }
            if let Some(head) = iv.heads.borrow().iter().find(|head| head.pane() == id) {
                let view: &NSView = head;
                views.push(view.retain());
            }
        }
        views
    }

    /// Takes `id` out of its layer ([`Layers::remove`]); the pane stays in the view and the list —
    /// the caller first moves the keyboard, then calls [`SplitView::detach`] (a window whose view
    /// carrying the first responder is removed would be left without a responder).
    pub(crate) fn remove_pane(&self, id: u64) -> Gone {
        let mut layers = self.layers();
        let gone = layers.remove(id);
        if matches!(gone, Gone::Closed { .. } | Gone::Landed { .. }) {
            self.ivars().tree.replace(layers.flat);
            self.ivars().cards.replace(layers.cards);
        }
        gone
    }

    /// The views of the cards, back to front — each card's shadow, panes, headers and handles, in
    /// that order — stand above everything of the flat area, and the focus frame stands over the
    /// views of the card it is round (and under the cards in front of that one) when it is round a
    /// floating pane, and under every card otherwise. Only the views that are out of place are
    /// moved: taking a pane out of its superview puts the keyboard down.
    fn restack(&self) {
        let iv = self.ivars();
        let cards = iv.cards.borrow();
        let shades = iv.shades.borrow();
        let card_handles = iv.card_handles.borrow();
        let heads = iv.heads.borrow();
        let ring: &NSView = &iv.focus_ring;
        let framed = iv.framed.get();
        let mut wanted: Vec<Retained<NSView>> = Vec::new();
        // How many card views stand under the focus frame when it is round a floating pane: it
        // stands over that card's own views and under the cards in front of it.
        let mut ring_slot: Option<usize> = None;
        for (index, card) in cards.iter().enumerate() {
            if let Some(shade) = shades.get(index) {
                let view: &NSView = shade;
                wanted.push(view.retain());
            }
            let ids = card.tree.leaves();
            for id in &ids {
                if let Some(pane) = self.pane(*id) {
                    let view: &NSView = &pane;
                    wanted.push(view.retain());
                }
                if let Some(head) = heads.iter().find(|head| head.pane() == *id) {
                    let view: &NSView = head;
                    wanted.push(view.retain());
                }
            }
            if let Some(list) = card_handles.get(index) {
                for handle in list {
                    let view: &NSView = handle;
                    wanted.push(view.retain());
                }
            }
            if framed.is_some_and(|id| ids.contains(&id)) {
                ring_slot = Some(wanted.len());
            }
        }
        if wanted.is_empty() {
            return;
        }
        // The grips stand over every card: a press on an edge is the front card's or the one
        // behind it, never a pane's.
        if let Some(grips) = iv.grips.borrow().as_ref() {
            let view: &NSView = grips;
            wanted.push(view.retain());
        }
        // The capsules stand over the cards and the grips: a capsule at the corner is on the
        // content, and a press on it is its own.
        for capsule in iv.capsules.borrow().iter() {
            let view: &NSView = capsule;
            wanted.push(view.retain());
        }
        let subviews = self.subviews();
        // Read afresh at every step: once a view has moved, the order taken before it no longer
        // says which view stands right over which (a card's header judged by the old order stayed
        // under the shade its pane had been moved over).
        let at = |view: &NSView| {
            self.subviews()
                .iter()
                .position(|candidate| std::ptr::eq(&*candidate, view))
        };
        let own = |view: &NSView| {
            std::ptr::eq(view, ring) || wanted.iter().any(|mine| std::ptr::eq(&**mine, view))
        };
        // The highest view of the flat area: what the cards stand on.
        let mut anchor: Option<Retained<NSView>> = subviews
            .iter()
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .find(|view| !own(view));
        let put = |view: &NSView, anchor: &mut Option<Retained<NSView>>| {
            let in_place = match (anchor.as_deref(), at(view)) {
                (Some(under), Some(now)) => at(under).is_some_and(|below| now == below + 1),
                (None, Some(now)) => now == 0,
                _ => false,
            };
            if !in_place {
                self.addSubview_positioned_relativeTo(
                    view,
                    NSWindowOrderingMode::Above,
                    anchor.as_deref(),
                );
            }
            *anchor = Some(view.retain());
        };
        if ring_slot.is_none() {
            put(ring, &mut anchor);
        }
        for (placed, view) in wanted.iter().enumerate() {
            if ring_slot == Some(placed) {
                put(ring, &mut anchor);
            }
            put(view, &mut anchor);
        }
        if ring_slot == Some(wanted.len()) {
            put(ring, &mut anchor);
        }
    }

    /// Where the pane of a terminal a program on a server asked for goes beside `anchor` of this
    /// tab, as the agent asked (`ask`): the rule is [`bt_shell_common::agent::place::place`],
    /// in this container's room with the spacing of the tree it becomes. `incoming` is the id
    /// the pane will have; it does not exist yet, so its smallest size is the anchor's (the
    /// same font, the same card). `None` if `anchor` is not a pane of this tab.
    pub(crate) fn plan_agent_pane(
        &self,
        anchor: u64,
        incoming: u64,
        ask: &bt_shell_common::agent::place::Ask,
    ) -> Option<bt_shell_common::agent::place::Landing> {
        let scale = self.scale();
        let own = self.ivars().panes.borrow().clone();
        let measured = |id: u64| {
            own.iter()
                .find(|pane| pane.id() == id)
                .and_then(|pane| pane.min_size())
                .map_or(Size::new(0.0, 0.0), |min| Size::new(min.width, min.height))
        };
        let anchor_min = measured(anchor);
        let min = move |id: u64| {
            if id == incoming {
                anchor_min
            } else {
                measured(id)
            }
        };
        let tree = self.ivars().tree.borrow();
        let room = Room {
            bounds: self.bounds_rect(),
            scale,
            spacing: spacing(scale, self.style(), self.beside_tabs()),
            min: &min,
        };
        bt_shell_common::agent::place::place(
            &tree,
            anchor,
            incoming,
            ask,
            self.zoomed().is_some(),
            &room,
        )
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
            spacing: spacing(scale, self.style(), self.beside_tabs()),
            min: &min,
        };
        tree.plan_beside(leaf, side, incoming, &room)
    }

    /// Where pane `moving` of this tab would land if it moved beside `leaf` on `side`, within the
    /// same tab: the tree without it takes it back, in this container's room
    /// ([`Tree::plan_beside`]). `None` when `leaf` or `moving` is not a pane here, they are one,
    /// or not even the whole area has room.
    pub(crate) fn plan_move_within(
        &self,
        moving: u64,
        leaf: u64,
        side: Direction,
    ) -> Option<Placement> {
        if moving == leaf {
            return None;
        }
        let scale = self.scale();
        let own = self.ivars().panes.borrow().clone();
        let min = move |id: u64| {
            own.iter()
                .find(|pane| pane.id() == id)
                .and_then(|pane| pane.min_size())
                .map_or(Size::new(0.0, 0.0), |min| Size::new(min.width, min.height))
        };
        let mut rest = self.ivars().tree.borrow().clone();
        if !matches!(rest.remove(moving), Removal::Removed { .. }) {
            return None;
        }
        let room = Room {
            bounds: self.bounds_rect(),
            scale,
            spacing: spacing(scale, self.style(), self.beside_tabs()),
            min: &min,
        };
        rest.plan_beside(leaf, side, &Tree::Leaf(moving), &room)
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
            return self.swap_in_card(a, b, &before);
        }
        if !self.fits() {
            self.ivars().tree.borrow_mut().swap(a, b);
            return false;
        }
        self.layout_panes();
        self.slide(&before, None, card::MOVE_SECS);
        true
    }

    /// Panes `a` and `b` of one card trade places inside it: the card keeps its place and its
    /// share (it is held to the least its tree then needs when it is laid out). `false` and no
    /// change if they are not in one card, or are one pane — a pane of the flat area and one
    /// of a card do not trade: they are not in one layer.
    fn swap_in_card(&self, a: u64, b: u64, before: &[Before]) -> bool {
        let mut layers = self.layers();
        let (Some(first), Some(second)) = (layers.card_of(a), layers.card_of(b)) else {
            return false;
        };
        if first != second || !layers.cards[first].tree.swap(a, b) {
            return false;
        }
        self.ivars().cards.replace(layers.cards);
        self.layout_panes();
        self.slide(before, None, card::MOVE_SECS);
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
        let scale = self.scale();
        let room = Room {
            bounds: self.bounds_rect(),
            scale,
            spacing: spacing(scale, self.style(), self.beside_tabs()),
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
            spacing(scale, self.style(), self.beside_tabs()),
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

    /// The tabs that hold `ids` opened together, as [`split::arrange`] lays them out in this
    /// container's room — a new tab of the window is the same size — with every pane held to
    /// `min` ([`TerminalPane::min_size`], measured on a pane of this window). `ids` are the
    /// panes' places, not yet their identities: the caller maps them once the panes are born.
    pub(crate) fn arranged(&self, ids: &[u64], min: Size) -> Vec<Tree> {
        let scale = self.scale();
        let smallest = move |_: u64| min;
        let room = Room {
            // The whole container: a new tab has no capsule strip of its own yet.
            bounds: self.full_rect(),
            scale,
            spacing: spacing(scale, self.style(), self.beside_tabs()),
            min: &smallest,
        };
        split::arrange(ids, &room)
    }

    /// Whether every pane's plain (unzoomed) frame passes its smallest-pane
    /// limit ([`TerminalPane::min_size`]) — a restored tree from a larger
    /// screen or a smaller font may not.
    pub(crate) fn fits(&self) -> bool {
        let limits = self.limits();
        self.ivars().tree.borrow().fits(&self.room(&limits))
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
            pane.set_visibility(window_visible && !pane.isHiddenOrHasHiddenAncestor());
        }
    }

    /// `from`'s neighbour in the direction (⌥⌘ + arrow; [`Layers::toward`]): in the flat area
    /// the neighbour there, in a card the one inside it and else the flat pane beyond its edge.
    pub(crate) fn neighbour(&self, from: u64, direction: Direction) -> Option<u64> {
        let min = self.limits();
        let ground = self.ground(&min);
        self.layers().toward(from, direction, &ground)
    }

    /// The next or previous pane (⌘] / ⌘[; [`Layers::cycle`]): the flat area, then the shown
    /// cards, the oldest first.
    pub(crate) fn cycle(&self, from: u64, forward: bool) -> Option<u64> {
        self.layers().cycle(from, forward)
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
            spacing: spacing(scale, self.style(), self.beside_tabs()),
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
    fn drag_divider(&self, card: Option<usize>, index: usize, position: f64) {
        let limits = self.limits();
        let moved = match card {
            None => {
                let room = self.room(&limits);
                self.ivars()
                    .tree
                    .borrow_mut()
                    .drag_within(index, position, &room)
            }
            Some(at) => {
                let ground = self.ground(&limits);
                let mut cards = self.ivars().cards.borrow_mut();
                let Some(card) = cards.get_mut(at) else {
                    return;
                };
                let room = Room {
                    bounds: ground.rect_of(card),
                    scale: ground.scale,
                    spacing: ground.card,
                    min: &limits,
                };
                card.tree.drag_within(index, position, &room)
            }
        };
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
            // Above the panes of the flat area and under the cards.
            let mut under: Retained<NSView> =
                self.ivars().hairline.retain().into_super().into_super();
            for _ in dividers {
                let handle = DividerHandle::new(self.mtm());
                self.addSubview_positioned_relativeTo(
                    &handle,
                    NSWindowOrderingMode::Above,
                    Some(&under),
                );
                under = (&*handle as &NSView).retain();
                handles.push(handle);
            }
        }
        let size = self.bounds().size;
        for (index, (handle, divider)) in handles.iter().zip(dividers).enumerate() {
            handle.place(None, index, *divider, size);
        }
    }

    /// The handles of card `at`'s own dividers: built again when their number changed, each above
    /// the card's last view (the restack settles the order).
    fn sync_card_handles(&self, at: usize, dividers: &[Divider], last: Option<&NSView>) {
        let mut all = self.ivars().card_handles.borrow_mut();
        while all.len() <= at {
            all.push(Vec::new());
        }
        let handles = &mut all[at];
        if handles.len() != dividers.len() {
            for handle in handles.drain(..) {
                handle.removeFromSuperview();
            }
            for _ in dividers {
                let handle = DividerHandle::new(self.mtm());
                self.addSubview_positioned_relativeTo(&handle, NSWindowOrderingMode::Above, last);
                handles.push(handle);
            }
        }
        let size = self.bounds().size;
        for (index, (handle, divider)) in handles.iter().zip(dividers).enumerate() {
            handle.place(Some(at), index, *divider, size);
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
        self.ivars().ring_theme.set(Some(*theme));
        for shade in self.ivars().shades.borrow().iter() {
            shade.paint(theme);
            shade.set_plate(Some(theme.separator_srgb()));
        }
        self.place_ring();
    }

    /// The focus frame stands round pane `pane`'s card, in `theme`'s frame colour, or nowhere
    /// (`None`): the focused pane with `[appearance] focus_frame` on (`TerminalTab::refresh_look`),
    /// shown only while the tab shows a split. A move to another pane slides it there.
    pub(crate) fn set_focus_frame(&self, pane: Option<u64>, theme: &Theme) {
        let moved = self.ivars().framed.replace(pane) != pane;
        self.ivars().ring_theme.set(Some(*theme));
        // Over the cards the frame stands among their views, right over the card it is round
        // ([`SplitView::restack`]): a frame that goes to another card goes up or down with it.
        if moved && !self.ivars().cards.borrow().is_empty() {
            self.restack();
        }
        self.place_ring();
    }

    /// The focus frame where the layout has its card now: header and pane, with the card's
    /// corners — a card's all round, a lined pane's square on top and the window's curve where
    /// its bottom meets the window's.
    fn place_ring(&self) {
        let iv = self.ivars();
        let Some(theme) = iv.ring_theme.get() else {
            return;
        };
        let still = crate::app::delegate(self.mtm()).is_some_and(|app| app.reduce_motion());
        let at = iv
            .framed
            .get()
            .filter(|_| self.shows_split())
            .and_then(|id| self.pane(id))
            .filter(|pane| !pane.isHidden())
            .map(|pane| {
                let (left, right) = pane.corners();
                // A floating pane is a card under either style; a split card's header is round
                // only at the card's own corners.
                let top = if self.carded() || self.floating(pane.id()) {
                    card::corner_pt()
                } else {
                    0.0
                };
                let (top_left, top_right) = iv
                    .head_corners
                    .borrow()
                    .iter()
                    .find(|(id, _)| *id == pane.id())
                    .map_or((true, true), |(_, corners)| *corners);
                let top_left = if top_left { top } else { 0.0 };
                let top_right = if top_right { top } else { 0.0 };
                let frame = self.card_frame(&pane);
                // A lined pane at the window's top edge (the tabs in the side column) has the
                // window's corner there, as its bottom corners do at the bottom edge: a square
                // frame would run past the window's curve and be cut by it.
                let (top_left, top_right) = if top > 0.0 {
                    (top_left, top_right)
                } else {
                    top_corners(
                        Rect::new(
                            frame.origin.x,
                            frame.origin.y,
                            frame.size.width,
                            frame.size.height,
                        ),
                        self.full_rect(),
                        self.beside_tabs(),
                        card::window_corner_pt(self.window().as_deref()),
                    )
                };
                (pane.id(), frame, (top_left, top_right, left, right))
            });
        // Round a card that is growing out of its capsule the frame does not slide there: it
        // grows with the card, from the capsule, in step with it.
        let growth = at.as_ref().and_then(|(id, _, _)| {
            iv.growing
                .borrow()
                .iter()
                .find(|growth| growth.leaves.contains(id))
                .map(|growth| (growth.card, growth.capsule, growth.begin))
        });
        iv.focus_ring.place(at, &theme, still || growth.is_some());
        if let Some((card_rect, capsule, begin)) = growth
            && CACurrentMediaTime() < begin + card::SPRING_SECS
            && let Some(layer) = iv.focus_ring.layer()
        {
            let frame = iv.focus_ring.frame();
            card::follow(
                &layer,
                Rect::new(
                    frame.origin.x,
                    frame.origin.y,
                    frame.size.width,
                    frame.size.height,
                ),
                card_rect,
                capsule,
                card::Flight::OutOf,
                begin,
            );
        }
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

    /// With the tabs in the side column the container's top edge is the window's, with no bar
    /// above it for the line to stand under: it would read as a stray border across the
    /// window's top, so it stays hidden and `line` cuts the text like `cut` does. The column
    /// has its own rule along the panes' left edge.
    fn show_hairline(&self, panes_touch_the_edge: bool) {
        let line = self.ivars().edge.get() == ContentEdge::Line;
        self.ivars()
            .hairline
            .setHidden(!(line && panes_touch_the_edge && !self.beside_tabs()));
    }

    /// The room a tab's only pane leaves round itself, in points: the card's
    /// margins across (left and right) and down (top and bottom) under cards,
    /// none under lines, and its header above it under either. A window fitted
    /// to a lone pane's cells adds it ([`crate::window::TerminalWindow`]'s
    /// step), so its text keeps the same margins in a card as it does filling
    /// the tab.
    pub(crate) fn margins(&self) -> NSSize {
        let scale = self.scale();
        let room = spacing(scale, self.style(), self.beside_tabs());
        NSSize::new(
            (room.left_px() + room.around_px()) / scale,
            (room.top_px() + room.head_px() + room.around_px()) / scale,
        )
    }

    /// Whether the panes are cards now: under `[appearance] split_style =
    /// "cards"`, every pane, a tab's only one and a zoomed one too.
    pub(crate) fn carded(&self) -> bool {
        self.style() == SplitStyle::Cards
    }

    /// Two or more panes in view: not one pane, not a zoomed one. The focus
    /// frame and the lines' divider fill show only then.
    pub(crate) fn shows_split(&self) -> bool {
        let flat = if self.flat_split() { 2 } else { 1 };
        let floating: usize = self
            .ivars()
            .cards
            .borrow()
            .iter()
            .filter(|card| !card.minimized)
            .map(|card| card.tree.leaves().len())
            .sum();
        flat + floating > 1
    }

    /// Two panes or more of the flat area in view: not one pane, not a zoomed one. The lines'
    /// divider fill shows only then.
    fn flat_split(&self) -> bool {
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

    /// Whether the side column stands along the container's left edge ([`spacing`],
    /// [`bottom_corners`]): the tabs are at the side and the window has not hidden the column
    /// ([`RootView::column_shown`]). Read at every layout, like [`Self::style`]; a change lays
    /// every container out again.
    fn beside_tabs(&self) -> bool {
        self.root().map_or_else(
            || crate::window::tabs_at_side(self.mtm()),
            |root| root.column_shown(),
        )
    }

    /// The divider's fill shows under [`SplitStyle::Lines`] while two panes
    /// or more are in view.
    fn sync_backdrop(&self) {
        let backdrop = &self.ivars().backdrop;
        let splits = self.bounds_rect();
        backdrop.setFrame(NSRect::new(
            NSPoint::new(splits.x, splits.y),
            NSSize::new(splits.width, splits.height),
        ));
        let shown = self.flat_split() && self.style() == SplitStyle::Lines;
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

    /// Applies the tree's frames to the panes. Under lines no fitting with a
    /// single pane: the pane is the container's bounds themselves, as before
    /// splitting. While zoomed only the zoomed pane is visible, the others are
    /// hidden with their frames in place. Under cards every pane, one alone
    /// or zoomed too, is laid out as a card ([`spacing`]); every pane is told
    /// so, which also gives it the scale its one-pixel frame needs.
    pub(crate) fn layout_panes(&self) {
        let panes = self.ivars().panes.borrow().clone();
        let scale = self.scale();
        let zoomed = {
            let tree = self.ivars().tree.borrow();
            self.ivars()
                .zoomed
                .get()
                .filter(|id| tree.leaves().contains(id))
        };
        let carded = self.carded();
        self.sync_backdrop();
        // The hairline's frame is the container's top edge, whatever the
        // tree: one pane, splits and zoom all get it. One device pixel, from
        // the same scale the frames snap to.
        let width = self.bounds().size.width;
        self.ivars().hairline.setFrame(NSRect::new(
            NSPoint::new(0.0, 0.0),
            NSSize::new(width, 1.0 / scale),
        ));
        self.show_hairline(!carded);
        // Every pane in view stands under its header — a tab's only one and a zoomed one too: a
        // pane is always there to be taken by it, whatever its program covers.
        for pane in &panes {
            pane.set_headed(true);
        }
        let layout = self.ivars().tree.borrow().layout_zoomed_spaced(
            self.bounds_rect(),
            scale,
            zoomed,
            spacing(scale, self.style(), self.beside_tabs()),
        );
        let layers = self.layers();
        let in_cards = layers
            .cards
            .iter()
            .flat_map(|card| card.tree.leaves())
            .collect::<Vec<_>>();
        for pane in &panes {
            // The cards' panes are laid out below, with their cards.
            if in_cards.contains(&pane.id()) {
                continue;
            }
            match layout.panes.iter().find(|(id, _)| *id == pane.id()) {
                Some((_, rect)) => {
                    pane.setHidden(false);
                    pane.set_card(carded);
                    // Together with the frame: the frame's resize takes the
                    // geometry once, with the corners already known.
                    let (left, right) = self.corners_of(*rect, carded);
                    pane.place(
                        NSRect::new(
                            NSPoint::new(rect.x, rect.y),
                            NSSize::new(rect.width, rect.height),
                        ),
                        left,
                        right,
                    );
                }
                None => pane.setHidden(true),
            }
        }
        let mut placed = layout.panes.clone();
        placed.extend(self.layout_cards(&layers));
        self.place_heads(&placed);
        self.sync_handles(&layout.dividers);
        self.place_capsules();
        self.restack();
        self.place_ring();
        self.ground_follows();
    }

    /// Lays the floating cards out over the flat area: each card's frame, its panes in it, its
    /// shadow and its own dividers' handles. The panes of a card shown, with their frames,
    /// for the headers to stand over.
    fn layout_cards(&self, layers: &Layers) -> Vec<(u64, Rect)> {
        let iv = self.ivars();
        let min = self.limits();
        let ground = self.ground(&min);
        let arranged = layers.layout(&ground).cards;
        let theme = iv.ring_theme.get();
        let bounds = self.bounds();
        // One shadow per card, made and dropped as cards come and go.
        {
            let mut shades = iv.shades.borrow_mut();
            while shades.len() > layers.cards.len() {
                if let Some(shade) = shades.pop() {
                    shade.removeFromSuperview();
                }
            }
            while shades.len() < layers.cards.len() {
                let shade = card::Shade::new(self.mtm(), bounds);
                if let Some(theme) = &theme {
                    shade.paint(theme);
                    shade.set_plate(Some(theme.separator_srgb()));
                }
                self.addSubview(&shade);
                shades.push(shade);
            }
            let mut handles = iv.card_handles.borrow_mut();
            while handles.len() > layers.cards.len() {
                if let Some(gone) = handles.pop() {
                    for handle in gone {
                        handle.removeFromSuperview();
                    }
                }
            }
        }
        let mut placed = Vec::new();
        let mut outlines = Vec::new();
        let mut heads = Vec::new();
        for (index, (card, (rect, inner))) in layers.cards.iter().zip(&arranged).enumerate() {
            let mut focused = false;
            let mut last: Option<Retained<NSView>> = None;
            for (id, frame) in &inner.panes {
                let Some(pane) = self.pane(*id) else {
                    continue;
                };
                pane.setHidden(card.minimized);
                pane.set_headed(true);
                pane.set_card(true);
                focused |= pane.shows_focus();
                // Round only where the card is: panes that meet inside it are square there.
                let corners = card_corners(*frame, *rect);
                heads.push((*id, corners.top));
                let (left, right) = self.corners_of(*frame, true);
                let left = if corners.bottom.0 { left } else { 0.0 };
                let right = if corners.bottom.1 { right } else { 0.0 };
                pane.place(
                    NSRect::new(
                        NSPoint::new(frame.x, frame.y),
                        NSSize::new(frame.width, frame.height),
                    ),
                    left,
                    right,
                );
                if !card.minimized {
                    placed.push((*id, *frame));
                }
                let view: &NSView = &pane;
                last = Some(view.retain());
            }
            let outline = NSRect::new(
                NSPoint::new(rect.x, rect.y),
                NSSize::new(rect.width, rect.height),
            );
            if let Some(shade) = iv.shades.borrow().get(index) {
                shade.setFrame(bounds);
                // The shade is not flipped and the container is: the card's frame goes through
                // the conversion, as the window's shade takes the flat cards' (`sync_ground`).
                let at = shade.convertRect_fromView(outline, Some(self));
                shade.set_cards(if card.minimized {
                    Vec::new()
                } else {
                    vec![(at, focused)]
                });
            }
            let dividers: &[Divider] = if card.minimized { &[] } else { &inner.dividers };
            self.sync_card_handles(index, dividers, last.as_deref());
            if !card.minimized
                && let Some(pane) = card.tree.leaves().first()
            {
                outlines.push((*pane, *rect));
            }
        }
        iv.outlines.replace(outlines);
        iv.head_corners.replace(heads);
        self.sync_grips();
        placed
    }

    /// The grips' view exists while a card is shown, over the whole container, and its cursor
    /// rects follow the cards.
    fn sync_grips(&self) {
        let iv = self.ivars();
        let shown = !iv.outlines.borrow().is_empty();
        let mut grips = iv.grips.borrow_mut();
        match (shown, grips.as_ref()) {
            (true, None) => {
                let view = CardGrips::new(self.mtm(), self.bounds());
                self.addSubview(&view);
                *grips = Some(view);
            }
            (false, Some(view)) => {
                view.removeFromSuperview();
                *grips = None;
            }
            _ => {}
        }
        if let Some(view) = grips.as_ref() {
            view.setFrame(self.bounds());
            if let Some(window) = self.window() {
                window.invalidateCursorRectsForView(view);
            }
        }
        drop(grips);
        // The dividers' cursors stop where the cards stand: a card that moved, came or went
        // changes where they are.
        if let Some(window) = self.window() {
            for handle in iv.handles.borrow().iter() {
                window.invalidateCursorRectsForView(handle);
            }
            for handle in iv.card_handles.borrow().iter().flatten() {
                window.invalidateCursorRectsForView(handle);
            }
        }
    }

    /// The frames of the shown cards, back to front, with a pane of each.
    fn outlines(&self) -> Vec<(u64, Rect)> {
        self.ivars().outlines.borrow().clone()
    }

    /// The frames of the shown cards with each one's place among the cards (back to front):
    /// what stands over a card's own dividers is only the cards in front of it.
    fn outlines_with_cards(&self) -> Vec<(usize, Rect)> {
        let layers = self.layers();
        self.outlines()
            .into_iter()
            .filter_map(|(pane, rect)| Some((layers.card_of(pane)?, rect)))
            .collect()
    }

    /// What a pointer at `point` (container coordinates) is on among the shown cards: a pane of
    /// the front-most card it is on or grips, and the grip, `None` for the card's inside
    /// ([`floating::pick`]).
    fn card_at(&self, point: (f64, f64)) -> Option<(u64, Option<Grip>)> {
        let outlines = self.ivars().outlines.borrow();
        let frames: Vec<Rect> = outlines.iter().map(|(_, rect)| *rect).collect();
        let (index, grip) = floating::pick(&frames, point, GRIP_PT)?;
        Some((outlines.get(index)?.0, grip))
    }

    /// A press on the card holding `pane` — its border (`grip`) or header (`None`) — at `at`
    /// (window coordinates): the drag that follows moves the card or the grip's edges
    /// ([`SplitView::drag_card`]). The card's shown share now is where it is measured from.
    pub(crate) fn begin_card_drag(&self, pane: u64, grip: Option<Grip>, at: NSPoint) {
        let layers = self.layers();
        let Some(index) = layers.card_of(pane) else {
            return;
        };
        let min = self.limits();
        // From where the card is seen, which may be inside its margins while its area is not.
        let start = self.ground(&min).shown(&layers.cards[index]);
        self.ivars().loose.set(Some(pane));
        self.ivars().card_drag.set(Some(CardDrag {
            pane,
            grip,
            start,
            origin: self.convertPoint_fromView(at, None),
            moved: false,
        }));
        // The pointer keeps the cursor it was pressed with until the drag ends: under a moving
        // card the cursor rects of what it passes over would take it.
        if let Some(window) = self.window() {
            window.disableCursorRects();
        }
        // A card still on its way back from the last drag stays where it is now.
        self.ivars().settling.set(self.ivars().settling.get() + 1);
    }

    /// The pointer is at `at` (window coordinates) in a card drag: the card follows, inside the
    /// tab and no smaller than its least. A drag is not undone, like a divider's.
    pub(crate) fn drag_card(&self, at: NSPoint) {
        let Some(drag) = self.ivars().card_drag.get() else {
            return;
        };
        let bounds = self.bounds_rect();
        if bounds.width <= 0.0 || bounds.height <= 0.0 {
            return;
        }
        let point = self.convertPoint_fromView(at, None);
        let (dx, dy) = (
            (point.x - drag.origin.x) / bounds.width,
            (point.y - drag.origin.y) / bounds.height,
        );
        let min = self.limits();
        let ground = self.ground(&min);
        let margins = self.card_margins();
        let pull = Pull::of_points(EDGE_GAP_PT, MAGNET_PT, self.bounds_rect());
        let changed = {
            let mut cards = self.ivars().cards.borrow_mut();
            let Some(card) = cards
                .iter_mut()
                .find(|card| card.tree.leaves().contains(&drag.pane))
            else {
                return;
            };
            let least = ground.least(&card.tree);
            // Carried, the card goes into its margins against a spring ([`Area::carried`]) and
            // comes back inside them when let go ([`SplitView::end_card_drag`]); an edge pulled by
            // a grip stops at the margin, so the card is not resized again on the way back.
            let area = match drag.grip {
                None => Area::new(
                    drag.start.x + dx,
                    drag.start.y + dy,
                    drag.start.width,
                    drag.start.height,
                )
                .carried(margins, pull),
                Some(grip) => drag
                    .start
                    .dragged(grip, dx, dy, least)
                    .held_inside(grip, margins, least),
            };
            let changed = area != card.area;
            card.area = area;
            changed
        };
        if changed {
            self.ivars().card_drag.set(Some(CardDrag {
                moved: true,
                ..drag
            }));
            self.layout_panes();
        }
    }

    /// The drag ended: the card's place is part of the layout on disk (once per drag, not per
    /// step), if the card moved at all. `false` when no drag was on.
    pub(crate) fn end_card_drag(&self) -> bool {
        let Some(drag) = self.ivars().card_drag.take() else {
            return false;
        };
        if let Some(window) = self.window() {
            window.enableCursorRects();
            window.resetCursorRects();
        }
        // A card carried past its margins comes back inside them; the layout on disk takes
        // where it comes to rest.
        if drag.moved && drag.grip.is_none() && self.settle_card(drag.pane) {
            return true;
        }
        self.ivars().loose.set(None);
        if drag.moved
            && let Some(app) = crate::app::delegate(self.mtm())
        {
            app.layout_changed();
        }
        true
    }

    /// The room a card keeps from the tab's edges once it is let go ([`Margins`]): under cards
    /// the flat cards' own margins — its edges line up with theirs —, under lines
    /// [`FLOAT_MARGIN_PT`] all round.
    fn card_margins(&self) -> Margins {
        let bounds = self.bounds_rect();
        if self.carded() {
            let panes = self.panes_rect();
            Margins::of_points(
                panes.x - bounds.x,
                panes.y - bounds.y,
                (bounds.x + bounds.width) - (panes.x + panes.width),
                (bounds.y + bounds.height) - (panes.y + panes.height),
                bounds,
            )
        } else {
            let margin = FLOAT_MARGIN_PT;
            Margins::of_points(margin, margin, margin, margin, bounds)
        }
    }

    /// Brings the card holding `pane` to rest ([`Area::rest`]: back inside its margins, into a
    /// corner it was let go near) over [`SETTLE_SECS`], easing out,
    /// one layout a frame as a drag lays it out; at once under Reduce Motion. The layout on disk
    /// takes where it ends. `false` when the card is inside them already (nothing to do).
    fn settle_card(&self, pane: u64) -> bool {
        let Some(from) = self
            .ivars()
            .cards
            .borrow()
            .iter()
            .find(|card| card.tree.leaves().contains(&pane))
            .map(|card| card.area)
        else {
            return false;
        };
        let pull = Pull::of_points(EDGE_GAP_PT, MAGNET_PT, self.bounds_rect());
        let to = from.rest(self.card_margins(), pull);
        if to == from {
            return false;
        }
        let round = self.ivars().settling.get() + 1;
        self.ivars().settling.set(round);
        self.ivars().loose.set(Some(pane));
        let still = crate::app::delegate(self.mtm()).is_some_and(|app| app.reduce_motion());
        if still {
            self.ivars().loose.set(None);
            self.set_card_area(pane, to);
            self.settled();
            return true;
        }
        let start = std::time::Instant::now();
        let this = self.retain();
        let tick = RcBlock::new(move |timer: NonNull<NSTimer>| {
            let stop = || {
                // SAFETY: the timer is the one this block runs for, alive while it runs.
                unsafe { timer.as_ref() }.invalidate();
            };
            // A newer drag took the card: it stays where that one puts it.
            if this.ivars().settling.get() != round {
                stop();
                return;
            }
            let t = (start.elapsed().as_secs_f64() / SETTLE_SECS).min(1.0);
            let eased = 1.0 - (1.0 - t).powi(3);
            if !this.set_card_area(pane, from.toward(to, eased)) && t < 1.0 {
                // The card went (closed, docked) on the way.
                this.ivars().loose.set(None);
                stop();
                return;
            }
            if t >= 1.0 {
                this.ivars().loose.set(None);
                this.set_card_area(pane, to);
                this.settled();
                stop();
            }
        });
        // SAFETY: a repeating timer on the main run loop with a block that takes it.
        let _ = unsafe {
            NSTimer::scheduledTimerWithTimeInterval_repeats_block(1.0 / 120.0, true, &tick)
        };
        true
    }

    /// The card holding `pane` takes `area` and is laid out there; `false` when there is no
    /// such card.
    fn set_card_area(&self, pane: u64, area: Area) -> bool {
        {
            let mut cards = self.ivars().cards.borrow_mut();
            let Some(card) = cards
                .iter_mut()
                .find(|card| card.tree.leaves().contains(&pane))
            else {
                return false;
            };
            card.area = area;
        }
        self.layout_panes();
        true
    }

    /// A card came to rest: its place is part of the layout on disk.
    fn settled(&self) {
        if let Some(app) = crate::app::delegate(self.mtm()) {
            app.layout_changed();
        }
    }

    /// ⌃⌘ + arrow on floating pane `pane` ([`Layers::resize_key`]): its card's right and bottom
    /// edges move by one key step. `true` if the card changed.
    pub(crate) fn resize_card(&self, pane: u64, direction: Direction) -> bool {
        self.nudge_card(pane, direction, Layers::resize_key)
    }

    /// ⌃⌥⌘ + arrow on floating pane `pane` ([`Layers::move_key`]): its card moves by one key
    /// step, inside the tab. `true` if the card changed.
    pub(crate) fn move_card(&self, pane: u64, direction: Direction) -> bool {
        self.nudge_card(pane, direction, Layers::move_key)
    }

    fn nudge_card(
        &self,
        pane: u64,
        direction: Direction,
        key: fn(&mut Layers, u64, Direction, f64, &Ground<'_>) -> bool,
    ) -> bool {
        let mut layers = self.layers();
        let step = floating::key_step(direction, self.bounds_rect());
        let moved = {
            let min = self.limits();
            let ground = self.ground(&min);
            key(&mut layers, pane, direction, step, &ground)
        };
        if moved {
            self.ivars().cards.replace(layers.cards);
            self.layout_panes();
        }
        moved
    }

    /// Puts a header over each pane `placed` (its id and frame), and takes the others away. Each
    /// header learns the grid's margins under it, so its mark stands on the text's left line.
    fn place_heads(&self, placed: &[(u64, Rect)]) {
        let mtm = self.mtm();
        let mut heads = self.ivars().heads.borrow_mut();
        let wanted: Vec<u64> = placed.iter().map(|&(id, _)| id).collect();
        heads.retain(|head| {
            let keep = wanted.contains(&head.pane());
            if !keep {
                head.removeFromSuperview();
            }
            keep
        });
        for &(id, rect) in placed {
            let head = match heads.iter().find(|head| head.pane() == id) {
                Some(head) => head.clone(),
                None => {
                    let head = PaneHead::new(mtm, id);
                    // A header made again (its card opened from a capsule) shows its pane's last
                    // look at once, not a blank band until the tab's next look.
                    if let (Some(theme), Some((_, look))) = (
                        self.ivars().ring_theme.get(),
                        self.ivars()
                            .looks
                            .borrow()
                            .iter()
                            .find(|(pane, _)| *pane == id),
                    ) {
                        head.set(look.clone(), &theme, card::corner_pt());
                    }
                    // Over its pane, under the dividers' handles.
                    self.addSubview_positioned_relativeTo(
                        &head,
                        NSWindowOrderingMode::Above,
                        self.pane(id).as_deref().map(|pane| pane as &NSView),
                    );
                    heads.push(head.clone());
                    head
                }
            };
            head.setFrame(NSRect::new(
                NSPoint::new(rect.x, rect.y - card::HEAD_PT),
                NSSize::new(rect.width, card::HEAD_PT),
            ));
            // A floating card's headers are round only at the card's top corners; every other
            // header, a card of its own, at both.
            let corners = self
                .ivars()
                .head_corners
                .borrow()
                .iter()
                .find(|(pane, _)| *pane == id)
                .map_or((true, true), |(_, corners)| *corners);
            head.set_corners(corners);
            // The pane's own metrics say where its text starts: they carry the gutter and what
            // its corners take, told with its frame.
            if let Some(margins) = self.pane(id).and_then(|pane| pane.side_margins()) {
                head.set_margins(margins);
            }
        }
    }

    /// Gives each header what it shows (`TerminalTab::refresh_heads`): `looks` by pane.
    pub(crate) fn set_head_looks(&self, looks: Vec<(u64, HeadLook)>, theme: &Theme) {
        let radius = card::corner_pt();
        for head in self.ivars().heads.borrow().iter() {
            if let Some((_, look)) = looks.iter().find(|(id, _)| *id == head.pane()) {
                head.set(look.clone(), theme, radius);
            }
        }
        // The capsules say what the headers would: a title that changed there changes here, and
        // the width with it.
        self.ivars().looks.replace(looks);
        self.ivars().ring_theme.set(Some(*theme));
        if !self.ivars().capsules.borrow().is_empty() {
            self.place_capsules();
        }
    }

    /// The capsules of the minimized cards, oldest first, in the row the setting says
    /// ([`capsule::row`]), and the strip's floor under lines. A capsule is made when a card is
    /// minimized and dropped when it opens; each shows its pane's last look.
    fn place_capsules(&self) {
        let iv = self.ivars();
        let wanted: Vec<u64> = iv
            .cards
            .borrow()
            .iter()
            .filter(|card| card.minimized)
            .map(|card| card.tree.leaves()[0])
            .collect();
        {
            let mut views = iv.capsules.borrow_mut();
            views.retain(|view| {
                let keep = wanted.contains(&view.pane());
                if !keep {
                    view.removeFromSuperview();
                }
                keep
            });
            for id in &wanted {
                if !views.iter().any(|view| view.pane() == *id) {
                    let view = Capsule::new(self.mtm(), *id);
                    self.addSubview(&view);
                    views.push(view);
                }
            }
            views.sort_by_key(|view| wanted.iter().position(|id| *id == view.pane()));
        }
        let views = iv.capsules.borrow();
        let place = self.capsule_place();
        let theme = iv.ring_theme.get();
        if let Some(theme) = &theme {
            let looks = iv.looks.borrow();
            for (index, view) in views.iter().enumerate() {
                if let Some((_, look)) = looks.iter().find(|(id, _)| *id == view.pane()) {
                    view.set(look, theme, place, index + 1 == views.len());
                }
            }
        }
        let widths: Vec<f64> = views.iter().map(|view| view.width()).collect();
        let frames = capsule::row(
            place,
            self.panes_rect(),
            self.carded(),
            card::HEAD_PT,
            &widths,
        );
        for (view, frame) in views.iter().zip(&frames) {
            view.place(NSRect::new(
                NSPoint::new(frame.x, frame.y),
                NSSize::new(frame.width, frame.height),
            ));
        }
        // Under lines the strip is the panes' own ground with a line along its top; under cards
        // it is the window's ground, which stands there already.
        let strip = self.strip_height();
        let floor = strip > 0.0 && self.style() == SplitStyle::Lines;
        iv.floor.setHidden(!floor);
        iv.floor_line.setHidden(!floor);
        if floor {
            let size = self.bounds().size;
            let top = (size.height - strip).max(0.0);
            iv.floor.setFrame(NSRect::new(
                NSPoint::new(0.0, top),
                NSSize::new(size.width, strip),
            ));
            iv.floor_line.setFrame(NSRect::new(
                NSPoint::new(0.0, top),
                NSSize::new(size.width, 1.0 / self.scale()),
            ));
            if let Some(theme) = &theme {
                let tone = |rgb: [u8; 3]| {
                    let [r, g, b] = rgb.map(|byte| f64::from(byte) / 255.0);
                    NSColor::colorWithSRGBRed_green_blue_alpha(r, g, b, 1.0)
                };
                iv.floor.setFillColor(&tone(theme.background_srgb()));
                iv.floor_line.setFillColor(&tone(theme.separator_srgb()));
            }
        }
    }

    /// The card `pane` is, in this container's space: its frame and, while it stands in view,
    /// the header over it — the card's shadow and the arrangement's outlines take it whole.
    pub(crate) fn card_frame(&self, pane: &TerminalPane) -> NSRect {
        let frame = pane.frame();
        let headed = self
            .ivars()
            .heads
            .borrow()
            .iter()
            .any(|head| head.pane() == pane.id() && !head.isHidden());
        if headed {
            NSRect::new(
                NSPoint::new(frame.origin.x, frame.origin.y - card::HEAD_PT),
                NSSize::new(frame.size.width, frame.size.height + card::HEAD_PT),
            )
        } else {
            frame
        }
    }

    /// The radii of the bottom corners of a pane at `rect` (points, in this
    /// container's coordinates): [`bottom_corners`] with the window's corner
    /// now ([`card::window_corner_pt`], square in full screen) and the card's.
    fn corners_of(&self, rect: Rect, carded: bool) -> (f64, f64) {
        bottom_corners(
            rect,
            self.full_rect(),
            carded,
            self.beside_tabs(),
            card::window_corner_pt(self.window().as_deref()),
            card::corner_pt(),
        )
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
    fn every_pane_is_a_card_under_cards() {
        // One pane or several, the gap is the card gap at the scale, and none
        // at the top: the cards start right under the title row.
        let cards = spaced(2.0, SplitStyle::Cards, false);
        assert_ne!(cards, Spacing::DIVIDED);
        assert_eq!(cards.between_px(), GAP_PT * 2.0);
        assert_eq!(cards.around_px(), GAP_PT * 2.0);
        assert_eq!(cards.top_px(), 0.0);
        assert_eq!(
            spaced(1.0, SplitStyle::Cards, false),
            Spacing::gapped(GAP_PT, GAP_PT, 1.0).with_top(0.0, 1.0)
        );
    }

    #[test]
    fn beside_the_side_column_the_cards_gap_is_the_columns_own_margin() {
        // The tabs stand along the left edge: the first card starts at the container's edge,
        // the column's margin being the gap there; the other three edges keep the card gap,
        // the top included (no title row sits above the cards).
        let beside = spaced(2.0, SplitStyle::Cards, true);
        assert_eq!(beside.left_px(), 0.0);
        assert_eq!(beside.top_px(), GAP_PT * 2.0);
        assert_eq!(beside.around_px(), GAP_PT * 2.0);
        assert_eq!(beside.between_px(), GAP_PT * 2.0);
        assert_eq!(
            beside,
            Spacing::gapped(GAP_PT, GAP_PT, 2.0).with_left(0.0, 2.0)
        );
        // With the tabs in the title row nothing changes: no top margin, the other three kept.
        let above = spaced(2.0, SplitStyle::Cards, false);
        assert_eq!((above.top_px(), above.left_px()), (0.0, GAP_PT * 2.0));
        // Lines touch, whatever the tabs do.
        assert_eq!(spaced(2.0, SplitStyle::Lines, true), Spacing::DIVIDED);
    }

    #[test]
    fn inside_a_floating_card_panes_touch_with_a_line_a_header_and_no_margin() {
        // The card's edge is its own frame: no margin round its panes, whatever the split style,
        // and its panes meet on one line, as lines' do, each under its header.
        let inside = card_spacing(2.0);
        assert_eq!(inside.around_px(), 0.0);
        assert_eq!(inside, Spacing::DIVIDED.with_head(card::HEAD_PT, 2.0));
        assert_ne!(inside.between_px(), GAP_PT * 2.0);
    }

    #[test]
    fn a_lined_pane_at_the_windows_top_right_has_the_windows_corner_there_beside_the_tabs() {
        let bounds = Rect::new(0.0, 0.0, 900.0, 600.0);
        let right = Rect::new(450.0, 0.0, 450.0, 600.0);
        let left = Rect::new(0.0, 0.0, 449.0, 600.0);
        let lower = Rect::new(450.0, 300.0, 450.0, 300.0);
        // Tabs in the side column: the container's top edge is the window's.
        assert_eq!(top_corners(right, bounds, true, 12.0), (0.0, 12.0));
        // The left corner is the column's; a pane under another reaches no corner.
        assert_eq!(top_corners(left, bounds, true, 12.0), (0.0, 0.0));
        assert_eq!(top_corners(lower, bounds, true, 12.0), (0.0, 0.0));
        // Tabs in the title row: the container starts under it, every top corner is square.
        assert_eq!(top_corners(right, bounds, false, 12.0), (0.0, 0.0));
    }

    #[test]
    fn a_dividers_cursor_stops_where_a_card_stands_over_it() {
        // A side-by-side split's line runs top to bottom: a card across its middle leaves the
        // part above it and the part below it.
        let line = Rect::new(400.0, 0.0, 8.0, 500.0);
        let card = Rect::new(300.0, 100.0, 300.0, 200.0);
        assert_eq!(
            uncovered(line, Axis::Horizontal, &[card]),
            vec![
                Rect::new(400.0, 0.0, 8.0, 100.0),
                Rect::new(400.0, 300.0, 8.0, 200.0)
            ]
        );
        // A card beside the line leaves it whole; one over all of it leaves nothing.
        let beside = Rect::new(500.0, 100.0, 100.0, 100.0);
        assert_eq!(uncovered(line, Axis::Horizontal, &[beside]), vec![line]);
        let over = Rect::new(300.0, -10.0, 300.0, 600.0);
        assert!(uncovered(line, Axis::Horizontal, &[over]).is_empty());
        // A line across, under a card: the parts left and right of it.
        let across = Rect::new(0.0, 250.0, 800.0, 8.0);
        assert_eq!(
            uncovered(across, Axis::Vertical, &[card]),
            vec![
                Rect::new(0.0, 250.0, 300.0, 8.0),
                Rect::new(600.0, 250.0, 200.0, 8.0)
            ]
        );
    }

    #[test]
    fn a_split_card_is_round_only_at_its_own_corners() {
        let head = card::HEAD_PT;
        let card = Rect::new(100.0, 50.0, 401.0, 300.0 + head);
        // Side by side: the left pane has the card's left corners, the right one its right ones.
        let left = Rect::new(100.0, 50.0 + head, 200.0, 300.0);
        let right = Rect::new(301.0, 50.0 + head, 200.0, 300.0);
        assert_eq!(
            card_corners(left, card),
            CardCorners {
                top: (true, false),
                bottom: (true, false)
            }
        );
        assert_eq!(
            card_corners(right, card),
            CardCorners {
                top: (false, true),
                bottom: (false, true)
            }
        );
        // One above the other: the top pane's header has the top corners and no bottom ones.
        let tall = Rect::new(100.0, 50.0, 401.0, 2.0 * (150.0 + head) + 1.0);
        let upper = Rect::new(100.0, 50.0 + head, 401.0, 150.0);
        let lower = Rect::new(100.0, 50.0 + head + 150.0 + 1.0 + head, 401.0, 150.0);
        assert_eq!(
            card_corners(upper, tall),
            CardCorners {
                top: (true, true),
                bottom: (false, false)
            }
        );
        assert_eq!(
            card_corners(lower, tall),
            CardCorners {
                top: (false, false),
                bottom: (true, true)
            }
        );
        // A card of one pane is round all round.
        let alone = Rect::new(100.0, 50.0 + head, 401.0, 300.0);
        assert_eq!(
            card_corners(alone, card),
            CardCorners {
                top: (true, true),
                bottom: (true, true)
            }
        );
    }

    #[test]
    fn the_cursor_rects_of_a_card_are_where_the_pointer_grips_it() {
        // The middle of each grip's rectangle grips exactly that grip, so the cursor over a
        // border says what a press there does.
        let card = Rect::new(100.0, 80.0, 300.0, 200.0);
        let rects = grip_rects(card, GRIP_PT);
        assert_eq!(rects.len(), 8);
        for (grip, rect) in rects {
            let middle = (
                rect.origin.x + rect.size.width / 2.0,
                rect.origin.y + rect.size.height / 2.0,
            );
            assert_eq!(
                floating::grip_at(card, middle, GRIP_PT),
                Some(grip),
                "{grip:?}"
            );
        }
        // A card narrower than two grips has no rectangle with a negative size.
        for (_, rect) in grip_rects(Rect::new(0.0, 0.0, 4.0, 4.0), GRIP_PT) {
            assert!(rect.size.width >= 0.0 && rect.size.height >= 0.0);
        }
    }

    #[test]
    fn lines_let_the_panes_touch() {
        // Under `lines` there are no cards: every pane count is laid out with
        // the one-pixel divider, the layout from before the cards.
        assert_eq!(spaced(2.0, SplitStyle::Lines, false), Spacing::DIVIDED);
    }

    /// The corners the container would tell each pane of `tree` laid out in a
    /// 1000×600 pt area at 2× with `spacing`, by pane id.
    fn corners(tree: &Tree, spacing: Spacing, carded: bool) -> Vec<(u64, (f64, f64))> {
        let bounds = Rect::new(0.0, 0.0, 1000.0, 600.0);
        tree.layout_spaced(bounds, 2.0, spacing)
            .panes
            .into_iter()
            .map(|(id, rect)| (id, bottom_corners(rect, bounds, carded, false, 20.5, 12.5)))
            .collect()
    }

    #[test]
    fn a_lone_pane_is_laid_out_like_a_zoomed_one() {
        // Under lines both cover the area and have the window's corners.
        assert_eq!(
            corners(&Tree::Leaf(1), Spacing::DIVIDED, false),
            [(1, (20.5, 20.5))]
        );
        let bounds = Rect::new(0.0, 0.0, 1000.0, 600.0);
        let mut tree = Tree::Leaf(1);
        assert!(tree.split(1, Axis::Horizontal, 2));
        let lines = spaced(2.0, SplitStyle::Lines, false);
        let layout = tree.layout_zoomed_spaced(bounds, 2.0, Some(2), lines);
        let (_, rect) = layout.panes[0];
        assert_eq!(rect, bounds);
        assert_eq!(
            bottom_corners(rect, bounds, false, false, 20.5, 12.5),
            (20.5, 20.5)
        );
        // Under cards both are a card on the ground, in the same place.
        let cards = spaced(2.0, SplitStyle::Cards, false);
        let zoomed = tree.layout_zoomed_spaced(bounds, 2.0, Some(2), cards);
        let alone = Tree::Leaf(2).layout_spaced(bounds, 2.0, cards);
        assert_eq!(zoomed.panes, alone.panes);
        assert_ne!(zoomed.panes[0].1, bounds);
    }

    #[test]
    fn under_lines_only_the_panes_on_the_windows_bottom_corners_have_them() {
        let mut tree = Tree::Leaf(1);
        assert!(tree.split(1, Axis::Horizontal, 2));
        // Side by side: each pane owns the corner on its own side.
        assert_eq!(
            corners(&tree, Spacing::DIVIDED, false),
            [(1, (20.5, 0.0)), (2, (0.0, 20.5))]
        );
        // Stacked: the upper pane is on no bottom corner, the lower on both.
        let mut tree = Tree::Leaf(1);
        assert!(tree.split(1, Axis::Vertical, 2));
        assert_eq!(
            corners(&tree, Spacing::DIVIDED, false),
            [(1, (0.0, 0.0)), (2, (20.5, 20.5))]
        );
        // Four: the grid's bottom row owns the corners, one each.
        assert!(tree.split(1, Axis::Horizontal, 3));
        assert!(tree.split(2, Axis::Horizontal, 4));
        assert_eq!(
            corners(&tree, Spacing::DIVIDED, false),
            [
                (1, (0.0, 0.0)),
                (3, (0.0, 0.0)),
                (2, (20.5, 0.0)),
                (4, (0.0, 20.5)),
            ]
        );
    }

    #[test]
    fn a_card_has_the_cards_corner_on_both_sides_wherever_it_stands() {
        let mut tree = Tree::Leaf(1);
        assert!(tree.split(1, Axis::Horizontal, 2));
        let cards = spaced(2.0, SplitStyle::Cards, false);
        assert_eq!(
            corners(&tree, cards, true),
            [(1, (12.5, 12.5)), (2, (12.5, 12.5))]
        );
        // The same layout not drawn as cards touches no window corner: the
        // gap around keeps every pane off them.
        assert_eq!(
            corners(&tree, cards, false),
            [(1, (0.0, 0.0)), (2, (0.0, 0.0))]
        );
    }

    #[test]
    fn beside_the_side_column_no_pane_owns_the_windows_left_corner() {
        // The container's left edge is the column's: the column's foot keeps the window's
        // corner there. A pane on the right edge keeps its own.
        let bounds = Rect::new(0.0, 0.0, 1000.0, 600.0);
        let lone = bounds;
        assert_eq!(
            bottom_corners(lone, bounds, false, true, 20.5, 12.5),
            (0.0, 20.5)
        );
        assert_eq!(
            bottom_corners(lone, bounds, false, false, 20.5, 12.5),
            (20.5, 20.5)
        );
        // Side by side under lines: the left pane has no window corner now, the right pane
        // still has.
        let mut tree = Tree::Leaf(1);
        assert!(tree.split(1, Axis::Horizontal, 2));
        let layout = tree.layout_spaced(bounds, 2.0, Spacing::DIVIDED);
        let corners: Vec<_> = layout
            .panes
            .iter()
            .map(|(id, rect)| (*id, bottom_corners(*rect, bounds, false, true, 20.5, 12.5)))
            .collect();
        assert_eq!(corners, [(1, (0.0, 0.0)), (2, (0.0, 20.5))]);
        // A card is a card wherever the tabs are.
        assert_eq!(
            bottom_corners(lone, bounds, true, true, 20.5, 12.5),
            (12.5, 12.5)
        );
    }

    #[test]
    fn a_pane_a_pixel_off_an_edge_is_still_on_it_and_a_gap_is_not() {
        let bounds = Rect::new(0.0, 0.0, 1000.0, 600.0);
        let near = Rect::new(0.25, 0.0, 400.0, 599.75);
        assert_eq!(
            bottom_corners(near, bounds, false, false, 20.5, 12.5),
            (20.5, 0.0)
        );
        let gapped = Rect::new(8.0, 0.0, 400.0, 592.0);
        assert_eq!(
            bottom_corners(gapped, bounds, false, false, 20.5, 12.5),
            (0.0, 0.0)
        );
        // A full screen window has no curve: nothing to keep clear of.
        assert_eq!(
            bottom_corners(bounds, bounds, false, false, 0.0, 12.5),
            (0.0, 0.0)
        );
    }

    #[test]
    fn a_pane_standing_over_the_strip_is_off_the_windows_bottom_edge() {
        // The container less the strip is the splits' room: a pane filling it stands 34 points
        // above the window's bottom, so the window's curve is not its corner — measured against
        // the whole container, as `corners_of` does.
        let full = Rect::new(0.0, 0.0, 1000.0, 600.0);
        let above = Rect::new(0.0, 0.0, 1000.0, 600.0 - capsule::STRIP);
        assert_eq!(
            bottom_corners(above, full, false, false, 20.5, 12.5),
            (0.0, 0.0)
        );
        // Without a strip the same pane is on the edge and takes it.
        assert_eq!(
            bottom_corners(full, full, false, false, 20.5, 12.5),
            (20.5, 20.5)
        );
    }

    /// The text of a function of this file, to its closing brace at the margin of the impl.
    fn body_of(name: &str) -> &'static str {
        let source = include_str!("split_view.rs");
        let at = source.find(name).expect(name);
        let end = source[at..].find("\n    }\n").expect("its end");
        &source[at..at + end]
    }

    #[test]
    fn the_strips_height_comes_off_every_layout_but_not_off_the_windows_corners() {
        assert!(body_of("fn bounds_rect(").contains("self.strip_height()"));
        assert!(body_of("fn corners_of(").contains("self.full_rect()"));
        assert!(!body_of("fn corners_of(").contains("self.bounds_rect()"));
    }

    #[test]
    fn the_splits_slide_only_when_the_strip_comes_or_goes() {
        let body = body_of("pub(crate) fn set_cards(");
        assert!(body.contains("self.strip_of(&cards) != self.strip_height()"));
        assert!(body.contains("self.slide(&before, None, card::MOVE_SECS)"));
        // A pane that is hidden now draws no frames, and one that is opened asks for one.
        assert!(body.contains("self.apply_visibility("));
        // Opening a card from its capsule takes the same road.
        assert!(body_of("pub(crate) fn reveal(").contains("self.set_cards(layers.cards)"));
    }

    #[test]
    fn the_capsules_are_placed_with_every_layout_and_stand_over_the_grips() {
        let layout = body_of("pub(crate) fn layout_panes(");
        let placed = layout
            .find("self.place_capsules()")
            .expect("capsules placed");
        let stacked = layout.rfind("self.restack()").expect("restacked");
        assert!(
            placed < stacked,
            "the capsules exist before the stack is put in order"
        );
        let restack = body_of("fn restack(");
        let grips = restack.find("iv.grips.borrow()").expect("the grips");
        let capsules = restack.find("iv.capsules.borrow()").expect("the capsules");
        assert!(
            grips < capsules,
            "a capsule at the corner stands over the grips"
        );
    }

    #[test]
    fn the_focus_frame_stands_over_its_own_card_and_under_the_cards_in_front() {
        let restack = body_of("fn restack(");
        // The slot is where the framed card's own views end, not "over everything".
        assert!(restack.contains("ring_slot = Some(wanted.len())"));
        assert!(!restack.contains("ring_over"));
    }

    #[test]
    fn a_click_that_moved_no_card_is_not_a_layout_change() {
        let end = body_of("pub(crate) fn end_card_drag(");
        assert!(end.contains("drag.moved"));
        let drag = body_of("pub(crate) fn drag_card(");
        assert!(drag.contains("moved: true"));
    }
}
