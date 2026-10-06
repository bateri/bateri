//! The draw list of one frame.
//!
//! `bt-core`'s `frame()` sink fills this directly: the grid coordinate is
//! converted to pixels here and enters the layout the GPU will see. The
//! renderer reads "what to draw" from here and does not know "what it means".
//!
//! **Ten lists (plus the dock's two effect lists, the selection's two and the
//! search's four), three surfaces, three pipelines** (of the lists here; the
//! effects belong to the fifth, `glyph_fx`). The lists come in quad/triple
//! groups per surface: the grid's four (command block stripes, backgrounds,
//! glyphs, rule lines), the dock's three (`dock_bg`/`dock_glyphs`/
//! `dock_rules`) and the fill band's three (`fill_bg`/`fill_glyphs`/
//! `fill_rules`). What separates the groups is not order but **coordinate
//! space**: each surface has its own `setViewport` and the row numbers are
//! surface-local, so they could not be told apart in a single list
//! ([`crate::Renderer`]). The pipelines, on the other hand, are three and
//! surface-independent: stripes and backgrounds go to `cell_bg`'s, glyphs and
//! rule lines to `cell`'s, and the caret to the sibling fragment (`caret`)
//! that shares `cell_bg`'s vertex. The selection's list belongs to the
//! sixth pipeline (`selection`): the same `Instance`, its own vertex and a
//! corner-masked fragment; the search's four lists (two each on the grid
//! and on the band, one per role) share the same pipeline. The reason the
//! lists within a group stay separate is draw order — glyphs must come on top
//! of backgrounds and rules on top of glyphs, and in a single list the order
//! would get mixed cell by cell. Glyph and rule being in separate lists is
//! the continuation of the same sentence: both go through the same pipeline
//! but the strikeout must be drawn after the letter under it. The stripe
//! shares `cell_bg` with the backgrounds but its list is separate and the
//! reason is not order but **lifetime**: [`Frame::move_caret`] trims the
//! background list, while the stripe must stay as it was in a motion frame
//! (see [`Frame::stripes`]).

use std::mem::offset_of;

use bt_atlas::{Face, RuleKind, SizeClass};
use bt_core::{
    Block, ButtonState, CaretShape, CaretStyle, Cell, ClusterId, Clusters, DockButton, LinearRgba,
    SearchRun, SelectionRun, UnderlineStyle, UnfocusedCaret,
};

use crate::glyph_fx::{Fx, GlyphFx, Kind};
use crate::metrics::CellMetrics;

/// Identical to `shaders/cell_bg.wgsl` -> `Instance`, field by field.
///
/// Not exposed outside the crate: this is a GPU byte layout, whereas
/// `bt-core`'s `Cell` is a grid coordinate that carries meaning. Making the
/// two the same type would nail the cell model to the shader layout.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Instance {
    pos: [f32; 2],
    size: [f32; 2],
    /// **Linear** RGBA. The target is `BGRA8Unorm_sRGB` and the ROP does the
    /// encoding: a second gamma correction on the shader side would encode the
    /// palette twice. Its source is `bt_core::color::linear_rgba`.
    rgba: [f32; 4],
}

// In MSL float2 is 8-aligned and float4 16-aligned; in Rust everything is
// 4-aligned but the field offsets and the stride coincide. These are the
// three numbers being pinned — if one drifts the GPU reads the wrong
// colour/position throughout and the symptom is silent. (`pos` being at 0 is
// `repr(C)`'s definition, not something to assert.) These pin only THIS
// side; the MSL side has its own `static_assert`s.
const _: () = assert!(size_of::<Instance>() == 32);
const _: () = assert!(offset_of!(Instance, size) == 8);
const _: () = assert!(offset_of!(Instance, rgba) == 16);

/// `Instance`'s field offsets, for the wgpu vertex layout
/// (`crate::renderer`): the fields are private and `offset_of!` only sees
/// them here. The layout's second consumer is fed from next to the asserts,
/// not from three hand-written numbers.
pub(crate) const INSTANCE_OFFSETS: [u64; 3] = [
    offset_of!(Instance, pos) as u64,
    offset_of!(Instance, size) as u64,
    offset_of!(Instance, rgba) as u64,
];

/// Identical to `shaders/cell.wgsl` -> `GlyphInstance`, field by field.
///
/// **No `size`, no uv size**: in this set every glyph is exactly one cell
/// tall (fixed slot grid) and both are constant across the
/// frame, so they are passed as uniforms rather than per instance. The gain
/// is not just bandwidth: the `{pos, size, uv0, rgba}` layout comes to 40
/// bytes in Rust and 48 in MSL (`float4` is 16-aligned, `[f32; 4]` 4) and
/// would only map with a padding field that exists purely for alignment. As
/// it is, the two sides overlap without padding and the stride is the same
/// 32 as `Instance`'s.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct GlyphInstance {
    /// Top-left corner of the cell, pixels.
    pub(crate) pos: [f32; 2],
    /// Top-left corner of the slot in the atlas, a **normalized** texture coordinate.
    pub(crate) uv0: [f32; 2],
    /// Foreground, **linear** RGBA; the same space and the same warning as `Instance.rgba`.
    pub(crate) rgba: [f32; 4],
}

// The same rationale as `Instance`, the same pair of ties.
const _: () = assert!(size_of::<GlyphInstance>() == 32);
const _: () = assert!(offset_of!(GlyphInstance, uv0) == 8);
const _: () = assert!(offset_of!(GlyphInstance, rgba) == 16);

/// `GlyphInstance`'s field offsets, for the wgpu vertex layout
/// (`crate::renderer`): fed from next to the asserts, like
/// [`INSTANCE_OFFSETS`].
pub(crate) const GLYPH_INSTANCE_OFFSETS: [u64; 3] = [
    offset_of!(GlyphInstance, pos) as u64,
    offset_of!(GlyphInstance, uv0) as u64,
    offset_of!(GlyphInstance, rgba) as u64,
];

/// Identical to `shaders/glyph_fx.wgsl` -> `FxInstance`, field by field: the
/// instance of the dock's typing effects.
///
/// **A sibling of [`GlyphInstance`], not an extension of it**: the stride of
/// all the glyph lists would grow for a handful of animated glyphs. The
/// first three fields are in the same places as its, the fourth is the
/// effect's parameters:
///
/// - `fx[0]` — progress `t`, `0..=1`; the curve is in the shader.
/// - `fx[1]` — effect id, plane and half in **one small integer**
///   (`id | plane << 5 | half << 6`), as an `f32`: an integer is represented
///   exactly in `f32`, and had it been carried as a bit pattern
///   (`from_bits`) a small pattern could be treated as a denormal and
///   flushed to zero in `flat` interpolation.
/// - `fx[2]` — seed ([`crate::glyph_fx::Fx::seed`]).
/// - `fx[3]` — spare, zero.
///
/// The layout has no padding: `float2` is 8-aligned, `float4` 16 → 0/8/16/32,
/// stride 48.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct FxInstance {
    pub(crate) pos: [f32; 2],
    pub(crate) uv0: [f32; 2],
    pub(crate) rgba: [f32; 4],
    pub(crate) fx: [f32; 4],
}

const _: () = assert!(size_of::<FxInstance>() == 48);
const _: () = assert!(offset_of!(FxInstance, uv0) == 8);
const _: () = assert!(offset_of!(FxInstance, rgba) == 16);
const _: () = assert!(offset_of!(FxInstance, fx) == 32);

/// `FxInstance`'s field offsets, for the wgpu vertex layout
/// (`crate::renderer`): fed from next to the asserts, like
/// [`INSTANCE_OFFSETS`].
pub(crate) const FX_INSTANCE_OFFSETS: [u64; 4] = [
    offset_of!(FxInstance, pos) as u64,
    offset_of!(FxInstance, uv0) as u64,
    offset_of!(FxInstance, rgba) as u64,
    offset_of!(FxInstance, fx) as u64,
];

/// Identical, field by field, to the `cursor_rect`/`cursor_rgba` fields of
/// `shaders/cell.wgsl` → `Immediates`: the caret's **pixel** rectangle and the
/// colour of the text that remains under the block.
///
/// A **uniform**, not an instance: there is a single caret across the frame
/// and every fragment going through the `cell` pipeline looks at it. The two
/// values are in one `#[repr(C)]` struct because they answer one question
/// ("is this fragment under the block, and if so what colour") and a single
/// binding means a single layout contract — had they been bound separately
/// there would be no offset left to pin either.
///
/// The rectangle is min/max (`x0, y0, x1, y1`), not corner+size: the
/// fragment test comes down to two comparisons with no addition. **An
/// invisible caret is a degenerate rectangle** (all zero: `x >= 0 && x < 0`
/// is true for no fragment) — there is no second flag in the shader, because
/// a flag and a rectangle would be two truths that could drift apart.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct CursorBlock {
    /// `[x0, y0, x1, y1]`, pixels; top-left origin — the same space as the
    /// fragment's `[[position]]`.
    rect: [f32; 4],
    /// **Linear** RGBA of the text that remains under the block; the same
    /// space and the same warning as `Instance.rgba`. Its source is
    /// `bt_core::Cursor::text`, so the decision is `bt-core`'s.
    rgba: [f32; 4],
}

// The same rationale as `Instance`, the same pair of ties; the MSL side has
// its own `static_assert`s.
const _: () = assert!(size_of::<CursorBlock>() == 32);
const _: () = assert!(offset_of!(CursorBlock, rgba) == 16);

/// The caret's instance in this frame: position and colour.
///
/// A separate type because the caret has **two slots** (grid and dock) and
/// both carry the same data; keeping two `Option<Instance>`s would mean
/// writing `size` twice. The cell size is `Frame`'s own field, so it is not
/// here.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Caret {
    /// Top-left corner, in pixels **in window space**.
    at: [f32; 2],
    /// The block's colour; the alpha arrives baked in during the fade mode.
    rgba: [f32; 4],
}

impl Caret {
    /// Converts to an instance; the caller supplies the cell size, shape, rule
    /// thickness and halo margin ([`Frame`]'s fields).
    ///
    /// **The quad swells by the halo margin**, the painted rectangle does not:
    /// the halo lives outside the paint and the fragment draws it from the
    /// distance **outside** the rectangle. The swelling is only here, because
    /// slot selection ([`Frame::push_caret`]) **must** look at the unswollen
    /// rectangle — had the halo enlarged the footprint and moved the caret
    /// into the dock slot, the caret would be drawn after the grid's glyphs
    /// and paint over the letter beneath it (the same trap was fallen into
    /// once before).
    fn instance(self, cell_px: (f32, f32), shape: CaretShape, rule: f32, glow: f32) -> Instance {
        let (pos, size) = caret_painted_rect(self.at, cell_px, shape, rule);
        Instance {
            pos: [pos[0] - glow, pos[1] - glow],
            size: [size[0] + glow * 2.0, size[1] + glow * 2.0],
            rgba: self.rgba,
        }
    }
}

/// The halo's margin, as a **ratio of the left margin**.
///
/// **Chosen, not measured**, and corrected once by eye: the margin was
/// initially the whole left margin (~8 px at the default point size) and as
/// the halo grew as wide as the caret itself the result was neon rather than
/// a "box shadow" — which was what the user said at first glance. Half of it
/// stays in the shadow range.
///
/// A ratio, because the margin's source is still single:
/// `CellMetrics::gutter_px`. There is no second design constant and the halo
/// grows when the point size grows.
///
/// It came down in two rounds: whole left margin → half → **two fifths**;
/// both by eye.
///
/// **This is a floor**, not the setting itself: `[terminal] cursor_glow` is
/// its **multiplier** ([`bt_core::CaretStyle`]). Keeping the constant here
/// preserves the "no second design constant" rule — the user scales the
/// design's measure, they do not invent a new measure.
pub(crate) const CARET_GLOW_RATIO: f32 = 0.4;

/// The halo's peak alpha — this at the rectangle's edge, zero at the tip of
/// the halo margin.
///
/// **Chosen, not measured**, and again corrected by eye: 0.35 glowed around a
/// gold block. What was wanted is "a clean, light design touch like a box
/// shadow", that is, an alpha at shadow scale. It is **multiplied** by the
/// caret's own alpha, so when the blink goes dark the halo goes dark too
/// and no second path is written.
///
/// It came down in two rounds: 0.35 → 0.14 → **0.10**; both by eye. Like
/// [`CARET_GLOW_RATIO`] it is a **floor**: `cursor_glow` scales it with the
/// same multiplier, because margin and alpha are one feeling.
const CARET_GLOW_ALPHA: f32 = 0.10;

/// The caret's corner radius, in pixels — **including its clamp**.
///
/// The clamp is here, not in the shader: there must be a single party that
/// decides the radius's value and that party is the one that knows the cell
/// size. `caret_fragment` keeps its own `min` but that is a **mathematical
/// precondition, not a policy** (the SDF wants the radius not to exceed half
/// the extent); this function chooses the value. Tests read from here too,
/// otherwise the formula would have a third writer.
///
/// The ratio is an **argument**, not a constant: it comes from the
/// setting ([`bt_core::CaretStyle`]) and its default has a single owner,
/// `bt-core` ([`bt_core::CURSOR_RADIUS`]).
pub(crate) fn caret_radius_px(cell_px: (f32, f32), ratio: f32) -> f32 {
    (cell_px.1 * ratio)
        .min(cell_px.0 / 2.0)
        .min(cell_px.1 / 2.0)
}

/// The selection shape's corner radius, as a ratio of the cell **height** —
/// a design constant, not a measured number.
///
/// **Separate** from the caret's ratio ([`bt_core::CURSOR_RADIUS`]) and more
/// than twice as large: the selection is a surface wrapping a block of text,
/// whereas the caret is a block one cell tall — the same pixel radius made
/// the corner invisible on the selection (a by-eye check, the
/// user: "you could increase the radius value a bit"; at 13pt@2x ≈3 px → ≈7
/// px). A ratio, not pixels: the corner grows with Cmd +/−. The clamp is from
/// [`caret_radius_px`]: on a one-cell selection the radius does not exceed
/// half the cell's short side, so the shape is not broken even if it becomes
/// a pill. The user's `cursor_radius` does not touch this (the key
/// is the caret's).
pub(crate) const SELECTION_RADIUS: f32 = 0.22;

/// One corner of the selection shape; its order in
/// [`selection_corners`]'s array is TL, TR, BR, BL — `selection_fragment`'s
/// mask order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Corner {
    /// An exposed corner: the neighbouring row's run on that edge does not
    /// cover the corner's column (either there is no neighbour, or the column
    /// is outside it).
    Convex,
    /// The neighbouring run covers the corner and the edge is aligned on
    /// both: the shape continues straight.
    Square,
    /// The neighbour covers the corner **and** overshoots this run's edge:
    /// the corner is square, and a concave fill piece falls outside the step.
    /// The fill is always produced by the **narrow** run — the wide one's edge
    /// on that side is not covered by its neighbour, so every step is filled
    /// exactly once.
    Concave,
}

/// The four corners of `runs[index]` (TL, TR, BR, BL).
///
/// A neighbour is only the **adjacent** row's run: if a row with no run comes
/// between (a row with no drawable cell produces no run) the shape
/// splits there and the two pieces take their own corners. `runs` is in row
/// order with at most one run per row (`bt_core::SelectionRuns::as_slice`).
///
/// The decision is on the column, not the pixel: a corner looks at whether
/// the neighbour on that edge covers that **column**. Two runs that touch only
/// diagonally (the upper starts at 5, the lower ends at 4) do not cover each
/// other and give two convex corners — the usual state of a selection in
/// line flow.
pub(crate) fn selection_corners(runs: &[SelectionRun], index: usize) -> [Corner; 4] {
    let this = runs[index];
    let adjacent = |other: Option<&SelectionRun>, row: Option<u16>| {
        other.copied().filter(|o| Some(o.row) == row)
    };
    let above = index
        .checked_sub(1)
        .and_then(|i| adjacent(runs.get(i), this.row.checked_sub(1)));
    let below = adjacent(runs.get(index + 1), this.row.checked_add(1));
    // The left corner looks at the `first` column, the right corner at the
    // `last` column; if the neighbour covers that column it is square, and if
    // it also overshoots in that direction, concave.
    let left = |n: Option<SelectionRun>| corner(n, this.first, |n| n.first < this.first);
    let right = |n: Option<SelectionRun>| corner(n, this.last, |n| n.last > this.last);
    [left(above), right(above), right(below), left(below)]
}

/// One corner of [`selection_corners`]: does the neighbour cover column
/// `col`, and if it does, does it overshoot this edge.
fn corner(
    neighbour: Option<SelectionRun>,
    col: u16,
    extends: impl Fn(SelectionRun) -> bool,
) -> Corner {
    match neighbour {
        Some(n) if (n.first..=n.last).contains(&col) => {
            if extends(n) {
                Corner::Concave
            } else {
                Corner::Square
            }
        }
        _ => Corner::Convex,
    }
}

/// The caret's **two** rectangles; both are top-left corner + size, window space.
///
/// The invariant ("one place, two consumers") is not broken, it was
/// **under-defined**: the area a caret paints and the area in which it
/// inverts the text beneath are not the same thing. On a solid caret the two
/// are equal; **on a hollow caret there is paint but no opaque interior** —
/// the inversion rests on the painted ground.
#[derive(Clone, Copy, Debug, PartialEq)]
struct CaretRects {
    /// The area where the fragment draws the body — the halo lives **outside** it.
    painted: ([f32; 2], [f32; 2]),
    /// The inversion's area ([`CursorBlock`]). An empty rectangle = no
    /// inversion, not a separate flag.
    opaque: ([f32; 2], [f32; 2]),
}

/// The caret's rectangles: top-left corner and size, in pixels **in window
/// space**.
///
/// **One place, two consumers:** the painted quad ([`Caret::instance`]) and
/// the inversion rectangle ([`CursorBlock`]). Had they been written
/// separately one would narrow while the other stayed at the whole cell, and
/// the symptom would be silent — the letter under a thin bar would look
/// inverted across the whole cell.
///
/// **The thickness is not made up:** it comes from the font's own underline
/// metric (`CellMetrics::rule_px`), the chevron's precedent. It cannot exceed
/// the cell — at a small point size the metric can come out larger than the
/// cell and the caret would spill into the neighbouring cell.
fn caret_painted_rect(
    at: [f32; 2],
    cell_px: (f32, f32),
    shape: CaretShape,
    rule: f32,
) -> ([f32; 2], [f32; 2]) {
    // **`min`+`max`, not `clamp`**: `f32::clamp`
    // wants `min <= max` and `Frame::default()`'s cell is `(0.0, 0.0)` — a
    // `push_caret` arriving without `clear` would **panic** inside the
    // display link callback. Not a panic path as such, but it would kill a
    // window; at a zero cell the thickness also stays zero, i.e. a caret that
    // is not drawn.
    let limit = cell_px.0.min(cell_px.1);
    let thick = rule.max(1.0).min(limit);
    match shape {
        CaretShape::Block => (at, [cell_px.0, cell_px.1]),
        // At the **bottom** of the cell, not at the font's underline
        // position: that position is just below the baseline and there the
        // caret would cut off the tail of a `g`. What is taken from the
        // metric is the **thickness**, not the position.
        CaretShape::Underline => ([at[0], at[1] + cell_px.1 - thick], [cell_px.0, thick]),
        CaretShape::Beam => (at, [thick, cell_px.1]),
    }
}

/// Adds the inversion area on top of [`caret_painted_rect`].
///
/// **The painted area does not know the focus** and must not: a hollow caret
/// occupies the same place, it just does not paint its interior. The only
/// thing that depends on focus is the opaque interior.
fn caret_rect(
    at: [f32; 2],
    cell_px: (f32, f32),
    shape: CaretShape,
    rule: f32,
    hollow: bool,
) -> CaretRects {
    let rect = caret_painted_rect(at, cell_px, shape, rule);
    CaretRects {
        painted: rect,
        // **A hollow caret has no opaque interior.** The inversion rests on
        // the painted ground: the letter under an unpainted pixel must stay
        // in its own colour, otherwise the text in the middle of the frame
        // would be drawn in the ground colour and become **invisible**. The
        // empty rectangle is not a separate flag, it is `CursorBlock`'s own
        // contract ("an invisible caret is a degenerate rectangle").
        opaque: if hollow {
            ([0.0, 0.0], [0.0, 0.0])
        } else {
            rect
        },
    }
}

/// A glyph to draw — **without uv**.
///
/// Which slot the character falls into is not known here and must not be:
/// slot resolution borrows the atlas `&mut` and this list is filled in
/// `Session::frame`'s sink, i.e. without touching the `Renderer` at all.
/// Hoisting the resolution into the sink would keep the atlas borrow alive
/// across `draw` and give a `BorrowMutError` on the first frame with a glyph
/// (`link.rs` holds `frame` exactly that way). This split also gives a free
/// guarantee: the whole frame is drawn with **one** atlas generation, with no
/// generation counter needed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct GlyphCell {
    pub(crate) pos: [f32; 2],
    pub(crate) ch: char,
    /// Which font face to rasterize from; [`face`]'s translation of
    /// `(bold, italic)`. [`GlyphInstance`] does **not** carry it: uv0 encodes
    /// the slot, and the slot already encodes the face.
    pub(crate) face: Face,
    /// Which point-size class to rasterize from; an axis **orthogonal** to
    /// the face.
    ///
    /// Its only producer is [`Frame::push_dock`] and its only value is
    /// `Small` on the dock's context row. A separate list was not opened and
    /// the reason is the encode: `GlyphInstance` is not affected by this
    /// either (uv0 encodes the slot), so the small glyph is drawn in the same
    /// list, in the same draw call and with the same `cell_px` uniform. The
    /// quad stays large and the small letter sits on its left edge;
    /// neighbouring quads overlap but the overlapping pixels are transparent
    /// and the blend is `SourceAlpha`, so the letter underneath is not
    /// damaged.
    pub(crate) size: SizeClass,
    pub(crate) rgba: [f32; 4],
    /// The boundary's `Cell::wide`: whether this is the head cell of a
    /// character **two columns** wide.
    ///
    /// The field is **not** `half` and this is deliberate: the answer to
    /// "which half" is born in the ink gate, i.e. in `Atlas::slot`, and this
    /// list never sees the atlas (the reason the type has no uv is right
    /// above). The fan-out is therefore in `slots::fan`: there the atlas is
    /// already borrowed and the cell size is at hand.
    pub(crate) wide: bool,
    /// The boundary's `Cell::cluster`: the emoji sequence's id in the
    /// list's **own** table — the grid and the fill band use
    /// [`Frame::clusters`], the dock [`Frame::dock_clusters`], the ghosts
    /// [`Frame::fx_clusters`]. The string goes into the atlas at fan-out
    /// time (`slots::fan`, `Atlas::intern`), like `ch`: this list does not see
    /// the atlas.
    pub(crate) cluster: Option<ClusterId>,
}

/// Moves a cluster id from the `from` table to the `to` table: lists
/// that outlive their source (typing effects) copy the string into their own
/// tables. An id not found in the source is `None` — the glyph is drawn with
/// its base character, not with a wrong string.
pub(crate) fn copy_cluster(
    id: Option<ClusterId>,
    from: &Clusters,
    to: &mut Clusters,
) -> Option<ClusterId> {
    id.and_then(|id| from.get(id))
        .and_then(|text| to.push(text))
}

/// A typing effect to draw: its glyph and the effect's parameters — without
/// uv, for the same reason as [`GlyphCell`] (slot resolution at `encode`
/// time).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct FxCell {
    pub(crate) glyph: GlyphCell,
    /// Progress, `0..=1`.
    pub(crate) t: f32,
    /// The shader's effect id ([`crate::glyph_fx`]).
    pub(crate) effect: u32,
    pub(crate) seed: f32,
}

/// A rule line to draw — [`GlyphCell`]'s sibling and uv-less for the same
/// reason: slot resolution happens where the atlas borrow lives
/// (`encode_glyphs`).
///
/// It carries no face because rules are independent of the face (the line
/// under bold text is not bold); the caller always asks for them with
/// [`Face::Regular`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct RuleCell {
    pub(crate) pos: [f32; 2],
    pub(crate) kind: RuleKind,
    pub(crate) rgba: [f32; 4],
}

/// `bt_core`'s SGR flags → `bt_atlas`'s font face.
///
/// **The translation is here because it is the only place.** `bt-atlas` does
/// not see `bt-core` and must not: that edge would pull `alacritty_terminal`
/// into the pure-CoreText crate (a dependency is an
/// architectural decision); `bt-gpu` is the only layer that sees both. Their
/// four variants are the same but **their reasons are separate** — one is SGR
/// 1/3 semantics, the other a CoreText trait. If they are merged because
/// "they look the same", the layer direction inverts: the merged type would
/// go either into `bt-core` (`bt-atlas` cannot see it) or into `bt-atlas`
/// (`bt-core` cannot see it).
fn face(bold: bool, italic: bool) -> Face {
    match (bold, italic) {
        (false, false) => Face::Regular,
        (true, false) => Face::Bold,
        (false, true) => Face::Italic,
        (true, true) => Face::BoldItalic,
    }
}

/// The palette's colour + this frame's opacity.
///
/// The alpha does **not** go into [`LinearRgba`] and this is a consequence of
/// the layer rule: that type carries the palette's space (`bt-core`), whereas
/// opacity is this frame's drawing state. Opening an alpha field on the theme
/// would spawn a second truth that could be read as "translucent accent";
/// here only the last component changes.
fn with_alpha(rgba: LinearRgba, alpha: f32) -> [f32; 4] {
    let [r, g, b, _] = rgba.to_array();
    [r, g, b, alpha]
}

/// Underline variant → rule sprite; [`UnderlineStyle::None`] wants no line.
///
/// All five variants find their exact counterpart and the `Option` carries
/// only "no line" — [`RuleKind`]'s sixth ([`RuleKind::Strike`]) does not go
/// through this translation, because in SGR strikeout is not a variant of
/// underline but a separate flag.
fn rule_kind(underline: UnderlineStyle) -> Option<RuleKind> {
    match underline {
        UnderlineStyle::None => None,
        UnderlineStyle::Single => Some(RuleKind::Single),
        UnderlineStyle::Double => Some(RuleKind::Double),
        UnderlineStyle::Curl => Some(RuleKind::Curl),
        UnderlineStyle::Dotted => Some(RuleKind::Dotted),
        UnderlineStyle::Dashed => Some(RuleKind::Dashed),
    }
}

/// The dock's two colours, one per frame.
///
/// The row count is **not** here: the cells' layout depends on it and
/// the cells are pushed **before** the surface ([`Frame::open_dock`]), so the
/// count is in a separate field written before the cells
/// ([`Frame::set_dock_rows`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct DockSurface {
    /// The surface's ground; **opaque** (`bt_core::Dock::ground`). During the
    /// slide the grid's overflowing bottom row stays under it.
    ground: [f32; 4],
    /// The **top** hairline that separates the dock from the grid
    /// (`bt_core::Dock::edge`): on a remote session it is a separate colour
    /// so it cannot be the same field as the second line.
    edge: [f32; 4],
    /// The hairline that separates the input block from the context row.
    separator: [f32; 4],
    /// The filled share of the top line, `0..=1`
    /// ([`Frame::set_dock_progress`]); `None` → the line is entirely `edge`.
    progress: Option<f32>,
    /// The empty track of the progress bar (`bt_core::Dock::track`); drawn only while `progress` is present.
    track: [f32; 4],
    /// The upload row's buttons ([`Frame::set_dock_buttons`]); opening clears
    /// them every frame, so a button exists only in the frame it is told.
    buttons: [Option<DockButton>; 2],
}

