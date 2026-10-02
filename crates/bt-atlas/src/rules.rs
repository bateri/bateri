//! The platformless half of the font chain: the ink gate, the shrink arm, the
//! cell formula, the requested-family check, the face ladder and the colour
//! plane's alpha conversion.
//!
//! The font is asked **only** through the [`FontSystem`] trait of the
//! platform's [`Backend`]; the arithmetic and its order live here once for
//! every backend (`.tasks/042-font-sistemi-linux/discussion.md` → Karar 2,
//! 3). The pure kernels ([`centre_shift`], [`ink_fits_placed`],
//! [`fit_ratio`], [`cell_metrics`]) take measurements as values, so the
//! census and the tests can call them without a font.
//!
//! Types are neutral: `f64` where CoreText says `CGFloat` (on macOS the latter
//! is an alias of the former, so the same operations in the same order give
//! the same bits) and [`InkRect`] where it says `CGRect` (a copy of four
//! `f64`s, no rounding).

use crate::system::{Backend, Font, FontSystem};

/// Font face — a **typographic concept**, not an SGR flag.
///
/// Its four variants match `bt-core`'s `bold`/`italic` flags, but **for
/// different reasons**: there it is terminal semantics (SGR 1 / SGR 3), here
/// it is a font trait. Merging the two into one type because "they look the
/// same" would add a `bt-core` edge to `bt-atlas`, and that edge would pull
/// `alacritty_terminal` into a pure font crate. The translation lives in
/// `bt-gpu`, the one layer that sees both.
// `repr(u8)`: see `RuleKind` — the key is on `slot()`'s hot path.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Face {
    #[default]
    Regular = 0,
    Bold = 1,
    Italic = 2,
    BoldItalic = 3,
}

/// Which **point-size class** a sprite is rasterized in.
///
/// An axis **orthogonal** to [`Face`] and deliberately separate: the face is
/// the text's style (the counterpart of SGR 1 / SGR 3), this is its size. Had
/// it been added to `Face` as a fifth variant, "bold small" would be
/// unrepresentable and [`Faces::effective`]'s ladder would line two separate
/// questions up in a single order.
///
/// The small class has **one** consumer, the dock's context line; that line
/// is the terminal's own footer and the shell's styling never reaches it, so
/// only the regular face is rasterized on the small side (`Atlas::slot`).
// `repr(u8)`: see `RuleKind` — the key is on `slot()`'s hot path.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum SizeClass {
    #[default]
    Normal = 0,
    Small = 1,
}

impl Face {
    /// The name used in the warning text.
    fn name(self) -> &'static str {
        match self {
            Face::Regular => "Regular",
            Face::Bold => "Bold",
            Face::Italic => "Italic",
            Face::BoldItalic => "BoldItalic",
        }
    }
}

/// Four faces, in `Face` order. The regular face comes from the chain, the
/// others are derived from it.
pub(crate) struct Faces {
    fonts: [Font; 4],
    /// The faces actually **acquired**; one that could not be acquired has
    /// collapsed onto the regular face.
    ///
    /// Without keeping this, the caller could not know which face it got and
    /// the slot waste that [`Faces::effective`] closes would silently stay
    /// open.
    acquired: [bool; 4],
}

/// The outcome of the requested family that has to be told to the user.
///
/// **Both cannot happen at once:** a family that is not found is replaced by
/// the chain, and both fonts of the chain are monospaced. That is why the
/// type is not a list.
///
/// No text: this crate builds no UI strings, it only reports the fact. The
/// type does not leave `bt-atlas` — `bt-gpu` translates it into its own
/// notice type (`bt-shell` does not see this crate, 003 R5).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FontIssue {
    /// The family is not on this machine; `using` is the name of the family
    /// the chain actually opened.
    FamilyNotFound { requested: String, using: String },
    /// The family opened but CoreText does not consider it monospaced. **Not
    /// rejected**: the cell derives from the space's width and letters are
    /// drawn clipped to the cell, i.e. a broken but working screen. `family`
    /// is the name CoreText reports.
    NotMonospaced { family: String },
}

impl Faces {
    /// Opens from the chain ([`open_chain`]) and derives the three faces.
    pub(crate) fn from_chain(family: Option<&str>, size: f64) -> (Self, Option<FontIssue>) {
        let (regular, issue) = open_chain(family, size);
        (Self::derive(regular), issue)
    }

    /// Derives from the given regular face; [`FontSystem::derive`] answers
    /// one face, `None` if it cannot be acquired.
    ///
    /// A separate constructor, for testing: the base of the chain carries all
    /// four faces, so the fallback branch can only be fired with a real font
    /// by passing a **single-face** family.
    pub(crate) fn derive(regular: Font) -> Self {
        let mut fonts = [regular.clone(), regular.clone(), regular.clone(), regular];
        let mut acquired = [true, false, false, false];
        let mut missing: Vec<&str> = Vec::new();
        for face in [Face::Bold, Face::Italic, Face::BoldItalic] {
            match Backend::derive(&fonts[Face::Regular as usize], face) {
                Some(font) => {
                    fonts[face as usize] = font;
                    acquired[face as usize] = true;
                }
                None => missing.push(face.name()),
            }
        }
        if !missing.is_empty() {
            // Once per atlas construction — `slot()` is on the drawing path
            // and a line printed there would repeat every frame. **Not "once
            // per lifetime":** `Atlas::ensure` rebuilds the atlas (and this)
            // when the family/size/scale changes, so the line shows up again
            // if the window moves between a Retina and an external display.
            // An accepted cost; silencing it would need persistent state
            // outside `Faces`. The prefix is the same as `open_chain`'s
            // (`bateri:`), for the same reason.
            eprintln!(
                "bateri: the font family has no {} face, using the regular face",
                missing.join(", ")
            );
        }
        Self { fonts, acquired }
    }

