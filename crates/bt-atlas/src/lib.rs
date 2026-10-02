//! bt-atlas — glyph rasterization and atlas packing.
//!
//! Rasterization, the fixed slot grid and the cell metric live here. The
//! font stack sits behind the `FontSystem` trait (`system`): the rules
//! (`rules`) are platformless and only the backend of the platform being
//! built sees its font libraries — on macOS `coretext`
//! (`objc2-core-text` / `objc2-core-graphics` / `objc2-core-foundation`), on
//! Linux `freetype` (FreeType + fontconfig, `harfrust` for clusters; 042).
//! AppKit and the GPU are **not seen**: the textures are `bt-gpu`'s — what
//! leaves here is a slot number and CPU bitmaps, which `bt-gpu` uploads into
//! its own two textures (mask `R8Unorm`, colour `RGBA8Unorm_sRGB`).
//!
//! The four font faces (`Face`) and the rule lines (`RuleKind`) live here: the
//! rule sprites take no glyph from the font, they are drawn procedurally. The
//! family comes from the settings (007 phase-5) and falls back to the chain if
//! it is missing on the machine; [`FontIssue`], which reports that, goes back
//! to the caller — this crate prints nothing to anyone.
//!
//! **Procedurally drawn characters are the second set** and they win without
//! the font ever being asked (`raster::is_procedural`): block elements
//! (U+2580–U+259F), Braille (U+2800–U+28FF), box drawing (U+2500–U+257F,
//! **except the diagonals `╱╲╳`**) and the terminal's graphics set
//! (U+23B8–U+23BF: two vertical edge lines, four scan lines, two corners —
//! `⎷` U+23B7 is left out). The reason is tiling — the font's em box is not
//! the cell box and Menlo's `█` does not fill the cell, leaving a stripe
//! between two stacked blocks. The last family closes another defect too: `⎿`
//! (the marker of Claude Code's tool results) used to arrive from the cascade
//! as a glyph that **does not fit the cell** and came out as a box. They are
//! face-independent (the four faces share one slot; the thin/heavy distinction
//! is already in the character itself) and drawn in **both** size classes,
//! each at its own cell: in the dock's context line the column step is the
//! small face's advance, so the small class's sprite is drawn at the small
//! cell and placed on the large slot's baseline (046 Karar 3).
//!
//! A **single-cell** character missing from the selected font comes from the
//! system's cascade (`rules::fallback_font`) and the gate is **geometric**: a
//! candidate is rejected if the **pixels it will paint** spill outside the
//! cell. What is measured is ink, not advance, because the glyphs of symbol
//! fonts paint narrower than they advance (`⏺` U+23FA) and a gate that
//! measured the advance rejected them although they fit the cell. A candidate
//! that does not fit is drawn into two cells if it is two columns wide (023),
//! and as a smaller-point copy if it does not fit and its overflow is within
//! the limit (041, `rules::SHRINK_LIMIT`); the rest is [`TOFU`].

// The fallback gate's scan and the guard of the tool characters; it has no
// consumer in production (041 phase-1).
#[cfg(all(test, target_os = "macos"))]
mod census;
#[cfg(target_os = "macos")]
mod coretext;
#[cfg(target_os = "linux")]
mod freetype;
mod raster;
mod rules;
mod system;

use std::collections::{HashMap, HashSet};

use raster::DrawResult;
pub use raster::RuleKind;
use rules::Faces;
pub use rules::{Face, FontIssue, Metrics, SizeClass};
pub use rules::{family_issue, monospaced_families};
use system::{Backend, Font, FontSystem};

/// The backend's sample characters and family names, measured on its fonts
/// (042 Karar 7). Test support, not API: `bt-gpu`'s glyph guards take their
/// samples from here (feature `fixture`, a dev-dependency there) so a
/// measurement lives in one place.
#[cfg(feature = "fixture")]
#[doc(hidden)]
pub use system::fixture;

/// What holds a slot in the atlas: a character, a rule line or a grapheme
/// sequence.
///
/// The three live in the same grid because all three are rasterized into
/// slots **one cell in size**: since 023 emoji and wide glyphs have joined the
/// same union by being split into two halves ([`Half`]) and into the colour
/// plane ([`Plane`]), i.e. no separate texture or separate packer was born.
///
/// [`Sprite::Cluster`] is the identity, in the **atlas's own** interner
/// ([`Atlas::intern`]), of a string (flag `🇹🇷`, ZWJ `👨‍👩‍👧`, skin tone
/// `👍🏽`, VS16 `❤️`); that is how `Sprite` stays `Copy + Hash` and the slot
/// key carries no string. If the sequence does not shape into a single glyph
/// the answer is the base character's ([`Atlas::slot`]).
///
/// Procedurally drawn characters (block, Braille) **did not get a third
/// variant**: they are characters and live as `Char`. Opening a `Sprite::Box`
/// would turn `bt-gpu`'s one-line today into the question "which sprite is
/// this character", i.e. it would leak terminal semantics into the renderer;
/// that is why the gate is inside [`Atlas::slot`].
// `repr(u8)`: see `RuleKind`.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Sprite {
    Char(char),
    Rule(RuleKind),
    Cluster(u32),
}

/// The **half** of a glyph in the cell grid — the fourth axis of the slot key
/// and the sibling of [`SizeClass`].
///
/// A wide character is centred in a box two cells wide and rasterized into
/// **two** slots; each slot is still exactly one cell, i.e. the texture
/// layout, `slot_bytes` and the grid arithmetic do not change at all. The quad
/// also stays one cell: `bt-gpu` emits two instances and `GlyphInstance`'s
/// 32-byte stride and the `cell_px` uniform are left untouched.
///
/// The axis was **not added as a variant** to [`Sprite`] and the reason is
/// written in that type's doc: a variant added to `Sprite` leaks the question
/// "which sprite is this character" to the renderer. The axis here, on the
/// other hand, is a request the caller **carries** — like `Face` and
/// `SizeClass` — and `Atlas::slot` normalizes it.
///
/// [`Half::Whole`] means "single cell" and **can also be born for wide
/// characters**: a character declared wide whose ink fits one cell (`☕`,
/// fullwidth `！`) is drawn from a single slot. The gate decides
/// ([`rules::fallback_font`]), not the caller.
// `repr(u8)`: see `RuleKind`.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Half {
    Whole,
    Left,
    Right,
}

/// **Which plane** of the atlas — mask or colour.
///
/// The two planes live inside a single [`Atlas`] and this is deliberate: a
/// second `Atlas` would bring five CoreText derivations (four faces + the
/// small face) and a **second [`Metrics`]** from the same key. `bt-gpu`'s
/// `sync_atlas` exists precisely to prevent this ("an `ensure` slipping in
/// between would serve the two from separate atlases if taken in a second
/// call") and the doc of [`Atlas::context_cell_w`] names the same smell.
///
/// Slots are **one cell in both planes**, i.e. the [`Half`] mechanism solves
/// the geometry of the wide emoji too and the grid arithmetic
/// ([`Atlas::slot_origin`], [`Atlas::capacity`]) is shared by both. The only
/// thing that differs is the pixel format: mask `R8`, colour `RGBA8` — and
/// each plane has its **own monotonic counter**, because the uv is baked in
/// `bt-gpu`'s `prepare` at resolve time and a shared counter whose meaning
/// changed mid-frame would invalidate the uvs of earlier passes.
// `repr(u8)`: see `RuleKind`.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Plane {
    Mask,
    Color,
}

/// The answer of [`Atlas::slot`]: the slot **and** which half was used.
///
/// The second field is a necessity, not a convenience: the decision "one slot
/// or two" is made by the ink gate, i.e. it is only known here — while the
/// caller (`bt-gpu`) has to decide whether to emit a second instance. Without
/// the field an empty quad would land to the right of `☕`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Placed {
    pub slot: u16,
    /// The plane the slot lives in; the caller picks the texture and the
    /// pipeline accordingly. Tofu is **always** [`Plane::Mask`]: the box is a
    /// mask.
    pub plane: Plane,
    /// The box the gate accepted: [`Half::Whole`] one cell, [`Half::Left`] the
    /// left of two cells. [`Half::Right`] is only asked for a character that
    /// returned `Left`.
    pub half: Half,
}

/// Slot 0 is the **resident tofu**: a full atlas and `.notdef` fall back here.
///
/// A visible loss (a box is drawn) instead of a silent one (the glyph is never
/// drawn): a missing font shows itself on screen instead of waiting in a log.
/// `bt-gpu` writes its content once at texture setup ([`Atlas::tofu_bitmap`])
/// and never touches it again — "resident" means exactly this.
pub const TOFU: u16 = 0;

/// The slot share set aside for rule sprites — the number of [`RuleKind`]
/// variants.
///
/// This much of the capacity is closed to characters. See [`Atlas::slot`].
const RULE_RESERVE: u16 = 7;

/// The slot count the atlas aims for — the texture edge is derived from
/// **this**.
///
/// **Not a measurement claim but a design constant** (like `GUTTER_PT` and
/// [`CONTEXT_SCALE`]), though its derivation comes from a measured number:
/// the procedural family is **421** characters (`docs/OLCUMLER.md` → Atlas
/// yuva ayak izi) and the slots it demands of the atlas are **429** — tofu (1)
/// and the rule share closed to characters ([`RULE_RESERVE`], 7) pile on top,
/// because [`Atlas::slot`] gives characters `capacity() - RULE_RESERVE` and
/// `next` starts at 1. The rule is "the family's share should not exceed half
/// the atlas", i.e. `2 × 429 = 858`, rounded up to **1024**.
///
/// The constant is **low-risk** and that honestly makes it a design constant:
/// *every* value in `(450, 1624]` gives the same behaviour at 13pt, 28pt and
/// 29pt@2x — 13pt stays at the floor anyway with 1984 slots, and 28 and 29pt
/// both fold once.
///
/// This number is a **floor**, not a ceiling: if the cell is small the
/// capacity overshoots the target many times over (13pt@2x → 1984) and nobody
/// clips it.
const SLOT_TARGET: u32 = 1024;

/// The floor of the texture edge, in pixels — **the promise to keep today's
/// behaviour**.
///
/// The default point size stays at this edge (it already exceeds
/// [`SLOT_TARGET`]), i.e. the grid, `texture_px()` and the raster do not
/// change bit for bit. If it is lowered the promise is broken: the default
/// point size's texture shrinks and the slot count drops.
const MIN_EDGE: u16 = 1024;

/// The ceiling of the texture edge, in pixels.
///
/// Without a ceiling, growth would go on unbounded at the corner of
/// [`MAX_POINT_SIZE`] × `MAX_LINE_HEIGHT`. 4096 is many times below Metal's
/// texture limit (16384) and even at that corner it keeps the capacity above
/// the family — its number is **computed** in
/// `capacity_clears_the_family_at_every_accepted_size`, not written here.
const MAX_EDGE: u16 = 4096;

/// The accepted range of the `point_size * scale` product.
///
/// The upper bound is not arbitrary: a point size running up to the end of the
/// `u16` metric demands a gigabyte-sized buffer per slot and `texture_px()`
/// exceeds Metal's texture limit many times over. The lower bound cuts
/// unreadable point sizes. The settings parser (`bt-core`) only says "finite
/// and greater than zero"; the range's **sole owner is here**: `Atlas` does
/// not leave its own invariant to the caller's discipline, and since the
/// criterion is `point size × scale`, a ceiling on the settings side would
/// change meaning every time the window moved to another screen. The clamp is
/// **silent** (`.tasks/007-ayarlar-ve-tema/discussion.md` → Karar 4).
/// The ratio of the context line to the display font.
///
/// **Not a measured number, a design constant** (like `CellMetrics::GUTTER_PT`):
/// the user's choice, to give a distinct hierarchy. The ratio, not an absolute
/// point size — when the display font grows with Cmd +/− the context line
/// grows too and the relation of the two lines stays constant.
///
/// The product enters the range of [`effective_point_size`], i.e. with a very
/// small display font it settles on the floor point size: 80% of 5pt is 4.0,
/// exactly the bound.
pub(crate) const CONTEXT_SCALE: f64 = 0.8;

const MIN_POINT_SIZE: f64 = 4.0;
const MAX_POINT_SIZE: f64 = 144.0;

/// A single slot to be written into the texture: **where** and **what**.
///
/// The two come in the same return because `replaceRegion` needs both. Had
/// they been separate (`slot` + a separate `slot_origin`) the caller would
/// have to ask for `&self` while holding the `&mut` borrow and the upload loop
/// would not compile — the boundary would fall on the wrong side of the borrow
/// rule.
pub struct Upload<'a> {
    /// The slot's top-left corner inside the texture, in pixels.
    pub origin: (u16, u16),
    /// A full slot's worth of `R8` coverage data ([`Metrics::slot_bytes`]).
    pub bytes: &'a [u8],
    /// The **right** half of a wide glyph — in the same return, from the same
    /// `&mut` borrow.
    ///
    /// The two halves are **atomic**: two slots are allocated in the same
    /// call, both are uploaded with this return and no second `slot()` round
    /// is expected for the right half. Had they been split into separate
    /// rounds the capacity limit could fall **between** the two — the left slot
    /// opens, the right falls to tofu and half a glyph + half a box appears on
    /// screen. This type's single return **cannot represent** that state:
    /// either both halves come or neither.
    pub right: Option<(u16, u16)>,
    /// The right half's bytes; meaningful if [`Upload::right`] is `Some`.
    pub right_bytes: &'a [u8],
    /// Which plane the bytes are to be written to — it determines the format
    /// and the row stride. `bt-gpu` has to derive `bytesPerRow` from here: if
    /// they diverge Metal reads past the short buffer and the symptom is
    /// silent.
    pub plane: Plane,
}

/// The glyph atlas living in a fixed slot grid.
///
/// There is no packer: in this set **all sprites are cell-sized** (emoji and
/// wide glyphs are out of scope), i.e. the `slot_no → pixel corner`
/// conversion is arithmetic. Procedural characters (block, Braille, line) do
/// not break that constraint — by definition they are exactly one cell; what's
/// more they are what demands the constraint, because their tiling runs to the
/// edge of the cell.
pub struct Atlas {
    faces: Faces,
    /// The context line's regular face: the same family, at [`CONTEXT_SCALE`]
    /// times the point size.
    ///
    /// It holds **one** face, not four, because the small class's only
    /// consumer is the dock's context line and there is no bold/italic there
    /// ([`SizeClass`]). Building four faces would have meant three CoreText
    /// derivations and a second "face could not be obtained" warning — both
    /// costs with no return.
    small: Font,
    metrics: Metrics,
    /// The cell's **fractional** advance, physical pixels — large class.
    ///
    /// [`Metrics::cell_px`]'s width is this rounded up and the grid's step is
    /// that; the fractional form stays here because two consumers cannot work
    /// with the rounded one — the fallback candidate's ink gate
    /// (`rules::fallback_font`) and the centring of the glyph in the cell
    /// (`raster::draw`). The whole rationale is in the doc of
    /// `rules::space_advance`; the guard that the two numbers give the same
    /// measure is `the_cell_is_the_rounded_advance`.
    cell_advance: f64,
    /// The small face's fractional advance: the small-class twin of
    /// [`Atlas::cell_advance`]. The fallback gate and the centring are
    /// **per class**, because both are bounded by that class's own cell.
    context_advance: f64,
    /// The small face's advance width, in pixels: the context line's column
    /// step.
    ///
    /// **Width only**, because the small glyph too is rasterized into the large
    /// slot, on the large cell's baseline ([`Atlas::slot`]): height and
    /// baseline are shared, the only thing that differs is the distance
    /// between letters.
    ///
    /// It is the rounded form of [`Atlas::context_advance`] and **derives from
    /// it**: if computed by two separate routes (one `rules::metrics`, the
    /// other `space_advance`) the same measure would have two sources.
    context_cell_w: u16,
    /// The small face's own cell: the measure a **procedural** character is
    /// drawn at in the small class (046 Karar 3).
    ///
    /// A font glyph does not need it — it is drawn into the large slot on the
    /// large baseline and its size comes from the font. A procedural sprite
    /// has no font size: it *is* the cell, so it has to be drawn at the cell
    /// the context line steps by, or a `█` at the large width would overlap
    /// its neighbour. Born from [`Atlas::context_advance`]
    /// (`rules::metrics_at`), i.e. its width is [`Atlas::context_cell_w`] and
    /// the small class still has one width source; the height follows the
    /// same rule and the same `line_height` as the large cell's.
    small_metrics: Metrics,
    /// The drawing buffer of a small procedural sprite,
    /// [`Atlas::small_metrics`]' `slot_bytes` long. The sprite is drawn here
    /// and then copied into [`Atlas::buffer`] baseline-aligned
    /// (`place_small`); a field for the same reason as `buffer` — no
    /// allocation per glyph.
    small_buffer: Vec<u8>,
    /// The (family, point size, scale) it was built with. The criterion of
    /// [`Atlas::ensure`].
    key: Key,
    /// What the chain said about the requested family; `None` → the requested
    /// one opened or no family was requested.
    font_issue: Option<FontIssue>,
    /// The grid's (column, row) slot count.
    grid: (u16, u16),
    /// The slot a character was **resolved** to — not only the loaded ones: a
    /// character the font does not know also lives here as [`TOFU`], otherwise
    /// the same character would be asked of CoreText again every frame.
    slots: HashMap<(Sprite, Face, SizeClass, Half), (u16, Plane)>,
    /// Keys whose single-cell entry was accepted **shrunk** (041).
    ///
    /// The `Whole` shortcut of a `Left` request may only trust the entry "fits
    /// one cell at full size": had a shrunk `Whole` answered a two-cell
    /// request, the glyph would sit small at the left of the wide cell, its
    /// right half empty — and depending on which request came first. A
    /// separate set, because `slots`' value is read in more than twenty places
    /// and the bit is only this shortcut's question; aliases (the face ladder,
    /// `cluster_as_base`) carry the bit over from their sources.
    shrunk: HashSet<(Sprite, Face, SizeClass)>,
    /// The next free slot; starts at 1 because [`TOFU`] is reserved. It cannot
    /// be derived from `slots.len()`: entries resolved to tofu spend no slot,
    /// i.e. the two numbers deliberately diverge.
    next: u16,
    /// The single-slot drawing buffer. Being a field prevents reallocating
    /// every frame; its content is overwritten with every new glyph.
    buffer: Vec<u8>,
    /// The buffer of the wide glyph's right half — the twin of [`Atlas::buffer`].
    ///
    /// A second buffer is **cheaper and more correct** than using the single
    /// buffer twice: the two halves return with the same `Upload` (atomicity),
    /// i.e. the bytes of both have to be live at the same time. Its size is
    /// exactly one slot, i.e. hundreds of bytes at the default cell.
    buffer_right: Vec<u8>,
    /// The colour plane's slot counter — **separate** from the mask's
    /// [`Atlas::next`].
    ///
    /// The reason for being separate is in [`Plane`]'s doc: the uv is baked at
    /// resolve time. A side gain is **capacity**: emoji slots do not pile onto
    /// the masks' pool and vice versa, i.e. an emoji-heavy session does not eat
    /// the letters' slots.
    ///
    /// **What separates is capacity, not the gate.** The capacity gate before
    /// drawing looks at the mask's counter (the reason is in [`Atlas::slot`],
    /// with three options weighed), i.e. **a full mask atlas rejects emoji too**.
    /// The reverse does not happen: a full colour plane does not affect
    /// letters.
    ///
    /// **No tofu share**: no tofu is born in the colour plane (the box is a
    /// mask), i.e. the counter starts at 0 and all of `capacity()` is open to
    /// emoji.
    color_next: u16,
    /// The colour slot's buffer and its twin; [`Metrics::slot_bytes_rgba`] long.
    ///
    /// **Separate** from the mask's buffer: a shared buffer would want to fit
    /// two formats into the same array, i.e. loosen `raster::draw`'s
    /// precondition assert — that assert is the precondition of the `unsafe`
    /// block and the only thing that catches the wrong plane's buffer.
    color_buffer: Vec<u8>,
    color_buffer_right: Vec<u8>,
    /// The resident tofu box; never changes for the lifetime.
    tofu: Vec<u8>,
    /// The interner's strings: the identity of [`Sprite::Cluster`] is the index
    /// into this list.
    ///
    /// There is no eviction and the policy is **the same as the slots'**: the
    /// entry lives for the atlas's lifetime and drops together with the slots
    /// when [`Atlas::ensure`] rebuilds the atlas. Had it had a separate
    /// lifetime, a rebuilt atlas would be left with sequences whose identity is
    /// alive but whose slot is dead; dropping at the same moment as the slots,
    /// the old identity in the caller's hands is either asked again or falls to
    /// tofu in [`Atlas::slot`].
    clusters: Vec<Box<str>>,
    /// String → identity; the reverse direction of [`Atlas::clusters`].
    cluster_ids: HashMap<Box<str>, u32>,
}

/// The atlas's key: if any one of these four changes, the metric, the raster
/// and the slot mapping are invalid.
///
/// `line_height` is part of the key too, because it also determines the cell
/// height: when the slot size changes the whole raster is invalid.
#[derive(Debug, PartialEq)]
struct Key {
    family: Option<String>,
    point_size: f64,
    scale: f64,
    line_height: f64,
}

impl Key {
    /// The comparison is **exact equality**: point size and scale jump between
    /// discrete values, there is no closeness between them to interpret. The
    /// family name as is — even if `"menlo"` and `"Menlo"` open the same font
    /// they are separate keys; the cost is a single rebuild.
    fn is(&self, family: Option<&str>, point_size: f64, scale: f64, line_height: f64) -> bool {
        self.family.as_deref() == family
            && self.point_size == point_size
            && self.scale == scale
            && self.line_height == line_height
    }
}

impl Atlas {
    /// `family` is the settings' family name (`None` → the chain), `point_size`
    /// the logical point size, `scale` the screen's backing scale,
    /// `line_height` the line spacing multiplier (`1.0` → the font's own
    /// spacing).
    ///
    /// Point size and scale are **multiplied** and fed to the font: the metric
    /// and the raster are born in the same physical pixel space, i.e. the scale
    /// is part of the cache key. The family likewise: another font's metric
    /// means another cell. What to do when the key changes is known to
    /// [`Atlas::ensure`].
    ///
    /// A family not found is **not an error**: the chain's font opens and
    /// [`Atlas::font_issue`] reports it. A terminal cannot open without a
    /// font; a misspelled name should not close the window.
    pub fn new(family: Option<&str>, point_size: f64, scale: f64, line_height: f64) -> Self {
        let (faces, font_issue) =
            Faces::from_chain(family, effective_point_size(point_size, scale));
        // The metric comes **only from the regular face**: the cell grid cannot
        // vary with the face. A bold glyph is rasterized into the same slot and
        // may be clipped by a pixel — every terminal does it this way.
        let metrics = rules::metrics(faces.get(Face::Regular), line_height);
        let cell_advance = rules::space_advance(faces.get(Face::Regular));
        // The small face comes from the **same chain**: `font_issue` is not
        // asked a second time and is ignored, because the same family gets the
        // same answer — a second record would make the user hear the same
        // warning twice.
        let (small, _) = rules::open_chain(
            family,
            effective_point_size(point_size * CONTEXT_SCALE, scale),
        );
        // `line_height` is **not asked**: the line spacing only grows the cell's
        // height and that height is shared by both classes, while the width is
        // the font's own advance. Building a whole `Metrics` and taking the
        // width from it would be deriving the same number by a second route.
        let context_advance = rules::space_advance(&small);
        let context_cell_w = rules::round_up(context_advance);
        // The small cell for procedural sprites. `line_height` **is** asked
        // here: this is a whole cell, and the context line's row grows with the
        // line spacing like the grid's.
        let small_metrics = rules::metrics_at(&small, context_advance, line_height);
        let (w, h) = metrics.cell_px;
        // The edge is derived from the **slot target**: as the cell grows the
        // capacity drops and somewhere it falls below the procedural family
        // (422 slots) — the measured break is 29pt on Retina
        // (`docs/OLCUMLER.md`). The floor is [`MIN_EDGE`], i.e. the default
        // point size stays at today's texture.
        //
        // Counted in `u32`: the product of the quotients overflows `u16` at a
        // small cell (13pt@1x, 4096 edge → 116 224). `grid` is still `u16`.
        //
        // **There is no path to `capacity()`'s `u16::MAX` clamp** and the reason
        // is the loop itself: folding runs only while the capacity is **below**
        // the target and each fold multiplies the capacity by four, i.e. the
        // capacity growth produces is always below `4 × SLOT_TARGET` (4096).
        // The clamp can only be seen at a never-folded floor and that is
        // already today's behaviour.
        //
        // `w`/`h` are at least 1 (`rules::round_up`), i.e. the division is
        // safe; `max(1)` is also for the extreme where the cell is larger than
        // the texture.
        let grid = grid_for(w, h);
        Self {
            faces,
            small,
            metrics,
            cell_advance,
            context_advance,
            context_cell_w,
            small_metrics,
            small_buffer: vec![0u8; small_metrics.slot_bytes()],
            key: Key {
                family: family.map(str::to_owned),
                point_size,
                scale,
                line_height,
            },
            font_issue,
            grid,
            slots: HashMap::new(),
            shrunk: HashSet::new(),
            next: TOFU + 1,
            buffer: vec![0u8; metrics.slot_bytes()],
            buffer_right: vec![0u8; metrics.slot_bytes()],
            color_next: 0,
            color_buffer: vec![0u8; metrics.slot_bytes_rgba()],
            color_buffer_right: vec![0u8; metrics.slot_bytes_rgba()],
            tofu: tofu_buffer(metrics),
            clusters: Vec::new(),
            cluster_ids: HashMap::new(),
        }
    }

    /// Rebuilds the atlas if the key ([`Atlas::new`]'s four values) changed
    /// and returns `true`.
    ///
    /// `true` also means **"reallocate the texture"**: the metric and hence
    /// [`Atlas::texture_px`] may have changed, and writing into the old-sized
    /// texture with the new metric silently corrupts. AppKit reports a scale
    /// change (`windowDidChangeBackingProperties:`), the settings file the
    /// family and point size; this method is the counterpart of both hooks and
    /// does not leave the rebuild decision to the caller's memory.
    #[must_use = "if true the atlas was rebuilt: the slot mapping and texture size may have changed and the texture must be reallocated too"]
    pub fn ensure(
        &mut self,
        family: Option<&str>,
        point_size: f64,
        scale: f64,
        line_height: f64,
    ) -> bool {
        if self.key.is(family, point_size, scale, line_height) {
            return false;
        }
        *self = Self::new(family, point_size, scale, line_height);
        true
    }

    pub fn metrics(&self) -> Metrics {
        self.metrics
    }

    /// The context line's column step, in pixels; see [`Atlas::context_cell_w`].
    ///
    /// At least 1: `rules::round_up` clamps the small face's advance to 1 too,
    /// i.e. it is safe to use as a divisor.
    pub fn context_cell_w(&self) -> u16 {
        self.context_cell_w
    }

    /// The outcome for the requested family: not found or not monospaced.
    /// `None` if no family was requested or a monospaced family was opened as
    /// requested.
    ///
    /// Born with the atlas and recomputed on rebuild; a scale change gives the
    /// same answer, i.e. moving the window to another screen does not move the
    /// answer.
    pub fn font_issue(&self) -> Option<&FontIssue> {
        self.font_issue.as_ref()
    }