/// Alpha of the upload button's fill and border, per state —
/// a **design constant**, the values of the approved design: a resting button
/// is a faint fill and a distinct border, under the pointer both darken, and
/// while pressed the fill goes one shade further.
///
/// The alpha is here, not in `bt-core`: the colour is the palette's (the
/// mark's colour), the opacity is this frame's drawing state ([`with_alpha`]'s
/// rationale).
const fn button_alpha(state: ButtonState) -> (f32, f32) {
    match state {
        ButtonState::Idle => (0.16, 0.38),
        ButtonState::Hover => (0.34, 0.7),
        ButtonState::Pressed => (0.5, 0.7),
    }
}

/// One draw of a rounded rectangle: the quad (dock-local), `caret_fragment`'s
/// core in window space and the shape uniform.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct RoundedDraw {
    pub(crate) instance: Instance,
    pub(crate) core: [f32; 4],
    pub(crate) shape: [f32; 4],
}

/// The share the PTY **reserves** for the dock, in rows: one input row + one
/// context row.
///
/// Two, because the dock's design is two rows: on top
/// `>` + ZLE's display, below `[folder] | [branch]`. The share is deducted
/// from the grid's height, so enlarging the number later would mean a second
/// `TIOCSWINSZ` that shortens the user's window by a row.
///
/// **Reserved, not drawn**: the dock's drawn band grows with the number
/// of input rows ([`band_px`]) but this share **never changes** — the shell
/// sees no SIGWINCH, the grid makes room by being offset upward in the
/// drawing.
///
/// A constant of this crate because this crate does the drawing; `bt-shell`
/// **consumes** it in the grid arithmetic (`split_into_grid`) and keeps no
/// second copy — the same discipline as the share being carried with
/// `CellMetrics`.
pub const DOCK_ROWS: u16 = 2;

/// The height the PTY **reserves** for the dock, in **pixels**; zero if
/// `dock_rows == 0`. This is not the drawn band's height: that is [`band_px`].
///
/// This is the formula's **only** copy and it has two consumers: the grid's
/// row arithmetic (`bt_shell`'s `split_into_grid`) and the second viewport's
/// origin ([`Frame::dock_layout_px`]). Had they been written separately they
/// would diverge for a frame during a resize — the same discipline as
/// `DOCK_ROWS` being consumed by `bt-shell`.
///
/// **Breathing room above and below the rows** (`2 *`): with the two rows
/// glued to the hairline the dock looked "ugly" (the user). The
/// user chose the adjacent + breathing-room look, not a detached surface.
///
/// The margin's source is **the left margin itself** ([`CellMetrics::gutter_px`]):
/// no second design constant was invented, the same inner indent is used on
/// two axes. A fixed pixel count would not do either — when the point size
/// grows with Cmd +/− the margin would stay the same and the proportion would
/// break; `gutter_px` is already multiplied by the scale.
pub fn dock_px(dock_rows: u16, cell: CellMetrics) -> f32 {
    dock_height(
        dock_rows,
        f32::from(cell.cell_px().1),
        f32::from(cell.gutter_px()),
    )
}

/// The height of the dock's **drawn** band, in pixels: `input_rows` input
/// rows plus the context row.
///
/// The sibling of [`dock_px`] (the share the PTY **reserves**) and a separate
/// name from it, because the two numbers now diverge: the share is fixed at
/// [`DOCK_ROWS`], the band grows with the input rows, and the difference is
/// closed by the grid being offset upward in the drawing. At
/// `input_rows == 1` the two are the **same** pixels.
///
/// There is no gap between input rows — a single editor surface; the gap and
/// the second hairline are only between the input block and the context row
/// The formula's body is still [`dock_height`]:
/// the band is a dock of `input_rows + 1` rows.
///
/// `None` is **no band** (`bt_core::Cursor::band_rows`): zero pixels — a
/// program reading the keyboard itself has the dock step aside, and the
/// grid is drawn the whole PTY share lower. A height, not a terminal
/// concept: this layer only learns that the band is empty.
pub(crate) fn band_px(input_rows: Option<u16>, cell: CellMetrics) -> f32 {
    input_rows.map_or(0.0, |rows| dock_px(rows.saturating_add(1), cell))
}

/// The ceiling of the dock's input rows: **half** of the grid's rows — a
/// **design constant**, not a measured number (the precedent of
/// [`bt_atlas::CONTEXT_SCALE`]).
///
/// Rationale: the surface where the command is typed and the output that is
/// its context should stay equal, and the editor should not swallow the
/// window. It is a **ratio**, not an absolute number: it scales with the
/// window and the point size. On an input that overflows, the dock opens a
/// vertical window inside itself that follows the caret (`bt_core`'s
/// `dock::window_top`).
///
/// The layout decision is the drawer's, hence the constant is here; `bt-core`
/// takes it as a budget ([`bt_core::DockBudget::share`]) and applies it to
/// the single reading of the grid's row count — this layer keeps no second
/// copy of the row count (`Layout`'s doc).
pub(crate) const DOCK_MAX_SHARE: f32 = 0.5;

/// The context row's column budget: **the same pixel strip, a small step**.
///
/// The dock shares the left margin with the grid ([`Frame::dock_pos`]), so
/// the horizontal strip the two rows occupy is exactly the same; the only
/// thing that differs is how many pixels a letter advances. The budget is
/// therefore a ratio: `cols * cell / context cell`.
///
/// The computation must be here — `bt-core` sees no pixels and must not
/// (`dock::render`'s `context_cols` is a **budget**, not a point-size
/// decision). It is multiplied in `u32`: 65535 columns × 65535 pixels would
/// overflow `u16`, whereas the intermediate value only enters a ratio.
///
/// The divisor is ≥ 1 and this is **structural**: [`CellMetrics::new`]
/// rejects a zero context width, so there is no second gate here.
pub fn context_cols(cols: u16, cell: CellMetrics) -> u16 {
    let span = u32::from(cols) * u32::from(cell.cell_px().0);
    u16::try_from(span / u32::from(cell.context_cell_px())).unwrap_or(u16::MAX)
}

/// The top of the context row's cell band, in pixels measured from the
/// **bottom** of the input block: the row gap if there is an input row,
/// otherwise (remote session) zero — [`Frame::dock_pos`]'s rule. The
/// mouse reads the upload button's vertical span from this:
/// the fill is exactly in that band ([`Frame::dock_button_draws`]).
pub fn context_row_offset(input_rows: u16, cell: CellMetrics) -> f32 {
    if input_rows == 0 {
        0.0
    } else {
        dock_row_gap(f32::from(cell.gutter_px()))
    }
}

/// The formula's body, with raw numbers: separate so that [`dock_px`] and
/// [`Frame`] share the same arithmetic. `Frame` cannot hold [`CellMetrics`]
/// as a field (its constructor rejects zero, so it has no `Default`), but it
/// already has the two components.
///
/// **A single row gap**: between the context row and the input block
/// above it. The input rows are adjacent among themselves, so a dock of
/// `rows` rows is `rows · cell_h + 2 · pad + gap`; a one-row dock (only in
/// tests) has no gap. Before multi-row input there was a gap between every row, and for a
/// two-row dock the two formulas give the same number.
fn dock_height(rows: u16, cell_h: f32, pad: f32) -> f32 {
    if rows == 0 {
        return 0.0;
    }
    let gap = if rows >= 2 { dock_row_gap(pad) } else { 0.0 };
    f32::from(rows) * cell_h + 2.0 * pad + gap
}

/// The gap **between** the dock's rows, in pixels.
///
/// **Twice** the outer margin, and this number is not a taste but the result
/// of a single rule: since a hairline comes between, each row became its own
/// **band** and the inside of a band must be symmetric. With one `pad` on
/// each side of the line, the dock's four gaps come out equal:
///
/// ```text
///   ─────────── top hairline
///        pad
///   input row
///        pad
///   ─────────── line between rows
///        pad
///   context row
///        pad
///   ─────────── bottom of the dock
/// ```
///
/// It used to be `pad / 2` and its rationale was "the outer gap is larger
/// than the inner one". That rule is right for **groups** but there is no
/// group here: the line turns the two rows into two separate things, and then
/// above the input row there was `pad` and below it `pad / 2` — the user saw
/// it ("the lines from the bottom and from the top should be at the same
/// distance"), and it was something they should not have had to see.
///
/// It is rounded, because an offset that does not land on the device grid
/// would blur all the dock text — the same rationale as
/// `Frame::set_origin_rows`.
fn dock_row_gap(pad: f32) -> f32 {
    (pad * 2.0).round()
}

/// The separator's thickness, in **pixels**.
///
/// It is not multiplied by the scale and this is deliberate: a hairline is a
/// line, and being two pixels at @2x would thicken it — the fineness the
/// retina display brings is exactly this. Not a measured number, a design
/// constant (the precedent of `CellMetrics::GUTTER_PT`).
const SEPARATOR_PX: f32 = 1.0;

/// What is to be drawn in a single frame.
///
/// Cell backgrounds and the caret live in the same list: both are drawn with
/// the same pipeline and the order is the draw order (the caret is appended at
/// the end so that it comes on top of the backgrounds). The glyphs are in a
/// separate list with the second pipeline, drawn on top of the caret too —
/// the caret is opaque and would cover the letter beneath it.
///
/// It is long-lived: the display link holds it in an ivar and refills it with
/// `clear` in a **content** frame. For that reason the grid geometry (cell
/// size and left margin) is **a parameter of `clear`, not a field** — had it
/// been frozen in the constructor it would silently go stale when the screen
/// scale changes (`windowDidChangeBackingProperties:`).
///
/// **Not every frame sees `clear`, and this is the distinction motion frames brought:**
/// a motion frame draws without finding the grid dirty, so it cannot clear the
/// list — [`Frame::move_caret`] moves only the caret, preserving it. The only
/// place `clear` is called is the content frame.
#[derive(Default)]
pub(crate) struct Frame {
    /// The command blocks' stripes in the left margin; through the **same**
    /// pipeline as the backgrounds but in a separate list.
    ///
    /// The reason for the separation is not draw order but lifetime: had it gone into `bg`, it would go in either uncounted — back
    /// then the motion frame trimmed `bg` to `bg_count` and the stripe would
    /// **flicker** as the caret glided; now the caret has its own slot
    /// ([`Frame::grid_caret`]) and its own pipeline — it never
    /// enters the background list and the trimming is gone, but the
    /// rationale for the separation stands — or counted, and the meaning of
    /// the `cells=` token would drift ("drawn cells" would now also count
    /// something that is not a cell). A third list makes both
    /// unrepresentable.
    stripes: Vec<RuleCell>,
    bg: Vec<Instance>,
    /// The mouse selection's row runs and concave fills; drawn from its
    /// own pipeline (`selection`, a corner-masked SDF), **after** the
    /// backgrounds and before the caret and the glyphs
    /// ([`Renderer::plan`](crate::renderer::Renderer)).
    ///
    /// The reason for the list's separation beyond the pipeline is the same
    /// as the stripe's: had it gone into `bg`, it would go in either counted
    /// and the meaning of the `cells=` token would drift ("drawn cells" would
    /// count a row run too), or uncounted and it would defeat the guard on
    /// `bg_count`. It has **no** counter: the smoke recipe has no selection,
    /// so the token would say nothing; the proof is `renderer.rs`'s offscreen
    /// read.
    selection: Vec<Instance>,
    /// The dock's selection run: the twin of `selection` on the
    /// dock surface — the same pipeline, the same colour and radius uniform,
    /// in the dock's own viewport ([`Frame::push_dock_selection`]). A
    /// separate list, because the grid's list slides with the offset while
    /// the dock is exempt from it.
    dock_selection: Vec<Instance>,
    /// The selection's colour in this frame ([`Frame::push_selection`] writes
    /// it); a motion frame keeps it just as it keeps the list.
    selection_rgba: [f32; 4],
    /// The search highlight's pieces, from `selection`'s pipeline and
    /// shape: all the matches (`search_match`) and the current match
    /// (`search_current`). **One list per role**, because the colour is a
    /// per-call uniform — two roles, two encodes ([`Frame::push_search`]).
    /// Separate from the selection, because the selection is drawn **on top
    /// of** the search and carries its own colour. No counter, the
    /// same rationale as the selection's.
    search_match: Vec<Instance>,
    search_current: Vec<Instance>,
    /// The two roles' colours in this frame; like `selection_rgba` they are
    /// kept in a motion frame.
    search_match_rgba: [f32; 4],
    search_current_rgba: [f32; 4],
    /// A scratch buffer for the per-match corner decision
    /// ([`Frame::search_parts`]): no per-frame allocation.
    search_scratch: Vec<SelectionRun>,
    glyphs: Vec<GlyphCell>,
    /// Rule lines; through the **same** pipeline as the glyphs but drawn after
    /// them (a strikeout must pass over the letter beneath it).
    rules: Vec<RuleCell>,
    cell_px: (f32, f32),
    /// The column step on the dock's context row, in pixels
    /// ([`CellMetrics::context_cell_px`]). A field for the same reason as
    /// `cell_px`: a motion frame does not call `clear` and **keeps** the
    /// value.
    context_cell_px: f32,
    /// The rule line's thickness, in pixels — the width of the thin carets
    /// ([`caret_rect`]). A field for the same reason as `cell_px`: a motion
    /// frame does not call `clear` and **keeps** the value.
    rule_px: f32,
    /// The caret's shape; [`Frame::push_caret`] writes it, [`Frame::move_caret`]
    /// keeps it — that path never goes to `bt-core`.
    caret_shape: CaretShape,
    /// Whether the caret is hollow — the mark of an unfocused window.
    ///
    /// It lives in [`Frame`] rather than being carried in a signature and
    /// forgotten: a motion frame ([`Frame::move_caret`]) never goes to
    /// `bt-core` and does not know the focus. Without the field the caret
    /// would fill in on the first motion frame of an unfocused window.
    ///
    /// It was **not added** to `CaretShape`: that enum is the settings
    /// file's vocabulary (`"block" | "underline" | "beam"`) and focus is an
    /// axis **orthogonal** to the shape.
    caret_hollow: bool,
    /// Whether the window is focused — [`Frame::push_caret`] writes it,
    /// [`Frame::move_caret`] **keeps** it.
    ///
    /// The **setting-derived** drawing numbers of the caret; the second
    /// argument of `clear` writes them, a motion frame keeps them (it does not
    /// call `clear`).
    ///
    /// It was **not loaded onto** `CellMetrics`: that is font
    /// geometry and has 32 call sites; the doc of `GUTTER_PT` already says
    /// "a constant, not a setting".
    caret_style: CaretStyle,
    /// The grid's left margin: every cell's x starts **after** it.
    ///
    /// A value [`Frame::clear`] carries rather than a field, for the same
    /// reason as `cell_px` (both arrive with one [`CellMetrics`]): when the
    /// scale changes the two are refreshed together. Had it been read from a
    /// separate constant it could diverge from the `cols` computation — its
    /// having a single source for all three is the condition for the gutter.
    gutter_px: f32,
    /// The **drawn** top of the dock band, **in pixels in window space**; if
    /// there is no dock, infinity (the caret never falls into the dock slot).
    ///
    /// The caller writes it ([`Frame::set_dock_band`], in the same call as the
    /// band's height), because the only place that knows the height of the
    /// touch is the frame path; `Frame` knows the lists' space, not the
    /// window's (the same rationale as [`Frame::dock_ground`] taking the
    /// width as an argument).
    dock_top_px: f32,
    /// The dock's **layout** rows: input rows + the context row
    /// ([`Frame::set_dock_rows`]). The cells' positions ([`Frame::dock_pos`])
    /// and which row is the context row come from here; the band's **drawn**
    /// height ([`Frame::dock_band`]) is the animation's current value and
    /// separate from it.
    ///
    /// [`Frame::clear`] returns it to [`DOCK_ROWS`]; since a motion frame does
    /// not call `clear` it keeps the value — the cells are kept too.
    dock_rows: u16,
    /// Whether the layout's **last row is the context row**
    /// ([`Frame::is_context_row`]).
    ///
    /// It is not derived from the row count, because a one-row layout has two
    /// meanings: in production a remote session's band consisting of the
    /// context row alone ([`Frame::set_dock_input_rows`] with zero) and
    /// in tests a single input row without context ([`Frame::set_dock_rows`]).
    /// The second stays under the "two or more → last row is context" rule.
    dock_context: bool,
    /// The drawn band's **excess** over the PTY share, in pixels (rounded to
    /// the device grid); `None` → this frame did not say and the band is the
    /// layout's height ([`Frame::set_dock_band`]).
    ///
    /// Its two consumers read the same number and must not diverge: the
    /// band's height ([`Frame::dock_band_px`]) and the grid's drawn origin
    /// ([`Frame::origin_px`], `− excess`). The grid's bottom edge and the
    /// band's top edge therefore slide together **structurally** — two
    /// separate roundings could differ by a pixel.
    ///
    /// It is read at encode time (the precedent of `fill_origin_px`) and
    /// **both** frame paths write it: a motion frame does not call `clear`
    /// but the band advances in its frame too.
    dock_band: Option<f32>,
    /// The PTY share the band's excess is measured from, rows; `None` is
    /// [`DOCK_ROWS`]. One on a remote session's alternate screen, where the
    /// dock is the context row alone and reserves exactly that
    /// ([`Frame::set_dock_share`]).
    dock_share: Option<u16>,
    /// The bottom of the window, in pixels — the base of the band's
    /// bottom-anchored top and of the dock geometry published to the mouse
    /// ([`Frame::set_dock_band`]).
    dock_bottom_px: f32,
    /// The caret's grid slot: drawn after the grid's backgrounds and before
    /// its glyphs.
    grid_caret: Option<Caret>,
    /// The caret's dock slot: drawn after the dock's opaque ground, i.e. on top
    /// of everything.
    dock_caret: Option<Caret>,
    /// The grid's vertical origin, in pixels: the content starts this much
    /// **lower**.
    ///
    /// The vertical twin of `gutter_px` but **not baked into the lists**:
    /// [`Frame::pos_at`] does not see it and must not. The reason is order —
    /// the sink runs inside the loop and bakes the cell into an `Instance` at
    /// push time, whereas the fill count is born only when the loop ends
    /// (`bt_core::Cursor::content_rows`). The offset is therefore applied at
    /// **draw time**: `setViewport` shifts all four of the grid's lists at
    /// once ([`crate::Renderer`]) at zero per-instance cost.
    ///
    /// **Its second reader is the fill band** ([`Frame::fill_origin_px`]): it
    /// has its own viewport but its origin is derived from here
    /// (`origin_px − fill_px`), so the band slides **together with** the
    /// grid. The derivation is also at read time, otherwise it would go stale
    /// in a motion frame.
    ///
    /// The one exception is the caret: its target is a **screen** row and it
    /// is exempt from the offset, so its instance **gives the offset back**
    /// on the CPU while the rectangle never takes it at all
    /// ([`Frame::push_caret`]).
    ///
    /// **Pixels**, not rows, and `f32`: the slide rests between two rows. The
    /// value itself is still **rounded to a device pixel**
    /// ([`Frame::set_origin_rows`]) — what is fractional is the **row**, not
    /// the pixel.
    ///
    /// [`Frame::clear`] zeroes it, so a content frame must restate the value
    /// every frame; a motion frame does not call `clear` but **still writes**
    /// the offset (`link.rs`'s second write point): the slide advances between
    /// two content frames and a preserved value cannot advance.
    ///
    /// **The scroll fraction is not here** ([`Frame::frac_px`]): the
    /// viewport's origin is the sum of the two and only [`Frame::origin_px`]
    /// gives that sum, so the call order of the two setters does not change
    /// the result.
    origin_px: f32,
    /// The scroll fraction (`bt_core::Cursor::scroll_frac`), **rounded to a
    /// device pixel** — the second term of [`Frame::origin_px`].
    ///
    /// Kept **separate** from the offset and rounded separately, because its
    /// two consumers diverge: the grid's viewport wants the sum of the two,
    /// while the caret wants **only this** ([`Frame::push_caret`]) — the caret
    /// is exempt from the offset's slide (its target is a screen row) but not
    /// from the fraction, which shifts the grid's whole world, the caret
    /// beneath included. Had the sum been written with a single rounding, the
    /// caret's share could not have been subtracted back out of it and a
    /// settled caret could drift a pixel from its letter.
    ///
    /// **Shorter than a cell** ([`Frame::set_scroll_frac`]): rounding would
    /// lift a fraction above `1 − ½/h` to a full cell and the grid would be
    /// drawn a row lower without the offset changing.
    ///
    /// [`Frame::clear`] zeroes it (a content frame restates it every frame);
    /// a motion frame **keeps** it, because it does not call `clear` and the
    /// fraction can change only in a content frame — while a glide is in
    /// flight the link falls onto the content path.
    frac_px: f32,
    /// The caret's pixel rectangle and the colour of the text under the block;
    /// the `cell` pipeline's uniform.
    ///
    /// A **field**, not a list: there is a single caret per frame and
    /// [`Frame::clear`] returns it to degenerate. Its being a field is also a
    /// condition of the motion frame: that path empties the slots with
    /// [`Frame::move_caret`] and calls [`Frame::push_caret`] again with the new
    /// position, so the second call must overwrite the first.
    cursor: CursorBlock,
    /// The caret's **painted** rectangle (x0, y0, x1, y1), window space.
    ///
    /// A separate field from [`CursorBlock::rect`] and the separation is a
    /// condition of the hollow caret: on a hollow caret there is paint but no opaque
    /// interior. Degenerate (all zero) = no caret to draw.
    caret_core: [f32; 4],
    /// The SDF uniform's **test override**; always `None` in production.
    ///
    /// **A half override**: it only flips the fragment
    /// uniform, not the quad's swelling by the halo margin
    /// ([`Frame::glow_px`]). So enlarging the halo margin here does not
    /// enlarge the quad and the halo cannot extend outside the core. The tests
    /// therefore turn the halo on **with a margined grid**, not with the
    /// override; the override is only to drive the radius and the edge.
    ///
    /// The single guard of the rollback path goes through here: the
    /// output of the "radius 0, halo 0" arm must be **bit for bit** the same as
    /// the plain quad and only the GPU can say that — in the degenerate arm
    /// the fragment uses `step`, in the open arm `smoothstep`, and their edge
    /// pixels would diverge. A constructor in production would be dead code.
    #[cfg(test)]
    caret_sdf_override: Option<[f32; 4]>,
    /// The dock surface: the **second coordinate space** below the window.
    ///
    /// An `Option`, because the dock is decided as the session is born
    /// (integrated zsh or not) and `None` means "no dock in this window" —
    /// `bt-shell` computes the grid height accordingly too. The difference
    /// between an empty dock and `None` is visible: an empty dock draws its
    /// ground, `None` draws nothing.
    dock: Option<DockSurface>,
    /// The dock's own backgrounds **and caret**; the twin of the grid's `bg`.
    ///
    /// Being a separate list has the **same** rationale as `stripes` and is one
    /// degree more compelling: had the dock entered the grid's list it would
    /// be tied to the grid's frame lifetime and torn from its own viewport.
    /// It also does not enter the counters (`bg_count`, `glyph_count`,
    /// `rule_count`): the `cells=8 glyphs=6 rules=15` smoke contract is
    /// measured in a shell without a dock and its meaning must be preserved
    /// bit for bit.
    dock_bg: Vec<Instance>,
    dock_glyphs: Vec<GlyphCell>,
    dock_rules: Vec<RuleCell>,
    /// The clusters' tables: they live and are cleared
    /// **together with** the lists, so a motion frame (which keeps the lists)
    /// draws the same ids with the same strings. Three tables, because there
    /// are three writers: `frame()` fills the grid and the fill band in one
    /// call (`clusters`), `dock()` is a separate call (`dock_clusters`), and
    /// the ghosts are rebuilt every frame from effects whose lifetime exceeds
    /// the dock table (`fx_clusters`, [`Frame::set_dock_fx`]). The arrivals
    /// are copies of the static glyph, so they read the dock table.
    ///
    /// While being filled the table is outside `Frame` ([`Frame::take_clusters`]):
    /// since the sinks borrow `Frame` they cannot be given a second `&mut` in
    /// the same call (the rationale of `fill`).
    clusters: Clusters,
    dock_clusters: Clusters,
    fx_clusters: Clusters,
    /// The dock's typing effects: the ghosts of deleted glyphs and the
    /// arriving glyphs. Two lists, because their draw orders are separate —
    /// ghosts **before** the dock glyphs, arrivals **after**
    /// ([`crate::Renderer`]'s `encode_dock`).
    ///
    /// **It has two writers** ([`Frame::set_dock_fx`]): the content frame and
    /// the motion frame. The motion frame does not call `clear` and the dock's
    /// static lists are kept; only these two change. They do not enter the
    /// counters.
    dock_ghosts: Vec<FxCell>,
    dock_arrivals: Vec<FxCell>,
    /// `heat`'s hot colour: the theme's `cursor` role, linear. A single value
    /// per frame rather than per effect, and it goes to the fragment as a
    /// uniform (`glyph_fx.wgsl` → `Immediates::heat`); it has no place in the
    /// instance (`FxInstance`'s `fx` has only one spare `f32`). Its writer is
    /// the same as the lists' ([`Frame::set_dock_fx`]), so the two cannot
    /// diverge.
    dock_fx_heat: [f32; 4],
    /// The dock glyphs with the static glyph of in-flight arrivals
    /// **removed**.
    ///
    /// `dock_glyphs` itself does not change and this is a condition: a motion
    /// frame does not re-push the dock, so the static glyph of an arrival whose
    /// effect has finished must still be there to come back. When no arrival
    /// is in flight this list is not read ([`Frame::dock_glyphs`]).
    dock_shown: Vec<GlyphCell>,
    /// The history rows that fill the gap at the top; the **third** twin of
    /// the grid's `bg` (after `stripes` and `dock_bg`).
    ///
    /// The rationale for the separation is the same as the dock's, with one
    /// more end: the band has its own viewport ([`Frame::fill_origin_px`]) and
    /// the row numbers are **fill-local**, i.e. they collide with the grid's —
    /// they could not be told apart in a single list and, drawn from the
    /// grid's space, would land on top of the content instead of in the
    /// band's place. They do not enter the counters either (`bg_count`,
    /// `glyph_count`, `rule_count`): the `cells=8 glyphs=6 rules=15` smoke
    /// contract is measured in a shell with no fill and its meaning must be
    /// preserved bit for bit.
    fill_bg: Vec<Instance>,
    fill_glyphs: Vec<GlyphCell>,
    fill_rules: Vec<RuleCell>,
    /// The fill band's search highlight: the band's rows are
    /// real history and their matches are highlighted like the grid's — the
    /// rows are fill-local, drawn in the band's viewport. The colours are the
    /// same uniform as the grid's. There is no selection drawing in the band.
    fill_search_match: Vec<Instance>,
    fill_search_current: Vec<Instance>,
    /// The fill band's height, in **rows** (`bt_core::Cursor::fill`); zero →
    /// no band and the third viewport is never set up.
    ///
    /// A field separate from the lists, in the `DockSurface` precedent: while
    /// it is zero nothing is drawn even if the lists are full, so a
    /// "half-opened fill" cannot be represented. The band's height is the
    /// boundary's own number and is not derived from the cells — a fill row
    /// with no cells (entirely empty) must also take up room in the band.
    fill_rows: u16,
    /// The number of **background** instances drawn; the caret is not counted.
    ///
    /// `make smoke`'s `cells=K` token reads this: the proof that the sink
    /// produced cells. Had the caret entered the count, K would be 1 even on
    /// an empty grid and defeat the claim. Note: this is a **CPU** counter; it
    /// does not prove the GPU painted those cells — the `renderer` offscreen
    /// read test does that.
    bg_count: usize,
}

