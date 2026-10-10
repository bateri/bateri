//! Split layout: the **pure** binary tree that carries a tab's panes. A leaf is a
//! pane identity (`TerminalPane::id`), a node is an axis and a ratio. Splitting, closing and the
//! frame computation are each a tree operation; the AppKit part (`split_view`) only applies these
//! frames to the panes and paints the dividers.
//!
//! It does not see AppKit and has its own tests (the `quote`/`upload`/`zoom` precedent).
//! Coordinates are **top-down** (the container is `isFlipped`): the second leaf of "split down" is
//! below, so no sign needs flipping.
//!
//! Navigation (order and direction), resizing (keyboard step and divider drag), equalizing and
//! zooming are tree operations too. The **minimum pane** is given to the
//! tree as one size per leaf (`min`): the point-size delta is per pane, and so is the cell; the
//! limit's source is the pane itself.
//!
//! **Moving a pane** is pure here too: [`Tree::swap`], [`Tree::insert_beside`] and
//! [`Tree::insert_at_edge`] change the tree; [`Tree::plan_beside`] and [`Tree::plan_edge`] answer
//! "if this subtree were let go there, where would it land and what would the tree become" with
//! the room made for it, [`Tree::fitting_edges`] which window edges would take it, and
//! [`Layout::zone_at`] which of those a pointer is asking for. A drag, a menu command and a drop
//! on a tab all call the same plan, so what the preview shows is what the drop does.
//!
//! **Spacing** ([`Spacing`]): the space between panes and around them is a parameter of the
//! frame computation, in device pixels. [`Spacing::DIVIDED`] is the one-pixel divider with no
//! margin and gives exactly the frames the ungapped functions always gave.

/// The split's axis.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    /// Side by side — Split Right (⌘D): the second leaf is on the right.
    Horizontal,
    /// Stacked — Split Down (⇧⌘D): the second leaf is below.
    Vertical,
}

impl Axis {
    /// The other axis.
    fn other(self) -> Self {
        match self {
            Self::Horizontal => Self::Vertical,
            Self::Vertical => Self::Horizontal,
        }
    }
}

/// The direction of navigation and resizing (⌥⌘ / ⌃⌘ + arrow), and the side of a pane or of the
/// window a pane is let go on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

impl Direction {
    /// The direction from a menu item's `tag` (`menu`'s Select/Resize Split ▸ items); an unknown
    /// `tag` is `None`.
    pub fn from_tag(tag: isize) -> Option<Self> {
        match tag {
            0 => Some(Self::Left),
            1 => Some(Self::Right),
            2 => Some(Self::Up),
            3 => Some(Self::Down),
            _ => None,
        }
    }

    /// The axis of the split whose divider this direction moves: left/right is the divider of a
    /// side-by-side split, up/down that of a stacked split.
    fn axis(self) -> Axis {
        match self {
            Self::Left | Self::Right => Axis::Horizontal,
            Self::Up | Self::Down => Axis::Vertical,
        }
    }

    /// Whether the coordinate grows in this direction (right, down — in the top-down layout).
    fn forward(self) -> bool {
        matches!(self, Self::Right | Self::Down)
    }
}

/// A size, in points — the minimum pane's measure.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Size {
    pub(crate) width: f64,
    pub(crate) height: f64,
}

impl Size {
    pub const fn new(width: f64, height: f64) -> Self {
        Self { width, height }
    }

    fn along(self, axis: Axis) -> f64 {
        match axis {
            Axis::Horizontal => self.width,
            Axis::Vertical => self.height,
        }
    }
}

/// A rectangle, in points, top-down.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub const fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// The centre point.
    pub fn center(self) -> (f64, f64) {
        (self.x + self.width / 2.0, self.y + self.height / 2.0)
    }

    /// The start and length along the axis.
    fn span(self, axis: Axis) -> (f64, f64) {
        match axis {
            Axis::Horizontal => (self.x, self.width),
            Axis::Vertical => (self.y, self.height),
        }
    }

    fn scaled(self, factor: f64) -> Self {
        Self::new(
            self.x * factor,
            self.y * factor,
            self.width * factor,
            self.height * factor,
        )
    }
}

/// The divider's thickness, in **device pixels**: one. In points `1 / scale` — half a point on
/// Retina. Not measured, a design constant ("the
/// divider is one pixel"); the same weight as the dock's hairlines.
const DIVIDER_PX: f64 = 1.0;

/// The space the frame computation leaves between panes and around them, in **device pixels**
/// (every boundary is computed in pixels, so a gap given in points is rounded to whole pixels
/// once, by [`Spacing::gapped`], and never again).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spacing {
    between: f64,
    around: f64,
    /// The margin at the area's top edge: `around` unless [`Spacing::with_top`] said otherwise.
    top: f64,
}

impl Spacing {
    /// A one-pixel divider and no margin: the layout of a tab whose panes touch.
    pub const DIVIDED: Self = Self {
        between: DIVIDER_PX,
        around: 0.0,
        top: 0.0,
    };

    /// A gap of `between` points between panes and `around` points between the panes and the
    /// area's edge, each rounded to whole device pixels at `scale`.
    pub fn gapped(between: f64, around: f64, scale: f64) -> Self {
        let pixels = |points: f64| (points * scale).round().max(0.0);
        Self {
            between: pixels(between),
            around: pixels(around),
            top: pixels(around),
        }
    }

    /// The same spacing with `top` points at the area's top edge instead of the margin around:
    /// a split tab's cards start right under the title row, whose own height already sets them
    /// off from the tabs — a full margin there read as the content drifting away from its tabs.
    pub fn with_top(self, top: f64, scale: f64) -> Self {
        Self {
            top: (top * scale).round().max(0.0),
            ..self
        }
    }

    /// The gap between two panes, in device pixels.
    pub fn between_px(self) -> f64 {
        self.between
    }

    /// The margin around the panes, in device pixels.
    pub fn around_px(self) -> f64 {
        self.around
    }

    /// The margin at the area's top edge, in device pixels.
    pub fn top_px(self) -> f64 {
        self.top
    }

    /// `rect` (whole pixels) less this spacing's margins: where the root node is laid out.
    fn inside(self, rect: Rect) -> Rect {
        Rect::new(
            rect.x + self.around,
            rect.y + self.top,
            (rect.width - 2.0 * self.around).max(0.0),
            (rect.height - self.around - self.top).max(0.0),
        )
    }
}

/// How much of the room a pane let go beside another pane asks for: half, the plain split. A
/// design constant, not measured; the room made when half does not fit is [`solve_share`]'s.
pub const PANE_EDGE_SHARE: f64 = 0.5;

/// How much of the window a pane let go at the window's edge asks for: two fifths. Half would
/// make a full-height column as wide as everything else put together; two fifths reads as an
/// addition to the layout and leaves the existing panes the larger part. A design constant.
pub const WINDOW_EDGE_SHARE: f64 = 0.4;

/// How deep into a pane the "swap" middle begins, as a fraction of the pane's extent from its
/// nearest edge: a pointer more than 28% in from every edge is over the middle, which leaves the
/// edge bands a comfortable 28% each and the middle 44% × 44% of the pane. A design constant.
pub const SWAP_CORE: f64 = 0.28;

/// The width of the strip along the window's edge that means "a full-length column or row", in
/// points: wide enough to hit without aiming, narrow enough that the panes' outer edges stay
/// ordinary pane edges. A design constant.
pub const WINDOW_EDGE_STRIP: f64 = 18.0;

/// Splits a leaf's frame in two along `axis`: the first half, the divider and the second half —
/// in pixels, with the divider subtracted. The ratio is the first half's share.
///
/// The boundary is snapped to a **whole pixel**: a pane sitting on a half pixel gets a fractional
/// drawable and its text blurs (the tab bar's symptom). If the input is already on
/// whole pixels, all three outputs are whole pixels too and tile the input with no gap and no
/// overlap.
fn halves_px(rect: Rect, axis: Axis, ratio: f64, between: f64) -> (Rect, Rect, Rect) {
    let span = match axis {
        Axis::Horizontal => rect.width,
        Axis::Vertical => rect.height,
    };
    let available = (span - between).max(0.0);
    let first = (available * ratio).round().clamp(0.0, available);
    let second = available - first;
    match axis {
        Axis::Horizontal => (
            Rect::new(rect.x, rect.y, first, rect.height),
            Rect::new(rect.x + first, rect.y, between, rect.height),
            Rect::new(rect.x + first + between, rect.y, second, rect.height),
        ),
        Axis::Vertical => (
            Rect::new(rect.x, rect.y, rect.width, first),
            Rect::new(rect.x, rect.y + first, rect.width, between),
            Rect::new(rect.x, rect.y + first + between, rect.width, second),
        ),
    }
}

/// What a pane that already stands at `to` must undergo to **look** as if it stood at `from`: a
/// scale about its centre, then a shift of that centre. The pane is laid out once, at its final
/// frame (its program is resized once), and a slide plays from this transform back to none —
/// the frames in between are never laid out. Linear in `from`'s edges, so the slide moves every
/// edge at the same pace.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Slide {
    pub scale_x: f64,
    pub scale_y: f64,
    /// The shift of the centre, in points, in the frames' own (top-down) direction.
    pub dx: f64,
    pub dy: f64,
}

impl Slide {
    /// No transform: the pane looks as it stands.
    pub const NONE: Self = Self {
        scale_x: 1.0,
        scale_y: 1.0,
        dx: 0.0,
        dy: 0.0,
    };
}

/// The [`Slide`] from `from` to `to`. A `to` with no extent along an axis keeps scale one there:
/// nothing stands at zero size at rest.
pub fn slide(from: Rect, to: Rect) -> Slide {
    let ratio = |was: f64, is: f64| if is > 0.0 { was / is } else { 1.0 };
    let (from_x, from_y) = from.center();
    let (to_x, to_y) = to.center();
    Slide {
        scale_x: ratio(from.width, to.width),
        scale_y: ratio(from.height, to.height),
        dx: from_x - to_x,
        dy: from_y - to_y,
    }
}

/// Where a pane that did not exist begins to grow: a line along its far edge on the axis it was
/// split on (the right edge of a pane split side by side, the bottom of one split above and below),
/// as long as the pane is across. A new pane is always the second of its split.
pub fn sliver(to: Rect, axis: Axis) -> Rect {
    match axis {
        Axis::Horizontal => Rect::new(to.x + to.width, to.y, 0.0, to.height),
        Axis::Vertical => Rect::new(to.x, to.y + to.height, to.width, 0.0),
    }
}

/// From points to pixels, rounded to whole pixels.
fn snap(rect: Rect, scale: f64) -> Rect {
    let px = rect.scaled(scale);
    Rect::new(
        px.x.round(),
        px.y.round(),
        px.width.round(),
        px.height.round(),
    )
}

/// `rect` pulled in by `margin` on every side (a side cannot go below nothing).
/// The split tree.
#[derive(Clone, Debug, PartialEq)]
pub enum Tree {
    /// A pane's identity.
    Leaf(u64),
    /// Two subtrees, side by side or stacked along `axis`; `ratio` is the first one's share (after
    /// the divider is subtracted).
    Split {
        axis: Axis,
        ratio: f64,
        first: Box<Tree>,
        second: Box<Tree>,
    },
}

/// The result of removing a leaf ([`Tree::remove`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Removal {
    /// The leaf is gone, its sibling was pulled up; focus should move to `focus`.
    Removed { focus: u64 },
    /// The tree's only leaf: not removed — closing the last pane means closing the tab, and that
    /// decision is the caller's.
    Last,
    /// The leaf is not in the tree.
    Missing,
}

/// The tree turned into frames ([`Tree::layout`]), in points.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Layout {
    /// Pane identity and its frame, in tree order.
    pub panes: Vec<(u64, Rect)>,
    /// Dividers, in the tree's **in-order** arrangement (the first subtree's, the node's, the
    /// second's): [`Tree::drag`]'s index is this order.
    pub dividers: Vec<Divider>,
}

/// A divider: its line and the axis of the split it divides (a side-by-side split's divider is a
/// vertical line, dragged horizontally).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Divider {
    pub rect: Rect,
    pub axis: Axis,
}

/// The floating-point comparison's tolerance: frames are points snapped to device pixels, so any
/// inequality is just rounding noise.
const EPSILON: f64 = 1e-6;

/// The overlapping length of `[a, a + a_len)` and `[b, b + b_len)` (zero if negative).
fn overlap(a: f64, a_len: f64, b: f64, b_len: f64) -> f64 {
    ((a + a_len).min(b + b_len) - a.max(b)).max(0.0)
}

impl Layout {
    /// `from`'s neighbour in `direction` (⌥⌘ + arrow): the pane beyond that edge, closest to the
    /// edge and overlapping most on the perpendicular axis; on a tie, the one earlier in tree
    /// order. `None` at the edge (no pane beyond it) or if `from` is not in the layout.
    pub fn neighbour(&self, from: u64, direction: Direction) -> Option<u64> {
        let (_, f) = self.panes.iter().find(|(id, _)| *id == from)?;
        let mut best: Option<(u64, f64, f64)> = None;
        for (id, r) in &self.panes {
            if *id == from {
                continue;
            }
            let (gap, shared) = match direction {
                Direction::Right => (r.x - (f.x + f.width), overlap(f.y, f.height, r.y, r.height)),
                Direction::Left => (f.x - (r.x + r.width), overlap(f.y, f.height, r.y, r.height)),
                Direction::Down => (r.y - (f.y + f.height), overlap(f.x, f.width, r.x, r.width)),
                Direction::Up => (f.y - (r.y + r.height), overlap(f.x, f.width, r.x, r.width)),
            };
            if gap < -EPSILON || shared <= EPSILON {
                continue;
            }
            let better = best.is_none_or(|(_, best_gap, best_shared)| {
                gap < best_gap - EPSILON
                    || ((gap - best_gap).abs() <= EPSILON && shared > best_shared + EPSILON)
            });
            if better {
                best = Some((*id, gap, shared));
            }
        }
        best.map(|(id, _, _)| id)
    }
}