    pub(crate) fn get(&self, face: Face) -> &Font {
        &self.fonts[face as usize]
    }

    /// The face that goes into the slot key: **a face that could not be
    /// acquired collapses to `Regular`**.
    ///
    /// The key must carry the **drawn** face, not the requested one. In a
    /// single-face family (`Monaco`), `(Char, Bold)` and `(Char, Regular)`
    /// would hold byte-for-byte the same bitmap in two separate slots; with
    /// four faces the atlas fills four times as fast, surplus glyphs fall to
    /// tofu and the symptom is silent. The second face of the same fact as
    /// `Sprite::Rule` being lowered to `Regular`: the requested face and the
    /// drawn face need not be the same.
    pub(crate) fn effective(&self, face: Face) -> Face {
        // A ladder, not a straight drop: lowering `BoldItalic` directly to
        // `Regular` would drop the **weight** too. Families with no real
        // `Bold Italic` face but with a `Bold` one are common; there SGR 1;3
        // text would come out regular although the bold face is at hand.
        let ladder: &[Face] = match face {
            Face::BoldItalic => &[Face::BoldItalic, Face::Bold, Face::Italic],
            Face::Bold => &[Face::Bold],
            Face::Italic => &[Face::Italic],
            Face::Regular => &[],
        };
        ladder
            .iter()
            .copied()
            .find(|&f| self.acquired[f as usize])
            .unwrap_or(Face::Regular)
    }
}

/// Cell measurements, in **physical pixels**.
///
/// `Atlas::new`'s `scale` parameter is multiplied with the point size and
/// goes into the font, so the display scale is inside these numbers. The
/// scale must be part of the key: a glyph rasterized at @1x blurs **without
/// error** at @2x and the symptom shows only on a two-display machine
/// (discussion.md → Muhakeme).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Metrics {
    /// (width, height).
    pub cell_px: (u16, u16),
    /// Pixels from the **top** of the cell to the baseline; the glyph sits
    /// there. Does not exceed the height of [`Metrics::cell_px`] —
    /// [`cell_metrics`] bounds it.
    pub baseline_px: u16,
    /// Underline: (position from the top of the cell, thickness).
    ///
    /// `position + thickness` **never** exceeds `cell_px.1` — [`rule_envelope`]
    /// bounds it. If it did, the line would appear at the top of the next row
    /// and the symptom would be silent.
    pub underline_px: (u16, u16),
    /// Strikeout: (position, thickness). The same guarantee.
    pub strikeout_px: (u16, u16),
}

impl Metrics {
    /// Byte count of a single slot (`R8`: one byte per pixel).
    ///
    /// This is the **single owner** of slot geometry: the atlas's buffer, the
    /// tofu drawing and the raster target all three read it. If the geometry
    /// changes (edge padding, alignment filler) there is one arithmetic point
    /// to fix; spread over three, forgetting one would make the buffers
    /// silently diverge.
    pub fn slot_bytes(self) -> usize {
        let (w, h) = self.cell_wh();
        w * h
    }

    /// A single slot of the colour plane (`RGBA8`: **four** bytes per pixel).
    ///
    /// Same slot geometry, different format — and the single-owner rule is
    /// not broken: both derive from [`Metrics::cell_wh`], so if edge padding
    /// or alignment filler ever comes in there is still one place to fix.
    /// The number could have been written as `slot_bytes() * 4`; a separate
    /// name forces whoever builds the buffer to **state** which plane it is
    /// for, and `raster::draw`'s assert catches the wrong plane.
    pub fn slot_bytes_rgba(self) -> usize {
        self.slot_bytes() * 4
    }

    /// Cell size as `usize` — for indexing and loop bounds.
    ///
    /// The same rationale as [`Metrics::slot_bytes`]: it keeps the widening
    /// with a single owner instead of spreading it over four places.
    pub(crate) fn cell_wh(self) -> (usize, usize) {
        (usize::from(self.cell_px.0), usize::from(self.cell_px.1))
    }
}

/// A glyph's **ink** box, relative to the left of/above the baseline, and
/// **fractional**: the crate's own counterpart of CoreText's `CGRect`, so the
/// gate does not see a platform type. `y` grows upwards from the baseline
/// (CoreText's convention, kept so the arithmetic below is unchanged).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct InkRect {
    pub(crate) x: f64,
    pub(crate) y: f64,
    pub(crate) width: f64,
    pub(crate) height: f64,
}

/// The font's raw vertical measurements, fractional pixels — the input of
/// [`cell_metrics`].
///
/// `underline_position` keeps CoreText's sign: **negative**, it points below
/// the baseline.
#[derive(Clone, Copy, Debug)]
pub(crate) struct RawMetrics {
    pub(crate) ascent: f64,
    pub(crate) descent: f64,
    pub(crate) leading: f64,
    pub(crate) underline_position: f64,
    pub(crate) underline_thickness: f64,
    pub(crate) x_height: f64,
}

/// Is the family name the font reports the requested name —
/// **case-insensitively**.
///
/// CoreText finds the name case-insensitively (`"menlo"` → `Menlo`,
/// measured) but reports it in its own spelling; an exact comparison would
/// ignore the font it found. A PostScript name (`Menlo-Regular`) **does not
/// match**: CoreText opens that too, but the family name differs, and that
/// name names a single face of the family — the setting asks for a family
/// (`docs/AYARLAR.md`).
pub(crate) fn same_family(returned: &str, requested: &str) -> bool {
    returned.to_lowercase() == requested.to_lowercase()
}