// All of it is `pub(crate)`: the only place that fills `Frame` is `link.rs`,
// i.e. this crate. The frame list is a GPU detail; `bt-shell` has no reason
// to see it and, if it does not, it cannot fill it with a wrong cell size.
impl Frame {
    /// Empties the buffers and sets up this frame's grid geometry. Allocated
    /// space is kept: no per-frame reallocation.
    ///
    /// [`CellMetrics`] rather than a tuple: the cell size and the left margin
    /// come from the same call and are written together here. With two
    /// separate parameters one could be refreshed and the other forgotten, and
    /// the symptom would be "the glyphs are shifted by the margin".
    pub(crate) fn clear(&mut self, metrics: CellMetrics, caret: CaretStyle) {
        self.caret_style = caret;
        let cell_px = metrics.cell_px();
        self.stripes.clear();
        self.bg.clear();
        self.selection.clear();
        self.dock_selection.clear();
        self.search_match.clear();
        self.search_current.clear();
        self.glyphs.clear();
        self.rules.clear();
        self.bg_count = 0;
        // The dock is in the content frame's contract too: its surface is
        // **reopened every frame** (`Frame::open_dock`). Had it been kept, in
        // a session with no dock the last frame's surface would hang on
        // screen.
        self.dock = None;
        self.dock_bg.clear();
        self.dock_glyphs.clear();
        self.dock_rules.clear();
        self.clusters.clear();
        self.dock_clusters.clear();
        // The effects too: in a frame with no dock (alternate screen) the
        // previous frame's ghost must not hang on.
        self.dock_ghosts.clear();
        self.dock_arrivals.clear();
        self.dock_shown.clear();
        self.fill_bg.clear();
        self.fill_glyphs.clear();
        self.fill_rules.clear();
        self.fill_search_match.clear();
        self.fill_search_current.clear();
        // **The band is restated every frame too** and zeroing it carries
        // the same rationale as the dock's surface: had it been kept, on the
        // first frame that turns the fill off (Ctrl-L, entering the alternate
        // screen, a window with no dock) the previous frame's band would hang
        // above the grid. Zero is at the same time the rollback strip's gate —
        // `Renderer::plan` never sets up the third viewport.
        self.fill_rows = 0;
        // The rectangle is zeroed too ([`Frame::clear_caret`]): had it stayed,
        // the caret going dark (`\e[?25l`) or scrolling into history would
        // remove the block from the screen but leave **the colour of the text
        // beneath it** in its old place — a ground-coloured letter, i.e. an
        // invisible cell.
        self.clear_caret();
        self.cell_px = (f32::from(cell_px.0), f32::from(cell_px.1));
        self.context_cell_px = f32::from(metrics.context_cell_px());
        self.rule_px = f32::from(metrics.rule_px());
        self.gutter_px = f32::from(metrics.gutter_px());
        // **Infinity**, not zero: zero would mean "the dock band is at the
        // top of the window" and every caret would fall into the dock slot.
        // The caller overwrites it in every dock frame
        // ([`Frame::set_dock_band`]) — the motion frame too, because the band
        // advances in its frame as well and the caret's slot decision must
        // look at the band's current top.
        self.dock_top_px = f32::INFINITY;
        // The layout returns to a single input row and the band to "not
        // said": the content frame restates both, and a frame that does not
        // (no dock) never adds the band to the grid.
        self.dock_rows = DOCK_ROWS;
        self.dock_context = true;
        self.dock_band = None;
        self.dock_bottom_px = 0.0;
        // The origin is **zeroed**, not derived from geometry: its source is
        // this frame's fill count and that is known only when the sink loop
        // ends. Leaving it at zero means "this frame has not said yet" and a
        // frame that does not say draws today's (ceiling-glued) layout — better
        // than a silent wrong offset.
        self.origin_px = 0.0;
        // The fraction is under the same contract: a frame that does not say
        // draws on a whole row.
        self.frac_px = 0.0;
    }

    /// This frame's vertical origin, in **rows**: the content starts this much
    /// lower.
    ///
    /// It takes rows and stores pixels, because the conversion needs the cell
    /// height and that is only here (`cell_px`, written by `clear`). Having the
    /// caller (`link.rs`) deal with pixels would have made it a second reader of
    /// the cell size.
    ///
    /// **A fractional row is legitimate, a fractional pixel is not.** The slide
    /// rests between two rows but the pixel is **rounded to the device grid**,
    /// because where the slide stops is permanent on screen: the link's "no
    /// damage" branch sleeps without ever drawing the frame in which the
    /// animation *settled*, so the last frame left on screen is one step
    /// before settling. For the caret that is a difference below half a pixel
    /// (`crate::motion::POS_EPSILON`), but for **all the text** it is a grid
    /// shifted by half a pixel — a screen that blurs after every Enter.
    /// Rounding closes that and also sharpens the slide itself: the text
    /// advances in whole-pixel steps.
    ///
    /// The second consequence of taking rows and storing pixels: rounding can
    /// only be done where the cell height is known.
    ///
    /// **The scroll fraction is rounded separately**
    /// ([`Frame::set_scroll_frac`]) and [`Frame::origin_px`] adds the two: the
    /// sum differs from a single rounding by at most one pixel — and that only
    /// while the offset is sliding and the fraction is also above zero, i.e. in
    /// a frame where two movements overlap. The gain is in the settled state:
    /// the caret's share comes from the fraction itself, not from the sum, and
    /// it stays at bit-for-bit the same pixel as its letter.
    pub(crate) fn set_origin_rows(&mut self, rows: f32) {
        debug_assert!(self.cell_px.1 > 0.0, "clear(metrics) was not called");
        self.origin_px = (rows * self.cell_px.1).round();
    }

    /// This frame's scroll fraction, in **rows** (`[0, 1)`,
    /// `bt_core::Cursor::scroll_frac`): the grid will be drawn this much lower.
    ///
    /// Called **before** the caret ([`Frame::push_caret`] adds the fraction to
    /// the grid's caret); its order relative to the offset is free. A zero
    /// fraction leaves the frame bit for bit the same as today's: a rounded
    /// zero, an added zero.
    ///
    /// The pixel is clipped to **shorter than a cell**: the `[0, 1)` row
    /// contract must still hold after rounding — a full cell would draw a grid
    /// whose offset has not changed a row lower, make the top row wholly
    /// unselectable and bury the last row's caret under the dock.
    pub(crate) fn set_scroll_frac(&mut self, frac: f32) {
        debug_assert!(self.cell_px.1 > 0.0, "clear(metrics) was not called");
        self.frac_px = (frac * self.cell_px.1)
            .round()
            .clamp(0.0, (self.cell_px.1 - 1.0).max(0.0));
    }

    /// This frame's vertical origin, in pixels; `setViewport`'s `originY` —
    /// the offset plus the scroll fraction, **minus the band's excess**.
    ///
    /// As the band's drawn height exceeds the PTY share, the grid is drawn
    /// that much higher: the full grid's top is clipped, and the fill band and
    /// the mouse mapping read the same value ([`Frame::dock_band`]). If there
    /// is no band or it is the share's height, the term is zero and the frame
    /// is bit for bit the same as today's.
    pub(crate) fn origin_px(&self) -> f32 {
        self.origin_px + self.frac_px - self.band_excess().unwrap_or(0.0)
    }

    /// The band's excess as both consumers read it ([`Frame::dock_band`]):
    /// **never below minus the share**. A band sliding to no band at all can
    /// overshoot its target on a spring; a deeper excess would draw a negative
    /// band and push the grid's bottom row out of the window while the band's
    /// top stays at its bottom. Clamped once, here, so the grid's bottom edge
    /// and the band's top still meet in the overshooting frames.
    fn band_excess(&self) -> Option<f32> {
        let share = dock_height(
            self.dock_share.unwrap_or(DOCK_ROWS),
            self.cell_px.1,
            self.gutter_px,
        );
        self.dock_band.map(|extra| extra.max(-share))
    }

    /// The sink's single entry: if the cell has a background it is painted,
    /// if it has ink it is drawn, if it has a rule it is drawn — if it has all
    /// three, all three.
    ///
    /// A cell produces **up to two** rules: underline and strikeout. They are
    /// separate fields in `bt-core` because they are separate in SGR; when they
    /// meet in the same cell both are drawn.
    pub(crate) fn push(&mut self, cell: Cell) {
        // The arithmetic common to the four branches (background, glyph,
        // underline, strikeout) once: calling `pos()` four times per cell has
        // no gain and would mean four copies that could diverge.
        let pos = self.pos(cell.col, cell.row);
        if let Some(bg) = cell.bg {
            // The counter and the list having the **same** length is the
            // proof that nothing not counted, like the caret, leaked into
            // `bg`; the caret has its own slot
            // ([`Frame::grid_caret`]) and never enters here.
            debug_assert_eq!(
                self.bg.len(),
                self.bg_count,
                "an instance that is not counted leaked into `bg`"
            );
            self.bg.push(Instance {
                pos,
                size: [self.cell_px.0, self.cell_px.1],
                rgba: bg.to_array(),
            });
            self.bg_count += 1;
        }
        // A cell with no ink produces no glyph: it would spend a slot in the
        // atlas, an instance in the buffer and a fully transparent quad on the
        // GPU. `bt-core` makes the distinction (a space, hidden text and the
        // second cell of a wide character are all `None`); there is no flag to
        // ask here. The rule branches do **not depend** on it: an underlined
        // space has no ink but gets its line (there are seven in the smoke
        // recipe).
        if let Some(ch) = cell.ch {
            self.glyphs.push(GlyphCell {
                pos,
                ch,
                face: face(cell.bold, cell.italic),
                // The grid is **always** in the display font: the only place
                // for the small class is the dock's context row
                // ([`Frame::push_dock`]).
                size: SizeClass::Normal,
                rgba: cell.fg.to_array(),
                wide: cell.wide,
                cluster: cell.cluster,
            });
        }
        if let Some(kind) = rule_kind(cell.underline) {
            self.rules.push(RuleCell {
                pos,
                kind,
                // SGR 58 if present, otherwise the foreground (`bt-core`).
                rgba: cell.underline_color.unwrap_or(cell.fg).to_array(),
            });
        }
        if cell.strikeout {
            self.rules.push(RuleCell {
                pos,
                kind: RuleKind::Strike,
                // Strikeout does **not** use SGR 58: in SGR strikeout has no
                // colour of its own and `underline_color` is, by name, the
                // underline's.
                rgba: cell.fg.to_array(),
            });
        }
    }

    /// The selection's row runs: **one** quad per run, as the pieces of a
    /// single rounded-corner shape.
    ///
    /// One instance per run, not per cell: the bridged gaps have no cell of
    /// their own (they never reach the sink) and the seam between neighbouring
    /// quads could leave a translucent line at a fractional cell width.
    ///
    /// **The whole slice at once**, not run by run: a corner's decision
    /// depends on the neighbouring row's run ([`selection_corners`]). The
    /// instance's `rgba` slot here is not a colour but a **corner mask** —
    /// per corner `1` convex, `0` square; the concave fill is a separate `r×r`
    /// instance and its mask has `-1` at the corner that is the circle's
    /// centre (`selection_fragment`). The colour and the radius are a single
    /// uniform per frame ([`Frame::selection_rgba`],
    /// [`Frame::selection_radius`]), the same path as the caret's alignment
    /// escape.
    ///
    /// The colour is the caller's choice by focus
    /// (`bt_core::SelectionRuns::color`): the focus does not enter `bt-core`
    /// and is not asked here either.
    pub(crate) fn push_selection(&mut self, runs: &[SelectionRun], rgba: LinearRgba) {
        debug_assert!(self.cell_px.0 > 0.0, "clear(metrics) was not called");
        self.selection_rgba = rgba.to_array();
        let mut out = std::mem::take(&mut self.selection);
        self.selection_parts(runs, |frame, col, row| frame.pos(col, row), &mut out);
        self.selection = out;
    }

    /// The dock's selection runs: one run per visual row of the input block
    /// (`bt_core::Session::dock`'s `runs`; a long line wraps). By the
    /// **same** path as the grid's shape ([`Frame::selection_parts`]) — the
    /// corner decision looks at the neighbouring row's run, so a selection
    /// across rows is a single-piece shape; the position is dock-local
    /// ([`Frame::dock_pos`]). The colour and radius are the same uniform as the
    /// grid's — there is a single selection in a window and its colour is
    /// written by [`Frame::push_selection`] in every content frame.
    pub(crate) fn push_dock_selection(&mut self, runs: &[SelectionRun]) {
        debug_assert!(self.cell_px.0 > 0.0, "clear(metrics) was not called");
        let mut out = std::mem::take(&mut self.dock_selection);
        self.selection_parts(runs, |frame, col, row| frame.dock_pos(col, row), &mut out);
        self.dock_selection = out;
    }

    /// The quads and concave fills of the selection runs, in `pos`'s
    /// coordinate space — the **single** shape decision of the two surfaces
    /// (grid, dock); only the cell's position differs.
    fn selection_parts(
        &self,
        runs: &[SelectionRun],
        pos: impl Fn(&Self, u16, u16) -> [f32; 2],
        out: &mut Vec<Instance>,
    ) {
        let r = self.selection_radius();
        let (cw, ch) = self.cell_px;
        for (index, run) in runs.iter().enumerate() {
            debug_assert!(run.first <= run.last, "reversed run: {run:?}");
            let corners = selection_corners(runs, index);
            let pos = pos(self, run.first, run.row);
            let width = (f32::from(run.last.saturating_sub(run.first)) + 1.0) * cw;
            out.push(Instance {
                pos,
                size: [width, ch],
                rgba: corners.map(|c| if c == Corner::Convex { 1.0 } else { 0.0 }),
            });
            if r <= 0.0 {
                continue;
            }
            let (x0, x1) = (pos[0], pos[0] + width);
            let (y0, y1) = (pos[1], pos[1] + ch);
            // The fill is **outside** the step, in this run's own row band:
            // the `r×r` square beside the corner, the circle's centre being the
            // square's end farthest from the corner. The order is [`Corner`]'s
            // (TL, TR, BR, BL); the second number is the fill quad's corner
            // that carries the centre.
            let fills = [
                ([x0 - r, y0], 3),
                ([x1, y0], 2),
                ([x1, y1 - r], 1),
                ([x0 - r, y1 - r], 0),
            ];
            for (corner, (fill_pos, centre)) in corners.into_iter().zip(fills) {
                if corner != Corner::Concave {
                    continue;
                }
                let mut mask = [0.0; 4];
                mask[centre] = -1.0;
                out.push(Instance {
                    pos: fill_pos,
                    size: [r, r],
                    rgba: mask,
                });
            }
        }
    }

    /// The search highlight's runs on the grid: the selection's shape
    /// ([`Frame::selection_parts`], `SELECTION_RADIUS`) but the corners are
    /// **per match** — [`selection_corners`] assumes one run per row and
    /// array adjacency, whereas search puts several runs on a row and two
    /// separate matches on consecutive rows must not fuse into a single shape
    /// A single wrapped match does fuse: `SearchRun::continues`
    /// says so.
    ///
    /// The colours are the caller's choice by focus (`bt_core::SearchRuns`),
    /// the selection's rule.
    pub(crate) fn push_search(
        &mut self,
        runs: &[SearchRun],
        matched: LinearRgba,
        current: LinearRgba,
    ) {
        debug_assert!(self.cell_px.0 > 0.0, "clear(metrics) was not called");
        self.search_match_rgba = matched.to_array();
        self.search_current_rgba = current.to_array();
        let mut lists = (
            std::mem::take(&mut self.search_match),
            std::mem::take(&mut self.search_current),
        );
        self.search_parts(runs, &mut lists);
        (self.search_match, self.search_current) = lists;
    }

    /// The fill band's search runs; the rows are fill-local and drawn in the
    /// band's own viewport (the precedent of [`Frame::push_fill_block`] — the
    /// position is still [`Frame::pos`], the space is the viewport's). The
    /// colours are the uniform [`Frame::push_search`] wrote: that must be
    /// called first.
    pub(crate) fn push_fill_search(&mut self, runs: &[SearchRun]) {
        debug_assert!(
            runs.iter().all(|run| run.row < self.fill_rows),
            "fill search is outside the band: {runs:?} / {}",
            self.fill_rows
        );
        let mut lists = (
            std::mem::take(&mut self.fill_search_match),
            std::mem::take(&mut self.fill_search_current),
        );
        self.search_parts(runs, &mut lists);
        (self.fill_search_match, self.fill_search_current) = lists;
    }

    /// Splits the runs into matches and passes each match through
    /// [`Frame::selection_parts`] on its own slice; the current match goes to
    /// the second list. A match is runs adjacent by the `continues` bit:
    /// `bt-core` gives a match's runs back to back but the next match can start
    /// on a higher row (after a match wrapping onto 5–6, a second one on 5), so
    /// the list is **not sorted**, only split. Even if the list's first run is
    /// `continues` it counts as a new match — its head is on the other surface
    /// (the band) or off screen.
    fn search_parts(&mut self, runs: &[SearchRun], lists: &mut (Vec<Instance>, Vec<Instance>)) {
        let mut scratch = std::mem::take(&mut self.search_scratch);
        let mut start = 0;
        while start < runs.len() {
            let head = runs[start];
            // The continuation run is on the row **immediately below** the
            // previous run; a continuation that does not hold the row (only on
            // malformed input) splits the shape, it does not fuse it.
            let end = runs
                .windows(2)
                .skip(start)
                .position(|pair| {
                    !pair[1].continues || Some(pair[1].row) != pair[0].row.checked_add(1)
                })
                .map_or(runs.len(), |i| start + 1 + i);
            scratch.clear();
            scratch.extend(runs[start..end].iter().map(|run| SelectionRun {
                row: run.row,
                first: run.first,
                last: run.last,
            }));
            let out = if head.current {
                &mut lists.1
            } else {
                &mut lists.0
            };
            self.selection_parts(&scratch, |frame, col, row| frame.pos(col, row), out);
            start = end;
        }
        self.search_scratch = scratch;
    }

    /// This frame's match-highlight pieces ([`Frame::push_search`]).
    pub(crate) fn search_match_instances(&self) -> &[Instance] {
        &self.search_match
    }

    /// This frame's current-match pieces ([`Frame::push_search`]).
    pub(crate) fn search_current_instances(&self) -> &[Instance] {
        &self.search_current
    }

    /// The band's match-highlight pieces, fill-local.
    pub(crate) fn fill_search_match_instances(&self) -> &[Instance] {
        &self.fill_search_match
    }

    /// The band's current-match pieces, fill-local.
    pub(crate) fn fill_search_current_instances(&self) -> &[Instance] {
        &self.fill_search_current
    }

    /// The `search_match` colour, linear — `selection_fragment`'s colour uniform.
    pub(crate) fn search_match_rgba(&self) -> [f32; 4] {
        self.search_match_rgba
    }

    /// The `search_current` colour, linear.
    pub(crate) fn search_current_rgba(&self) -> [f32; 4] {
        self.search_current_rgba
    }

    /// This frame's selection pieces; [`Frame::push_selection`]'s quads.
    pub(crate) fn selection_instances(&self) -> &[Instance] {
        &self.selection
    }

    /// The dock's selection pieces, dock-local ([`Frame::push_dock_selection`]).
    pub(crate) fn dock_selection_instances(&self) -> &[Instance] {
        &self.dock_selection
    }

    /// The selection's colour, linear — `selection_fragment`'s colour uniform.
    pub(crate) fn selection_rgba(&self) -> [f32; 4] {
        self.selection_rgba
    }

    /// The selection's corner radius, in pixels: the selection's own ratio
    /// ([`SELECTION_RADIUS`]), not the user's `cursor_radius` (the
    /// key is the caret's). Its clamp is from [`caret_radius_px`], i.e. half
    /// the width on a one-cell run.
    pub(crate) fn selection_radius(&self) -> f32 {
        caret_radius_px(self.cell_px, SELECTION_RADIUS)
    }

    /// A command block's mark: the chevron drawn at **column 0**, on the
    /// command's own row.
    ///
    /// **The colour is carried, not produced.** `bt-core` hands over the
    /// "which rows, which colour" question already resolved ([`Block`]); a
    /// branch here that recognises exit codes would be in the wrong place
    /// (the decision is here, the painting there).
    ///
    /// It **goes through** [`Frame::pos`] and must: the dock's prompt mark
    /// goes through the same row (`dock::render`, column 0), so the alignment
    /// of the two marks is not something computed but the result of a single
    /// formula. As long as it was placed with a separate arithmetic — in the
    /// middle of the margin — it stood half a margin to the left of the
    /// dock's.
    ///
    /// The column the mark sits in is **empty**, because the prompt really is
    /// two columns wide (`assets/shell/zsh/bateri.zsh` → `__bateri_ps1`, the
    /// same number as `dock::TEXT_COL`). The mark does not cover the command's
    /// letter and the drawing does not lie to the terminal — the alternative
    /// was shifting the command line while drawing it, which would have broken
    /// the mouse mapping and the line wrapping.
    ///
    /// The sprite is exactly one cell tall (the `cell` pipeline's fixed slot)
    /// but its ink is gathered in the middle of the cell
    /// (`bt_atlas::raster::chevron`).
    pub(crate) fn push_block(&mut self, block: Block) {
        let h = self.cell_px.1;
        debug_assert!(h > 0.0, "clear(metrics) was not called");
        // **The mark is no longer a rectangle, it is the dock's chevron
        // itself** (the user: "in the grid part the result colour
        // boxes will be this new > too, their colours staying the same"). The
        // two already said the same thing — a prompt mark in the phase colour —
        // and drawing them with separate shapes was not a design decision but
        // a leftover.
        //
        // The sprite is exactly one cell tall (the `cell` pipeline's fixed
        // slot) but its ink is gathered in the middle of the cell
        // (`bt_atlas::raster::chevron`), so it stays within the left margin.
        //
        // **AT COLUMN 0, not inside the margin.** The dock's prompt mark is
        // there too (`dock::render`, column 0), so the alignment of the two
        // marks is not computed — both go through `Frame::pos` and come from
        // the same formula. As long as it was put in the middle of the margin
        // it stood half a margin to the left of the dock's mark and the user
        // saw this.
        //
        // The column does not collide with the command, because the prompt is
        // now **really** two columns wide (`assets/shell/zsh/bateri.zsh` →
        // `__bateri_ps1`): the command text starts at column 2 and the mark
        // falls on top of the gap at 0. The drawing does not lie to the
        // terminal, so the mouse mapping and the line wrapping are left
        // untouched.
        //
        // The height is exactly one cell: the mark shows the command's row,
        // not a range (`bt_core::Block`). The row range does not cross the
        // boundary, so there is no "reversed range" left to validate here.
        self.stripes.push(RuleCell {
            pos: self.pos(0, block.row),
            kind: RuleKind::Chevron,
            rgba: block.stripe.to_array(),
        });
    }

    /// The fill band's block mark — [`Frame::push_block`]'s band twin.
    ///
    /// A separate call, because it is a separate **coordinate space**: the row
    /// is fill-local (`0..fill`) and the list is drawn in the band's own
    /// `setViewport` (`fill_rules`, the third surface). Had it been written to
    /// the grid's `stripes`, the mark would have appeared beside the grid row
    /// with the same number, not beside the row the band shows.
    ///
    /// The shape, column and colour source are the **same** as the grid's: the
    /// same chevron sprite, column 0, the phase colour `bt-core` gives. There
    /// is no second design decision — all the band gained is access to the
    /// list.
    pub(crate) fn push_fill_block(&mut self, block: Block) {
        debug_assert!(
            block.row < self.fill_rows,
            "fill mark is outside the band: {} / {}",
            block.row,
            self.fill_rows
        );
        self.fill_rules.push(RuleCell {
            pos: self.pos(0, block.row),
            kind: RuleKind::Chevron,
            rgba: block.stripe.to_array(),
        });
    }

    /// The caret block; it does **not** enter `bg_count` and an invisible
    /// caret is not drawn.
    ///
    /// It writes two things at once, deliberately in a single call: the block
    /// itself into the background list as a rectangle (`rgba` — the theme's
    /// accent), and the colour of **the text under the block** into the `cell`
    /// pipeline's uniform (`cursor.text` — `bt-core`'s decision). Had they been
    /// separate calls one could be called and the other forgotten, and the
    /// symptom would be silent: the caret in the right place, the letter under
    /// it unreadable.
    ///
    /// **The position comes from `at`, not from `cursor`**, and in cell units
    /// as `f32`: a gliding caret is not an integer while between two cells.
    /// `cursor` is still needed, because the others (`visible`, `text`) are
    /// `bt-core`'s decision and do not depend on position — the intermediate
    /// position is the drawer's, the target the boundary's.
    ///
    /// **`at` is a screen row, not a grid row** (`crate::motion`): the
    /// caret is exempt from the offset, because on Enter the grid row goes up
    /// by one while the offset goes down by one and the caret's place on
    /// screen never changes. While the content flows up behind it, the caret
    /// stays on its bottom row.
    ///
    /// **`alpha` is written to both** (`crate::motion::Motion::alpha`): while
    /// Reduce Motion is on the caret fades in at its new cell and the block
    /// and the colour of the text under it must fade in **together**. Had they
    /// been separate the letter would be painted in the colour of a block not
    /// yet visible — a ground-coloured letter on the ground, i.e. an unreadable
    /// cell. Outside the fade it is `1.0`, so this path carries the same two
    /// values every frame.
    pub(crate) fn push_caret(
        &mut self,
        at: [f32; 2],
        text: LinearRgba,
        rgba: LinearRgba,
        alpha: f32,
        shape: CaretShape,
        focused: bool,
    ) {
        // **Hollowing out is only for the block.** The underline and the
        // vertical bar are already thin strips; their "hollow" state is a
        // one-pixel frame, i.e. nothing — moreover the edge thickness is
        // `rule_px` and so is the strip's own, so the subtraction would swallow
        // the body entirely. For those shapes the unfocused signal is the
        // blink stopping.
        //
        // **The third term is the user's** (`[terminal] cursor_unfocused`):
        // `"solid"` turns hollowing off and does **not touch** the blink —
        // the blink stopping when unfocused is a separate decision, the two
        // being separate signals.
        let hollow = !focused
            && matches!(shape, CaretShape::Block)
            && self.caret_style.unfocused == UnfocusedCaret::Hollow;
        self.caret_hollow = hollow;
        let mut pos = self.pos_at(at);
        // **The slot is chosen before the shift** and the fraction is added
        // to the caret in the grid's slot ([`Frame::frac_px`]): the grid is
        // that much lower, and the caret must stay on top of its letter. The
        // dock slot is exempt from the shift — the dock is a separate surface
        // and a caret standing there is not part of the scrolled screen. The
        // choice looks at the unshifted position, because the fraction is not
        // a handover: had it pushed the caret on the grid's last row into the
        // dock's slot, it would be drawn not **under** the dock's ground but on
        // top of it, detached from its letter — whereas the dock also covers
        // the grid's overflowing letter.
        //
        // **Known limit:** in a grid-to-dock handover slide the fraction drops
        // in the frame that crosses the threshold, so the caret jumps by the
        // fraction in that frame. The fraction is above zero only during a
        // gesture in progress and the handover is at the end of a command, so
        // the two coinciding in the same frame is narrow; closing it would mean
        // moving the fraction into the caret's animator.
        //
        // **An overlap below half a pixel is not a handover**: in a
        // remote session the band's excess is fractional
        // (`(band_px − dock_px) / cell_h`) and not exactly representable in
        // `f32`; the bottom edge of the caret on the last row would exceed the
        // band's top by an epsilon, pass into the dock slot, paint its letter
        // and lose the fraction. A real overlap is at least one pixel.
        let in_dock = pos[1] + self.cell_px.1 > self.dock_top_px + 0.5;
        if !in_dock {
            pos[1] += self.frac_px;
        }
        // **The shape lives in `Frame`**, not carried in a signature and
        // forgotten: a motion frame ([`Frame::move_caret`]) never goes to
        // `bt-core` and does not know the shape. Without the field a beam
        // would turn back into a block on the first motion frame and the
        // symptom would be "the caret sometimes changes shape".
        self.caret_shape = shape;
        // **The rectangle is in window space and goes to both glyph
        // encodes.** It is compared with the fragment's `[[position]]`, and
        // that coordinate is **after** the viewport transform, i.e. the same
        // space for both the grid's and the dock's glyphs. Since `at` is a
        // screen row no translation is needed — formerly the dock had its own
        // rectangle and the encode moved it with `shifted_y`; the single caret
        // removed that translation altogether.
        let rects = caret_rect(pos, self.cell_px, shape, self.rule_px, hollow);
        let (opaque_pos, opaque_size) = rects.opaque;
        let mut bottom = opaque_pos[1] + opaque_size[1];
        // **The grid's caret does not invert in the dock band.** The caret on
        // the last row, shifted by the fraction, can enter the band and there,
        // together with its letter, stay under the dock's opaque ground; the
        // rectangle, however, goes to both glyph encodes and, had it not been
        // clipped, the dock's letters in that column would be drawn in the
        // ground colour, i.e. invisible. In a frame with no dock the limit is
        // infinite and the clipping is the identity.
        if !in_dock {
            bottom = bottom.min(self.dock_top_px);
        }
        self.cursor = CursorBlock {
            rect: [
                opaque_pos[0],
                opaque_pos[1],
                opaque_pos[0] + opaque_size[0],
                bottom.max(opaque_pos[1]),
            ],
            rgba: with_alpha(text, alpha),
        };
        // **The painted rectangle is kept separate**, not derived from
        // `CursorBlock`'s: today they are equal, but on a hollow caret
        // the inversion area empties while the painted area
        // stays. This is the fragment SDF's core; in screen space, i.e. the
        // same space as `[[position]]`.
        let (painted_pos, painted_size) = rects.painted;
        self.caret_core = [
            painted_pos[0],
            painted_pos[1],
            painted_pos[0] + painted_size[0],
            painted_pos[1] + painted_size[1],
        ];
        // **Slot selection is a necessity of the painter's algorithm.** The
        // block must be drawn **after** the ground of the surface it will stand
        // on but **before** its glyphs: had it stayed in the grid's slot the
        // dock's opaque ground would cover it, and had it stayed in the dock's
        // slot it would paint the grid's letter. The criterion is overlap: if
        // the caret has entered the dock band by even one pixel it moves to the
        // dock's slot and stays **on top** there — so that no half-clipped
        // block is seen in the handover frames.
        //
        // One caret, one rectangle, one instance; what changes is only which
        // encode it enters.
        let caret = Caret {
            at: pos,
            rgba: with_alpha(rgba, alpha),
        };
        if in_dock {
            self.dock_caret = Some(caret);
        } else {
            self.grid_caret = Some(caret);
        }
    }

