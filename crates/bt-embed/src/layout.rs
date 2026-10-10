//! The layout engine behind the C interface: the same rules bateri's own windows follow for
//! moving panes and tabs, for a host that keeps its windows, tabs and splits in a model of its own.
//!
//! **The host's model is the one truth.** The engine keeps nothing between calls. Before it asks,
//! the host paints a picture of what it holds ([`BtWorld`]: windows, their strips, every tab's
//! split tree with its focused pane, each tab's area and spacing, each pane's smallest size) and
//! asks as many questions of it as it likes — a drag asks for a verdict on every pointer move of
//! one picture. A picture changes only by the host's own setters; it is painted again once the
//! host's model changed.
//!
//! **A plan is carried out by the host.** [`bt_plan_new`] answers a move ([`BtMove`]) with a
//! refusal or a plan of flat, tagged steps in two parts: the main steps move panes and tabs, the
//! after steps say what comes forward. A main step the host does not know makes it refuse the
//! whole plan — skipping one would leave its model wrong; an after step it does not know is safely
//! skipped. A tree or a strip a step carries is lent by the plan and lives as long as it.
//!
//! **Identities are the host's.** Windows, tabs and panes are named by the host's numbers, with no
//! assumption about their range or order. A tab a move makes is given the picture's next identity
//! ([`bt_world_new`]), then the one after it, and the plan says how many it used.
//!
//! **Geometry is points, top-down**: an area's origin is its top-left corner and `y` grows
//! downward, the frame every rectangle here is in. The engine needs no thread of its own: a
//! picture, a plan or a tree may be used from any thread, never from two at once.

use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::ffi::{CStr, CString, c_char};
use std::ptr::{null, null_mut};

use bt_shell_common::moves::{self, Host, Joins, Move, Plan, Refusal, Slot, Step, Window};
use bt_shell_common::split::{
    Axis, Direction, Layout, Rect, Room, Size, Spacing, Tree, Verdict, Zone,
};
use bt_shell_common::tabs::Tabs;
use bt_shell_common::tree_text;
use bt_shell_common::undo::{Record, Shape};

use crate::{guarded, handed, text};

/// The part of a plan a step is in ([`bt_plan_step_count`]).
pub mod part {
    /// The steps that move panes and tabs. An unknown kind here refuses the plan.
    pub const MAIN: u32 = 1;
    /// The steps that follow once the panes have landed. An unknown kind here is skipped.
    pub const AFTER: u32 = 2;
}

/// A step's kind ([`bt_plan_step_kind`]); what each fills is said in the header.
pub mod step {
    pub const RELEASE_PANE: u32 = 1;
    pub const RELEASE_TAB: u32 = 2;
    pub const UNPACK: u32 = 3;
    pub const FOLD: u32 = 4;
    pub const NEW_TAB: u32 = 5;
    pub const WRAP: u32 = 6;
    pub const ADOPT_TAB: u32 = 7;
    pub const MOVE_TAB: u32 = 8;
    pub const DISSOLVE: u32 = 9;
    pub const RESHAPE: u32 = 10;
    pub const PUT_STRIP: u32 = 11;
    pub const FIT: u32 = 12;
    pub const UNDONE: u32 = 13;
    pub const OPEN_WINDOW: u32 = 14;
    pub const ADOPT_PANES: u32 = 15;
    pub const CLOSE_IF_EMPTIED: u32 = 16;
    pub const RAISE: u32 = 17;
    pub const SELECT: u32 = 18;
    pub const FOCUS: u32 = 19;
    pub const PULSE: u32 = 20;
}

/// Why nothing moves ([`bt_plan_new`]); 0 when a plan came.
pub mod refusal {
    /// The move names nothing there is.
    pub const QUIET: i32 = 1;
    /// It cannot be done now (a window holds a question, the panes do not fit): a beep says so.
    pub const BEEP: i32 = 2;
    /// Undo Move's picture is no longer what is there: a beep, and the record is dropped.
    pub const STALE: i32 = 3;
}

/// A split's axis.
pub mod axis {
    /// Side by side: the second subtree is on the right.
    pub const HORIZONTAL: u32 = 1;
    /// Stacked: the second subtree is below.
    pub const VERTICAL: u32 = 2;
}

/// A side of a pane or of an area.
pub mod side {
    pub const LEFT: u32 = 1;
    pub const RIGHT: u32 = 2;
    pub const UP: u32 = 3;
    pub const DOWN: u32 = 4;
}

/// Where a tab joining a strip goes ([`bt_plan_step_slot`]).
pub mod slot {
    /// Before the tab at [`bt_plan_step_index`], and it comes up selected.
    pub const AT: u32 = 1;
    /// At the end, the selection where it was.
    pub const END: u32 = 2;
}

/// What a carried block shows under the pointer ([`bt_verdict_kind`]).
pub mod verdict {
    pub const NOTHING: u32 = 1;
    pub const LANDS: u32 = 2;
    pub const SWAPS: u32 = 3;
    pub const TOO_SMALL: u32 = 4;
    pub const NO_ROOM: u32 = 5;
}

/// What the pointer asks for ([`bt_verdict_zone`]); 0 for none.
pub mod zone {
    /// The strip along the area's edge: a full-length column or row on that side.
    pub const WINDOW_EDGE: u32 = 1;
    /// The half of a pane nearest the pointer: beside it, on that side.
    pub const BESIDE: u32 = 2;
    /// The middle of a pane: the two trade places.
    pub const SWAP: u32 = 3;
    /// Over the carried pane's own place.
    pub const OWN: u32 = 4;
    /// Over no pane at all.
    pub const OUTSIDE: u32 = 5;
}

/// Which layout a tab's spacing is for ([`bt_world_set_spacing`]).
pub mod spacing {
    /// A tab of one pane.
    pub const LONE: u32 = 1;
    /// A tab of several.
    pub const SPLIT: u32 = 2;
}

fn axis_of(code: u32) -> Option<Axis> {
    match code {
        axis::HORIZONTAL => Some(Axis::Horizontal),
        axis::VERTICAL => Some(Axis::Vertical),
        _ => None,
    }
}

fn axis_code(axis: Axis) -> u32 {
    match axis {
        Axis::Horizontal => axis::HORIZONTAL,
        Axis::Vertical => axis::VERTICAL,
    }
}

fn side_of(code: u32) -> Option<Direction> {
    match code {
        side::LEFT => Some(Direction::Left),
        side::RIGHT => Some(Direction::Right),
        side::UP => Some(Direction::Up),
        side::DOWN => Some(Direction::Down),
        _ => None,
    }
}

fn side_code(direction: Direction) -> u32 {
    match direction {
        Direction::Left => side::LEFT,
        Direction::Right => side::RIGHT,
        Direction::Up => side::UP,
        Direction::Down => side::DOWN,
    }
}

/// Writes `rect` where the four pointers point, those that are not NULL.
///
/// # Safety
/// Each pointer is NULL or writable.
unsafe fn write_rect(rect: Rect, x: *mut f64, y: *mut f64, width: *mut f64, height: *mut f64) {
    for (out, value) in [
        (x, rect.x),
        (y, rect.y),
        (width, rect.width),
        (height, rect.height),
    ] {
        if !out.is_null() {
            // SAFETY: the caller's promise.
            unsafe { out.write(value) };
        }
    }
}

/// Writes `value` where `out` points, if it points anywhere.
///
/// # Safety
/// `out` is NULL or writable.
unsafe fn put<T>(out: *mut T, value: T) {
    if !out.is_null() {
        // SAFETY: the caller's promise.
        unsafe { out.write(value) };
    }
}

/// Hands `value` to the caller as an owned pointer, freed by its `_free` function.
fn owned<T>(value: T) -> *mut T {
    Box::into_raw(Box::new(value))
}