impl Tree {
    /// The order of pane identities — depth first, first subtree first
    /// (left to right, top to bottom).
    pub fn leaves(&self) -> Vec<u64> {
        let mut out = Vec::new();
        self.collect(&mut out);
        out
    }

    fn collect(&self, out: &mut Vec<u64>) {
        match self {
            Tree::Leaf(id) => out.push(*id),
            Tree::Split { first, second, .. } => {
                first.collect(out);
                second.collect(out);
            }
        }
    }

    fn first_leaf(&self) -> u64 {
        match self {
            Tree::Leaf(id) => *id,
            Tree::Split { first, .. } => first.first_leaf(),
        }
    }

    fn last_leaf(&self) -> u64 {
        match self {
            Tree::Leaf(id) => *id,
            Tree::Split { second, .. } => second.last_leaf(),
        }
    }

    /// The pane after (`forward`) or before `from`, in tree order and cyclic (⌘] / ⌘[). `None`
    /// with a single pane or if `from` is not in the tree.
    pub fn cycle(&self, from: u64, forward: bool) -> Option<u64> {
        let leaves = self.leaves();
        if leaves.len() < 2 {
            return None;
        }
        let index = leaves.iter().position(|id| *id == from)?;
        let next = if forward {
            (index + 1) % leaves.len()
        } else {
            (index + leaves.len() - 1) % leaves.len()
        };
        Some(leaves[next])
    }

    /// Splits the `target` leaf in two along `axis`: the old pane in the first half (left or top),
    /// `new` in the second, equal area.
    /// If the leaf is missing, `false` and the tree does not change.
    pub fn split(&mut self, target: u64, axis: Axis, new: u64) -> bool {
        match self {
            Tree::Leaf(id) if *id == target => {
                *self = Tree::Split {
                    axis,
                    ratio: 0.5,
                    first: Box::new(Tree::Leaf(target)),
                    second: Box::new(Tree::Leaf(new)),
                };
                true
            }
            Tree::Leaf(_) => false,
            Tree::Split { first, second, .. } => {
                first.split(target, axis, new) || second.split(target, axis, new)
            }
        }
    }

    /// Removes the `target` leaf; its sibling takes the parent's place and gets the whole area.
    ///
    /// The neighbour that receives focus is the sibling's **adjacent** leaf: if the removed one
    /// was in the first half, the sibling's first leaf; if in the second, the sibling's last leaf
    /// — in both cases the pane touching the closed pane's divider.
    pub fn remove(&mut self, target: u64) -> Removal {
        match self {
            Tree::Leaf(id) if *id == target => Removal::Last,
            Tree::Leaf(_) => Removal::Missing,
            Tree::Split { .. } => match remove_in(self, target) {
                Some(focus) => Removal::Removed { focus },
                None => Removal::Missing,
            },
        }
    }

    /// Lays the tree out in `bounds`: every pane's frame and the dividers.
    /// `scale` is the window's scale; boundaries snap to device pixels ([`halves_px`]), so the
    /// frames together with the dividers tile `bounds` with no gap and no overlap.
    pub fn layout(&self, bounds: Rect, scale: f64) -> Layout {
        self.layout_spaced(bounds, scale, Spacing::DIVIDED)
    }

    /// [`Tree::layout`] with the space `spacing` leaves around the panes and between them.
    /// [`Layout::dividers`] are then the gaps themselves — the strips between two panes, full
    /// across — so a handle placed on one sits inside the gap. Frames, gaps and the margin tile
    /// `bounds` with no overlap, every edge on a device pixel.
    pub fn layout_spaced(&self, bounds: Rect, scale: f64, spacing: Spacing) -> Layout {
        let mut out = Layout::default();
        let root = spacing.inside(snap(bounds, scale));
        self.place(root, spacing.between, &mut out);
        let points = 1.0 / scale;
        for (_, rect) in &mut out.panes {
            *rect = rect.scaled(points);
        }
        for divider in &mut out.dividers {
            divider.rect = divider.rect.scaled(points);
        }
        out
    }

    /// [`Tree::layout`] with a zoomed leaf (⇧⌘↩): if `zoomed` is in the tree, only it, covering
    /// the whole area and without dividers; the other panes are absent from the layout (the
    /// container hides them). `None` or a leaf not in the tree gives the ordinary layout — since
    /// undoing does not change the tree, the old frames come back bit for bit.
    pub fn layout_zoomed(&self, bounds: Rect, scale: f64, zoomed: Option<u64>) -> Layout {
        self.layout_zoomed_spaced(bounds, scale, zoomed, Spacing::DIVIDED)
    }

    /// [`Tree::layout_zoomed`] with the space `spacing` leaves when nothing is zoomed. The zoomed
    /// pane covers the whole area whatever the spacing: a margin around the one pane in view
    /// would read as a card, and a zoomed pane is meant to look like a tab of one.
    pub fn layout_zoomed_spaced(
        &self,
        bounds: Rect,
        scale: f64,
        zoomed: Option<u64>,
        spacing: Spacing,
    ) -> Layout {
        match zoomed {
            Some(id) if self.leaves().contains(&id) => Layout {
                panes: vec![(id, snap(bounds, scale).scaled(1.0 / scale))],
                dividers: Vec::new(),
            },
            _ => self.layout_spaced(bounds, scale, spacing),
        }
    }

    /// ⌃⌘ + arrow: moves the divider of `target`'s **nearest ancestor** on `direction`'s axis by
    /// `step` points in that direction (Ghostty's behaviour: the arrow is the direction the
    /// divider goes, not the growing pane's). Both sides are clamped at the minimum pane limit
    /// ([`place_divider`]). `false` if there is no ancestor on that axis, the divider is already
    /// at the limit, or the leaf is not in the tree.
    pub fn resize(
        &mut self,
        target: u64,
        direction: Direction,
        step: f64,
        bounds: Rect,
        scale: f64,
        min: &dyn Fn(u64) -> Size,
    ) -> bool {
        let room = Room {
            bounds,
            scale,
            spacing: Spacing::DIVIDED,
            min,
        };
        self.resize_within(target, direction, step, &room)
    }

    /// [`Tree::resize`] in a layout with `room`'s spacing: the same arithmetic, so the divider
    /// moves exactly as far as the frames it produces.
    pub fn resize_within(
        &mut self,
        target: u64,
        direction: Direction,
        step: f64,
        room: &Room<'_>,
    ) -> bool {
        let step_px = (step * room.scale).round();
        let delta = if direction.forward() {
            step_px
        } else {
            -step_px
        };
        matches!(
            resize_in(
                self,
                room.root(),
                target,
                direction.axis(),
                delta,
                &room.limits()
            ),
            Found::Done(true)
        )
    }

    /// Divider drag: moves the `index`th divider of [`Layout::dividers`] to `position` (in the
    /// container's coordinates, in points, along the divider's axis), clamping at the limit.
    /// `true` if the position changed.
    pub fn drag(
        &mut self,
        index: usize,
        position: f64,
        bounds: Rect,
        scale: f64,
        min: &dyn Fn(u64) -> Size,
    ) -> bool {
        let room = Room {
            bounds,
            scale,
            spacing: Spacing::DIVIDED,
            min,
        };
        self.drag_within(index, position, &room)
    }

    /// [`Tree::drag`] in a layout with `room`'s spacing.
    pub fn drag_within(&mut self, index: usize, position: f64, room: &Room<'_>) -> bool {
        let mut index = index;
        drag_in(
            self,
            room.root(),
            &mut index,
            position * room.scale,
            &room.limits(),
        ) == Some(true)
    }

    /// ⌃⌘=: each node's ratio comes from its subtrees' pane count **on that axis** — splits
    /// chained on the same axis count their children, a subtree on the other axis is a single
    /// column (or row). Result: all panes on the same axis are equal (in an L layout the left pane
    /// is half width, not a third).
    pub fn equalize(&mut self) {
        if let Tree::Split {
            axis,
            ratio,
            first,
            second,
        } = self
        {
            first.equalize();
            second.equalize();
            let a = first.weight(*axis) as f64;
            let b = second.weight(*axis) as f64;
            *ratio = a / (a + b);
        }
    }

    /// The number of panes standing side by side along `axis` ([`Tree::equalize`]).
    fn weight(&self, axis: Axis) -> usize {
        match self {
            Tree::Split {
                axis: own,
                first,
                second,
                ..
            } if *own == axis => first.weight(axis) + second.weight(axis),
            _ => 1,
        }
    }

    /// This subtree's minimum length along `axis`, in pixels: the length that keeps every leaf at
    /// the minimum pane limit. The inner splits' ratios are taken as **fixed** (resizing moves only
    /// one node), so in a split on the same axis the limit is not the sum, but comes from the
    /// share of the side with the smaller share.
    fn min_px(&self, axis: Axis, limits: &Limits<'_>) -> f64 {
        match self {
            Tree::Leaf(id) => ((limits.min)(*id).along(axis) * limits.scale).ceil(),
            Tree::Split {
                axis: own,
                ratio,
                first,
                second,
            } => {
                let a = first.min_px(axis, limits);
                let b = second.min_px(axis, limits);
                if *own == axis {
                    let r = ratio.clamp(EPSILON, 1.0 - EPSILON);
                    (a / r).max(b / (1.0 - r)).ceil() + limits.spacing.between
                } else {
                    a.max(b)
                }
            }
        }
    }

    fn place(&self, rect: Rect, between: f64, out: &mut Layout) {
        match self {
            Tree::Leaf(id) => out.panes.push((*id, rect)),
            Tree::Split {
                axis,
                ratio,
                first,
                second,
            } => {
                let (a, divider, b) = halves_px(rect, *axis, *ratio, between);
                first.place(a, between, out);
                out.dividers.push(Divider {
                    rect: divider,
                    axis: *axis,
                });
                second.place(b, between, out);
            }
        }
    }
}

/// The resizing limit: the minimum size per leaf (points), the scale and the spacing.
struct Limits<'a> {
    min: &'a dyn Fn(u64) -> Size,
    scale: f64,
    spacing: Spacing,
}

/// The ground every question about a layout stands on: the area (points), the window's scale,
/// the spacing and the minimum pane per leaf. A plan, a resize and a drag in the same `Room` do
/// their arithmetic on the same frames.
pub struct Room<'a> {
    /// The area the panes are laid out in, in points.
    pub bounds: Rect,
    pub scale: f64,
    pub spacing: Spacing,
    /// The minimum size of a leaf's pane, in points — also asked of the leaves of a subtree that
    /// is not in the tree yet.
    pub min: &'a dyn Fn(u64) -> Size,
}

impl<'a> Room<'a> {
    /// The area inside the margin, on whole pixels: where the root node is laid out.
    fn root(&self) -> Rect {
        self.spacing.inside(snap(self.bounds, self.scale))
    }

    fn limits(&self) -> Limits<'a> {
        Limits {
            min: self.min,
            scale: self.scale,
            spacing: self.spacing,
        }
    }
}

/// `want` pixels, rounded and kept within `[low, high]`; `None` when the two limits cross — no
/// length satisfies both sides.
fn within(low: f64, high: f64, want: f64) -> Option<f64> {
    (low <= high).then(|| want.round().clamp(low, high))
}

/// Tries to move a node's divider to where the first half would be `desired` pixels: neither side
/// goes below its minimum length ([`Tree::min_px`]). If the two limits cross (the area is already
/// narrow) nothing changes. `true` if the first half's pixel size changed.
fn place_divider(
    axis: Axis,
    ratio: &mut f64,
    first: &Tree,
    second: &Tree,
    rect: Rect,
    desired: f64,
    limits: &Limits<'_>,
) -> bool {
    let (_, span) = rect.span(axis);
    let available = (span - limits.spacing.between).max(0.0);
    if available <= 0.0 {
        return false;
    }
    let low = first.min_px(axis, limits);
    let high = available - second.min_px(axis, limits);
    let Some(target) = within(low, high, desired) else {
        return false;
    };
    let current = (available * *ratio).round().clamp(0.0, available);
    if target == current {
        return false;
    }
    *ratio = target / available;
    true
}

/// The result of [`Tree::resize`]'s search.
enum Found {
    /// The leaf is not in this subtree.
    Absent,
    /// The leaf is here, but no ancestor on that axis has been found yet.
    Pending,
    /// An ancestor was found; `true` if the divider moved.
    Done(bool),
}

fn resize_in(
    node: &mut Tree,
    rect: Rect,
    target: u64,
    axis: Axis,
    delta: f64,
    limits: &Limits<'_>,
) -> Found {
    match node {
        Tree::Leaf(id) if *id == target => Found::Pending,
        Tree::Leaf(_) => Found::Absent,
        Tree::Split {
            axis: own,
            ratio,
            first,
            second,
        } => {
            let (a, _, b) = halves_px(rect, *own, *ratio, limits.spacing.between);
            let found = match resize_in(first, a, target, axis, delta, limits) {
                Found::Absent => resize_in(second, b, target, axis, delta, limits),
                found => found,
            };
            match found {
                Found::Pending if *own == axis => {
                    let (_, current) = a.span(axis);
                    Found::Done(place_divider(
                        axis,
                        ratio,
                        first,
                        second,
                        rect,
                        current + delta,
                        limits,
                    ))
                }
                found => found,
            }
        }
    }
}