    /// **The motion frame's single end**: moves the caret while keeping the
    /// lists.
    ///
    /// The grid is not dirty, so the glyph and rule lists are still valid —
    /// rebuilding them would mean taking the `Term` lock 120 times a second
    /// and fighting "the render path does not block" right there. The background list is trimmed to `bg_count`: the only thing
    /// trimmed is the previous frame's caret, because [`Frame::push`] must add
    /// backgrounds **before** the caret and a `debug_assert` holds that. The
    /// trimming keeps that guard valid — the list stays in "backgrounds first,
    /// then caret" order in any case.
    ///
    /// **It does not touch the stripe list**, which is part of the same
    /// sentence: if the grid did not change, the blocks' row ranges did not
    /// change either, so the stripe must not change. Had the stripe not been
    /// in a separate list, this trimming would have erased it.
    pub(crate) fn move_caret(
        &mut self,
        at: [f32; 2],
        text: LinearRgba,
        rgba: LinearRgba,
        alpha: f32,
        focused: bool,
    ) {
        // **The shape is kept, the focus is not** and the distinction is at
        // the source: the shape comes from `bt-core` and this path has no
        // access to it, whereas the focus is `bt-gpu`'s own bit
        // (`DisplayLink::set_focused`) and can be read every frame. Keeping a
        // stored copy would make it **stale**: if an animation is in flight
        // while the focus turns, the caret would keep being drawn hollow and
        // fill in only at the next **content** frame — the user saw this as
        // "the frame stays, the inside fills in later" (2026-09-20).
        let shape = self.caret_shape;
        self.clear_caret();
        self.push_caret(at, text, rgba, alpha, shape, focused);
    }

    /// Empties all three of the caret's slots: the two instances and the
    /// rectangle.
    ///
    /// A separate function, because it has two callers and both **must**
    /// empty **everything**: [`Frame::clear`] (new frame) and
    /// [`Frame::move_caret`] (motion frame). Had one been forgotten, in a
    /// handover frame the caret would be drawn in two places at once — the
    /// slot changes but the old one is not cleared.
    fn clear_caret(&mut self) {
        self.grid_caret = None;
        self.dock_caret = None;
        self.cursor = CursorBlock::default();
        self.caret_core = [0.0; 4];
    }

    /// Overrides the SDF uniform — test only; its limit is in the field's doc.
    #[cfg(test)]
    pub(crate) fn force_caret_sdf(&mut self, shape: [f32; 4]) {
        self.caret_sdf_override = Some(shape);
    }

    /// One cell of the dock; [`Frame::push`]'s twin but in **dock-local**
    /// coordinates and without entering the counters.
    ///
    /// The row comes from `bt-core` dock-local (0 = the input row) and stays
    /// so here: what carries the dock onto the screen is the second
    /// `setViewport` ([`crate::Renderer`]), not arithmetic. That it never sees
    /// the offset (`origin_px`) follows from this — the exemption is
    /// **structural**, not by subtraction.
    ///
    /// It shares the left margin with the grid ([`Frame::pos_at`]): the dock's
    /// columns are aligned with the grid's and the margin reserved for the
    /// stripe stays empty in the dock too.
    pub(crate) fn push_dock(&mut self, cell: Cell) {
        let pos = self.dock_pos(cell.col, cell.row);
        if let Some(glyph) = self.dock_glyph(cell) {
            self.dock_glyphs.push(glyph);
        }
        if let Some(bg) = cell.bg {
            // The caret is **no longer in this list**: it has its own slot and
            // the encode draws it after the dock's backgrounds
            // ([`Frame::push_caret`]), so the "no background after the caret"
            // guard is no longer needed either — the order is in the encode,
            // not in the list.
            self.dock_bg.push(Instance {
                pos,
                size: [self.cell_px.0, self.cell_px.1],
                rgba: bg.to_array(),
            });
        }
        if let Some(kind) = rule_kind(cell.underline) {
            self.dock_rules.push(RuleCell {
                pos,
                kind,
                rgba: cell.underline_color.unwrap_or(cell.fg).to_array(),
            });
        }
        if cell.strikeout {
            self.dock_rules.push(RuleCell {
                pos,
                kind: RuleKind::Strike,
                rgba: cell.fg.to_array(),
            });
        }
    }

    /// A dock cell's glyph — the **common** translation of [`Frame::push_dock`]
    /// and the typing effects ([`Frame::set_dock_fx`]).
    ///
    /// A single place, because the effect's drawing at `t = 1` must be pixel
    /// for pixel the same as the static glyph: had the
    /// position, face, size class or colour been computed in two places, the
    /// letter would jump for a moment in the handover frame.
    fn dock_glyph(&self, cell: Cell) -> Option<GlyphCell> {
        cell.ch.map(|ch| GlyphCell {
            pos: self.dock_pos(cell.col, cell.row),
            ch,
            face: face(cell.bold, cell.italic),
            // The **same threshold** as the position ([`Frame::column_px`]):
            // had they diverged the letter would be in one measure and its step
            // in another.
            size: if self.is_context_row(cell.row) {
                SizeClass::Small
            } else {
                SizeClass::Normal
            },
            // **The boundary can now also give `true` here**: the
            // dock's column accumulates from width, not from the character
            // index, so the head cell of a wide character arrives marked and a
            // glyphless ground cell falls on the spacer column. Earlier the
            // comment on this line said "the boundary always gives `false`" and
            // that was exactly the rationale for reading the field from the
            // cell — "if `bt-core` ever removed it this line would silently
            // stay old". It did; the line did not silently stay old, because
            // the constant was not written.
            wide: cell.wide,
            rgba: cell.fg.to_array(),
            // The context row (small class) carries no clusters: the
            // boundary pushes no clustered cell there.
            cluster: cell.cluster,
        })
    }

    /// Finishes the in-flight arrivals **whose static glyph cannot be found**
    /// — in the content frame, after the dock has been pushed.
    ///
    /// The match is position (row and column; the pixel passing through the
    /// same [`Frame::dock_pos`], i.e. exact equality) and character. An
    /// arrival that cannot be found either fell outside the window or that
    /// character is no longer there — on a wrapped input the letter that
    /// slides behind the edit also ends here; had it been drawn,
    /// a letter not on the line would appear. Ghosts are not asked: they have
    /// no static glyph to begin with.
    pub(crate) fn suppress_dock(&self, fx: &mut GlyphFx) {
        fx.retain(|fx| fx.kind == Kind::Ghost || self.static_arrival(fx).is_some());
    }

    /// The arrival's static glyph, in `dock_glyphs`.
    fn static_arrival(&self, fx: &Fx) -> Option<usize> {
        let pos = self.dock_pos(fx.cell.col, fx.cell.row);
        self.dock_glyphs
            .iter()
            .position(|glyph| glyph.pos == pos && Some(glyph.ch) == fx.cell.ch)
    }

    /// Writes the typing effects for this frame; both the content frame and
    /// the motion frame pass through here (the dock's static lists are kept
    /// in the motion frame, only the effects advance).
    ///
    /// The static glyph of an in-flight arrival is **removed** from the list to
    /// be drawn (`dock_shown`), otherwise `fade` would fade in on top of the
    /// static glyph and nothing would be visible. `dock_glyphs` is not
    /// touched: when the effect finishes the static glyph must come back
    /// without the dock being pushed again.
    ///
    /// `heat` is the theme's `cursor` colour (the `heat` effect's hot colour);
    /// both writers hold the theme.
    ///
    /// `table` is the effects' cluster table ([`GlyphFx::clusters`]); the
    /// ghosts' clusters are copied into [`Frame::fx_clusters`], while the
    /// arrivals, being copies of the static glyph, are in the dock's table.
    pub(crate) fn set_dock_fx(
        &mut self,
        fx: impl IntoIterator<Item = Fx>,
        table: &Clusters,
        heat: LinearRgba,
    ) {
        self.dock_fx_heat = heat.to_array();
        self.dock_ghosts.clear();
        self.fx_clusters.clear();
        self.dock_arrivals.clear();
        self.dock_shown.clear();
        let mut hidden = [usize::MAX; crate::glyph_fx::FX_MAX];
        let mut hidden_len = 0;
        for fx in fx {
            let fx_cell = |glyph| FxCell {
                glyph,
                t: fx.t,
                effect: fx.effect,
                seed: fx.seed,
            };
            match fx.kind {
                Kind::Ghost => {
                    if let Some(glyph) = self.dock_glyph(fx.cell) {
                        let cluster = copy_cluster(glyph.cluster, table, &mut self.fx_clusters);
                        self.dock_ghosts
                            .push(fx_cell(GlyphCell { cluster, ..glyph }));
                    }
                }
                Kind::Arrival => {
                    // If it has no static glyph the arrival is not drawn (see
                    // [`Frame::suppress_dock`]; the content frame already
                    // finished it, this is the motion frame's defence).
                    let Some(index) = self.static_arrival(&fx) else {
                        continue;
                    };
                    if let Some(slot) = hidden.get_mut(hidden_len) {
                        *slot = index;
                        hidden_len += 1;
                    }
                    // **The glyph is the hidden static glyph itself**, not the
                    // cell from the moment it was written: the highlight can
                    // change afterwards (`zsh-syntax-highlighting` paints `l`
                    // red and `ls` green) and the effect would end in the old
                    // colour and jump to the new — the `t = 1` equality would
                    // break.
                    self.dock_arrivals.push(fx_cell(self.dock_glyphs[index]));
                }
            }
        }
        if hidden_len > 0 {
            let hidden = &hidden[..hidden_len];
            self.dock_shown.extend(
                self.dock_glyphs
                    .iter()
                    .enumerate()
                    .filter(|(index, _)| !hidden.contains(index))
                    .map(|(_, glyph)| *glyph),
            );
        }
    }

    /// The dock's prompt mark: in the first row's first column, in the phase
    /// colour.
    ///
    /// A separate call, because the mark is **not a cell** — `bt-core` gives
    /// only its colour across the boundary (`bt_core::Dock::sigil`) and the
    /// shape is this layer's decision. Had it gone through as a character, the
    /// user's font's `>` would be drawn; but the mark is the terminal itself
    /// and is the **same** sprite as the grid's block mark
    /// ([`Frame::push_block`]).
    pub(crate) fn push_dock_sigil(&mut self, rgba: LinearRgba) {
        self.dock_rules.push(RuleCell {
            pos: self.dock_pos(0, 0),
            kind: RuleKind::Chevron,
            rgba: rgba.to_array(),
        });
    }

    /// The dock's **layout** rows in this frame, context row included
    /// ([`Frame::dock_rows`]); called **before** the cells, because the cell's
    /// position depends on it.
    ///
    /// The presence of the context row comes from the row count: two or more →
    /// the last row is context, a single row → an input row without context.
    /// The production path is [`Frame::set_dock_input_rows`]; this state remains
    /// for the tests' one-row dock — compiled only in tests.
    #[cfg(test)]
    pub(crate) fn set_dock_rows(&mut self, rows: u16) {
        self.dock_rows = rows;
        self.dock_context = rows >= 2;
    }

    /// The frame path's layout: `input_rows` input rows
    /// (`bt_core::Cursor::band_rows`) and **always** a context row below.
    ///
    /// Zero input rows is legitimate (remote session): the
    /// layout is the context row alone, in the small face and with no row gap
    /// above it — there is no input row to separate.
    ///
    /// `None` is **no band**: zero layout rows and no context row, so the
    /// layout's height is zero and the mouse's input block has zero rows
    /// ([`Frame::dock_hit`]) — the surface stays open, a click falls to the
    /// grid.
    pub(crate) fn set_dock_input_rows(&mut self, input_rows: Option<u16>) {
        self.dock_rows = input_rows.map_or(0, |rows| rows.saturating_add(1));
        self.dock_context = input_rows.is_some();
    }

    /// The band's **drawn** height in this frame: the window's bottom and the
    /// excess over the PTY share, in rows (`crate::motion::Motion::band`,
    /// fractional).
    ///
    /// Both frame paths call it ([`Frame::dock_band`]); the band's top, which
    /// selects the caret's slot ([`Frame::dock_top_px`]), is written in the
    /// same call — had they been written separately the caret could look at
    /// one top of the surface and the band at another.
    ///
    /// The excess's pixels are **rounded to the device grid** (the rationale
    /// of `set_origin_rows`): the grid's origin subtracts the same rounded
    /// number too.
    /// The window's current PTY share (rows) the band's excess is added to —
    /// the same number `band_target` subtracted, so the band's drawn height is
    /// `band_px(input_rows)` whatever the share.
    pub(crate) fn set_dock_share(&mut self, rows: u16) {
        self.dock_share = Some(rows);
    }

    pub(crate) fn set_dock_band(&mut self, bottom_px: f32, extra_rows: f32) {
        debug_assert!(self.cell_px.1 > 0.0, "clear(metrics) was not called");
        self.dock_band = Some((extra_rows * self.cell_px.1).round());
        self.dock_bottom_px = bottom_px;
        self.dock_top_px = bottom_px - self.band_height();
    }

    /// Opens the dock surface for this frame: its ground and the colours of the
    /// two hairlines.
    ///
    /// It is called **after** the cells and this is not an order preference but
    /// a necessity: the colours come back from `bt-core`'s dock call and that
    /// call produces them while pushing the cells into the sink. `Frame`
    /// therefore keeps the surface independent of the cells — while the lists
    /// are full `dock` can still be `None` and in that state nothing is drawn,
    /// so a "half-opened dock" cannot be represented.
    pub(crate) fn open_dock(
        &mut self,
        ground: LinearRgba,
        edge: LinearRgba,
        separator: LinearRgba,
    ) {
        self.dock = Some(DockSurface {
            ground: ground.to_array(),
            edge: edge.to_array(),
            separator: separator.to_array(),
            progress: None,
            track: separator.to_array(),
            buttons: [None; 2],
        });
    }

    /// The upload row's buttons (`bt_core::Dock::buttons`): after
    /// [`Frame::open_dock`]; a no-op if the dock is not open.
    pub(crate) fn set_dock_buttons(&mut self, buttons: [Option<DockButton>; 2]) {
        if let Some(dock) = &mut self.dock {
            dock.buttons = buttons;
        }
    }