/// Takes back an owned pointer and drops it; NULL is ignored.
///
/// # Safety
/// `pointer` is NULL or came from [`owned`] with the same type, not yet freed.
unsafe fn release<T>(pointer: *mut T) {
    if !pointer.is_null() {
        // SAFETY: the caller's promise.
        drop(unsafe { Box::from_raw(pointer) });
    }
}

// ─── trees ───────────────────────────────────────────────────────────────

/// A split tree: a pane, or two subtrees side by side or stacked with the first one's share.
/// Owned ones are the caller's to free; lent ones (a subtree, a plan's or a verdict's) live as
/// long as what lent them.
#[repr(transparent)]
pub struct BtTree(Tree);

impl BtTree {
    fn lend(tree: &Tree) -> *const BtTree {
        std::ptr::from_ref(tree).cast()
    }
}

/// Runs `body` on the tree `tree` points at; `failed` if it is NULL or `body` panics.
///
/// # Safety
/// `tree` is NULL or a live tree.
unsafe fn with_tree<T: Copy>(tree: *const BtTree, failed: T, body: impl FnOnce(&Tree) -> T) -> T {
    guarded(failed, || {
        // SAFETY: the caller's promise.
        unsafe { tree.as_ref() }.map_or(failed, |tree| body(&tree.0))
    })
}

/// A tree of one pane, `pane`.
#[unsafe(no_mangle)]
pub extern "C" fn bt_tree_leaf(pane: u64) -> *mut BtTree {
    guarded(null_mut(), || owned(BtTree(Tree::Leaf(pane))))
}

/// Two trees side by side or stacked (`axis`), the first taking `ratio` of the length, strictly
/// between 0 and 1. Consumes `first` and `second` whatever the outcome — they become part of the
/// new tree. NULL for an unknown axis, a ratio out of range, a NULL tree, or a pane in both.
///
/// # Safety
/// `first` and `second` are NULL or owned trees, not used again.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_tree_split(
    axis: u32,
    ratio: f64,
    first: *mut BtTree,
    second: *mut BtTree,
) -> *mut BtTree {
    guarded(null_mut(), || {
        // SAFETY: the caller's promise; both are consumed here.
        let first = (!first.is_null()).then(|| unsafe { Box::from_raw(first) });
        // SAFETY: as above.
        let second = (!second.is_null()).then(|| unsafe { Box::from_raw(second) });
        let (Some(axis), Some(first), Some(second)) = (axis_of(axis), first, second) else {
            return null_mut();
        };
        if !(ratio.is_finite() && ratio > 0.0 && ratio < 1.0) {
            return null_mut();
        }
        let theirs: HashSet<u64> = first.0.leaves().into_iter().collect();
        if second.0.leaves().iter().any(|pane| theirs.contains(pane)) {
            return null_mut();
        }
        owned(BtTree(Tree::Split {
            axis,
            ratio,
            first: Box::new(first.0),
            second: Box::new(second.0),
        }))
    })
}

/// A copy of `tree`, the caller's — to keep a lent one past its lender.
///
/// # Safety
/// `tree` is NULL or a live tree — for every `bt_tree_*` query.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_tree_copy(tree: *const BtTree) -> *mut BtTree {
    // SAFETY: the caller's promise.
    unsafe { with_tree(tree, null_mut(), |tree| owned(BtTree(tree.clone()))) }
}

/// Frees an owned tree. NULL is ignored; a lent one is never freed.
///
/// # Safety
/// `tree` is NULL or an owned tree, not yet freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_tree_free(tree: *mut BtTree) {
    // SAFETY: the caller's promise.
    guarded((), || unsafe { release(tree) });
}

/// Whether `tree` is a single pane.
///
/// # Safety
/// As [`bt_tree_copy`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_tree_is_leaf(tree: *const BtTree) -> bool {
    // SAFETY: the caller's promise.
    unsafe { with_tree(tree, false, |tree| matches!(tree, Tree::Leaf(_))) }
}

/// The pane of a single-pane tree; 0 for a split or NULL — ask [`bt_tree_is_leaf`] first.
///
/// # Safety
/// As [`bt_tree_copy`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_tree_pane(tree: *const BtTree) -> u64 {
    // SAFETY: the caller's promise.
    unsafe {
        with_tree(tree, 0, |tree| match tree {
            Tree::Leaf(pane) => *pane,
            Tree::Split { .. } => 0,
        })
    }
}

/// A split's axis; 0 for a single pane.
///
/// # Safety
/// As [`bt_tree_copy`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_tree_axis(tree: *const BtTree) -> u32 {
    // SAFETY: the caller's promise.
    unsafe {
        with_tree(tree, 0, |tree| match tree {
            Tree::Split { axis, .. } => axis_code(*axis),
            Tree::Leaf(_) => 0,
        })
    }
}

/// A split's first share, strictly between 0 and 1; 0 for a single pane.
///
/// # Safety
/// As [`bt_tree_copy`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_tree_ratio(tree: *const BtTree) -> f64 {
    // SAFETY: the caller's promise.
    unsafe {
        with_tree(tree, 0.0, |tree| match tree {
            Tree::Split { ratio, .. } => *ratio,
            Tree::Leaf(_) => 0.0,
        })
    }
}

/// A split's first subtree (left or top), lent by `tree`; NULL for a single pane.
///
/// # Safety
/// As [`bt_tree_copy`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_tree_first(tree: *const BtTree) -> *const BtTree {
    // SAFETY: the caller's promise.
    unsafe {
        with_tree(tree, null(), |tree| match tree {
            Tree::Split { first, .. } => BtTree::lend(first),
            Tree::Leaf(_) => null(),
        })
    }
}

/// A split's second subtree (right or bottom), lent by `tree`; NULL for a single pane.
///
/// # Safety
/// As [`bt_tree_copy`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_tree_second(tree: *const BtTree) -> *const BtTree {
    // SAFETY: the caller's promise.
    unsafe {
        with_tree(tree, null(), |tree| match tree {
            Tree::Split { second, .. } => BtTree::lend(second),
            Tree::Leaf(_) => null(),
        })
    }
}

/// How many panes `tree` holds.
///
/// # Safety
/// As [`bt_tree_copy`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_tree_pane_count(tree: *const BtTree) -> usize {
    // SAFETY: the caller's promise.
    unsafe { with_tree(tree, 0, |tree| tree.leaves().len()) }
}

/// The `index`th pane of `tree` in tree order (left to right, top to bottom); 0 past the end.
///
/// # Safety
/// As [`bt_tree_copy`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_tree_pane_at(tree: *const BtTree, index: usize) -> u64 {
    // SAFETY: the caller's promise.
    unsafe {
        with_tree(tree, 0, |tree| {
            tree.leaves().get(index).copied().unwrap_or(0)
        })
    }
}

/// `tree` with every split's ratio evened out by its panes on that axis (bateri's Equalize
/// Splits), the caller's.
///
/// # Safety
/// As [`bt_tree_copy`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_tree_equalized(tree: *const BtTree) -> *mut BtTree {
    // SAFETY: the caller's promise.
    unsafe {
        with_tree(tree, null_mut(), |tree| {
            let mut even = tree.clone();
            even.equalize();
            owned(BtTree(even))
        })
    }
}

/// `tree` as versioned text, the caller's to free — for keeping a layout on disk. Every later
/// build reads it ([`bt_tree_decode`]). NULL for a tree holding a pane twice.
///
/// # Safety
/// As [`bt_tree_copy`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_tree_encode(tree: *const BtTree) -> *mut c_char {
    // SAFETY: the caller's promise.
    unsafe {
        with_tree(tree, null_mut(), |tree| {
            tree_text::encode(tree).map_or(null_mut(), |text| handed(text.as_bytes()))
        })
    }
}