/// [`Tree::drag`]'s walk: counts dividers in [`Tree::place`]'s order and moves the divider of the
/// node where `index` reaches zero to `position` (pixels). `None` if no divider was found.
fn drag_in(
    node: &mut Tree,
    rect: Rect,
    index: &mut usize,
    position: f64,
    limits: &Limits<'_>,
) -> Option<bool> {
    let Tree::Split {
        axis,
        ratio,
        first,
        second,
    } = node
    else {
        return None;
    };
    let (a, _, b) = halves_px(rect, *axis, *ratio, limits.spacing.between);
    if let Some(done) = drag_in(first, a, index, position, limits) {
        return Some(done);
    }
    if *index == 0 {
        let (start, _) = rect.span(*axis);
        return Some(place_divider(
            *axis,
            ratio,
            first,
            second,
            rect,
            position - start,
            limits,
        ));
    }
    *index -= 1;
    drag_in(second, b, index, position, limits)
}

/// [`Tree::remove`]'s node half: if one of `node`'s children is the `target` leaf, puts its sibling
/// in `node`'s place and returns the neighbour for focus.
fn remove_in(node: &mut Tree, target: u64) -> Option<u64> {
    let Tree::Split { first, second, .. } = node else {
        return None;
    };
    let (kept, focus) = if **first == Tree::Leaf(target) {
        let focus = second.first_leaf();
        (std::mem::replace(&mut **second, Tree::Leaf(target)), focus)
    } else if **second == Tree::Leaf(target) {
        let focus = first.last_leaf();
        (std::mem::replace(&mut **first, Tree::Leaf(target)), focus)
    } else {
        return remove_in(first, target).or_else(|| remove_in(second, target));
    };
    *node = kept;
    Some(focus)
}

// ---------------------------------------------------------------------------------------------
// Moving a pane: the tree operations, the plan and the pointer's zone.

/// Where a pane or a block of panes let go beside another pane, or at the window's edge, would
/// land ([`Tree::plan_beside`], [`Tree::plan_edge`]).
#[derive(Clone, Debug, PartialEq)]
pub struct Placement {
    /// The tree with the dropped subtree in place; the rest of it is untouched.
    pub tree: Tree,
    /// Where the dropped subtree lands, in points: its panes and the gaps between them.
    pub landing: Rect,
    /// The plain split was not what happened: the dropped subtree took more than its preferred
    /// share, the neighbour was squeezed to its minimum, or the drop moved up to a group above the
    /// pane because the pane itself was too small. The preview can tell the user that neighbours
    /// will shrink.
    pub made_room: bool,
}

/// How a length is shared between a subtree being added and the one it is added beside
/// ([`solve_share`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Share {
    /// The added subtree's length, in pixels.
    pub new_px: f64,
    /// The length is not the preferred one: a minimum moved it.
    pub squeezed: bool,
}

/// Shares `total` pixels (the length both subtrees have between them, the gap already taken out)
/// between a subtree being added, which would like `preferred` of it, and the one already there.
/// Neither goes below its minimum length: when half does not fit the added subtree it takes
/// exactly what it needs, and when taking that much would leave the old one less than it needs
/// the old one keeps its minimum. `None` when the two minimums do not fit in `total` together.
pub fn solve_share(total: f64, preferred: f64, need_new: f64, need_old: f64) -> Option<Share> {
    let want = (total * preferred).round();
    let new_px = within(need_new, total - need_old, want)?;
    Some(Share {
        new_px,
        squeezed: new_px != want,
    })
}

/// What a pointer over a layout is asking for while a pane is carried ([`Layout::zone_at`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Zone {
    /// The strip along the window's edge: a full-length column or row on that side.
    WindowEdge(Direction),
    /// The half of pane `target` nearest the pointer's edge: beside it, on `side`.
    Beside { target: u64, side: Direction },
    /// The middle of pane `target`: the two panes trade places.
    Swap { target: u64 },
    /// Over the carried pane's own place: nothing to do.
    Own,
    /// Over no pane at all, outside the area.
    Outside,
}

impl Zone {
    /// The word a preview writes in the region the zone asks for: where the block lands next
    /// to the pane, the full-length column or row at the window's edge, or the swap. `None`
    /// for the zones that ask for nothing.
    pub fn label(self) -> Option<&'static str> {
        match self {
            Zone::Beside { side, .. } => Some(match side {
                Direction::Left => "Left",
                Direction::Right => "Right",
                Direction::Up => "Above",
                Direction::Down => "Below",
            }),
            Zone::WindowEdge(side) => Some(match side {
                Direction::Left | Direction::Right => "Full height",
                Direction::Up | Direction::Down => "Full width",
            }),
            Zone::Swap { .. } => Some("Swap"),
            Zone::Own | Zone::Outside => None,
        }
    }
}

/// What a carried block shows under the pointer, and so what letting go would do
/// ([`Tree::verdict`]).
#[derive(Clone, Debug, PartialEq)]
pub enum Verdict {
    /// Letting go changes nothing: the pointer is over the pane's own place or outside the area,
    /// or there is nothing to move it beside.
    Nothing,
    /// The block lands: `placement` is where and in what tree, `zone` what the pointer asked for.
    Lands { zone: Zone, placement: Placement },
    /// The carried pane trades places with `target`, whose frame is `frame`; `fits` is whether
    /// both panes keep their minimum in each other's place.
    Swaps {
        target: u64,
        frame: Rect,
        fits: bool,
    },
    /// The block does not fit where the pointer asks (`zone`): `region` is the part of the
    /// layout it pointed at and `edges` the edges of the area that would take it, each with the
    /// place it would land.
    TooSmall {
        zone: Zone,
        region: Rect,
        edges: Vec<(Direction, Rect)>,
    },
    /// The block fits nowhere in this tab: no pane, no group and no edge has room.
    NoRoom,
}

/// The `share` of `frame` along `side`: the half a drop beside a pane asks for, the stretch of
/// the area a drop at the window's edge asks for.
fn share_of(frame: Rect, side: Direction, share: f64) -> Rect {
    match side {
        Direction::Left => Rect::new(frame.x, frame.y, frame.width * share, frame.height),
        Direction::Right => Rect::new(
            frame.x + frame.width * (1.0 - share),
            frame.y,
            frame.width * share,
            frame.height,
        ),
        Direction::Up => Rect::new(frame.x, frame.y, frame.width, frame.height * share),
        Direction::Down => Rect::new(
            frame.x,
            frame.y + frame.height * (1.0 - share),
            frame.width,
            frame.height * share,
        ),
    }
}

impl Layout {
    /// What the pointer at `point` (points, the area's space) asks for inside `bounds`, in this
    /// order: the strip [`WINDOW_EDGE_STRIP`] wide along the area's edge, nearest edge first —
    /// a full-length column or row; then, over a pane (a point in a gap or margin belongs to the
    /// nearest one), the carried pane's own place; then the pane's middle — more than
    /// [`SWAP_CORE`] in from every edge, measured as a fraction of the pane's extent along that
    /// edge's axis — which trades places; and last the pane's edge half, the edge being the
    /// nearest by that same fraction (a tie goes to left, right, up, down in that order).
    ///
    /// `own` is the carried pane when it belongs to this layout. A pane or block that comes from
    /// another tab has no place here to fall back on and nothing to trade with, so for `None`
    /// there is no `Own` and no `Swap`: the middle is the nearest edge's half like the rest.
    pub fn zone_at(&self, bounds: Rect, point: (f64, f64), own: Option<u64>) -> Zone {
        let (x, y) = point;
        let right = bounds.x + bounds.width;
        let bottom = bounds.y + bounds.height;
        if x < bounds.x || y < bounds.y || x >= right || y >= bottom {
            return Zone::Outside;
        }
        let (depth, side) = nearest_edge([
            (x - bounds.x, Direction::Left),
            (right - x, Direction::Right),
            (y - bounds.y, Direction::Up),
            (bottom - y, Direction::Down),
        ]);
        if depth < WINDOW_EDGE_STRIP {
            return Zone::WindowEdge(side);
        }
        let Some((target, frame)) = self.nearest_pane(point) else {
            return Zone::Outside;
        };
        if own == Some(target) {
            return Zone::Own;
        }
        let across = |from: f64, length: f64, at: f64| ((at - from) / length).clamp(0.0, 1.0);
        let u = across(frame.x, frame.width, x);
        let v = across(frame.y, frame.height, y);
        let (depth, side) = nearest_edge([
            (u, Direction::Left),
            (1.0 - u, Direction::Right),
            (v, Direction::Up),
            (1.0 - v, Direction::Down),
        ]);
        if own.is_some() && depth > SWAP_CORE {
            Zone::Swap { target }
        } else {
            Zone::Beside { target, side }
        }
    }

    /// The pane whose frame is nearest `point` — the one holding it, or for a point in a gap or
    /// in the margin the closest; the earlier in tree order on a tie. Frames with no area do not
    /// count.
    fn nearest_pane(&self, (x, y): (f64, f64)) -> Option<(u64, Rect)> {
        let distance = |frame: &Rect| {
            let dx = (frame.x - x).max(x - (frame.x + frame.width)).max(0.0);
            let dy = (frame.y - y).max(y - (frame.y + frame.height)).max(0.0);
            dx * dx + dy * dy
        };
        self.panes
            .iter()
            .filter(|(_, frame)| frame.width > 0.0 && frame.height > 0.0)
            .min_by(|(_, a), (_, b)| distance(a).total_cmp(&distance(b)))
            .map(|(id, frame)| (*id, *frame))
    }
}

/// The smallest of four distances with its side; the first on a tie.
fn nearest_edge(edges: [(f64, Direction); 4]) -> (f64, Direction) {
    edges
        .into_iter()
        .reduce(|best, edge| if edge.0 < best.0 { edge } else { best })
        .unwrap_or((f64::INFINITY, Direction::Left))
}

impl Tree {
    /// Whether `id` is a pane of this tree.
    fn holds(&self, id: u64) -> bool {
        match self {
            Tree::Leaf(own) => *own == id,
            Tree::Split { first, second, .. } => first.holds(id) || second.holds(id),
        }
    }

    /// Whether any pane of `other` is also a pane of this tree: a pane cannot be in a tree twice,
    /// so such a subtree cannot be added.
    fn shares_a_pane_with(&self, other: &Tree) -> bool {
        other.leaves().into_iter().any(|id| self.holds(id))
    }

    /// The minimum length of this subtree along `axis` in `room`, in pixels: the length that keeps
    /// every one of its panes at its minimum, its inner ratios taken as fixed. What a block that is
    /// about to be added needs, and what the subtree it is added beside is left with.
    pub fn min_length(&self, axis: Axis, room: &Room<'_>) -> f64 {
        self.min_px(axis, &room.limits())
    }

    /// Whether every pane of this tree, laid out in `room`, keeps at least its smallest size
    /// (`room.min`): what a swap or a rearrangement must leave true, or it is not made — a pane
    /// at a larger point size has a larger smallest.
    pub fn fits(&self, room: &Room<'_>) -> bool {
        self.layout_spaced(room.bounds, room.scale, room.spacing)
            .panes
            .iter()
            .all(|(id, rect)| {
                let min = (room.min)(*id);
                rect.width >= min.width && rect.height >= min.height
            })
    }

    /// Makes panes `a` and `b` trade places: each takes the other's frame, the ratios stay.
    /// `false` and no change if they are the same pane or either is missing.
    pub fn swap(&mut self, a: u64, b: u64) -> bool {
        if a == b || !self.holds(a) || !self.holds(b) {
            return false;
        }
        self.exchange(a, b);
        true
    }

    fn exchange(&mut self, a: u64, b: u64) {
        match self {
            Tree::Leaf(id) if *id == a => *id = b,
            Tree::Leaf(id) if *id == b => *id = a,
            Tree::Leaf(_) => {}
            Tree::Split { first, second, .. } => {
                first.exchange(a, b);
                second.exchange(a, b);
            }
        }
    }

    /// Adds `incoming` beside the node `up` levels above pane `leaf` (0 is the pane itself, 1 its
    /// parent, … the root), on `side` of it, taking `share` of the room the two have between
    /// them. The subtree keeps its own inner ratios. `false` and no change if the pane is not in
    /// the tree, `up` reaches past the root, `share` is not within `0..=1`, or a pane of
    /// `incoming` is already in the tree.
    pub fn insert_beside(
        &mut self,
        leaf: u64,
        up: usize,
        side: Direction,
        incoming: Tree,
        share: f64,
    ) -> bool {
        let Some(path) = self.path_to(leaf) else {
            return false;
        };
        let Some(depth) = path.len().checked_sub(up) else {
            return false;
        };
        if !valid_share(share) || self.shares_a_pane_with(&incoming) {
            return false;
        }
        wrap(
            self.node_at_mut(&path[..depth]),
            side,
            incoming,
            first_ratio(side, share),
        );
        true
    }