    /// The upload buttons' draws: per button **fill, then border** — both
    /// from `caret_fragment` (the rounded rectangle's SDF and edge band are
    /// already there; no new pipeline or shader). The quad is dock-local and
    /// the core is in window space (lower by `origin_y`): the fragment
    /// compares it with `[[position]]` ([`Frame::dock_caret`]'s same
    /// translation in the opposite direction).
    ///
    /// The rectangle is the context row's cell band: horizontally the
    /// button's column span ([`Frame::dock_pos`], the small class's step),
    /// vertically the row's height. **Exactly the click area** — the mouse
    /// reads the same column span from `bt_core::transfer_button_at`. The
    /// edges are rounded to the device pixel: the border is hairline
    /// thickness and an edge that does not land would spread over two pixels
    /// and fade.
    ///
    /// The radius is the selection's ([`Frame::selection_radius`]), the
    /// thickness the font's own rule metric — no second number was made up.
    pub(crate) fn dock_button_draws(
        &self,
        origin_y: f32,
    ) -> impl Iterator<Item = RoundedDraw> + '_ {
        let buttons = self.dock.map_or([None; 2], |dock| dock.buttons);
        let row = self.dock_rows.saturating_sub(1);
        let radius = self.selection_radius();
        let stroke = self.rule_px.max(1.0);
        buttons.into_iter().flatten().flat_map(move |button| {
            let [x0, y0] = self.dock_pos(button.start, row);
            let [x1, _] = self.dock_pos(button.end, row);
            let (x0, x1) = (x0.round(), x1.round());
            let (y0, y1) = (y0.round(), (y0 + self.cell_px.1).round());
            let (fill, edge) = button_alpha(button.state);
            let instance = |alpha: f32| Instance {
                pos: [x0, y0],
                size: [x1 - x0, y1 - y0],
                rgba: with_alpha(button.color, alpha),
            };
            let core = [x0, y0 + origin_y, x1, y1 + origin_y];
            [
                RoundedDraw {
                    instance: instance(fill),
                    core,
                    shape: [radius, 0.0, 0.0, 0.0],
                },
                RoundedDraw {
                    instance: instance(edge),
                    core,
                    shape: [radius, stroke, 0.0, 0.0],
                },
            ]
        })
    }

    /// Turns the top hairline into a **progress bar** for this frame
    /// (`bt_core::Dock::progress`, in ten-thousandths): the filled
    /// part in `edge`'s colour, the rest in `track`'s (`bt_core::Dock::track`).
    /// After [`Frame::open_dock`]; opening resets it to `None`
    /// every frame, so the bar exists only in the frame it is told. A no-op if
    /// the dock is not open.
    pub(crate) fn set_dock_progress(&mut self, progress: Option<u16>, track: LinearRgba) {
        if let Some(dock) = &mut self.dock {
            dock.progress = progress.map(|p| f32::from(p.min(10_000)) / 10_000.0);
            dock.track = track.to_array();
        }
    }

    /// This frame's dock surface; `None` → no dock, the second viewport is not set up.
    pub(crate) fn dock(&self) -> Option<DockSurface> {
        self.dock
    }

    /// The height of the dock's **layout**, in pixels: the number that gives
    /// the origin of the cells' viewport and the caret's shift. Zero if there
    /// is no dock.
    ///
    /// The band's drawn height ([`Frame::dock_band_px`]) diverges from this
    /// during the animation; since the cells are bottom-anchored (the context
    /// row is at the bottom) the divergence moves only the place of the ground
    /// and the top hairline.
    pub(crate) fn dock_layout_px(&self) -> f32 {
        if self.dock.is_none() {
            return 0.0;
        }
        dock_height(self.dock_rows, self.cell_px.1, self.gutter_px)
    }

    /// The band's **drawn** height in this frame, in pixels: the number that
    /// gives the viewport of the ground and the top hairline. Zero if there is
    /// no dock; if the band was not told, the layout's height.
    pub(crate) fn dock_band_px(&self) -> f32 {
        if self.dock.is_none() {
            return 0.0;
        }
        self.band_height()
    }

    /// The dock-independent body of [`Frame::dock_band_px`]: if the band was
    /// told, the PTY share plus the excess, otherwise the layout's height.
    /// The excess is the clamped one ([`Frame::band_excess`]), so the height
    /// never goes below zero.
    fn band_height(&self) -> f32 {
        match self.band_excess() {
            Some(extra) => {
                dock_height(
                    self.dock_share.unwrap_or(DOCK_ROWS),
                    self.cell_px.1,
                    self.gutter_px,
                ) + extra
            }
            None => dock_height(self.dock_rows, self.cell_px.1, self.gutter_px),
        }
    }

    /// The mouse mapping's dock geometry: the top of the input block (in
    /// pixels in window space, from the top) and the number of input rows.
    /// `None` if there is no dock or this frame did not tell the window's
    /// bottom.
    ///
    /// If there is no input row (remote session) the row count is
    /// **zero**, not `None`: `None` goes, on the mouse side, to the one-row
    /// fallback of "no frame yet" and a phantom input block would be born on
    /// the context row. With no band at all, the same zero — a click on the
    /// lowered grid's bottom row is the grid's, not a phantom dock row's.
    ///
    /// **From the layout**, not from the drawn band: the text is
    /// bottom-anchored and stays in place throughout the animation, so the
    /// letter that is clicked is the layout's letter.
    pub(crate) fn dock_hit(&self) -> Option<(f32, u16)> {
        self.dock_band?;
        self.dock?;
        Some((
            self.dock_bottom_px - self.dock_layout_px() + self.dock_pad(),
            self.dock_rows - u16::from(self.dock_context),
        ))
    }

    /// Whether this row is the context row: the layout's **last** row, in a
    /// dock of two or more rows. The single decision point for the point-size
    /// class, the column step and the row gap — had they looked at separate
    /// thresholds the letter would be in one measure and its step in another.
    ///
    /// It looks at the row number and no flag crosses the boundary: "which row
    /// is small" is the drawing's decision, `bt-core` gives the cell (placing
    /// the context row under the input block, at row `input_rows`), and this
    /// layer chooses the point-size class.
    fn is_context_row(&self, row: u16) -> bool {
        self.dock_context && row.saturating_add(1) == self.dock_rows
    }

    /// The dock contents' dock-local vertical shift: the **breathing room**.
    ///
    /// The hairline stays at the top of the viewport (y = 0) and the margin
    /// starts **below** it; putting the margin above would push the line into
    /// the grid.
    fn dock_pad(&self) -> f32 {
        self.gutter_px
    }

    /// The dock's ground and separator, **at the given width**.
    ///
    /// The width is an argument, because `Frame` does not know the width of
    /// the touch and must not: the lists are born from the cell grid, whereas
    /// the surface must cover the **whole** window. Keeping the arithmetic
    /// here prevents the rectangle from being built by hand in `renderer.rs` —
    /// built there it would be a second writer of the `Instance` layout.
    ///
    /// The ground is **opaque** and full width: during the slide the grid's
    /// overflowing bottom row falls onto the dock (the overflow written by
    /// `LinkDelegate::set_origin`) and this rectangle is the only thing that
    /// covers it.
    ///
    /// **In the band's space**: all three are drawn from the drawn
    /// band's viewport (`Renderer::encode_dock`, `height − band`), so the
    /// ground and the top hairline rise and fall with the animation; the
    /// second hairline is **bottom-anchored** — it stays in the gap above the
    /// context row and its place comes from the difference between the band's
    /// height and the layout's.
    pub(crate) fn dock_ground(&self, width_px: f32) -> [Instance; 4] {
        let dock = self.dock.unwrap_or(DockSurface {
            ground: [0.0; 4],
            edge: [0.0; 4],
            separator: [0.0; 4],
            progress: None,
            track: [0.0; 4],
            buttons: [None; 2],
        });
        // While progress is present the line's ground is the empty track and
        // the filled part on top is in the edge's colour; when absent the
        // ground is the edge itself and the fill has zero width (the array's
        // length is fixed, so the caller does not branch).
        let (edge_base, fill) = match dock.progress {
            Some(p) => (dock.track, width_px * p.clamp(0.0, 1.0)),
            None => (dock.edge, 0.0),
        };
        let band = self.dock_band_px();
        let rows = if self.dock.is_some() {
            self.dock_rows
        } else {
            0
        };
        [
            Instance {
                // The ground covers the whole surface **margins included**: a
                // rectangle short by the margin would, during the slide, show
                // the overflowing grid row exactly where the breathing room is.
                pos: [0.0, 0.0],
                size: [width_px, band],
                rgba: dock.ground,
            },
            // The separator is **above** the ground and in the dock's topmost
            // pixel: that is the boundary between the grid and the dock. Its
            // colour is from its own field: in a remote session the surface's
            // edge says the distance, it is not a divider.
            Instance {
                pos: [0.0, 0.0],
                size: [width_px, SEPARATOR_PX],
                rgba: edge_base,
            },
            // While an upload runs, the part of the line that
            // fills from the left: by the bytes of the whole queue, in the
            // host's colour. It is rounded (the hairline's rationale: an edge
            // that does not land on the device grid would fade).
            Instance {
                pos: [0.0, 0.0],
                size: [fill.round(), SEPARATOR_PX],
                rgba: dock.edge,
            },
            // **The second separator: between the input row and the context
            // row.** Locally the same colour and the same thickness as the top
            // one, because it says the same thing — "these are two separate
            // surfaces"; it does not take the remote session's colour. An earlier
            // design had put a gap between; a gap *suggests* a separation, a line
            // **says** it (the user asked).
            //
            // Its place is the **middle** of the gap, not its top or bottom
            // edge: placed at an edge it would stick to one row and look as if
            // it belonged to it. In the middle both rows are equidistant from
            // it.
            //
            // While `rows < 2` its height is **zero**: there are not two rows
            // to separate. The array's length stays fixed so the caller does
            // not branch; a zero-height rectangle produces no fragments.
            Instance {
                pos: [
                    0.0,
                    band - self.dock_layout_px() + self.dock_row_divider_y(rows),
                ],
                size: [width_px, if rows < 2 { 0.0 } else { SEPARATOR_PX }],
                rgba: dock.separator,
            },
        ]
    }

    /// The **top** edge of the line separating the input block from the
    /// context row, in pixels in the layout's space.
    ///
    /// The rows' layout is in [`Frame::dock_pos`]: the input rows are adjacent
    /// at `pad + r·cell_h`, the context row is below the gap — i.e. the gap is
    /// between `pad + (rows − 1)·cell_h` and its `gap` excess. The line sits
    /// in the middle of that interval.
    fn dock_row_divider_y(&self, rows: u16) -> f32 {
        if rows < 2 {
            return 0.0;
        }
        let pad = self.dock_pad();
        let gap = dock_row_gap(pad);
        // Rounded: a hairline that does not land on the device grid would
        // spread over two pixels and fade — the same source as the rationale
        // for `SEPARATOR_PX` not being multiplied by the scale.
        (pad + f32::from(rows - 1) * self.cell_px.1 + (gap - SEPARATOR_PX) * 0.5).round()
    }

    pub(crate) fn dock_bg(&self) -> &[Instance] {
        &self.dock_bg
    }

    /// The dock's glyphs to draw: if an arrival is in flight, the list with
    /// their static glyph removed ([`Frame::set_dock_fx`]).
    pub(crate) fn dock_glyphs(&self) -> &[GlyphCell] {
        if self.dock_arrivals.is_empty() {
            &self.dock_glyphs
        } else {
            &self.dock_shown
        }
    }

    /// The ghosts of deleted glyphs; drawn **before** the dock's glyphs.
    pub(crate) fn dock_ghosts(&self) -> &[FxCell] {
        &self.dock_ghosts
    }

    /// The arriving glyphs; drawn **after** the dock's glyphs.
    pub(crate) fn dock_arrivals(&self) -> &[FxCell] {
        &self.dock_arrivals
    }

    /// The `heat` effect's hot colour ([`Frame::set_dock_fx`]).
    pub(crate) fn dock_fx_heat(&self) -> &[f32; 4] {
        &self.dock_fx_heat
    }

    pub(crate) fn dock_rules(&self) -> &[RuleCell] {
        &self.dock_rules
    }

    /// The caret's grid slot; `None` → the caret is not on the grid in this frame.
    pub(crate) fn grid_caret(&self) -> Option<Instance> {
        self.grid_caret.map(|caret| {
            let mut instance =
                caret.instance(self.cell_px, self.caret_shape, self.rule_px, self.glow_px());
            // **It gives the offset back** and this is not an asymmetry: this
            // slot goes through the grid's viewport, which applies
            // `+ origin_px`. The rectangle, however, is compared with the
            // fragment's `[[position]]`, and that coordinate is **after** the
            // transform, i.e. window space — there is no subtraction there. The
            // dock slot's twin does the same job with `origin_y`
            // ([`Frame::dock_caret`]).
            //
            // If the subtraction is forgotten the caret is drawn lower by the
            // offset; if the addition is forgotten (the offset put into the
            // rectangle) the colour of the letter under it falls on another
            // row — a ground-coloured letter, i.e. an invisible cell. It is
            // done at read time, because `origin_px` can still change between
            // `push_caret` and the encode (`set_origin_rows` is called after
            // the sink).
            instance.pos[1] -= self.origin_px();
            instance
        })
    }

    /// The caret's dock slot, in **dock-local** coordinates; `None` → the
    /// caret is not in the dock band in this frame.
    ///
    /// The instance is born in window space ([`Frame::push_caret`]) and the
    /// dock viewport starts `origin_y` lower: we give the difference back
    /// here. `Frame` doing the translation is the same discipline as
    /// `dock_ground` taking the width as an argument — so that no second writer
    /// of the `Instance` layout is born.
    pub(crate) fn dock_caret(&self, origin_y: f32) -> Option<Instance> {
        self.dock_caret.map(|caret| {
            let mut instance =
                caret.instance(self.cell_px, self.caret_shape, self.rule_px, self.glow_px());
            instance.pos[1] -= origin_y;
            instance
        })
    }

    /// Writes the dock band's top directly — **a shortcut for the tests**.
    /// In production the top is written in the same call as the band's height
    /// ([`Frame::set_dock_band`]) and the two cannot diverge.
    #[cfg(test)]
    pub(crate) fn set_dock_top(&mut self, top_px: f32) {
        self.dock_top_px = top_px;
    }

    /// The height of the fill **channel** in this frame, in rows: the band's
    /// (`bt_core::Cursor::fill`) plus the fraction's top row
    /// (`bt_core::Cursor::top_row`), i.e. `top_row + fill`.
    ///
    /// The top row is not a separate surface but the channel's topmost row:
    /// fill-local `0`, with the band's rows below it. The band's origin
    /// ([`Frame::fill_origin_px`]) is derived from the length, so the top row
    /// slides together with the band and the grid and no second arithmetic is
    /// born.
    ///
    /// It is called **before** the cells and that is the opposite order from
    /// the dock's: the dock's colours come back from the call that pushes the
    /// cells ([`Frame::open_dock`]), whereas the band's length is ready on
    /// `frame()`'s return and [`Frame::push_fill`]'s guard reads it. In the
    /// reverse order the guard would look at its own zero every frame.
    pub(crate) fn set_fill_rows(&mut self, rows: u16) {
        self.fill_rows = rows;
    }

    /// The height of the fill channel, in rows (top row included); zero → no
    /// channel.
    pub(crate) fn fill_rows(&self) -> u16 {
        self.fill_rows
    }

    /// The cell of a row being filled; [`Frame::push`]'s twin but in a
    /// **fill-local** row and without entering the counters.
    ///
    /// **The geometry is the grid's own** ([`Frame::pos`]), not a separate
    /// surface with a margin of its own like the dock's: the rows being filled
    /// are the grid's rows, only standing **above** the offset. Had a separate
    /// arithmetic been written the band's and the content's columns could
    /// diverge.
    ///
    /// **The position is not baked at push time** and could not be: a
    /// motion frame keeps the lists and rewrites only `origin_px`
    /// (`LinkDelegate::set_origin`), so a baked position would go stale in
    /// every frame of the slide — the fill would freeze in place while the grid
    /// glides. What carries the band onto the screen is the third
    /// `setViewport` ([`Frame::fill_origin_px`]) and it reads the origin **at
    /// encode time**.
    pub(crate) fn push_fill(&mut self, cell: Cell) {
        // The row is fill-local (`0..fill`): the boundary says "which rows",
        // it does not say "where". The band's length must be written before
        // this call, otherwise a cell that will not reach the screen would
        // silently enter the list.
        debug_assert!(
            cell.row < self.fill_rows,
            "fill row is outside the band: {} / {}",
            cell.row,
            self.fill_rows
        );
        let pos = self.pos(cell.col, cell.row);
        if let Some(bg) = cell.bg {
            self.fill_bg.push(Instance {
                pos,
                size: [self.cell_px.0, self.cell_px.1],
                rgba: bg.to_array(),
            });
        }
        if let Some(ch) = cell.ch {
            self.fill_glyphs.push(GlyphCell {
                pos,
                ch,
                face: face(cell.bold, cell.italic),
                // The grid's measure: the band is the grid's history, not a
                // separate class like the dock's context row.
                size: SizeClass::Normal,
                rgba: cell.fg.to_array(),
                wide: cell.wide,
                cluster: cell.cluster,
            });
        }
        if let Some(kind) = rule_kind(cell.underline) {
            self.fill_rules.push(RuleCell {
                pos,
                kind,
                rgba: cell.underline_color.unwrap_or(cell.fg).to_array(),
            });
        }
        if cell.strikeout {
            self.fill_rules.push(RuleCell {
                pos,
                kind: RuleKind::Strike,
                rgba: cell.fg.to_array(),
            });
        }
    }

    pub(crate) fn fill_bg(&self) -> &[Instance] {
        &self.fill_bg
    }

    pub(crate) fn fill_glyphs(&self) -> &[GlyphCell] {
        &self.fill_glyphs
    }

    pub(crate) fn fill_rules(&self) -> &[RuleCell] {
        &self.fill_rules
    }

    /// The fill band's viewport origin, in pixels: `origin_px − fill_px`.
    ///
    /// **The formula's only copy**, and it stays in `Frame` because both its
    /// terms live here (`origin_px` and the band's row count × cell height);
    /// had it been built in `renderer.rs` it would have been a second reader
    /// of the cell size — the same discipline as [`Frame::dock_ground`]
    /// keeping its arithmetic inside.
    ///
    /// **Negative is legitimate** and happens in production: when `origin_px`
    /// is smaller than the band's height the band's oldest rows overflow the
    /// top of the window and Metal clips them. Measured (Apple M1
    /// Pro / macOS 26.4.1, API validation layer on); the witness is
    /// `Renderer::tests::a_negative_viewport_origin_draws_and_clips_from_the_top`.
    ///
    /// **At read time**, not at push time: between the two `set_origin_rows`
    /// runs once more (motion frame) and the band must slide **together with**
    /// it.
    pub(crate) fn fill_origin_px(&self) -> f32 {
        self.origin_px() - f32::from(self.fill_rows) * self.cell_px.1
    }

    /// The halo's margin, in pixels — derived from the left margin
    /// ([`CARET_GLOW_RATIO`]), not a second design constant.
    ///
    /// The same inner indent is used for the third time: the left margin, the
    /// dock's breathing room and now the halo — all three from a **single
    /// source**, and when the point size grows all three grow. The halo is not
    /// the whole of that source but [`CARET_GLOW_RATIO`] of it; when it was
    /// the whole, the result was neon, not a shadow.
    fn glow_px(&self) -> f32 {
        self.gutter_px * CARET_GLOW_RATIO * self.caret_style.glow as f32
    }

    /// The caret fragment's **painted** core (x0, y0, x1, y1), window space;
    /// the fragment compares it with `[[position]]`.
    pub(crate) fn caret_core(&self) -> [f32; 4] {
        self.caret_core
    }

    /// The caret fragment's shape uniform: radius, edge, halo margin, halo
    /// alpha — all in pixels, the last 0..1.
    ///
    /// **A bare `[f32; 4]`, not a struct**: in Rust `[f32; 4]` is
    /// 4-aligned, in MSL `float4` is 16-aligned, and when the two meet inside
    /// a struct the stride silently diverges. As a lone argument both are 16
    /// bytes at offset 0, so the trap is never born.
    ///
    /// **The edge is zero on a solid caret and `rule_px` on a hollow one**:
    /// zero tells the shader "fill". Hollowing comes from focus
    /// ([`Frame::push_caret`]) and applies only to the block.
    pub(crate) fn caret_sdf(&self) -> [f32; 4] {
        #[cfg(test)]
        if let Some(shape) = self.caret_sdf_override {
            return shape;
        }
        [
            caret_radius_px(self.cell_px, self.caret_style.radius_ratio as f32),
            // The edge only on a hollow caret; on a solid caret 0 = fill. The
            // thickness is again from the font's own metric (`rule_px`), no
            // second design constant.
            //
            // **The floor is the same as `caret_painted_rect`'s**
            // (`rule.max(1.0)`) and required: `CellMetrics::new` accepts a zero
            // rule, while a zero edge tells the shader "solid". In that case,
            // with the inversion long gone, the caret would be drawn **opaque**
            // and the letter under it would stay in its own colour — the very
            // combination that cannot be read. The two halves
            // of the decision must see the same floor.
            if self.caret_hollow {
                self.rule_px.max(1.0)
            } else {
                0.0
            },
            self.glow_px(),
            CARET_GLOW_ALPHA * self.caret_style.glow as f32,
        ]
    }

    pub(crate) fn bg_count(&self) -> usize {
        self.bg_count
    }

    /// This frame's caret uniform; a degenerate rectangle for an invisible caret.
    pub(crate) fn cursor_block(&self) -> &CursorBlock {
        &self.cursor
    }

    /// The number of glyphs to draw in this frame; `make smoke`'s `glyphs=G`
    /// token. Like `bg_count` a **CPU** counter: it does not prove the atlas
    /// actually rasterized those glyphs, the offscreen test does that.
    pub(crate) fn glyph_count(&self) -> usize {
        self.glyphs.len()
    }

    /// The number of rule lines to draw in this frame; `make smoke`'s
    /// `rules=R` token.
    ///
    /// **This is the token's boundary's only owner** — the three readers
    /// (`app.rs`'s smoke gate, `Renderer::last_rule_count`, the offscreen
    /// tests) point here; with four copies, when the guard was renamed three
    /// would silently go stale. The boundary has two floors: (1) like
    /// `bg_count` and `glyph_count` a **CPU** counter, it does not prove the
    /// GPU painted those lines; (2) it **cannot see the style distinction** —
    /// code that draws all five varieties as plain lines prints the same R.
    /// The first is closed by `rule_band_is_not_uniform_along_x` and
    /// `sgr58_color_differs_from_foreground`, the second by that curl test and
    /// `bt-core`'s `smoke_shell_distinguishes_five_styles`.
    pub(crate) fn rule_count(&self) -> usize {
        self.rules.len()
    }

    /// This frame's cell size in pixels; the glyph quad's size.
    ///
    /// It is not carried per instance (see [`GlyphInstance`]) but goes as a
    /// uniform — a single value across the frame. The return is `[f32; 2]`:
    /// its only consumer passes it to the shader that way.
    pub(crate) fn cell_px(&self) -> [f32; 2] {
        [self.cell_px.0, self.cell_px.1]
    }

    pub(crate) fn bg_instances(&self) -> &[Instance] {
        &self.bg
    }

    /// This frame's command block stripes.
    ///
    /// It has **no** counter, deliberately: its three siblings (`bg_count`,
    /// `glyph_count`, `rule_count`) are `make smoke`'s tokens and the token
    /// line is a machine contract. Had the stripe entered there it would
    /// either open a new token — its value would always be zero because the
    /// smoke recipe emits no OSC 133, i.e. a gate that says nothing — or
    /// shift the meaning of an existing token. The stripe's proof is not a
    /// counter but `renderer.rs`'s offscreen pixel read.
    pub(crate) fn stripes(&self) -> &[RuleCell] {
        &self.stripes
    }

    pub(crate) fn glyphs(&self) -> &[GlyphCell] {
        &self.glyphs
    }

    /// The cluster table of the grid and the fill band ([`GlyphCell::cluster`]).
    pub(crate) fn clusters(&self) -> &Clusters {
        &self.clusters
    }

    /// The cluster table of the dock (and of the arrivals).
    pub(crate) fn dock_clusters(&self) -> &Clusters {
        &self.dock_clusters
    }

    /// The cluster table of the ghosts ([`Frame::set_dock_fx`]).
    pub(crate) fn fx_clusters(&self) -> &Clusters {
        &self.fx_clusters
    }

    /// Takes the grid's table **out** for `frame()` to fill; after the call
    /// [`Frame::put_clusters`] puts it back. A move, not a copy: while the
    /// sinks borrow `Frame` the table cannot be the same call's second `&mut`.
    /// The clearing is in [`Frame::clear`], so the table taken is empty.
    pub(crate) fn take_clusters(&mut self) -> Clusters {
        std::mem::take(&mut self.clusters)
    }

    pub(crate) fn put_clusters(&mut self, clusters: Clusters) {
        self.clusters = clusters;
    }

    /// The twin of [`Frame::take_clusters`] for the dock's table.
    pub(crate) fn take_dock_clusters(&mut self) -> Clusters {
        std::mem::take(&mut self.dock_clusters)
    }

    pub(crate) fn put_dock_clusters(&mut self, clusters: Clusters) {
        self.dock_clusters = clusters;
    }

    pub(crate) fn rules(&self) -> &[RuleCell] {
        &self.rules
    }

    /// The top-left corner of a grid coordinate, in pixels — **the arithmetic
    /// common to the three lists**.
    ///
    /// [`Frame::push`] calls it once per cell, so the guard now runs on
    /// **every** pushed cell: a cell pushed without `clear` being called is
    /// born zero-sized and silently vanishes from the screen. Since there is a
    /// single call, the formula can no longer be copied into the branches; the
    /// caret path ([`Frame::push_caret`]) is a second caller through the same
    /// function.
    fn pos(&self, col: u16, row: u16) -> [f32; 2] {
        self.pos_at([f32::from(col), f32::from(row)])
    }

    /// [`Frame::pos`]'s dock version: the same column arithmetic, **plus the
    /// breathing room**.
    ///
    /// This is the **only** place the margin is added and the rationale is the
    /// same as the left margin's ([`Frame::pos_at`]): background, glyph, rule
    /// and caret all four go through this line; had it been added in a second
    /// place the margin would be applied twice.
    fn dock_pos(&self, col: u16, row: u16) -> [f32; 2] {
        // The column **step** by row: on the context row, the small face's
        // advance. Had it been drawn with the large step the small letters
        // would be scattered over the large cells' left edges with gaps
        // between them for no reason — the eye reads that as "written letter
        // by letter".
        //
        // The vertical arithmetic **does not change**: both the row's height
        // and the margin are common, the small glyph stands on the large
        // cell's baseline. The band ([`dock_px`]) therefore never shortens.
        //
        // **The row gap is only above the context row**: the input rows
        // are a single editor surface, adjacent. The layout is counted from
        // the top but its viewport is bottom-anchored (`height − layout`,
        // `Renderer::encode_dock`), so the context row is always at the bottom
        // of the band and the input rows are stacked above it.
        let [_, y] = self.pos(0, row);
        let w = self.column_px(row);
        let pad = self.dock_pad();
        // The gap separates the context row from **the input row above it**:
        // in a layout that is the context row alone (remote session) there is
        // nothing to separate and the band's height does not count it either
        // ([`dock_height`]).
        let gap = if self.is_context_row(row) && self.dock_rows >= 2 {
            dock_row_gap(pad)
        } else {
            0.0
        };
        [self.gutter_px + f32::from(col) * w, y + pad + gap]
    }

    /// The column step in the dock's `row`, in pixels.
    ///
    /// The single decision point: which row is small is asked **only** here
    /// and in [`Frame::push_dock`], and both look at the same constant. Had
    /// they looked at separate thresholds the position would be small and the
    /// glyph large (or the reverse).
    fn column_px(&self, row: u16) -> f32 {
        if self.is_context_row(row) {
            self.context_cell_px
        } else {
            self.cell_px.0
        }
    }

    /// [`Frame::pos`]'s fractional version — when the caret is between two
    /// cells.
    ///
    /// This is the formula's **only** copy; the integer path goes through here
    /// so that the gliding caret and a resting cell share the same arithmetic.
    /// Had they diverged, a settled caret could drift half a pixel from the
    /// letter under it.
    ///
    /// The left margin is also added **only here**: column 0 starts where the
    /// margin ends and background, glyph, rule and caret all four go through
    /// this line. Had it been added in a second place the margin would be
    /// applied twice.
    fn pos_at(&self, at: [f32; 2]) -> [f32; 2] {
        let (w, h) = self.cell_px;
        debug_assert!(w > 0.0 && h > 0.0, "clear(metrics) was not called");
        [self.gutter_px + at[0] * w, at[1] * h]
    }
}

#[cfg(test)]
mod tests {
    use bt_core::{CaretShape, CaretStyle, Cursor};

    use super::*;

    // The extremes are deliberate: `0.0`/`1.0` are the fixed points of the
    // sRGB transfer function, so these tests are independent of the colour
    // space. The place that tests the space is `renderer.rs` →
    // `cell_bg_paints_pixels_on_the_gpu`.
    //
    // The embedded theme's two distinct colours; the names tell their sources,
    // not their roles. What is looked at here is layout, not colour, so there
    // is no need to make up colours (`LinearRgba::from_srgb`) — `renderer.rs`'s
    // offscreen tests use that because they need three distinct tones.
    const BG: LinearRgba = bt_core::Theme::BATERI.background_linear();
    const CURSOR: LinearRgba = bt_core::Theme::BATERI.accent_linear();
    /// The colour of the text under the block; in production the theme's ground
    /// (`bt-core` → `Cursor::text`). Here it is enough for it to be **distinct**
    /// from the block's colour.
    const TEXT: LinearRgba = BG;
    /// The opacity of a settled caret — every frame outside the fade is this
    /// (`crate::motion::Motion::alpha`). The only place that tests the fade is
    /// `cursor_alpha_reaches_the_block_and_the_text`.
    const OPAQUE: f32 = 1.0;

    #[test]
    fn a_lone_run_rounds_all_four_corners() {
        let runs = [run(3, 2, 9)];
        assert_eq!(selection_corners(&runs, 0), [Corner::Convex; 4]);
    }

    #[test]
    fn two_equal_runs_square_their_inner_corners() {
        use Corner::{Convex, Square};
        let runs = [run(3, 2, 9), run(4, 2, 9)];
        assert_eq!(
            selection_corners(&runs, 0),
            [Convex, Convex, Square, Square]
        );
        assert_eq!(
            selection_corners(&runs, 1),
            [Square, Square, Convex, Convex]
        );
    }

    #[test]
    fn a_step_gets_one_concave_fill_from_the_narrower_run() {
        use Corner::{Concave, Convex, Square};
        // The typical state of a flow selection: on top from 4 to the end of
        // the row, below from the start of the row to the end. On the left edge
        // the lower row is longer → the upper run's BL is concave; the right
        // edge is aligned → square.
        let runs = [run(3, 4, 9), run(4, 0, 9)];
        assert_eq!(
            selection_corners(&runs, 0),
            [Convex, Convex, Square, Concave]
        );
        assert_eq!(
            selection_corners(&runs, 1),
            [Convex, Square, Convex, Convex]
        );
        // The reverse step: the lower run is narrow and stays to the right →
        // its TR is concave.
        let runs = [run(3, 0, 9), run(4, 0, 5)];
        assert_eq!(
            selection_corners(&runs, 0),
            [Convex, Convex, Convex, Square]
        );
        assert_eq!(
            selection_corners(&runs, 1),
            [Square, Concave, Convex, Convex]
        );
        // The fill is only in the narrow run: the two rows' concave-corner
        // total is one.
        let concave = |runs: &[SelectionRun]| {
            (0..runs.len())
                .flat_map(|i| selection_corners(runs, i))
                .filter(|&c| c == Concave)
                .count()
        };
        assert_eq!(concave(&[run(3, 4, 9), run(4, 0, 9)]), 1);
        assert_eq!(
            concave(&[run(3, 3, 6), run(4, 0, 9)]),
            2,
            "a step on both sides"
        );
    }

    #[test]
    fn diagonal_runs_touch_only_at_a_point_and_stay_convex() {
        // The upper starts at 5, the lower ends at 4: they share no column at
        // all, so two separate rounded pieces.
        let runs = [run(3, 5, 9), run(4, 0, 4)];
        assert_eq!(selection_corners(&runs, 0), [Corner::Convex; 4]);
        assert_eq!(selection_corners(&runs, 1), [Corner::Convex; 4]);
    }

    #[test]
    fn a_blank_row_between_runs_splits_the_shape() {
        let runs = [run(3, 0, 9), run(5, 0, 9)];
        assert_eq!(selection_corners(&runs, 0), [Corner::Convex; 4]);
        assert_eq!(selection_corners(&runs, 1), [Corner::Convex; 4]);
    }

    /// A grid measure with a **zero** left margin: most of the tests in this
    /// module ask about the lists' layout, not the origin, and a zero margin
    /// keeps their expected pixels in cell arithmetic. The margin's own tests
    /// use [`GUTTER`] and name it.
    fn grid(width: u16, height: u16) -> CellMetrics {
        CellMetrics::new(width, height, width, 0, 1).expect("non-zero cell")
    }

    /// A selection's row run; shorthand for the corner-decision tests.
    fn run(row: u16, first: u16, last: u16) -> SelectionRun {
        SelectionRun { row, first, last }
    }

    /// The measure for the tests that query the margin. The value need not be
    /// the same as production's — what is asked is "is the margin added to the
    /// origin", not its width — and it was chosen **distinct** from the cell
    /// width so that an accidental overlap of the two factors does not let the
    /// test pass silently.
    const GUTTER: u16 = 7;

    /// The test shell of the old `Frame::push_cursor` signature: it translates
    /// into the single caret API. Most tests talk to `Cursor` and that record
    /// is still `bt-core`'s boundary type; only `Frame`'s inner slot changed.
    fn push_cursor(frame: &mut Frame, cursor: Cursor, at: [f32; 2], rgba: LinearRgba, alpha: f32) {
        if cursor.visible {
            frame.push_caret(at, cursor.text, rgba, alpha, cursor.shape, true);
        }
    }

    /// The test shell of `Frame::move_cursor`; an invisible cursor deletes the caret.
    fn move_cursor(
        frame: &mut Frame,
        cursor: Cursor,
        at: [f32; 2],
        rgba: LinearRgba,
        alpha: f32,
        focused: bool,
    ) {
        frame.move_caret(at, cursor.text, rgba, alpha, focused);
        if !cursor.visible {
            frame.clear_caret();
        }
    }

    fn cursor(col: u16, row: u16, visible: bool) -> Cursor {
        Cursor {
            next_tick: None,
            col,
            row,
            visible,
            // This module tests the grid's lists; the handover is `link`'s question.
            caret_in_dock: false,
            input_rows: 1,
            band_hidden: false,
            shape: CaretShape::Block,
            blink: false,
            text: TEXT,
            // The scrolling decision is motion's job (`motion.rs`); it does
            // not concern this list, because the position already comes from
            // outside.
            display_offset: 0,
            // The fill count does not concern this list either: the offset
            // is told by `set_origin_rows` and these two fields are its
            // **input**, i.e. where `link.rs` reads. A full grid, i.e. offset
            // zero.
            content_rows: 1,
            // Drawing the fill is a separate path; this list does not
            // consume it.
            fill: 0,
            // Fractional scrolling does not concern this list either:
            // at a whole row, no top row.
            top_row: 0,
            scrolled: 0,
            scroll_frac: 0.0,
            scroll_generation: 0,
            rows: 1,
        }
    }

    /// Draws the cursor in **its own** cell: the settled (unanimated) state.
    /// The only place that tests the in-between position is
    /// `cursor_slides_between_cells`.
    fn push_settled(frame: &mut Frame, cursor: Cursor) {
        push_cursor(
            frame,
            cursor,
            [f32::from(cursor.col), f32::from(cursor.row)],
            CURSOR,
            OPAQUE,
        );
    }

    fn bg_cell(col: u16, row: u16) -> Cell {
        Cell {
            col,
            row,
            fg: CURSOR,
            bg: Some(BG),
            ..Default::default()
        }
    }

    #[test]
    fn frame_bg_count_excludes_cursor() {
        let mut frame = Frame::default();
        frame.clear(grid(9, 18), CaretStyle::default());

        frame.push(bg_cell(0, 0));
        frame.push(bg_cell(1, 0));
        push_settled(&mut frame, cursor(5, 2, true));

        // The caret **never enters** the background list any more: it has its
        // own slot and the encode draws it separately. So the list's length is
        // now the same number as `cells=K` — the counter excluding the caret
        // used to be a subtraction, now it is structural.
        assert_eq!(frame.bg_instances().len(), 2);
        assert_eq!(frame.bg_count(), 2);
        assert!(frame.grid_caret().is_some(), "caret is not drawn");

        frame.clear(grid(9, 18), CaretStyle::default());
        assert_eq!(frame.bg_count(), 0);
        assert!(frame.bg_instances().is_empty());
    }

    #[test]
    fn invisible_cursor_is_not_drawn() {
        let mut frame = Frame::default();
        frame.clear(grid(9, 18), CaretStyle::default());
        push_settled(&mut frame, cursor(0, 0, false));
        assert!(frame.bg_instances().is_empty());
        // The uniform is also left untouched: a degenerate rectangle is the
        // only way to say "no block", there is no second flag in the shader.
        assert_eq!(frame.cursor_block(), &CursorBlock::default());
    }

    #[test]
    fn an_unfocused_block_is_hollow_and_stops_inverting() {
        // **The two marks of an unfocused window** and both from a single
        // decision (`push_caret`'s `focused`): the caret's inside is hollowed
        // out (edge thickness `rule_px`) and the inversion **is lifted**.
        //
        // The second is required: the inversion rests on the painted ground.
        // Had the letter in the middle of the frame been drawn in the ground
        // colour, since nothing is painted under it it would become
        // **invisible** — a hollow caret would swallow the text.
        let mut frame = Frame::default();
        frame.clear(grid(9, 18), CaretStyle::default());
        frame.push_caret([1.0, 1.0], TEXT, CURSOR, OPAQUE, CaretShape::Block, false);

        assert_eq!(
            frame.cursor_block().rect,
            [0.0; 4],
            "unfocused caret still inverts: the letter under it vanishes"
        );
        assert!(
            frame.caret_sdf()[1] > 0.0,
            "hollow caret's edge is not drawn"
        );

        // The painted area is **in the same place**: a hollow caret occupies the same cell.
        let hollow = frame.grid_caret().expect("caret");
        let mut lit = Frame::default();
        lit.clear(grid(9, 18), CaretStyle::default());
        lit.push_caret([1.0, 1.0], TEXT, CURSOR, OPAQUE, CaretShape::Block, true);
        let solid = lit.grid_caret().expect("caret");
        assert_eq!(hollow.pos, solid.pos, "footprint moved when hollowed");
        assert_eq!(hollow.size, solid.size);
        assert_eq!(lit.caret_sdf()[1], 0.0, "focused caret is hollow");
    }