/// The tree `text` holds, written by this build or an earlier one, the caller's; NULL if it is
/// not one (a newer build's text included).
///
/// # Safety
/// `text` is NULL or a NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_tree_decode(text_in: *const c_char) -> *mut BtTree {
    guarded(null_mut(), || {
        // SAFETY: the caller's promise.
        unsafe { text(text_in) }
            .and_then(tree_text::decode)
            .map_or(null_mut(), |tree| owned(BtTree(tree)))
    })
}

// ─── the picture ─────────────────────────────────────────────────────────

/// A tab's ground: where its panes are laid out, at what scale, and the spacing for one pane and
/// for several, in points.
#[derive(Clone, Copy)]
struct Area {
    bounds: Rect,
    scale: f64,
    lone: Option<(f64, f64, f64)>,
    split: Option<(f64, f64, f64)>,
}

impl Area {
    fn spacing(&self, panes: usize) -> Spacing {
        let points = if panes > 1 { self.split } else { self.lone };
        points.map_or(Spacing::DIVIDED, |(between, around, top)| {
            Spacing::gapped(between, around, self.scale).with_top(top, self.scale)
        })
    }
}

/// What the host holds, painted for the engine's questions: windows, strips, tabs and their
/// trees, the tabs' areas and the panes' smallest sizes, and the next identity a move may give.
pub struct BtWorld {
    windows: Vec<Window>,
    next_id: u64,
    areas: HashMap<u64, Area>,
    minimums: HashMap<u64, Size>,
}

impl BtWorld {
    fn shape(&self, tab: u64) -> Option<&Shape> {
        self.windows.iter().find_map(|window| window.tab(tab))
    }

    fn window_of(&self, tab: u64) -> Option<&Window> {
        self.windows.iter().find(|window| window.tab(tab).is_some())
    }

    fn min(&self, pane: u64) -> Size {
        self.minimums
            .get(&pane)
            .copied()
            .unwrap_or(Size::new(0.0, 0.0))
    }

    /// Tab `tab`'s room for a tree of `panes` panes; `None` if the host gave it no area.
    fn room<'a>(&self, tab: u64, panes: usize, min: &'a dyn Fn(u64) -> Size) -> Option<Room<'a>> {
        let area = self.areas.get(&tab)?;
        Some(Room {
            bounds: area.bounds,
            scale: area.scale,
            spacing: area.spacing(panes),
            min,
        })
    }

    fn holds_pane(&self, pane: u64) -> bool {
        self.windows
            .iter()
            .flat_map(|window| &window.tabs)
            .any(|shape| shape.tree.leaves().contains(&pane))
    }
}

/// The engine's questions answered from a picture: room beside a pane from the areas and the
/// smallest sizes the host gave, new identities counted from the picture's next one.
struct Engine<'a> {
    world: &'a BtWorld,
    next: Cell<u64>,
}

impl Host for Engine<'_> {
    fn beside(&self, tab: u64, side: Direction, incoming: &Tree) -> Option<Tree> {
        let shape = self.world.shape(tab)?;
        let min = |pane: u64| self.world.min(pane);
        let panes = shape.tree.leaves().len() + incoming.leaves().len();
        let room = self.world.room(tab, panes, &min)?;
        shape
            .tree
            .plan_beside(shape.focus, side, incoming, &room)
            .map(|placement| placement.tree)
    }

    fn fresh_id(&self) -> u64 {
        let id = self.next.get();
        self.next.set(id.wrapping_add(1));
        id
    }
}

/// Runs `body` on the picture `world` points at; `failed` if it is NULL or `body` panics.
///
/// # Safety
/// `world` is NULL or a live picture.
unsafe fn with_world<T: Copy>(
    world: *const BtWorld,
    failed: T,
    body: impl FnOnce(&BtWorld) -> T,
) -> T {
    guarded(failed, || {
        // SAFETY: the caller's promise.
        unsafe { world.as_ref() }.map_or(failed, body)
    })
}

/// [`with_world`] for the setters.
///
/// # Safety
/// As [`with_world`], and not in use elsewhere.
unsafe fn with_world_mut(world: *mut BtWorld, body: impl FnOnce(&mut BtWorld) -> bool) -> bool {
    guarded(false, || {
        // SAFETY: the caller's promise.
        unsafe { world.as_mut() }.is_some_and(body)
    })
}

/// An empty picture. A tab a move makes takes `next_id`, the next one `next_id + 1`, and so on
/// ([`bt_plan_ids_used`]).
#[unsafe(no_mangle)]
pub extern "C" fn bt_world_new(next_id: u64) -> *mut BtWorld {
    guarded(null_mut(), || {
        owned(BtWorld {
            windows: Vec::new(),
            next_id,
            areas: HashMap::new(),
            minimums: HashMap::new(),
        })
    })
}

/// Frees a picture. NULL is ignored.
///
/// # Safety
/// `world` is NULL or a picture from [`bt_world_new`], not yet freed — for every `bt_world_*`
/// call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_world_free(world: *mut BtWorld) {
    // SAFETY: the caller's promise.
    guarded((), || unsafe { release(world) });
}

/// Adds window `window`, with no tab yet. `stays_empty`: it outlives its tabs — a move that takes
/// its last one leaves it open (bateri's windows close). `asking`: it holds a question of its own
/// that blocks all of it; nothing moves into or out of it. `false` if the window is there already.
///
/// # Safety
/// As [`bt_world_free`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_world_add_window(
    world: *mut BtWorld,
    window: u64,
    stays_empty: bool,
    asking: bool,
) -> bool {
    // SAFETY: the caller's promise.
    unsafe {
        with_world_mut(world, |world| {
            if world.windows.iter().any(|known| known.id == window) {
                return false;
            }
            world.windows.push(Window {
                id: window,
                order: Tabs::empty(),
                tabs: Vec::new(),
                asking,
                stays_empty,
            });
            true
        })
    }
}

/// Adds tab `tab` at the end of window `window`'s strip: its split tree (copied), the pane the
/// keyboard is in (`focus`, one of the tree's), its name (NULL: none) and whether it is the one
/// on screen — a window's first tab is, unless another is said to be. `false` if the window is
/// not there, the tab is, the focus is not in the tree, or a pane of the tree is in another tab.
///
/// # Safety
/// As [`bt_world_free`]; `tree` a live tree; `name` NULL or a NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_world_add_tab(
    world: *mut BtWorld,
    window: u64,
    tab: u64,
    tree: *const BtTree,
    focus: u64,
    name: *const c_char,
    selected: bool,
) -> bool {
    // SAFETY: the caller's promises.
    unsafe {
        with_world_mut(world, |world| {
            let Some(tree) = tree.as_ref().map(|tree| tree.0.clone()) else {
                return false;
            };
            let name = if name.is_null() {
                None
            } else {
                match text(name) {
                    Some(name) => Some(name.to_owned()),
                    None => return false,
                }
            };
            let panes = tree.leaves();
            if !panes.contains(&focus)
                || world.shape(tab).is_some()
                || panes.iter().any(|&pane| world.holds_pane(pane))
            {
                return false;
            }
            let Some(window) = world.windows.iter_mut().find(|known| known.id == window) else {
                return false;
            };
            window.order.append(tab);
            if selected {
                window.order.select(tab);
            }
            window.tabs.push(Shape {
                tab,
                name,
                tree,
                focus,
            });
            true
        })
    }
}

/// Tab `tab`'s area — where its panes are laid out, in points, top-down — and its screen's scale
/// (2 on a Retina screen). A move beside a pane and a verdict need it; without it they find no
/// room.
///
/// # Safety
/// As [`bt_world_free`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_world_set_area(
    world: *mut BtWorld,
    tab: u64,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    scale: f64,
) -> bool {
    // SAFETY: the caller's promise.
    unsafe {
        with_world_mut(world, |world| {
            if !(scale.is_finite() && scale > 0.0 && width >= 0.0 && height >= 0.0) {
                return false;
            }
            let area = world.areas.entry(tab).or_insert(Area {
                bounds: Rect::new(x, y, width, height),
                scale,
                lone: None,
                split: None,
            });
            area.bounds = Rect::new(x, y, width, height);
            area.scale = scale;
            true
        })
    }
}