    /// Adds `incoming` along the whole of one edge of the area — a column on the left or right, a
    /// row above or below — taking `share` of the area. `false` and no change if `share` is not
    /// within `0..=1` or a pane of `incoming` is already in the tree.
    pub fn insert_at_edge(&mut self, side: Direction, incoming: Tree, share: f64) -> bool {
        if !valid_share(share) || self.shares_a_pane_with(&incoming) {
            return false;
        }
        wrap(self, side, incoming, first_ratio(side, share));
        true
    }

    /// Where `incoming` would land if let go on `side` of pane `leaf`, and the tree that
    /// makes. It asks for [`PANE_EDGE_SHARE`] of the pane; when the pane is too small to give
    /// that, `incoming` takes what it needs ([`solve_share`]) and the pane shrinks to its minimum,
    /// and when it cannot give even that the drop moves up to the group the pane is part of, then
    /// the group above it, until the whole area. Every pane stays at or above its minimum.
    /// `None` when not even the whole area has room.
    ///
    /// The tree must not hold `incoming` — a pane carried within its own tab is taken out of the
    /// tree first (`remove`), and `leaf` is a pane that stays.
    pub fn plan_beside(
        &self,
        leaf: u64,
        side: Direction,
        incoming: &Tree,
        room: &Room<'_>,
    ) -> Option<Placement> {
        let path = self.path_to(leaf)?;
        self.plan_climbing(&path, side, incoming, PANE_EDGE_SHARE, room)
    }

    /// Where `incoming` would land if let go along one edge of the area ([`WINDOW_EDGE_SHARE`] of
    /// it, made smaller or larger by the minimums the way [`Tree::plan_beside`] does), and the
    /// tree that makes. `None` when the edge cannot take it.
    pub fn plan_edge(
        &self,
        side: Direction,
        incoming: &Tree,
        room: &Room<'_>,
    ) -> Option<Placement> {
        self.plan_climbing(&[], side, incoming, WINDOW_EDGE_SHARE, room)
    }

    /// The edges of the area that could take `incoming` ([`Tree::plan_edge`]), in the order left,
    /// right, up, down. When it is empty `incoming` fits nowhere in this tab: no pane, no group
    /// and no edge has room, since an edge is the last place a drop beside a pane climbs to.
    pub fn fitting_edges(&self, incoming: &Tree, room: &Room<'_>) -> Vec<Direction> {
        [
            Direction::Left,
            Direction::Right,
            Direction::Up,
            Direction::Down,
        ]
        .into_iter()
        .filter(|&side| self.plan_edge(side, incoming, room).is_some())
        .collect()
    }

    /// What `carried` shows with the pointer at `point` (points, the area's space) and so what
    /// letting go there does — the one answer the preview, the drop and the menu stand on, in
    /// `room`'s ground.
    ///
    /// The pointer is read against the layout **as it stands**, `carried` still in it when it is
    /// a pane of this tree ([`Layout::zone_at`]); nothing moves while it is carried. The plan is
    /// made on the tree without it ([`Tree::plan_beside`], [`Tree::plan_edge`]), so `room`'s
    /// spacing is the one of the tree it would become. A pane of another tab (or a block) is no
    /// pane here: it has no place to fall back on and nothing to trade with.
    ///
    /// Swapping needs no room, so it is judged first; every other zone is refused with the
    /// edges that would take the block, or with [`Verdict::NoRoom`] when none does.
    pub fn verdict(&self, carried: &Tree, room: &Room<'_>, point: (f64, f64)) -> Verdict {
        let layout = self.layout_spaced(room.bounds, room.scale, room.spacing);
        let own = match carried {
            Tree::Leaf(id) if self.holds(*id) => Some(*id),
            _ => None,
        };
        let zone = layout.zone_at(room.bounds, point, own);
        let frame_of = |pane: u64| {
            layout
                .panes
                .iter()
                .find(|(id, _)| *id == pane)
                .map(|(_, frame)| *frame)
        };
        if let Zone::Swap { target } = zone {
            let (Some(own), Some(frame)) = (own, frame_of(target)) else {
                return Verdict::Nothing;
            };
            let mut swapped = self.clone();
            if !swapped.swap(own, target) {
                return Verdict::Nothing;
            }
            let after = swapped.layout_spaced(room.bounds, room.scale, room.spacing);
            let fits = after.panes.iter().all(|(id, frame)| {
                let min = (room.min)(*id);
                frame.width >= min.width && frame.height >= min.height
            });
            return Verdict::Swaps {
                target,
                frame,
                fits,
            };
        }
        let mut rest = self.clone();
        match own {
            Some(id) => {
                if !matches!(rest.remove(id), Removal::Removed { .. }) {
                    return Verdict::Nothing;
                }
            }
            None if self.shares_a_pane_with(carried) => return Verdict::Nothing,
            None => {}
        }
        let (placement, region) = match zone {
            Zone::Beside { target, side } => {
                let Some(frame) = frame_of(target) else {
                    return Verdict::Nothing;
                };
                (
                    rest.plan_beside(target, side, carried, room),
                    share_of(frame, side, PANE_EDGE_SHARE),
                )
            }
            Zone::WindowEdge(side) => (
                rest.plan_edge(side, carried, room),
                share_of(room.bounds, side, WINDOW_EDGE_SHARE),
            ),
            Zone::Swap { .. } | Zone::Own | Zone::Outside => return Verdict::Nothing,
        };
        if let Some(placement) = placement {
            return Verdict::Lands { zone, placement };
        }
        let edges: Vec<(Direction, Rect)> = [
            Direction::Left,
            Direction::Right,
            Direction::Up,
            Direction::Down,
        ]
        .into_iter()
        .filter_map(|side| {
            rest.plan_edge(side, carried, room)
                .map(|placement| (side, placement.landing))
        })
        .collect();
        if edges.is_empty() {
            Verdict::NoRoom
        } else {
            Verdict::TooSmall {
                zone,
                region,
                edges,
            }
        }
    }

    /// The plan at the node `path` leads to, then at each node above it up to the root, the
    /// first that has room.
    fn plan_climbing(
        &self,
        path: &[bool],
        side: Direction,
        incoming: &Tree,
        preferred: f64,
        room: &Room<'_>,
    ) -> Option<Placement> {
        if self.shares_a_pane_with(incoming) {
            return None;
        }
        (0..=path.len()).rev().find_map(|depth| {
            let mut placement = self.plan_at(&path[..depth], side, incoming, preferred, room)?;
            placement.made_room |= depth != path.len();
            Some(placement)
        })
    }

    /// The plan with `incoming` taking the node at `path` as its neighbour.
    fn plan_at(
        &self,
        path: &[bool],
        side: Direction,
        incoming: &Tree,
        preferred: f64,
        room: &Room<'_>,
    ) -> Option<Placement> {
        let limits = room.limits();
        let between = room.spacing.between;
        let node = self.node_at(path);
        let frame = self.frame_at(room.root(), path, between);
        let axis = side.axis();
        let (_, length) = frame.span(axis);
        let (_, breadth) = frame.span(axis.other());
        // The subtree takes the node's whole breadth, so its own minimum across must fit it.
        if incoming.min_px(axis.other(), &limits) > breadth {
            return None;
        }
        let total = length - between;
        if total <= 0.0 {
            return None;
        }
        let share = solve_share(
            total,
            preferred,
            incoming.min_px(axis, &limits),
            node.min_px(axis, &limits),
        )?;
        let ratio = first_ratio(side, share.new_px / total);
        let (before, _, after) = halves_px(frame, axis, ratio, between);
        let landing = if side.forward() { after } else { before };
        let mut tree = self.clone();
        wrap(tree.node_at_mut(path), side, incoming.clone(), ratio);
        Some(Placement {
            tree,
            landing: landing.scaled(1.0 / room.scale),
            made_room: share.squeezed,
        })
    }

    /// The turns from the root down to pane `leaf` (`false` for the first subtree, `true` for the
    /// second); `None` if it is not in the tree. A node is addressed by a prefix of its pane's
    /// path, which is how a group — a node with no identity of its own — is named.
    fn path_to(&self, leaf: u64) -> Option<Vec<bool>> {
        match self {
            Tree::Leaf(id) => (*id == leaf).then(Vec::new),
            Tree::Split { first, second, .. } => {
                if let Some(mut path) = first.path_to(leaf) {
                    path.insert(0, false);
                    Some(path)
                } else {
                    second.path_to(leaf).map(|mut path| {
                        path.insert(0, true);
                        path
                    })
                }
            }
        }
    }

    /// The node `path` leads to (the pane itself if the path outruns the tree).
    fn node_at(&self, path: &[bool]) -> &Tree {
        match (self, path.split_first()) {
            (Tree::Split { first, second, .. }, Some((&turn, rest))) => {
                if turn { second } else { first }.node_at(rest)
            }
            _ => self,
        }
    }

    fn node_at_mut(&mut self, path: &[bool]) -> &mut Tree {
        match (self, path.split_first()) {
            (Tree::Split { first, second, .. }, Some((&turn, rest))) => {
                if turn { second } else { first }.node_at_mut(rest)
            }
            (node, _) => node,
        }
    }

    /// The pixels the node at `path` occupies when the tree is laid out in `root`.
    fn frame_at(&self, root: Rect, path: &[bool], between: f64) -> Rect {
        let mut node = self;
        let mut frame = root;
        for &turn in path {
            let Tree::Split {
                axis,
                ratio,
                first,
                second,
            } = node
            else {
                break;
            };
            let (a, _, b) = halves_px(frame, *axis, *ratio, between);
            (node, frame) = if turn { (second, b) } else { (first, a) };
        }
        frame
    }
}

/// Whether `share` can be a share of a length.
fn valid_share(share: f64) -> bool {
    (0.0..=1.0).contains(&share)
}

/// The ratio of a node (the first subtree's share) whose added subtree has `share`, standing
/// first when it is let go on the left or above and second otherwise.
fn first_ratio(side: Direction, share: f64) -> f64 {
    if side.forward() { 1.0 - share } else { share }
}

/// Replaces `node` with a split of it and `incoming`, `incoming` on `side`.
fn wrap(node: &mut Tree, side: Direction, incoming: Tree, ratio: f64) {
    let old = std::mem::replace(node, Tree::Leaf(0));
    let (first, second) = if side.forward() {
        (old, incoming)
    } else {
        (incoming, old)
    };
    *node = Tree::Split {
        axis: side.axis(),
        ratio,
        first: Box::new(first),
        second: Box::new(second),
    };
}

#[cfg(test)]
mod tests {
    use super::{
        Axis, Direction, Divider, Layout, PANE_EDGE_SHARE, Placement, Rect, Removal, Room,
        SWAP_CORE, Share, Size, Slide, Spacing, Tree, Verdict, WINDOW_EDGE_SHARE,
        WINDOW_EDGE_STRIP, Zone, slide, sliver, solve_share,
    };

    fn area(rect: &Rect) -> f64 {
        rect.width * rect.height
    }

    fn overlaps(a: &Rect, b: &Rect) -> bool {
        a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height
    }

    /// Three panes: left half, right half split in two (L).
    fn three() -> Tree {
        let mut tree = Tree::Leaf(1);
        assert!(tree.split(1, Axis::Horizontal, 2));
        assert!(tree.split(2, Axis::Vertical, 3));
        tree
    }

    #[test]
    fn a_split_gives_two_equal_leaves() {
        let mut tree = Tree::Leaf(7);
        assert!(tree.split(7, Axis::Horizontal, 8));
        assert_eq!(tree.leaves(), vec![7, 8], "old pane left, new pane right");
        let layout = tree.layout(Rect::new(0.0, 0.0, 801.0, 600.0), 1.0);
        let [(_, left), (_, right)] = layout.panes.as_slice() else {
            panic!("expected two frames: {layout:?}");
        };
        assert_eq!(left.width, right.width, "the area splits evenly");
        assert_eq!(left.height, 600.0);
        assert_eq!(
            layout.dividers,
            vec![Divider {
                rect: Rect::new(400.0, 0.0, 1.0, 600.0),
                axis: Axis::Horizontal,
            }]
        );
    }

    #[test]
    fn a_split_down_puts_the_new_pane_below() {
        let mut tree = Tree::Leaf(1);
        assert!(tree.split(1, Axis::Vertical, 2));
        let layout = tree.layout(Rect::new(0.0, 0.0, 800.0, 601.0), 1.0);
        let top = layout.panes[0].1;
        let bottom = layout.panes[1].1;
        assert_eq!(layout.panes[1].0, 2);
        assert!(
            bottom.y > top.y,
            "in top-down coordinates the second leaf is below"
        );
        assert_eq!(top.height, bottom.height);
    }

    #[test]
    fn splitting_a_missing_leaf_changes_nothing() {
        let mut tree = three();
        let before = tree.clone();
        assert!(!tree.split(9, Axis::Horizontal, 10));
        assert_eq!(tree, before);
    }

    #[test]
    fn removing_pulls_the_sibling_up() {
        let mut tree = three();
        // Remove 3: 2 takes the whole right half, the tree drops to a single split.
        assert_eq!(tree.remove(3), Removal::Removed { focus: 2 });
        let mut expected = Tree::Leaf(1);
        assert!(expected.split(1, Axis::Horizontal, 2));
        assert_eq!(tree, expected);
        // Remove 1: the only remaining leaf is 2.
        assert_eq!(tree.remove(1), Removal::Removed { focus: 2 });
        assert_eq!(tree, Tree::Leaf(2));
    }