    #[test]
    fn a_hollow_caret_survives_a_zero_rule_metric() {
        // **The two halves of the decision must see the same floor**.
        // `CellMetrics::new` accepts a zero rule; had the
        // edge been left without a floor the hollow caret's `stroke` would be
        // 0, the shader would read it as "solid" and the caret would be drawn
        // **opaque** — and since the inversion is already lifted, the letter
        // under it would stay in its own colour and the unreadable combination
        // would appear.
        let mut frame = Frame::default();
        frame.clear(
            CellMetrics::new(10, 20, 10, 0, 0).expect("non-zero cell"),
            CaretStyle::default(),
        );
        frame.push_caret([0.0, 0.0], TEXT, CURSOR, OPAQUE, CaretShape::Block, false);
        assert!(
            frame.caret_sdf()[1] > 0.0,
            "hollow caret filled in under a zero rule metric"
        );
        assert_eq!(
            frame.cursor_block().rect,
            [0.0; 4],
            "the inversion should have been lifted"
        );
    }

    #[test]
    fn the_unfocused_setting_keeps_the_caret_solid() {
        // `[terminal] cursor_unfocused = "solid"`: even when focus is gone it
        // is not hollowed out and **the inversion stays** — both from one decision.
        let mut frame = Frame::default();
        frame.clear(
            grid(9, 18),
            CaretStyle {
                unfocused: UnfocusedCaret::Solid,
                ..CaretStyle::default()
            },
        );
        frame.push_caret([1.0, 1.0], TEXT, CURSOR, OPAQUE, CaretShape::Block, false);
        assert_eq!(frame.caret_sdf()[1], 0.0, "hollowed out while solid");
        assert_ne!(
            frame.cursor_block().rect,
            [0.0; 4],
            "inversion was lifted while solid"
        );

        // The default (`hollow`) hollows it on the same input: the distinction
        // really comes from the key, not from anything else.
        let mut lit = Frame::default();
        lit.clear(grid(9, 18), CaretStyle::default());
        lit.push_caret([1.0, 1.0], TEXT, CURSOR, OPAQUE, CaretShape::Block, false);
        assert!(lit.caret_sdf()[1] > 0.0, "stayed filled while hollow");
    }

    #[test]
    fn thin_carets_never_go_hollow() {
        // **Hollowing is only for the block.** The underline and the vertical
        // bar are already thin strips and the edge thickness is `rule_px` too,
        // so the subtraction would swallow the body entirely and the caret
        // would become invisible. For those shapes the unfocused signal is the
        // blink stopping.
        for shape in [CaretShape::Underline, CaretShape::Beam] {
            let mut frame = Frame::default();
            frame.clear(grid(9, 18), CaretStyle::default());
            frame.push_caret([0.0, 0.0], TEXT, CURSOR, OPAQUE, shape, false);
            assert_eq!(frame.caret_sdf()[1], 0.0, "{shape:?} went hollow");
            assert_ne!(
                frame.cursor_block().rect,
                [0.0; 4],
                "{shape:?} stopped inverting"
            );
        }
    }

    #[test]
    fn a_motion_frame_follows_the_live_focus() {
        // **The focus is fresh in a motion frame too** and this came with a
        // user report (2026-09-20): "when I return to the window the frame
        // stays but the inside is empty, it fills in later". The cause was a
        // stored copy being kept — the focus is `bt-gpu`'s own bit and the
        // motion frame **has access** to it, so keeping it made it stale.
        // Keeping is right for the shape: that comes from `bt-core` and this
        // path has no access to it.
        let mut frame = Frame::default();
        frame.clear(grid(9, 18), CaretStyle::default());

        // Pushed unfocused, the motion frame arrives **focused**: the caret must fill in.
        frame.push_caret([1.0, 1.0], TEXT, CURSOR, OPAQUE, CaretShape::Block, false);
        assert!(frame.caret_sdf()[1] > 0.0, "should have started hollow");
        frame.move_caret([2.0, 1.0], TEXT, CURSOR, OPAQUE, true);
        assert_eq!(frame.caret_sdf()[1], 0.0, "motion frame missed focus");
        assert_ne!(frame.cursor_block().rect, [0.0; 4], "inversion not back");

        // The reverse direction: when focus leaves, the motion frame hollows it at once too.
        frame.move_caret([3.0, 1.0], TEXT, CURSOR, OPAQUE, false);
        assert!(frame.caret_sdf()[1] > 0.0);
        assert_eq!(frame.cursor_block().rect, [0.0; 4]);
    }

    #[test]
    fn cursor_block_covers_its_cell_and_clears_with_the_frame() {
        // The rectangle must sit on the same cell as the block's **own**
        // instance: had they diverged the block would be in one place and the
        // colour of the text under it in another, and both would be drawn
        // silently wrong.
        let mut frame = Frame::default();
        frame.clear(grid(9, 18), CaretStyle::default());
        push_settled(&mut frame, cursor(3, 2, true));

        let block = *frame.cursor_block();
        assert_eq!(block.rect, [27.0, 36.0, 36.0, 54.0]);
        assert_eq!(
            block.rgba,
            TEXT.to_array(),
            "the text colour comes from `Cursor`"
        );
        let instance = frame.grid_caret().expect("block instance");
        assert_eq!(
            [
                instance.pos[0],
                instance.pos[1],
                instance.pos[0] + instance.size[0],
                instance.pos[1] + instance.size[1],
            ],
            block.rect,
            "rectangle diverged from the block's instance"
        );

        // `clear` zeroes the uniform too: if a caret that has gone dark
        // (`\e[?25l`) removes the block but leaves the colour in the old
        // cell, that cell becomes invisible.
        frame.clear(grid(9, 18), CaretStyle::default());
        assert_eq!(frame.cursor_block(), &CursorBlock::default());
    }

    #[test]
    fn cursor_alpha_reaches_the_block_and_the_text() {
        // Reduce Motion's fade must be written to **two** places at once: the
        // block's instance and the `cell` pipeline's uniform. Had it been
        // written only to the block, the letter would be painted in the colour
        // of a block not yet visible — a ground-coloured letter on the ground,
        // i.e. an unreadable cell; and the symptom is confined to exactly that
        // cell, no counter sees it.
        let mut frame = Frame::default();
        frame.clear(grid(9, 18), CaretStyle::default());
        push_cursor(&mut frame, cursor(3, 2, true), [3.0, 2.0], CURSOR, 0.25);

        let instance = frame.grid_caret().expect("block instance");
        assert_eq!(instance.rgba[3], 0.25, "block opacity was not carried");
        assert_eq!(
            frame.cursor_block().rgba[3],
            0.25,
            "text opacity diverged from the block's"
        );
        // The colour components are **unaffected** by the opacity: the GPU
        // does the blending, there is no premultiplication here
        // (`Renderer::pipeline`).
        assert_eq!(instance.rgba[..3], CURSOR.to_array()[..3]);
        assert_eq!(frame.cursor_block().rgba[..3], TEXT.to_array()[..3]);

        // A settled caret is opaque and in that case both arrays are the theme itself.
        frame.clear(grid(9, 18), CaretStyle::default());
        push_settled(&mut frame, cursor(3, 2, true));
        assert_eq!(frame.grid_caret().expect("block").rgba, CURSOR.to_array());
        assert_eq!(frame.cursor_block().rgba, TEXT.to_array());
    }

    #[test]
    fn cursor_slides_between_cells() {
        // The in-between position: while the block is between two cells the
        // rectangle too must sit on a fractional pixel. Had it been rounded to
        // an integer the slide would jump cell by cell and the whole animation
        // would be invisible.
        let mut frame = Frame::default();
        frame.clear(grid(10, 20), CaretStyle::default());
        push_cursor(&mut frame, cursor(3, 2, true), [2.5, 1.25], CURSOR, OPAQUE);
        assert_eq!(frame.cursor_block().rect, [25.0, 25.0, 35.0, 45.0]);
        assert_eq!(frame.grid_caret().expect("caret").pos, [25.0, 25.0]);
    }

    #[test]
    fn a_motion_frame_keeps_the_lists_and_moves_only_the_cursor() {
        // The motion frame's contract: the grid is not dirty, so the glyph and
        // rule lists must stay valid; the only thing trimmed is the previous
        // frame's caret. Without the trimming every motion frame would add one
        // more rectangle to the list — twenty-four ghost carets in a 200 ms
        // slide.
        let mut frame = Frame::default();
        frame.clear(grid(8, 16), CaretStyle::default());
        frame.push(bg_cell(0, 0));
        frame.push(Cell {
            col: 1,
            row: 0,
            ch: Some('b'),
            fg: CURSOR,
            bg: Some(BG),
            underline: UnderlineStyle::Single,
            ..Default::default()
        });
        push_settled(&mut frame, cursor(0, 0, true));
        let (cells, glyphs, rules) = (frame.bg_count(), frame.glyph_count(), frame.rule_count());
        assert_eq!((cells, glyphs, rules), (2, 1, 1));

        for _ in 0..3 {
            move_cursor(
                &mut frame,
                cursor(5, 0, true),
                [4.5, 0.0],
                CURSOR,
                OPAQUE,
                true,
            );
            // All three counters stayed put: `cells=8 glyphs=6 rules=15` must
            // stay the same in the smoke run even if it ends on a motion frame.
            assert_eq!(frame.bg_count(), cells);
            assert_eq!(frame.glyph_count(), glyphs);
            assert_eq!(frame.rule_count(), rules);
            // The list is **independent of the caret**: the caret is in its own
            // slot and the motion frame writes it there, without touching the
            // backgrounds.
            assert_eq!(frame.bg_instances().len(), cells);
            assert!(frame.grid_caret().is_some(), "motion frame lost caret");
        }
        assert_eq!(frame.cursor_block().rect, [36.0, 0.0, 44.0, 16.0]);

        // A motion frame arriving with an invisible caret **removes** the
        // block: had the uniform stayed in its old place, a ground-coloured
        // letter would stand there.
        move_cursor(
            &mut frame,
            cursor(5, 0, false),
            [4.5, 0.0],
            CURSOR,
            OPAQUE,
            true,
        );
        assert_eq!(frame.bg_instances().len(), cells);
        assert!(frame.grid_caret().is_none(), "invisible caret left a block");
        assert_eq!(frame.cursor_block(), &CursorBlock::default());
    }

    #[test]
    fn clear_updates_cell_size() {
        // This is the only reason the grid measure is a parameter of `clear`:
        // had it been a field it would go stale when the screen scale changes
        // and no test would see it. The margin comes from the same call too,
        // i.e. under the same guard.
        let mut frame = Frame::default();
        frame.clear(grid(9, 18), CaretStyle::default());
        frame.push(bg_cell(1, 1));
        assert_eq!(frame.bg_instances()[0].pos, [9.0, 18.0]);

        frame.clear(grid(18, 36), CaretStyle::default());
        frame.push(bg_cell(1, 1));
        assert_eq!(frame.bg_instances()[0].pos, [18.0, 36.0]);
        assert_eq!(frame.bg_instances()[0].size, [18.0, 36.0]);
    }

    #[test]
    fn grid_coords_convert_to_pixels() {
        let mut frame = Frame::default();
        frame.clear(grid(9, 18), CaretStyle::default());
        frame.push(bg_cell(3, 2));
        assert_eq!(
            frame.bg_instances()[0],
            Instance {
                pos: [27.0, 36.0],
                size: [9.0, 18.0],
                rgba: BG.to_array(),
            }
        );
    }

    #[test]
    fn the_gutter_offsets_every_pixel_position() {
        // The left margin is added to the drawing origin
        // **once** in `pos_at`; since all four consumers (background, glyph,
        // rule, caret) go through that line, all four shift by the same
        // amount. Had it been added in two places one would apply the margin
        // twice and the symptom would be "the glyph is shifted from its
        // background".
        let mut frame = Frame::default();
        frame.clear(
            CellMetrics::new(9, 18, 9, GUTTER, 1).expect("metrics"),
            CaretStyle::default(),
        );
        frame.push(Cell {
            col: 3,
            row: 2,
            ch: Some('x'),
            fg: CURSOR,
            bg: Some(BG),
            underline: UnderlineStyle::Single,
            ..Default::default()
        });
        push_settled(&mut frame, cursor(1, 0, true));

        let shifted = [f32::from(GUTTER) + 27.0, 36.0];
        assert_eq!(frame.bg_instances()[0].pos, shifted, "background");
        assert_eq!(frame.glyphs()[0].pos, shifted, "glyph");
        assert_eq!(frame.rules()[0].pos, shifted, "rule");
        // The caret goes through the same line: column 1 is 9 pixels to the right of the margin.
        assert_eq!(
            frame.cursor_block().rect[0],
            f32::from(GUTTER) + 9.0,
            "caret"
        );

        // **The size does not shift, only the position**: the margin pushes
        // the grid, it does not enlarge the cell.
        assert_eq!(frame.bg_instances()[0].size, [9.0, 18.0]);
    }

    #[test]
    fn the_cursor_rect_keeps_the_screen_row_and_the_instance_gives_the_origin_back() {
        // **The third symptom that was left without a guard.** The
        // caret's rectangle and its `bg` instance live in two separate spaces:
        // the instance goes through the vertex stage, i.e. `setViewport`, and
        // the GPU **gives the offset back**; the rectangle is compared with the
        // fragment's `[[position]]`, and that coordinate is **after** the
        // transform, i.e. already a screen coordinate. If the two diverge the
        // caret appears in the right place but the colour of the text beneath
        // it falls on another row — a defect that leaves `make check` green
        // and is noticed by eye as "a cell became invisible".
        //
        // Two offsets are tested and the non-zero one is the real one: at zero
        // the two sides are equal and code that deletes `origin_px`
        // altogether would pass too.
        let mut frame = Frame::default();
        frame.clear(grid(9, 18), CaretStyle::default());
        frame.push(bg_cell(0, 2));
        push_settled(&mut frame, cursor(0, 2, true));
        let cell_y = frame.bg_instances()[0].pos[1];
        assert_eq!(cell_y, 36.0);
        assert_eq!(
            frame.cursor_block().rect[1],
            cell_y,
            "in a frame without offset the rectangle is on the same row as the cell"
        );

        // The same frame, offset by two rows. The caret's target is a
        // **screen** row, i.e. grid row 2 + offset 2 = 4; its place on screen
        // is `cell_y + 36`, because the content slid down that much too.
        // Pushing the caret again is required: the rectangle bakes the offset
        // at `push_cursor` time and in production the order is like that too
        // (`link.rs` writes the origin **before** the caret).
        frame.set_origin_rows(2.0);
        move_cursor(
            &mut frame,
            cursor(0, 2, true),
            [0.0, 4.0],
            CURSOR,
            OPAQUE,
            true,
        );
        assert_eq!(
            frame.grid_caret().expect("caret").pos[1],
            cell_y,
            "instance did not give the offset back: the viewport adds it once more"
        );
        assert_eq!(
            frame.cursor_block().rect[1],
            cell_y + 36.0,
            "rectangle is not at the screen row: the text under the block stays on another row"
        );
        assert_eq!(
            frame.cursor_block().rect[3],
            cell_y + 36.0 + 18.0,
            "the rectangle's bottom must shift as much too: otherwise its height changes"
        );
    }

    #[test]
    fn a_sliding_origin_lands_on_whole_device_pixels() {
        // **Where the slide stops is permanent on screen:** the link's "no
        // damage" branch sleeps without ever drawing the frame in which the
        // animation *settled*, i.e. the last frame left on screen is one step
        // before settling. Had a fractional pixel frozen there all the text
        // would be shifted by up to half a pixel, i.e. blurry after every
        // Enter — no counter sees it, the eye does.
        let mut frame = Frame::default();
        frame.clear(grid(9, 18), CaretStyle::default());
        frame.set_origin_rows(1.51);
        assert_eq!(frame.origin_px(), 27.0, "offset was not rounded to pixels");

        // One step before settling (~0.009 rows ≈ 0.16 pixels) the offset's
        // pixel is **exactly** at the target: the screen is sharp even before
        // the slide ends.
        frame.set_origin_rows(2.0 - 0.009);
        assert_eq!(frame.origin_px(), 36.0, "frame before settling is shifted");

        // At a whole row rounding is the identity: the three origin guards and
        // the settled state in production go through here.
        frame.set_origin_rows(3.0);
        assert_eq!(frame.origin_px(), 54.0);
    }

    #[test]
    fn a_zero_gutter_leaves_the_origin_at_the_edge() {
        // A zero margin is a legitimate answer (in a future without
        // integration, or at a degenerate scale): the grid starts at the edge
        // and the arithmetic returns bit for bit to the state where the margin
        // is not added. This is also the contract the other tests in this
        // module rest on through `grid()`.
        let mut frame = Frame::default();
        frame.clear(grid(9, 18), CaretStyle::default());
        frame.push(bg_cell(3, 2));
        assert_eq!(frame.bg_instances()[0].pos, [27.0, 36.0]);
    }

    /// The stripe's colour: the theme's status role, the same source as the
    /// value `frame()`'s boundary resolves and gives (`bt-core` → `Block::stripe`).
    const SUCCESS: LinearRgba = bt_core::Theme::BATERI.success_linear();

    fn block(row: u16) -> Block {
        Block {
            row,
            stripe: SUCCESS,
        }
    }

    #[test]
    fn a_mark_covers_one_row_and_lines_up_with_the_dock_sigil() {
        // All three of the mark's claims can silently break: (1) **the same
        // sprite as the dock's chevron** — both are a prompt mark in the phase
        // colour and drawing them with separate shapes was a leftover; (2) it
        // starts on its own row — it shows the command's row, not a range; (3)
        // **at column 0**, i.e. at the same x as the dock's prompt mark.
        let mut frame = Frame::default();
        frame.clear(
            CellMetrics::new(9, 18, 9, GUTTER, 1).expect("metrics"),
            CaretStyle::default(),
        );
        frame.push_block(block(2));

        let mark = frame.stripes()[0];
        assert_eq!(mark.kind, RuleKind::Chevron, "mark is still a rectangle");
        assert_eq!(mark.pos[1], 36.0, "mark must start on its own row");
        assert_eq!(mark.rgba, SUCCESS.to_array(), "colour from the boundary");
        // **The alignment is not computed, it comes from a single formula.**
        // The dock's mark is at column 0 too and goes through `Frame::pos` as
        // well; as long as the two were placed with separate arithmetic they
        // stood half a margin apart.
        frame.clear(
            CellMetrics::new(9, 18, 9, GUTTER, 1).expect("metrics"),
            CaretStyle::default(),
        );
        frame.push_dock(Cell {
            col: 0,
            row: 0,
            ch: Some('>'),
            ..Cell::default()
        });
        frame.push_block(block(0));
        assert_eq!(
            frame.stripes()[0].pos[0],
            frame.dock_glyphs()[0].pos[0],
            "grid's mark is not in the same column as the dock's"
        );
        // Also the same when the margin changes: both go through the same margin.
        frame.clear(
            CellMetrics::new(4, 18, 4, 12, 1).expect("metrics"),
            CaretStyle::default(),
        );
        frame.push_dock(Cell {
            col: 0,
            row: 0,
            ch: Some('>'),
            ..Cell::default()
        });
        frame.push_block(block(0));
        assert_eq!(frame.stripes()[0].pos[0], 12.0, "mark skipped the margin");
        assert_eq!(frame.stripes()[0].pos[0], frame.dock_glyphs()[0].pos[0]);

        frame.clear(
            CellMetrics::new(9, 18, 9, GUTTER, 1).expect("metrics"),
            CaretStyle::default(),
        );
        assert!(
            frame.stripes().is_empty(),
            "`clear` must empty the marks too"
        );
    }

    #[test]
    fn the_context_budget_is_the_same_strip_in_small_steps() {
        // The budget is a **ratio**: the two rows occupy the same horizontal
        // strip (the left margin is common), the only thing that differs is how
        // many pixels a letter advances. The number is born here because
        // `bt-core` sees no pixels.
        let m = |w, cw| CellMetrics::new(w, 20, cw, GUTTER, 1).expect("metrics");
        // 80 × 10 pixels = 800; 100 columns at an 8-pixel step.
        assert_eq!(context_cols(80, m(10, 8)), 100);
        // If the ratio is 1 the budget is the same too: the "no shrinking" arm
        // is a **supported** state and its output is bit for bit today's.
        assert_eq!(context_cols(80, m(10, 10)), 80);
        // A ratio that does not divide evenly rounds **down**: giving one
        // column too many would push the row out of the margin.
        assert_eq!(context_cols(10, m(10, 3)), 33);
        // No overflow at the degenerate extreme: the product lives in `u32` and the result is clamped.
        assert_eq!(context_cols(u16::MAX, m(u16::MAX, 1)), u16::MAX);
    }

    #[test]
    fn the_context_row_steps_by_the_small_advance() {
        // **The dock's two rows at two separate column steps.** The input row
        // is in the display font, the context row in the small face: more
        // letters fit in the same pixel strip. Since the small glyph stands on
        // the large slot's left edge the quad can stay large and no separate
        // draw call is born (`GlyphCell::size`).
        let mut frame = Frame::default();
        frame.clear(
            CellMetrics::new(10, 20, 8, GUTTER, 1).expect("metrics"),
            CaretStyle::default(),
        );
        let at = |col, row| Cell {
            col,
            row,
            ch: Some('x'),
            ..Cell::default()
        };
        frame.push_dock(at(0, 0));
        frame.push_dock(at(3, 0));
        frame.push_dock(at(0, 1));
        frame.push_dock(at(3, 1));

        let g = frame.dock_glyphs();
        // Both rows start from the left margin: the step is separate, **the
        // start is common**.
        assert_eq!(g[0].pos[0], g[2].pos[0], "rows start in other columns");
        // The input row advances at the display measure.
        assert_eq!(g[1].pos[0] - g[0].pos[0], 30.0, "input row's step");
        // The context row at the small measure.
        assert_eq!(g[3].pos[0] - g[2].pos[0], 24.0, "context row's step");
        // The point-size class is from **the same threshold** as the position:
        // the letter cannot be in one measure and its step in another.
        assert_eq!(g[1].size, SizeClass::Normal);
        assert_eq!(g[3].size, SizeClass::Small);

        // **The vertical arithmetic is untouched.** The band's height and the
        // rows' y both come from the large cell: the small letter stands on
        // the large row's baseline and the row does not shrink its own band.
        // So `dock_px`'s consumers (`split_into_grid`, the second viewport)
        // did not change at all.
        assert_eq!(
            g[2].pos[1] - g[0].pos[1],
            20.0 + dock_row_gap(GUTTER as f32)
        );
        frame.open_dock(BG, CURSOR, CURSOR);
        assert_eq!(
            frame.dock_layout_px(),
            dock_px(
                DOCK_ROWS,
                CellMetrics::new(10, 20, 8, GUTTER, 1).expect("metrics")
            ),
            "the small point size must not shorten the band"
        );

        // The grid **never** enters the small class: the only consumer is the dock's bottom row.
        frame.push(at(0, 1));
        assert_eq!(frame.glyphs()[0].size, SizeClass::Normal);
    }

    #[test]
    fn an_upload_button_fills_its_columns_on_the_context_row() {
        // The fill's edge is the button's column boundary — the
        // same columns as the mouse's hit range (`bt_core::transfer_button_at`),
        // at the context row's small step. The label's glyph is inside that range.
        let metrics = CellMetrics::new(10, 20, 8, GUTTER, 1).expect("metrics");
        let mut frame = Frame::default();
        frame.clear(metrics, CaretStyle::default());
        frame.push_dock(Cell {
            col: 3,
            row: 1,
            ch: Some('C'),
            ..Cell::default()
        });
        frame.open_dock(BG, CURSOR, CURSOR);
        assert_eq!(frame.dock_button_draws(0.0).count(), 0, "no button told");
        let button = DockButton {
            start: 2,
            end: 5,
            color: CURSOR,
            state: ButtonState::Hover,
        };
        frame.set_dock_buttons([None, Some(button)]);
        let draws: Vec<_> = frame.dock_button_draws(100.0).collect();
        assert_eq!(draws.len(), 2, "fill + border");
        let [fill, edge] = [draws[0], draws[1]];
        let glyph = frame.dock_glyphs()[0].pos;
        assert_eq!(
            fill.instance.pos,
            [glyph[0] - 8.0, glyph[1]],
            "left edge one column before the label"
        );
        assert_eq!(
            fill.instance.size,
            [24.0, 20.0],
            "three small columns, one row"
        );
        // The mouse's vertical range is the same band: the input block's top
        // (margin) + one input row + `context_row_offset`
        // (`BateriView::context_column`).
        assert_eq!(
            fill.instance.pos[1],
            (GUTTER as f32 + 20.0 + context_row_offset(1, metrics)).round(),
            "mouse and fill do not read the same row band"
        );
        assert_eq!(fill.instance.rgba[3], 0.34);
        assert_eq!(edge.instance.rgba[3], 0.7);
        assert_eq!(fill.shape[1], 0.0, "fill");
        assert_eq!(edge.shape[1], 1.0, "border at rule thickness");
        assert_eq!(fill.shape[0], frame.selection_radius());
        // The core is in window space: the quad is dock-local, the viewport is
        // `origin_y` lower.
        assert_eq!(fill.core[1], fill.instance.pos[1] + 100.0);
        assert_eq!(fill.core[2] - fill.core[0], 24.0);

        frame.set_dock_buttons([
            None,
            Some(DockButton {
                state: ButtonState::Idle,
                ..button
            }),
        ]);
        let idle: Vec<_> = frame.dock_button_draws(0.0).collect();
        assert!(
            idle[0].instance.rgba[3] < fill.instance.rgba[3],
            "darkens under the pointer"
        );
        // Opening clears them every frame.
        frame.open_dock(BG, CURSOR, CURSOR);
        assert_eq!(frame.dock_button_draws(0.0).count(), 0);
    }

    #[test]
    fn stripes_stay_out_of_the_cell_count_and_survive_motion_frames() {
        // The real contract: the stripe does **not** enter
        // `bg`. Had it entered uncounted, `move_cursor`'s trimming would erase
        // it in every motion frame and the stripe would flicker as the caret
        // glides; had it been counted, the `cells=` token would also count
        // something that is not a cell and the smoke gate's meaning would drift.
        let mut frame = Frame::default();
        frame.clear(
            CellMetrics::new(8, 16, 8, GUTTER, 1).expect("metrics"),
            CaretStyle::default(),
        );
        frame.push(bg_cell(0, 0));
        frame.push_block(block(0));
        push_settled(&mut frame, cursor(0, 0, true));

        assert_eq!(frame.bg_count(), 1, "stripe must not count as a cell");
        assert_eq!(frame.bg_instances().len(), 1, "caret leaked into bg");
        assert_eq!(frame.stripes().len(), 1);

        let stripes = frame.stripes().to_vec();
        for _ in 0..3 {
            move_cursor(
                &mut frame,
                cursor(5, 0, true),
                [4.5, 0.0],
                CURSOR,
                OPAQUE,
                true,
            );
            // The grid did not change, so the blocks' row ranges did not
            // change either: the stripe must stay as it was in a motion frame.
            assert_eq!(frame.stripes(), stripes, "motion frame moved the stripe");
            assert_eq!(frame.bg_count(), 1);
        }
    }