    /// The atlas texture's size in pixels; `bt-gpu` allocates the texture
    /// accordingly.
    ///
    /// The **full grid**, not the derived edge: the leftover strip on the edge
    /// falls into no slot, and there is no point allocating it.
    pub fn texture_px(&self) -> (u16, u16) {
        let (w, h) = self.metrics.cell_px;
        (self.grid.0 * w, self.grid.1 * h)
    }

    /// The slot's top-left corner inside the texture, in pixels. The uv
    /// arithmetic is the caller's.
    pub fn slot_origin(&self, slot: u16) -> (u16, u16) {
        // An out-of-grid slot falls to [`TOFU`]. This is not a defensive reflex,
        // it is a real path: `ensure()` can shrink the grid and the caller may
        // hold a slot number left over from a previous scale. A `debug_assert`
        // would not suffice — in release a corner pointing outside the texture
        // is returned, `replaceRegion` writes out of bounds and the symptom is
        // silent.
        let slot = if slot < self.capacity() { slot } else { TOFU };
        let (w, h) = self.metrics.cell_px;
        ((slot % self.grid.0) * w, (slot / self.grid.0) * h)
    }

    /// The string's sprite: the same string always gets the same identity.
    ///
    /// **A single-code-point string lands on `Char`** — the cluster path is
    /// open only to more than one code point, and holding a lone character as
    /// a `Cluster` would rasterize the same glyph under two keys, in two
    /// slots. An empty string carries nothing to draw; it lands on a space.
    pub fn intern(&mut self, text: &str) -> Sprite {
        let mut chars = text.chars();
        let base = match (chars.next(), chars.next()) {
            (None, _) => return Sprite::Char(' '),
            (Some(ch), None) => return Sprite::Char(ch),
            (Some(ch), Some(_)) => ch,
        };
        if let Some(&id) = self.cluster_ids.get(text) {
            return Sprite::Cluster(id);
        }
        // The table has a **ceiling** and the ceiling is the negative cache's:
        // there is no eviction and every distinct string lives for the atlas's
        // lifetime, i.e. an interner without a ceiling would never give back the
        // memory of random output (`cat`ed binary data, wide cells carrying
        // combining marks). A new sequence beyond the ceiling lands on its
        // **base character** — that is also the answer for a sequence that does
        // not shape, i.e. the image is no worse than before 035; the atlas's
        // slots could not hold that many distinct sequences anyway.
        if self.clusters.len() >= self.negative_cache_cap() {
            return Sprite::Char(base);
        }
        let Ok(id) = u32::try_from(self.clusters.len()) else {
            return Sprite::Char(base);
        };
        self.clusters.push(text.into());
        self.cluster_ids.insert(text.into(), id);
        Sprite::Cluster(id)
    }

    /// The permanent content of slot [`TOFU`]; `bt-gpu` writes it once at
    /// texture setup. When [`Atlas::slot`] falls to tofu it does **not** give
    /// the bitmap: the data is already in the texture and re-uploading on
    /// every fall would be a wasted write.
    pub fn tofu_bitmap(&self) -> &[u8] {
        &self.tofu
    }

    /// The character's slot.
    ///
    /// The second value is filled if the slot was **newly opened**; for a
    /// loaded slot and on a fall to tofu it is `None` and the texture is left
    /// untouched.
    pub fn slot(
        &mut self,
        sprite: Sprite,
        face: Face,
        size: SizeClass,
        want: Half,
    ) -> (Placed, Option<Upload<'_>>) {
        // The key carries the **drawn** face, not the **requested** one. They
        // can diverge for three separate reasons and all three are faces of the
        // same sentence:
        //   - rule lines are face-independent (the line under bold text is not
        //     bold) and size-independent too: the dock's context line has no
        //     rules, i.e. a small rule sprite is never born,
        //   - a face missing from the font has collapsed to the regular face
        //     (`Faces::effective`),
        //   - in the small class only the regular face exists
        //     ([`Atlas::small`]),
        //   - a procedurally drawn character is, like a rule, face-independent
        //     ([`raster::is_procedural`]).
        // The normalization is **here**, not in the caller's discipline: a
        // diverging key keeps a byte-for-byte identical bitmap in separate
        // slots, the atlas fills up many times faster and the symptom is
        // silent.
        // `want` is normalized too and is forced down to `Whole` in two places:
        // rule sprites (independent of face and size, always one cell) and the
        // **small class**. The reason for the second: the column step of the
        // dock's context line is the small face's advance, yet the two-cell box
        // derives from the large cell — a two-cell glyph would overlap its
        // neighbour there. (The procedural family is single-cell by
        // definition, so this arm does not touch it.) The dock's input
        // line is `Normal` but `wide` never arrives there (an invariant of
        // `bt_core::dock`), i.e. this arm only closes the context line.
        let want = match (sprite, size) {
            (Sprite::Rule(_), _) | (_, SizeClass::Small) => Half::Whole,
            _ => want,
        };
        let (face, size) = match (sprite, size) {
            (Sprite::Rule(_), _) => (Face::Regular, SizeClass::Normal),
            // A small procedural character lands here too: the face is regular
            // anyway and the class stays `Small`, so its key never collides
            // with the large class's sprite (two cells, two measures).
            (Sprite::Char(_), SizeClass::Small) => (Face::Regular, SizeClass::Small),
            // Unicode carries the thin/heavy distinction **in the character
            // itself** (`─` U+2500 thin, `━` U+2501 heavy), i.e. SGR bold
            // thickening the line would encode the information twice. A side
            // gain: the four faces share one slot and a bold TUI frame
            // loads the atlas once, not four times.
            //
            // The pattern is `SizeClass::Normal`, **not** `_`: had `_` been
            // written, a small request would be forced to `Normal` and drawn at
            // the large cell's width — while the dock's context line steps by
            // the small face's advance (`Frame::column_px`), i.e. the sprite
            // would overlap its neighbour. The small class keeps its own key
            // and its own measure ([`Atlas::small_metrics`], 046 Karar 3).
            (Sprite::Char(ch), SizeClass::Normal) if raster::is_procedural(ch) => {
                (Face::Regular, SizeClass::Normal)
            }
            (Sprite::Char(_), SizeClass::Normal) => (self.faces.effective(face), SizeClass::Normal),
            // The cluster's face is **regular**: the shaped glyph comes from the
            // colour emoji font and there is no bold/italic there, i.e. four
            // faces would hold a byte-for-byte identical bitmap in four separate
            // slots. The base character of a sequence that does not shape is
            // also asked from the regular face (an alias of the same key). The
            // size class is kept: the small row's glyph is the small font's.
            (Sprite::Cluster(_), size) => (Face::Regular, size),
        };
        // The key carries the **requested** half but the answer's half need not
        // be the same as requested: a character that was asked as `Left` but
        // fits one cell is written under the `Whole` key, i.e. the second time
        // it is asked it gives the same slot and the same answer. That is why
        // the gate runs once per key in the atlas's lifetime.
        let key = (sprite, face, size, want);
        if let Some(&(slot, plane)) = self.slots.get(&key) {
            // A cached **rejection** answers `Whole`, like the fresh one: the
            // rejection arm below writes `TOFU` under the `Left` and `Right`
            // keys too, and answering `want` from them made the second ask
            // for a rejected wide character a pair — `bt-gpu` then drew one
            // box on the first frame and two from the second on.
            let half = if (slot, plane) == (TOFU, Plane::Mask) {
                Half::Whole
            } else {
                want
            };
            return (Placed { slot, half, plane }, None);
        }
        // `Left` was asked but if the character was previously **accepted as
        // single-cell**, that gives the answer. Without this branch `☕` would
        // be rasterized twice, spend two slots and its right half would stay
        // empty.
        //
        // **`TOFU` does not pass through this branch and the gate is
        // mandatory.** A single-cell **rejection** is **not** the answer of a
        // two-cell request: the `Whole` request is eliminated with `cols = 1`
        // and that criterion is strictly tighter than the two-cell one, i.e.
        // the inference is one-way — if `Left` was rejected `Whole` is rejected
        // too, not the reverse. Without the gate the path died like this (until
        // dock 024 the input line **always** asked with `wide: false` and the
        // line was `SizeClass::Normal`): a CJK character typed at the prompt
        // was first asked as `Whole` and entered the negative cache; after
        // Enter the same character arrives in the grid with `wide: true`, does
        // not find the `Left` key, gets `TOFU` from here and the whole set stayed
        // **dead** for that character for the atlas's lifetime.
        //
        // A rejection is recognized by **the whole entry**, not by the slot
        // number: the colour plane's slot 0 is the first emoji's real slot and
        // a gate looking at the number would mistake its single-cell acceptance
        // for a rejection and rasterize it a second time (the twin of
        // `cluster_as_base` and of the negative-cache filter).
        //
        // **A shrunk acceptance does not pass through this branch either**
        // (041, [`Atlas::shrunk`]): a `漢` shrunk to one cell should be a
        // full-size double in a two-cell request, not its small single-cell
        // copy.
        if want == Half::Left && !self.shrunk.contains(&(sprite, face, size)) {
            let whole = (sprite, face, size, Half::Whole);
            if let Some(&(slot, plane)) = self.slots.get(&whole)
                && (slot, plane) != (TOFU, Plane::Mask)
            {
                return (
                    Placed {
                        slot,
                        half: Half::Whole,
                        plane,
                    },
                    None,
                );
            }
        }
        // A **share is set aside** for rule sprites: all six are procedural,
        // deterministic and needed for the lifetime. Without the share, after
        // seeing a few thousand distinct glyphs (CJK text, icon-heavy TUI) the
        // grid fills up and from then on a tofu box, instead of the line, would
        // appear under **every** underlined cell. Characters cannot eat the
        // last `RULE_RESERVE` slots; rules stay lazy but their places are
        // guaranteed.
        let cap = match sprite {
            Sprite::Rule(_) => self.capacity(),
            Sprite::Char(_) | Sprite::Cluster(_) => self.capacity().saturating_sub(RULE_RESERVE),
        };
        // **A wide request wants two slots and wants both at once.** The number
        // comes from `want`, not from the gate: the gate only runs during
        // drawing and by then the allocation decision has to have been made.
        // Asking for extra errs on the safe side — a wide character that fits
        // one cell spends one slot, and even if it is rejected one slot short
        // of the limit it has room at the next `ensure`. Erring in the opposite
        // direction would open the left half and drop the right to tofu.
        //
        // `Half::Right` **never reaches** here: the call that accepts the pair
        // writes both keys at once, i.e. the right half returns from the cache
        // round above. On a full atlas, since it is not written to the cache, it
        // falls here and gets tofu — its left half got tofu from the same
        // number too, so the answer stays consistent.
        // `Right` counts **two** as well: it is one half of the same pair, and
        // only the same count makes "the left half got tofu from the same
        // number" true. Counted as one, a full atlas with a single free slot
        // turned the right half into a shrunk whole glyph beside a tofu left
        // half — hidden on macOS, where the fixture's wide character is
        // `.LastResort` and never shrinks (042 phase-4, seen on Linux).
        let need = u32::from(if want == Half::Whole { 1u16 } else { 2 });
        // The criterion is **the mask's** counter and this is a deliberate
        // narrowing. The plane is only known during drawing, i.e. there is no
        // plane-aware pre-gate. Three options were weighed (not measured — none
        // of them had a number taken; what separates them is the **structural**
        // defect of the first two):
        //
        //   - `min(next, color_next)`: the gate **never closes**, because
        //     `color_next` stays 0 for life in a session that sees no emoji —
        //     i.e. on a full atlas every uncached glyph paid one
        //     `CGBitmapContext` + `draw_glyphs` per frame, and for a character
        //     missing from the base font a cascade walk on top. On the main
        //     thread.
        //   - `max(..)`: a full colour plane would drop **letters** to tofu.
        //   - the mask's counter (this one): a full mask atlas rejects emoji too.
        //
        // The third was chosen and its cost **narrows** the promise of
        // [`Atlas::color_next`]: the separate counter separates *capacity*
        // (emoji do not eat the mask's slots, nor the mask the emoji's) but
        // does not separate the *gate*. The exact criterion is asked right
        // before allocation, once the plane is known.
        if u32::from(self.next) + need > u32::from(cap) {
            // A full atlas is **not cached**: this is the atlas's temporary
            // state, not a permanent fact of the font. The capacity derives
            // from the cell size ([`SLOT_TARGET`]), i.e. the same character may
            // find a slot at another point size and a record written here would
            // pin it to tofu.
            //
            // Falling here means **more distinct glyphs than the target in a
            // single frame** and that scenario is **not measured** (022). If
            // measured, its remedy is not LRU but recycling at the
            // `encode_pass` boundary: the slot number is not stored in frame
            // data, `slot_uv` bakes the uv at resolve time and `prepare` runs
            // four times per frame, i.e. any reuse done **mid**-frame
            // invalidates the uvs of earlier passes.
            return (
                Placed {
                    slot: TOFU,
                    half: Half::Whole,
                    plane: Plane::Mask,
                },
                None,
            );
        }
        // Not left in the borrow match's scrutinee: had `&mut self.buffer`
        // stayed there, `&self.buffer` could not be taken inside the arms.
        // The box's advance: `Whole` one cell, `Left` two. `Right` does not
        // reach here (above).
        // Whether the accepted candidate came from the shrink arm: written to
        // [`Atlas::shrunk`] next to the `Whole` entry.
        let mut shrunk = false;
        let result = match sprite {
            // **Procedural drawing comes before the font.** The order is
            // mandatory and "if it is not in the font
            // is not in the font, draw procedurally" would be the wrong arm:
            // `█` *is* in Menlo but does not fill the cell, i.e. that
            // character arrives broken without ever going to the fallback. `⠋`
            // is not in Menlo and if the fallback runs Apple Braille would come
            // and be rejected by the width gate. This is the one place that
            // closes both — and because the arm sits **above** the font arm
            // below, `raster::draw` stays pure as a font path and the doc of
            // `DrawResult` ("the font's answer") is not strained.
            //
            // **Both classes** take this arm, each at its own cell (046 Karar
            // 3): the large class at [`Atlas::metrics`] straight into the slot,
            // the small class at [`Atlas::small_metrics`] into a separate
            // buffer and from there into the large slot, its baseline on the
            // large cell's — the row the small font's letters sit on, so a
            // sparkline stands on the same line as the text beside it and tiles
            // at the context line's column step.
            // The procedural family is **single-cell by definition**: block
            // elements, Braille, box drawing and the technical set are
            // single-column from start to finish (measured, the 023
            // inventory). So `Whole` is not an assumption but the family's own
            // property.
            Sprite::Char(ch) if raster::is_procedural(ch) => {
                match size {
                    SizeClass::Normal => {
                        raster::draw_procedural(ch, self.metrics, &mut self.buffer);
                    }
                    SizeClass::Small => {
                        raster::draw_procedural(ch, self.small_metrics, &mut self.small_buffer);
                        place_small(
                            &self.small_buffer,
                            self.small_metrics,
                            &mut self.buffer,
                            self.metrics,
                        );
                    }
                }
                (DrawResult::Drawn, Half::Whole, Plane::Mask)
            }
            Sprite::Char(ch) => {
                // **The metric is the large cell's in both classes**: the small
                // glyph is drawn into the large slot, on the large cell's
                // baseline (`raster::draw` puts the glyph at `(0, baseline)`).
                // Since the slot size stays shared the grid, the texture and
                // `slot_bytes` do not change — the only thing that differs is
                // the letter's own size, and that comes from the font.
                // The fallback gate's limit is per class too: the small glyph
                // has to fit the small cell's advance, not the large one's.
                let (font, cell_advance) = match size {
                    SizeClass::Normal => (self.faces.get(face), self.cell_advance),
                    SizeClass::Small => (&self.small, self.context_advance),
                };
                // **The base font is always single-cell.** In a monospaced base
                // font every glyph's advance is the cell's advance itself (its
                // guard is `every_base_glyph_advance_is_the_cell_advance`), i.e.
                // if a character declared wide exists in the base font it is
                // drawn into one cell there and its drawing stays **bit for
                // bit** the same as before. Menlo's `☕ ⚡ ♈` family is exactly
                // this arm: 21 of the measured 65.
                let drawn =
                    raster::draw(font, ch, self.metrics, cell_advance, 0.0, &mut self.buffer);
                // **Fallback font.** The arm runs inside the `NoGlyph` leaf and
                // only on the regular face, i.e. the order is: first the face
                // ladder (the `face != Regular` arm below falls to the regular
                // face), then here, the negative cache last. Had the fallback
                // been reached before the ladder was exhausted, a bold `─` would
                // come from a system font and the family's own regular face
                // would never be asked.
                //
                // The arm is not in `match drawn` but **inside the drawing
                // step**, for two mandatory reasons: the `match` arms cannot fall
                // into the negative cache arm (if the fallback is rejected we
                // have to land there) and `ch` is in scope only here. The
                // semantic order does not change.
                //
                // The accepted candidate passes through the `Drawn` arm below:
                // the same key, the same `Upload`, the same slot arithmetic. So
                // the search runs **once** per key in the atlas's lifetime — the
                // rejection enters the negative cache too.
                if drawn == DrawResult::NoGlyph && face == Face::Regular {
                    // The gate is passed the **column count** and its only source
                    // is the caller: `bt-atlas` does not see `unicode-width` (it
                    // would be a new dependency *and* a second width authority
                    // that could diverge from the grid's). The order is inside
                    // the gate: first one cell, then two.
                    let cols = if want == Half::Left { 2 } else { 1 };
                    match rules::fallback_font(font, ch, cell_advance, cols) {
                        Some(alt) => {
                            shrunk = alt.shrunk;
                            self.draw_accepted(&alt, cell_advance)
                        }
                        None => (drawn, Half::Whole, Plane::Mask),
                    }
                } else {
                    (drawn, Half::Whole, Plane::Mask)
                }
            }
            // The **cluster** is the sibling of `Char`'s font arm: the
            // procedural gate is not applied to it (a sequence is not a block or
            // line character) and the candidate again goes to the cascade from
            // the class's own font.
            Sprite::Cluster(id) => {
                let Some(text) = self.clusters.get(id as usize) else {
                    // The identity is not in this atlas's interner: the caller
                    // carries an identity left over from an atlas before it was
                    // rebuilt ([`Atlas::clusters`]). **Not cached** — for the
                    // full atlas's reason: the identity is not a permanent fact,
                    // when asked again it may be bound to a different string. Not
                    // a panic, because `slot()` is in the display link's callback.
                    return (
                        Placed {
                            slot: TOFU,
                            half: Half::Whole,
                            plane: Plane::Mask,
                        },
                        None,
                    );
                };
                // The base is the first character of a string carrying at least
                // two code points, as `intern` separates them; it cannot be
                // empty but `slot()` is on the drawing path, i.e. the assumption
                // is a space, not a panic.
                let base = text.chars().next().unwrap_or(' ');
                let (font, cell_advance) = match size {
                    SizeClass::Normal => (self.faces.get(face), self.cell_advance),
                    SizeClass::Small => (&self.small, self.context_advance),
                };
                // The column count is from the same source as `Char`'s (the half
                // the caller asked for) and the gate's order is the same: first
                // one, then two.
                let cols = if want == Half::Left { 2 } else { 1 };
                match rules::shape_cluster(font, text, cell_advance, cols) {
                    Some(alt) => {
                        shrunk = alt.shrunk;
                        self.draw_accepted(&alt, cell_advance)
                    }
                    // It did not shape into a single glyph or was rejected by the
                    // gate: the answer is the **base character's** (035 R1.1).
                    // Not a box, because the base character can often be drawn
                    // (the `👍` of `👍👍`); not half a glyph, because the base
                    // character passes through its own gate.
                    None => return self.cluster_as_base(sprite, base, size, want),
                }
            }
            // Procedural drawing cannot fail: the font is not asked, no context
            // is built. `Drawn` is not an assumption, it is the type itself.
            Sprite::Rule(kind) => {
                raster::draw_rule(kind, self.metrics, &mut self.buffer);
                (DrawResult::Drawn, Half::Whole, Plane::Mask)
            }
        };
        let (result, half, plane) = result;
        // The key carries the **resolved** half: a character asked as `Left`
        // that fits one cell is written under `Whole`, i.e. the second time it
        // is asked the same answer returns from the cache and the gate does not
        // run again.
        let key = (sprite, face, size, half);
        match result {
            // **This is the gate that decides.** The `need` gate above looks at
            // whichever of the two planes is free and closes only when both are
            // full, i.e. one can get here with an atlas whose one plane is full.
            // The plane is now known (the candidate font's trait bit), i.e. the
            // criterion is exact: that plane's own counter.
            DrawResult::Drawn
                if u32::from(match plane {
                    Plane::Mask => self.next,
                    Plane::Color => self.color_next,
                }) + u32::from(if half == Half::Left { 2u16 } else { 1 })
                    > u32::from(cap) =>
            {
                (
                    Placed {
                        slot: TOFU,
                        half: Half::Whole,
                        plane: Plane::Mask,
                    },
                    None,
                )
            }
            DrawResult::Drawn => {
                // The counter is **the plane's own counter**: the two planes
                // share the same grid arithmetic but the slot numbers are in
                // separate spaces (reason: [`Plane`]).
                let slot = match plane {
                    Plane::Mask => self.next,
                    Plane::Color => self.color_next,
                };
                // **The pair is atomic.** Two slots are allocated in the same
                // expression, two keys are written in the same expression and
                // two byte arrays return with the same `Upload`: there is no
                // second `slot()` round for the right half, i.e. the capacity
                // limit cannot fall between the two. The `need` above is this
                // expression's precondition.
                let pair = half == Half::Left;
                let step = if pair { 2 } else { 1 };
                match plane {
                    Plane::Mask => self.next += step,
                    Plane::Color => self.color_next += step,
                }
                self.slots.insert(key, (slot, plane));
                if shrunk && !pair {
                    self.shrunk.insert((sprite, face, size));
                }
                let right = pair.then(|| {
                    let right_slot = slot + 1;
                    self.slots
                        .insert((sprite, face, size, Half::Right), (right_slot, plane));
                    self.slot_origin(right_slot)
                });
                let origin = self.slot_origin(slot);
                let (bytes, right_bytes) = match plane {
                    Plane::Mask => (&self.buffer, &self.buffer_right),
                    Plane::Color => (&self.color_buffer, &self.color_buffer_right),
                };
                (
                    Placed { slot, half, plane },
                    Some(Upload {
                        origin,
                        bytes,
                        right,
                        right_bytes,
                        plane,
                    }),
                )
            }
            // Both are **permanent**: the font does not have that character, or
            // the context setup (its arguments constant for the atlas's
            // lifetime) always fails. Had they not entered the cache, the same
            // character would be asked of CoreText again every frame as long as
            // it stayed on screen.
            // The **coverage difference between faces** is real: in many
            // families the regular face carries a wide Unicode block while
            // bold/italic carry only Latin. This is the glyph-level counterpart
            // of the fallback `Faces::effective` does at the face level — without
            // it the '→' in a bold line would be a tofu box, while the same
            // character would be drawn properly in a regular line. The recursion
            // is a single step: in the regular face `face == Regular` and this
            // arm does not fire again.
            //
            // **The requested key is written too.** Had it not been, the
            // fallback would be relived every frame: `(Char('→'), Bold)` never
            // appears in the map, `raster::draw` asks CoreText for the bold font
            // every frame (`FontSystem::glyph`), gets `NoGlyph` and falls to the
            // regular face — and this, because `slot()` is on the drawing path,
            // on the main thread, in the middle of the frame budget. Exactly the
            // reason in the comment right above, "had they not entered the cache
            // it would be asked again every frame"; that reason holds for this
            // arm too. If the regular face also gives `NoGlyph` the alias is
            // bound to `TOFU` and the negative cache's ceiling sweeps it as well.
            // The size class is **kept**: today this arm never runs in the small
            // class (the face is already `Regular` there, the condition is
            // closed), but writing `Normal` would silently bind the fallback to
            // the large face — the small row's missing glyph would appear as a
            // large letter.
            DrawResult::NoGlyph if face != Face::Regular => {
                let (placed, upload) = self.slot(sprite, Face::Regular, size, want);
                // `map` consumes `upload` and the `self.buffer` borrow ends here;
                // `insert` is possible only after that. The buffer still carries
                // the bytes the recursive call drew, i.e. the `Upload` can be
                // rebuilt with the same content.
                // The half the regular face **resolved** is written, not the
                // requested one: the answer coming back from the ladder may be
                // `Whole` (the bold face of `☕`) and writing the alias as `Left`
                // would make the caller emit a second instance.
                let origin = upload.as_ref().map(|upload| upload.origin);
                let right = upload.as_ref().and_then(|upload| upload.right);
                self.slots.insert(
                    (sprite, face, size, placed.half),
                    (placed.slot, placed.plane),
                );
                if placed.half == Half::Whole
                    && self.shrunk.contains(&(sprite, Face::Regular, size))
                {
                    self.shrunk.insert((sprite, face, size));
                }
                if right.is_some() {
                    // The right half's alias is written too, otherwise the right
                    // half asked in the bold face would rasterize the regular
                    // face again.
                    self.slots.insert(
                        (sprite, face, size, Half::Right),
                        (placed.slot.saturating_add(1), placed.plane),
                    );
                }
                let (bytes, right_bytes) = match placed.plane {
                    Plane::Mask => (&self.buffer, &self.buffer_right),
                    Plane::Color => (&self.color_buffer, &self.color_buffer_right),
                };
                let upload = origin.map(|origin| Upload {
                    origin,
                    bytes,
                    right,
                    right_bytes,
                    plane: placed.plane,
                });
                (placed, upload)
            }
            DrawResult::NoGlyph | DrawResult::NoContext => {
                // The ceiling: the negative cache spends no slot, i.e. `next`
                // does not bound it. `cat`ing a binary file can produce millions
                // of distinct codepoints and the map would silently grow — this
                // was the crate's only number without a ceiling.
                //
                // When the ceiling is reached the **negative entries are thrown
                // out wholesale**, not "stop caching from now on". The difference
                // emerged in this set: `slot()` is now on the drawing path
                // (`bt-gpu` calls it in the display link callback), i.e. an
                // uncached character would be asked back of CoreText **every
                // frame** as long as it stayed on screen — on the main thread, in
                // the middle of the frame budget. The eviction cost is amortized:
                // at least `capacity()` new entries fit between two evictions.
                // Positive entries (real slots) are kept: throwing them out
                // wholesale would require dropping the texture too and that
                // decision was left out of scope in 022 — the capacity derives
                // from the cell size, i.e. the positive side filling up is now
                // much harder.
                //
                // **The cost grew with the fallback** and this was accepted
                // deliberately: a character asked back after the eviction now
                // pays not only `CTFontGetGlyphsForCharacters` but a cascade walk
                // too. The hot walk was measured and is cheap (the set's
                // `phase-1.md` → Uygulama Notları); what is expensive is a
                // **family's first open** and that is not affected by the
                // eviction — the font stays open in CoreText, it is not reloaded.
                // So the cost the eviction brings back is the hot walk, not the
                // cold open.
                if self.slots.len() >= self.negative_cache_cap() {
                    // The criterion is **the whole entry**, not the slot number:
                    // the colour plane's counter starts at 0 and `TOFU` is 0 too,
                    // i.e. a filter looking at the number would throw out the
                    // first emoji's **positive** entry as well. The symptom is
                    // silent and two-tiered: the next time the emoji is seen it
                    // is rasterized again, its old slot is left orphaned and
                    // `slots2=` swells.
                    self.slots
                        .retain(|_, &mut entry| entry != (TOFU, Plane::Mask));
                }
                self.slots.insert(key, (TOFU, Plane::Mask));
                // **The rejection is written to the `want` key too** and this is
                // mandatory: the `key` above carries the **resolved** half and in
                // the rejection arm that is always `Whole`, i.e. the `Left`
                // request's own key would never be written. Since the alias no
                // longer lets `TOFU` through, that request would pay a cascade
                // walk every frame — on the main thread, in the middle of the
                // frame budget. All three halves are written and all three are
                // right: the `cols = 1` criterion is strictly tighter than the
                // two-cell one, i.e. if `Left` was rejected `Whole` was rejected
                // too.
                if want == Half::Left {
                    self.slots
                        .insert((sprite, face, size, Half::Left), (TOFU, Plane::Mask));
                    self.slots
                        .insert((sprite, face, size, Half::Right), (TOFU, Plane::Mask));
                }
                (
                    Placed {
                        slot: TOFU,
                        half: Half::Whole,
                        plane: Plane::Mask,
                    },
                    None,
                )
            }
        }
    }