/// Walks the chain: the requested family (if any), otherwise the backend's
/// default chain ([`FontSystem::open_default`]). Anything to tell the user is
/// in the second value.
///
/// The requested family is checked **by the returned name** like the other
/// links ([`same_family`]): for a name that does not exist CoreText gives
/// Helvetica on this machine, so unchecked, every misspelled name would open
/// with a proportional font and the symptom would be a "not monospaced"
/// warning — one that names a side effect, not the actual error.
///
/// [`FontSystem::open`] gives the font **together** with the family name it
/// actually opened; the default chain itself belongs to the backend.
pub(crate) fn open_chain(family: Option<&str>, point_size: f64) -> (Font, Option<FontIssue>) {
    let Some(requested) = family else {
        return (Backend::open_default(point_size).0, None);
    };
    let (font, returned) = Backend::open(requested, point_size);
    if !same_family(&returned, requested) {
        let (font, using) = Backend::open_default(point_size);
        let issue = FontIssue::FamilyNotFound {
            requested: requested.to_owned(),
            using,
        };
        return (font, Some(issue));
    }
    let issue =
        (!Backend::is_monospaced(&font)).then_some(FontIssue::NotMonospaced { family: returned });
    (font, issue)
}

/// Names of the monospaced families on this machine, in case-insensitive
/// order — the settings window's Font list.
///
/// A family enters the list only if the chain would open it **without a
/// warning**: the font system resolves the name to its own family
/// ([`same_family`]) and the font it opens is monospaced
/// ([`FontSystem::is_monospaced`]). The backend's candidates
/// ([`FontSystem::families`]) are only a pre-filter — the last word belongs
/// to these two criteria, so no family they reject can get into the list.
pub fn monospaced_families() -> Vec<String> {
    let mut names = Backend::families();
    names.sort_by_cached_key(|name| name.to_lowercase());
    names.dedup();
    // The point size does not matter: family and monospacing are independent
    // of it.
    const PROBE_SIZE: f64 = 12.0;
    names.retain(|name| {
        let (font, returned) = Backend::open(name, PROBE_SIZE);
        same_family(&returned, name) && Backend::is_monospaced(&font)
    });
    names
}

/// What the chain would say while opening `family` — the state of a family
/// **not** in the settings window's Font list (`— not found` / `— not
/// monospaced`, 029 Karar 3). The question is [`open_chain`] itself, so what
/// the window says and what the subtitle says cannot diverge.
pub fn family_issue(family: &str) -> Option<FontIssue> {
    // The point size does not matter: family and monospacing are independent
    // of it.
    const PROBE_SIZE: f64 = 12.0;
    open_chain(Some(family), PROBE_SIZE).1
}

/// Derives the cell size from the font's own metrics ([`cell_metrics`] with
/// the font's raw measurements).
pub(crate) fn metrics(font: &Font, line_height: f64) -> Metrics {
    metrics_at(font, space_advance(font), line_height)
}

/// [`metrics`] with the advance **handed in**: the caller already holds the
/// font's [`space_advance`] and the width must not be derived a second time.
/// The small class's `Metrics` is born this way, from the very number its
/// column step rounds (`Atlas::context_advance`), i.e. the small class keeps
/// one width source.
pub(crate) fn metrics_at(font: &Font, space_advance: f64, line_height: f64) -> Metrics {
    cell_metrics(Backend::raw_metrics(font), space_advance, line_height)
}

/// The space's horizontal advance — the cell width, **fractional**.
///
/// The monospace assumption is in the chain itself; if the setting's family
/// is not monospaced the cell still derives from the space, wide letters get
/// clipped and [`FontIssue::NotMonospaced`] says so. The measured character
/// is the space because every font has one; the choice is fixed in the body,
/// because calling it with another character would tie the cell width to
/// that letter of the font.
///
/// The return is **unrounded**: the grid's pitch is the rounded one
/// ([`Metrics::cell_px`]) but two consumers want the fractional one —
/// [`fallback_font`]'s ink gate and `raster::draw`'s centring. Had both
/// worked with the rounded value, the base font's **own** glyph would look
/// narrower than the cell (7.827 < 8) and the centring would shift every
/// glyph by under half a pixel: the output would no longer be bit-for-bit
/// the same. Its guard is `the_cell_is_the_rounded_advance`.
pub(crate) fn space_advance(font: &Font) -> f64 {
    let Some(glyph) = Backend::glyph(font, ' ') else {
        return 0.0;
    };
    Backend::advance(font, glyph)
}