    /// A dock cell; the dock twin of the grid's [`bg_cell`].
    fn dock_cell(col: u16) -> Cell {
        Cell {
            col,
            row: 0,
            ch: Some('x'),
            fg: CURSOR,
            bg: Some(BG),
            ..Default::default()
        }
    }

    #[test]
    fn the_dock_keeps_its_own_lists_and_stays_out_of_the_counters() {
        // The dock's first contract: the dock lists do **not** enter `bg`.
        // Had they entered they would be tied to the grid's frame lifetime and
        // torn from their own viewport — the same rationale as the stripe
        // being a separate list, with a more visible symptom.
        // Not entering the counters is the second contract: `cells=8 glyphs=6
        // rules=15` is measured in the smoke run and its meaning must be
        // preserved bit for bit.
        let mut frame = Frame::default();
        frame.clear(grid(8, 16), CaretStyle::default());
        frame.push(bg_cell(0, 0));
        push_settled(&mut frame, cursor(0, 0, true));
        frame.push_dock(dock_cell(0));
        frame.push_dock(Cell {
            underline: UnderlineStyle::Single,
            ..dock_cell(1)
        });
        frame.open_dock(BG, CURSOR, CURSOR);

        assert_eq!(frame.bg_count(), 1, "dock cell was counted");
        assert_eq!(frame.glyph_count(), 0, "dock glyph was counted");
        assert_eq!(frame.rule_count(), 0, "dock rule was counted");
        // The caret is **no longer in** `bg`: it has its own slot and the
        // encode draws it separately. Only the cell itself stays in the
        // background list.
        assert_eq!(frame.bg_instances().len(), 1, "caret leaked into bg");
        assert!(frame.grid_caret().is_some(), "caret not in grid slot");

        let (dock_bg, dock_glyphs, dock_rules) = (
            frame.dock_bg().to_vec(),
            frame.dock_glyphs().to_vec(),
            frame.dock_rules().to_vec(),
        );
        assert_eq!(dock_bg.len(), 2, "caret leaked into the dock background");
        assert_eq!(dock_glyphs.len(), 2);
        assert_eq!(dock_rules.len(), 1);

        for _ in 0..3 {
            move_cursor(
                &mut frame,
                cursor(5, 0, true),
                [4.5, 0.0],
                CURSOR,
                OPAQUE,
                true,
            );
            assert_eq!(frame.dock_bg(), dock_bg, "motion frame moved the dock");
            assert_eq!(frame.dock_glyphs(), dock_glyphs);
            assert_eq!(frame.dock_rules(), dock_rules);
            assert!(frame.dock().is_some(), "motion frame closed the surface");
        }

        // `clear` empties **all of it**: a preserved surface would hang in the
        // window of a session with no dock.
        frame.clear(grid(8, 16), CaretStyle::default());
        assert!(frame.dock().is_none());
        assert!(frame.dock_bg().is_empty());
        assert!(frame.dock_glyphs().is_empty());
        assert!(frame.dock_rules().is_empty());
        assert!(frame.grid_caret().is_none(), "clear kept the caret");
        assert!(frame.dock_caret(0.0).is_none());
    }

    #[test]
    fn the_dock_never_reads_the_origin() {
        // The exemption is **structural**: the dock lists are born dock-local
        // and never see the offset; what carries them onto the screen is the
        // second `setViewport`. It could not have been built with arithmetic —
        // `clear` zeroes the offset and `set_origin_rows` is called after the
        // sink, i.e. while the dock cells are being pushed the value is not yet
        // known. This test pins that independence on the CPU side; the pixel's
        // witness is in `renderer.rs`.
        let mut frame = Frame::default();
        frame.clear(grid(8, 16), CaretStyle::default());
        frame.push_dock(dock_cell(1));
        let settled_bg = frame.dock_bg()[0];

        // The same frame, offset by two rows: the grid's cell slides, the
        // dock's does not.
        frame.clear(grid(8, 16), CaretStyle::default());
        frame.set_origin_rows(2.0);
        frame.push_dock(dock_cell(1));
        assert_eq!(frame.dock_bg()[0], settled_bg, "dock took the offset");
        // The grid's caret sees the offset **in the same frame**: the claim
        // that the two are in separate spaces is proved only when both are asked.
        push_cursor(&mut frame, cursor(0, 0, true), [0.0, 2.0], CURSOR, OPAQUE);
        assert_eq!(
            frame.cursor_block().rect[1],
            2.0 * 16.0,
            "grid's caret is not at the screen row"
        );
    }

    /// A cell of a filled row; the band twin of the grid's [`bg_cell`].
    fn fill_cell(col: u16, row: u16) -> Cell {
        Cell {
            col,
            row,
            ch: Some('x'),
            fg: CURSOR,
            bg: Some(BG),
            ..Default::default()
        }
    }

    #[test]
    fn the_fill_keeps_its_own_lists_and_stays_out_of_the_counters() {
        // The sibling of the dock guard and the same two contracts: the
        // fill lists do **not** enter the grid's — had they entered they would
        // be drawn from the grid's space, i.e. on top of the content instead
        // of in the band's place — and they do not enter the counters either:
        // `cells=8 glyphs=6 rules=15` is measured in the smoke run and its
        // meaning must be preserved bit for bit.
        let mut frame = Frame::default();
        frame.clear(grid(8, 16), CaretStyle::default());
        frame.push(bg_cell(0, 0));
        push_settled(&mut frame, cursor(0, 0, true));
        frame.set_fill_rows(2);
        frame.push_fill(fill_cell(0, 0));
        frame.push_fill(Cell {
            underline: UnderlineStyle::Single,
            ..fill_cell(1, 1)
        });

        assert_eq!(frame.bg_count(), 1, "fill cell was counted");
        assert_eq!(frame.glyph_count(), 0, "fill glyph was counted");
        assert_eq!(frame.rule_count(), 0, "fill rule was counted");
        assert_eq!(frame.bg_instances().len(), 1, "fill leaked into `bg`");

        let (fill_bg, fill_glyphs, fill_rules) = (
            frame.fill_bg().to_vec(),
            frame.fill_glyphs().to_vec(),
            frame.fill_rules().to_vec(),
        );
        assert_eq!(fill_bg.len(), 2);
        assert_eq!(fill_glyphs.len(), 2);
        assert_eq!(fill_rules.len(), 1);

        // A motion frame **keeps** the lists: the grid is not dirty, so the
        // band's cells are still valid too (the dock's precedent).
        for _ in 0..3 {
            move_cursor(
                &mut frame,
                cursor(5, 0, true),
                [4.5, 0.0],
                CURSOR,
                OPAQUE,
                true,
            );
            assert_eq!(frame.fill_bg(), fill_bg, "motion frame moved the band");
            assert_eq!(frame.fill_glyphs(), fill_glyphs);
            assert_eq!(frame.fill_rules(), fill_rules);
            assert_eq!(frame.fill_rows(), 2, "motion frame closed the band");
        }

        // `clear` empties **all of it**, the band's height included: a
        // preserved height would hang on screen in the first frame that turns
        // the fill off (Ctrl-L, a window with no dock) — and `fill_rows == 0`
        // is the very state in which the third viewport is not set up (the
        // rollback strip).
        frame.clear(grid(8, 16), CaretStyle::default());
        assert_eq!(frame.fill_rows(), 0, "clear did not release the band");
        assert!(frame.fill_bg().is_empty());
        assert!(frame.fill_glyphs().is_empty());
        assert!(frame.fill_rules().is_empty());
    }

    #[test]
    fn the_fill_band_rides_the_origin() {
        // **The CPU half.** The band stands above the offset and slides
        // **together with** the offset: the cells are born fill-local, what
        // carries them onto the screen is `origin_px − fill_px`, and that is
        // derived at read time. Had it been baked at push time the motion frame
        // — the lists are kept, only `origin_px` changes — would have frozen
        // the band in place. The pixel's witness is in `renderer.rs`.
        let mut frame = Frame::default();
        frame.clear(grid(8, 16), CaretStyle::default());
        frame.set_fill_rows(2);
        frame.push_fill(fill_cell(1, 0));
        let pushed = frame.fill_bg()[0];

        // With no offset the band's origin is negative: both rows overflow the
        // top of the window and Metal clips them (measured).
        assert_eq!(frame.fill_origin_px(), -32.0, "band did not slide");
        for rows in [3.0, 2.5, 1.0] {
            frame.set_origin_rows(rows);
            assert_eq!(
                frame.fill_origin_px(),
                rows * 16.0 - 32.0,
                "band's origin did not follow the offset"
            );
            // The cell itself does not move: what slides is the space itself.
            assert_eq!(frame.fill_bg()[0], pushed, "band took the offset");
        }
    }

    #[test]
    fn the_scroll_fraction_lowers_the_grid_by_whole_device_pixels() {
        // The scroll fraction draws the grid that much lower and,
        // like the pixel offset, lands on the **device grid** — a scroll
        // resting at half a pixel would blur all the text. The fraction is
        // rounded **separately** from the offset, because the caret is exempt
        // from the offset but not from the fraction and wants its own share on
        // its own
        // (`the_grid_caret_rides_the_fraction_and_the_dock_caret_does_not`).
        let mut frame = Frame::default();
        frame.clear(grid(9, 18), CaretStyle::default());
        frame.set_scroll_frac(0.3);
        frame.set_origin_rows(0.0);
        assert_eq!(frame.origin_px(), 5.0, "fraction was not rounded to pixels");
        frame.set_origin_rows(2.0);
        assert_eq!(frame.origin_px(), 41.0, "fraction not added to offset");

        // A motion frame does not call `clear`: the fraction is kept and is
        // added again every time the offset is written.
        frame.set_origin_rows(1.0);
        assert_eq!(frame.origin_px(), 23.0, "motion frame dropped the fraction");

        // `clear` releases the fraction: the content frame restates it every
        // frame, and a frame that does not say draws on a whole row.
        frame.clear(grid(9, 18), CaretStyle::default());
        frame.set_origin_rows(2.0);
        assert_eq!(frame.origin_px(), 36.0, "clear kept the fraction");

        // The order is free: even if the fraction is written after the offset, in the sum.
        frame.set_scroll_frac(0.3);
        assert_eq!(frame.origin_px(), 41.0, "fraction depends on the order");

        // Rounding does not reach a full cell: a grid whose offset has not
        // changed cannot be drawn a row lower.
        frame.set_scroll_frac(0.99);
        assert_eq!(
            frame.origin_px(),
            36.0 + 17.0,
            "fraction was rounded up to a cell"
        );
    }

    #[test]
    fn a_grid_caret_pushed_into_the_dock_band_does_not_invert_the_dock() {
        // The caret of the last row, shifted by the fraction, can enter the
        // dock band and there, together with its letter, sits under the dock's
        // ground. The inversion rectangle goes to both glyph encodes: had it
        // not been clipped, the dock's letters in that column would be drawn in
        // the ground colour, invisible.
        let mut frame = Frame::default();
        frame.clear(grid(8, 16), CaretStyle::default());
        frame.set_dock_top(64.0);
        frame.set_scroll_frac(0.5);
        frame.push_caret([0.0, 3.0], TEXT, CURSOR, OPAQUE, CaretShape::Block, true);
        assert!(
            frame.grid_caret().is_some(),
            "the fraction handed the caret over to the dock"
        );
        let rect = frame.cursor_block().rect;
        assert_eq!(rect[1], 56.0);
        assert_eq!(rect[3], 64.0, "inversion overflowed into the dock band");
    }

    #[test]
    fn the_top_row_sits_above_the_band_and_rides_with_it() {
        // The row that closes the strip the fraction opens is at the
        // very top of the fill channel (fill-local `0`), with the band's rows
        // below it. The channel's height is `top_row + fill` and the band's
        // origin is derived from it, so the top row slides **together with**
        // the band and the grid. It does not enter the counters either:
        // `cells=`/`glyphs=`/`rules=` are the grid's witnesses.
        let mut frame = Frame::default();
        frame.clear(grid(8, 16), CaretStyle::default());
        frame.set_scroll_frac(0.25);
        // One top row, two band rows.
        frame.set_fill_rows(3);
        frame.push_fill(fill_cell(0, 0));
        frame.push_fill(fill_cell(0, 1));
        frame.push_fill(fill_cell(0, 2));
        frame.push_fill_block(block(0));

        assert_eq!(frame.bg_count(), 0, "top row was counted as a cell");
        assert_eq!(frame.glyph_count(), 0, "top row was counted as a glyph");
        assert_eq!(frame.rule_count(), 0, "top row was counted as a rule");

        for rows in [2.0, 1.5, 0.0] {
            frame.set_origin_rows(rows);
            let origin = frame.origin_px();
            assert_eq!(origin, (rows * 16.0).round() + 4.0);
            let band = frame.fill_origin_px();
            assert_eq!(band, origin - 48.0, "channel did not count the top row");
            // The places on screen: the top row is three rows above the grid,
            // the band's last row one row above — the gap between is zero.
            let top = band + frame.fill_bg()[0].pos[1];
            let last = band + frame.fill_bg()[2].pos[1];
            assert_eq!(top, origin - 48.0);
            assert_eq!(last + 16.0, origin, "band does not touch the grid");
        }
        // The top row's mark is also in the band's list, not in the grid's.
        assert_eq!(frame.fill_rules().len(), 1);
        assert!(
            frame.stripes().is_empty(),
            "top row's mark fell onto the grid"
        );
    }

    #[test]
    fn the_grid_caret_rides_the_fraction_and_the_dock_caret_does_not() {
        // **The fraction shifts the grid's whole world, the caret included.**
        // The caret is exempt from the offset (its target is a screen row) but
        // not from the fraction: had the caret stayed in place while the grid is
        // drawn that much lower, the block would cover part of its letter and a
        // strip of the row above, and the inversion would flip the wrong
        // pixels — the caret of `sleep 10` and half a row up on a trackpad.
        // The dock caret, however, is exempt from scrolling: the dock is a
        // separate surface.
        let mut frame = Frame::default();
        frame.clear(grid(8, 16), CaretStyle::default());
        frame.set_dock_top(64.0);
        frame.set_scroll_frac(0.5);
        frame.set_origin_rows(0.0);
        frame.push(bg_cell(0, 2));
        frame.push_caret([0.0, 2.0], TEXT, CURSOR, OPAQUE, CaretShape::Block, true);
        // The cell's place on screen: the position in the list + the viewport's origin.
        let cell_on_screen = frame.bg_instances()[0].pos[1] + frame.origin_px();
        assert_eq!(cell_on_screen, 40.0);
        assert_eq!(
            frame.grid_caret().expect("caret").pos[1] + frame.origin_px(),
            cell_on_screen,
            "caret detached from its letter"
        );
        assert_eq!(
            frame.cursor_block().rect[1],
            cell_on_screen,
            "inversion is not on top of the letter"
        );
        assert_eq!(frame.caret_core()[1], cell_on_screen);

        // The motion frame applies the same scroll too: the fraction stays in `Frame`.
        frame.move_caret([0.0, 2.0], TEXT, CURSOR, OPAQUE, true);
        assert_eq!(frame.cursor_block().rect[1], cell_on_screen);

        // Slot selection looks at the **unshifted** position: the fraction is
        // not a handover. The caret in the dock band does not budge.
        frame.move_caret([0.0, 4.0], TEXT, CURSOR, OPAQUE, true);
        let caret = frame.dock_caret(64.0).expect("caret not in dock slot");
        assert_eq!(caret.pos[1], 0.0, "dock caret shifted with the fraction");
        assert_eq!(frame.cursor_block().rect[1], 64.0);
    }

    /// The dock's selection in the grid's shape: a single row, all four
    /// corners rounded, a dock-local position (left margin + breathing room)
    /// and it does **not** enter the grid's list — the dock is exempt from
    /// the offset and is drawn in its own viewport.
    #[test]
    fn a_dock_selection_is_one_rounded_run_in_dock_space() {
        let mut frame = Frame::default();
        let metrics = CellMetrics::new(9, 18, 9, GUTTER, 1).expect("non-zero cell");
        frame.clear(metrics, CaretStyle::default());
        frame.push_selection(&[], BG);
        frame.push_dock_selection(&[SelectionRun {
            row: 0,
            first: 3,
            last: 5,
        }]);
        assert!(frame.selection_instances().is_empty(), "leaked to grid");
        let [run] = frame.dock_selection_instances() else {
            panic!(
                "a single quad expected: {:?}",
                frame.dock_selection_instances()
            );
        };
        let pad = f32::from(GUTTER);
        assert_eq!(run.pos, [pad + 3.0 * 9.0, pad]);
        assert_eq!(run.size, [27.0, 18.0]);
        assert_eq!(run.rgba, [1.0; 4], "corners are not rounded");
        // The colour is the same uniform as the grid's.
        assert_eq!(frame.selection_rgba(), BG.to_array());
        // The content frame empties the list.
        frame.clear(metrics, CaretStyle::default());
        assert!(frame.dock_selection_instances().is_empty());
    }

    fn search_run(row: u16, first: u16, last: u16, current: bool, continues: bool) -> SearchRun {
        SearchRun {
            row,
            first,
            last,
            current,
            continues,
        }
    }

    /// A frame ready for the search drawing: 9×18 cells, the colours two distinct tones.
    fn search_frame() -> Frame {
        let mut frame = Frame::default();
        let metrics = CellMetrics::new(9, 18, 9, GUTTER, 1).expect("non-zero cell");
        frame.clear(metrics, CaretStyle::default());
        frame
    }

    /// The search's corners are **per match**. Two separate matches on
    /// consecutive rows are two separate shapes — all four corners rounded, no
    /// concave fill —; when the same two runs are a single match's wrapping
    /// they fuse into one shape and the steps are closed with fills.
    #[test]
    fn search_corners_are_per_match() {
        let mut frame = search_frame();
        let red = LinearRgba::from_srgb(0xff, 0, 0);
        let green = LinearRgba::from_srgb(0, 0xff, 0);
        frame.push_search(
            &[
                search_run(0, 2, 4, false, false),
                search_run(1, 0, 6, false, false),
            ],
            red,
            green,
        );
        let separate = frame.search_match_instances();
        assert_eq!(separate.len(), 2, "a concave fill was born: {separate:?}");
        assert!(
            separate.iter().all(|part| part.rgba == [1.0; 4]),
            "two separate matches fused: {separate:?}"
        );
        assert_eq!(frame.search_match_rgba(), red.to_array());
        assert_eq!(frame.search_current_rgba(), green.to_array());

        let mut frame = search_frame();
        frame.push_search(
            &[
                search_run(0, 2, 4, false, false),
                search_run(1, 0, 6, false, true),
            ],
            red,
            green,
        );
        let wrapped = frame.search_match_instances();
        // The upper run's two bottom corners are concave: two fill pieces.
        assert_eq!(wrapped.len(), 4, "wrapped match did not fuse: {wrapped:?}");
        assert_eq!(
            wrapped[0].rgba,
            [1.0, 1.0, 0.0, 0.0],
            "the upper run's corners"
        );
        // The lower run overshoots the upper: its corners are outside the step, i.e. exposed.
        assert_eq!(wrapped[3].rgba, [1.0; 4], "the lower run's corners");
    }

    /// The next match can start on a higher row (the second on row 5 after a
    /// match wrapping onto 5–6): the list is not sorted, it is split by
    /// `continues`. The current match goes to its own list and both lists are
    /// emptied in the content frame.
    #[test]
    fn search_runs_split_by_match_and_role() {
        let mut frame = search_frame();
        let color = LinearRgba::from_srgb(0x80, 0x80, 0x80);
        frame.push_search(
            &[
                search_run(5, 3, 6, false, false),
                search_run(6, 0, 1, false, true),
                search_run(5, 8, 9, true, false),
            ],
            color,
            color,
        );
        let [upper, lower] = frame.search_match_instances() else {
            panic!("two pieces expected: {:?}", frame.search_match_instances());
        };
        // The wrapped match's two runs touch diagonally: they fuse but do not
        // overlap, all four corners exposed.
        assert_eq!((upper.rgba, lower.rgba), ([1.0; 4], [1.0; 4]));
        let [current] = frame.search_current_instances() else {
            panic!("one current piece: {:?}", frame.search_current_instances());
        };
        assert_eq!(current.pos, frame.pos(8, 5));
        assert_eq!(current.size, [18.0, 18.0]);
        assert_eq!(current.rgba, [1.0; 4]);
        assert!(frame.selection_instances().is_empty(), "selection leak");
        assert!(
            frame.fill_search_match_instances().is_empty(),
            "leaked into the band"
        );

        let metrics = CellMetrics::new(9, 18, 9, GUTTER, 1).expect("non-zero cell");
        frame.clear(metrics, CaretStyle::default());
        assert!(frame.search_match_instances().is_empty());
        assert!(frame.search_current_instances().is_empty());
    }

    /// The band's runs go to the band's lists, with a fill-local row; the
    /// grid's lists stay empty.
    #[test]
    fn fill_search_runs_stay_in_the_band() {
        let mut frame = search_frame();
        let color = LinearRgba::from_srgb(0x80, 0x80, 0x80);
        frame.set_fill_rows(2);
        frame.push_search(&[], color, color);
        frame.push_fill_search(&[
            search_run(0, 1, 2, false, false),
            search_run(1, 0, 0, true, false),
        ]);
        assert!(frame.search_match_instances().is_empty(), "leaked to grid");
        assert!(
            frame.search_current_instances().is_empty(),
            "leaked into the grid"
        );
        let [matched] = frame.fill_search_match_instances() else {
            panic!("{:?}", frame.fill_search_match_instances());
        };
        assert_eq!(matched.pos, frame.pos(1, 0));
        let [current] = frame.fill_search_current_instances() else {
            panic!("{:?}", frame.fill_search_current_instances());
        };
        assert_eq!(current.pos, frame.pos(0, 1));
        let metrics = CellMetrics::new(9, 18, 9, GUTTER, 1).expect("non-zero cell");
        frame.clear(metrics, CaretStyle::default());
        assert!(frame.fill_search_match_instances().is_empty());
        assert!(frame.fill_search_current_instances().is_empty());
    }

    /// On a wrapped input the selection is one run per row: the second
    /// run is one cell lower — the input rows are adjacent, with no gap
    /// between them — and the corner decision looks at the neighbouring row
    /// like the grid's.
    #[test]
    fn a_dock_selection_across_rows_stacks_its_runs() {
        let mut frame = Frame::default();
        let metrics = CellMetrics::new(9, 18, 9, GUTTER, 1).expect("non-zero cell");
        frame.clear(metrics, CaretStyle::default());
        frame.set_dock_rows(3);
        frame.push_dock_selection(&[
            SelectionRun {
                row: 0,
                first: 5,
                last: 9,
            },
            SelectionRun {
                row: 1,
                first: 2,
                last: 7,
            },
        ]);
        let runs: Vec<&Instance> = frame
            .dock_selection_instances()
            .iter()
            .filter(|part| part.size[1] == 18.0)
            .collect();
        let [top, bottom] = runs[..] else {
            panic!("two runs expected: {runs:?}");
        };
        assert_eq!(bottom.pos[1] - top.pos[1], 18.0, "rows are not adjacent");
        // A single-piece shape: the corners on the two runs' overlapping edge
        // are square (a concave step), the exposed ones are rounded.
        assert_eq!(top.rgba, [1.0, 1.0, 1.0, 0.0], "upper run");
        assert_eq!(bottom.rgba, [1.0, 0.0, 1.0, 1.0], "lower run");
    }

    #[test]
    fn the_dock_ground_spans_the_given_width() {
        // The width is an argument, because `Frame` does not know the width
        // of the touch: the lists are born from the cell grid, whereas the
        // surface must cover the **whole** window. The ground must be opaque —
        // during the slide the grid's overflowing bottom row stays under it.
        let mut frame = Frame::default();
        frame.clear(grid(9, 18), CaretStyle::default());
        // The top line's colour is a separate field: the two lines are
        // opened with two separate colours so that if one took the other's
        // colour it would show.
        frame.open_dock(BG, SUCCESS, CURSOR);
        assert_eq!(frame.dock_layout_px(), 36.0, "rows not converted");

        let [ground, separator, _, divider] = frame.dock_ground(500.0);
        assert_eq!(ground.pos, [0.0, 0.0], "ground not from margin");
        assert_eq!(ground.size, [500.0, 36.0]);
        assert_eq!(ground.rgba, BG.to_array());
        assert_eq!(ground.rgba[3], 1.0, "translucent ground shows overflow");
        // The separator is at the dock's **topmost** pixel: that is the boundary with the grid.
        assert_eq!(separator.pos, [0.0, 0.0]);
        assert_eq!(separator.size, [500.0, SEPARATOR_PX]);
        assert_eq!(
            separator.rgba,
            SUCCESS.to_array(),
            "top line is the edge's colour"
        );
        // The second separator is **between** the two rows and in the
        // separator's colour — it does not take the remote session's edge
        // colour. Without a margin at this measure the row gap is zero, so the
        // line is exactly on the row boundary.
        assert_eq!(divider.pos, [0.0, 18.0]);
        assert_eq!(divider.size, [500.0, SEPARATOR_PX]);
        assert_eq!(divider.rgba, CURSOR.to_array());

        // While an upload runs the top line is a bar: its ground
        // in the empty track's colour, the part filling from the left
        // in the edge's colour; the second separator keeps its own colour.
        frame.set_dock_progress(Some(2_500), BG);
        let [_, base, fill, divider] = frame.dock_ground(500.0);
        assert_eq!(base.size, [500.0, SEPARATOR_PX]);
        assert_eq!(base.rgba, BG.to_array(), "bar's ground is the empty track");
        assert_eq!(divider.rgba, CURSOR.to_array(), "separator has no track");
        assert_eq!(fill.pos, [0.0, 0.0]);
        assert_eq!(fill.size, [125.0, SEPARATOR_PX]);
        assert_eq!(fill.rgba, SUCCESS.to_array(), "fill is the edge colour");
        // Opening resets every frame: the bar exists only in the frame it is told.
        frame.open_dock(BG, SUCCESS, CURSOR);
        let [_, base, fill, _] = frame.dock_ground(500.0);
        assert_eq!(base.rgba, SUCCESS.to_array());
        assert_eq!(fill.size[0], 0.0);

        // A frame without a dock gives no height: the second viewport is not set up.
        frame.clear(grid(9, 18), CaretStyle::default());
        assert_eq!(frame.dock_layout_px(), 0.0);
    }