    /// Draws a candidate that passed the gate: the plane, one or two halves
    /// and the joint success of both.
    ///
    /// The **shared** drawing of the fallback character and the grapheme
    /// sequence; had they been written separately, centring the two halves in
    /// the same box and accepting the pair atomically would live in two copies
    /// and when one diverged the other would not notice.
    fn draw_accepted(
        &mut self,
        alt: &rules::Accepted,
        cell_advance: f64,
    ) -> (DrawResult, Half, Plane) {
        // **The plane comes from the candidate's own property**: a font
        // carrying colour glyphs goes to the `RGBA8` plane, the others to the
        // mask. The criterion is the trait bit, not the family name (reason:
        // [`FontSystem::has_color_glyphs`]).
        let plane = if Backend::has_color_glyphs(&alt.font) {
            Plane::Color
        } else {
            Plane::Mask
        };
        // The two halves are centred in the **same box** and both are drawn in
        // the same call: the right half's offset is a whole number of pixels,
        // i.e. the AA phase is exactly the same in both.
        let pair = alt.cols >= 2;
        let box_advance = cell_advance * f64::from(alt.cols);
        let shift = f64::from(self.metrics.cell_px.0);
        let half = if pair { Half::Left } else { Half::Whole };
        let rise = alt.rise(self.metrics);
        // One drawer, two recipes: `Plane` says which it will be and the buffer
        // matches it. If they do not match, `raster`'s precondition assert
        // fires — that assert is the only thing that catches the wrong plane.
        let left = match plane {
            Plane::Mask => raster::draw_glyph(
                &alt.font,
                alt.glyph,
                self.metrics,
                box_advance,
                0.0,
                rise,
                &mut self.buffer,
            ),
            Plane::Color => raster::draw_color_glyph(
                &alt.font,
                alt.glyph,
                self.metrics,
                box_advance,
                0.0,
                rise,
                &mut self.color_buffer,
            ),
        };
        if !pair {
            return (left, half, plane);
        }
        let right = match plane {
            Plane::Mask => raster::draw_glyph(
                &alt.font,
                alt.glyph,
                self.metrics,
                box_advance,
                shift,
                rise,
                &mut self.buffer_right,
            ),
            Plane::Color => raster::draw_color_glyph(
                &alt.font,
                alt.glyph,
                self.metrics,
                box_advance,
                shift,
                rise,
                &mut self.color_buffer_right,
            ),
        };
        // The two calls ask for the same glyph of the same font, i.e. both
        // succeed or neither does. Still **both** are tested: if one fails the
        // pair must not be accepted, otherwise a glyph with an empty half would
        // be drawn.
        let both = left == DrawResult::Drawn && right == DrawResult::Drawn;
        let worst = if both {
            DrawResult::Drawn
        } else {
            DrawResult::NoGlyph
        };
        (worst, half, plane)
    }

    /// The answer for a sequence that does not shape: the **base character's**
    /// slot, aliased to the sequence's key.
    ///
    /// The same pattern and the same reason as the face ladder's alias (the
    /// `DrawResult::NoGlyph if face != Regular` arm): had the alias not been
    /// written, the sequence would build a `CTLine`, shape and be rejected by
    /// the gate again every frame — on the main thread, in the middle of the
    /// frame budget. The base character's own entry lives separately, i.e. the
    /// same character standing alone in the grid does not open a second slot.
    fn cluster_as_base(
        &mut self,
        sprite: Sprite,
        base: char,
        size: SizeClass,
        want: Half,
    ) -> (Placed, Option<Upload<'_>>) {
        let (placed, upload) = self.slot(Sprite::Char(base), Face::Regular, size, want);
        // `upload` is consumed here so that the `self.buffer` borrow ends; the
        // buffer still carries the bytes the recursive call drew.
        let origin = upload.as_ref().map(|upload| upload.origin);
        let right = upload.as_ref().and_then(|upload| upload.right);
        let key = |half| (sprite, Face::Regular, size, half);
        // The half the base character **resolved** is written, not the
        // requested one: the `❤` of `❤️` may fit one cell and writing the alias
        // as `Left` would make the caller emit a second instance.
        self.slots
            .insert(key(placed.half), (placed.slot, placed.plane));
        if placed.half == Half::Whole
            && self
                .shrunk
                .contains(&(Sprite::Char(base), Face::Regular, size))
        {
            self.shrunk.insert((sprite, Face::Regular, size));
        }
        // The right half's alias comes from the resolved half, not from the
        // upload: if the base character returned from the cache there is no
        // upload but the pair is still two neighbouring slots.
        if placed.half == Half::Left {
            self.slots.insert(
                key(Half::Right),
                (placed.slot.saturating_add(1), placed.plane),
            );
        }
        // The rejection is written to the **requested** key too (the negative
        // cache's rule): the rejection always resolves `Whole`, i.e. had the
        // `Left` request not been written it would be reshaped every frame. The
        // plane has to be asked as well: `TOFU` is slot 0 of the mask plane,
        // while slot 0 of the colour plane is the first emoji's real slot.
        if placed.slot == TOFU && placed.plane == Plane::Mask {
            self.slots.insert(key(want), (TOFU, Plane::Mask));
            if want == Half::Left {
                self.slots.insert(key(Half::Right), (TOFU, Plane::Mask));
            }
        }
        let (bytes, right_bytes) = match placed.plane {
            Plane::Mask => (&self.buffer, &self.buffer_right),
            Plane::Color => (&self.color_buffer, &self.color_buffer_right),
        };
        let upload = origin.map(|origin| Upload {
            origin,
            bytes,
            right,
            right_bytes,
            plane: placed.plane,
        });
        (placed, upload)
    }

    /// (used, total) slots.
    ///
    /// Tofu counts as used: the texture holds that slot too and the occupancy
    /// ratio will be read from these two numbers in `/measure`.
    pub fn occupancy(&self) -> (usize, usize) {
        (usize::from(self.next), usize::from(self.capacity()))
    }

    /// The colour plane's (used, total) slots.
    ///
    /// Published **separately** from the mask and the reason is the token
    /// contract: the smoke gate's `slots=` counter counts only the mask plane
    /// and adding a second plane into it would leave the question "which plane
    /// filled up" unanswered. A plane it cannot see would be exactly 021's
    /// Braille shape: spending zero slots, silent.
    ///
    /// The total is the same in both ([`Atlas::capacity`]): the two planes
    /// share the same slot grid, only the pixel format and the counter differ.
    pub fn color_occupancy(&self) -> (usize, usize) {
        (usize::from(self.color_next), usize::from(self.capacity()))
    }

    /// The most entries the map accepts — positive and negative together.
    /// **Twice** the capacity: one share is the largest value the positive
    /// entries can reach, the second is the share left to the negative cache.
    ///
    /// The capacity meant is [`Atlas::capacity`], i.e. the number from the
    /// **derived** edge ([`SLOT_TARGET`]) — not a fixed ceiling. When the edge
    /// folds this share grows with it and its growth is right: if there are
    /// more slots on the positive side, more characters have been tried on the
    /// negative side too.
    fn negative_cache_cap(&self) -> usize {
        usize::from(self.capacity()).saturating_mul(2)
    }

    /// The total slot count.
    ///
    /// Clamped to `u16`: the slot number is handed outside as `u16` and at very
    /// small cells the grid may exceed that limit. Clamping narrows the
    /// capacity, while overflow would silently overlap slots.
    fn capacity(&self) -> u16 {
        let total = u32::from(self.grid.0) * u32::from(self.grid.1);
        u16::try_from(total).unwrap_or(u16::MAX)
    }
}

/// The texture edge that falls to the cell size, in pixels — the **sole**
/// source of the decision.
///
/// The floor is [`MIN_EDGE`]; it doubles as long as the capacity stays below
/// [`SLOT_TARGET`] and the ceiling ([`MAX_EDGE`]) has not been reached. Tests
/// **call** this function, they do not write a second copy: a mirrored
/// derivation cannot see its own error.
fn edge_for(w: u16, h: u16) -> u16 {
    let mut edge = MIN_EDGE;
    while slots_at(grid_at(edge, w, h)) < SLOT_TARGET && edge < MAX_EDGE {
        // `min` is not a defensive reflex, it is what makes [`MAX_EDGE`]'s doc
        // **true**: the guard looks **before** folding, i.e. if the ceiling is
        // not `MIN_EDGE * 2^k` the product would exceed it and the constant
        // would no longer say what its name says. Overflow is closed on the
        // same line too.
        edge = edge.saturating_mul(2).min(MAX_EDGE);
    }
    edge
}

/// The grid's row/column count at the given edge — **one expression, two
/// readers** (the decision of [`edge_for`] and the grid [`grid_for`] builds).
///
/// Had there been two copies they could silently diverge: the growth loop
/// would say "target met" by one number while the built grid gave another.
fn grid_at(edge: u16, w: u16, h: u16) -> (u16, u16) {
    ((edge / w).max(1), (edge / h).max(1))
}

/// The grid's slot count. `u32`: the product exceeds `u16` at a small cell
/// (13pt@1x, 4096 edge → 116 224).
fn slots_at((cols, rows): (u16, u16)) -> u32 {
    u32::from(cols) * u32::from(rows)
}

/// [`edge_for`] converted to a grid.
fn grid_for(w: u16, h: u16) -> (u16, u16) {
    grid_at(edge_for(w, h), w, h)
}

/// The point size that will enter the font: multiplied by the scale and
/// clamped into the range.
///
/// NaN is handled separately because `clamp` **lets it through**; falling to
/// the floor point size is better than both crashing and silently corrupting —
/// the result is visibly wrong and gets noticed.
fn effective_point_size(point_size: f64, scale: f64) -> f64 {
    let v = point_size * scale;
    if v.is_finite() {
        v.clamp(MIN_POINT_SIZE, MAX_POINT_SIZE)
    } else {
        MIN_POINT_SIZE
    }
}

/// Draws the tofu box: a 1 px frame, one pixel inside the cell edge.
///
/// The font's `.notdef` glyph is **not used**: in some fonts it is empty, in
/// others a box and which one depends on the font version. Drawing the frame
/// ourselves makes tofu font-independent — the "visible loss" claim holds only
/// this way.
/// Copies a small-class sprite (`src`, drawn at `small`) into a large slot
/// (`dst`, `large`): left edge at column 0, the small cell's baseline row on
/// the large cell's (046 Karar 3).
///
/// What falls outside the large slot is **clipped**, not a panic: the small
/// cell is shorter and narrower than the large one with every real font, but
/// the relation is the fonts' data, not a type — and this runs in the display
/// link's callback. The rest of the slot is zero, so neither the previous
/// glyph nor the small cell's own box edge leaks into the neighbour.
fn place_small(src: &[u8], small: Metrics, dst: &mut [u8], large: Metrics) {
    dst.fill(0);
    let (sw, sh) = small.cell_wh();
    let (lw, lh) = large.cell_wh();
    // Signed: the small baseline above the large one is the normal case
    // (`dy > 0`), but nothing in the type rules out the reverse.
    let dy = i64::from(large.baseline_px) - i64::from(small.baseline_px);
    let cols = sw.min(lw);
    for y in 0..sh {
        let Some(ty) = i64::try_from(y)
            .ok()
            .and_then(|y| usize::try_from(y + dy).ok())
            .filter(|&ty| ty < lh)
        else {
            continue;
        };
        dst[ty * lw..ty * lw + cols].copy_from_slice(&src[y * sw..y * sw + cols]);
    }
}

fn tofu_buffer(m: Metrics) -> Vec<u8> {
    let (w, h) = m.cell_wh();
    let mut target = vec![0u8; m.slot_bytes()];
    let (x0, x1) = (1usize, w.saturating_sub(2));
    let (y0, y1) = (1usize, h.saturating_sub(2));
    if x1 <= x0 || y1 <= y0 {
        // The cell is too narrow for a frame; an empty slot beats a box.
        return target;
    }
    // audit: `x1 < w` and `y1 < h` (both `saturating_sub(2)`), i.e. the largest
    // index `y1 * w + x1 < w * h` — within the slice bounds.
    for x in x0..=x1 {
        target[y0 * w + x] = 0xff;
        target[y1 * w + x] = 0xff;
    }
    for y in y0..=y1 {
        target[y * w + x0] = 0xff;
        target[y * w + x1] = 0xff;
    }
    target
}

#[cfg(test)]
mod tests {
    use super::*;
    // Sample characters and family names come from the backend's fixture
    // (042 Karar 7): a platformless test names no font.
    use crate::system::fixture::{
        self, CLUSTER_BASE, CLUSTER_SCALE, CLUSTERS, FALLBACK_CHAR, GATE_PROBES, UNKNOWN_CHAR,
        WIDE_CHAR,
    };

    /// The test point size is deliberately large: the grid derives from the
    /// cell size, i.e. large point size = few slots. That way the "full atlas"
    /// test runs without rasterizing thousands of glyphs. **The pool grew in
    /// 022** (procedural family + ASCII × four faces ≈ 800 requests) because
    /// once the edge was derived the smallest capacity rose to 564 and the
    /// 95-character ASCII cannot fill it; i.e. "dozens" is no longer right, but
    /// it is not thousands either and the reason for the choice stays the same.
    /// The value is [`MAX_POINT_SIZE`]: asking for more is silently clamped and
    /// the test would misjudge the capacity.
    const LARGE_POINT_SIZE: f64 = MAX_POINT_SIZE;
    /// The largest line spacing the settings parser accepts.
    ///
    /// Its source is `bt_core::settings::MAX_LINE_HEIGHT` but it **cannot be
    /// read from there**: the layer direction forbids `bt-atlas` from seeing
    /// `bt-core`. The copy is deliberate and narrow — only to build the worst
    /// corner; if the two diverge this test misses the corner, it does not
    /// produce a wrong drawing.
    const LARGEST_LINE_HEIGHT: f64 = 2.0;
    const POINT_SIZE: f64 = 13.0;
    /// A family that is on no machine; CoreText gives another font instead.
    const MISSING_FAMILY: &str = "Bu Aile Yok 12345";

    /// An atlas built through the chain — the one production builds when the
    /// settings name no family.
    fn atlas(point_size: f64, scale: f64) -> Atlas {
        Atlas::new(None, point_size, scale, 1.0)
    }