/// Derives the cell size from the font's own metrics.
///
/// `space_advance` is the space's fractional advance (the cell width,
/// [`space_advance`]). `line_height` is the user's line-spacing
/// multiplier (`[font] line_height`, base `1.0`). The surplus is distributed
/// **equally below and above** the glyph: half pushes the baseline down, the
/// rest stays at the bottom. Added to one side only, the text would shift up
/// or down inside its cell, and the shift would grow as the line spacing
/// opens up.
///
/// Underline and strikeout follow **by themselves**: both are measured from
/// the baseline and the baseline has already moved. A separate correction
/// would tear the lines away from the letters as the multiplier grows.
pub(crate) fn cell_metrics(raw: RawMetrics, space_advance: f64, line_height: f64) -> Metrics {
    let (ascent, descent, leading) = (raw.ascent, raw.descent, raw.leading);
    // The height is found by rounding the two parts **separately** and adding
    // them, not by `round_up(ascent + descent + leading)`. The difference was
    // a measurable clip: on this machine Menlo 13pt gives ascent 12.067,
    // descent 3.066, and rounding the sum up gives 16 — with the baseline at
    // 13 that leaves 3 pixels below, while the font asks for 3.066. What is
    // lost is the last coverage row under `g j p q y ,`; the symptom would be
    // "the text looks a bit off". The numbers depend on the font version and
    // may go stale, **the claim does not**: its guard is
    // `descender_fits_in_the_cell`, which reads that metric from the font
    // itself.
    let natural = round_up(ascent).saturating_add(round_up(descent + leading));
    // The multiplier is applied to the **cell**, not to the ascent: the
    // criterion is the distance between lines, and the font's definition of
    // that distance is `ascent + descent + leading`. At `1.0` the surplus is
    // zero, so by default this path is a no-op.
    let extra = round_up(f64::from(natural) * (line_height - 1.0));
    let above = extra / 2;
    let baseline = round_up(ascent).saturating_add(above);
    let cell_px = (
        round_up(space_advance),
        // `saturating_add`: both parts can go up to `u16::MAX`.
        natural.saturating_add(extra),
    );
    let (u_pos, u_thick, x_h) = (
        raw.underline_position,
        raw.underline_thickness,
        raw.x_height,
    );
    // CoreText's `underline_position` is **negative**: it points below the
    // baseline. The sign is flipped when converting to a position measured
    // from the top of the cell.
    let thickness = round_up(u_thick);
    let underline_px = rule_envelope(
        baseline.saturating_add(round_up(-u_pos)),
        thickness,
        cell_px.1,
    );
    // Strikeout has **no** CoreText counterpart; half the x-height above the
    // baseline, the usual place in typography. `saturating_sub`: at small
    // point sizes the x-height can exceed the baseline.
    let strikeout_px = rule_envelope(
        baseline.saturating_sub(round_up(x_h / 2.0)),
        thickness,
        cell_px.1,
    );
    Metrics {
        cell_px,
        // The baseline stays inside the cell, and that is now a consequence,
        // not a wish: the bottom part is at least 1 because of `round_up`, so
        // `baseline_px < cell_px.1`. That is why `raster`'s
        // `cell_h - baseline` subtraction does not overflow.
        baseline_px: baseline,
        underline_px,
        strikeout_px,
    }
}

/// Fits a rule line **inside** the cell: (position from the top, thickness).
///
/// The return's invariant is `position + thickness <= cell_h`. In this
/// repository the bound is never stressed with today's font (Menlo) — at
/// 13pt the underline is 14+1, the cell 18 — but the clamp is a contract, not
/// a wish: `underline_position` is the font's own data and a font with a
/// narrow descent can push the line out of the cell. The symptom is silent:
/// one row's underline appears at the top of the row below. Its guard is
/// `envelope_stays_inside_cell`, and it tests with **synthetic** input,
/// because a real font never fires this branch.
pub(crate) fn rule_envelope(top: u16, thickness: u16, cell_h: u16) -> (u16, u16) {
    // No rule fits in zero height. It cannot be reached through
    // `cell_metrics()` (`round_up` pins every measurement to >= 1), but the
    // function's only reason to exist is carrying the invariant: it should
    // not both state it and break it.
    if cell_h == 0 {
        return (0, 0);
    }
    // The thickness cannot exceed the cell; at least 1 — a line that is not
    // drawn is not a rule.
    let thickness = thickness.clamp(1, cell_h);
    (top.min(cell_h - thickness), thickness)
}

/// The glyph's horizontal shift inside the cell — **one formula, two
/// consumers**.
///
/// Drawing (`raster::draw`) puts the glyph here, the gate
/// ([`fallback_font`]) measures the ink from here. Written separately,
/// the gate would test a placement that will not be drawn and the two would
/// silently diverge: an accepted candidate could paint outside the cell, or a
/// fitting candidate would be rejected.
///
/// The argument is the **box's** advance, not the cell's: for a single-cell
/// glyph the two are the same number, for a two-column character the box is
/// two cells ([`crate::Half`]). The rename from `cell_advance` to
/// `box_advance` in 023 was not a one-line rename: the same number goes to
/// both the gate and the drawing, so were it multiplied by the columns in
/// only one of them, the gate would test a placement that will not be drawn.
///
/// `max(0.0)` is the drawing's own rule: a glyph whose advance exceeds the
/// box sticks to the left, because clipping should happen on the right — the
/// rationale is in the body of `raster::draw_glyph`. The gate must share it
/// **exactly**, otherwise it would assume a negative shift and believe the
/// candidate's left side to be inside the cell.
pub(crate) fn centre_shift(box_advance: f64, advance: f64) -> f64 {
    ((box_advance - advance) / 2.0).max(0.0)
}

/// The font-free body of [`ink_fits_box`]: does a glyph with advance
/// `advance` and ink `ink` fit in the box at the place [`centre_shift`] puts
/// it.
///
/// It is separate because of the census (`census`): it asks **how much** a
/// candidate would have to shrink to fit, and gives the scaled measurements
/// without a font. The rule stays in one place, so what the census says
/// "fits" and what the gate accepts cannot diverge.
pub(crate) fn ink_fits_placed(box_advance: f64, advance: f64, ink: InkRect) -> bool {
    let left = ink.x + centre_shift(box_advance, advance);
    // The left edge is tested too: a candidate carrying a negative `origin.x`
    // overflows the cell on the left and CG clips it **from the left**. In
    // Latin script a letter is recognised from its left side, so that clip
    // would be a silent corruption — the box is honest.
    left >= 0.0 && left + ink.width <= box_advance
}