    #[test]
    fn the_dock_breathes_above_and_below_its_rows() {
        // **Breathing room** (the user: "there is literally no
        // padding top"). There is margin above and below the two rows, and its
        // source is the left margin itself — no second design constant was
        // made up.
        let mut frame = Frame::default();
        frame.clear(
            CellMetrics::new(9, 18, 9, GUTTER, 1).expect("metrics"),
            CaretStyle::default(),
        );
        frame.open_dock(BG, CURSOR, CURSOR);
        // 2×18 + 2×GUTTER + 1×(2×GUTTER) = 36 + 14 + 14 = 64. The row gap is
        // **twice** the outer margin, because a line passes through its middle:
        // with one margin on each side of the line all four gaps come out equal.
        assert_eq!(frame.dock_layout_px(), 64.0, "margin not in height");

        // **The ground covers the margins too**: a rectangle short by the
        // margin would, during the slide, show the overflowing grid row exactly
        // in the breathing room.
        let [ground, separator, _, divider] = frame.dock_ground(500.0);
        assert_eq!(ground.size, [500.0, 64.0]);
        // The line between rows is in the **middle** of the gap: margin 7,
        // cell 18, gap 14 → 7 + 18 + (14 − 1)/2 = 31.5 → 32. Placed at an edge
        // it would stick to a row and look as if it belonged to it.
        assert_eq!(divider.pos, [0.0, 32.0]);
        // **The rhythm is equal — written pixel by pixel.** `pad` 7, cell 18,
        // gap 14, line 1 px. The boxes: top line [0,1], input [7,25], middle
        // line [32,33], context [39,57], bottom 64. The four gaps in between
        // are 6, 7, 6, 7 in order — the difference comes from the lines'
        // **own** pixels and from rounding 31.5, not from the formula. One
        // pixel cannot be equalized: the gap is even (14), the line odd (1), so
        // the centre always falls on a half pixel. In the old state these gaps
        // were 6, 1, 1, 7.
        assert_eq!(GUTTER, 7, "the pixel table above depends on this margin");
        let row1_top = f32::from(GUTTER) + 18.0 + dock_row_gap(f32::from(GUTTER));
        assert_eq!(row1_top, 39.0);
        assert_eq!(divider.pos[1] - (f32::from(GUTTER) + 18.0), 7.0);
        assert_eq!(row1_top - (divider.pos[1] + SEPARATOR_PX), 6.0);
        assert_eq!(frame.dock_layout_px() - (row1_top + 18.0), 7.0);
        assert_eq!(divider.size, [500.0, SEPARATOR_PX]);
        // The hairline is **above** the margin, at the top of the viewport:
        // that is the boundary with the grid, and putting the margin above it
        // would push the line into the grid.
        assert_eq!(separator.pos, [0.0, 0.0]);

        // The content starts below the margin: the first row is at y = margin.
        frame.clear(
            CellMetrics::new(9, 18, 9, GUTTER, 1).expect("metrics"),
            CaretStyle::default(),
        );
        frame.push_dock(Cell {
            col: 0,
            row: 0,
            ch: Some('x'),
            ..Cell::default()
        });
        frame.open_dock(BG, CURSOR, CURSOR);
        assert_eq!(
            frame.dock_glyphs()[0].pos[1],
            f32::from(GUTTER),
            "content did not go down to the margin"
        );
        // The second row is one cell lower, i.e. the margin is applied **once**.
        frame.clear(
            CellMetrics::new(9, 18, 9, GUTTER, 1).expect("metrics"),
            CaretStyle::default(),
        );
        frame.push_dock(Cell {
            col: 0,
            row: 1,
            ch: Some('x'),
            ..Cell::default()
        });
        // The second row is one cell **plus the row gap** lower; the outer
        // margin is not applied to it once more.
        assert_eq!(
            frame.dock_glyphs()[0].pos[1],
            f32::from(GUTTER) + 18.0 + 2.0 * f32::from(GUTTER),
            "row gap or outer margin applied wrongly"
        );
    }

    #[test]
    fn a_remote_dock_is_only_its_context_row() {
        // **Zero input rows**: the layout is only the context
        // row — small face, small step, no row gap above it (there is no input
        // row to separate) and the band is one row plus two outer margins. The
        // top line is at the top of the band and in the edge's colour; the
        // second separator has zero height.
        let metrics = CellMetrics::new(9, 18, 9, GUTTER, 1).expect("metrics");
        let mut frame = Frame::default();
        frame.clear(metrics, CaretStyle::default());
        frame.set_dock_input_rows(Some(0));
        for col in 0..2 {
            frame.push_dock(Cell {
                col,
                row: 0,
                ch: Some('x'),
                ..Cell::default()
            });
        }
        let edge = bt_core::Theme::BATERI.info_linear();
        frame.open_dock(BG, edge, CURSOR);
        let band = 18.0 + 2.0 * f32::from(GUTTER);
        assert_eq!(frame.dock_layout_px(), band);
        assert_eq!(band_px(Some(0), metrics), band);
        let glyphs = frame.dock_glyphs();
        assert_eq!(glyphs[0].pos[1], f32::from(GUTTER), "gap was applied");
        assert_eq!(
            glyphs[0].size,
            SizeClass::Small,
            "context row was drawn large"
        );
        assert_eq!(
            glyphs[1].pos[0] - glyphs[0].pos[0],
            f32::from(metrics.context_cell_px()),
            "context row's step is the small face's advance"
        );
        let [ground, separator, _, divider] = frame.dock_ground(500.0);
        assert_eq!(ground.size, [500.0, band]);
        assert_eq!(separator.pos, [0.0, 0.0]);
        assert_eq!(separator.rgba, edge.to_array());
        assert_eq!(divider.size[1], 0.0, "there are not two rows to separate");
        // The tests' one-row dock without context stays under its old rule.
        frame.clear(metrics, CaretStyle::default());
        frame.set_dock_rows(1);
        frame.push_dock(Cell {
            col: 0,
            row: 0,
            ch: Some('x'),
            ..Cell::default()
        });
        assert_eq!(frame.dock_glyphs()[0].size, SizeClass::Normal);
    }

    #[test]
    fn no_band_opens_a_surface_of_zero_rows() {
        // **No band** (a program reading the keyboard itself): the layout has
        // no row, the settled band has no height, so neither the ground nor
        // either hairline covers a pixel of the window; the surface is still
        // open and the mouse reads a zero-row input block, so a click on the
        // grid's bottom row is the grid's.
        let metrics = CellMetrics::new(9, 18, 9, GUTTER, 1).expect("metrics");
        assert_eq!(band_px(None, metrics), 0.0);
        let mut frame = Frame::default();
        frame.clear(metrics, CaretStyle::default());
        frame.set_dock_input_rows(None);
        let share = dock_px(DOCK_ROWS, metrics);
        // Settled: the excess is the whole share, negative.
        frame.set_dock_band(600.0, -share / 18.0);
        frame.open_dock(BG, CURSOR, CURSOR);
        assert_eq!(frame.dock_layout_px(), 0.0);
        assert_eq!(frame.dock_band_px(), 0.0);
        let [ground, separator, fill, divider] = frame.dock_ground(500.0);
        assert_eq!(ground.size, [500.0, 0.0]);
        // The top hairline sits on the band's top, i.e. at the window's bottom:
        // the band's viewport starts there, so it is below every pixel.
        assert_eq!(separator.pos, [0.0, 0.0]);
        assert_eq!(fill.size[0], 0.0, "no progress");
        assert_eq!(divider.size[1], 0.0);
        assert_eq!(frame.dock_hit(), Some((600.0 + f32::from(GUTTER), 0)));
        // The grid's origin is lower by the whole share.
        frame.set_origin_rows(5.0);
        assert_eq!(frame.origin_px(), 5.0 * 18.0 + share);
        // A spring overshooting past zero does not draw a negative band, and
        // the grid stops with it: its bottom does not leave the window.
        frame.set_dock_band(600.0, -share / 18.0 - 0.5);
        assert_eq!(frame.dock_band_px(), 0.0);
        assert_eq!(frame.dock_top_px, 600.0);
        assert_eq!(frame.origin_px(), 5.0 * 18.0 + share);
    }

    /// A cell of a dock with three input rows: `row` 0..3 input, 3 context.
    fn dock_row(row: u16) -> Cell {
        Cell {
            col: 0,
            row,
            ch: Some('x'),
            ..Cell::default()
        }
    }

    #[test]
    fn a_three_row_band_stacks_its_input_above_the_context_row() {
        // **The multi-row test hook** (`n = 3`): the band is `n` input rows
        // plus the context row; the input rows are adjacent, the gap and the
        // second hairline only between the input block and the context row.
        // At @1x, 9×18 cells, margin 7: `4·18 + 2·7 + 14 = 100` px.
        let metrics = CellMetrics::new(9, 18, 9, GUTTER, 1).expect("metrics");
        assert_eq!(band_px(Some(3), metrics), 100.0);
        // With one row it is the PTY share itself — the screen is bit for bit the same.
        assert_eq!(band_px(Some(1), metrics), dock_px(DOCK_ROWS, metrics));

        let mut frame = Frame::default();
        frame.clear(metrics, CaretStyle::default());
        frame.set_dock_rows(4);
        for row in 0..4 {
            frame.push_dock(dock_row(row));
        }
        frame.set_dock_band(600.0, 2.0);
        frame.open_dock(BG, CURSOR, CURSOR);
        assert_eq!(frame.dock_layout_px(), 100.0);
        assert_eq!(
            frame.dock_band_px(),
            100.0,
            "the resting band diverged from the layout"
        );

        // The input rows are adjacent from below the margin; the context row
        // is below the gap and only it is in the small class.
        let ys: Vec<f32> = frame.dock_glyphs().iter().map(|g| g.pos[1]).collect();
        assert_eq!(ys, [7.0, 25.0, 43.0, 75.0]);
        let small: Vec<bool> = frame
            .dock_glyphs()
            .iter()
            .map(|g| g.size == SizeClass::Small)
            .collect();
        assert_eq!(small, [false, false, false, true]);
        // The context row is at the **bottom** of the band: only the outer margin below it.
        assert_eq!(frame.dock_layout_px() - (ys[3] + 18.0), 7.0);

        // A single hairline below the input block, in the middle of the gap:
        // 7 + 3·18 + (14 − 1)/2 = 67.5 → 68. No line between the input rows.
        let [ground, separator, _, divider] = frame.dock_ground(500.0);
        assert_eq!(ground.size, [500.0, 100.0]);
        assert_eq!(separator.pos, [0.0, 0.0]);
        assert_eq!(divider.pos, [0.0, 68.0]);
        assert!(divider.pos[1] > ys[2] + 18.0 && divider.pos[1] < ys[3]);

        // The geometry that goes to the mouse: the input block's top
        // (600 − 100 + 7) and three rows.
        assert_eq!(frame.dock_hit(), Some((507.0, 3)));
    }

    #[test]
    fn a_growing_band_keeps_its_rows_and_divider_on_the_bottom() {
        // **Bottom-anchored**: while the band is halfway (excess
        // targeting 2, now 0.5) the ground and the top hairline are at the
        // animation's height, the cells and the second hairline at the
        // layout's — the text stays in place, only the band's top rises. The
        // grid's origin is higher by the band's excess.
        let metrics = CellMetrics::new(9, 18, 9, GUTTER, 1).expect("metrics");
        let mut frame = Frame::default();
        frame.clear(metrics, CaretStyle::default());
        frame.set_dock_rows(4);
        frame.set_origin_rows(5.0);
        frame.set_dock_band(600.0, 0.5);
        frame.open_dock(BG, CURSOR, CURSOR);
        // The PTY share 64 + the rounded pixels of half a row, 9.
        assert_eq!(frame.dock_band_px(), 73.0);
        assert_eq!(frame.origin_px(), 5.0 * 18.0 - 9.0);
        let [ground, _, _, divider] = frame.dock_ground(500.0);
        assert_eq!(ground.size, [500.0, 73.0]);
        // In window space the line is at the same pixel as in the layout: the
        // band's viewport is at `600 − 73`, the layout's at `600 − 100`.
        assert_eq!(600.0 - 73.0 + divider.pos[1], 600.0 - 100.0 + 68.0);
    }

    #[test]
    fn the_caret_picks_its_slot_from_the_dock_band() {
        // **One caret, two slots.** The block must be drawn after the ground
        // of the surface it will stand on but before its glyphs; had it stayed
        // in the grid, the dock's opaque ground would cover it, and had it
        // stayed in the dock it would paint the grid's letter. The criterion
        // is **overlap**, not the centre: so that no half-clipped block is seen
        // in the handover frames, a caret that touches the band rounds down,
        // not up.
        let mut frame = Frame::default();
        frame.clear(grid(8, 16), CaretStyle::default());
        frame.set_dock_top(64.0);

        // Fully above the band: the grid's slot.
        frame.push_caret([3.0, 2.0], TEXT, CURSOR, OPAQUE, CaretShape::Block, true);
        assert!(
            frame.grid_caret().is_some(),
            "caret did not fall into the grid slot"
        );
        assert!(frame.dock_caret(64.0).is_none(), "caret is in both slots");

        // The moment it **touches** the band the dock's slot — even if half of it is still on the grid.
        frame.move_caret([3.0, 3.5], TEXT, CURSOR, OPAQUE, true);
        assert!(frame.grid_caret().is_none(), "old slot was not cleared");
        let caret = frame.dock_caret(64.0).expect("caret not in dock slot");
        // The instance is born in window space (y = 3.5 × 16 = 56) and the dock
        // viewport starts at 64: the difference is **negative**, i.e. the caret
        // is drawn above the band. The frame in the middle of the handover is
        // exactly this.
        assert_eq!(caret.pos[1], -8.0, "dock-local translation is wrong");

        // **The `f32` error of a fractional band excess is not a handover**
        // even if the last row's bottom edge exceeds the band's top by
        // an epsilon, it stays on the grid.
        frame.move_caret([3.0, 3.000_001], TEXT, CURSOR, OPAQUE, true);
        assert!(frame.grid_caret().is_some(), "epsilon moved caret");

        // `clear` zeroes the band too: in the next frame with no dock the
        // caret must again fall into the grid's slot.
        frame.clear(grid(8, 16), CaretStyle::default());
        frame.push_caret([3.0, 40.0], TEXT, CURSOR, OPAQUE, CaretShape::Block, true);
        assert!(
            frame.grid_caret().is_some(),
            "caret fell into the dock slot in a frame without a dock"
        );
    }

    #[test]
    fn caret_shapes_narrow_both_rectangles() {
        // **The two narrow together** and this is a requirement: had the
        // painted quad and the inversion rectangle diverged, the letter under
        // a thin bar would be inverted across the whole cell — which is exactly
        // what would have happened on the first write.
        //
        // The thickness is from `CellMetrics::rule_px`; here 2.
        let mut frame = Frame::default();
        let metrics = CellMetrics::new(10, 20, 10, 0, 2).expect("metrics");

        frame.clear(metrics, CaretStyle::default());
        frame.push_caret([1.0, 1.0], TEXT, CURSOR, OPAQUE, CaretShape::Block, true);
        let block = frame.grid_caret().expect("no caret");
        assert_eq!((block.pos, block.size), ([10.0, 20.0], [10.0, 20.0]));
        assert_eq!(frame.cursor_block().rect, [10.0, 20.0, 20.0, 40.0]);

        // The underline is at the **bottom** of the cell: 20 + 20 − 2.
        frame.clear(metrics, CaretStyle::default());
        frame.push_caret(
            [1.0, 1.0],
            TEXT,
            CURSOR,
            OPAQUE,
            CaretShape::Underline,
            true,
        );
        let under = frame.grid_caret().expect("no caret");
        assert_eq!((under.pos, under.size), ([10.0, 38.0], [10.0, 2.0]));
        assert_eq!(frame.cursor_block().rect, [10.0, 38.0, 20.0, 40.0]);

        // The vertical bar is at the left of the cell and full height.
        frame.clear(metrics, CaretStyle::default());
        frame.push_caret([1.0, 1.0], TEXT, CURSOR, OPAQUE, CaretShape::Beam, true);
        let beam = frame.grid_caret().expect("no caret");
        assert_eq!((beam.pos, beam.size), ([10.0, 20.0], [2.0, 20.0]));
        assert_eq!(frame.cursor_block().rect, [10.0, 20.0, 12.0, 40.0]);
    }

    #[test]
    fn caret_geometry_survives_a_zero_cell() {
        // `Frame::default()`'s cell is `(0.0, 0.0)` and the old
        // `rule.clamp(1.0, 0.0)` **panicked** because `min > max`.
        // In debug `push_caret`'s own
        // `debug_assert` fires first, but `f32::clamp`'s assert exists **in
        // release too** — i.e. in a release build it would kill a window. The
        // geometry is tested directly, because the way to reach `push_caret`
        // in that state is closed in debug.
        let (pos, size) = caret_painted_rect([0.0, 0.0], (0.0, 0.0), CaretShape::Beam, 1.0);
        assert_eq!((pos, size), ([0.0, 0.0], [0.0, 0.0]));
        let (_, size) = caret_painted_rect([0.0, 0.0], (0.0, 0.0), CaretShape::Underline, 1.0);
        assert_eq!(size, [0.0, 0.0]);
    }

    #[test]
    fn the_caret_is_at_least_one_pixel_thick() {
        // Even if the rule metric comes out zero the thin caret stays visible.
        let mut frame = Frame::default();
        frame.clear(
            CellMetrics::new(10, 20, 10, 0, 0).expect("metrics"),
            CaretStyle::default(),
        );
        frame.push_caret([0.0, 0.0], TEXT, CURSOR, OPAQUE, CaretShape::Beam, true);
        assert_eq!(frame.grid_caret().expect("caret").size, [1.0, 20.0]);
    }

    #[test]
    fn a_motion_frame_keeps_the_caret_shape() {
        // A motion frame never goes to `bt-core`, so it does not know the
        // shape. Had the field not been in `Frame`, a beam would turn back
        // into a block on the first blink-off — blink goes
        // through exactly this path.
        let mut frame = Frame::default();
        frame.clear(
            CellMetrics::new(10, 20, 10, 0, 2).expect("metrics"),
            CaretStyle::default(),
        );
        frame.push_caret([1.0, 1.0], TEXT, CURSOR, OPAQUE, CaretShape::Beam, true);
        frame.move_caret([2.0, 1.0], TEXT, CURSOR, OPAQUE, true);
        let moved = frame.grid_caret().expect("no caret");
        assert_eq!(moved.size, [2.0, 20.0], "motion frame swallowed the shape");
    }

    #[test]
    fn an_underline_caret_still_moves_to_the_dock_slot() {
        // **The place of the narrowing.** Slot selection looks at the cell's
        // **footprint**; had the narrowing been done before it, the underline
        // caret (2 pixels high) would stay above the band and fall into the
        // grid slot — the dock's opaque ground would cover it.
        let mut frame = Frame::default();
        frame.clear(
            CellMetrics::new(8, 16, 8, 0, 1).expect("metrics"),
            CaretStyle::default(),
        );
        frame.set_dock_top(64.0);
        frame.push_caret(
            [3.0, 3.5],
            TEXT,
            CURSOR,
            OPAQUE,
            CaretShape::Underline,
            true,
        );
        assert!(
            frame.grid_caret().is_none(),
            "underline caret stayed in the grid slot"
        );
        assert!(
            frame.dock_caret(64.0).is_some(),
            "underline caret did not move to the dock slot"
        );
    }

    #[test]
    fn inkless_cell_yields_background_without_glyph() {
        // This is the line that separates `cells=K` from `glyphs=G`:
        // `" bateri "` is eight cells with a background but six glyphs. Had the
        // two been read from a single counter the smoke gate would never have
        // asked one of them.
        let mut frame = Frame::default();
        frame.clear(grid(8, 16), CaretStyle::default());
        frame.push(bg_cell(0, 0)); // inkless
        frame.push(Cell {
            col: 1,
            row: 0,
            ch: Some('b'),
            fg: CURSOR,
            bg: Some(BG),
            ..Default::default()
        });
        // A cell with no background but with ink: only into the glyph list.
        frame.push(Cell {
            col: 2,
            row: 0,
            ch: Some('a'),
            fg: CURSOR,
            bg: None,
            ..Default::default()
        });

        assert_eq!(frame.bg_count(), 2);
        assert_eq!(frame.glyph_count(), 2);
        assert_eq!(
            frame.glyphs()[1],
            GlyphCell {
                pos: [16.0, 0.0],
                ch: 'a',
                face: Face::Regular,
                size: SizeClass::Normal,
                rgba: CURSOR.to_array(),
                wide: false,
                cluster: None,
            }
        );

        frame.clear(grid(8, 16), CaretStyle::default());
        assert_eq!(frame.glyph_count(), 0);
    }

    #[test]
    fn cell_yields_up_to_two_rules() {
        // Five claims, all five can silently break: a cell with no rule
        // produces no rule, an underlined **inkless** cell does (`ch: None`
        // does not drop the rule), the underline colour comes from SGR 58, the
        // strikeout always from the foreground, the two can meet in the same
        // cell — and `clear` empties the rule list too (all three lists must
        // be zeroed in the same call).
        let mut frame = Frame::default();
        frame.clear(grid(8, 16), CaretStyle::default());

        frame.push(bg_cell(0, 0));
        assert_eq!(frame.rule_count(), 0, "cell with no rule produced a rule");

        let red = LinearRgba::from_srgb(0xff, 0x00, 0x00);
        frame.push(Cell {
            col: 1,
            row: 0,
            fg: CURSOR,
            underline: UnderlineStyle::Curl,
            underline_color: Some(red),
            strikeout: true,
            ..Default::default()
        });

        assert_eq!(frame.rule_count(), 2);
        assert_eq!(frame.glyph_count(), 0, "rule cell produced no ink");
        assert_eq!(
            frame.rules(),
            [
                RuleCell {
                    pos: [8.0, 0.0],
                    kind: RuleKind::Curl,
                    rgba: red.to_array(),
                },
                RuleCell {
                    pos: [8.0, 0.0],
                    kind: RuleKind::Strike,
                    rgba: CURSOR.to_array(),
                },
            ]
        );

        frame.clear(grid(8, 16), CaretStyle::default());
        assert_eq!(frame.rule_count(), 0);
    }

    /// A cell with a glyph on the dock's input row.
    fn typed_cell(col: u16, ch: char) -> Cell {
        Cell {
            col,
            row: 0,
            ch: Some(ch),
            fg: CURSOR,
            ..Default::default()
        }
    }

    fn fx(cell: Cell, kind: Kind) -> Fx {
        Fx {
            cell,
            kind,
            effect: 1,
            t: 0.5,
            seed: 0.0,
        }
    }

    #[test]
    fn an_arriving_glyph_is_drawn_by_its_effect_not_twice() {
        // Had `fade` faded in on top of the static glyph nothing would be
        // visible: the in-flight arrival's static glyph is removed from the
        // list to be drawn, but not from `dock_glyphs` itself — a motion frame
        // does not re-push the dock and when the effect ends the static glyph
        // must come back.
        let mut frame = Frame::default();
        frame.clear(grid(8, 16), CaretStyle::default());
        frame.push_dock(typed_cell(2, 'l'));
        frame.push_dock(typed_cell(3, 's'));
        frame.set_dock_fx(
            [fx(typed_cell(3, 's'), Kind::Arrival)],
            &Clusters::default(),
            CURSOR,
        );
        let shown: Vec<char> = frame.dock_glyphs().iter().map(|g| g.ch).collect();
        assert_eq!(shown, ['l']);
        assert_eq!(frame.dock_arrivals().len(), 1);
        assert_eq!(frame.dock_arrivals()[0].glyph, frame.dock_glyphs[1]);
        // The effect ended (motion frame, the list empty): the static glyph
        // comes back without the dock being re-pushed.
        frame.set_dock_fx([], &Clusters::default(), CURSOR);
        let shown: Vec<char> = frame.dock_glyphs().iter().map(|g| g.ch).collect();
        assert_eq!(shown, ['l', 's']);
    }

    #[test]
    fn an_arrival_wears_the_static_glyphs_current_color() {
        // The highlight can change in flight (`l` was written red, and when
        // `s` arrived `ls` turned green): the arrival must carry the colour of
        // the static glyph it hides, not of the moment it was written,
        // otherwise the effect ends in the old colour and jumps to the new.
        let mut frame = Frame::default();
        frame.clear(grid(8, 16), CaretStyle::default());
        let recolored = LinearRgba::from_srgb(0x20, 0xc0, 0x40);
        let now = Cell {
            fg: recolored,
            ..typed_cell(2, 'l')
        };
        frame.push_dock(now);
        frame.set_dock_fx(
            [fx(typed_cell(2, 'l'), Kind::Arrival)],
            &Clusters::default(),
            CURSOR,
        );
        assert_eq!(frame.dock_arrivals().len(), 1);
        assert_eq!(frame.dock_arrivals()[0].glyph.rgba, recolored.to_array());
    }

    #[test]
    fn a_clustered_ghost_outlives_the_dock_table() {
        // The ghost's cell points into the dock's frame table and
        // that table is cleared in the next content frame; the effect copies
        // the string into its own table, and `Frame` into the ghost list's
        // table.
        let mut frame = Frame::default();
        frame.clear(grid(8, 16), CaretStyle::default());
        let mut dock = Clusters::default();
        let ghost = Cell {
            wide: true,
            cluster: dock.push("🇹🇷"),
            ..typed_cell(4, '🇹')
        };
        let mut glyph_fx = GlyphFx::default();
        glyph_fx.apply(
            bt_core::DockEdit::Erase {
                row: 0,
                col: 4,
                ghosts: [ghost].into_iter().collect(),
                shift: 0,
            },
            crate::motion::Motion::default(),
            1,
            &dock,
        );
        dock.clear();
        dock.push("başka");
        for _ in 0..2 {
            // The second round is a motion frame: the table is rebuilt on every write.
            frame.set_dock_fx(glyph_fx.iter(), glyph_fx.clusters(), CURSOR);
            let ghosts = frame.dock_ghosts();
            assert_eq!(ghosts.len(), 1);
            let text = ghosts[0]
                .glyph
                .cluster
                .and_then(|id| frame.fx_clusters().get(id));
            assert_eq!(text, Some("🇹🇷"));
        }
    }

    #[test]
    fn an_arrival_without_its_static_glyph_finishes() {
        // An arrival whose static glyph cannot be found finishes: a letter
        // that is not on the line must not appear. Ghosts are not asked — they
        // have no static glyph to begin with.
        let mut frame = Frame::default();
        frame.clear(grid(8, 16), CaretStyle::default());
        frame.push_dock(typed_cell(2, 'l'));
        let mut glyph_fx = GlyphFx::default();
        let motion = crate::motion::Motion::default();
        let rows = 1;
        for edit in [
            bt_core::DockEdit::Arrive {
                row: 0,
                col: 2,
                cells: [typed_cell(2, 'l')].into_iter().collect(),
                shift: 0,
            },
            bt_core::DockEdit::Erase {
                row: 0,
                col: 5,
                ghosts: [typed_cell(5, 'q')].into_iter().collect(),
                shift: 0,
            },
            // Wrong character: the column holds but `x` is not there.
            bt_core::DockEdit::Arrive {
                row: 0,
                col: 6,
                cells: [typed_cell(6, 'x')].into_iter().collect(),
                shift: 0,
            },
        ] {
            glyph_fx.apply(edit, motion, rows, &Clusters::default());
        }
        frame.suppress_dock(&mut glyph_fx);
        let left: Vec<(u16, Kind)> = glyph_fx.iter().map(|fx| (fx.cell.col, fx.kind)).collect();
        assert_eq!(left, [(2, Kind::Arrival), (5, Kind::Ghost)]);
        frame.set_dock_fx(glyph_fx.iter(), glyph_fx.clusters(), CURSOR);
        assert_eq!(frame.dock_ghosts().len(), 1);
        assert_eq!(frame.dock_arrivals().len(), 1);
        assert!(
            frame.dock_glyphs().is_empty(),
            "arrival's static glyph was drawn twice"
        );
    }

    #[test]
    fn the_effects_live_and_die_with_the_content_frame() {
        // In a frame with no dock (alternate screen) the previous frame's
        // ghost must not hang on.
        let mut frame = Frame::default();
        frame.clear(grid(8, 16), CaretStyle::default());
        frame.set_dock_fx(
            [fx(typed_cell(3, 's'), Kind::Ghost)],
            &Clusters::default(),
            CURSOR,
        );
        assert_eq!(frame.dock_ghosts().len(), 1);
        frame.clear(grid(8, 16), CaretStyle::default());
        assert!(frame.dock_ghosts().is_empty() && frame.dock_arrivals().is_empty());
    }

    #[test]
    fn sgr_flags_translate_to_four_faces() {
        // This is the translation's only place, and when two of the four arms
        // get mixed the symptom is "italic text is drawn bold" — no counter
        // sees it.
        assert_eq!(face(false, false), Face::Regular);
        assert_eq!(face(true, false), Face::Bold);
        assert_eq!(face(false, true), Face::Italic);
        assert_eq!(face(true, true), Face::BoldItalic);
    }
}
