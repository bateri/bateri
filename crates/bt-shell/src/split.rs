//! Split layout: the **pure** binary tree that carries a tab's panes (039 Karar 6). A leaf is a
//! pane identity (`TerminalPane::id`), a node is an axis and a ratio. Splitting, closing and the
//! frame computation are each a tree operation; the AppKit part (`split_view`) only applies these
//! frames to the panes and paints the dividers.
//!
//! It does not see AppKit and has its own tests (the `quote`/`upload`/`zoom` precedent).
//! Coordinates are **top-down** (the container is `isFlipped`): the second leaf of "split down" is
//! below, so no sign needs flipping.
//!
//! Navigation (order and direction), resizing (keyboard step and divider drag), equalizing and
//! zooming are tree operations too (039 phase-4). The **minimum pane** (Karar 14) is given to the
//! tree as one size per leaf (`min`): the point-size delta is per pane, and so is the cell; the
//! limit's source is the pane itself.

/// The split's axis.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Axis {
    /// Side by side — Split Right (⌘D): the second leaf is on the right.
    Horizontal,
    /// Stacked — Split Down (⇧⌘D): the second leaf is below.
    Vertical,
}

/// The direction of navigation and resizing (⌥⌘ / ⌃⌘ + arrow).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Direction {
    Left,
    Right,
    Up,
    Down,
}