    #[test]
    fn the_neighbour_touches_the_closed_pane() {
        // When the left pane (1) closes, focus goes to the right subtree's adjacent leaf:
        // its first leaf (2, top right).
        let mut tree = three();
        assert_eq!(tree.remove(1), Removal::Removed { focus: 2 });
        // When the top right (2) closes, its sibling is 3 — a single leaf, itself.
        let mut tree = three();
        assert_eq!(tree.remove(2), Removal::Removed { focus: 3 });
        // When the second half's subtree closes, focus goes to the first half's last leaf.
        let mut tree = Tree::Leaf(1);
        assert!(tree.split(1, Axis::Vertical, 2));
        assert!(tree.split(1, Axis::Horizontal, 3));
        // Layout: [1 | 3] on top, 2 below. When 2 closes, focus is 3 (the top's last leaf).
        assert_eq!(tree.remove(2), Removal::Removed { focus: 3 });
    }

    #[test]
    fn the_last_leaf_is_not_removed() {
        let mut tree = Tree::Leaf(4);
        assert_eq!(tree.remove(4), Removal::Last);
        assert_eq!(tree, Tree::Leaf(4));
        assert_eq!(tree.remove(5), Removal::Missing);
        assert_eq!(three().remove(9), Removal::Missing);
    }

    #[test]
    fn order_is_depth_first() {
        assert_eq!(three().leaves(), vec![1, 2, 3]);
        let mut tree = three();
        assert!(tree.split(1, Axis::Vertical, 4));
        assert_eq!(tree.leaves(), vec![1, 4, 2, 3]);
    }

    #[test]
    fn frames_and_dividers_tile_the_bounds_exactly() {
        // Odd width, two scales and a fractional point boundary: frames and
        // dividers tile the area with no gap, do not overlap, and every edge is
        // on a device pixel.
        for scale in [1.0, 2.0] {
            let bounds = Rect::new(0.0, 0.0, 901.5, 603.0);
            let mut tree = three();
            assert!(tree.split(1, Axis::Vertical, 4));
            let layout = tree.layout(bounds, scale);
            let mut rects: Vec<Rect> = layout.panes.iter().map(|(_, rect)| *rect).collect();
            rects.extend(layout.dividers.iter().map(|divider| divider.rect));
            let total: f64 = rects.iter().map(area).sum();
            // The fractional point boundary snaps to a pixel at 1× (901.5 → 902):
            // the covered area is that of the snapped bounds.
            let snapped =
                (bounds.width * scale).round() * (bounds.height * scale).round() / (scale * scale);
            assert!(
                (total - snapped).abs() < 1e-9,
                "scale {scale}: total {total}"
            );
            for (i, a) in rects.iter().enumerate() {
                for b in &rects[i + 1..] {
                    assert!(!overlaps(a, b), "scale {scale}: {a:?} ∩ {b:?}");
                }
                for edge in [a.x, a.y, a.x + a.width, a.y + a.height] {
                    let px = edge * scale;
                    assert!(
                        (px - px.round()).abs() < 1e-9,
                        "scale {scale}: {edge} not on a pixel"
                    );
                }
            }
            for divider in &layout.dividers {
                let divider = divider.rect;
                assert!(
                    (divider.width.min(divider.height) * scale - 1.0).abs() < 1e-9,
                    "divider is one pixel: {divider:?}"
                );
            }
        }
    }

    fn frame_of(layout: &Layout, id: u64) -> Rect {
        layout
            .panes
            .iter()
            .find(|(pane, _)| *pane == id)
            .map(|(_, rect)| *rect)
            .expect("pane must be in the layout")
    }

    fn no_min(_: u64) -> Size {
        Size::new(0.0, 0.0)
    }

    #[test]
    fn the_neighbour_is_found_by_direction_in_an_l_layout() {
        // Layout: left 1 (full height) | top right 2 / bottom right 3.
        let tree = three();
        let layout = tree.layout(Rect::new(0.0, 0.0, 801.0, 601.0), 1.0);
        assert_eq!(
            layout.neighbour(1, Direction::Right),
            Some(2),
            "tree order on equal overlap"
        );
        assert_eq!(layout.neighbour(2, Direction::Left), Some(1));
        assert_eq!(layout.neighbour(3, Direction::Left), Some(1));
        assert_eq!(layout.neighbour(2, Direction::Down), Some(3));
        assert_eq!(layout.neighbour(3, Direction::Up), Some(2));
        assert_eq!(
            layout.neighbour(1, Direction::Left),
            None,
            "no neighbour at the edge"
        );
        assert_eq!(layout.neighbour(1, Direction::Up), None);
        assert_eq!(layout.neighbour(2, Direction::Up), None);
        assert_eq!(
            layout.neighbour(9, Direction::Up),
            None,
            "pane not in the layout"
        );
        // Once the bottom right moves up, the left pane overlaps 3 the most.
        let mut tree = three();
        assert!(tree.resize(
            3,
            Direction::Up,
            200.0,
            Rect::new(0.0, 0.0, 801.0, 601.0),
            1.0,
            &no_min
        ));
        let layout = tree.layout(Rect::new(0.0, 0.0, 801.0, 601.0), 1.0);
        assert_eq!(
            layout.neighbour(1, Direction::Right),
            Some(3),
            "the most overlapping"
        );
    }

    #[test]
    fn next_and_previous_cycle_in_tree_order() {
        let tree = three();
        assert_eq!(tree.cycle(1, true), Some(2));
        assert_eq!(tree.cycle(3, true), Some(1), "last to first");
        assert_eq!(tree.cycle(1, false), Some(3), "first to last");
        assert_eq!(Tree::Leaf(1).cycle(1, true), None, "single pane");
        assert_eq!(tree.cycle(9, true), None);
    }

    #[test]
    fn resizing_moves_the_nearest_divider_on_that_axis() {
        let bounds = Rect::new(0.0, 0.0, 801.0, 601.0);
        let mut tree = three();
        // 2 is top right: the nearest horizontal ancestor is the root. Right → divider right.
        assert!(tree.resize(2, Direction::Right, 10.0, bounds, 1.0, &no_min));
        let layout = tree.layout(bounds, 1.0);
        assert_eq!(frame_of(&layout, 1).width, 410.0);
        assert_eq!(frame_of(&layout, 2).width, 390.0);
        assert_eq!(frame_of(&layout, 3).width, 390.0, "same subtree together");
        // The nearest vertical ancestor is the right subtree; 1 has no vertical ancestor.
        assert!(tree.resize(2, Direction::Down, 20.0, bounds, 1.0, &no_min));
        let layout = tree.layout(bounds, 1.0);
        assert_eq!(frame_of(&layout, 2).height, 320.0);
        assert_eq!(frame_of(&layout, 3).height, 280.0);
        assert!(!tree.resize(1, Direction::Down, 20.0, bounds, 1.0, &no_min));
        assert!(!tree.resize(9, Direction::Right, 20.0, bounds, 1.0, &no_min));
    }

    #[test]
    fn resizing_stops_at_the_minimum_and_keeps_the_area() {
        let bounds = Rect::new(0.0, 0.0, 801.0, 601.0);
        let min = |_: u64| Size::new(200.0, 150.0);
        let mut tree = three();
        let covered = |layout: &Layout| -> f64 {
            layout.panes.iter().map(|(_, r)| area(r)).sum::<f64>()
                + layout.dividers.iter().map(|d| area(&d.rect)).sum::<f64>()
        };
        let area_before = covered(&tree.layout(bounds, 1.0));
        // Keep going left: 1 stops at the minimum width.
        let mut steps = 0;
        while tree.resize(1, Direction::Left, 50.0, bounds, 1.0, &min) {
            steps += 1;
            assert!(steps < 100, "must stop at the limit");
        }
        let layout = tree.layout(bounds, 1.0);
        assert_eq!(frame_of(&layout, 1).width, 200.0);
        let area_after = covered(&layout);
        assert_eq!(area_before, area_after, "total area is preserved");
        // Keep going right: the right subtree stops at the minimum width.
        while tree.resize(1, Direction::Right, 50.0, bounds, 1.0, &min) {}
        let layout = tree.layout(bounds, 1.0);
        assert_eq!(frame_of(&layout, 2).width, 200.0);
        assert_eq!(frame_of(&layout, 3).width, 200.0);
        // Down: 3 at the minimum height.
        while tree.resize(2, Direction::Down, 50.0, bounds, 1.0, &min) {}
        assert_eq!(frame_of(&tree.layout(bounds, 1.0), 3).height, 150.0);
    }

    #[test]
    fn a_nested_side_is_limited_by_its_smallest_pane() {
        // Left [1 | 4] (4 is narrow but above the limit), right 2. Narrowing the left subtree
        // narrows 4 too: the limit is not the sum, it comes from 4's share.
        let bounds = Rect::new(0.0, 0.0, 1001.0, 400.0);
        let mut tree = Tree::Leaf(1);
        assert!(tree.split(1, Axis::Horizontal, 2));
        assert!(tree.split(1, Axis::Horizontal, 4));
        let layout = tree.layout(bounds, 1.0);
        let divider = layout.dividers[0];
        assert!(tree.drag(0, divider.rect.x + 140.0, bounds, 1.0, &no_min));
        assert!(frame_of(&tree.layout(bounds, 1.0), 4).width > 100.0);
        let min = |_: u64| Size::new(100.0, 10.0);
        while tree.resize(2, Direction::Left, 25.0, bounds, 1.0, &min) {}
        let layout = tree.layout(bounds, 1.0);
        for id in [1, 2, 4] {
            assert!(frame_of(&layout, id).width >= 100.0, "{id}: {layout:?}");
        }
    }

    #[test]
    fn a_drag_moves_exactly_the_dragged_divider() {
        let bounds = Rect::new(0.0, 0.0, 801.0, 601.0);
        let mut tree = three();
        assert!(tree.split(1, Axis::Vertical, 4));
        let before = tree.layout(bounds, 1.0);
        for (index, divider) in before.dividers.iter().enumerate() {
            let mut moved = tree.clone();
            let (position, axis) = match divider.axis {
                Axis::Horizontal => (divider.rect.x - 30.0, Axis::Horizontal),
                Axis::Vertical => (divider.rect.y - 30.0, Axis::Vertical),
            };
            assert!(moved.drag(index, position, bounds, 1.0, &no_min));
            let after = moved.layout(bounds, 1.0);
            for (other, (a, b)) in before.dividers.iter().zip(&after.dividers).enumerate() {
                if other == index {
                    let (was, now) = match axis {
                        Axis::Horizontal => (a.rect.x, b.rect.x),
                        Axis::Vertical => (a.rect.y, b.rect.y),
                    };
                    assert_eq!(now, was - 30.0, "dragged divider {index} at the pointer");
                } else if a.axis == b.axis && a.axis != axis {
                    // Dividers on the other axis keep their position (their length may change).
                    let (was, now) = match a.axis {
                        Axis::Horizontal => (a.rect.x, b.rect.x),
                        Axis::Vertical => (a.rect.y, b.rect.y),
                    };
                    assert_eq!(was, now, "divider {other} in place");
                }
            }
        }
        // A drag beyond the limit is clamped, a nonexistent divider is a no-op.
        let min = |_: u64| Size::new(100.0, 100.0);
        let mut tree = three();
        assert!(tree.drag(0, -500.0, bounds, 1.0, &min));
        assert_eq!(frame_of(&tree.layout(bounds, 1.0), 1).width, 100.0);
        assert!(!tree.drag(7, 10.0, bounds, 1.0, &min));
    }

    #[test]
    fn equalizing_gives_panes_on_one_axis_the_same_span() {
        let bounds = Rect::new(0.0, 0.0, 901.0, 601.0);
        // Three columns [1 | 2 | 3] (nested) and the right column split in two: the columns
        // are equal, the right column's two panes are equal.
        let mut tree = Tree::Leaf(1);
        assert!(tree.split(1, Axis::Horizontal, 2));
        assert!(tree.split(2, Axis::Horizontal, 3));
        assert!(tree.split(3, Axis::Vertical, 4));
        assert!(tree.drag(0, 100.0, bounds, 1.0, &no_min));
        assert!(tree.drag(2, 150.0, bounds, 1.0, &no_min));
        tree.equalize();
        let layout = tree.layout(bounds, 1.0);
        let widths: Vec<f64> = [1, 2, 3]
            .iter()
            .map(|id| frame_of(&layout, *id).width)
            .collect();
        for width in &widths {
            assert!(
                (width - widths[0]).abs() <= 1.0,
                "columns equal: {widths:?}"
            );
        }
        assert!((frame_of(&layout, 3).height - frame_of(&layout, 4).height).abs() <= 1.0);
        // In an L layout the left pane is half width: the count on the axis, not the leaf count.
        let mut tree = three();
        assert!(tree.drag(0, 100.0, bounds, 1.0, &no_min));
        tree.equalize();
        let layout = tree.layout(bounds, 1.0);
        assert_eq!(frame_of(&layout, 1).width, frame_of(&layout, 2).width);
    }

    #[test]
    fn a_zoomed_leaf_takes_the_whole_area_and_gives_it_back() {
        let bounds = Rect::new(0.0, 0.0, 801.5, 601.0);
        let tree = three();
        let before = tree.layout(bounds, 2.0);
        let zoomed = tree.layout_zoomed(bounds, 2.0, Some(3));
        assert_eq!(zoomed.panes, vec![(3, Rect::new(0.0, 0.0, 801.5, 601.0))]);
        assert!(zoomed.dividers.is_empty(), "no dividers while zoomed");
        assert_eq!(
            tree.layout_zoomed(bounds, 2.0, None),
            before,
            "old frames back after unzoom"
        );
        assert_eq!(
            tree.layout_zoomed(bounds, 2.0, Some(9)),
            before,
            "leaf not in the tree"
        );
    }