/// Do the pixels the candidate paints stay inside the **box**.
///
/// The box is one cell or two cells (`box_advance`): a character declared
/// wide occupies two columns, so if its ink fits in two cells it should be
/// accepted. The order itself is in [`accept`].
///
/// The criterion is horizontal and only horizontal. Testing the vertical too
/// **rejects no candidate** today (measured: every candidate that passes the
/// horizontal gate also fits the cell's baseline window; the only set that
/// overflows vertically is emoji, which at full size is rejected
/// horizontally in a single cell and whose shrunk copy is vertically centred
/// in the cell and fits inside — [`Accepted::rise`]), so a second criterion
/// would be a rule written down without a witness. The limit is written by
/// name: a candidate that overflows vertically today falls to **clipping**,
/// not to the box.
fn ink_fits_box(font: &Font, glyph: u32, box_advance: f64) -> bool {
    ink_fits_placed(
        box_advance,
        Backend::advance(font, glyph),
        Backend::ink(font, glyph),
    )
}

/// A system font that can draw `ch` — **if it fits the cell**.
///
/// Three steps in one function, because all three answer one question: "can
/// we draw this character with an acceptable font". `None` does not mean "no
/// candidate found" but **"not accepted"**, and the caller does not have to
/// tell them apart — the answer to both is [`crate::TOFU`].
///
/// 1. **Candidate** ([`FontSystem::cascade`]): the system's cascade, which
///    the base font's own glyph lookup ([`FontSystem::glyph`]) does not walk
///    — this difference is 019's reason to exist.
/// 2. **Glyph.** Can the candidate really draw it. The candidate may be
///    `base` itself, and then this step gives `None` — we only land here
///    after `base` gave `.notdef`, so no separate "is it the same font"
///    comparison is needed.
/// 3. **Ink gate** ([`accept`]). Does the candidate paint outside the cell at
///    the place it will be drawn — a candidate that does not fit is shrunk
///    if within the limit ([`SHRINK_LIMIT`]), otherwise box. The criterion
///    is geometric: no family name and no trait bit, the only exception is
///    the shrink arm's last resort ([`FontSystem::is_last_resort`]; geometry
///    cannot tell it apart from emoji). The measured numbers are in
///    `.tasks/019-glyph-yedegi/phase-1.md` and
///    `.tasks/041-yedek-glyph-kucultme/`. The bound is the **fractional**
///    cell advance ([`space_advance`]), not the rounded cell width: the same
///    number also feeds `raster::draw`'s centring, and keeping two numbers
///    for two jobs would make them diverge.
///
/// The criterion was once the **advance** (`advance <= cell_advance`) and
/// its symptom was seen by the user: Claude Code's tool marker `⏺` (U+23FA)
/// came out as a box. The cause was measured — the candidate from STIX Two
/// Math **advances** 4.6% wider than the cell but **paints** 8.6% narrower,
/// so the gate measuring the advance rejected a glyph that fit comfortably
/// in the cell. 019's calibration samples (2.17× / 1.83× / 1.66×) had no
/// candidate near 1.0, and the gate had never been tested against symbol
/// fonts.
///
/// Changing the criterion also closes the gap in the other direction: a
/// candidate advancing narrow but painting wide is now a **box**, where it
/// used to be silently clipped from the right. "Box or full glyph" is for the
/// first time a contract, not a wish; since 041 "box, full glyph or a glyph
/// shrunk just enough to fit".
///
/// A candidate with no ink **passes** the gate (zero width fits any cell);
/// what gets drawn is an invisible glyph, not a box. Today this path does not
/// arise because combining marks never reach a grid cell as a separate
/// sprite.
///
/// **No log:** the function is on [`crate::Atlas::slot`]'s drawing path and a
/// line printed per glyph would land in the middle of the frame budget
/// ([`Faces::derive`]'s written rule).
pub(crate) fn fallback_font(
    base: &Font,
    ch: char,
    cell_advance: f64,
    cols: u8,
) -> Option<Accepted> {
    let mut utf8 = [0u8; 4];
    let candidate = Backend::cascade(base, ch.encode_utf8(&mut utf8))?;
    let glyph = Backend::glyph(&candidate, ch)?;
    accept(candidate, glyph, cell_advance, cols)
}

/// Shapes a grapheme cluster (`🇹🇷`, `👨‍👩‍👧`, `👍🏽`, `❤️`) into a **single
/// glyph** ([`FontSystem::shape`]) and passes it through [`fallback_font`]'s
/// gate; `None` means "not a single glyph, or rejected by the gate" and the
/// caller falls back to the base character (035 R1.1).
///
/// The gate measures and the atlas draws **the font the shaper returns** —
/// the one that actually produces the glyph, not the cascade's candidate:
/// drawing another font's glyph number would draw an entirely different
/// letter.
///
/// **No log**, same rationale as [`fallback_font`]: on the drawing path.
pub(crate) fn shape_cluster(
    base: &Font,
    text: &str,
    cell_advance: f64,
    cols: u8,
) -> Option<Accepted> {
    let (font, glyph) = Backend::shape(base, text)?;
    accept(font, glyph, cell_advance, cols)
}