    /// The (name, base font, **fractional** cell advance) triple of a size
    /// class. Every guard about the fallback has to walk both classes
    /// separately: the base font and the limit are per class, a test that
    /// passes through only one would never have tested the other.
    fn size_classes(a: &Atlas) -> [(&'static str, &Font, f64); 2] {
        [
            ("regular face", a.faces.get(Face::Regular), a.cell_advance),
            ("small face", &a.small, a.context_advance),
        ]
    }

    #[test]
    fn metrics_are_in_a_sane_range() {
        let m = atlas(POINT_SIZE, 1.0).metrics();
        assert!(m.cell_px.0 > 0, "width is zero: {m:?}");
        assert!(m.cell_px.1 > m.cell_px.0, "a monospace cell is tall: {m:?}");
        assert!(m.baseline_px > 0, "baseline is zero: {m:?}");
        assert!(
            m.baseline_px <= m.cell_px.1,
            "baseline is inside the cell: {m:?}"
        );
        assert_eq!(
            m.slot_bytes(),
            usize::from(m.cell_px.0) * usize::from(m.cell_px.1)
        );
    }

    #[test]
    fn the_cell_is_the_rounded_advance() {
        // The cell width lives in two representations: fractional
        // ([`Atlas::cell_advance`], the input of the fallback gate and of
        // centring) and rounded up ([`Metrics::cell_px`], the grid's step). The
        // two have to be **the same measure**; if they diverge the gate looks at
        // one cell and the centring at another and the symptom is silent. In the
        // small class there is also a history: `context_cell_w` was once derived
        // via `rules::metrics(&small, ..)`, i.e. the same number had two
        // sources.
        for (point_size, scale) in [
            (POINT_SIZE, 1.0),
            (POINT_SIZE, 2.0),
            (LARGE_POINT_SIZE, 1.0),
        ] {
            let a = atlas(point_size, scale);
            assert_eq!(
                rules::round_up(a.cell_advance),
                a.metrics.cell_px.0,
                "{point_size}×{scale}: the two representations of the large class diverged ({})",
                a.cell_advance
            );
            assert_eq!(
                rules::round_up(a.context_advance),
                a.context_cell_w,
                "{point_size}×{scale}: the two representations of the small class diverged ({})",
                a.context_advance
            );
            // The small cell of procedural sprites (046) is a third reader of
            // the same width, not a third source.
            assert_eq!(
                a.small_metrics.cell_px.0, a.context_cell_w,
                "{point_size}×{scale}: the small procedural cell is not the context line's column step"
            );
        }
    }

    #[test]
    fn every_base_glyph_advance_is_the_cell_advance() {
        // Centring is **universal** and in the base font it has to be exactly
        // zero: if `(cell - advance) / 2` gave a fractional result CG's edge
        // smoothing would change and the ground under all of the repo's pixel
        // guards (`glyph_sits_on_the_baseline`, `descender_fits_in_the_cell`, …)
        // would be silently hollowed out. This test holds the **reason** for
        // that zero.
        //
        // The claim is not "the font is monospaced", it is stronger than that:
        // every glyph's advance is equal to the cell's advance **bit for bit**.
        // A non-monospaced family would not fail this (the chain's base is
        // Menlo) but there the centring really shifts — `raster::draw`'s
        // `max(0.0)` explains that path by name.
        let a = atlas(POINT_SIZE, 1.0);
        // **All five fonts**, not two: `cell_advance` is the regular face's
        // measure (`Metrics` derives only from it) but the atlas centres the
        // bold, italic and bold-italic faces with the **same** number too. In a
        // family whose bold face is narrower than its regular face every bold
        // glyph would shift right and the shift would show in one direction only
        // — `max(0.0)` swallows the other. Had the guard been limited to the
        // regular and small faces it would never have seen that arm.
        let fonts = [
            ("regular face", a.faces.get(Face::Regular), a.cell_advance),
            ("bold face", a.faces.get(Face::Bold), a.cell_advance),
            ("italic face", a.faces.get(Face::Italic), a.cell_advance),
            ("bold italic", a.faces.get(Face::BoldItalic), a.cell_advance),
            ("small face", &a.small, a.context_advance),
        ];
        for (label, face_font, cell) in fonts {
            // Printable ASCII, box drawing and Menlo's own symbols: if any glyph
            // advances differently from the cell it shows up here. Combining
            // marks are in the list too: a glyph with zero advance would be
            // rasterized into the **middle** of the cell and the symptom would
            // only be visible on screen (in Menlo U+0301 advances a full cell,
            // i.e. this arm is closed today — measured).
            for ch in (' '..='~').chain(fixture::BASE_SYMBOLS.chars()) {
                let Some(glyph) = Backend::glyph(face_font, ch) else {
                    continue;
                };
                assert_eq!(
                    Backend::advance(face_font, glyph),
                    cell,
                    "{label}: '{ch}' advances differently from the cell, centring is no longer a no-op"
                );
            }
        }

        // The second half: `raster::draw` **really** consumes this number. Had
        // only the equality above been tested, the claim would amount to the
        // input being correct; `draw` ignoring the input and looking at the
        // rounded cell (`cell_px.0`) — i.e. shifting every glyph of the base
        // font by 0.09 pixels and silently changing the whole raster — would
        // pass from here. The criterion: a wider cell advance should push the
        // bitmap to the right.
        let m = a.metrics();
        let font = a.faces.get(Face::Regular);
        let mut own = vec![0u8; m.slot_bytes()];
        let mut wider = vec![0u8; m.slot_bytes()];
        assert_eq!(
            raster::draw(font, 'W', m, a.cell_advance, 0.0, &mut own),
            DrawResult::Drawn
        );
        // A cell four pixels wider pushes the glyph two pixels to the right.
        assert_eq!(
            raster::draw(font, 'W', m, a.cell_advance + 4.0, 0.0, &mut wider),
            DrawResult::Drawn
        );
        assert_ne!(
            own, wider,
            "`draw` ignores the cell advance: centring does not depend on the input"
        );
    }

    #[test]
    fn fallback_glyph_is_drawn_in_both_size_classes() {
        let mut a = atlas(POINT_SIZE, 1.0);
        let (w, h) = a.metrics().cell_wh();
        // The rightmost column of the coverage; the same as the criterion of
        // `the_small_class_is_narrower_…` and for the same reason: a single
        // pixel value depends on the font version, the bound does not.
        let ink_right = |bytes: &[u8]| {
            (0..w)
                .rev()
                .find(|&x| (0..h).any(|y| bytes[y * w + x] > 0))
                .expect("the fallback glyph painted no pixels at all")
        };

        let mut edge = Vec::new();
        for size in [SizeClass::Normal, SizeClass::Small] {
            let (placed, upload) = a.slot(
                Sprite::Char(FALLBACK_CHAR),
                Face::Regular,
                size,
                Half::Whole,
            );
            let slot = placed.slot;
            assert_ne!(
                slot, TOFU,
                "{size:?}: '{FALLBACK_CHAR}' should come from the fallback, not a box"
            );
            edge.push(ink_right(upload.expect("new slot").bytes));
        }

        // The two classes are evaluated **separately** and each holds its own
        // slot: if the key carried no size class, there would be one slot.
        assert_eq!(
            a.occupancy().0,
            3,
            "the two size classes must get separate slots (three with tofu)"
        );
        // And the fallback's **base** is that class's own font: in the small
        // class a narrower trace must land in the same slot. Without this, the
        // claim "both classes work" could not be told apart from an
        // implementation that draws a large-point glyph on a small row — and
        // the symptom would be silent, because something would still be
        // visible.
        assert!(
            edge[1] < edge[0],
            "the small class's fallback did not narrow: right edge {} in large, {} in small",
            edge[0],
            edge[1]
        );
    }

    #[test]
    fn fallback_glyph_fits_the_cell() {
        // `slot != TOFU` **cannot see** clipping: CG silently cuts ink that
        // spills out of the cell and the bitmap still looks full. The gate now
        // measures ink horizontally, but **does not measure vertically**
        // (rationale in the doc of `rules::ink_fits_box`: the only cluster
        // vertical would reject is emoji, and it returns at full size
        // horizontally, then is centred in the cell once shrunk) — a
        // candidate with a tall ascent can pass the gate and still get
        // clipped. The criterion is therefore the font's own bounding
        // rectangle and **all four edges at once**: the gate's witness
        // horizontally, the only guard vertically.
        let a = atlas(POINT_SIZE, 1.0);
        let m = a.metrics();
        // CG's origin is bottom-left: the baseline sits this far above the
        // slot's bottom (same arithmetic as `raster::draw`).
        let baseline = f64::from(m.cell_px.1 - m.baseline_px);
        for (label, base, cell) in size_classes(&a) {
            let alt = rules::fallback_font(base, FALLBACK_CHAR, cell, 1)
                .map(|accepted| accepted.font)
                .unwrap_or_else(|| panic!("{label}: '{FALLBACK_CHAR}' must pass the gate"));
            let glyph = Backend::glyph(&alt, FALLBACK_CHAR)
                .expect("the candidate that passed the gate can draw");
            let rect = Backend::ink(&alt, glyph);
            let x = rules::centre_shift(cell, Backend::advance(&alt, glyph));
            let (left, right) = (x + rect.x, x + rect.x + rect.width);
            assert!(left >= 0.0, "{label}: ink spilled over the left ({left})");
            assert!(
                right <= cell,
                "{label}: ink spilled over the right ({right} > {cell})"
            );
            let (bottom, top) = (baseline + rect.y, baseline + rect.y + rect.height);
            assert!(
                bottom >= 0.0,
                "{label}: ink spilled over the bottom ({bottom})"
            );
            let cell_h = f64::from(m.cell_px.1);
            assert!(
                top <= cell_h,
                "{label}: ink spilled over the top ({top} > {cell_h})"
            );

            // And centring **really** runs on the fallback path: drawing the
            // same candidate without the shift (making the bound exactly the
            // glyph's advance, i.e. zeroing the shift by construction) gives a
            // different bitmap.
            //
            // The criterion is **not** "the ink's centre of mass moved closer
            // to the cell's middle": what gets centred is the glyph's
            // **advance box**, not its ink, and `⏵`'s side bearings are
            // asymmetric (measured: 1.04 on the left, 0.13 on the right). A
            // centre criterion would point the wrong way on this glyph and
            // fail a correct implementation. The claim is therefore more
            // modest but still observable: the shift is applied and, unlike
            // the base font's, is not zero.
            let advance = Backend::advance(&alt, glyph);
            let mut centred = vec![0u8; m.slot_bytes()];
            let mut flush = vec![0u8; m.slot_bytes()];
            assert_eq!(
                raster::draw(&alt, FALLBACK_CHAR, m, cell, 0.0, &mut centred),
                DrawResult::Drawn
            );
            assert_eq!(
                raster::draw(&alt, FALLBACK_CHAR, m, advance, 0.0, &mut flush),
                DrawResult::Drawn
            );
            assert_ne!(
                centred, flush,
                "{label}: the fallback glyph was not shifted (gate {cell}, advance {advance})"
            );
        }
    }

    #[test]
    fn the_gate_decides_by_ink_alone() {
        // What the guard tests is **not** "is this character a box": that is
        // a fact that depends on which fonts are installed on the machine and
        // is not a property of the code. `U+E0B0` falls to `.LastResort` on
        // this machine and is rejected, but on a machine with a Nerd Font it
        // falls to a real glyph and **drawing it is correct**; writing the
        // expectation as a constant would fail `make check` on correct code.
        //
        // What is tested is **the gate's rule**: if the pixels the candidate
        // will paint stay inside the cell it is drawn, if they spill over it
        // is a box. The expectation is derived from the candidate's own ink
        // box, so the criterion is the same on every machine — and the
        // observation and the expectation come from two separate calls (one
        // `fallback_font`, the other `slot`), so it is not a tautology: if
        // the gate does not run on `slot`'s path, this test fails.
        //
        // If the expectation were derived from the **advance**, this test
        // would fail on [`INK_CHAR`]; that is why it stays in the list —
        // reverting the criterion must not be silent. (`⠋` was the second
        // witness until 046 opened the procedural gate in the small class:
        // it no longer reaches the fallback in either class.)
        let mut a = atlas(POINT_SIZE, 1.0);
        let classes = size_classes(&a);
        let mut plan: Vec<(char, SizeClass, bool, String)> = Vec::new();
        for ch in GATE_PROBES {
            for (i, size) in [SizeClass::Normal, SizeClass::Small]
                .into_iter()
                .enumerate()
            {
                let (label, base, cell) = classes[i];
                // If the base font has it, the fallback path never runs: not the experiment's subject.
                if Backend::glyph(base, ch).is_some() {
                    continue;
                }
                // A procedurally drawn character is not the experiment's
                // subject either: the gate stands **before** it and the font
                // is never asked — in both classes since 046 (Karar 3).
                if raster::is_procedural(ch) {
                    continue;
                }
                // If there is no candidate at all it is not the gate's subject either — the gate does not issue the rejection.
                let Some(open) = rules::fallback_font(base, ch, f64::INFINITY, 1).map(|a| a.font)
                else {
                    continue;
                };
                let glyph = Backend::glyph(&open, ch).expect("the candidate can draw");
                let advance = Backend::advance(&open, glyph);
                // The candidate's ink **at the place it will be drawn**: the
                // shift is exactly what `raster::draw` applies
                // (`rules::centre_shift`), otherwise the test would measure a
                // placement that is never drawn.
                let ink = Backend::ink(&open, glyph);
                let left = ink.x + rules::centre_shift(cell, advance);
                let right = left + ink.width;
                // A candidate that does not fit is drawn shrunk if it is
                // within the limit and is not `.LastResort` (041); the
                // expectation again comes from the candidate's own
                // measurements, from the same function as the shrink
                // coefficient.
                let fit = rules::fit_ratio(cell, advance, ink);
                let shrinks = fit <= rules::SHRINK_LIMIT && !Backend::is_last_resort(&open);
                let family = fixture::family_name(&open);
                plan.push((
                    ch,
                    size,
                    (left >= 0.0 && right <= cell) || shrinks,
                    format!(
                        "{label}, {family}, ink {left}..{right} / cell {cell} \
                         (advance {advance}, fit {fit:.3})"
                    ),
                ));
            }
        }

        let (mut fits, mut wide) = (0usize, 0usize);
        for (ch, size, should_fit, why) in plan {
            let placed = a.slot(Sprite::Char(ch), Face::Regular, size, Half::Whole).0;
            // Slot 0 of the colour plane is a real slot, even though it
            // carries the same number as `TOFU`
            // ([`color_slot_zero_answers_the_left_request`]).
            let slot = if placed.plane == Plane::Color {
                TOFU + 1
            } else {
                placed.slot
            };
            if should_fit {
                assert_ne!(
                    slot, TOFU,
                    "a candidate that fits the cell or shrinks was not drawn: '{ch}' ({why})"
                );
                fits += 1;
            } else {
                assert_eq!(
                    slot, TOFU,
                    "a candidate that does not fit the cell was drawn: '{ch}' ({why})"
                );
                wide += 1;
            }
        }
        // The experiment **cannot be empty**: the gate must have been
        // observed in both directions. Without this line, rejecting every
        // candidate (or passing every one) would silently make the test
        // meaningless and it would still be green.
        assert!(
            fits > 0 && wide > 0,
            "the gate was tested in only one direction: fitting {fits}, non-fitting {wide}"
        );
        // A rejected candidate **spends no slot**; otherwise one CJK file
        // would exhaust the atlas. `fits` slots + tofu are expected, in the
        // sum of the two planes: a shrunk emoji goes to the colour plane.
        assert_eq!(
            a.occupancy().0 + a.color_occupancy().0,
            fits + 1,
            "a rejected candidate spent a slot (drawn {fits})"
        );
    }

    #[test]
    fn same_char_gets_same_slot() {
        let mut a = atlas(POINT_SIZE, 1.0);
        let (placed_first, upload) = a.slot(
            Sprite::Char('A'),
            Face::Regular,
            SizeClass::Normal,
            Half::Whole,
        );
        let first = placed_first.slot;
        assert_ne!(first, TOFU, "a recognised character must not fall to tofu");
        assert!(upload.is_some(), "an upload must come on the first ask");
        let (placed_second, again) = a.slot(
            Sprite::Char('A'),
            Face::Regular,
            SizeClass::Normal,
            Half::Whole,
        );
        let second = placed_second.slot;
        assert_eq!(first, second);
        assert!(
            again.is_none(),
            "the slot is already uploaded: the texture must stay untouched"
        );
    }

    #[test]
    fn a_wide_request_gets_the_same_answer_from_the_cache() {
        // The cache must answer exactly what the gate answered: `bt-gpu`
        // asks every frame and fans a `Left` answer out to two quads. A
        // rejected wide request used to come back `Whole` fresh and `Left`
        // cached — one box on the first frame, two from the second on. The
        // probes cover a pair, a shrunk glyph and (on Linux, with no
        // last-resort font) a rejection.
        for size in [POINT_SIZE, 16.0] {
            let mut a = atlas(size, 1.0);
            for ch in GATE_PROBES {
                let ask = |a: &mut Atlas| {
                    a.slot(
                        Sprite::Char(ch),
                        Face::Regular,
                        SizeClass::Normal,
                        Half::Left,
                    )
                    .0
                };
                let fresh = ask(&mut a);
                let cached = ask(&mut a);
                assert_eq!(
                    fresh, cached,
                    "{size}pt '{ch}': the cache changed the answer"
                );
            }
        }
    }

    #[test]
    fn upload_carries_slot_origin() {
        // This guard's real job is to compile: if the corner and the bytes
        // came from separate calls, `bt-gpu`'s upload loop would have to ask
        // for `&self` while holding the `&mut` borrow, and it would not
        // compile.
        let mut a = atlas(POINT_SIZE, 1.0);
        let slot_len = a.metrics().slot_bytes();
        // The shape of `bt-gpu`'s upload loop: the upload is consumed in its
        // own block, then the same atlas is read again for the uv. If the
        // corner were not inside `Upload`, that block would need `&self` and
        // would not compile because of `slot`'s `&mut` borrow.
        let (placed_slot, upload) = a.slot(
            Sprite::Char('A'),
            Face::Regular,
            SizeClass::Normal,
            Half::Whole,
        );
        let slot = placed_slot.slot;
        let mut written = None;
        if let Some(y) = upload {
            assert_eq!(y.bytes.len(), slot_len);
            written = Some(y.origin);
        }
        assert_eq!(written, Some(a.slot_origin(slot)));
    }

    #[test]
    fn rasterized_glyph_is_not_empty() {
        // Without this guard, the state "everything works but the atlas is
        // completely empty" would go unnoticed: slot numbers right, texture
        // the right size, screen blank.
        let mut a = atlas(POINT_SIZE, 1.0);
        let (placed__, upload) = a.slot(
            Sprite::Char('W'),
            Face::Regular,
            SizeClass::Normal,
            Half::Whole,
        );
        let _ = placed__.slot;
        let bytes = upload.expect("an upload must come on the first ask").bytes;
        assert!(bytes.iter().any(|&b| b > 0), "'W' painted no pixels at all");
        // A space is also a recognised glyph but paints nothing: the
        // criterion is "did the raster run", not "is the bitmap filled".
        let (placed_slot, blank) = a.slot(
            Sprite::Char(' '),
            Face::Regular,
            SizeClass::Normal,
            Half::Whole,
        );
        let slot = placed_slot.slot;
        assert_ne!(slot, TOFU);
        assert!(
            blank.expect("new slot").bytes.iter().all(|&b| b == 0),
            "a space must not paint"
        );
    }

    #[test]
    fn tofu_box_is_drawn_and_resident() {
        let mut a = atlas(POINT_SIZE, 1.0);
        assert_eq!(a.tofu_bitmap().len(), a.metrics().slot_bytes());
        assert!(
            a.tofu_bitmap().iter().any(|&b| b > 0),
            "tofu cannot be an empty box"
        );
        // A call that falls to tofu **gives no upload**: the data is already
        // in the texture. With the arrival of the fallback search the scope
        // of this claim grew: a rejected candidate is never drawn into the
        // buffer, so the path still lands here, meaning the word "resident"
        // also holds for a fallback rejection. An accepted candidate goes
        // through the `Drawn` arm and an upload **does** come there — an
        // implementation that confused the two would write an empty buffer
        // into the texture.
        assert!(
            a.slot(
                Sprite::Char(UNKNOWN_CHAR),
                Face::Regular,
                SizeClass::Normal,
                Half::Whole
            )
            .1
            .is_none(),
            "a resident slot is not uploaded again"
        );
    }

    /// The single-cell acceptance in slot 0 of the colour plane also
    /// answers a `Left` request: the number is the same as `TOFU` but the
    /// record is not a rejection. The guard builds the internal table,
    /// because a colour glyph that fits a single cell does not exist in
    /// today's fonts — the rule is still the path itself.
    #[test]
    fn color_slot_zero_answers_the_left_request() {
        let mut a = atlas(POINT_SIZE, 1.0);
        let sprite = Sprite::Char('😀');
        let whole = (sprite, Face::Regular, SizeClass::Normal, Half::Whole);
        a.slots.insert(whole, (TOFU, Plane::Color));
        let before = (a.occupancy(), a.color_occupancy());
        let (placed, upload) = a.slot(sprite, Face::Regular, SizeClass::Normal, Half::Left);
        assert_eq!(
            (placed.slot, placed.half, placed.plane),
            (TOFU, Half::Whole, Plane::Color),
            "slot 0 of the colour plane was mistaken for a rejection"
        );
        assert!(upload.is_none(), "an accepted glyph was rasterized again");
        assert_eq!((a.occupancy(), a.color_occupancy()), before);
    }

    #[test]
    fn unknown_char_is_cached() {
        let mut a = atlas(POINT_SIZE, 1.0);
        assert_eq!(
            a.slot(
                Sprite::Char(UNKNOWN_CHAR),
                Face::Regular,
                SizeClass::Normal,
                Half::Whole
            )
            .0
            .slot,
            TOFU
        );
        // Persistence of the rejection: CoreText must not be visited on the
        // second ask. The guard looks at the internal table because whether
        // an FFI call happened cannot be observed from outside.
        //
        // With the arrival of the fallback search this claim protects
        // something **more expensive**. The uncached record used to mean one
        // `CTFontGetGlyphsForCharacters` per frame; now a
        // `CTFontCreateForString` is added to it and that walks the cascade.
        // The cost of the cold first call is comparable to the frame budget;
        // its number and environment are held in escrow in
        // `.tasks/019-glyph-yedegi/phase-1.md` → Uygulama Notları (the first
        // `/measure` moves it to `docs/OLCUMLER.md`). The place where the
        // record is born **on the main thread** is `slot()`'s draw path, so
        // this would be not an uncapped leak but a latency paid every frame.
        assert_eq!(
            a.slots.get(&(
                Sprite::Char(UNKNOWN_CHAR),
                Face::Regular,
                SizeClass::Normal,
                Half::Whole,
            )),
            Some(&(TOFU, Plane::Mask)),
            "the tofu resolution must enter the cache"
        );
        assert_eq!(a.occupancy().0, 1, "a tofu fall must not spend a slot");
    }

    // Calibration: names a font or a measured number (042 Karar 7).
    #[cfg(target_os = "macos")]
    #[test]
    fn face_fallback_is_cached_under_the_requested_face() {
        // `╱` (U+2571) was **measured** (this machine, macOS 26.4.1, Menlo
        // 13pt): present in the regular face, absent in bold. So the
        // glyph-level fallback can be fired with a real font.
        //
        // The fixture **has to be a diagonal** and that is no coincidence:
        // the same measurement also counted the code points missing from
        // Menlo Bold and found **one** block across the whole BMP and SMP —
        // U+2500–U+257F, exactly 128 characters. The whole of that block is
        // within 021's scope, only its three diagonals (`╱╲╳`, Karar 3B) are
        // deliberately left out. So what keeps this test's load-bearing
        // claim standing is that hole in the coverage: if the hole were
        // closed, the `DrawResult::NoGlyph if face != Face::Regular` arm
        // would have **no** guard on this machine and the arm would die
        // silently.
        //
        // At one point the fixture was `─` (U+2500) and its doc said "a bold
        // TUI frame goes through this arm"; it no longer does, the frame is
        // drawn procedurally and the `(Char('─'), Bold)` key is never
        // created. Had it not been changed, `bold == regular` and
        // `occupancy == 2` would have stayed green and the test would have
        // lived on testing nothing.
        const FACE_LADDER_PROBE: char = '╱';
        let mut a = atlas(POINT_SIZE, 1.0);
        let regular = a
            .slot(
                Sprite::Char(FACE_LADDER_PROBE),
                Face::Regular,
                SizeClass::Normal,
                Half::Whole,
            )
            .0
            .slot;
        let bold = a
            .slot(
                Sprite::Char(FACE_LADDER_PROBE),
                Face::Bold,
                SizeClass::Normal,
                Half::Whole,
            )
            .0
            .slot;

        // The fallback itself: the bold request must resolve to the regular
        // face's slot, not to tofu, otherwise a frame on a bold line would
        // look like box after box.
        assert_ne!(
            bold, TOFU,
            "a glyph missing from the bold face fell to tofu"
        );
        assert_eq!(
            bold, regular,
            "the fallback must give the regular face's slot"
        );

        // The real guard: the key of the **requested** face is in the map
        // too. Without it this resolution would never enter the cache, and
        // because it is on `slot()`'s draw path, every bold frame cell on
        // screen would be asked of CoreText again **every frame** — on the
        // main thread. Since it cannot be observed from outside, the guard
        // looks at the internal table; same rationale as
        // `unknown_char_is_cached`.
        assert_eq!(
            a.slots.get(&(
                Sprite::Char(FACE_LADDER_PROBE),
                Face::Bold,
                SizeClass::Normal,
                Half::Whole
            )),
            Some(&(regular, Plane::Mask)),
            "the fallback must enter the cache under the requested face's key"
        );
        assert_eq!(a.occupancy().0, 2, "the fallback spent a second slot");
    }

    /// The procedural family's characters — **single source**, the scan is
    /// the whole BMP and the filter is `raster::is_procedural`.
    ///
    /// Writing a narrow range would mirror the implementation's table; a
    /// mirrored table cannot see its own gap (the lesson of 021's arm table).
    fn procedural_chars() -> impl Iterator<Item = char> {
        (0u32..=0xFFFF)
            .filter_map(char::from_u32)
            .filter(|&ch| raster::is_procedural(ch))
    }

    /// The number of slots the family asks of the atlas — in a form
    /// **directly comparable to the capacity**.
    ///
    /// The slots the characters get are not `capacity()`: `Atlas::slot`
    /// gives them `capacity() - RULE_RESERVE` and `next` starts from **1**
    /// because tofu is reserved. So for the family to fit, the capacity has
    /// to exceed the family's size by `1 + RULE_RESERVE`. Not counting these
    /// seven slots would silently loosen the invariant and the guard would
    /// pass green while the last few Braille characters stayed boxes.
    fn procedural_family_size() -> usize {
        procedural_chars().count() + 1 + usize::from(RULE_RESERVE)
    }

    /// The default path must stay **bit for bit the same** (022 R2).
    ///
    /// Deriving the edge only kicks in when the cell grows; the default
    /// point size is already many times above [`SLOT_TARGET`]. Without this
    /// guard, when [`MIN_EDGE`] or [`SLOT_TARGET`] moved, the default user's
    /// grid, texture and **raster** would silently change.
    // Calibration: names a font or a measured number (042 Karar 7).
    #[cfg(target_os = "macos")]
    #[test]
    fn the_default_size_keeps_todays_texture() {
        let a = atlas(POINT_SIZE, 2.0);
        assert_eq!(
            edge_for(a.metrics.cell_px.0, a.metrics.cell_px.1),
            MIN_EDGE,
            "the default point size must stay at the floor"
        );
        // Measured number: `docs/OLCUMLER.md` → Atlas yuva ayak izi, 13pt@2x.
        assert_eq!(a.occupancy().1, 1984, "the 13pt@2x capacity changed");
    }

    /// Invariant: at **every accepted size** the capacity is above the
    /// procedural family (022 R3).
    ///
    /// This guard replaces 021's saturation table: the table was an
    /// observation, this is a contract. Family + tofu = 422 slots
    /// (`docs/OLCUMLER.md`); the number is derived here **not as a constant**
    /// but by counting from `raster::is_procedural`, so if a character is
    /// added to the family the guard tightens by itself.
    #[test]
    fn capacity_clears_the_family_at_every_accepted_size() {
        // **The range is not mirrored:** the scan is the whole BMP and the
        // filter is `raster::is_procedural` itself. A narrow scan range would
        // be a second copy of the implementation's table and when a new
        // block is added to the family (Legacy Computing, U+1FB00–1FBFF — the
        // roadmap's next candidate) the guard would stay green: exactly the
        // silent divergence 021 warned about.
        let family = procedural_family_size();
        // The point × scale product sits in the
        // [`MIN_POINT_SIZE`]..[`MAX_POINT_SIZE`] range, so what sets the
        // corner is the product's ceiling and the line spacing's ceiling.
        // **Two axes.** The cell size comes not only from point/scale/line
        // spacing but also from the **family**, and the user's family is not
        // rejected, only warned about
        // (`proportional_family_opens_with_a_warning`). A single-axis guard
        // would miss, through the second axis, exactly the defect this set
        // turned into a contract.
        for family_name in [None, Some(fixture::PROPORTIONAL_FAMILY)] {
            for point_size in [MIN_POINT_SIZE, 13.0, 29.0, 56.0, MAX_POINT_SIZE] {
                for scale in [1.0, 2.0] {
                    for line_height in [1.0, LARGEST_LINE_HEIGHT] {
                        let a = Atlas::new(family_name, point_size, scale, line_height);
                        let total = a.occupancy().1;
                        assert!(
                            total >= family,
                            "{family_name:?} {point_size}pt@{scale}x \
                             lh={line_height}: capacity {total} < family {family}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn full_atlas_returns_tofu_without_caching() {
        // The corner that gives the smallest capacity: the largest point size
        // **and** the largest line spacing. The edge hits the ceiling
        // ([`MAX_EDGE`]) and stops there, so the capacity bottoms out here.
        let mut a = Atlas::new(None, LARGE_POINT_SIZE, 1.0, LARGEST_LINE_HEIGHT);
        let (used, total) = a.occupancy();
        assert_eq!(used, 1, "only tofu must be reserved in a new atlas");
        // The pool is made of two sets: the procedural family (it **always**
        // spends a slot because it is drawn without asking the font) and
        // printable ASCII. Since the capacity is now derived from the cell
        // size, ASCII alone is not enough — and a character that falls to
        // tofu **spends no slot** (negative cache), so the pool has to be
        // built from characters that can really be drawn. The range table is
        // **not mirrored**, the filter is `raster::is_procedural` itself: a
        // second copy would silently drift.
        // ASCII holds a separate slot in all four faces; the procedural
        // family is normalized to `Regular`, so it is counted **once**
        // (`Atlas::slot`).
        let pool: Vec<(char, Face)> = procedural_chars()
            .map(|ch| (ch, Face::Regular))
            .chain(
                [Face::Regular, Face::Bold, Face::Italic, Face::BoldItalic]
                    .into_iter()
                    .flat_map(|f| (' '..='~').map(move |ch| (ch, f))),
            )
            .collect();
        assert!(
            pool.len() > total,
            "the test pool must exceed the capacity: pool={} capacity={total}",
            pool.len()
        );
        let dropped: Vec<(char, Face)> = pool
            .iter()
            .copied()
            .filter(|&(ch, face)| {
                a.slot(Sprite::Char(ch), face, SizeClass::Normal, Half::Whole)
                    .0
                    .slot
                    == TOFU
            })
            .collect();
        assert!(
            !dropped.is_empty(),
            "tofu is expected once the capacity is exceeded"
        );
        assert_eq!(
            a.occupancy(),
            (total - usize::from(RULE_RESERVE), total),
            "the characters must fill the grid except for the rule reserve"
        );
        // **The reserve itself.** Even after the characters hit the ceiling,
        // a rule sprite still gets a real slot. Without the reserve, from
        // this point on a tofu box would appear instead of a line under every
        // underlined cell, and the symptom would only surface after a long
        // session.
        let rule = a
            .slot(
                Sprite::Rule(RuleKind::Single),
                Face::Regular,
                SizeClass::Normal,
                Half::Whole,
            )
            .0
            .slot;
        assert_ne!(rule, TOFU, "the rule sprite fell to tofu in a full atlas");
        // A full atlas is a transient state: the same character may find a
        // slot at another point size, so they must not stay tied to tofu.
        for (ch, face) in dropped {
            assert!(
                !a.slots
                    .contains_key(&(Sprite::Char(ch), face, SizeClass::Normal, Half::Whole)),
                "'{ch}' ({face:?}) was permanently written to tofu"
            );
        }
    }

    #[test]
    fn glyph_sits_on_the_baseline() {
        // Without this guard, if the y axis were flipped (CG's origin is
        // bottom-**left**) or the baseline miscomputed, every test would stay
        // green: `rasterized_glyph_is_not_empty` only says "there are pixels
        // somewhere". `bt-gpu`'s offscreen gate could not see it either: it
        // says "the inside of the cell is not uniform with the background",
        // not that the letter is in the right place. An inverted baseline
        // would only be seen by eye.
        let mut a = atlas(POINT_SIZE, 1.0);
        let m = a.metrics();
        let (placed__, upload) = a.slot(
            Sprite::Char('W'),
            Face::Regular,
            SizeClass::Normal,
            Half::Whole,
        );
        let _ = placed__.slot;
        let bytes = upload.expect("new slot").bytes;
        let w = usize::from(m.cell_px.0);
        let has_ink = |row: usize| bytes[row * w..(row + 1) * w].iter().any(|&b| b > 0);
        let baseline = usize::from(m.baseline_px);
        // 'W' carries neither a descender nor an accent: all of the coverage is above the baseline.
        assert!(
            (0..baseline).any(has_ink),
            "the area above the baseline is empty: {m:?}"
        );
        assert!(
            !(baseline..usize::from(m.cell_px.1)).any(has_ink),
            "'W' must not spill below the baseline: {m:?}"
        );
    }

    #[test]
    fn line_height_grows_the_cell_and_keeps_the_glyph_centred() {
        // `[font] line_height` (user: "could we open the line spacing up a
        // bit? could it even be a variable?").
        //
        // Three claims and all three can break silently:
        let tight = atlas(POINT_SIZE, 1.0).metrics();
        let airy = Atlas::new(None, POINT_SIZE, 1.0, 1.5).metrics();

        // (1) **Only the height grows.** The width comes from the font's
        //     advance and has nothing to do with line spacing; if it grew,
        //     the monospaced grid would break and the text would spread out.
        assert_eq!(airy.cell_px.0, tight.cell_px.0, "the width grew too");
        assert!(
            airy.cell_px.1 > tight.cell_px.1,
            "the height did not grow: {tight:?} → {airy:?}"
        );

        // (2) **The excess is equal above and below.** As much room as the
        //     baseline drops must open below as well; if it were added to one
        //     side, the text would drift inside the cell and the drift would
        //     grow with the multiplier. `±1`: if the excess is odd, half of
        //     it rounds down.
        let extra = airy.cell_px.1 - tight.cell_px.1;
        let above = airy.baseline_px - tight.baseline_px;
        let below = extra - above;
        assert!(
            above.abs_diff(below) <= 1,
            "the excess was not split evenly: {above} above, {below} below"
        );

        // (3) **The rules drop together with the baseline.** Both are
        //     measured from the baseline; if a separate correction were
        //     added, the underline would detach from the letter as the
        //     multiplier grew.
        assert_eq!(
            airy.underline_px.0 - tight.underline_px.0,
            above,
            "the underline did not drop with the baseline"
        );
        assert_eq!(
            airy.strikeout_px.0 - tight.strikeout_px.0,
            above,
            "the strikeout did not drop with the baseline"
        );

        // (4) **`1.0` is reproducible.** The same quadruple gives the same
        //     metrics; `metrics()` is pure, not tied to hidden state.
        //
        //     This line used to be read as "`1.0` is a no-op" and that claim
        //     was **wrong**: `tight` is also built with `1.0`, so the
        //     comparison was a tautology. Measured at 019's gate — at `1.0`,
        //     `extra = round_up(natural * 0.0)` and `round_up`'s floor is 1,
        //     so the default path **adds one pixel** to the cell (Menlo 13pt:
        //     the font wants 17, the cell becomes 18). The excess falls to
        //     the bottom, the baseline does not move; the symptom is one
        //     pixel too much line spacing. The fix is outside this set and is
        //     a debt in `docs/YOL-HARITASI.md`: it would move every user's
        //     grid, so it is a product decision.
        assert_eq!(Atlas::new(None, POINT_SIZE, 1.0, 1.0).metrics(), tight);
    }

    #[test]
    fn a_taller_line_still_fits_the_descender() {
        // The multiplier version of [`descender_fits_in_the_cell`]: when line
        // spacing opens up, 'g' must not slide toward the bottom. The
        // baseline drops by half the excess, so the room left below also
        // **grows** — the chance of clipping shrinks, not grows. It is still
        // tested: if the arithmetic were built backwards (the whole excess
        // going on top), the bottom room would stay the same and rounding
        // could eat a pixel.
        let mut a = Atlas::new(None, POINT_SIZE, 1.0, 1.5);
        let m = a.metrics();
        let (placed__, upload) = a.slot(
            Sprite::Char('g'),
            Face::Regular,
            SizeClass::Normal,
            Half::Whole,
        );
        let _ = placed__.slot;
        let bytes = upload.expect("new slot").bytes;
        let w = usize::from(m.cell_px.0);
        let has_ink = |row: usize| bytes[row * w..(row + 1) * w].iter().any(|&b| b > 0);
        assert!(
            !has_ink(usize::from(m.cell_px.1) - 1),
            "with open line spacing 'g' leaned against the bottom of the cell"
        );
        // And there is room on top too: the excess did not all go to one side.
        assert!(
            !has_ink(0),
            "with open line spacing the glyph leaned against the top"
        );
    }

    #[test]
    fn descender_fits_in_the_cell() {
        // If the baseline and the height were not rounded **separately**
        // (`round_up(ascent + descent + leading)` in one go), less room than
        // the font's descent would be left at the bottom and the last
        // coverage row of letters like 'g' would be clipped. A clipped glyph
        // fills the cell's last row; a fitting glyph leaves it empty — that
        // is the criterion. Testing with 'W' is not enough: a letter with no
        // descender looks the same under both roundings.
        let mut a = atlas(POINT_SIZE, 1.0);
        let m = a.metrics();
        let (placed__, upload) = a.slot(
            Sprite::Char('g'),
            Face::Regular,
            SizeClass::Normal,
            Half::Whole,
        );
        let _ = placed__.slot;
        let bytes = upload.expect("new slot").bytes;
        let w = usize::from(m.cell_px.0);
        let has_ink = |row: usize| bytes[row * w..(row + 1) * w].iter().any(|&b| b > 0);
        let baseline = usize::from(m.baseline_px);
        assert!(
            has_ink(baseline),
            "'g' must descend below the baseline: {m:?}"
        );
        assert!(
            !has_ink(usize::from(m.cell_px.1) - 1),
            "the descender is clipped on the cell's last row: {m:?}"
        );
    }

    #[test]
    fn negative_cache_is_capped_and_evicted() {
        let mut a = atlas(LARGE_POINT_SIZE, 1.0);
        let cap = a.negative_cache_cap();
        // Let a recognised character take its slot first: a positive record
        // is needed to test that eviction throws away **only** the negative
        // records.
        let (placed_letter, _) = a.slot(
            Sprite::Char('A'),
            Face::Regular,
            SizeClass::Normal,
            Half::Whole,
        );
        let letter = placed_letter.slot;
        assert_ne!(letter, TOFU, "'A' exists in Menlo");

        // An unrecognised character spends no slot, so `next` does not limit
        // it. Without a cap, the map would grow as large as the number of
        // distinct codepoints it sees, and `cat`ing a binary file would turn
        // that into a real path. This was the crate's only number without a
        // cap.
        //
        // **The pool still works after the fallback search**, but each record
        // now pays a `CTFontCreateForString`; the pool's cost was measured
        // and narrowing it was not needed, its number and environment are
        // held in escrow in `.tasks/019-glyph-yedegi/phase-1.md` → Uygulama
        // Notları.
        //
        // The pool is **filtered** and this is not a convenience but a
        // necessity: the gate's answer depends on which font the character
        // falls to. The pool was CJK until 041; when shrinking made that fit
        // a single cell, the pool moved to the sixteenth plane's private-use
        // area (rationale of [`UNKNOWN_CHAR`]: `.LastResort`, not shrunk). The
        // experiment's subject is the negative cache, so only the truly
        // rejected enter the pool; the gate's own guard is
        // [`the_gate_decides_by_ink_alone`] and the filter amounts to asking
        // for its answer.
        let pool: Vec<char> = {
            let (_, base, cell) = size_classes(&a)[0];
            ('\u{100000}'..'\u{10FFFD}')
                .filter(|&ch| rules::fallback_font(base, ch, cell, 1).is_none())
                .take(cap * 3)
                .collect()
        };
        assert!(pool.len() > cap, "the pool must exceed the cap");
        for &ch in &pool {
            assert_eq!(
                a.slot(
                    Sprite::Char(ch),
                    Face::Regular,
                    SizeClass::Normal,
                    Half::Whole
                )
                .0
                .slot,
                TOFU,
                "'{ch}' is not in Menlo/SF Mono and its fallback does not fit the cell"
            );
            assert!(
                a.slots.len() <= cap,
                "the negative cache cap was exceeded: {} > {cap}",
                a.slots.len()
            );
        }
        assert_eq!(a.occupancy().0, 2, "tofu falls must not spend slots");

        // When the cap fills up, caching **does not stop**, eviction happens:
        // a record arriving after the eviction enters the map. Under the old
        // behaviour ("cap full → never write") this would come back empty
        // and every unsupported character on screen would be asked of
        // CoreText again every frame — since `slot()` entered the draw path
        // in this set, the cost would be paid on the main thread.
        let last = *pool.last().expect("the pool is not empty");
        assert_eq!(
            a.slots.get(&(
                Sprite::Char(last),
                Face::Regular,
                SizeClass::Normal,
                Half::Whole
            )),
            Some(&(TOFU, Plane::Mask)),
            "the record after the eviction must enter the cache"
        );
        // A positive record does not take part in eviction: its slot stays.
        assert_eq!(
            a.slot(
                Sprite::Char('A'),
                Face::Regular,
                SizeClass::Normal,
                Half::Whole
            )
            .0
            .slot,
            letter,
            "the positive record was lost in the eviction"
        );
    }

    // Calibration: names a font or a measured number (042 Karar 7).
    #[cfg(target_os = "macos")]
    #[test]
    fn non_bmp_char_path_works() {
        // Surrogate pair: `encode_utf16` produces two units, CoreText
        // touches the second unit too and returns `false` without producing
        // a glyph. `FontSystem::glyph` (CoreText) deliberately ignores that
        // return and derives its pointers from the slice; the rationale for
        // both is tested only if this path runs.
        //
        // With the arrival of the fallback search the non-BMP path goes
        // through **two places** and the second is new: `rules::fallback_font`'s
        // `CFRange` also counts UTF-16 units, so if `1` were written instead
        // of `len_utf16`, half of the surrogate pair would be requested and
        // the cascade would look up the wrong character. This test is now
        // that range's guard too. The candidate (STIX Two Math) is found,
        // returns from the ink gate (1.07×) and since 041 is drawn shrunk.
        let mut a = atlas(POINT_SIZE, 1.0);
        assert_ne!(
            a.slot(
                Sprite::Char('𝔸'),
                Face::Regular,
                SizeClass::Normal,
                Half::Whole
            )
            .0
            .slot,
            TOFU,
            "Menlo/SF Mono contain no mathematical alphabet; its fallback must be drawn shrunk"
        );
        // If a non-BMP character **can** pass the gate, the same range must
        // also be right on the draw path; `\u{10FFFD}` (.LastResort, 1.83×)
        // and `𝔸` are the two ends of the same arm and both **find** a
        // candidate — if the range were broken no candidate would be found
        // and this test would still stay green.
        let (label, base, _) = size_classes(&a)[0];
        assert!(
            rules::fallback_font(base, '𝔸', f64::INFINITY, 1).is_some(),
            "{label}: no candidate was found for the non-BMP character — the cascade's UTF-16 range is suspect"
        );
    }

    #[test]
    fn broken_point_size_does_not_break_atlas() {
        // `NaN as u16` is zero and `clamp` lets NaN through: without a bound
        // the grid would divide by zero. A huge point size, meanwhile, would
        // ask for a gigabyte-sized buffer per slot.
        for (point_size, scale) in [(f64::NAN, 1.0), (13.0, f64::INFINITY), (1e9, 1.0)] {
            let a = atlas(point_size, scale);
            let m = a.metrics();
            assert!(
                m.cell_px.0 > 0 && m.cell_px.1 > 0,
                "{point_size}×{scale}: {m:?}"
            );
            let (tw, th) = a.texture_px();
            // The ceiling is now [`MAX_EDGE`]: the edge can double toward the
            // target but stops there. Since extreme inputs (NaN, infinity,
            // 1e9) fit the point size into the range, a finite texture must
            // land here too.
            assert!(
                tw <= MAX_EDGE && th <= MAX_EDGE,
                "{point_size}×{scale}: {tw}×{th}"
            );
        }
    }

    #[test]
    fn scale_is_part_of_the_key() {
        let one = atlas(POINT_SIZE, 1.0).metrics();
        let two = atlas(POINT_SIZE, 2.0).metrics();
        assert_ne!(
            one.cell_px, two.cell_px,
            "an @2x cell cannot equal an @1x one"
        );
        // Exactly double is not expected: each measure is rounded up separately.
        assert!(
            two.cell_px.0 + 2 >= one.cell_px.0 * 2 && two.cell_px.0 <= one.cell_px.0 * 2 + 2,
            "the @2x width must be close to double: {one:?} → {two:?}"
        );
    }

    #[test]
    fn ensure_rebuilds_only_when_key_changes() {
        let mut a = atlas(POINT_SIZE, 1.0);
        a.slot(
            Sprite::Char('A'),
            Face::Regular,
            SizeClass::Normal,
            Half::Whole,
        );
        assert!(
            !a.ensure(None, POINT_SIZE, 1.0, 1.0),
            "the same key must not rebuild"
        );
        assert_eq!(a.occupancy().0, 2, "slots must be kept");
        assert!(
            a.ensure(None, POINT_SIZE, 2.0, 1.0),
            "the scale changed: it must rebuild"
        );
        assert_eq!(a.occupancy().0, 1, "only tofu in a new atlas");
        assert_eq!(a.metrics(), atlas(POINT_SIZE, 2.0).metrics());
    }

    #[test]
    fn ensure_rebuilds_when_family_changes() {
        // Monaco and Menlo can give the same cell at 13pt; the criterion is
        // not the metrics but the slots being reset. If the family were not
        // in the key, the old font's glyphs would stay in the new font's
        // atlas and the symptom would be silent.
        let mut a = atlas(POINT_SIZE, 1.0);
        a.slot(
            Sprite::Char('A'),
            Face::Regular,
            SizeClass::Normal,
            Half::Whole,
        );
        assert!(
            !a.ensure(None, POINT_SIZE, 1.0, 1.0),
            "the same key must not rebuild"
        );
        assert!(
            a.ensure(Some(fixture::SECOND_FAMILY), POINT_SIZE, 1.0, 1.0),
            "the family changed: it must rebuild"
        );
        assert_eq!(a.occupancy().0, 1, "only tofu in a new atlas");
        a.slot(
            Sprite::Char('A'),
            Face::Regular,
            SizeClass::Normal,
            Half::Whole,
        );
        assert!(
            !a.ensure(Some(fixture::SECOND_FAMILY), POINT_SIZE, 1.0, 1.0),
            "the same family must not rebuild"
        );
        assert_eq!(a.occupancy().0, 2, "slots must be kept");
        assert!(
            a.ensure(None, POINT_SIZE, 1.0, 1.0),
            "going back to the chain is also a change"
        );
    }

    #[test]
    fn missing_family_opens_the_chain_and_says_so() {
        let a = Atlas::new(Some(MISSING_FAMILY), POINT_SIZE, 1.0, 1.0);
        let (_, chain) = Backend::open_default(POINT_SIZE);
        assert_eq!(
            a.font_issue(),
            Some(&FontIssue::FamilyNotFound {
                requested: MISSING_FAMILY.to_owned(),
                using: chain,
            })
        );
        // The chain was opened, not CoreText's substitute (Helvetica on this machine).
        assert_eq!(a.metrics(), atlas(POINT_SIZE, 1.0).metrics());
        assert_eq!(
            atlas(POINT_SIZE, 1.0).font_issue(),
            None,
            "the chain is silent"
        );
    }

    #[test]
    fn family_name_is_matched_regardless_of_case() {
        // CoreText finds `"menlo"` and reports the name as `"Menlo"`
        // (measured); an exact comparison would count the font it found as
        // "missing" and fall to the chain.
        let family = fixture::DEFAULT_FAMILY;
        for name in [
            family.to_owned(),
            family.to_lowercase(),
            family.to_uppercase(),
        ] {
            let a = Atlas::new(Some(&name), POINT_SIZE, 1.0, 1.0);
            assert_eq!(a.font_issue(), None, "{name}");
        }
    }

    #[test]
    fn proportional_family_opens_with_a_warning() {
        // Helvetica exists on every macOS and is not monospaced.
        let mut a = Atlas::new(Some(fixture::PROPORTIONAL_FAMILY), POINT_SIZE, 1.0, 1.0);
        assert_eq!(
            a.font_issue(),
            Some(&FontIssue::NotMonospaced {
                family: fixture::PROPORTIONAL_FAMILY.to_owned()
            })
        );
        // It is not rejected, it is drawn: a 'W' wider than the cell is
        // rasterized into the slot clipped, the buffer does not overflow.
        let slot_len = a.metrics().slot_bytes();
        let bytes = slot_bytes_of(&mut a, Sprite::Char('W'), Face::Regular);
        assert_eq!(bytes.len(), slot_len);
        assert!(bytes.iter().any(|&b| b > 0), "'W' painted no pixels at all");
    }

    #[test]
    fn monospaced_families_are_the_ones_the_chain_accepts() {
        // The settings window's Font list: every selectable family opens
        // from the chain without a warning — the criterion is the same as
        // `open_chain`'s.
        let families = monospaced_families();
        assert!(
            families.iter().any(|f| f == fixture::DEFAULT_FAMILY),
            "{families:?}"
        );
        assert!(
            !families.iter().any(|f| f == fixture::PROPORTIONAL_FAMILY),
            "{families:?}"
        );
        assert!(!families.iter().any(|f| f.starts_with('.')), "{families:?}");
        assert!(
            families
                .windows(2)
                .all(|pair| pair[0].to_lowercase() <= pair[1].to_lowercase()),
            "{families:?}"
        );
        for family in &families {
            let (_, issue) = rules::open_chain(Some(family), POINT_SIZE);
            assert_eq!(issue, None, "{family}");
        }
    }

    #[test]
    fn missing_family_is_substituted() {
        // CoreText does not error, it hands back the nearest font: "the font
        // opened" is not proof, which is why the chain compares the name
        // that comes back.
        const MISSING: &str = "Bu Aile Yok 12345";
        let (_, returned) = Backend::open(MISSING, POINT_SIZE);
        assert_ne!(
            returned, MISSING,
            "a substitute is expected for a nonexistent family"
        );
    }

    #[test]
    fn slot_origin_walks_the_grid() {
        let a = atlas(POINT_SIZE, 1.0);
        let (w, h) = a.metrics().cell_px;
        let cols = a.grid.0;
        assert_eq!(a.slot_origin(TOFU), (0, 0));
        assert_eq!(a.slot_origin(1), (w, 0));
        assert_eq!(
            a.slot_origin(cols),
            (0, h),
            "the first slot drops one row down"
        );
        // The texture must wrap the grid and no more than one cell may be
        // wasted at the edge. The edge is now derived, so what is checked is
        // **the grid's own edge**, not a constant: the product of the
        // row/column count and the cell size must give the texture, and
        // adding one more cell must exceed the edge. The old test tied the
        // texture to the constant `TEXTURE_EDGE`; now that the edge is
        // derived, the place to tie it to is `edge_for`. **This is the real
        // bound**: if the texture exceeded the derived edge, `slot_origin`
        // would point past the last column and `replaceRegion` would write
        // outside the row — the symptom is silent.
        let (tw, th) = a.texture_px();
        let edge = edge_for(w, h);
        assert!(
            tw <= edge && th <= edge,
            "the texture exceeds the edge: {tw}×{th} > {edge}"
        );
        // And no more than one cell is wasted.
        assert!(
            tw + w > edge && th + h > edge,
            "the leftover strip is bigger than one cell: {tw}×{th}, edge {edge}"
        );
    }

    /// Copies a slot's bytes — the `Upload` borrow locks the atlas.
    fn slot_bytes_of(a: &mut Atlas, sprite: Sprite, face: Face) -> Vec<u8> {
        let (_, upload) = a.slot(sprite, face, SizeClass::Normal, Half::Whole);
        upload
            .expect("a new slot must give an upload")
            .bytes
            .to_vec()
    }

    #[test]
    fn bold_face_gets_own_slot() {
        let mut a = atlas(POINT_SIZE, 1.0);
        let mut slots = Vec::new();
        // Only the faces the family really has: a missing face collapses to
        // the regular one **by design** (`Faces::effective`) and shares its
        // slot. On macOS Menlo has all four; the Linux image's DejaVu Sans
        // Mono has no italic (042 phase-4).
        let faces: Vec<Face> = [Face::Regular, Face::Bold, Face::Italic, Face::BoldItalic]
            .into_iter()
            .filter(|&face| a.faces.effective(face) == face)
            .collect();
        assert!(
            faces.contains(&Face::Bold),
            "the base family has no bold face"
        );
        for &face in &faces {
            let slot = a
                .slot(Sprite::Char('M'), face, SizeClass::Normal, Half::Whole)
                .0
                .slot;
            assert_ne!(slot, TOFU, "{face:?} fell to tofu");
            // If the key did not carry the face, all four would share the
            // same slot and a bold 'M' would be drawn as a regular 'M' —
            // silent, because something would still be visible.
            assert!(
                !slots.contains(&slot),
                "{face:?} shared another face's slot"
            );
            slots.push(slot);
        }
        assert_eq!(slots.len(), faces.len());
    }

    /// The remote session's mark (`bt_core::dock::REMOTE_MARK`, 036 Karar 7).
    ///
    /// **A second copy, deliberately**: this crate does not see `bt-core`
    /// (layer direction), so the character is written by hand here. The
    /// link is in `bt-core`'s `the_remote_mark_is_the_one_the_atlas_checks`
    /// test: if the mark changes, that one fails and points to this constant.
    #[cfg(target_os = "macos")]
    const REMOTE_MARK: char = '⇄';

    // Calibration: names a font or a measured number (042 Karar 7).
    #[cfg(target_os = "macos")]
    #[test]
    fn the_remote_mark_is_a_glyph_in_the_small_class() {
        // The mark is an ordinary cell of the context line and the context
        // line is in the small size class (the procedural gate is closed
        // there, the fallback is open). The gate is **Menlo, by name**: the
        // default chain varies from machine to machine (if SF Mono is
        // installed it opens), the measured font is Menlo.
        let slot_of = |a: &mut Atlas| {
            a.slot(
                Sprite::Char(REMOTE_MARK),
                Face::Regular,
                SizeClass::Small,
                Half::Whole,
            )
            .0
            .slot
        };
        let mut menlo = Atlas::new(Some("Menlo"), POINT_SIZE, 1.0, 1.0);
        assert_eq!(menlo.font_issue(), None, "Menlo did not open");
        assert_ne!(
            slot_of(&mut menlo),
            TOFU,
            "'{REMOTE_MARK}' is a box in Menlo's small class"
        );

        // SF Mono is **not a gate**: if it is not installed the query is
        // skipped and says so; if it is installed, the result is the
        // subject of the visual check, here it is only printed.
        let mut sf = Atlas::new(Some("SF Mono"), POINT_SIZE, 1.0, 1.0);
        if sf.font_issue().is_some() {
            eprintln!("SF Mono is not installed; the '{REMOTE_MARK}' query was skipped");
        } else {
            let tofu = slot_of(&mut sf) == TOFU;
            eprintln!("SF Mono '{REMOTE_MARK}' in the small class: box = {tofu}");
        }
    }

    // Calibration: names a font or a measured number (042 Karar 7).
    #[cfg(target_os = "macos")]
    #[test]
    fn the_reconnect_placeholder_has_no_box_in_the_normal_class() {
        // 037 Karar 8: the reconnect offer's placeholder is **in the input
        // line**, i.e. in the large class — 036's test only asked the small
        // class (the context line) and the face ladder is different there.
        // The three non-ASCII characters too: the mark, the separator and ⏎.
        // If a box appears, the string must change in `bt-core`
        // (`dock::RECONNECT_HINT`). Menlo, by name.
        let mut menlo = Atlas::new(Some("Menlo"), POINT_SIZE, 1.0, 1.0);
        assert_eq!(menlo.font_issue(), None, "Menlo did not open");
        for ch in [REMOTE_MARK, '·', '⏎'] {
            let slot = menlo
                .slot(
                    Sprite::Char(ch),
                    Face::Regular,
                    SizeClass::Normal,
                    Half::Whole,
                )
                .0
                .slot;
            assert_ne!(slot, TOFU, "'{ch}' is a box in Menlo's large class");
        }
    }

    // Calibration: names a font or a measured number (042 Karar 7).
    #[cfg(target_os = "macos")]
    #[test]
    fn the_upload_row_has_no_box_in_the_small_class() {
        // 037 Karar 7: the upload's status line is in the context line, i.e.
        // in the small class. The characters are a hand copy of `bt-core`'s
        // `UPLOAD_GLYPHS` (this crate cannot see it;
        // `the_upload_row_is_the_one_the_atlas_checks` links them). If a box
        // appears, the string must change in `bt-shell`. Menlo, by name.
        let mut menlo = Atlas::new(Some("Menlo"), POINT_SIZE, 1.0, 1.0);
        assert_eq!(menlo.font_issue(), None, "Menlo did not open");
        for ch in ['↑', '↓', '⌘', '✓', '—', '·', '…', '→'] {
            let slot = menlo
                .slot(
                    Sprite::Char(ch),
                    Face::Regular,
                    SizeClass::Small,
                    Half::Whole,
                )
                .0
                .slot;
            assert_ne!(slot, TOFU, "'{ch}' is a box in Menlo's small class");
        }
    }

    #[test]
    fn the_small_class_is_narrower_and_keeps_its_own_slot() {
        let mut a = atlas(POINT_SIZE, 1.0);

        // **The slot grid is shared**: the small glyph is drawn into the
        // large slot, on the large cell's baseline. The texture size,
        // `slot_bytes` and the grid therefore never change — all of the
        // cheapness comes from here.
        let big = slot_bytes_of(&mut a, Sprite::Char('M'), Face::Regular);
        let (placed_small_slot, small_upload) = a.slot(
            Sprite::Char('M'),
            Face::Regular,
            SizeClass::Small,
            Half::Whole,
        );
        let small_slot = placed_small_slot.slot;
        let small = small_upload
            .expect("a new slot must give an upload")
            .bytes
            .to_vec();
        assert_eq!(
            big.len(),
            small.len(),
            "the small class moved the slot size"
        );

        // A separate slot: if the key did not carry the size, the small 'M'
        // would be drawn as the large 'M' — silent, because something would
        // still be visible.
        assert_ne!(small_slot, TOFU, "the small class fell to tofu");
        assert_ne!(
            small_slot,
            a.slot(
                Sprite::Char('M'),
                Face::Regular,
                SizeClass::Normal,
                Half::Whole
            )
            .0
            .slot,
            "the small class shared the regular face's slot"
        );

        // **The letter really is small.** The criterion is the coverage's
        // rightmost column: the small face must leave a narrower trace in
        // the same slot. The bound is tested, not individual pixel values —
        // the coverage depends on the font version, so it is not a claim.
        let ink_right = |bytes: &[u8]| {
            let (w, h) = a.metrics().cell_wh();
            (0..w)
                .rev()
                .find(|&x| (0..h).any(|y| bytes[y * w + x] > 0))
                .map(|x| x + 1)
                .unwrap_or(0)
        };
        assert!(
            ink_right(&small) < ink_right(&big),
            "the small class is not narrow: small {}, large {}",
            ink_right(&small),
            ink_right(&big)
        );

        // Rule sprites are **independent of size**: there is no rule in the
        // context line and `slot` normalizes this together with the face.
        assert_eq!(
            a.slot(
                Sprite::Rule(RuleKind::Single),
                Face::Regular,
                SizeClass::Small,
                Half::Whole
            )
            .0
            .slot,
            a.slot(
                Sprite::Rule(RuleKind::Single),
                Face::Regular,
                SizeClass::Normal,
                Half::Whole
            )
            .0
            .slot,
        );
    }

    #[test]
    fn bold_glyph_fits_regular_face_slot() {
        // The metrics come only from the regular face (R1.3); the bold glyph
        // is rasterized into the same slot. Clipping is an accepted cost,
        // but the slot **not overflowing** is the contract: `raster::draw`
        // asserts the buffer's length.
        let mut a = atlas(POINT_SIZE, 1.0);
        let bytes = slot_bytes_of(&mut a, Sprite::Char('M'), Face::Bold);
        assert_eq!(bytes.len(), a.metrics().slot_bytes());
        assert!(
            bytes.iter().any(|&b| b > 0),
            "the bold 'M' gave no ink at all"
        );
    }

    #[test]
    fn rule_sprites_are_not_empty_and_differ() {
        let mut a = atlas(POINT_SIZE, 1.0);
        let mut seen: Vec<(RuleKind, Vec<u8>)> = Vec::new();
        for kind in [
            RuleKind::Single,
            RuleKind::Double,
            RuleKind::Curl,
            RuleKind::Dotted,
            RuleKind::Dashed,
            RuleKind::Strike,
            RuleKind::Chevron,
        ] {
            let bytes = slot_bytes_of(&mut a, Sprite::Rule(kind), Face::Regular);
            assert!(
                bytes.iter().any(|&b| b > 0),
                "{kind:?} painted no pixels at all"
            );
            for (prev_kind, prev_bytes) in &seen {
                // **Telling the five styles apart** is this test's job. Code
                // that collapsed them all to a plain line would pass the
                // `rules=R` token.
                assert_ne!(
                    prev_bytes, &bytes,
                    "{kind:?} and {prev_kind:?} were drawn the same"
                );
            }
            seen.push((kind, bytes));
        }
    }

    #[test]
    fn rule_keeps_one_slot_regardless_of_face() {
        let mut a = atlas(POINT_SIZE, 1.0);
        let regular = a
            .slot(
                Sprite::Rule(RuleKind::Single),
                Face::Regular,
                SizeClass::Normal,
                Half::Whole,
            )
            .0
            .slot;
        // Even if the caller errs and passes a face, normalization leads to
        // the same slot; otherwise six kinds with four faces would spend
        // twenty-four slots.
        let bold = a
            .slot(
                Sprite::Rule(RuleKind::Single),
                Face::BoldItalic,
                SizeClass::Normal,
                Half::Whole,
            )
            .0
            .slot;
        assert_eq!(regular, bold, "the rule held a separate slot per face");
    }

    #[test]
    fn the_chevron_points_right_and_sits_on_the_x_height() {
        // **The mark is the terminal's own, not the font's** (012 phase-9): a
        // procedural chevron instead of the `>` character. Three claims, and
        // all three can break silently.
        let mut a = atlas(POINT_SIZE, 1.0);
        let m = a.metrics();
        let (w, h) = (usize::from(m.cell_px.0), usize::from(m.cell_px.1));
        let bytes = slot_bytes_of(&mut a, Sprite::Rule(RuleKind::Chevron), Face::Regular);

        // The rightmost painted column of each row: since the chevron opens
        // to the right, this series must rise toward the middle and then
        // fall — the apex is in the middle.
        let rights: Vec<Option<usize>> = (0..h)
            .map(|y| (0..w).rev().find(|&x| bytes[y * w + x] > 0))
            .collect();
        let apex_row = rights
            .iter()
            .enumerate()
            .filter_map(|(y, right)| right.map(|x| (x, y)))
            .max()
            .expect("the chevron painted no pixels")
            .1;

        // **The vertical centre is the strikeout's centre**, i.e. the middle
        // of the x-height: the cell's geometric centre falls below the
        // baseline and the mark would look low relative to the text.
        let center = usize::from(m.strikeout_px.0) + usize::from(m.strikeout_px.1) / 2;
        assert!(
            apex_row.abs_diff(center) <= 1,
            "the apex is not at the x-height centre: {apex_row} / {center}"
        );

        // **The ink gathers in the middle of the cell.** In the grid the mark
        // is drawn inside the left gutter and the gutter can be narrower than
        // a cell; if it spilled over, it would land on the first letter of
        // the command text.
        let painted: Vec<usize> = (0..w)
            .filter(|&x| (0..h).any(|y| bytes[y * w + x] > 0))
            .collect();
        let (left, right) = (painted[0], painted[painted.len() - 1]);
        assert!(left > 0, "the chevron stuck to the left edge: {left}");
        assert!(
            right < w - 1,
            "the chevron stuck to the right edge: {right}"
        );

        // And symmetric: the two arms of the `>` mark are the same.
        let above = (0..center).filter(|&y| rights[y].is_some()).count();
        let below = (center + 1..h).filter(|&y| rights[y].is_some()).count();
        assert!(
            above.abs_diff(below) <= 1,
            "the arms are not symmetric: {above} / {below}"
        );
    }

    #[test]
    fn curl_is_really_a_wave() {
        let mut a = atlas(POINT_SIZE, 1.0);
        let m = a.metrics();
        let (w, h) = (usize::from(m.cell_px.0), usize::from(m.cell_px.1));
        let bytes = slot_bytes_of(&mut a, Sprite::Rule(RuleKind::Curl), Face::Regular);
        // The topmost painted row of each column; the wave must move these.
        let tops: Vec<usize> = (0..w)
            .filter_map(|x| (0..h).find(|&y| bytes[y * w + x] > 0))
            .collect();
        assert_eq!(tops.len(), w, "the curl did not paint some columns at all");
        let (min_top, max_top) = (
            *tops.iter().min().expect("there is a column"),
            *tops.iter().max().expect("there is a column"),
        );
        // On a plain line this difference is **zero**. Code that collapsed
        // the curl to a plain line fails right here — and the `rules=R`
        // token cannot see it.
        assert!(
            max_top - min_top >= 1,
            "the curl does not oscillate: the top row is constant between {min_top}..{max_top}"
        );
    }

    #[test]
    fn curl_is_continuous_across_cell_edges() {
        let mut a = atlas(POINT_SIZE, 1.0);
        let m = a.metrics();
        let (w, h) = m.cell_wh();
        let bytes = slot_bytes_of(&mut a, Sprite::Rule(RuleKind::Curl), Face::Regular);
        let tops: Vec<usize> = (0..w)
            .filter_map(|x| (0..h).find(|&y| bytes[y * w + x] > 0))
            .collect();
        // If a **whole** number of waves fits in the cell (R2.4), the sine is
        // mirror-symmetric about the middle axis: `center(x) + center(w-1-x)`
        // is constant. If it does not fit, the phase breaks at the cell
        // boundary and a multi-cell underline looks interrupted — the sprite
        // is one cell wide and is tiled with its neighbours.
        //
        // Expecting the edge columns to be **equal** would be wrong: over one
        // full period the first and last columns are not equal, they are
        // MIRRORS about the middle axis.
        let total = tops[0] + tops[w - 1];
        for x in 0..w {
            let pair = tops[x] + tops[w - 1 - x];
            assert!(
                pair.abs_diff(total) <= 1,
                "the wave period does not divide the cell exactly: x={x} pair {pair}, edge pair {total}"
            );
        }
    }

    #[test]
    fn envelope_stays_inside_cell() {
        // **Synthetic input, deliberately**: Menlo on this machine puts the
        // underline at 14+1 with a cell of 17 — so the clipping branch never
        // fires with a real font and a test written from there could not
        // catch a mutation. `rules::rule_envelope`'s contract is tested here
        // as pure arithmetic.
        for (top, thick, h) in [(100u16, 3u16, 17u16), (16, 4, 17), (0, 99, 17)] {
            let (position, thickness) = rules::rule_envelope(top, thick, h);
            assert!(
                position + thickness <= h,
                "the envelope exceeded the cell: input ({top},{thick},{h}) → ({position},{thickness})"
            );
            assert!(
                thickness >= 1,
                "the thickness dropped to zero: a line that is not drawn is not a rule"
            );
        }
    }

    #[test]
    fn rule_envelope_fits_cell_with_real_font() {
        for point_size in [POINT_SIZE, LARGE_POINT_SIZE] {
            let m = atlas(point_size, 2.0).metrics();
            let h = m.cell_px.1;
            assert!(
                m.underline_px.0 + m.underline_px.1 <= h,
                "the underline spilled over at {point_size}pt"
            );
            assert!(
                m.strikeout_px.0 + m.strikeout_px.1 <= h,
                "the strikeout spilled over at {point_size}pt"
            );
        }
    }
    // Calibration: names a font or a measured number (042 Karar 7).
    #[cfg(target_os = "macos")]
    #[test]
    fn missing_face_falls_back_to_regular() {
        // Monaco is **single-faced**: on this machine Bold/Italic/BoldItalic
        // cannot all three be derived. Since the chain's base (Menlo) carries
        // all four, the fallback branch can be fired only with such a family
        // — `Faces::derive` exists as a separate constructor for exactly this.
        let (monaco, name) = Backend::open("Monaco", POINT_SIZE);
        assert_eq!(
            name, "Monaco",
            "Monaco is not on the machine; the test's premise fell"
        );
        let faces = rules::Faces::derive(monaco);
        for face in [Face::Bold, Face::Italic, Face::BoldItalic] {
            // The face collapses to the regular face AND the key collapses
            // too: otherwise the same bitmap would spend four slots.
            assert_eq!(
                faces.effective(face),
                Face::Regular,
                "the {face:?} key did not collapse"
            );
        }
        // There is no collapse in Menlo — the test itself must also be able to see the difference.
        let menlo = rules::Faces::derive(Backend::open("Menlo", POINT_SIZE).0);
        assert_eq!(
            menlo.effective(Face::Bold),
            Face::Bold,
            "Menlo's bold face collapsed"
        );
    }

    /// The (point size, scale) pairs the procedural invariants run at.
    ///
    /// All three are needed and each opens a different arithmetic (the
    /// measures are on this machine, Menlo): the 13pt@1x cell is 8×18 —
    /// eighth slices land fractional (18/8 = 2.25) and anti-aliasing really
    /// runs; 13pt@2x is 16×33, i.e. an **odd** height, and halves fall to a
    /// fraction too; 144pt@1x is 87×169, a large cell where most slices
    /// divide evenly. An invariant that runs at only one pair has never
    /// tested the others — the lesson in `envelope_stays_inside_cell`'s doc
    /// ("the clipping branch never fires with a real font").
    const PROCEDURAL_SIZES: [(f64, f64); 3] = [
        (POINT_SIZE, 1.0),
        (POINT_SIZE, 2.0),
        (LARGE_POINT_SIZE, 1.0),
    ];

    /// The bytes of a procedural sprite — **not** from `Atlas::slot`, but directly.
    ///
    /// At `LARGE_POINT_SIZE` the capacity is a few dozen slots and the 256
    /// Braille patterns alone do not fit there: an invariant test running
    /// through `slot()` would fall to tofu, no `Upload` would ever arrive
    /// and the guard would have tested the capacity, not the geometry. The
    /// guards that show the gate really running on `slot()`'s path are
    /// separate and at 13pt
    /// ([`procedural_chars_share_one_slot_across_faces`],
    /// [`the_small_class_still_asks_the_font`]).
    fn procedural(m: Metrics, ch: char) -> Vec<u8> {
        let mut bytes = vec![0u8; m.slot_bytes()];
        raster::draw_procedural(ch, m, &mut bytes);
        bytes
    }

    /// The pixel-by-pixel saturating sum of two sprites.
    fn saturating_sum(a: &[u8], b: &[u8]) -> Vec<u8> {
        a.iter().zip(b).map(|(x, y)| x.saturating_add(*y)).collect()
    }

    /// The pixel-max of two sprites.
    fn pixel_max(a: &[u8], b: &[u8]) -> Vec<u8> {
        a.iter().zip(b).map(|(x, y)| *x.max(y)).collect()
    }

    #[test]
    fn the_full_block_fills_the_cell() {
        // **The exact opposite of the reported defect**, equality and not `> 0`:
        // Menlo's `█` paints only rows 3-16 of an 8×18 cell, which left a ~5 pixel
        // strip between two stacked blocks (019 phase-2, reported by the user with
        // a screenshot). A single missing byte is a faint copy of that strip, so
        // the criterion cannot be "is there any ink at all".
        for (point_size, scale) in PROCEDURAL_SIZES {
            let m = atlas(point_size, scale).metrics();
            let bytes = procedural(m, '\u{2588}');
            let (w, _) = m.cell_wh();
            let gap = bytes.iter().position(|&b| b != 255);
            assert!(
                gap.is_none(),
                "{point_size}pt@{scale}x: `█` did not fill the cell, first missing pixel \
                 ({}, {}) = {}",
                gap.unwrap_or(0) % w,
                gap.unwrap_or(0) / w,
                bytes[gap.unwrap_or(0)]
            );
        }
    }

    #[test]
    fn disjoint_blocks_tile_the_cell() {
        // The union of the disjoint parts must give **full** coverage, and the
        // criterion is the saturating sum: at h = 33 of 13pt@2x the half falls on
        // 16.5, and the two neighbouring parts leave 128 each on that row. `max`
        // would have left a 50% strip in the **middle** of the cell — the defect
        // this set came to close, moved inside the cell, and a guard testing `> 0`
        // would not have seen it.
        for (point_size, scale) in PROCEDURAL_SIZES {
            let m = atlas(point_size, scale).metrics();
            let full = procedural(m, '\u{2588}');
            for (a, b, name) in [
                ('\u{2580}', '\u{2584}', "upper/lower half"),
                ('\u{258C}', '\u{2590}', "left/right half"),
            ] {
                assert_eq!(
                    saturating_sum(&procedural(m, a), &procedural(m, b)),
                    full,
                    "{point_size}pt@{scale}x: {name} did not give `█`"
                );
            }
            // The four quadrants carry the same law and also see the **middle
            // seam**: fractional horizontal and vertical row/column in one frame.
            let quarters = ['\u{2598}', '\u{259D}', '\u{2596}', '\u{2597}'];
            let union = quarters.iter().fold(vec![0u8; m.slot_bytes()], |acc, &ch| {
                saturating_sum(&acc, &procedural(m, ch))
            });
            assert_eq!(
                union, full,
                "{point_size}pt@{scale}x: the four quadrants did not give `█`"
            );
        }
    }

    #[test]
    fn the_eighth_ladders_are_nested() {
        // Two ladders, two directions: from the bottom `▁..█` grows **as** the
        // code point **increases**, from the left `▉..▏` grows as the code point
        // **decreases**. The second direction is Unicode's own ordering and a sign
        // error there would stay silent — the ladder still looks like a ladder,
        // only reversed.
        for (point_size, scale) in PROCEDURAL_SIZES {
            let m = atlas(point_size, scale).metrics();
            let full = procedural(m, '\u{2588}');
            for (name, steps) in [
                ("bottom", (0x2581..=0x2588).collect::<Vec<u32>>()),
                ("left", (0x2589..=0x258F).rev().collect::<Vec<u32>>()),
            ] {
                let mut previous = vec![0u8; m.slot_bytes()];
                let mut previous_ink = 0u64;
                for cp in steps {
                    let ch = char::from_u32(cp).expect("block code point");
                    let bytes = procedural(m, ch);
                    // **Nested**: each step covers the one before it.
                    assert!(
                        bytes.iter().zip(&previous).all(|(b, p)| b >= p),
                        "{point_size}pt@{scale}x: the {name} ladder went backwards at U+{cp:04X}"
                    );
                    // And it really **grows**: a code drawing all of them the
                    // same would pass the nesting test.
                    let ink: u64 = bytes.iter().map(|&b| u64::from(b)).sum();
                    assert!(
                        ink > previous_ink,
                        "{point_size}pt@{scale}x: the {name} ladder did not grow at U+{cp:04X} \
                         ({previous_ink} → {ink})"
                    );
                    previous = bytes;
                    previous_ink = ink;
                }
                // The ladder's last step is the full block: the left ladder's
                // last step is `▉` at seven eighths, not full, because
                // `2589..=258F` is walked in reverse — so only the bottom
                // ladder is compared.
                if name == "bottom" {
                    assert_eq!(
                        previous, full,
                        "{point_size}pt@{scale}x: the bottom ladder did not reach `█`"
                    );
                }
            }
        }
    }

    /// Unicode names of the quadrant and one-eighth blocks
    /// (`unicodedata`, UCD 16.0), the oracle for `raster::QUADRANTS`.
    ///
    /// Same reason as the line family ([`LINE_NAMES`]): this too is a
    /// hand-written table and no invariant looking at geometry can see "right
    /// geometry, wrong character" — if the masks of `▙` and `▟` were swapped
    /// the union of the four quadrants would still be `█`.
    // `rustfmt::skip`: the aligned name comments are the only reason the
    // table can be scanned by eye.
    #[rustfmt::skip]
    const QUARTER_NAMES: [(char, &str); 12] = [
        ('▔', "UPPER ONE EIGHTH BLOCK"),
        ('▕', "RIGHT ONE EIGHTH BLOCK"),
        ('▖', "QUADRANT LOWER LEFT"),
        ('▗', "QUADRANT LOWER RIGHT"),
        ('▘', "QUADRANT UPPER LEFT"),
        ('▙', "QUADRANT UPPER LEFT AND LOWER LEFT AND LOWER RIGHT"),
        ('▚', "QUADRANT UPPER LEFT AND LOWER RIGHT"),
        ('▛', "QUADRANT UPPER LEFT AND UPPER RIGHT AND LOWER LEFT"),
        ('▜', "QUADRANT UPPER LEFT AND UPPER RIGHT AND LOWER RIGHT"),
        ('▝', "QUADRANT UPPER RIGHT"),
        ('▞', "QUADRANT UPPER RIGHT AND LOWER LEFT"),
        ('▟', "QUADRANT UPPER RIGHT AND LOWER LEFT AND LOWER RIGHT"),
    ];

    #[test]
    fn the_quadrants_come_from_the_unicode_names() {
        // Two claims, both from the name: the **single** quadrants sit in the
        // quadrant their name says (ink is there and nowhere else), and the
        // **composite** ones are the saturating sum of the single quadrants
        // counted in their names. The first sees mirroring, the second sees mask
        // errors; had only the second been written, swapping `▘` and `▝` would
        // pass both tests, since the composites would use the same swapped
        // singles.
        for (point_size, scale) in PROCEDURAL_SIZES {
            let m = atlas(point_size, scale).metrics();
            let (w, h) = m.cell_wh();
            let singles = |name: &str| -> Vec<char> {
                name.trim_start_matches("QUADRANT ")
                    .split(" AND ")
                    .map(|quarter| match quarter {
                        "UPPER LEFT" => '▘',
                        "UPPER RIGHT" => '▝',
                        "LOWER LEFT" => '▖',
                        "LOWER RIGHT" => '▗',
                        other => panic!("unrecognised quadrant: {other}"),
                    })
                    .collect()
            };
            for (ch, name) in QUARTER_NAMES {
                let bytes = procedural(m, ch);
                assert!(
                    bytes.iter().any(|&b| b > 0),
                    "{point_size}pt@{scale}x: '{ch}' ({name}) painted no pixel at all"
                );
                // The box the name draws: one-eighths their own strips,
                // quadrants their own quadrants, composites the whole cell.
                let (x0, x1, y0, y1) = match name {
                    "UPPER ONE EIGHTH BLOCK" => (0, w, 0, h.div_ceil(8)),
                    "RIGHT ONE EIGHTH BLOCK" => (w - w.div_ceil(8), w, 0, h),
                    _ if name.contains(" AND ") => (0, w, 0, h),
                    _ => {
                        let left = name.ends_with("LEFT");
                        let upper = name.contains("UPPER");
                        (
                            if left { 0 } else { w / 2 },
                            if left { w.div_ceil(2) } else { w },
                            if upper { 0 } else { h / 2 },
                            if upper { h.div_ceil(2) } else { h },
                        )
                    }
                };
                for y in 0..h {
                    for x in 0..w {
                        let outside = x < x0 || x >= x1 || y < y0 || y >= y1;
                        assert!(
                            !outside || bytes[y * w + x] == 0,
                            "{point_size}pt@{scale}x: '{ch}' ({name}) painted pixel \
                             ({x}, {y}) — outside the box of its name"
                        );
                    }
                }
                if name.contains(" AND ") {
                    let expected = singles(name)
                        .into_iter()
                        .fold(vec![0u8; m.slot_bytes()], |acc, quarter| {
                            saturating_sum(&acc, &procedural(m, quarter))
                        });
                    assert_eq!(
                        bytes, expected,
                        "{point_size}pt@{scale}x: '{ch}' ({name}) is not the sum \
                         of the quadrants in its name"
                    );
                }
            }
        }
    }

    #[test]
    fn the_shades_are_flat_and_ordered() {
        // The shades are **patternless** (see the `SHADE_LEVELS` doc of `raster`):
        // a checkerboard tiles only if the step divides both dimensions of the
        // cell, and it does not: on this machine the 13pt@2x cell is 16×33 and 33
        // is odd. Flat coverage tiles by construction and this is its guard: every
        // shade is single-valued.
        for (point_size, scale) in PROCEDURAL_SIZES {
            let m = atlas(point_size, scale).metrics();
            let mut previous = 0u8;
            for cp in 0x2591..=0x2593u32 {
                let ch = char::from_u32(cp).expect("shade code point");
                let bytes = procedural(m, ch);
                let first = bytes[0];
                assert!(
                    bytes.iter().all(|&b| b == first),
                    "{point_size}pt@{scale}x: U+{cp:04X} is not flat, the pattern breaks when tiled"
                );
                assert!(
                    first > previous,
                    "{point_size}pt@{scale}x: U+{cp:04X} is not darker than the previous one \
                     ({previous} → {first})"
                );
                previous = first;
            }
            assert!(
                previous < 255,
                "the darkest shade must not equal the full block"
            );
        }
    }

    #[test]
    fn braille_dots_come_from_the_code_point_bits() {
        // **No table**: the low 8 bits are directly the dot mask. The guard is
        // tableless too — instead of writing the expectation out for 256
        // patterns it derives it from the sprites of eight **single dots**, so it
        // does not read the implementation's own mapping.
        for (point_size, scale) in PROCEDURAL_SIZES {
            let m = atlas(point_size, scale).metrics();
            let blank = procedural(m, '\u{2800}');
            assert!(
                blank.iter().all(|&b| b == 0),
                "{point_size}pt@{scale}x: the blank Braille pattern left ink"
            );

            let dots: Vec<Vec<u8>> = (0..8)
                .map(|bit| {
                    let ch = char::from_u32(0x2800 | (1u32 << bit)).expect("Braille code point");
                    procedural(m, ch)
                })
                .collect();
            for (bit, dot) in dots.iter().enumerate() {
                assert!(
                    dot.iter().any(|&b| b > 0),
                    "{point_size}pt@{scale}x: bit {bit} painted no pixel at all"
                );
            }
            // **The supports are disjoint**: if two dots touched the same pixel
            // the 2×4 grid would flow together and the pattern be unreadable.
            for i in 0..8 {
                for j in i + 1..8 {
                    let touching = dots[i].iter().zip(&dots[j]).any(|(a, b)| *a > 0 && *b > 0);
                    assert!(
                        !touching,
                        "{point_size}pt@{scale}x: bit {i} and bit {j} touched the same pixel"
                    );
                }
            }
            // The union law, on **all** 256 patterns.
            for mask in 0u32..=0xFF {
                let ch = char::from_u32(0x2800 | mask).expect("Braille code point");
                let expected = (0..8)
                    .filter(|bit| mask & (1 << bit) != 0)
                    .fold(vec![0u8; m.slot_bytes()], |acc, bit| {
                        pixel_max(&acc, &dots[bit])
                    });
                assert_eq!(
                    procedural(m, ch),
                    expected,
                    "{point_size}pt@{scale}x: U+{:04X} is not the union of its bits",
                    0x2800 | mask
                );
            }
        }
    }

    #[test]
    fn procedural_chars_share_one_slot_across_faces() {
        // Unicode carries the light/heavy distinction in the character itself,
        // so SGR bold thickening the block would encode the information twice.
        // The side benefit is measurable: four faces, one slot.
        let mut a = atlas(POINT_SIZE, 1.0);
        let regular = a
            .slot(
                Sprite::Char('\u{2588}'),
                Face::Regular,
                SizeClass::Normal,
                Half::Whole,
            )
            .0
            .slot;
        assert_ne!(regular, TOFU, "the procedural character fell to tofu");
        for face in [Face::Bold, Face::Italic, Face::BoldItalic] {
            assert_eq!(
                a.slot(
                    Sprite::Char('\u{2588}'),
                    face,
                    SizeClass::Normal,
                    Half::Whole
                )
                .0
                .slot,
                regular,
                "{face:?} took a separate slot"
            );
        }
        assert_eq!(a.occupancy().0, 2, "expected tofu + a single slot");
    }

    /// The bytes of a fresh small-class slot.
    fn small_slot_bytes(a: &mut Atlas, ch: char) -> (u16, Vec<u8>) {
        let (placed, upload) = a.slot(
            Sprite::Char(ch),
            Face::Regular,
            SizeClass::Small,
            Half::Whole,
        );
        let bytes = upload
            .expect("a new slot must yield an upload")
            .bytes
            .to_vec();
        (placed.slot, bytes)
    }

    /// Where the small cell lands in the large slot: (row of its top edge,
    /// its rows, its columns), clipped to the slot — the same placement
    /// `place_small` applies, derived from the two `Metrics` here.
    fn small_box(a: &Atlas) -> (usize, usize, usize) {
        let (sw, sh) = a.small_metrics.cell_wh();
        let (lw, lh) = a.metrics.cell_wh();
        let top = usize::from(a.metrics.baseline_px - a.small_metrics.baseline_px);
        (top, sh.min(lh - top), sw.min(lw))
    }

    #[test]
    fn the_small_class_draws_procedurally_at_its_own_cell() {
        // 046 Karar 3: the gate is **open** in the small class, at the small
        // face's own cell. `█` fills exactly that box — the context line's
        // column step wide, the small cell high, its baseline on the large
        // cell's — and nothing outside it, otherwise neighbouring blocks would
        // overlap (wider) or leave a seam (narrower). Every point size and
        // scale the other measure guards walk.
        for (point_size, scale) in [
            (POINT_SIZE, 1.0),
            (POINT_SIZE, 2.0),
            (LARGE_POINT_SIZE, 1.0),
        ] {
            let mut a = atlas(point_size, scale);
            let normal_slot = a
                .slot(
                    Sprite::Char('\u{2588}'),
                    Face::Regular,
                    SizeClass::Normal,
                    Half::Whole,
                )
                .0
                .slot;
            let (small_slot, bytes) = small_slot_bytes(&mut a, '\u{2588}');
            assert_ne!(small_slot, TOFU, "the small block fell to tofu");
            assert_ne!(
                small_slot, normal_slot,
                "the small class shared the large one's slot"
            );
            let (lw, lh) = a.metrics.cell_wh();
            let (top, rows, cols) = small_box(&a);
            assert!(
                cols < lw,
                "{point_size}×{scale}: the small cell is not narrower than the large one"
            );
            for y in 0..lh {
                for x in 0..lw {
                    let inside = (top..top + rows).contains(&y) && x < cols;
                    let want = if inside { 255 } else { 0 };
                    assert_eq!(
                        bytes[y * lw + x],
                        want,
                        "{point_size}×{scale}: ({x}, {y}) of the small `█`; box rows {top}..{} cols ..{cols}",
                        top + rows
                    );
                }
            }
        }
    }

    #[test]
    fn small_lower_blocks_stand_on_one_row_in_eighths() {
        // The sparkline's eight levels (U+2581–2588) in the small class: all
        // stand on the same bottom row — the small cell's bottom edge — and
        // each is an eighth of the small cell taller than the one before. The
        // height is read as the column's summed coverage, i.e. the fractional
        // edge row counts by its share, the same way `add_rect` paints it.
        for scale in [1.0, 2.0] {
            let mut a = atlas(POINT_SIZE, scale);
            let (top, rows, _) = small_box(&a);
            let (lw, _) = a.metrics.cell_wh();
            let sh = a.small_metrics.cell_px.1;
            let mut previous = 0.0;
            for (k, ch) in ('\u{2581}'..='\u{2588}').enumerate() {
                let (_, bytes) = small_slot_bytes(&mut a, ch);
                let column: Vec<u8> = (0..bytes.len() / lw).map(|y| bytes[y * lw]).collect();
                let bottom = column
                    .iter()
                    .rposition(|&b| b > 0)
                    .expect("the block painted nothing");
                assert_eq!(
                    bottom,
                    top + rows - 1,
                    "×{scale}: '{ch}' does not stand on the small cell's bottom row"
                );
                let height: f64 = column.iter().map(|&b| f64::from(b) / 255.0).sum();
                let want = f64::from(sh) * (k + 1) as f64 / 8.0;
                assert!(
                    (height - want).abs() <= 0.5,
                    "×{scale}: '{ch}' is {height} rows high, an eighth step wants {want}"
                );
                assert!(height > previous, "×{scale}: '{ch}' is not taller");
                previous = height;
            }
        }
    }

    // Calibration: names a font (042 Karar 7).
    #[cfg(target_os = "macos")]
    #[test]
    fn the_stats_glyphs_have_no_box_in_the_small_class() {
        // 046: the load indicator sits in the context line, i.e. the small
        // class. A hand copy of `bt-core`'s `STATS_GLYPHS` minus the
        // procedural blocks (this crate cannot see it;
        // `the_stats_glyphs_are_the_ones_the_atlas_checks` links them) — the twin of
        // `the_upload_row_has_no_box_in_the_small_class`. Menlo, by name.
        let mut menlo = Atlas::new(Some("Menlo"), POINT_SIZE, 1.0, 1.0);
        assert_eq!(menlo.font_issue(), None, "Menlo did not open");
        for ch in ['▲', '●'] {
            let slot = menlo
                .slot(
                    Sprite::Char(ch),
                    Face::Regular,
                    SizeClass::Small,
                    Half::Whole,
                )
                .0
                .slot;
            assert_ne!(slot, TOFU, "'{ch}' is a box in Menlo's small class");
        }
    }

    #[test]
    fn pattern_period_divides_cell_evenly() {
        // The dot/dash pattern is tiled with `x % period` and the sprite is one
        // cell wide: if the period does not divide the cell evenly, dash lengths
        // look different in two neighbouring cells. In the wave this constraint
        // held by construction through `WAVE_COUNT`, in `band` it did not — it
        // had been overlooked.
        for w in 1..=40usize {
            for wanted in 1..=40usize {
                let p = raster::dividing_period(wanted, w);
                assert!(p >= 1, "the period cannot be zero (w={w}, wanted={wanted})");
                assert_eq!(w % p, 0, "period {p} does not divide the cell ({w})");
            }
        }
    }

    /// The **second copy** of the line family's arm table, from a separate
    /// source: this list is the Unicode names of the characters (`unicodedata`,
    /// UCD 16.0; the `BOX DRAWINGS ` prefix dropped), while `raster::LINES` is
    /// arm sets written from geometry. A test reading the implementation's table
    /// would prove nothing, and what it would miss has a name: **right geometry,
    /// wrong character** — in a mirrored or shifted table every sprite looks
    /// flawless, it just sits at the wrong code point.
    // `rustfmt::skip`: the aligned name comments are the only reason the
    // table can be scanned by eye.
    #[rustfmt::skip]
    const LINE_NAMES: [&str; 128] = [
        "LIGHT HORIZONTAL",                            // ─
        "HEAVY HORIZONTAL",                            // ━
        "LIGHT VERTICAL",                              // │
        "HEAVY VERTICAL",                              // ┃
        "LIGHT TRIPLE DASH HORIZONTAL",                // ┄
        "HEAVY TRIPLE DASH HORIZONTAL",                // ┅
        "LIGHT TRIPLE DASH VERTICAL",                  // ┆
        "HEAVY TRIPLE DASH VERTICAL",                  // ┇
        "LIGHT QUADRUPLE DASH HORIZONTAL",             // ┈
        "HEAVY QUADRUPLE DASH HORIZONTAL",             // ┉
        "LIGHT QUADRUPLE DASH VERTICAL",               // ┊
        "HEAVY QUADRUPLE DASH VERTICAL",               // ┋
        "LIGHT DOWN AND RIGHT",                        // ┌
        "DOWN LIGHT AND RIGHT HEAVY",                  // ┍
        "DOWN HEAVY AND RIGHT LIGHT",                  // ┎
        "HEAVY DOWN AND RIGHT",                        // ┏
        "LIGHT DOWN AND LEFT",                         // ┐
        "DOWN LIGHT AND LEFT HEAVY",                   // ┑
        "DOWN HEAVY AND LEFT LIGHT",                   // ┒
        "HEAVY DOWN AND LEFT",                         // ┓
        "LIGHT UP AND RIGHT",                          // └
        "UP LIGHT AND RIGHT HEAVY",                    // ┕
        "UP HEAVY AND RIGHT LIGHT",                    // ┖
        "HEAVY UP AND RIGHT",                          // ┗
        "LIGHT UP AND LEFT",                           // ┘
        "UP LIGHT AND LEFT HEAVY",                     // ┙
        "UP HEAVY AND LEFT LIGHT",                     // ┚
        "HEAVY UP AND LEFT",                           // ┛
        "LIGHT VERTICAL AND RIGHT",                    // ├
        "VERTICAL LIGHT AND RIGHT HEAVY",              // ┝
        "UP HEAVY AND RIGHT DOWN LIGHT",               // ┞
        "DOWN HEAVY AND RIGHT UP LIGHT",               // ┟
        "VERTICAL HEAVY AND RIGHT LIGHT",              // ┠
        "DOWN LIGHT AND RIGHT UP HEAVY",               // ┡
        "UP LIGHT AND RIGHT DOWN HEAVY",               // ┢
        "HEAVY VERTICAL AND RIGHT",                    // ┣
        "LIGHT VERTICAL AND LEFT",                     // ┤
        "VERTICAL LIGHT AND LEFT HEAVY",               // ┥
        "UP HEAVY AND LEFT DOWN LIGHT",                // ┦
        "DOWN HEAVY AND LEFT UP LIGHT",                // ┧
        "VERTICAL HEAVY AND LEFT LIGHT",               // ┨
        "DOWN LIGHT AND LEFT UP HEAVY",                // ┩
        "UP LIGHT AND LEFT DOWN HEAVY",                // ┪
        "HEAVY VERTICAL AND LEFT",                     // ┫
        "LIGHT DOWN AND HORIZONTAL",                   // ┬
        "LEFT HEAVY AND RIGHT DOWN LIGHT",             // ┭
        "RIGHT HEAVY AND LEFT DOWN LIGHT",             // ┮
        "DOWN LIGHT AND HORIZONTAL HEAVY",             // ┯
        "DOWN HEAVY AND HORIZONTAL LIGHT",             // ┰
        "RIGHT LIGHT AND LEFT DOWN HEAVY",             // ┱
        "LEFT LIGHT AND RIGHT DOWN HEAVY",             // ┲
        "HEAVY DOWN AND HORIZONTAL",                   // ┳
        "LIGHT UP AND HORIZONTAL",                     // ┴
        "LEFT HEAVY AND RIGHT UP LIGHT",               // ┵
        "RIGHT HEAVY AND LEFT UP LIGHT",               // ┶
        "UP LIGHT AND HORIZONTAL HEAVY",               // ┷
        "UP HEAVY AND HORIZONTAL LIGHT",               // ┸
        "RIGHT LIGHT AND LEFT UP HEAVY",               // ┹
        "LEFT LIGHT AND RIGHT UP HEAVY",               // ┺
        "HEAVY UP AND HORIZONTAL",                     // ┻
        "LIGHT VERTICAL AND HORIZONTAL",               // ┼
        "LEFT HEAVY AND RIGHT VERTICAL LIGHT",         // ┽
        "RIGHT HEAVY AND LEFT VERTICAL LIGHT",         // ┾
        "VERTICAL LIGHT AND HORIZONTAL HEAVY",         // ┿
        "UP HEAVY AND DOWN HORIZONTAL LIGHT",          // ╀
        "DOWN HEAVY AND UP HORIZONTAL LIGHT",          // ╁
        "VERTICAL HEAVY AND HORIZONTAL LIGHT",         // ╂
        "LEFT UP HEAVY AND RIGHT DOWN LIGHT",          // ╃
        "RIGHT UP HEAVY AND LEFT DOWN LIGHT",          // ╄
        "LEFT DOWN HEAVY AND RIGHT UP LIGHT",          // ╅
        "RIGHT DOWN HEAVY AND LEFT UP LIGHT",          // ╆
        "DOWN LIGHT AND UP HORIZONTAL HEAVY",          // ╇
        "UP LIGHT AND DOWN HORIZONTAL HEAVY",          // ╈
        "RIGHT LIGHT AND LEFT VERTICAL HEAVY",         // ╉
        "LEFT LIGHT AND RIGHT VERTICAL HEAVY",         // ╊
        "HEAVY VERTICAL AND HORIZONTAL",               // ╋
        "LIGHT DOUBLE DASH HORIZONTAL",                // ╌
        "HEAVY DOUBLE DASH HORIZONTAL",                // ╍
        "LIGHT DOUBLE DASH VERTICAL",                  // ╎
        "HEAVY DOUBLE DASH VERTICAL",                  // ╏
        "DOUBLE HORIZONTAL",                           // ═
        "DOUBLE VERTICAL",                             // ║
        "DOWN SINGLE AND RIGHT DOUBLE",                // ╒
        "DOWN DOUBLE AND RIGHT SINGLE",                // ╓
        "DOUBLE DOWN AND RIGHT",                       // ╔
        "DOWN SINGLE AND LEFT DOUBLE",                 // ╕
        "DOWN DOUBLE AND LEFT SINGLE",                 // ╖
        "DOUBLE DOWN AND LEFT",                        // ╗
        "UP SINGLE AND RIGHT DOUBLE",                  // ╘
        "UP DOUBLE AND RIGHT SINGLE",                  // ╙
        "DOUBLE UP AND RIGHT",                         // ╚
        "UP SINGLE AND LEFT DOUBLE",                   // ╛
        "UP DOUBLE AND LEFT SINGLE",                   // ╜
        "DOUBLE UP AND LEFT",                          // ╝
        "VERTICAL SINGLE AND RIGHT DOUBLE",            // ╞
        "VERTICAL DOUBLE AND RIGHT SINGLE",            // ╟
        "DOUBLE VERTICAL AND RIGHT",                   // ╠
        "VERTICAL SINGLE AND LEFT DOUBLE",             // ╡
        "VERTICAL DOUBLE AND LEFT SINGLE",             // ╢
        "DOUBLE VERTICAL AND LEFT",                    // ╣
        "DOWN SINGLE AND HORIZONTAL DOUBLE",           // ╤
        "DOWN DOUBLE AND HORIZONTAL SINGLE",           // ╥
        "DOUBLE DOWN AND HORIZONTAL",                  // ╦
        "UP SINGLE AND HORIZONTAL DOUBLE",             // ╧
        "UP DOUBLE AND HORIZONTAL SINGLE",             // ╨
        "DOUBLE UP AND HORIZONTAL",                    // ╩
        "VERTICAL SINGLE AND HORIZONTAL DOUBLE",       // ╪
        "VERTICAL DOUBLE AND HORIZONTAL SINGLE",       // ╫
        "DOUBLE VERTICAL AND HORIZONTAL",              // ╬
        "LIGHT ARC DOWN AND RIGHT",                    // ╭
        "LIGHT ARC DOWN AND LEFT",                     // ╮
        "LIGHT ARC UP AND LEFT",                       // ╯
        "LIGHT ARC UP AND RIGHT",                      // ╰
        "LIGHT DIAGONAL UPPER RIGHT TO LOWER LEFT",    // ╱
        "LIGHT DIAGONAL UPPER LEFT TO LOWER RIGHT",    // ╲
        "LIGHT DIAGONAL CROSS",                        // ╳
        "LIGHT LEFT",                                  // ╴
        "LIGHT UP",                                    // ╵
        "LIGHT RIGHT",                                 // ╶
        "LIGHT DOWN",                                  // ╷
        "HEAVY LEFT",                                  // ╸
        "HEAVY UP",                                    // ╹
        "HEAVY RIGHT",                                 // ╺
        "HEAVY DOWN",                                  // ╻
        "LIGHT LEFT AND HEAVY RIGHT",                  // ╼
        "LIGHT UP AND HEAVY DOWN",                     // ╽
        "HEAVY LEFT AND LIGHT RIGHT",                  // ╾
        "HEAVY UP AND LIGHT DOWN",                     // ╿
    ];

    // Arm indices — **the test's own order**, separate from `raster`'s: had
    // the two shared one constant the oracle would have read a part of the
    // implementation.
    const NAMED_UP: usize = 0;
    const NAMED_DOWN: usize = 1;
    const NAMED_LEFT: usize = 2;
    const NAMED_RIGHT: usize = 3;

    /// The arm style the name says. `SINGLE` and `LIGHT` are the same thing:
    /// in the double-line family Unicode calls the thin arm "single".
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    enum Named {
        Light,
        Heavy,
        Double,
    }

    /// The description read from the **name** of a line character.
    struct NamedLine {
        arms: [Option<Named>; 4],
        dashes: u8,
        arc: bool,
    }

    /// Turns a Unicode name into an arm set.
    ///
    /// The name is split into groups at ` AND `; each group carries a set of
    /// directions and — if present — a style. A styleless group inherits the
    /// **first** style of the name (`LIGHT DOWN AND RIGHT` → both light, `HEAVY
    /// VERTICAL AND RIGHT` → all three heavy). There are two traps and both come
    /// from the naming itself: `DOUBLE` is a style but in `DOUBLE DASH` it is
    /// the density count, and `SINGLE` is absent from the style vocabulary — it
    /// is `LIGHT`'s name in the double-line family.
    ///
    /// An unrecognised word or a directionless group **panics**: skipping it
    /// silently would drain the oracle of meaning.
    fn parse_line_name(name: &str) -> NamedLine {
        let words: Vec<&str> = name.split_whitespace().collect();
        let dashes = words
            .iter()
            .position(|&word| word == "DASH")
            .map_or(0u8, |at| match words[at - 1] {
                "DOUBLE" => 2,
                "TRIPLE" => 3,
                "QUADRUPLE" => 4,
                other => panic!("unknown density: {other} ({name})"),
            });
        // The first style word of the name: the inheritance of styleless groups.
        let mut inherited = None;
        for (at, &word) in words.iter().enumerate() {
            let style = match word {
                "LIGHT" | "SINGLE" => Some(Named::Light),
                "HEAVY" => Some(Named::Heavy),
                "DOUBLE" if words.get(at + 1) != Some(&"DASH") => Some(Named::Double),
                _ => None,
            };
            if style.is_some() {
                inherited = style;
                break;
            }
        }

        let mut arms = [None; 4];
        for group in name.split(" AND ") {
            let mut style = None;
            let mut directions: Vec<usize> = Vec::new();
            let words: Vec<&str> = group.split_whitespace().collect();
            for (at, &word) in words.iter().enumerate() {
                match word {
                    "LIGHT" | "SINGLE" => style = Some(Named::Light),
                    "HEAVY" => style = Some(Named::Heavy),
                    "DOUBLE" if words.get(at + 1) != Some(&"DASH") => style = Some(Named::Double),
                    "DOUBLE" | "TRIPLE" | "QUADRUPLE" | "DASH" | "ARC" => {}
                    "UP" => directions.push(NAMED_UP),
                    "DOWN" => directions.push(NAMED_DOWN),
                    "LEFT" => directions.push(NAMED_LEFT),
                    "RIGHT" => directions.push(NAMED_RIGHT),
                    "VERTICAL" => directions.extend([NAMED_UP, NAMED_DOWN]),
                    "HORIZONTAL" => directions.extend([NAMED_LEFT, NAMED_RIGHT]),
                    other => panic!("unrecognised word in name: {other} ({name})"),
                }
            }
            assert!(
                !directions.is_empty(),
                "directionless group: {group} ({name})"
            );
            let style = style
                .or(inherited)
                .unwrap_or_else(|| panic!("styleless name: {name}"));
            for direction in directions {
                arms[direction] = Some(style);
            }
        }
        assert!(arms.iter().any(Option::is_some), "armless name: {name}");
        NamedLine {
            arms,
            dashes,
            arc: name.contains("ARC"),
        }
    }

    /// The line characters in scope: U+2500–U+257F, **diagonals excluded**.
    fn line_chars() -> impl Iterator<Item = (char, NamedLine)> {
        (0x2500..=0x257Fu32)
            .filter(|cp| !(0x2571..=0x2573).contains(cp))
            .map(|cp| {
                let ch = char::from_u32(cp).expect("line code point");
                (ch, parse_line_name(LINE_NAMES[(cp - 0x2500) as usize]))
            })
    }

    /// The pixel profile on one edge of the sprite — a row on the top/bottom
    /// edge, a column on the left/right edge.
    fn edge(bytes: &[u8], m: Metrics, side: usize) -> Vec<u8> {
        let (w, h) = m.cell_wh();
        match side {
            NAMED_UP => bytes[..w].to_vec(),
            NAMED_DOWN => bytes[(h - 1) * w..].to_vec(),
            NAMED_LEFT => (0..h).map(|y| bytes[y * w]).collect(),
            _ => (0..h).map(|y| bytes[y * w + w - 1]).collect(),
        }
    }

    /// The profile on the same edge of the character carrying the arm alone —
    /// the seam's criterion.
    fn reference_edge(m: Metrics, style: Named, vertical: bool) -> Vec<u8> {
        let ch = match (vertical, style) {
            (true, Named::Light) => '│',
            (true, Named::Heavy) => '┃',
            (true, Named::Double) => '║',
            (false, Named::Light) => '─',
            (false, Named::Heavy) => '━',
            (false, Named::Double) => '═',
        };
        let side = if vertical { NAMED_UP } else { NAMED_LEFT };
        edge(&procedural(m, ch), m, side)
    }

    /// The number of unbroken ink bands in the profile.
    fn runs(profile: &[u8]) -> usize {
        profile
            .iter()
            .zip(std::iter::once(&0).chain(profile))
            .filter(|(current, previous)| **current > 0 && **previous == 0)
            .count()
    }

    #[test]
    fn the_name_parser_reads_the_grammar() {
        // The oracle's own guard: a broken parser turning every name into the
        // same arms would leave all the tests green. Four names carry the
        // grammar's four traps — the inherited style, `DOUBLE DASH` not being a
        // style, `SINGLE` meaning thin, and `ARC`.
        let probe = parse_line_name("UP HEAVY AND RIGHT DOWN LIGHT"); // ┞
        assert_eq!(
            probe.arms,
            [
                Some(Named::Heavy),
                Some(Named::Light),
                None,
                Some(Named::Light)
            ]
        );
        let probe = parse_line_name("HEAVY DOUBLE DASH HORIZONTAL"); // ╍
        assert_eq!(
            probe.arms,
            [None, None, Some(Named::Heavy), Some(Named::Heavy)]
        );
        assert_eq!((probe.dashes, probe.arc), (2, false));
        let probe = parse_line_name("VERTICAL SINGLE AND HORIZONTAL DOUBLE"); // ╪
        assert_eq!(
            probe.arms,
            [
                Some(Named::Light),
                Some(Named::Light),
                Some(Named::Double),
                Some(Named::Double)
            ]
        );
        let probe = parse_line_name("LIGHT ARC DOWN AND RIGHT"); // ╭
        assert_eq!(
            probe.arms,
            [None, Some(Named::Light), None, Some(Named::Light)]
        );
        assert!(probe.arc && probe.dashes == 0);
        // And the parser can read **all** 125 names: an unrecognised word or a
        // directionless group panics, so this round does not stay silent.
        assert_eq!(line_chars().count(), 125);
    }

    #[test]
    fn the_arms_come_from_the_unicode_names() {
        // The oracle is **independent**: the expectation is parsed from the
        // character's Unicode name (see [`LINE_NAMES`]), not from the
        // implementation's table. What it sees is a mirrored or shifted table:
        // had `├` and `┤` swapped places both would be drawn flawlessly, only at
        // the wrong code point, and no invariant looking at geometry could have
        // seen it.
        for (point_size, scale) in PROCEDURAL_SIZES {
            let m = atlas(point_size, scale).metrics();
            for (ch, named) in line_chars() {
                let bytes = procedural(m, ch);
                for side in [NAMED_UP, NAMED_DOWN, NAMED_LEFT, NAMED_RIGHT] {
                    let profile = edge(&bytes, m, side);
                    let inked = profile.iter().any(|&b| b > 0);
                    // The **closing** edge of a dashed line is empty: the pattern
                    // starts full and ends with a gap (today's behaviour of `band`
                    // too). The arm is not missing there, the dash is.
                    let trailing = named.dashes > 0 && (side == NAMED_DOWN || side == NAMED_RIGHT);
                    if named.arms[side].is_some() && !trailing {
                        assert!(
                            inked,
                            "{point_size}pt@{scale}x: '{ch}' ({}) arm {side} \
                             did not reach the edge",
                            LINE_NAMES[(u32::from(ch) - 0x2500) as usize]
                        );
                    }
                    if named.arms[side].is_none() {
                        assert!(
                            !inked,
                            "{point_size}pt@{scale}x: '{ch}' ({}) left ink on the edge \
                             of the absent arm {side}",
                            LINE_NAMES[(u32::from(ch) - 0x2500) as usize]
                        );
                    }
                }
                assert_eq!(
                    named.arc,
                    matches!(ch, '╭' | '╮' | '╯' | '╰'),
                    "arc flag does not match the name: '{ch}'"
                );
            }
        }
    }

    #[test]
    fn arms_tile_across_the_cell_edge() {
        // **Seam continuity**: the profile on the edge must depend only on the
        // arm's *style*, not on the rest of the character. Two `─` side by side,
        // a `─` placed to the right of `├`, a `│` placed under `┼` — all the same
        // claim, and the claim is in this one equality: each character's edge
        // profile is **equal** to the profile of that style's single-arm
        // reference. The criterion is equality, not "is there ink": a one-byte
        // difference means a faint seam between neighbouring cells.
        for (point_size, scale) in PROCEDURAL_SIZES {
            let m = atlas(point_size, scale).metrics();
            // References **once per point size**: had all six been rasterized
            // again on every edge it would cost 1500 full cell drawings over
            // three sizes and `make check` would pay it at every phase gate.
            // The outer index is the axis (`vertical`), the inner index is
            // `Named`'s own order — `style as usize` reads it, so the two lists
            // must change together.
            let references: [[Vec<u8>; 3]; 2] = [
                [
                    reference_edge(m, Named::Light, false),
                    reference_edge(m, Named::Heavy, false),
                    reference_edge(m, Named::Double, false),
                ],
                [
                    reference_edge(m, Named::Light, true),
                    reference_edge(m, Named::Heavy, true),
                    reference_edge(m, Named::Double, true),
                ],
            ];
            for (ch, named) in line_chars() {
                let bytes = procedural(m, ch);
                for side in [NAMED_UP, NAMED_DOWN, NAMED_LEFT, NAMED_RIGHT] {
                    let Some(style) = named.arms[side] else {
                        continue;
                    };
                    // The single exemption, named: the **closing** edge of a
                    // dashed line, since the pattern starts full and ends with a
                    // gap. **The arc is not exempt** — it once was and the
                    // exemption covered a real defect: when the radius reached
                    // the cell edge the tangent point fell there and the arc, not
                    // the stem, painted the edge column (255 → 246 on the rail's
                    // row, 0 → 13 on the one below it), and in only two of the
                    // four corners. `corner`'s radius, one pixel further in,
                    // closed it; with the exemption gone this row became the
                    // guard of that fix.
                    if named.dashes > 0 && (side == NAMED_DOWN || side == NAMED_RIGHT) {
                        continue;
                    }
                    let vertical = side == NAMED_UP || side == NAMED_DOWN;
                    assert_eq!(
                        edge(&bytes, m, side),
                        references[usize::from(vertical)][style as usize],
                        "{point_size}pt@{scale}x: '{ch}' seam broke on \
                         edge {side}"
                    );
                }
            }
        }
    }

    #[test]
    fn disjoint_arms_unite_into_the_joint() {
        // The union law: the pixel-max of two characters with disjoint arm sets
        // is the character of the union set. It must hold structurally — the
        // same arm gives the same rectangle in every character — and that is
        // exactly why a break would be no accident but proof that the arm's
        // extent varies with the character.
        //
        // **Double lines are not in this list** and the reason is geometry: the
        // top rail of `╔` runs past the junction to close the corner, while in
        // `╬` the same rail turns into an elbow and stops (the channel must stay
        // open). So `╔ ∪ ╝ ≠ ╬` and it need not be; the guard of the double line
        // is [`double_junctions_keep_the_channel_open`].
        for (point_size, scale) in PROCEDURAL_SIZES {
            let m = atlas(point_size, scale).metrics();
            for (a, b, joint) in [
                ('┌', '┘', '┼'),
                ('┐', '└', '┼'),
                ('┏', '┛', '╋'),
                ('┓', '┗', '╋'),
                ('╴', '╶', '─'),
                ('╵', '╷', '│'),
                ('╸', '╺', '━'),
                ('╹', '╻', '┃'),
                ('├', '┤', '┼'),
            ] {
                assert_eq!(
                    pixel_max(&procedural(m, a), &procedural(m, b)),
                    procedural(m, joint),
                    "{point_size}pt@{scale}x: '{a}' ∪ '{b}' did not give '{joint}'"
                );
            }
        }
    }

    #[test]
    fn double_junctions_keep_the_channel_open() {
        // A double line is not a line but a **channel with two walls**, and
        // every decision at a junction follows from one sentence: the channel
        // does not close. This guard is the only witness of the difference
        // between a rail "turning" and "passing" — neither the union law nor the
        // seam can see it, both look at the edges and the sum.
        for (point_size, scale) in PROCEDURAL_SIZES {
            let m = atlas(point_size, scale).metrics();
            let (w, h) = m.cell_wh();
            // The **inner** gap: the empty row left between ink. The criterion
            // could not be "is there an empty row" — above `╒` there are eight
            // empty rows with no arm, and they are not the channel but the
            // outside of the character.
            let gap_row = |ch: char| {
                let bytes = procedural(m, ch);
                let inked = |y: usize| bytes[y * w..(y + 1) * w].iter().any(|&b| b > 0);
                (0..h).any(|y| !inked(y) && (0..y).any(inked) && (y + 1..h).any(inked))
            };
            let gap_column = |ch: char| {
                let bytes = procedural(m, ch);
                let inked = |x: usize| (0..h).any(|y| bytes[y * w + x] > 0);
                (0..w).any(|x| !inked(x) && (0..x).any(inked) && (x + 1..w).any(inked))
            };
            let full_row = |ch: char| {
                let bytes = procedural(m, ch);
                (0..h).any(|y| bytes[y * w..(y + 1) * w].iter().all(|&b| b == 255))
            };
            let full_column = |ch: char| {
                let bytes = procedural(m, ch);
                (0..w).any(|x| (0..h).all(|y| bytes[y * w + x] == 255))
            };
            let at = format!("{point_size}pt@{scale}x");

            // `╬` is four elbows: both an empty row and an empty column pass
            // through its middle. `╋` has the same arms and has neither — the
            // criterion is not "is there a line" but the openness of the channel.
            assert!(
                gap_row('╬') && gap_column('╬'),
                "{at}: the channel of `╬` closed"
            );
            assert!(
                !gap_row('╋') && !gap_column('╋'),
                "{at}: `╋` opened a gap in its middle"
            );
            // `╠`: the outer wall is unbroken, the inner wall is broken. Because
            // of the unbroken wall there is **no** empty row; if there were, the
            // left edge of the frame would be torn at the T-junction.
            assert!(
                full_column('╠'),
                "{at}: the outer wall of `╠` is not unbroken"
            );
            assert!(!gap_row('╠'), "{at}: `╠` tore the left edge");
            assert!(full_row('╦'), "{at}: the outer wall of `╦` is not unbroken");
            assert!(!gap_column('╦'), "{at}: `╦` tore the top edge");
            // A single rail **passes** through a double-rail junction — if it
            // has the opposite arm. The vertical line of `╪` end to end, the
            // horizontal of `╫` likewise.
            assert!(
                !gap_row('╪'),
                "{at}: the vertical line of `╪` broke in the middle"
            );
            assert!(
                !gap_column('╫'),
                "{at}: the horizontal line of `╫` broke in the middle"
            );
            // Without the opposite arm it **stops**: the stem of `╤` starts at
            // the lower rail and the row between the two rails stays empty.
            assert!(gap_row('╤'), "{at}: the stem of `╤` closed the channel");
            // But at a corner the same stem goes all the way to the **far** rail,
            // otherwise `╒` would be left without a corner.
            assert!(
                !gap_row('╒'),
                "{at}: the stem of `╒` did not reach the top rail"
            );
        }
    }

    #[test]
    fn heavy_is_thicker_and_double_is_two_rails() {
        // Three styles carry three separate claims and all three can be read
        // from the edge profile: heavy is **thicker** than light, double is **two
        // separate** bands.
        for (point_size, scale) in PROCEDURAL_SIZES {
            let m = atlas(point_size, scale).metrics();
            for vertical in [false, true] {
                let light = reference_edge(m, Named::Light, vertical);
                let heavy = reference_edge(m, Named::Heavy, vertical);
                let double = reference_edge(m, Named::Double, vertical);
                let ink = |profile: &[u8]| profile.iter().map(|&b| u32::from(b)).sum::<u32>();
                assert!(
                    ink(&heavy) > ink(&light),
                    "{point_size}pt@{scale}x (vertical={vertical}): heavy is not thicker than light"
                );
                assert_eq!(runs(&light), 1, "a light line must be a single band");
                assert_eq!(runs(&heavy), 1, "a heavy line must be a single band");
                assert_eq!(
                    runs(&double),
                    2,
                    "{point_size}pt@{scale}x (vertical={vertical}): a double line must be \
                     two separate bands"
                );
            }
        }
    }

    #[test]
    fn dashed_densities_collapse_only_with_the_period() {
        // `dividing_period` is kept (`discussion.md` → Karar 4): the period
        // must divide the cell evenly, otherwise the pattern breaks phase at the
        // cell boundary and tiling is the reason this set exists. The cost is a
        // visible loss of information — on this machine at `w = 8` `┄` and `╌`
        // collapse into **the same sprite** — and the guard does not write it
        // into a list, it **derives** it: two densities are equal only if their
        // periods are equal. Had the number been written into a list it would
        // be wrong at another point size.
        for (point_size, scale) in PROCEDURAL_SIZES {
            let m = atlas(point_size, scale).metrics();
            let (w, h) = m.cell_wh();
            for (axis, extent, family) in [
                ("horizontal", w, ['╌', '┄', '┈']),
                ("vertical", h, ['╎', '┆', '┊']),
            ] {
                for (i, first) in family.into_iter().enumerate() {
                    for second in family.into_iter().skip(i + 1) {
                        let period = |ch: char| {
                            let dashes = match ch {
                                '╌' | '╎' => 2usize,
                                '┄' | '┆' => 3,
                                _ => 4,
                            };
                            raster::dividing_period(extent.div_ceil(dashes), extent)
                        };
                        assert_eq!(
                            procedural(m, first) == procedural(m, second),
                            period(first) == period(second),
                            "{point_size}pt@{scale}x {axis}: '{first}' and '{second}' \
                             have periods {} and {}",
                            period(first),
                            period(second)
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn the_arcs_round_the_corner() {
        // The arc is not a separate technique but a separate **shape**: `╭`
        // and `┌` carry the same arms, touch the same edges and are drawn
        // differently. The guard demands the difference between the two,
        // otherwise the arc flag could be silently ignored.
        for (point_size, scale) in PROCEDURAL_SIZES {
            let m = atlas(point_size, scale).metrics();
            for (arc, sharp) in [('╭', '┌'), ('╮', '┐'), ('╯', '┘'), ('╰', '└')] {
                assert_ne!(
                    procedural(m, arc),
                    procedural(m, sharp),
                    "{point_size}pt@{scale}x: '{arc}' was drawn the same as the sharp corner"
                );
            }
        }
    }

    #[test]
    fn the_technical_set_hugs_the_cell_edges() {
        // The **only** place this set departs from the U+2500 family is the
        // position of the axis: there the arms meet in the middle of the cell,
        // here the lines follow the edge. The criterion is pixel-by-pixel
        // equality, not "is there ink" — a `⎿` meeting in the middle would have
        // ink too but would draw a half-height corner.
        type Mask = fn(usize, usize, usize, usize, usize) -> bool;
        let cases: [(char, Mask); 4] = [
            ('\u{23B8}', |x, _y, _w, _h, thin| x < thin),
            ('\u{23B9}', |x, _y, w, _h, thin| x >= w - thin),
            ('\u{23BE}', |x, y, _w, _h, thin| x < thin || y < thin),
            ('\u{23BF}', |x, y, _w, h, thin| x < thin || y >= h - thin),
        ];
        for (point_size, scale) in PROCEDURAL_SIZES {
            let m = atlas(point_size, scale).metrics();
            let (w, h) = m.cell_wh();
            let thin = usize::from(m.underline_px.1.max(1));
            for (ch, mask) in cases {
                let bytes = procedural(m, ch);
                for y in 0..h {
                    for x in 0..w {
                        let want = if mask(x, y, w, h, thin) { 255 } else { 0 };
                        assert_eq!(
                            bytes[y * w + x],
                            want,
                            "{point_size}pt@{scale}x: '{ch}' edge profile is \
                             broken at pixel ({x}, {y})"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn the_technical_pairs_are_mirrors() {
        // The mirroring is **structural**: the first band is `[0, thin)`, the
        // last band `[length - thin, length)` and the two are exact reflections
        // of each other, so this equality must hold at every size regardless of
        // rounding. A break is the second way of saying "the edge line is not on
        // the edge".
        for (point_size, scale) in PROCEDURAL_SIZES {
            let m = atlas(point_size, scale).metrics();
            let (w, h) = m.cell_wh();
            let flip_h: Vec<u8> = (0..h)
                .flat_map(|y| (0..w).map(move |x| (y, w - 1 - x)))
                .map(|(y, x)| procedural(m, '\u{23B8}')[y * w + x])
                .collect();
            assert_eq!(
                flip_h,
                procedural(m, '\u{23B9}'),
                "{point_size}pt@{scale}x: '⎸' and '⎹' are not mirrors of each other"
            );
            let top = procedural(m, '\u{23BE}');
            let flip_v: Vec<u8> = (0..h)
                .flat_map(|y| {
                    let row = (h - 1 - y) * w;
                    top[row..row + w].to_vec()
                })
                .collect();
            assert_eq!(
                flip_v,
                procedural(m, '\u{23BF}'),
                "{point_size}pt@{scale}x: '⎾' and '⎿' are not mirrors of each other"
            );
        }
    }

    #[test]
    fn the_scan_lines_step_down_the_cell() {
        // Two claims, both from the name itself: "HORIZONTAL SCAN LINE-N"
        //
        // 1. The line passes **across the whole cell**. The font's version did
        //    not: Monaco's ink is 0.03–19.19 in a 20 px cell, so scan lines laid
        //    side by side looked dashed.
        // 2. The 1st, 3rd, 5th, 7th and 9th of nine bands — and **the fifth is
        //    `─`**, because Unicode joined it with U+2500. The five bands coming
        //    out evenly spaced is the witness that the formula invents no second
        //    constant; the ±1 pixel allowance comes from `rail` snapping to the
        //    grid (at 13pt@1x the five bands divide exactly, at 16pt@2x 9/9/8/9).
        for (point_size, scale) in PROCEDURAL_SIZES {
            let m = atlas(point_size, scale).metrics();
            let (w, h) = m.cell_wh();
            let mut starts = Vec::new();
            for ch in ['\u{23BA}', '\u{23BB}', '\u{2500}', '\u{23BC}', '\u{23BD}'] {
                let bytes = procedural(m, ch);
                let rows: Vec<usize> = (0..h)
                    .filter(|&y| bytes[y * w..y * w + w].iter().any(|&b| b > 0))
                    .collect();
                let (&first, &last) = (
                    rows.first().expect("scan line was drawn empty"),
                    rows.last().expect("scan line was drawn empty"),
                );
                assert_eq!(
                    rows.len(),
                    last - first + 1,
                    "{point_size}pt@{scale}x: '{ch}' is not a single band"
                );
                for y in first..=last {
                    assert!(
                        bytes[y * w..y * w + w].iter().all(|&b| b == 255),
                        "{point_size}pt@{scale}x: '{ch}' does not span the cell \
                         on row {y}"
                    );
                }
                starts.push(first);
            }
            assert!(
                starts.windows(2).all(|pair| pair[0] < pair[1]),
                "{point_size}pt@{scale}x: scan lines are not ordered \
                 top to bottom: {starts:?}"
            );
            let steps: Vec<usize> = starts.windows(2).map(|pair| pair[1] - pair[0]).collect();
            let (low, high) = (
                *steps.iter().min().expect("four steps"),
                *steps.iter().max().expect("four steps"),
            );
            assert!(
                high - low <= 1,
                "{point_size}pt@{scale}x: bands are not evenly spaced: {steps:?}"
            );
        }
    }

    #[test]
    fn the_diagonals_stay_out_of_scope() {
        // The diagonals are **a hole deliberately left** inside the scope
        // (Karar 3B) and the hole has a second job: the fixture (`╱`) of
        // `face_fallback_is_cached_under_the_requested_face` lives there — on
        // this machine the only block in U+2500–U+257F that is in Menlo Regular
        // but not in Bold, and the rest is now procedural.
        for ch in ['╱', '╲', '╳'] {
            assert!(
                !raster::is_procedural(ch),
                "'{ch}' entered the scope: the face ladder's fixture is gone"
            );
        }
        // Both ends of the three families and their neighbours outside. An
        // overflowing range is **silent**: `braille` takes the mask with `& 0xFF`,
        // `block`'s quadrant arm gives a zero mask to a character it does not
        // recognise, so writing the range one character too wide produces an
        // empty sprite and no invariant looking at geometry can see it.
        for (ch, inside, family) in [
            ('\u{24FF}', false, "below the line family"),
            ('\u{2500}', true, "start of the line family"), // ─
            ('\u{257F}', true, "end of the line family"),   // ╿
            ('\u{2580}', true, "start of the blocks"),      // ▀
            ('\u{259F}', true, "end of the blocks"),        // ▟
            ('\u{25A0}', false, "above the blocks"),        // ■, geometric shapes
            ('\u{27FF}', false, "below Braille"),
            ('\u{2800}', true, "start of Braille"),
            ('\u{28FF}', true, "end of Braille"),
            ('\u{2900}', false, "above Braille"),
            // The fourth family and its **two** neighbours are meaningful:
            // U+23B7 (`⎷`) is deliberately outside too (the root's tail is not
            // a rail, and its cascade version passes the gate), so the lower
            // bound is not a boundary but a **decision**.
            ('\u{23B7}', false, "below the technical set"), // ⎷
            ('\u{23B8}', true, "start of the technical set"), // ⎸
            ('\u{23BF}', true, "end of the technical set"), // ⎿
            ('\u{23C0}', false, "above the technical set"), // ⏀
        ] {
            assert_eq!(
                raster::is_procedural(ch),
                inside,
                "{family} (U+{:04X}) is on the wrong side",
                u32::from(ch)
            );
        }
    }

    /// A wide character is drawn from **two slots** and both arrive **in the
    /// same answer**.
    ///
    /// Atomicity is no convenience: had the two halves been split across
    /// rounds, the capacity limit could fall between them, the left slot would
    /// be granted and the right fall to tofu, and half a glyph + half a box
    /// would appear on screen. `CLAUDE.md`'s rule forbids this by name: "a box
    /// is a visible omission, a clipped glyph a silent corruption".
    #[test]
    fn a_wide_char_takes_two_slots_in_one_answer() {
        let mut a = atlas(POINT_SIZE, 1.0);
        let before = a.occupancy().0;
        let (placed, upload) = a.slot(
            Sprite::Char(WIDE_CHAR),
            Face::Regular,
            SizeClass::Normal,
            Half::Left,
        );
        assert_eq!(
            placed.half,
            Half::Left,
            "'{WIDE_CHAR}' ink fits two cells: a pair is expected"
        );
        assert_ne!(placed.slot, TOFU, "the accepted pair must not fall to tofu");
        let upload = upload.expect("a new pair must yield an upload");
        let right = upload
            .right
            .expect("the right half must arrive in the same answer");
        assert_ne!(
            upload.origin, right,
            "both halves are written to the same slot: the corners must differ"
        );
        assert_eq!(
            upload.bytes.len(),
            upload.right_bytes.len(),
            "both halves are a full slot"
        );
        assert_eq!(
            a.occupancy().0 - before,
            2,
            "a wide character must spend exactly two slots"
        );
        // The right half **does not ask for a second gate round**: the call
        // that accepted the pair wrote both keys, so this question is answered
        // from the cache and the cascade walk does not run again in the middle
        // of the frame budget.
        let (right_placed, right_upload) = a.slot(
            Sprite::Char(WIDE_CHAR),
            Face::Regular,
            SizeClass::Normal,
            Half::Right,
        );
        assert_eq!(
            right_placed.slot,
            placed.slot + 1,
            "right half is the left's neighbour"
        );
        assert!(
            right_upload.is_none(),
            "the right half was already uploaded: the texture must stay untouched"
        );
        assert_eq!(
            a.occupancy().0 - before,
            2,
            "the right half must not open a third slot"
        );
    }

    /// **Gate order: single cell first.** A character declared wide whose ink
    /// fits one cell is drawn from a single slot and its raster is **bit for
    /// bit** the same as with the `Half::Whole` request.
    ///
    /// This is the contract of the 65 measured characters ("declared wide, fits
    /// a single cell"): this set does not move the drawings that have worked
    /// since 021. Had the order been reversed, `centre_shift` would centre them
    /// against the two-cell box and they would all move from their places.
    // Calibration: names a font or a measured number (042 Karar 7).
    #[cfg(target_os = "macos")]
    #[test]
    fn a_wide_char_that_fits_one_cell_keeps_the_single_slot_raster() {
        // The base font's own glyph: in a monospaced face the advance is the
        // cell's advance itself, so by definition a single cell. `☕` is in
        // Menlo on this machine and Unicode declares it two columns wide —
        // among the measured 21.
        const NARROW_WIDE: char = '☕';
        let mut a = atlas(POINT_SIZE, 1.0);
        let whole = {
            let (placed, upload) = a.slot(
                Sprite::Char(NARROW_WIDE),
                Face::Regular,
                SizeClass::Normal,
                Half::Whole,
            );
            (placed, upload.map(|u| u.bytes.to_vec()))
        };
        let Some(whole_bytes) = whole.1 else {
            // If no font carrying the character is installed the test is moot.
            return;
        };
        assert_ne!(whole.0.slot, TOFU, "'{NARROW_WIDE}' must be drawable");

        let mut b = atlas(POINT_SIZE, 1.0);
        let (placed, upload) = b.slot(
            Sprite::Char(NARROW_WIDE),
            Face::Regular,
            SizeClass::Normal,
            Half::Left,
        );
        assert_eq!(
            placed.half,
            Half::Whole,
            "a wide character fitting a single cell must not turn into a pair"
        );
        let upload = upload.expect("a new slot must yield an upload");
        assert!(
            upload.right.is_none(),
            "a single-cell answer must not give a right half: the caller would emit an empty quad"
        );
        assert_eq!(
            upload.bytes,
            &whole_bytes[..],
            "the raster must stay bit for bit the same as with the `Half::Whole` request"
        );
        assert_eq!(b.occupancy().0, 2, "single slot + tofu");
    }

    /// The capacity limit **does not fall between the pair**: with exactly one
    /// free slot, both halves of the requested wide character return tofu and
    /// `next` does not move.
    ///
    /// What is sought is the **absence** of "half a glyph + half a box", and
    /// that state does not arise structurally because of atomicity — so this
    /// guard tests not the gate but the gate's **boundary**. A test looking for
    /// occupancy would stay green for nothing.
    #[test]
    fn a_wide_char_is_rejected_whole_when_only_one_slot_is_left() {
        let mut a = Atlas::new(None, LARGE_POINT_SIZE, 1.0, LARGEST_LINE_HEIGHT);
        let cap = a.capacity().saturating_sub(RULE_RESERVE);
        // The pool is built **for the same reason** as that of
        // `full_atlas_returns_tofu_without_caching`: a character falling to tofu
        // spends no slot, so the fill must be of characters that can really be
        // drawn. The procedural family is normalised to `Regular` so it takes
        // one slot, ASCII takes a separate slot in each of the four faces.
        let pool: Vec<(char, Face)> = procedural_chars()
            .map(|ch| (ch, Face::Regular))
            .chain(
                [Face::Regular, Face::Bold, Face::Italic, Face::BoldItalic]
                    .into_iter()
                    .flat_map(|f| (' '..='~').map(move |ch| (ch, f))),
            )
            .collect();
        // Fill until exactly **one** slot is left free.
        for &(ch, face) in &pool {
            if a.next + 1 >= cap {
                break;
            }
            a.slot(Sprite::Char(ch), face, SizeClass::Normal, Half::Whole);
        }
        assert_eq!(
            a.next + 1,
            cap,
            "the pool must fill the capacity: exactly one slot will be left free"
        );
        let next_before = a.next;
        let (placed, upload) = a.slot(
            Sprite::Char(WIDE_CHAR),
            Face::Regular,
            SizeClass::Normal,
            Half::Left,
        );
        assert_eq!(
            placed.slot, TOFU,
            "one slot is not enough for two halves: the pair must be rejected whole"
        );
        assert!(upload.is_none(), "a rejected pair must not yield an upload");
        assert_eq!(a.next, next_before, "a rejected pair must not spend a slot");
        // The right half gives the same answer: no half glyph arises on screen.
        let (right, _) = a.slot(
            Sprite::Char(WIDE_CHAR),
            Face::Regular,
            SizeClass::Normal,
            Half::Right,
        );
        assert_eq!(right.slot, TOFU, "the right half must be tofu too");
        assert_eq!(
            a.next, next_before,
            "the right half must not spend a slot either"
        );
    }

    /// **A single-cell rejection is not the answer to a two-cell request.**
    ///
    /// This is exactly the order in production: the dock **always** asks for
    /// the input line with `wide: false` (an invariant of `bt_core::dock`) and
    /// the dock's line is `SizeClass::Normal`, so a CJK character typed at the
    /// prompt is first asked as `Half::Whole` and enters the **negative cache**.
    /// After Enter the same character arrives in the grid with `wide: true`. If
    /// the alias lets that tofu record through, the whole set stays dead for
    /// that character for the life of the atlas — and the symptom is silent: a
    /// box is drawn and no counter moves.
    // Calibration: the single-cell rejection comes from CoreText's
    // `.LastResort`, which the shrink arm keeps out by name. A backend
    // without a last-resort font cannot produce it — a glyph that fits two
    // cells fits one at half the size, under `SHRINK_LIMIT` — so the premise
    // exists only on macOS (042 phase-4).
    #[cfg(target_os = "macos")]
    #[test]
    fn a_single_cell_rejection_does_not_answer_the_wide_request() {
        let mut a = atlas(POINT_SIZE, 1.0);
        // 1. The dock's question: a single cell, and the character does not fit there.
        let (whole, _) = a.slot(
            Sprite::Char(WIDE_CHAR),
            Face::Regular,
            SizeClass::Normal,
            Half::Whole,
        );
        assert_eq!(
            whole.slot, TOFU,
            "'{WIDE_CHAR}' does not fit a single cell: it must enter the negative cache"
        );
        // 2. The grid's question: two cells. The same character must now be drawn.
        let (left, upload) = a.slot(
            Sprite::Char(WIDE_CHAR),
            Face::Regular,
            SizeClass::Normal,
            Half::Left,
        );
        assert_ne!(
            left.slot, TOFU,
            "the single-cell rejection poisoned the two-cell request"
        );
        assert_eq!(left.half, Half::Left, "a pair is expected");
        assert!(upload.is_some(), "a new pair must yield an upload");

        // 3. **Two keys together and their answers separate.** `Whole` is still
        // tofu (it really does not fit a single cell), `Left` is a real slot.
        // Both must be in the cache: were one missing, that request would walk
        // the cascade again on every frame — on the main thread, in the middle
        // of the frame budget.
        let key = |half| {
            (
                Sprite::Char(WIDE_CHAR),
                Face::Regular,
                SizeClass::Normal,
                half,
            )
        };
        assert_eq!(
            a.slots.get(&key(Half::Whole)),
            Some(&(TOFU, Plane::Mask)),
            "the single-cell rejection must stay in the cache"
        );
        assert_eq!(
            a.slots.get(&key(Half::Left)).map(|&(slot, _)| slot),
            Some(left.slot),
            "the two-cell acceptance must be in the cache too"
        );
        assert_eq!(
            a.slots.get(&key(Half::Right)).map(|&(slot, _)| slot),
            Some(left.slot + 1),
            "the right half is in the cache too: a second gate round must not run"
        );
    }

    /// A request that fits neither cell is written to **its own key**.
    ///
    /// Had it not been, a rejected [`Half::Left`] request would walk the
    /// cascade again on every frame: the rejection arm builds the key with the
    /// **resolved** half, and in that arm the resolved half is always
    /// [`Half::Whole`], so the key of the requested half would never be
    /// written. Now that the alias no longer lets tofu through, the gap would
    /// turn directly into a per-frame cost.
    #[test]
    fn a_rejected_wide_request_caches_its_own_key() {
        let mut a = atlas(POINT_SIZE, 1.0);
        // A code point that is in no font: if the cascade cannot give a glyph
        // it is `NoGlyph`, if it does the ink gate decides — either way the
        // result is tofu and what this test asks is the **key**, not which arm
        // it came from.
        const NOBODY: char = '\u{10FFFD}';
        let (placed, _) = a.slot(
            Sprite::Char(NOBODY),
            Face::Regular,
            SizeClass::Normal,
            Half::Left,
        );
        // If it is accepted the test is moot: it would mean a font on this
        // machine fits that character into two cells.
        if placed.slot != TOFU {
            return;
        }
        for half in [Half::Whole, Half::Left, Half::Right] {
            assert_eq!(
                a.slots
                    .get(&(Sprite::Char(NOBODY), Face::Regular, SizeClass::Normal, half)),
                Some(&(TOFU, Plane::Mask)),
                "{half:?} key was not written: that request walks the cascade on every frame"
            );
        }
    }

    /// The sequence is shaped into **a single glyph** and arrives in the
    /// colour plane with two halves.
    ///
    /// The criterion is the two columns the grid reserves: the sequence
    /// glyph's geometry is the same as the single-code-point emoji's (035
    /// `context.md` → Ölçülen: şekillendirme), so it must pass 023's two-cell
    /// gate. The right half must not be empty — an empty right half would leave
    /// the "took two slots" test green and draw half an emoji on screen.
    #[test]
    fn a_cluster_takes_two_colour_slots() {
        let mut a = atlas(POINT_SIZE, CLUSTER_SCALE);
        for text in CLUSTERS {
            let sprite = a.intern(text);
            assert!(
                matches!(sprite, Sprite::Cluster(_)),
                "'{text}' is several code points: it must be a cluster"
            );
            let before = a.color_occupancy().0;
            let (placed, upload) = a.slot(sprite, Face::Regular, SizeClass::Normal, Half::Left);
            assert_eq!(
                placed.plane,
                Plane::Color,
                "'{text}' must be in the colour plane"
            );
            assert_eq!(
                placed.half,
                Half::Left,
                "'{text}' must arrive with two halves"
            );
            let upload = upload.expect("a new pair must yield an upload");
            assert!(
                upload.right.is_some(),
                "'{text}' right half must arrive in the same answer"
            );
            assert!(
                upload.bytes.iter().any(|&b| b > 0),
                "'{text}' left half is empty"
            );
            assert!(
                upload.right_bytes.iter().any(|&b| b > 0),
                "'{text}' right half is empty"
            );
            assert_eq!(
                a.color_occupancy().0 - before,
                2,
                "'{text}' must spend exactly two colour slots"
            );
        }
    }

    /// The same string gets the same id and the same slot; different strings
    /// get different ids.
    ///
    /// The id is part of the key, so producing a new id on the second ask would
    /// mean reshaping the same glyph every frame and putting it in a new slot —
    /// silently, until the atlas fills.
    #[test]
    fn the_same_cluster_is_interned_and_cached_once() {
        let mut a = atlas(POINT_SIZE, CLUSTER_SCALE);
        let first = a.intern(CLUSTERS[0]);
        assert_eq!(
            a.intern(CLUSTERS[0]),
            first,
            "the same string must get the same id"
        );
        let ids: Vec<Sprite> = CLUSTERS.iter().map(|text| a.intern(text)).collect();
        for (i, x) in ids.iter().enumerate() {
            for y in &ids[i + 1..] {
                assert_ne!(x, y, "different strings got the same id");
            }
        }
        let (placed, upload) = a.slot(first, Face::Regular, SizeClass::Normal, Half::Left);
        assert!(upload.is_some(), "the first ask must yield an upload");
        let occupied = a.color_occupancy().0;
        // The face **falls to the plain face**: a flag on a bold line must not open a separate slot.
        for face in [Face::Regular, Face::Bold] {
            let sprite = a.intern(CLUSTERS[0]);
            let (again, upload) = a.slot(sprite, face, SizeClass::Normal, Half::Left);
            assert_eq!(
                again, placed,
                "{face:?}: the second ask must give the same answer"
            );
            assert!(
                upload.is_none(),
                "{face:?}: an uploaded slot must not be uploaded again"
            );
        }
        assert_eq!(
            a.color_occupancy().0,
            occupied,
            "the second ask must not open a slot"
        );
        // A single code point is not a cluster: the same glyph must not be held under two keys.
        assert_eq!(a.intern("A"), Sprite::Char('A'));
        // The ceiling: when the table is full a new sequence falls to its base
        // character, a known sequence keeps its id.
        let known = a.intern("\u{1F44D}\u{1F3FD}");
        let cap = a.negative_cache_cap();
        for n in 0..cap {
            let _ = a.intern(&format!("\u{1F44D}{n}"));
        }
        assert!(a.clusters.len() <= cap, "the table exceeded its ceiling");
        assert_eq!(a.intern("\u{1F44D}\u{1F3FD}"), known);
        assert_eq!(
            a.intern("\u{1F4A9}\u{200D}\u{1F525}"),
            Sprite::Char('\u{1F4A9}')
        );
        assert_eq!(a.intern(""), Sprite::Char(' '));
    }

    /// A string that does not shape into a single glyph gets the **base
    /// character's** answer — not a box, not half a glyph (R1.1).
    ///
    /// `👍👍` shapes into two separate glyphs; the grid would never cluster it
    /// but the boundary is the cleanest way to test this arm. The answer is
    /// byte for byte that of `Char('👍')`: it is compared with the single
    /// character asked in a separate atlas, so "base character" is not a
    /// figure of speech but the same raster.
    #[test]
    fn an_unshaped_cluster_answers_with_its_base_char() {
        let mut reference = atlas(POINT_SIZE, CLUSTER_SCALE);
        let (base, base_upload) = reference.slot(
            Sprite::Char(CLUSTER_BASE),
            Face::Regular,
            SizeClass::Normal,
            Half::Left,
        );
        assert_eq!(
            base.plane,
            Plane::Color,
            "the base character must be drawable"
        );
        let base_upload = base_upload.expect("the first ask must yield an upload");
        let base_bytes = (base_upload.bytes.to_vec(), base_upload.right_bytes.to_vec());

        let mut a = atlas(POINT_SIZE, CLUSTER_SCALE);
        let sprite = a.intern(&format!("{CLUSTER_BASE}{CLUSTER_BASE}"));
        let (placed, upload) = a.slot(sprite, Face::Regular, SizeClass::Normal, Half::Left);
        assert_eq!(
            placed, base,
            "an unshaped sequence must get the base character's answer"
        );
        let upload = upload.expect("the first ask must yield an upload");
        assert_eq!(
            (upload.bytes.to_vec(), upload.right_bytes.to_vec()),
            base_bytes,
            "the raster must be bit for bit the same as the base character's"
        );
        // The alias is written: the second ask does not run shaping again.
        let (again, upload) = a.slot(sprite, Face::Regular, SizeClass::Normal, Half::Left);
        assert_eq!(again, placed);
        assert!(upload.is_none(), "the alias must be in the cache");
        let (right, _) = a.slot(sprite, Face::Regular, SizeClass::Normal, Half::Right);
        assert_eq!(
            right.slot,
            placed.slot + 1,
            "the right half's alias must be written too"
        );
    }

    /// In a rebuilt atlas an old id is **tofu**, not a panic.
    ///
    /// The interner falls together with the slots ([`Atlas::clusters`]); an id
    /// left in the caller's hands may reach `slot()` — the display link's
    /// callback — and a panic there would drop the frame.
    #[test]
    fn a_stale_cluster_id_is_tofu() {
        let mut a = atlas(POINT_SIZE, CLUSTER_SCALE);
        let sprite = a.intern(CLUSTERS[0]);
        assert!(a.ensure(None, POINT_SIZE + 1.0, 1.0, 1.0), "key changed");
        let (placed, upload) = a.slot(sprite, Face::Regular, SizeClass::Normal, Half::Left);
        assert_eq!(placed.slot, TOFU);
        assert!(upload.is_none());
        assert_eq!(
            a.intern(CLUSTERS[0]),
            sprite,
            "a cluster asked for again gets its identity again"
        );
    }
}