    // ----- spacing -----

    #[test]
    fn the_divided_spacing_is_the_layout_the_ungapped_functions_give() {
        for scale in [1.0, 2.0] {
            let bounds = Rect::new(3.0, 5.0, 901.5, 603.0);
            let mut tree = three();
            assert!(tree.split(1, Axis::Vertical, 4));
            assert_eq!(
                tree.layout_spaced(bounds, scale, Spacing::DIVIDED),
                tree.layout(bounds, scale),
                "scale {scale}"
            );
        }
        assert_eq!(Spacing::DIVIDED.between_px(), 1.0);
        assert_eq!(Spacing::DIVIDED.around_px(), 0.0);
    }

    #[test]
    fn a_gap_given_in_points_is_whole_pixels() {
        let spacing = Spacing::gapped(6.0, 6.0, 2.0);
        assert_eq!((spacing.between_px(), spacing.around_px()), (12.0, 12.0));
        let spacing = Spacing::gapped(2.25, 0.0, 1.0);
        assert_eq!((spacing.between_px(), spacing.around_px()), (2.0, 0.0));
        let spacing = Spacing::gapped(-3.0, -1.0, 2.0);
        assert_eq!((spacing.between_px(), spacing.around_px()), (0.0, 0.0));
        let spacing = Spacing::gapped(8.0, 8.0, 2.0).with_top(0.0, 2.0);
        assert_eq!((spacing.around_px(), spacing.top_px()), (16.0, 0.0));
    }

    #[test]
    fn a_top_margin_of_its_own_moves_only_the_top_edge() {
        // Two panes side by side: with no top margin they start at the area's top and keep the
        // margin round the other three sides.
        let mut tree = Tree::Leaf(1);
        assert!(tree.split(1, Axis::Horizontal, 2));
        let bounds = Rect::new(0.0, 0.0, 400.0, 300.0);
        let even = tree.layout_spaced(bounds, 1.0, Spacing::gapped(8.0, 8.0, 1.0));
        let open = tree.layout_spaced(
            bounds,
            1.0,
            Spacing::gapped(8.0, 8.0, 1.0).with_top(0.0, 1.0),
        );
        for ((_, a), (_, b)) in even.panes.iter().zip(&open.panes) {
            assert_eq!((b.x, b.width), (a.x, a.width));
            assert_eq!(b.y, 0.0);
            assert_eq!(a.y, 8.0);
            assert_eq!(b.y + b.height, a.y + a.height, "the bottom margin stays");
        }
    }

    #[test]
    fn a_gapped_layout_tiles_the_area_inside_its_margin() {
        // Two scales, a fractional point boundary: panes and gaps tile the area inside the
        // margin with no overlap, every edge on a device pixel, and the gaps are the strips
        // between two panes.
        for scale in [1.0, 2.0] {
            let bounds = Rect::new(0.0, 0.0, 901.5, 603.0);
            let spacing = Spacing::gapped(6.0, 6.0, scale);
            let mut tree = three();
            assert!(tree.split(1, Axis::Vertical, 4));
            let layout = tree.layout_spaced(bounds, scale, spacing);
            let mut rects: Vec<Rect> = layout.panes.iter().map(|(_, rect)| *rect).collect();
            rects.extend(layout.dividers.iter().map(|divider| divider.rect));
            let around = spacing.around_px();
            let inner = (bounds.width * scale).round() - 2.0 * around;
            let inner_height = (bounds.height * scale).round() - 2.0 * around;
            let total: f64 = rects.iter().map(area).sum();
            assert!(
                (total - inner * inner_height / (scale * scale)).abs() < 1e-9,
                "scale {scale}: total {total}"
            );
            for (i, a) in rects.iter().enumerate() {
                for b in &rects[i + 1..] {
                    assert!(!overlaps(a, b), "scale {scale}: {a:?} ∩ {b:?}");
                }
                for edge in [a.x, a.y, a.x + a.width, a.y + a.height] {
                    let px = edge * scale;
                    assert!((px - px.round()).abs() < 1e-9, "scale {scale}: {edge}");
                }
                assert!(a.x * scale >= around - 1e-9 && a.y * scale >= around - 1e-9);
                assert!(
                    (a.x + a.width) * scale <= around + inner + 1e-9
                        && (a.y + a.height) * scale <= around + inner_height + 1e-9
                );
            }
            for divider in &layout.dividers {
                let thickness = divider.rect.width.min(divider.rect.height) * scale;
                assert!(
                    (thickness - spacing.between_px()).abs() < 1e-9,
                    "scale {scale}: {divider:?}"
                );
            }
        }
    }

    #[test]
    fn a_gapped_resize_and_drag_move_the_divider_where_the_layout_puts_it() {
        let min = |_: u64| Size::new(100.0, 60.0);
        let room = Room {
            bounds: Rect::new(0.0, 0.0, 1000.0, 600.0),
            scale: 2.0,
            spacing: Spacing::gapped(6.0, 6.0, 2.0),
            min: &min,
        };
        let divider_x = |tree: &Tree| {
            tree.layout_spaced(room.bounds, room.scale, room.spacing)
                .dividers[0]
                .rect
                .x
        };
        let mut tree = Tree::Leaf(1);
        assert!(tree.split(1, Axis::Horizontal, 2));
        let before = divider_x(&tree);
        assert!(tree.resize_within(1, Direction::Right, 10.0, &room));
        assert_eq!(divider_x(&tree), before + 10.0, "a step moves it a step");
        assert!(tree.drag_within(0, 300.0, &room));
        assert_eq!(divider_x(&tree), 300.0, "a drag puts it under the pointer");
        // Past the limit the right pane keeps its minimum (the gap is not part of either).
        assert!(tree.drag_within(0, 10_000.0, &room));
        let layout = tree.layout_spaced(room.bounds, room.scale, room.spacing);
        assert_eq!(frame_of(&layout, 2).width, 100.0);
        assert_eq!(frame_of(&layout, 2).x + 100.0, 1000.0 - 6.0);
    }

    // ----- swapping and inserting -----

    #[test]
    fn two_panes_trade_places_and_keep_the_ratios() {
        let mut tree = three();
        assert!(tree.resize(
            1,
            Direction::Right,
            40.0,
            Rect::new(0.0, 0.0, 800.0, 600.0),
            1.0,
            &no_min
        ));
        let before = tree.layout(Rect::new(0.0, 0.0, 800.0, 600.0), 1.0);
        assert!(tree.swap(1, 3));
        let after = tree.layout(Rect::new(0.0, 0.0, 800.0, 600.0), 1.0);
        assert_eq!(tree.leaves(), vec![3, 2, 1]);
        assert_eq!(frame_of(&after, 3), frame_of(&before, 1));
        assert_eq!(frame_of(&after, 1), frame_of(&before, 3));
        assert_eq!(frame_of(&after, 2), frame_of(&before, 2));
        assert!(tree.swap(3, 1), "swapping back restores the tree");
        assert_eq!(tree.leaves(), vec![1, 2, 3]);
    }

    #[test]
    fn a_swap_of_the_same_or_a_missing_pane_changes_nothing() {
        let mut tree = three();
        let before = tree.clone();
        assert!(!tree.swap(2, 2));
        assert!(!tree.swap(2, 9));
        assert!(!tree.swap(9, 2));
        assert_eq!(tree, before);
    }

    fn pair(axis: Axis, ratio: f64, first: u64, second: u64) -> Tree {
        Tree::Split {
            axis,
            ratio,
            first: Box::new(Tree::Leaf(first)),
            second: Box::new(Tree::Leaf(second)),
        }
    }

    #[test]
    fn a_subtree_is_added_on_the_side_it_is_let_go_on() {
        for (side, axis, share, expected) in [
            (
                Direction::Right,
                Axis::Horizontal,
                0.5,
                pair(Axis::Horizontal, 0.5, 1, 2),
            ),
            (
                Direction::Left,
                Axis::Horizontal,
                0.4,
                pair(Axis::Horizontal, 0.4, 2, 1),
            ),
            (
                Direction::Down,
                Axis::Vertical,
                0.3,
                pair(Axis::Vertical, 0.7, 1, 2),
            ),
            (
                Direction::Up,
                Axis::Vertical,
                0.25,
                pair(Axis::Vertical, 0.25, 2, 1),
            ),
        ] {
            let mut tree = Tree::Leaf(1);
            assert!(
                tree.insert_beside(1, 0, side, Tree::Leaf(2), share),
                "{side:?}"
            );
            assert_eq!(tree, expected, "{side:?} / {axis:?}");
        }
    }

    #[test]
    fn a_subtree_is_added_beside_a_group_when_asked_to_go_up() {
        // [1 | (2 / 3)]: one level above pane 3 is the right group, two is the whole tree.
        let mut tree = three();
        assert!(tree.insert_beside(3, 1, Direction::Right, Tree::Leaf(9), 0.5));
        let Tree::Split { second, .. } = &tree else {
            panic!("root must stay a split: {tree:?}");
        };
        assert_eq!(
            **second,
            Tree::Split {
                axis: Axis::Horizontal,
                ratio: 0.5,
                first: Box::new(pair(Axis::Vertical, 0.5, 2, 3)),
                second: Box::new(Tree::Leaf(9)),
            }
        );
        let mut tree = three();
        assert!(tree.insert_beside(3, 2, Direction::Up, Tree::Leaf(9), 0.5));
        assert_eq!(tree.leaves(), vec![9, 1, 2, 3], "above the whole tree");
        let mut tree = three();
        let before = tree.clone();
        assert!(
            !tree.insert_beside(3, 3, Direction::Up, Tree::Leaf(9), 0.5),
            "past the root"
        );
        assert_eq!(tree, before);
    }

    #[test]
    fn a_block_keeps_its_inner_ratios_and_a_bad_request_changes_nothing() {
        let block = pair(Axis::Vertical, 0.3, 7, 8);
        let mut tree = three();
        assert!(tree.insert_beside(2, 0, Direction::Left, block.clone(), 0.4));
        let path = tree.path_to(7).expect("block pane is in the tree");
        assert_eq!(
            *tree.node_at(&path[..path.len() - 1]),
            block,
            "the block's own split, ratio and all"
        );
        let before = tree.clone();
        assert!(
            !tree.insert_beside(9, 0, Direction::Left, Tree::Leaf(20), 0.5),
            "missing"
        );
        assert!(
            !tree.insert_beside(1, 0, Direction::Left, Tree::Leaf(2), 0.5),
            "pane twice"
        );
        assert!(
            !tree.insert_beside(1, 0, Direction::Left, Tree::Leaf(20), 1.5),
            "share"
        );
        assert!(
            !tree.insert_beside(1, 0, Direction::Left, Tree::Leaf(20), f64::NAN),
            "share"
        );
        assert!(!tree.insert_at_edge(Direction::Left, pair(Axis::Vertical, 0.5, 20, 7), 0.4));
        assert_eq!(tree, before);
    }

    #[test]
    fn a_subtree_is_added_along_the_whole_edge() {
        let mut tree = three();
        assert!(tree.insert_at_edge(Direction::Left, Tree::Leaf(9), 0.4));
        assert_eq!(
            tree,
            Tree::Split {
                axis: Axis::Horizontal,
                ratio: 0.4,
                first: Box::new(Tree::Leaf(9)),
                second: Box::new(three()),
            }
        );
        let mut tree = three();
        assert!(tree.insert_at_edge(Direction::Down, Tree::Leaf(9), 0.4));
        let layout = tree.layout(Rect::new(0.0, 0.0, 801.0, 1001.0), 1.0);
        let row = frame_of(&layout, 9);
        assert_eq!((row.x, row.width), (0.0, 801.0), "the row spans the area");
        assert_eq!(row.y + row.height, 1001.0);
    }

    // ----- room, plans and zones -----