/// The ink gate: does the candidate's glyph fit first in one cell, then (if
/// declared two columns) in two cells — and if not, does it fit **shrunk**.
///
/// The **shared** gate of the single-glyph fallback ([`fallback_font`]) and
/// the grapheme cluster ([`shape_cluster`]) — the "box, full glyph or a glyph
/// shrunk just enough to fit" contract goes through the same order in both,
/// so a cluster's glyph cannot be accepted by a criterion different from a
/// single-code-point emoji's. It is also the census's (`census`) gate.
///
/// The candidate glyph's own measurements are read **once** and both gates
/// test the **same** `(advance, ink)` pair; [`shrink`] is the third arm.
pub(crate) fn accept(candidate: Font, glyph: u32, cell_advance: f64, cols: u8) -> Option<Accepted> {
    let advance = Backend::advance(&candidate, glyph);
    let ink = Backend::ink(&candidate, glyph);
    // **The order is mandatory: one cell first.** A candidate that fits in
    // one cell fits today too and is drawn from a single slot; asked directly
    // with the two-cell box, `centre_shift` would move it to the middle of
    // two cells and a drawing *that works today* would move. Measured (023
    // `context.md`): 65 characters are declared wide but their ink fits in
    // one cell — 21 are Menlo's own glyphs, 44 are CJK punctuation and
    // fullwidth forms with slender ink from the cascade (`、 。 》 ！`). A side
    // benefit is capacity: those 65 do not spend a second slot.
    if ink_fits_placed(cell_advance, advance, ink) {
        return Some(Accepted {
            font: candidate,
            glyph,
            cols: 1,
            shrunk: false,
        });
    }
    // The second gate opens only for a character **declared two columns**.
    // Giving two cells to a single-column character would paint over its
    // neighbour: the grid reserves no spacer for it and that cell has its own
    // ink. That is why the criterion is `min(columns, ink)`.
    let box_advance = cell_advance * f64::from(cols.max(1));
    if cols >= 2 && ink_fits_placed(box_advance, advance, ink) {
        return Some(Accepted {
            font: candidate,
            glyph,
            cols,
            shrunk: false,
        });
    }
    // **Third arm: shrinking** (041). The last arm, so a candidate that
    // passes either gate never gets here and its raster is bit-for-bit
    // today's (R3.3).
    shrink(&candidate, glyph, box_advance).map(|font| Accepted {
        font,
        glyph,
        cols: cols.max(1),
        shrunk: true,
    })
}

/// A copy of a candidate rejected by the gate, at the point size that fits
/// the box — if within the limit.
///
/// The box is the area the grid reserves for the character: one cell for a
/// single-column one, two cells for a two-column one. For a two-column
/// character the one-cell box is not tried separately: a narrower box needs
/// more shrinking, so a candidate that does not fit two cells within the
/// limit never fits one.
///
/// The factor comes from [`fit_ratio`], not from the ink/box ratio:
/// shrinking shrinks the advance too, and a glyph whose advance still
/// exceeds the box sticks to the left by [`centre_shift`]'s rule. `⧉` is the
/// example — the copy shrunk by the ratio (1.11) is rejected again on
/// re-test, 1.22 is needed (`.tasks/041-yedek-glyph-kucultme/phase-1.md` →
/// Uygulama Notları).
///
/// The copy ([`FontSystem::at_size`]) is **the same font** at another point
/// size: the glyph number, the colour trait and hence the plane do not
/// change, the drawing and the centring ([`centre_shift`]) stay untouched.
/// The copy is tested **again** with [`ink_fits_box`] (the gate at the copy's
/// own measurements) — the rule that the gate measures the ink where the
/// candidate will be drawn holds for the small copy too; if it fails, box.
///
/// The backend's last resort is kept out of this arm (R3.2): rationale in
/// [`FontSystem::is_last_resort`].
fn shrink(candidate: &Font, glyph: u32, box_advance: f64) -> Option<Font> {
    let size = Backend::size(candidate);
    let advance = Backend::advance(candidate, glyph);
    let ink = Backend::ink(candidate, glyph);
    let last_resort = Backend::is_last_resort(candidate);
    let at = |s: f64| Backend::at_size(candidate, s);
    let fits = |f: &Font| ink_fits_box(f, glyph, box_advance);
    let fit = fit_ratio(box_advance, advance, ink);
    if !(fit.is_finite() && fit <= SHRINK_LIMIT) || last_resort {
        return None;
    }
    let first = at(size / fit);
    if fits(&first) {
        return Some(first);
    }
    // The factor misses on a font that does not scale linearly with the point
    // size: Apple Color Emoji's advance is rounded to an integer and at small
    // sizes wider than its proportion (at 16pt @2x the copy shrunk by 1.661
    // advances 23 px, the cell is 19.27). The copy's **own** measurement
    // finds the remaining margin: the largest point size that fits, by
    // bisection between `size / fit` and half of it.
    let (mut hi, mut lo) = (size / fit, size / fit / 2.0);
    let mut best = at(lo);
    if !fits(&best) {
        return None;
    }
    for _ in 0..SHRINK_STEPS {
        let mid = (lo + hi) / 2.0;
        let copy = at(mid);
        if fits(&copy) {
            lo = mid;
            best = copy;
        } else {
            hi = mid;
        }
    }
    Some(best)
}

/// The bisection steps of [`shrink`]'s second round: the interval is
/// `size / fit` and half of it, and ten steps bring it under a thousandth of
/// the point size (0.016 pt at 32 pt). Enough: in the census (`make scan`,
/// four combinations) the number of candidates within the limit that are
/// rejected on re-test is **zero**.
const SHRINK_STEPS: usize = 10;