/// The space tab `tab` leaves between its panes and around them, in points, for one pane or for
/// several (`panes`: a `BT_SPACING_*`): `between` panes, `around` them and at the `top` edge. Not
/// given: a one-pixel divider and no margin. Set after the tab's area.
///
/// # Safety
/// As [`bt_world_free`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_world_set_spacing(
    world: *mut BtWorld,
    tab: u64,
    panes: u32,
    between: f64,
    around: f64,
    top: f64,
) -> bool {
    // SAFETY: the caller's promise.
    unsafe {
        with_world_mut(world, |world| {
            let Some(area) = world.areas.get_mut(&tab) else {
                return false;
            };
            let points = Some((between, around, top));
            match panes {
                spacing::LONE => area.lone = points,
                spacing::SPLIT => area.split = points,
                _ => return false,
            }
            true
        })
    }
}

/// Pane `pane`'s smallest size, in points: no move, swap or divider takes it below. Not given:
/// none. A terminal pane's comes from `bt_pane_min_size`.
///
/// # Safety
/// As [`bt_world_free`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_world_set_minimum(
    world: *mut BtWorld,
    pane: u64,
    width: f64,
    height: f64,
) -> bool {
    // SAFETY: the caller's promise.
    unsafe {
        with_world_mut(world, |world| {
            world.minimums.insert(pane, Size::new(width, height));
            true
        })
    }
}

/// Whether `tree` (laid out in tab `tab`'s area) keeps every pane at its smallest size — the check
/// bateri makes once more after taking a new tree for a tab, a pane carried within it included:
/// a LANDS verdict keeps room around the landing, but a tab that no longer fit (its window
/// shrank) is refused. `false` without the tab's area.
///
/// # Safety
/// As [`bt_world_free`]; `tree` NULL or a live tree.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_world_tree_fits(
    world: *const BtWorld,
    tab: u64,
    tree: *const BtTree,
) -> bool {
    guarded(false, || {
        // SAFETY: the caller's promises.
        let (Some(world), Some(tree)) = (unsafe { world.as_ref() }, unsafe { tree.as_ref() })
        else {
            return false;
        };
        let min = |pane: u64| world.min(pane);
        world
            .room(tab, tree.0.leaves().len(), &min)
            .is_some_and(|room| tree.0.fits(&room))
    })
}

/// Tab `tab`'s tree as it would stand with panes `a` and `b` trading places, the caller's; NULL if
/// either is not in it or a pane would go below its smallest size.
///
/// # Safety
/// As [`bt_world_free`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_world_tree_swapped(
    world: *const BtWorld,
    tab: u64,
    a: u64,
    b: u64,
) -> *mut BtTree {
    // SAFETY: the caller's promise.
    unsafe {
        with_world(world, null_mut(), |world| {
            let Some(shape) = world.shape(tab) else {
                return null_mut();
            };
            let mut tree = shape.tree.clone();
            let min = |pane: u64| world.min(pane);
            let Some(room) = world.room(tab, tree.leaves().len(), &min) else {
                return null_mut();
            };
            if !tree.swap(a, b) || !tree.fits(&room) {
                return null_mut();
            }
            owned(BtTree(tree))
        })
    }
}

/// Tab `tab`'s tree with divider `divider` (its index in [`bt_layout_divider`]) dragged to
/// `position` — points along its axis, in the area's frame — clamped so no pane goes below its
/// smallest size; the caller's. NULL if nothing moved.
///
/// # Safety
/// As [`bt_world_free`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_world_tree_dragged(
    world: *const BtWorld,
    tab: u64,
    divider: usize,
    position: f64,
) -> *mut BtTree {
    // SAFETY: the caller's promise.
    unsafe {
        with_world(world, null_mut(), |world| {
            let Some(shape) = world.shape(tab) else {
                return null_mut();
            };
            let mut tree = shape.tree.clone();
            let min = |pane: u64| world.min(pane);
            let Some(room) = world.room(tab, tree.leaves().len(), &min) else {
                return null_mut();
            };
            if !tree.drag_within(divider, position, &room) {
                return null_mut();
            }
            owned(BtTree(tree))
        })
    }
}

/// Tab `tab`'s tree with the divider nearest pane `pane` on `side`'s axis moved `step` points
/// toward `side` (bateri's Resize Split), clamped at the smallest sizes; the caller's. NULL if
/// nothing moved.
///
/// # Safety
/// As [`bt_world_free`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_world_tree_resized(
    world: *const BtWorld,
    tab: u64,
    pane: u64,
    side: u32,
    step: f64,
) -> *mut BtTree {
    // SAFETY: the caller's promise.
    unsafe {
        with_world(world, null_mut(), |world| {
            let (Some(shape), Some(direction)) = (world.shape(tab), side_of(side)) else {
                return null_mut();
            };
            let mut tree = shape.tree.clone();
            let min = |id: u64| world.min(id);
            let Some(room) = world.room(tab, tree.leaves().len(), &min) else {
                return null_mut();
            };
            if !tree.resize_within(pane, direction, step, &room) {
                return null_mut();
            }
            owned(BtTree(tree))
        })
    }
}

// ─── layouts ─────────────────────────────────────────────────────────────

/// A tab's tree turned into frames in its area: each pane's and each divider's, in points.
pub struct BtLayout(Layout);

/// Tab `tab`'s frames as the picture holds it — the same arithmetic every verdict and divider
/// question here is answered in, so what the host draws and what the engine judges agree. NULL
/// without the tab or its area.
///
/// # Safety
/// As [`bt_world_free`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_layout_new(world: *const BtWorld, tab: u64) -> *mut BtLayout {
    // SAFETY: the caller's promise.
    unsafe {
        with_world(world, null_mut(), |world| {
            let Some(shape) = world.shape(tab) else {
                return null_mut();
            };
            let Some(area) = world.areas.get(&tab) else {
                return null_mut();
            };
            let spacing = area.spacing(shape.tree.leaves().len());
            owned(BtLayout(shape.tree.layout_spaced(
                area.bounds,
                area.scale,
                spacing,
            )))
        })
    }
}

/// Frees a layout. NULL is ignored.
///
/// # Safety
/// `layout` is NULL or a layout from [`bt_layout_new`], not yet freed — for every `bt_layout_*`
/// call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_layout_free(layout: *mut BtLayout) {
    // SAFETY: the caller's promise.
    guarded((), || unsafe { release(layout) });
}

/// How many panes the layout places.
///
/// # Safety
/// As [`bt_layout_free`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_layout_pane_count(layout: *const BtLayout) -> usize {
    guarded(0, || {
        // SAFETY: the caller's promise.
        unsafe { layout.as_ref() }.map_or(0, |layout| layout.0.panes.len())
    })
}

/// The `index`th pane in tree order: its id and frame. `false` past the end.
///
/// # Safety
/// As [`bt_layout_free`]; each out-pointer NULL or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_layout_pane(
    layout: *const BtLayout,
    index: usize,
    pane: *mut u64,
    x: *mut f64,
    y: *mut f64,
    width: *mut f64,
    height: *mut f64,
) -> bool {
    guarded(false, || {
        // SAFETY: the caller's promise.
        let Some(&(id, rect)) =
            unsafe { layout.as_ref() }.and_then(|layout| layout.0.panes.get(index))
        else {
            return false;
        };
        // SAFETY: the caller's promise.
        unsafe {
            put(pane, id);
            write_rect(rect, x, y, width, height);
        }
        true
    })
}

/// How many dividers the layout has.
///
/// # Safety
/// As [`bt_layout_free`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_layout_divider_count(layout: *const BtLayout) -> usize {
    guarded(0, || {
        // SAFETY: the caller's promise.
        unsafe { layout.as_ref() }.map_or(0, |layout| layout.0.dividers.len())
    })
}