impl Direction {
    /// The direction from a menu item's `tag` (`menu`'s Select/Resize Split ▸ items); an unknown
    /// `tag` is `None`.
    pub(crate) fn from_tag(tag: isize) -> Option<Self> {
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
pub(crate) struct Size {
    pub(crate) width: f64,
    pub(crate) height: f64,
}

impl Size {
    pub(crate) const fn new(width: f64, height: f64) -> Self {
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
pub(crate) struct Rect {
    pub(crate) x: f64,
    pub(crate) y: f64,
    pub(crate) width: f64,
    pub(crate) height: f64,
}

impl Rect {
    pub(crate) const fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
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
/// Retina. Not measured, a design constant (039 Karar
/// 7: "the divider is one pixel"); the same weight as the dock's hairlines.
const DIVIDER_PX: f64 = 1.0;

/// Splits a leaf's frame in two along `axis`: the first half, the divider and the second half —
/// in pixels, with the divider subtracted. The ratio is the first half's share.
///
/// The boundary is snapped to a **whole pixel**: a pane sitting on a half pixel gets a fractional
/// drawable and its text blurs (the tab bar symptom of 026 phase-4). If the input is already on
/// whole pixels, all three outputs are whole pixels too and tile the input with no gap and no
/// overlap.
fn halves_px(rect: Rect, axis: Axis, ratio: f64) -> (Rect, Rect, Rect) {
    let span = match axis {
        Axis::Horizontal => rect.width,
        Axis::Vertical => rect.height,
    };
    let available = (span - DIVIDER_PX).max(0.0);
    let first = (available * ratio).round().clamp(0.0, available);
    let second = available - first;
    match axis {
        Axis::Horizontal => (
            Rect::new(rect.x, rect.y, first, rect.height),
            Rect::new(rect.x + first, rect.y, DIVIDER_PX, rect.height),
            Rect::new(rect.x + first + DIVIDER_PX, rect.y, second, rect.height),
        ),
        Axis::Vertical => (
            Rect::new(rect.x, rect.y, rect.width, first),
            Rect::new(rect.x, rect.y + first, rect.width, DIVIDER_PX),
            Rect::new(rect.x, rect.y + first + DIVIDER_PX, rect.width, second),
        ),
    }
}

/// The sizes of a pane's two halves if it were split, in points — the question of the split limit
/// (039 Karar 14): the **same** arithmetic as the frame computation, so the half the check approves
/// is exactly the half that will be drawn.
pub(crate) fn split_halves(frame: Rect, axis: Axis, scale: f64) -> (Rect, Rect) {
    let (first, _, second) = halves_px(snap(frame, scale), axis, 0.5);
    (first.scaled(1.0 / scale), second.scaled(1.0 / scale))
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

/// The split tree.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Tree {
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
pub(crate) enum Removal {
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
pub(crate) struct Layout {
    /// Pane identity and its frame, in tree order.
    pub(crate) panes: Vec<(u64, Rect)>,
    /// Dividers, in the tree's **in-order** arrangement (the first subtree's, the node's, the
    /// second's): [`Tree::drag`]'s index is this order.
    pub(crate) dividers: Vec<Divider>,
}

/// A divider: its line and the axis of the split it divides (a side-by-side split's divider is a
/// vertical line, dragged horizontally).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Divider {
    pub(crate) rect: Rect,
    pub(crate) axis: Axis,
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
    pub(crate) fn neighbour(&self, from: u64, direction: Direction) -> Option<u64> {
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
    pub(crate) fn leaves(&self) -> Vec<u64> {
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
    pub(crate) fn cycle(&self, from: u64, forward: bool) -> Option<u64> {
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
    /// `new` in the second, equal area (039 Karar 9).
    /// If the leaf is missing, `false` and the tree does not change.
    pub(crate) fn split(&mut self, target: u64, axis: Axis, new: u64) -> bool {
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
    pub(crate) fn remove(&mut self, target: u64) -> Removal {
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
    pub(crate) fn layout(&self, bounds: Rect, scale: f64) -> Layout {
        let mut out = Layout::default();
        self.place(snap(bounds, scale), &mut out);
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
    pub(crate) fn layout_zoomed(&self, bounds: Rect, scale: f64, zoomed: Option<u64>) -> Layout {
        match zoomed {
            Some(id) if self.leaves().contains(&id) => Layout {
                panes: vec![(id, snap(bounds, scale).scaled(1.0 / scale))],
                dividers: Vec::new(),
            },
            _ => self.layout(bounds, scale),
        }
    }

    /// ⌃⌘ + arrow: moves the divider of `target`'s **nearest ancestor** on `direction`'s axis by
    /// `step` points in that direction (Ghostty's behaviour: the arrow is the direction the
    /// divider goes, not the growing pane's). Both sides are clamped at the minimum pane limit
    /// ([`place_divider`]). `false` if there is no ancestor on that axis, the divider is already
    /// at the limit, or the leaf is not in the tree.
    pub(crate) fn resize(
        &mut self,
        target: u64,
        direction: Direction,
        step: f64,
        bounds: Rect,
        scale: f64,
        min: &dyn Fn(u64) -> Size,
    ) -> bool {
        let step_px = (step * scale).round();
        let delta = if direction.forward() {
            step_px
        } else {
            -step_px
        };
        let limits = Limits { min, scale };
        matches!(
            resize_in(
                self,
                snap(bounds, scale),
                target,
                direction.axis(),
                delta,
                &limits
            ),
            Found::Done(true)
        )
    }

    /// Divider drag: moves the `index`th divider of [`Layout::dividers`] to `position` (in the
    /// container's coordinates, in points, along the divider's axis), clamping at the limit.
    /// `true` if the position changed.
    pub(crate) fn drag(
        &mut self,
        index: usize,
        position: f64,
        bounds: Rect,
        scale: f64,
        min: &dyn Fn(u64) -> Size,
    ) -> bool {
        let limits = Limits { min, scale };
        let mut index = index;
        drag_in(
            self,
            snap(bounds, scale),
            &mut index,
            position * scale,
            &limits,
        ) == Some(true)
    }

    /// ⌃⌘=: each node's ratio comes from its subtrees' pane count **on that axis** — splits
    /// chained on the same axis count their children, a subtree on the other axis is a single
    /// column (or row). Result: all panes on the same axis are equal (in an L layout the left pane
    /// is half width, not a third).
    pub(crate) fn equalize(&mut self) {
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
                    (a / r).max(b / (1.0 - r)).ceil() + DIVIDER_PX
                } else {
                    a.max(b)
                }
            }
        }
    }

    fn place(&self, rect: Rect, out: &mut Layout) {
        match self {
            Tree::Leaf(id) => out.panes.push((*id, rect)),
            Tree::Split {
                axis,
                ratio,
                first,
                second,
            } => {
                let (a, divider, b) = halves_px(rect, *axis, *ratio);
                first.place(a, out);
                out.dividers.push(Divider {
                    rect: divider,
                    axis: *axis,
                });
                second.place(b, out);
            }
        }
    }
}

/// The resizing limit: the minimum size per leaf (points) and the scale.
struct Limits<'a> {
    min: &'a dyn Fn(u64) -> Size,
    scale: f64,
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
    let available = (span - DIVIDER_PX).max(0.0);
    if available <= 0.0 {
        return false;
    }
    let low = first.min_px(axis, limits);
    let high = available - second.min_px(axis, limits);
    if low > high {
        return false;
    }
    let current = (available * *ratio).round().clamp(0.0, available);
    let target = desired.round().clamp(low, high);
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
            let (a, _, b) = halves_px(rect, *own, *ratio);
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
    let (a, _, b) = halves_px(rect, *axis, *ratio);
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

#[cfg(test)]
mod tests {
    use super::{Axis, Direction, Divider, Layout, Rect, Removal, Size, Tree, split_halves};

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

    #[test]
    fn split_halves_match_the_drawn_frames() {
        // The halves the split limit asks about are the same as the frames to be drawn.
        let bounds = Rect::new(0.0, 0.0, 700.5, 400.0);
        let (first, second) = split_halves(bounds, Axis::Horizontal, 2.0);
        let mut tree = Tree::Leaf(1);
        assert!(tree.split(1, Axis::Horizontal, 2));
        let layout = tree.layout(bounds, 2.0);
        assert_eq!(layout.panes[0].1, first);
        assert_eq!(layout.panes[1].1, second);
    }
}