/// The upper bound of shrinking: a candidate whose [`fit_ratio`] is larger
/// than this stays a box.
///
/// A design constant (after `GUTTER_PT`), its value from the census
/// distribution (`make scan`, Menlo 13/16 pt × @1x/@2x,
/// `.tasks/041-yedek-glyph-kucultme/` → phase-2 Uygulama Notları):
///
/// - **The largest that must stay inside** is Apple Color Emoji's
///   single-column `fit`: 1.661–1.681 at @2x, **2.124** at @1x (Karar 2, the
///   user preferred a small emoji to a box; a non-Retina display is a user
///   too). A two-column emoji fits in neither one nor two cells at @1x, and
///   its `fit` in the two-cell box is 1.062 — the same arm covers it too.
/// - **The smallest that stays outside** is 2.250 (a single glyph of Apple
///   Symbols); then STIX Two Math 2.307 / 2.583, Symbol 2.803, the user font
///   Inter Display 4.2. These would shrink by more than half and become a dot
///   in the cell.
///
/// 2.2 lies between the two: 3.6% above emoji, 2.2% below the outside one.
/// `.LastResort` (1.660) is **below** the limit and cannot be told apart by
/// geometry — what keeps it outside is [`FontSystem::is_last_resort`].
pub(crate) const SHRINK_LIMIT: f64 = 2.2;