/// The `index`th divider — the index [`bt_world_tree_dragged`] takes: the axis of the split it
/// divides and its line. `false` past the end.
///
/// # Safety
/// As [`bt_layout_free`]; each out-pointer NULL or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_layout_divider(
    layout: *const BtLayout,
    index: usize,
    axis_out: *mut u32,
    x: *mut f64,
    y: *mut f64,
    width: *mut f64,
    height: *mut f64,
) -> bool {
    guarded(false, || {
        // SAFETY: the caller's promise.
        let Some(divider) =
            unsafe { layout.as_ref() }.and_then(|layout| layout.0.dividers.get(index))
        else {
            return false;
        };
        // SAFETY: the caller's promise.
        unsafe {
            put(axis_out, axis_code(divider.axis));
            write_rect(divider.rect, x, y, width, height);
        }
        true
    })
}

// ─── moves ───────────────────────────────────────────────────────────────

/// What the user asked for, built by one of the `bt_move_*` constructors.
pub struct BtMove(Move);

/// Copies the tree `tree` points at, if it is one.
///
/// # Safety
/// `tree` is NULL or a live tree.
unsafe fn tree_arg(tree: *const BtTree) -> Option<Tree> {
    // SAFETY: the caller's promise.
    unsafe { tree.as_ref() }.map(|tree| tree.0.clone())
}

/// Pane `pane` joins tab `into` beside its focused pane, on `side` — neighbours make room down to
/// their smallest sizes. A tab's only pane joins as its tab. NULL for an unknown side.
#[unsafe(no_mangle)]
pub extern "C" fn bt_move_pane_to_tab(pane: u64, into: u64, side: u32) -> *mut BtMove {
    guarded(null_mut(), || {
        side_of(side).map_or(null_mut(), |side| {
            owned(BtMove(Move::PaneToTab {
                pane,
                into,
                place: Joins::Beside(side),
            }))
        })
    })
}

/// Pane `pane` joins tab `into` laid out exactly as `tree` (copied) — the landing a verdict
/// showed. Refused if the tab has changed since.
///
/// # Safety
/// `tree` is NULL or a live tree.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_move_pane_to_tab_planned(
    pane: u64,
    into: u64,
    tree: *const BtTree,
) -> *mut BtMove {
    guarded(null_mut(), || {
        // SAFETY: the caller's promise.
        unsafe { tree_arg(tree) }.map_or(null_mut(), |tree| {
            owned(BtMove(Move::PaneToTab {
                pane,
                into,
                place: Joins::Planned(tree),
            }))
        })
    })
}

/// Tab `tab` joins tab `into` as a block of panes with its own layout, beside the focused pane on
/// `side`; the tab and its name are thrown away. NULL for an unknown side.
#[unsafe(no_mangle)]
pub extern "C" fn bt_move_tab_to_tab(tab: u64, into: u64, side: u32) -> *mut BtMove {
    guarded(null_mut(), || {
        side_of(side).map_or(null_mut(), |side| {
            owned(BtMove(Move::TabToTab {
                tab,
                into,
                place: Joins::Beside(side),
            }))
        })
    })
}

/// Tab `tab` joins tab `into` laid out exactly as `tree` (copied).
///
/// # Safety
/// `tree` is NULL or a live tree.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_move_tab_to_tab_planned(
    tab: u64,
    into: u64,
    tree: *const BtTree,
) -> *mut BtMove {
    guarded(null_mut(), || {
        // SAFETY: the caller's promise.
        unsafe { tree_arg(tree) }.map_or(null_mut(), |tree| {
            owned(BtMove(Move::TabToTab {
                tab,
                into,
                place: Joins::Planned(tree),
            }))
        })
    })
}

/// Pane `pane` becomes a tab of its own in window `window`'s strip, before the tab at `gap` (the
/// strip's length is the end). A tab's only pane is the tab: the tab itself moves there.
#[unsafe(no_mangle)]
pub extern "C" fn bt_move_pane_to_new_tab(pane: u64, window: u64, gap: usize) -> *mut BtMove {
    guarded(null_mut(), || {
        owned(BtMove(Move::PaneToNewTab { pane, window, gap }))
    })
}

/// Tab `tab` becomes a window of its own; `has_at`: let go at the screen point `x`, `y` (the
/// platform's coordinates), else at its old window's place, cascaded.
#[unsafe(no_mangle)]
pub extern "C" fn bt_move_tab_to_new_window(tab: u64, has_at: bool, x: f64, y: f64) -> *mut BtMove {
    guarded(null_mut(), || {
        owned(BtMove(Move::TabToNewWindow {
            tab,
            at: has_at.then_some((x, y)),
        }))
    })
}

/// Pane `pane` becomes a window of its own, as its one tab; `has_at` as in
/// [`bt_move_tab_to_new_window`].
#[unsafe(no_mangle)]
pub extern "C" fn bt_move_pane_to_new_window(
    pane: u64,
    has_at: bool,
    x: f64,
    y: f64,
) -> *mut BtMove {
    guarded(null_mut(), || {
        owned(BtMove(Move::PaneToNewWindow {
            pane,
            at: has_at.then_some((x, y)),
        }))
    })
}

/// Tab `tab` takes place `index` in window `window`'s strip — along its own strip, or on
/// another window's, where it comes up selected.
#[unsafe(no_mangle)]
pub extern "C" fn bt_move_tab_to_strip(tab: u64, window: u64, index: usize) -> *mut BtMove {
    guarded(null_mut(), || {
        owned(BtMove(Move::TabToStrip { tab, window, index }))
    })
}

/// Every other window's tabs join window `into`'s at its end; the windows they leave close
/// (those that do not outlive their tabs).
#[unsafe(no_mangle)]
pub extern "C" fn bt_move_merge_all_windows(into: u64) -> *mut BtMove {
    guarded(null_mut(), || owned(BtMove(Move::MergeAllWindows { into })))
}

/// Undo Move: `record`'s picture goes back as one step (the record is copied).
///
/// # Safety
/// `record` is NULL or a live record.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_move_undo(record: *const BtRecord) -> *mut BtMove {
    guarded(null_mut(), || {
        // SAFETY: the caller's promise.
        unsafe { record.as_ref() }.map_or(null_mut(), |record| {
            owned(BtMove(Move::Undo {
                record: record.0.clone(),
            }))
        })
    })
}

/// Frees a move. NULL is ignored.
///
/// # Safety
/// `mv` is NULL or a move from a constructor, not yet freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_move_free(mv: *mut BtMove) {
    // SAFETY: the caller's promise.
    guarded((), || unsafe { release(mv) });
}

// ─── plans ───────────────────────────────────────────────────────────────

/// What a move comes to: the steps in two parts, how many new identities they use, and the record
/// Undo Move keeps.
pub struct BtPlan {
    parts: [Vec<Step>; 2],
    /// Each step's name (a reshaped tab's), as the host reads it.
    names: [Vec<Option<CString>>; 2],
    ids_used: u64,
    undo: Option<Record>,
}