    fn room_in<'a>(
        width: f64,
        height: f64,
        spacing: Spacing,
        min: &'a dyn Fn(u64) -> Size,
    ) -> Room<'a> {
        Room {
            bounds: Rect::new(0.0, 0.0, width, height),
            scale: 1.0,
            spacing,
            min,
        }
    }

    fn min_100_by_60(_: u64) -> Size {
        Size::new(100.0, 60.0)
    }

    /// The box around the frames of `ids`.
    fn union_of(layout: &Layout, ids: &[u64]) -> Rect {
        let frames: Vec<Rect> = ids.iter().map(|id| frame_of(layout, *id)).collect();
        let left = frames.iter().map(|f| f.x).fold(f64::INFINITY, f64::min);
        let top = frames.iter().map(|f| f.y).fold(f64::INFINITY, f64::min);
        let right = frames.iter().map(|f| f.x + f.width).fold(0.0, f64::max);
        let bottom = frames.iter().map(|f| f.y + f.height).fold(0.0, f64::max);
        Rect::new(left, top, right - left, bottom - top)
    }

    fn assert_at_minimum(layout: &Layout, scale: f64) {
        for (id, frame) in &layout.panes {
            assert!(
                frame.width * scale + 1e-9 >= (100.0 * scale).ceil()
                    && frame.height * scale + 1e-9 >= (60.0 * scale).ceil(),
                "pane {id} is below its minimum: {frame:?}"
            );
        }
    }

    #[test]
    fn shares_keep_both_sides_at_their_minimum() {
        assert_eq!(
            solve_share(1000.0, 0.5, 100.0, 100.0),
            Some(Share {
                new_px: 500.0,
                squeezed: false
            })
        );
        // Half does not fit the newcomer: it takes what it needs.
        assert_eq!(
            solve_share(200.0, 0.5, 150.0, 40.0),
            Some(Share {
                new_px: 150.0,
                squeezed: true
            })
        );
        // The newcomer's half would leave the old one too little: the old one keeps its minimum.
        assert_eq!(
            solve_share(200.0, 0.5, 10.0, 150.0),
            Some(Share {
                new_px: 50.0,
                squeezed: true
            })
        );
        assert_eq!(
            solve_share(200.0, 0.5, 150.0, 100.0),
            None,
            "the minimums cross"
        );
    }

    #[test]
    fn a_pane_let_go_beside_a_pane_that_has_room_takes_half() {
        let min = min_100_by_60;
        let room = room_in(1001.0, 600.0, Spacing::DIVIDED, &min);
        let tree = Tree::Leaf(1);
        let plan = tree
            .plan_beside(1, Direction::Right, &Tree::Leaf(2), &room)
            .expect("there is room");
        assert!(!plan.made_room);
        assert_eq!(plan.tree, pair(Axis::Horizontal, 0.5, 1, 2));
        assert_eq!(plan.landing, Rect::new(501.0, 0.0, 500.0, 600.0));
        let layout = plan.tree.layout(room.bounds, 1.0);
        assert_eq!(
            frame_of(&layout, 2),
            plan.landing,
            "the preview is the drop"
        );
        // And the left side puts the newcomer first.
        let plan = tree
            .plan_beside(1, Direction::Left, &Tree::Leaf(2), &room)
            .expect("there is room");
        assert_eq!(plan.landing, Rect::new(0.0, 0.0, 500.0, 600.0));
    }

    #[test]
    fn a_block_too_big_for_half_takes_what_it_needs() {
        // Three side by side at equal ratios need 403 px (their inner ratios are fixed), a
        // plain half of 600 would be 300.
        let min = min_100_by_60;
        let room = room_in(600.0, 600.0, Spacing::DIVIDED, &min);
        let block = Tree::Split {
            axis: Axis::Horizontal,
            ratio: 0.5,
            first: Box::new(Tree::Leaf(10)),
            second: Box::new(pair(Axis::Horizontal, 0.5, 11, 12)),
        };
        assert_eq!(block.min_length(Axis::Horizontal, &room), 403.0);
        let plan = Tree::Leaf(1)
            .plan_beside(1, Direction::Right, &block, &room)
            .expect("403 + 100 + the divider fits in 600");
        assert!(plan.made_room);
        let layout = plan.tree.layout(room.bounds, 1.0);
        assert_eq!(
            frame_of(&layout, 1).width,
            196.0,
            "the old pane is left the rest"
        );
        assert_eq!(plan.landing, union_of(&layout, &[10, 11, 12]));
        assert_eq!(plan.landing.width, 403.0);
        assert_at_minimum(&layout, 1.0);
    }

    #[test]
    fn a_pane_too_small_to_share_hands_the_drop_to_its_group() {
        // [1 / 2] with 2 only 150 px tall: below it there is no room for 100 more, but below the
        // whole group there is.
        let min = |_: u64| Size::new(100.0, 100.0);
        let room = room_in(600.0, 600.0, Spacing::DIVIDED, &min);
        let tree = pair(Axis::Vertical, 0.75, 1, 2);
        let layout = tree.layout(room.bounds, 1.0);
        assert_eq!(frame_of(&layout, 2).height, 150.0);
        let plan = tree
            .plan_beside(2, Direction::Down, &Tree::Leaf(9), &room)
            .expect("the group has room");
        assert!(plan.made_room, "the neighbours had to shrink");
        assert_eq!(plan.tree.leaves(), vec![1, 2, 9]);
        let layout = plan.tree.layout(room.bounds, 1.0);
        assert_eq!(plan.landing, frame_of(&layout, 9));
        assert_eq!(plan.landing.width, 600.0, "a row under everything");
        for (_, frame) in &layout.panes {
            assert!(frame.height >= 100.0 && frame.width >= 100.0, "{frame:?}");
        }
    }

    #[test]
    fn a_pane_that_fits_nowhere_has_no_plan() {
        let min = min_100_by_60;
        let room = room_in(150.0, 150.0, Spacing::DIVIDED, &min);
        let tree = Tree::Leaf(1);
        assert_eq!(
            tree.plan_beside(1, Direction::Right, &Tree::Leaf(2), &room),
            None
        );
        assert_eq!(tree.plan_edge(Direction::Left, &Tree::Leaf(2), &room), None);
        assert_eq!(
            tree.plan_beside(9, Direction::Right, &Tree::Leaf(2), &room),
            None
        );
        // Above and below there is room for 60 + 60.
        assert!(
            tree.plan_beside(1, Direction::Down, &Tree::Leaf(2), &room)
                .is_some()
        );
        // A pane already in the tree is not a newcomer.
        assert_eq!(
            tree.plan_beside(1, Direction::Down, &Tree::Leaf(1), &room),
            None
        );
    }

    #[test]
    fn the_edges_that_fit_are_those_that_take_the_drop() {
        let min = min_100_by_60;
        // 150 wide: two 100-wide panes do not fit side by side, two 60-tall ones stack easily.
        let room = room_in(150.0, 900.0, Spacing::DIVIDED, &min);
        let tree = Tree::Leaf(1);
        assert_eq!(
            tree.fitting_edges(&Tree::Leaf(2), &room),
            vec![Direction::Up, Direction::Down]
        );
        // A stacked pair needs 121 tall: it fits a column at the sides of a 150-tall area, not a
        // row above or below it.
        let room = room_in(600.0, 150.0, Spacing::DIVIDED, &min);
        let stack = pair(Axis::Vertical, 0.5, 7, 8);
        assert_eq!(
            tree.fitting_edges(&stack, &room),
            vec![Direction::Left, Direction::Right]
        );
        let room = room_in(120.0, 100.0, Spacing::DIVIDED, &min);
        assert!(tree.fitting_edges(&Tree::Leaf(2), &room).is_empty());
    }

    #[test]
    fn the_window_edge_asks_for_two_fifths_and_a_pane_edge_for_half() {
        assert_eq!((WINDOW_EDGE_SHARE, PANE_EDGE_SHARE), (0.4, 0.5));
        let min = min_100_by_60;
        let room = room_in(1001.0, 600.0, Spacing::DIVIDED, &min);
        let tree = three();
        let plan = tree
            .plan_edge(Direction::Right, &Tree::Leaf(9), &room)
            .expect("there is room");
        assert_eq!(plan.landing, Rect::new(601.0, 0.0, 400.0, 600.0));
        assert!(!plan.made_room);
        let layout = plan.tree.layout(room.bounds, 1.0);
        assert_eq!(frame_of(&layout, 9), plan.landing);
        assert_at_minimum(&layout, 1.0);
    }

    #[test]
    fn a_plan_never_takes_a_pane_below_its_minimum() {
        // Every size of area, every side of every pane, a single pane and a block as the
        // newcomer, plain and gapped: wherever there is a plan, all panes are at their minimum, the
        // landing is where the newcomer is drawn, and the layout still tiles the area.
        let min = min_100_by_60;
        let block = pair(Axis::Vertical, 0.5, 20, 21);
        let mut plans = 0;
        for scale in [1.0, 2.0] {
            for spacing in [Spacing::DIVIDED, Spacing::gapped(6.0, 6.0, scale)] {
                for width in (160..=900).step_by(37) {
                    for height in (130..=700).step_by(41) {
                        let room = Room {
                            bounds: Rect::new(0.0, 0.0, f64::from(width), f64::from(height)),
                            scale,
                            spacing,
                            min: &min,
                        };
                        let mut tree = three();
                        assert!(tree.split(2, Axis::Horizontal, 4));
                        // Only an area the tree itself fits: a plan keeps the panes it does not
                        // touch as they are.
                        let before = tree.layout_spaced(room.bounds, scale, spacing);
                        if before.panes.iter().any(|(_, f)| {
                            f.width * scale + 1e-9 < (100.0 * scale).ceil()
                                || f.height * scale + 1e-9 < (60.0 * scale).ceil()
                        }) {
                            continue;
                        }
                        for incoming in [Tree::Leaf(9), block.clone()] {
                            let ids = incoming.leaves();
                            let mut plan_list: Vec<Placement> = Vec::new();
                            for leaf in tree.leaves() {
                                for side in [
                                    Direction::Left,
                                    Direction::Right,
                                    Direction::Up,
                                    Direction::Down,
                                ] {
                                    plan_list
                                        .extend(tree.plan_beside(leaf, side, &incoming, &room));
                                }
                            }
                            for side in tree.fitting_edges(&incoming, &room) {
                                plan_list.extend(tree.plan_edge(side, &incoming, &room));
                            }
                            for plan in plan_list {
                                plans += 1;
                                let layout = plan.tree.layout_spaced(room.bounds, scale, spacing);
                                assert_eq!(layout.panes.len(), tree.leaves().len() + ids.len());
                                assert_eq!(
                                    plan.landing,
                                    union_of(&layout, &ids),
                                    "{width}x{height}"
                                );
                                // The existing panes are at their minimum, scaled to pixels.
                                for (id, frame) in &layout.panes {
                                    assert!(
                                        frame.width * scale + 1e-9 >= (100.0 * scale).ceil()
                                            && frame.height * scale + 1e-9 >= (60.0 * scale).ceil(),
                                        "{width}x{height} @{scale}: pane {id} is {frame:?}"
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
        assert!(
            plans > 1000,
            "the sweep must have covered real plans: {plans}"
        );
    }

    #[test]
    fn a_gapped_plan_lands_where_the_gapped_layout_draws_it() {
        let min = min_100_by_60;
        let spacing = Spacing::gapped(6.0, 6.0, 2.0);
        let room = Room {
            bounds: Rect::new(0.0, 0.0, 1000.0, 600.0),
            scale: 2.0,
            spacing,
            min: &min,
        };
        let tree = pair(Axis::Horizontal, 0.5, 1, 2);
        let plan = tree
            .plan_beside(2, Direction::Down, &Tree::Leaf(9), &room)
            .expect("there is room");
        let layout = plan.tree.layout_spaced(room.bounds, 2.0, spacing);
        assert_eq!(plan.landing, frame_of(&layout, 9));
        assert!(!plan.made_room);
    }

    #[test]
    fn a_point_over_the_window_edge_asks_for_the_whole_edge() {
        let bounds = Rect::new(0.0, 0.0, 1000.0, 600.0);
        let mut tree = Tree::Leaf(1);
        assert!(tree.split(1, Axis::Horizontal, 2));
        let layout = tree.layout(bounds, 1.0);
        assert_eq!(WINDOW_EDGE_STRIP, 18.0);
        for (point, expected) in [
            ((5.0, 300.0), Zone::WindowEdge(Direction::Left)),
            ((995.0, 300.0), Zone::WindowEdge(Direction::Right)),
            ((750.0, 10.0), Zone::WindowEdge(Direction::Up)),
            ((750.0, 595.0), Zone::WindowEdge(Direction::Down)),
            ((3.0, 4.0), Zone::WindowEdge(Direction::Left)),
            ((4.0, 3.0), Zone::WindowEdge(Direction::Up)),
            ((17.99, 300.0), Zone::WindowEdge(Direction::Left)),
            ((0.0, 0.0), Zone::WindowEdge(Direction::Left)),
        ] {
            assert_eq!(
                layout.zone_at(bounds, point, Some(1)),
                expected,
                "{point:?}"
            );
        }
        assert_ne!(
            layout.zone_at(bounds, (18.0, 300.0), Some(2)),
            Zone::WindowEdge(Direction::Left),
            "the strip is 18 points deep, no more"
        );
    }

    #[test]
    fn a_point_outside_the_area_asks_for_nothing() {
        let bounds = Rect::new(0.0, 0.0, 1000.0, 600.0);
        let layout = Tree::Leaf(1).layout(bounds, 1.0);
        for point in [
            (-1.0, 300.0),
            (1000.0, 300.0),
            (500.0, -0.5),
            (500.0, 600.0),
        ] {
            assert_eq!(
                layout.zone_at(bounds, point, None),
                Zone::Outside,
                "{point:?}"
            );
        }
        assert_eq!(
            Layout::default().zone_at(bounds, (500.0, 300.0), None),
            Zone::Outside
        );
    }

    #[test]
    fn a_point_over_a_pane_asks_for_the_edge_it_is_nearest() {
        // Pane 2 is x 501…1000; 1 is x 0…500.
        let bounds = Rect::new(0.0, 0.0, 1000.0, 600.0);
        let mut tree = Tree::Leaf(1);
        assert!(tree.split(1, Axis::Horizontal, 2));
        let layout = tree.layout(bounds, 1.0);
        for (point, expected) in [
            (
                (950.0, 300.0),
                Zone::Beside {
                    target: 2,
                    side: Direction::Right,
                },
            ),
            (
                (520.0, 300.0),
                Zone::Beside {
                    target: 2,
                    side: Direction::Left,
                },
            ),
            (
                (750.0, 40.0),
                Zone::Beside {
                    target: 2,
                    side: Direction::Up,
                },
            ),
            (
                (750.0, 560.0),
                Zone::Beside {
                    target: 2,
                    side: Direction::Down,
                },
            ),
            (
                (100.0, 300.0),
                Zone::Beside {
                    target: 1,
                    side: Direction::Left,
                },
            ),
        ] {
            assert_eq!(
                layout.zone_at(bounds, point, Some(9)),
                expected,
                "{point:?}"
            );
        }
    }

    #[test]
    fn the_middle_of_a_pane_swaps_only_within_its_own_tab() {
        let layout = Layout {
            panes: vec![(1, Rect::new(100.0, 100.0, 200.0, 100.0))],
            dividers: Vec::new(),
        };
        let bounds = Rect::new(0.0, 0.0, 500.0, 300.0);
        assert_eq!(SWAP_CORE, 0.28);
        // 28% of 200 is 56: at exactly that depth the point is still an edge's.
        assert_eq!(
            layout.zone_at(bounds, (156.0, 150.0), Some(7)),
            Zone::Beside {
                target: 1,
                side: Direction::Left
            }
        );
        assert_eq!(
            layout.zone_at(bounds, (157.0, 150.0), Some(7)),
            Zone::Swap { target: 1 }
        );
        assert_eq!(
            layout.zone_at(bounds, (200.0, 150.0), Some(7)),
            Zone::Swap { target: 1 }
        );
        // A pane from another tab has nothing to trade with: the middle is the nearest edge's.
        assert_eq!(
            layout.zone_at(bounds, (200.0, 150.0), None),
            Zone::Beside {
                target: 1,
                side: Direction::Left
            },
        );
        assert_eq!(
            layout.zone_at(bounds, (290.0, 150.0), None),
            Zone::Beside {
                target: 1,
                side: Direction::Right
            },
        );
    }

    #[test]
    fn the_carried_panes_own_place_asks_for_nothing() {
        let bounds = Rect::new(0.0, 0.0, 1000.0, 600.0);
        let mut tree = Tree::Leaf(1);
        assert!(tree.split(1, Axis::Horizontal, 2));
        let layout = tree.layout(bounds, 1.0);
        // Anywhere over it, edge halves and middle alike…
        for point in [(250.0, 300.0), (30.0, 300.0), (490.0, 300.0), (250.0, 30.0)] {
            assert_eq!(
                layout.zone_at(bounds, point, Some(1)),
                Zone::Own,
                "{point:?}"
            );
        }
        // …but not the strip along the window's edge, and not for a pane from elsewhere.
        assert_eq!(
            layout.zone_at(bounds, (5.0, 300.0), Some(1)),
            Zone::WindowEdge(Direction::Left)
        );
        assert_ne!(layout.zone_at(bounds, (250.0, 300.0), None), Zone::Own);
    }

    #[test]
    fn a_point_in_a_gap_belongs_to_the_nearest_pane() {
        let bounds = Rect::new(0.0, 0.0, 1000.0, 600.0);
        let mut tree = Tree::Leaf(1);
        assert!(tree.split(1, Axis::Horizontal, 2));
        let layout = tree.layout_spaced(bounds, 1.0, Spacing::gapped(6.0, 6.0, 1.0));
        let gap = layout.dividers[0].rect;
        assert_eq!(gap.width, 6.0);
        assert_eq!(
            layout.zone_at(bounds, (gap.x + 1.0, 300.0), Some(9)),
            Zone::Beside {
                target: 1,
                side: Direction::Right
            }
        );
        assert_eq!(
            layout.zone_at(bounds, (gap.x + 5.0, 300.0), Some(9)),
            Zone::Beside {
                target: 2,
                side: Direction::Left
            }
        );
    }

    #[test]
    fn a_slide_puts_the_final_frame_back_where_it_was() {
        let from = Rect::new(0.0, 0.0, 1000.0, 600.0);
        let to = Rect::new(6.0, 6.0, 494.0, 588.0);
        let moved = slide(from, to);
        // The scaled frame about its own centre, shifted: `from` again.
        let (cx, cy) = to.center();
        let width = to.width * moved.scale_x;
        let height = to.height * moved.scale_y;
        assert!((cx + moved.dx - width / 2.0 - from.x).abs() < 1e-9);
        assert!((cy + moved.dy - height / 2.0 - from.y).abs() < 1e-9);
        assert!((width - from.width).abs() < 1e-9);
        assert!((height - from.height).abs() < 1e-9);
        // A pane that does not move does not slide.
        assert_eq!(slide(to, to), Slide::NONE);
    }

    #[test]
    fn a_new_pane_grows_from_its_far_edge() {
        let to = Rect::new(503.0, 6.0, 491.0, 588.0);
        assert_eq!(
            sliver(to, Axis::Horizontal),
            Rect::new(994.0, 6.0, 0.0, 588.0)
        );
        assert_eq!(
            sliver(to, Axis::Vertical),
            Rect::new(503.0, 594.0, 491.0, 0.0)
        );
        // Nothing to divide by at rest: the zero-size end is the start, never the rest.
        let from = sliver(to, Axis::Horizontal);
        assert_eq!(slide(from, to).scale_x, 0.0);
        assert_eq!(slide(from, to).scale_y, 1.0);
        assert_eq!(slide(to, from).scale_x, 1.0);
    }

    #[test]
    fn a_subtrees_minimum_is_the_trees_own_arithmetic() {
        let min = min_100_by_60;
        let room = room_in(1000.0, 600.0, Spacing::DIVIDED, &min);
        let tree = pair(Axis::Horizontal, 0.5, 1, 2);
        assert_eq!(tree.min_length(Axis::Horizontal, &room), 201.0);
        assert_eq!(tree.min_length(Axis::Vertical, &room), 60.0);
        let room = room_in(1000.0, 600.0, Spacing::gapped(6.0, 6.0, 1.0), &min);
        assert_eq!(
            tree.min_length(Axis::Horizontal, &room),
            206.0,
            "the gap is the gap"
        );
    }

    // ----- the verdict -----

    fn stacked(count: u64) -> Tree {
        // Panes 1…count one above the other, equal.
        let mut tree = Tree::Leaf(1);
        for id in 2..=count {
            assert!(tree.split(id - 1, Axis::Vertical, id));
        }
        tree.equalize();
        tree
    }

    #[test]
    fn a_zone_names_what_it_asks_for() {
        let beside = |side| Zone::Beside { target: 1, side };
        assert_eq!(beside(Direction::Left).label(), Some("Left"));
        assert_eq!(beside(Direction::Right).label(), Some("Right"));
        assert_eq!(beside(Direction::Up).label(), Some("Above"));
        assert_eq!(beside(Direction::Down).label(), Some("Below"));
        assert_eq!(
            Zone::WindowEdge(Direction::Left).label(),
            Some("Full height")
        );
        assert_eq!(
            Zone::WindowEdge(Direction::Down).label(),
            Some("Full width")
        );
        assert_eq!(Zone::Swap { target: 2 }.label(), Some("Swap"));
        assert_eq!(Zone::Own.label(), None);
        assert_eq!(Zone::Outside.label(), None);
    }

    #[test]
    fn a_pane_over_the_edge_half_of_another_lands_there() {
        let min = min_100_by_60;
        let room = room_in(1001.0, 600.0, Spacing::DIVIDED, &min);
        let tree = pair(Axis::Horizontal, 0.5, 1, 2);
        // Pane 1 carried over the right edge of pane 2.
        let verdict = tree.verdict(&Tree::Leaf(1), &room, (950.0, 300.0));
        let Verdict::Lands { zone, placement } = verdict else {
            panic!("expected a landing, got {verdict:?}");
        };
        assert_eq!(
            zone,
            Zone::Beside {
                target: 2,
                side: Direction::Right
            }
        );
        assert_eq!(placement.tree, pair(Axis::Horizontal, 0.5, 2, 1));
        assert!(placement.landing.x > 500.0, "{:?}", placement.landing);
        assert!(!placement.made_room);
    }

    #[test]
    fn the_middle_of_another_pane_is_a_swap_that_asks_for_no_room() {
        let min = min_100_by_60;
        let room = room_in(1001.0, 600.0, Spacing::DIVIDED, &min);
        let tree = pair(Axis::Horizontal, 0.5, 1, 2);
        let verdict = tree.verdict(&Tree::Leaf(1), &room, (750.0, 300.0));
        let Verdict::Swaps {
            target,
            frame,
            fits,
        } = verdict
        else {
            panic!("expected a swap, got {verdict:?}");
        };
        assert_eq!(target, 2);
        assert!(frame.x > 500.0);
        assert!(fits);
    }

    #[test]
    fn a_swap_that_leaves_a_pane_under_its_minimum_says_so() {
        // Pane 1 is the narrow one (≈200 px) and pane 2 asks for 300: they cannot trade.
        let min = |id: u64| {
            if id == 2 {
                Size::new(300.0, 60.0)
            } else {
                Size::new(100.0, 60.0)
            }
        };
        let room = room_in(1001.0, 600.0, Spacing::DIVIDED, &min);
        let tree = pair(Axis::Horizontal, 0.2, 1, 2);
        let verdict = tree.verdict(&Tree::Leaf(1), &room, (600.0, 300.0));
        assert!(
            matches!(verdict, Verdict::Swaps { fits: false, .. }),
            "{verdict:?}"
        );
    }

    #[test]
    fn a_block_from_another_tab_has_no_swap() {
        let min = min_100_by_60;
        let room = room_in(1001.0, 600.0, Spacing::DIVIDED, &min);
        let tree = pair(Axis::Horizontal, 0.5, 1, 2);
        // The same point that swaps a pane of this tab: for pane 9 it is the nearest edge's half.
        let verdict = tree.verdict(&Tree::Leaf(9), &room, (750.0, 300.0));
        assert!(matches!(verdict, Verdict::Lands { .. }), "{verdict:?}");
    }

    #[test]
    fn the_carried_panes_own_place_and_the_outside_ask_for_nothing() {
        let min = min_100_by_60;
        let room = room_in(1001.0, 600.0, Spacing::DIVIDED, &min);
        let tree = pair(Axis::Horizontal, 0.5, 1, 2);
        assert_eq!(
            tree.verdict(&Tree::Leaf(1), &room, (250.0, 300.0)),
            Verdict::Nothing
        );
        assert_eq!(
            tree.verdict(&Tree::Leaf(1), &room, (-5.0, 300.0)),
            Verdict::Nothing
        );
    }

    #[test]
    fn a_tabs_only_pane_has_nowhere_to_go_inside_it() {
        let min = min_100_by_60;
        let room = room_in(1001.0, 600.0, Spacing::DIVIDED, &min);
        let tree = Tree::Leaf(1);
        for point in [(500.0, 300.0), (5.0, 300.0), (990.0, 20.0)] {
            let verdict = tree.verdict(&Tree::Leaf(1), &room, point);
            assert!(
                matches!(verdict, Verdict::Nothing),
                "{point:?}: {verdict:?}"
            );
        }
    }

    #[test]
    fn the_window_edge_asks_for_the_whole_edge_and_lands_there() {
        let min = min_100_by_60;
        let room = room_in(1001.0, 600.0, Spacing::DIVIDED, &min);
        let tree = pair(Axis::Horizontal, 0.5, 1, 2);
        // Pane 2 carried to the window's left edge strip.
        let verdict = tree.verdict(&Tree::Leaf(2), &room, (5.0, 300.0));
        let Verdict::Lands { zone, placement } = verdict else {
            panic!("expected a landing, got {verdict:?}");
        };
        assert_eq!(zone, Zone::WindowEdge(Direction::Left));
        assert_eq!(placement.landing.x, 0.0);
        assert_eq!(placement.landing.height, 600.0);
    }

    #[test]
    fn a_block_that_does_not_fit_there_is_refused_with_the_edges_that_would_take_it() {
        // Four panes stacked in 300 px with a 60 px minimum: a fifth cannot go above or below
        // anything, but a column on either side takes it.
        let min = min_100_by_60;
        let room = room_in(1001.0, 300.0, Spacing::DIVIDED, &min);
        let tree = stacked(4);
        let layout = tree.layout_spaced(room.bounds, room.scale, room.spacing);
        let top = frame_of(&layout, 2).y + 4.0;
        let verdict = tree.verdict(&Tree::Leaf(5), &room, (500.0, top));
        let Verdict::TooSmall {
            zone,
            region,
            edges,
        } = verdict
        else {
            panic!("expected a refusal, got {verdict:?}");
        };
        assert_eq!(
            zone,
            Zone::Beside {
                target: 2,
                side: Direction::Up
            }
        );
        let pane = frame_of(&layout, 2);
        assert_eq!(region.y, pane.y);
        assert!((region.height - pane.height * PANE_EDGE_SHARE).abs() < 1e-9);
        let sides: Vec<Direction> = edges.iter().map(|(side, _)| *side).collect();
        assert_eq!(sides, [Direction::Left, Direction::Right]);
    }

    #[test]
    fn a_block_that_fits_nowhere_says_there_is_no_room() {
        let min = |id: u64| {
            if id == 5 {
                Size::new(2000.0, 60.0)
            } else {
                Size::new(100.0, 60.0)
            }
        };
        let room = room_in(1001.0, 300.0, Spacing::DIVIDED, &min);
        let tree = stacked(4);
        let verdict = tree.verdict(&Tree::Leaf(5), &room, (500.0, 100.0));
        assert_eq!(verdict, Verdict::NoRoom);
    }
}