/// The smallest `fit` the gate passes when the glyph is shrunk by a `1 / fit`
/// scale.
///
/// Bisection, not a closed form: the rule is [`ink_fits_placed`] itself and a
/// closed form would write it (including the stick-to-the-left arm) a second
/// time. As the scale goes to zero every glyph shrinks to the middle of the
/// box and fits, so the lower end always passes; `1` for a candidate that
/// passes the gate. Two consumers: the shrink factor ([`shrink`]) and the
/// census (`census`) — written separately, what the census calls "within the
/// limit" and what the gate shrinks could diverge.
///
/// The assumption that ink and advance scale **linearly** with the point
/// size is covered by the copy's re-test ([`shrink`]).
pub(crate) fn fit_ratio(box_advance: f64, advance: f64, ink: InkRect) -> f64 {
    let fits = |s: f64| {
        let mut scaled = ink;
        scaled.x *= s;
        scaled.width *= s;
        ink_fits_placed(box_advance, advance * s, scaled)
    };
    if fits(1.0) {
        return 1.0;
    }
    let (mut lo, mut hi) = (0.0, 1.0);
    for _ in 0..60 {
        let mid = (lo + hi) / 2.0;
        if fits(mid) {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    1.0 / lo
}

/// The accepted candidate and **how many cells** it fits in.
///
/// `cols` is not the number of columns the grid reserves but the box the gate
/// accepted: if a character declared two columns fits in one cell, this is
/// `1` and it is drawn from a single slot.
pub(crate) struct Accepted {
    pub(crate) font: Font,
    /// The glyph the gate measured — also the one drawn. For a single code
    /// point it is [`FontSystem::glyph`]'s answer, for a cluster the one the
    /// shaper produced.
    pub(crate) glyph: u32,
    pub(crate) cols: u8,
    /// Did the candidate come from the shrink arm ([`shrink`]). `false` for a
    /// candidate that passed either gate, and then the drawing is
    /// bit-for-bit today's (R3.3).
    pub(crate) shrunk: bool,
}

impl Accepted {
    /// The glyph's vertical shift from the baseline (px, up is positive) —
    /// **one formula**, both drawing recipes (`raster::draw_glyph`,
    /// `raster::draw_color_glyph`) read it from here.
    ///
    /// Zero for an unshrunk candidate: every glyph accepted today stays on
    /// its baseline. For a shrunk one the vertical middle of the ink comes to
    /// the middle of the **cell**, because the point size shrinks relative to
    /// the origin above the baseline and the glyph collapses towards the
    /// baseline — an emoji shrunk to half sat low next to the letters, its
    /// bottom at the level of `y`'s tail. The choice was made by eye: the
    /// baseline, the cell's middle and the middle of the x-height were drawn
    /// side by side, and emoji and `⧉` read together with the text in the
    /// second (`.tasks/041-yedek-glyph-kucultme/phase-2.md` → Uygulama
    /// Notları). It is rounded to an integer pixel: the baseline is already an
    /// integer and a fractional shift would change the AA phase and blur the
    /// edge.
    ///
    /// The ink is asked only for a shrunk candidate, so the unshrunk path
    /// costs no font call.
    ///
    /// The horizontal gate is unaffected by this (its criterion is horizontal
    /// only, [`ink_fits_box`]); the cell's middle also reduces vertical
    /// overflow.
    pub(crate) fn rise(&self, m: Metrics) -> f64 {
        if !self.shrunk {
            return 0.0;
        }
        let ink = Backend::ink(&self.font, self.glyph);
        let baseline = f64::from(m.cell_px.1 - m.baseline_px);
        (f64::from(m.cell_px.1) / 2.0 - baseline - (ink.y + ink.height / 2.0)).round()
    }
}

/// Rounds up and squeezes into a `u16`.
///
/// Lower bound 1: for a broken or missing font a metric can come back zero,
/// and a zero-width cell divides the grid by zero. The upper bound is the
/// type itself.
///
/// NaN is handled separately because `clamp` **lets it through** and
/// `NaN as u16` is 0: the lower bound is silently breached and the error
/// blows up at the division, not at its source.
pub(crate) fn round_up(v: f64) -> u16 {
    if !v.is_finite() {
        return 1;
    }
    v.ceil().clamp(1.0, f64::from(u16::MAX)) as u16
}

/// Undoes premultiplication: `encode(c)·a` → `encode(c)`.
///
/// **Required, because the premultiplication happens in the wrong space.**
/// The CG context is sRGB and `PremultipliedLast`, so the value it stores is
/// `encode(c)·a` — the *encoded* component multiplied by alpha. Metal's
/// `RGBA8Unorm_sRGB` texture, however, decodes each RGB channel
/// **independently of alpha**, and the sRGB decode is convex:
/// `decode(encode(c)·a) < decode(encode(c))·a`. The result drifts dark at
/// every edge pixel — half-transparent white on a black background comes out
/// `0x80` instead of `0xBC`, i.e. **a visible dark ring on every antialiased
/// edge**.
///
/// The right place would be a linear context, but CG does not offer one at 8
/// bits: among the alpha formats `CGBitmapContext` supports there is **no
/// straight alpha** (`NoneSkip*` or `Premultiplied*`). The two remaining
/// ways are undoing the premultiplication or a 16-bit linear texture; the
/// latter doubles the colour plane and splits `slot_bytes_rgba` three ways
/// per format.
///
/// **The price is precision at low alpha:** at `a = 1` the division
/// multiplies the quantisation error by 255. It is not visible — that pixel's
/// contribution to the screen is also only `a/255`, so the error's weight
/// fades with it. The price is paid **once** per slot (the raster is cached),
/// not per frame.
///
/// The output is **straight alpha**, so the emoji pipeline's blend stays the
/// **same** as the mask path's: RGB source factor `SourceAlpha`. That is why
/// 008 phase-5's "blend is not a parameter" decision was not reverted.
pub(crate) fn unpremultiply(target: &mut [u8]) {
    for px in target.chunks_exact_mut(4) {
        let a = u32::from(px[3]);
        if a == 0 {
            // A fully transparent pixel has **no** colour; the division is
            // undefined as well. Leaving it at zero is right under `nearest`
            // sampling too: that pixel never carries weight.
            continue;
        }
        for c in &mut px[..3] {
            // `min(255)`: since the product with `a` was rounded, the
            // division can exceed 255 by one unit (half-transparent white is
            // exactly this corner).
            *c = u8::try_from((u32::from(*c) * 255 / a).min(255)).unwrap_or(u8::MAX);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Undoing premultiplication: `encode(c)·a` → `encode(c)`.
    ///
    /// The numbers are what CG actually writes: half-transparent white
    /// (`a = 0x80`) is stored premultiplied as `0x80` and must become `0xff`
    /// when turned back into straight alpha. Without the conversion `0x80` is
    /// read from the texture, and because the channel decode is independent
    /// of alpha the result drops to a quarter in linear space and every
    /// antialiased edge gets a dark ring — its witness on the GPU side is
    /// `bt_gpu::renderer::tests::a_translucent_edge_composites_in_linear_space`.
    #[test]
    fn unpremultiply_recovers_straight_alpha() {
        // Order: half-transparent white, fully transparent (no colour),
        // opaque red, and the smallest alpha, which is **the hardest corner**.
        let mut px = vec![
            0x80, 0x80, 0x80, 0x80, // premultiplied white, a = 0.5
            0x00, 0x00, 0x00, 0x00, // fully transparent
            0xff, 0x00, 0x00, 0xff, // opaque red: must be left untouched
            0x01, 0x00, 0x00, 0x01, // a = 1/255: the division must give 0xff
        ];
        unpremultiply(&mut px);
        assert_eq!(&px[0..4], &[0xff, 0xff, 0xff, 0x80], "translucent white");
        assert_eq!(
            &px[4..8],
            &[0x00, 0x00, 0x00, 0x00],
            "a fully transparent pixel has no colour: the division is undefined, it must stay zero"
        );
        assert_eq!(
            &px[8..12],
            &[0xff, 0x00, 0x00, 0xff],
            "an opaque pixel does not change"
        );
        assert_eq!(
            &px[12..16],
            &[0xff, 0x00, 0x00, 0x01],
            "smallest alpha: must clamp to 255, not overflow"
        );
    }

    /// The conversion is **invertible**: multiplying back gives the original
    /// byte.
    ///
    /// What is sought is not a copy of the formula but an **invariant**, i.e.
    /// "whatever we wrote, the GPU multiplying it by alpha must return the
    /// premultiplied byte we had". The sweep walks every (component, alpha)
    /// pair — not a hand-picked corner — and it is also what shows the reason
    /// for `min(255)`: for half-transparent white the division exceeds 255 by
    /// one unit.
    ///
    /// The slack is **±1** and comes from two roundings: one in CG's
    /// premultiplication, one in our division. A larger deviation at low alpha
    /// is legitimate and the criterion carries it (when `a` is small one byte
    /// corresponds to a ratio far above its linear contribution) — hence the
    /// slack scales with alpha.
    #[test]
    fn unpremultiply_round_trips_through_the_gpu_multiply() {
        for a in 1u32..=255 {
            for c in 0u32..=a {
                // A premultiplied component **cannot exceed** alpha; an input
                // that does never comes from CG and is not in the sweep.
                let mut px = [
                    u8::try_from(c).expect("c ≤ 255"),
                    0,
                    0,
                    u8::try_from(a).expect("a ≤ 255"),
                ];
                unpremultiply(&mut px);
                // What the GPU does: multiply the straight component by alpha.
                let back = u32::from(px[0]) * a / 255;
                // The division's quantisation slack: a one-byte straight error
                // shrinks to `a/255` of premultiplied error, plus two roundings.
                let slack = a.div_ceil(255) + 1;
                assert!(
                    back.abs_diff(c) <= slack,
                    "multiplying back did not give the original byte: c={c} a={a} → {} → {back}",
                    px[0]
                );
            }
        }
    }
}