/// What `mv` comes to in `world`: a plan, the caller's — or NULL, with why in `refusal` (a
/// `BT_REFUSAL_*`; 0 when a plan comes). Neither is changed: one picture answers any number of
/// moves.
///
/// # Safety
/// `world` and `mv` NULL or live; `refusal` NULL or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_plan_new(
    world: *const BtWorld,
    mv: *const BtMove,
    refusal_out: *mut i32,
) -> *mut BtPlan {
    guarded(null_mut(), || {
        // SAFETY: the caller's promises.
        let (Some(world), Some(mv)) = (unsafe { world.as_ref() }, unsafe { mv.as_ref() }) else {
            return null_mut();
        };
        let engine = Engine {
            world,
            next: Cell::new(world.next_id),
        };
        let answer = moves::plan(&world.windows, mv.0.clone(), &engine);
        let code = match &answer {
            Ok(_) => 0,
            Err(Refusal::Quiet) => refusal::QUIET,
            Err(Refusal::Beep) => refusal::BEEP,
            Err(Refusal::Stale) => refusal::STALE,
        };
        // SAFETY: the caller's promise.
        unsafe { put(refusal_out, code) };
        let Ok(Plan { steps, after, undo }) = answer else {
            return null_mut();
        };
        let names = |steps: &[Step]| -> Vec<Option<CString>> {
            steps
                .iter()
                .map(|step| match step {
                    Step::Reshape {
                        name: Some(name), ..
                    } => crate::c_text(name.as_bytes()),
                    _ => None,
                })
                .collect()
        };
        owned(BtPlan {
            names: [names(&steps), names(&after)],
            parts: [steps, after],
            ids_used: engine.next.get().wrapping_sub(world.next_id),
            undo,
        })
    })
}

/// Frees a plan, and with it every tree and strip it lent. NULL is ignored.
///
/// # Safety
/// `plan` is NULL or a plan from [`bt_plan_new`], not yet freed — for every `bt_plan_*` call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_plan_free(plan: *mut BtPlan) {
    // SAFETY: the caller's promise.
    guarded((), || unsafe { release(plan) });
}

/// How many new identities the plan gave, counted from the picture's next one: the host moves
/// its counter on by this many.
///
/// # Safety
/// As [`bt_plan_free`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_plan_ids_used(plan: *const BtPlan) -> u64 {
    guarded(0, || {
        // SAFETY: the caller's promise.
        unsafe { plan.as_ref() }.map_or(0, |plan| plan.ids_used)
    })
}

/// The steps of `part` (a `BT_PART_*`).
fn steps(plan: &BtPlan, part: u32) -> Option<&[Step]> {
    match part {
        part::MAIN => Some(&plan.parts[0]),
        part::AFTER => Some(&plan.parts[1]),
        _ => None,
    }
}

/// Runs `body` on step `index` of `part`; `failed` if there is none.
///
/// # Safety
/// As [`bt_plan_free`].
unsafe fn with_step<T: Copy>(
    plan: *const BtPlan,
    part: u32,
    index: usize,
    failed: T,
    body: impl FnOnce(&Step, &BtPlan) -> T,
) -> T {
    guarded(failed, || {
        // SAFETY: the caller's promise.
        let Some(plan) = (unsafe { plan.as_ref() }) else {
            return failed;
        };
        steps(plan, part)
            .and_then(|steps| steps.get(index))
            .map_or(failed, |step| body(step, plan))
    })
}

/// How many steps `part` (a `BT_PART_*`) holds.
///
/// # Safety
/// As [`bt_plan_free`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_plan_step_count(plan: *const BtPlan, part: u32) -> usize {
    guarded(0, || {
        // SAFETY: the caller's promise.
        unsafe { plan.as_ref() }
            .and_then(|plan| steps(plan, part))
            .map_or(0, <[Step]>::len)
    })
}

/// The kind of step `index` of `part` (a `BT_STEP_*`); 0 past the end.
///
/// # Safety
/// As [`bt_plan_free`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_plan_step_kind(plan: *const BtPlan, part: u32, index: usize) -> u32 {
    // SAFETY: the caller's promise.
    unsafe {
        with_step(plan, part, index, 0, |step, _| match step {
            Step::ReleasePane { .. } => step::RELEASE_PANE,
            Step::ReleaseTab { .. } => step::RELEASE_TAB,
            Step::Unpack { .. } => step::UNPACK,
            Step::Fold { .. } => step::FOLD,
            Step::NewTab { .. } => step::NEW_TAB,
            Step::Wrap { .. } => step::WRAP,
            Step::AdoptTab { .. } => step::ADOPT_TAB,
            Step::MoveTab { .. } => step::MOVE_TAB,
            Step::Dissolve { .. } => step::DISSOLVE,
            Step::Reshape { .. } => step::RESHAPE,
            Step::PutStrip { .. } => step::PUT_STRIP,
            Step::Fit { .. } => step::FIT,
            Step::Undone { .. } => step::UNDONE,
            Step::OpenWindow { .. } => step::OPEN_WINDOW,
            Step::AdoptPanes { .. } => step::ADOPT_PANES,
            Step::CloseIfEmptied { .. } => step::CLOSE_IF_EMPTIED,
            Step::Raise { .. } => step::RAISE,
            Step::Select { .. } => step::SELECT,
            Step::Focus { .. } => step::FOCUS,
            Step::Pulse { .. } => step::PULSE,
        })
    }
}

/// The plain fields of a step — what the getters read; a kind that has no such field reads 0.
#[derive(Clone, Copy, Default)]
struct Fields {
    window: u64,
    tab: u64,
    pane: u64,
    into: u64,
    from: u64,
    gap: usize,
    index: usize,
    slot: u32,
    at: Option<(f64, f64)>,
}

fn fields(step: &Step) -> Fields {
    let none = Fields::default();
    match *step {
        Step::ReleasePane { window, tab, pane } | Step::Focus { window, tab, pane } => Fields {
            window,
            tab,
            pane,
            ..none
        },
        Step::ReleaseTab { window, tab }
        | Step::Unpack { window, tab }
        | Step::Wrap { window, tab }
        | Step::Dissolve { window, tab }
        | Step::Reshape { window, tab, .. }
        | Step::Fit { window, tab }
        | Step::AdoptPanes { window, tab, .. }
        | Step::Select { window, tab }
        | Step::Pulse { window, tab } => Fields {
            window,
            tab,
            ..none
        },
        Step::Fold { window, tab, into } => Fields {
            window,
            tab,
            into,
            ..none
        },
        Step::NewTab { window, tab, gap } => Fields {
            window,
            tab,
            gap,
            ..none
        },
        Step::AdoptTab { window, tab, slot } => match slot {
            Slot::At(index) => Fields {
                window,
                tab,
                index,
                slot: slot::AT,
                ..none
            },
            Slot::End => Fields {
                window,
                tab,
                slot: slot::END,
                ..none
            },
        },
        Step::MoveTab { window, tab, index } => Fields {
            window,
            tab,
            index,
            ..none
        },
        Step::OpenWindow {
            window,
            from,
            tab,
            at,
        } => Fields {
            window,
            tab,
            from,
            at,
            ..none
        },
        Step::PutStrip { window, .. }
        | Step::Undone { window }
        | Step::CloseIfEmptied { window }
        | Step::Raise { window } => Fields { window, ..none },
    }
}

/// Field `pick` of step `index` of `part`; its type's zero past the end.
///
/// # Safety
/// As [`bt_plan_free`].
unsafe fn step_field<T: Copy + Default>(
    plan: *const BtPlan,
    part: u32,
    index: usize,
    pick: impl FnOnce(Fields) -> T,
) -> T {
    // SAFETY: the caller's promise.
    unsafe {
        with_step(plan, part, index, T::default(), |step, _| {
            pick(fields(step))
        })
    }
}

/// The window a step acts in (every kind has one).
///
/// # Safety
/// As [`bt_plan_free`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_plan_step_window(plan: *const BtPlan, part: u32, index: usize) -> u64 {
    // SAFETY: the caller's promise.
    unsafe { step_field(plan, part, index, |fields| fields.window) }
}

/// The tab a step acts on.
///
/// # Safety
/// As [`bt_plan_free`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_plan_step_tab(plan: *const BtPlan, part: u32, index: usize) -> u64 {
    // SAFETY: the caller's promise.
    unsafe { step_field(plan, part, index, |fields| fields.tab) }
}

/// The pane of a RELEASE_PANE or FOCUS step.
///
/// # Safety
/// As [`bt_plan_free`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_plan_step_pane(plan: *const BtPlan, part: u32, index: usize) -> u64 {
    // SAFETY: the caller's promise.
    unsafe { step_field(plan, part, index, |fields| fields.pane) }
}

/// The tab a FOLD step's tab folds into.
///
/// # Safety
/// As [`bt_plan_free`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_plan_step_into(plan: *const BtPlan, part: u32, index: usize) -> u64 {
    // SAFETY: the caller's promise.
    unsafe { step_field(plan, part, index, |fields| fields.into) }
}

/// The window an OPEN_WINDOW step's new window takes its size from.
///
/// # Safety
/// As [`bt_plan_free`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_plan_step_from(plan: *const BtPlan, part: u32, index: usize) -> u64 {
    // SAFETY: the caller's promise.
    unsafe { step_field(plan, part, index, |fields| fields.from) }
}

/// A NEW_TAB step's gap: the new tab goes before the tab at this place.
///
/// # Safety
/// As [`bt_plan_free`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_plan_step_gap(plan: *const BtPlan, part: u32, index: usize) -> usize {
    // SAFETY: the caller's promise.
    unsafe { step_field(plan, part, index, |fields| fields.gap) }
}

/// A MOVE_TAB step's new place, or an ADOPT_TAB step's when its slot is `BT_SLOT_AT`.
///
/// # Safety
/// As [`bt_plan_free`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_plan_step_index(plan: *const BtPlan, part: u32, index: usize) -> usize {
    // SAFETY: the caller's promise.
    unsafe { step_field(plan, part, index, |fields| fields.index) }
}

/// An ADOPT_TAB step's slot (a `BT_SLOT_*`).
///
/// # Safety
/// As [`bt_plan_free`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_plan_step_slot(plan: *const BtPlan, part: u32, index: usize) -> u32 {
    // SAFETY: the caller's promise.
    unsafe { step_field(plan, part, index, |fields| fields.slot) }
}

/// An OPEN_WINDOW step's screen point, if it has one: `true` and the point written.
///
/// # Safety
/// As [`bt_plan_free`]; `x` and `y` NULL or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_plan_step_at(
    plan: *const BtPlan,
    part: u32,
    index: usize,
    x: *mut f64,
    y: *mut f64,
) -> bool {
    // SAFETY: the caller's promise.
    unsafe {
        with_step(plan, part, index, false, |step, _| {
            let Some((at_x, at_y)) = fields(step).at else {
                return false;
            };
            put(x, at_x);
            put(y, at_y);
            true
        })
    }
}

/// A RESHAPE step's tab name, lent by the plan; NULL when the tab has none.
///
/// # Safety
/// As [`bt_plan_free`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_plan_step_name(
    plan: *const BtPlan,
    part: u32,
    index: usize,
) -> *const c_char {
    guarded(null(), || {
        // SAFETY: the caller's promise.
        let Some(plan) = (unsafe { plan.as_ref() }) else {
            return null();
        };
        let names = match part {
            part::MAIN => &plan.names[0],
            part::AFTER => &plan.names[1],
            _ => return null(),
        };
        names
            .get(index)
            .and_then(Option::as_deref)
            .map_or(null(), CStr::as_ptr)
    })
}

/// A RESHAPE or ADOPT_PANES step's tree, lent by the plan (it lives as long as the plan); NULL for
/// other kinds.
///
/// # Safety
/// As [`bt_plan_free`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_plan_step_tree(
    plan: *const BtPlan,
    part: u32,
    index: usize,
) -> *const BtTree {
    // SAFETY: the caller's promise.
    unsafe {
        with_step(plan, part, index, null(), |step, _| match step {
            Step::Reshape { tree, .. } | Step::AdoptPanes { tree, .. } => BtTree::lend(tree),
            _ => null(),
        })
    }
}

/// A PUT_STRIP step's strip — the order and the selection to put back — lent by the plan; NULL
/// for other kinds.
///
/// # Safety
/// As [`bt_plan_free`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_plan_step_strip(
    plan: *const BtPlan,
    part: u32,
    index: usize,
) -> *const crate::strip::BtStrip {
    // SAFETY: the caller's promise.
    unsafe {
        with_step(plan, part, index, null(), |step, _| match step {
            Step::PutStrip { order, .. } => crate::strip::BtStrip::lend(order),
            _ => null(),
        })
    }
}

/// The record Undo Move keeps for this plan, the caller's — taken out, so a second call is NULL;
/// NULL also for a move that cannot be taken back. Keep it once every main step was carried out.
///
/// # Safety
/// As [`bt_plan_free`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_plan_take_undo(plan: *mut BtPlan) -> *mut BtRecord {
    guarded(null_mut(), || {
        // SAFETY: the caller's promise.
        unsafe { plan.as_mut() }
            .and_then(|plan| plan.undo.take())
            .map_or(null_mut(), |record| owned(BtRecord(record)))
    })
}

// ─── Undo Move's records ─────────────────────────────────────────────────

/// What Undo Move takes back: a picture of the strips and tabs a move changed.
pub struct BtRecord(Record);

/// A record of tab `tab` as it stands, for a change the host makes inside one tab (a swap, a
/// pane carried within its tab): taken before the change, kept after it. NULL without the tab.
///
/// # Safety
/// As [`bt_world_free`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_record_of_tab(world: *const BtWorld, tab: u64) -> *mut BtRecord {
    // SAFETY: the caller's promise.
    unsafe {
        with_world(world, null_mut(), |world| {
            world.window_of(tab).map_or(null_mut(), |window| {
                owned(BtRecord(Record {
                    scenes: vec![window.scene(&[tab])],
                    born: Vec::new(),
                }))
            })
        })
    }
}

/// Whether `record` is still a picture of what `world` holds — whether Undo Move can be offered.
/// A pane born or gone since, or a window gone, makes it false: the host drops it.
///
/// # Safety
/// `world` and `record` NULL or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_record_standing(
    world: *const BtWorld,
    record: *const BtRecord,
) -> bool {
    guarded(false, || {
        // SAFETY: the caller's promises.
        match (unsafe { world.as_ref() }, unsafe { record.as_ref() }) {
            (Some(world), Some(record)) => moves::standing(&world.windows, &record.0),
            _ => false,
        }
    })
}

/// Frees a record. NULL is ignored.
///
/// # Safety
/// `record` is NULL or an owned record, not yet freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_record_free(record: *mut BtRecord) {
    // SAFETY: the caller's promise.
    guarded((), || unsafe { release(record) });
}

// ─── verdicts ────────────────────────────────────────────────────────────

/// What a carried block shows with the pointer at a point of a tab, and so what letting go there
/// does.
pub struct BtVerdict {
    verdict: Verdict,
    /// The zone's word, for debugging; the host shows its own.
    label: Option<CString>,
}

impl BtVerdict {
    fn zone(&self) -> Option<Zone> {
        match &self.verdict {
            Verdict::Lands { zone, .. } | Verdict::TooSmall { zone, .. } => Some(*zone),
            Verdict::Swaps { target, .. } => Some(Zone::Swap { target: *target }),
            Verdict::Nothing | Verdict::NoRoom => None,
        }
    }
}

/// What `carried` (copied: a pane of the tab, or a block from elsewhere) shows over tab `tab` with
/// the pointer at `x`, `y` (the area's frame, points, top-down), the caller's. The same answer the
/// preview draws and the drop carries out: a LANDS verdict's tree goes to a `_planned` move.
/// NULL without the tab or its area.
///
/// # Safety
/// `world` and `carried` NULL or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_verdict_new(
    world: *const BtWorld,
    tab: u64,
    carried: *const BtTree,
    x: f64,
    y: f64,
) -> *mut BtVerdict {
    guarded(null_mut(), || {
        // SAFETY: the caller's promises.
        let (Some(world), Some(carried)) = (unsafe { world.as_ref() }, unsafe { carried.as_ref() })
        else {
            return null_mut();
        };
        let Some(shape) = world.shape(tab) else {
            return null_mut();
        };
        let here = shape.tree.leaves();
        let foreign = carried
            .0
            .leaves()
            .iter()
            .filter(|pane| !here.contains(pane))
            .count();
        let min = |pane: u64| world.min(pane);
        let Some(room) = world.room(tab, here.len() + foreign, &min) else {
            return null_mut();
        };
        let mut answer = BtVerdict {
            verdict: shape.tree.verdict(&carried.0, &room, (x, y)),
            label: None,
        };
        answer.label = answer
            .zone()
            .and_then(Zone::label)
            .and_then(|label| CString::new(label).ok());
        owned(answer)
    })
}

/// Frees a verdict, and with it the tree it lent. NULL is ignored.
///
/// # Safety
/// `verdict` is NULL or a verdict from [`bt_verdict_new`], not yet freed — for every
/// `bt_verdict_*` call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_verdict_free(verdict: *mut BtVerdict) {
    // SAFETY: the caller's promise.
    guarded((), || unsafe { release(verdict) });
}

/// Runs `body` on the verdict; `failed` if it is NULL.
///
/// # Safety
/// As [`bt_verdict_free`].
unsafe fn with_verdict<T: Copy>(
    verdict: *const BtVerdict,
    failed: T,
    body: impl FnOnce(&BtVerdict) -> T,
) -> T {
    guarded(failed, || {
        // SAFETY: the caller's promise.
        unsafe { verdict.as_ref() }.map_or(failed, body)
    })
}

/// The verdict's kind (a `BT_VERDICT_*`).
///
/// # Safety
/// As [`bt_verdict_free`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_verdict_kind(verdict: *const BtVerdict) -> u32 {
    // SAFETY: the caller's promise.
    unsafe {
        with_verdict(verdict, 0, |answer| match answer.verdict {
            Verdict::Nothing => verdict::NOTHING,
            Verdict::Lands { .. } => verdict::LANDS,
            Verdict::Swaps { .. } => verdict::SWAPS,
            Verdict::TooSmall { .. } => verdict::TOO_SMALL,
            Verdict::NoRoom => verdict::NO_ROOM,
        })
    }
}

/// What the pointer asks for (a `BT_ZONE_*`): a LANDS or TOO_SMALL verdict's zone, SWAP for a
/// SWAPS verdict; 0 otherwise. The host names it in its own language.
///
/// # Safety
/// As [`bt_verdict_free`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_verdict_zone(verdict: *const BtVerdict) -> u32 {
    // SAFETY: the caller's promise.
    unsafe {
        with_verdict(verdict, 0, |answer| match answer.zone() {
            Some(Zone::WindowEdge(_)) => zone::WINDOW_EDGE,
            Some(Zone::Beside { .. }) => zone::BESIDE,
            Some(Zone::Swap { .. }) => zone::SWAP,
            Some(Zone::Own) => zone::OWN,
            Some(Zone::Outside) => zone::OUTSIDE,
            None => 0,
        })
    }
}

/// The zone's side (a `BT_SIDE_*`) for WINDOW_EDGE and BESIDE; 0 otherwise.
///
/// # Safety
/// As [`bt_verdict_free`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_verdict_side(verdict: *const BtVerdict) -> u32 {
    // SAFETY: the caller's promise.
    unsafe {
        with_verdict(verdict, 0, |answer| match answer.zone() {
            Some(Zone::WindowEdge(side) | Zone::Beside { side, .. }) => side_code(side),
            _ => 0,
        })
    }
}

/// The pane the zone is about — beside it, or trading places with it; 0 otherwise.
///
/// # Safety
/// As [`bt_verdict_free`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_verdict_target(verdict: *const BtVerdict) -> u64 {
    // SAFETY: the caller's promise.
    unsafe {
        with_verdict(verdict, 0, |answer| match answer.zone() {
            Some(Zone::Beside { target, .. } | Zone::Swap { target }) => target,
            _ => 0,
        })
    }
}

/// The zone's English word ("Left", "Swap", …), lent by the verdict — for debugging only; NULL
/// when there is none.
///
/// # Safety
/// As [`bt_verdict_free`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_verdict_label(verdict: *const BtVerdict) -> *const c_char {
    // SAFETY: the caller's promise.
    unsafe {
        with_verdict(verdict, null(), |answer| {
            answer.label.as_deref().map_or(null(), CStr::as_ptr)
        })
    }
}

/// The verdict's rectangle: where a LANDS block lands, a SWAPS target's frame, the region a
/// TOO_SMALL zone points at. `false` for the other kinds.
///
/// # Safety
/// As [`bt_verdict_free`]; each out-pointer NULL or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_verdict_rect(
    verdict: *const BtVerdict,
    x: *mut f64,
    y: *mut f64,
    width: *mut f64,
    height: *mut f64,
) -> bool {
    // SAFETY: the caller's promise.
    unsafe {
        with_verdict(verdict, false, |answer| {
            let rect = match &answer.verdict {
                Verdict::Lands { placement, .. } => placement.landing,
                Verdict::Swaps { frame, .. } => *frame,
                Verdict::TooSmall { region, .. } => *region,
                Verdict::Nothing | Verdict::NoRoom => return false,
            };
            write_rect(rect, x, y, width, height);
            true
        })
    }
}

/// A LANDS verdict made room: neighbours shrink, or the block took more than its share — the
/// preview can say so.
///
/// # Safety
/// As [`bt_verdict_free`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_verdict_made_room(verdict: *const BtVerdict) -> bool {
    // SAFETY: the caller's promise.
    unsafe {
        with_verdict(verdict, false, |answer| match &answer.verdict {
            Verdict::Lands { placement, .. } => placement.made_room,
            _ => false,
        })
    }
}

/// A SWAPS verdict's panes both keep their smallest sizes in each other's place.
///
/// # Safety
/// As [`bt_verdict_free`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_verdict_fits(verdict: *const BtVerdict) -> bool {
    // SAFETY: the caller's promise.
    unsafe {
        with_verdict(verdict, false, |answer| match answer.verdict {
            Verdict::Swaps { fits, .. } => fits,
            _ => false,
        })
    }
}

/// A LANDS verdict's tree — the whole tab once the block has landed — lent by the verdict; NULL
/// for the other kinds. Hand it to a `_planned` move, or for a pane carried within its tab, take
/// it as the tab's new tree.
///
/// # Safety
/// As [`bt_verdict_free`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_verdict_tree(verdict: *const BtVerdict) -> *const BtTree {
    // SAFETY: the caller's promise.
    unsafe {
        with_verdict(verdict, null(), |answer| match &answer.verdict {
            Verdict::Lands { placement, .. } => BtTree::lend(&placement.tree),
            _ => null(),
        })
    }
}

/// How many edges of the area would take a TOO_SMALL block.
///
/// # Safety
/// As [`bt_verdict_free`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_verdict_edge_count(verdict: *const BtVerdict) -> usize {
    // SAFETY: the caller's promise.
    unsafe {
        with_verdict(verdict, 0, |answer| match &answer.verdict {
            Verdict::TooSmall { edges, .. } => edges.len(),
            _ => 0,
        })
    }
}

/// The `index`th edge that would take a TOO_SMALL block: its side and where the block would land
/// there. `false` past the end.
///
/// # Safety
/// As [`bt_verdict_free`]; each out-pointer NULL or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bt_verdict_edge(
    verdict: *const BtVerdict,
    index: usize,
    side_out: *mut u32,
    x: *mut f64,
    y: *mut f64,
    width: *mut f64,
    height: *mut f64,
) -> bool {
    // SAFETY: the caller's promise.
    unsafe {
        with_verdict(verdict, false, |answer| {
            let Verdict::TooSmall { edges, .. } = &answer.verdict else {
                return false;
            };
            let Some(&(side, rect)) = edges.get(index) else {
                return false;
            };
            put(side_out, side_code(side));
            write_rect(rect, x, y, width, height);
            true
        })
    }
}

#[cfg(test)]
mod tests;
