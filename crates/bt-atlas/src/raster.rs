//! Draws a single sprite into slot bytes: a font glyph (through the
//! platform's [`FontSystem`], positioned here), a rule line or a procedural
//! character (block elements, Braille, box drawing — no font asked).

use crate::rules::{self, GlyphBox, Metrics, unpremultiply};
use crate::system::{Backend, Font, FontSystem};

/// The result of [`draw`].
///
/// The two failures are separate variants because their **diagnoses** differ,
/// not their behaviour: both fall back to tofu and both are cached. Caching
/// `NoContext` looks wrong at first sight ("a transient error"), but the
/// drawing context has no argument that depends on the character — all of
/// them are fixed for the atlas's lifetime, so if it fails once it always
/// fails. If it were **not** cached, every cell would pay for a failed
/// context setup on every frame and not a single glyph would be drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DrawResult {
    Drawn,
    /// The font has no glyph for this character (`.notdef`).
    NoGlyph,
    /// The backend could not set up its drawing target (CoreText: the
    /// `CGBitmapContext`); a backend's load or render failure falls in the
    /// same diagnostic bucket.
    NoContext,
}

/// Draws the coverage (alpha) bytes of `ch` into `target`.
///
/// `m` is the **slot** metric: the target is one slot, the glyph sits on
/// the slot's baseline. `bx` is the fractional **box** the glyph is centred
/// in ([`rules::space_advance`], twice that for a wide character) with its
/// pad inside the slot ([`GlyphBox`]); `m.cell_px.0` is not the box and does
/// not enter here (the reason is in the doc of [`rules::space_advance`]).
/// `x_offset` is a **whole** pixel shift applied after drawing — non-zero
/// only for the right half of a wide glyph.
///
/// The buffer is cleared only if drawing actually happens.
pub(crate) fn draw(
    font: &Font,
    ch: char,
    m: Metrics,
    bx: GlyphBox,
    x_offset: f64,
    target: &mut [u8],
) -> DrawResult {
    let Some(glyph) = Backend::glyph(font, ch) else {
        return DrawResult::NoGlyph;
    };
    draw_glyph(font, glyph, m, bx, x_offset, 0.0, target)
}

/// Where a glyph goes in its slot — **one formula** for both drawing recipes
/// ([`draw_glyph`], [`draw_color_glyph`]): `(x, baseline)`, `x` from the
/// slot's left edge and `baseline` above the slot's **bottom** edge, the
/// convention [`FontSystem::draw_mask`] takes.
///
/// `rise` is the vertical shift from the baseline (px, positive is up); it is
/// non-zero only for a shrunk fallback and its formula lives in one place
/// ([`rules::Accepted::rise`]).
fn position(
    font: &Font,
    glyph: u32,
    m: Metrics,
    bx: GlyphBox,
    x_offset: f64,
    rise: f64,
) -> (f64, f64) {
    // The baseline sits `cell_h - baseline_px` above the bottom of the slot.
    // The subtraction cannot overflow: `rules::cell_metrics` builds the
    // height as baseline + (descent+leading) and the second part is at least
    // 1.
    let baseline = f64::from(m.cell_px.1 - m.baseline_px) + rise;
    // The glyph is **centred horizontally** in the cell: a fallback font's
    // advance can be narrower than the cell's and a mark stuck to the left
    // looks misaligned between its neighbours. The rule is **universal**, not
    // conditional on the fallback — in a monospaced base font every glyph's
    // advance is exactly the font's own cell advance, so at
    // `letter_spacing = 1` the subtraction is exactly zero and the base
    // font's raster stays bit-for-bit the same (guarded by
    // `every_base_glyph_advance_is_the_cell_advance`); opened up, the same
    // formula puts the glyph in the middle of the wider cell. Written conditionally,
    // the "is it a fallback" question would add a second branch to the
    // drawing path and a second code path to the tests.
    //
    // The shift's formula lives in [`rules::centre_shift`] because its second
    // consumer is the fallback's ink gate: the gate must measure where the
    // candidate will stand **here**. The `max(0.0)` inside is this drawing's
    // rule — the width gate runs only for the **fallback**, the base font may
    // not be monospaced ([`rules::FontIssue::NotMonospaced`]) and a wide glyph
    // may exceed the cell. The target is one **slot** and clips there; the
    // slot may be larger than the grid cell (below `1` the cell sits in
    // its middle and the glyph keeps its room), so spilling into the
    // neighbouring cell happens by the pad and no further. The issue at the
    // slot's edge is the **direction** of clipping: a negative shift cuts off
    // the glyph's left side, whereas clipping must happen on the right. Latin
    // script is recognised from the left; a 'W' with its left edge cut off
    // cannot be told from a 'V'.
    // `x_offset` is for the **right half** of a wide glyph: the same glyph is
    // centred in the same two-cell box, then shifted one cell to the left and
    // the overflowing left half is clipped. The offset is a **whole** pixel
    // count (`m.cell_px.0`), so the AA phase of the two calls is identical and
    // the two halves come out bit-for-bit the same as a single 2w-wide raster
    // split in two — no split buffer, no second `slot_bytes` and no risk of a
    // seam at half a pixel. In a single-cell drawing it is zero, and then
    // this line is the same as it was before wide glyphs were split.
    let x = rules::centre_shift(bx, Backend::advance(font, glyph)) - x_offset;
    (x, baseline)
}

/// The body of [`draw`], called with a glyph number.
///
/// It is separate because of grapheme sequences: a sequence's glyph
/// comes not from a code point but from shaping ([`rules::shape_cluster`]),
/// so the `glyph(ch)` question cannot be asked there. Placement and centring
/// stay **in one place** ([`position`]); [`draw`] only finds the number.
pub(crate) fn draw_glyph(
    font: &Font,
    glyph: u32,
    m: Metrics,
    bx: GlyphBox,
    x_offset: f64,
    rise: f64,
    target: &mut [u8],
) -> DrawResult {
    let (x, baseline) = position(font, glyph, m, bx, x_offset, rise);
    Backend::draw_mask(font, glyph, m, x, baseline, target)
}

/// Draws the **colour** pixels of a glyph into `target` (`RGBA8`, sRGB,
/// **straight** alpha).
///
/// A sibling of [`draw_glyph`] with the **same** position ([`position`]):
/// wide emoji also goes through the `Half` mechanism, so its right half is
/// obtained with the same whole-pixel offset; `rise` is the same too. The
/// backend paints premultiplied and the premultiplication is undone here,
/// once, for every backend ([`rules::unpremultiply`] says why it is needed).
///
/// It is called with a glyph **number**, not a character: both sources of a
/// colour glyph (a fallback candidate and a grapheme sequence) have already
/// found the number while passing the gate ([`rules::Accepted`]), so a
/// character-taking wrapper would have no caller.
pub(crate) fn draw_color_glyph(
    font: &Font,
    glyph: u32,
    m: Metrics,
    bx: GlyphBox,
    x_offset: f64,
    rise: f64,
    target: &mut [u8],
) -> DrawResult {
    let (x, baseline) = position(font, glyph, m, bx, x_offset, rise);
    let drawn = Backend::draw_color(font, glyph, m, x, baseline, target);
    if drawn == DrawResult::Drawn {
        unpremultiply(target);
    }
    drawn
}

/// Rule line kind — holds a slot in the atlas like a character.
///
/// Independent of the face: the line under bold text is not bold. The caller
/// always asks for these with [`crate::Face::Regular`] and `Atlas::slot`
/// normalises that separately as well.
// `repr(u8)`: the derived `Hash` writes the discriminant as `isize` by
// default — 8 bytes. The key is on `slot()`'s hot path and every byte that
// enters the hash is paid per cell per frame.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RuleKind {
    Single,
    Double,
    Curl,
    Dotted,
    Dashed,
    Strike,
    /// Prompt mark: the chevron that stands in for `>`.
    ///
    /// **Not a rule line, but from the same family**, and the reason it is
    /// here is mechanism: this enum is the set of procedurally drawn sprites
    /// that **have no code point of their own** — they do not come from the
    /// font, are independent of the face (pinned to [`crate::Face::Regular`])
    /// and keep their own reserve in the atlas. Five are underlines, one is
    /// strikeout, and one is this.
    ///
    /// The **second** set of procedural drawing is characters
    /// ([`is_procedural`]) and that set does not come in here: it lives as
    /// `Sprite::Char`, takes no slot reserve ([`crate::RULE_RESERVE`]) and
    /// `bt-gpu` does not tell it apart from an ordinary letter. What separates
    /// them is not "who draws it" but "does it have a name": a rule line is an
    /// SGR style, a block is a character.
    ///
    /// Even though the two sets share the same ink, their **boundaries are
    /// separate**: this enum is closed with seven members, the other is 413
    /// characters defined by code point ranges. A box-drawing character
    /// ([`Family::Line`]) cannot be added here — the underline of `Single`
    /// also draws a `─`, but that is an underline whose position lives in the
    /// font's `underline` metric; `─` sits in the middle of the cell and must
    /// tile with its neighbour.
    ///
    /// We do **not** take a `>` from the font, and the reason is a product
    /// decision: the mark is the terminal's own, not the user's font's. The
    /// prompt's shape must not change when the font does (the
    /// user: "could you draw this yourself, nicer").
    Chevron,
}

/// Whole number of waves that fit in one cell.
///
/// The period is `cell_px.0 / WAVE_COUNT` and this division **must be exact**:
/// the sprite is one cell wide and tiled with its neighbours, so if the period
/// does not divide the cell exactly the phase breaks at the boundary of two
/// cells and a multi-cell underline looks interrupted. `1` satisfies this by
/// construction (period = cell width) and gives the gentlest wave; if it is
/// ever raised, a value that divides `cell_px.0` must be chosen.
const WAVE_COUNT: f32 = 1.0;

/// Vertical extent of the curl, as a multiple of the thickness.
///
/// The smallest extent needed for the eye to read the wave as a wave. Its
/// base is pinned to the underline's base, which is already inside the cell
/// (`rules::rule_envelope`), so the curl is inside by construction.
const CURL_FACTOR: f32 = 3.0;

/// Draws the coverage bytes of a rule line into `target`.
///
/// A sibling of [`draw`] but **does not use CG**: like `tofu_buffer` it writes
/// bytes directly. Three gains — the drawing is deterministic (it does not
/// depend on CG's antialiasing version, tests can assert the exact structure),
/// the failure branch never arises (no `NoContext`, the return is `()`), and
/// the font is never asked.
pub(crate) fn draw_rule(kind: RuleKind, m: Metrics, target: &mut [u8]) {
    // audit: the same precondition as `draw`'s, the same reasoning. The buffer
    // is `self.buffer`, sized from `m`, so a mismatch is structurally
    // impossible; the assert catches it at the boundary and by name, not as a
    // meaningless index panic inside `band`/`curl`'s loop.
    assert_eq!(
        target.len(),
        m.slot_bytes(),
        "the buffer must be exactly one slot"
    );
    // The buffer is shared and holds the previous glyph's pixels; if it were
    // not zeroed, that glyph would show through under the rule line.
    target.fill(0);

    let (position, thickness) = match kind {
        RuleKind::Strike => m.strikeout_px,
        _ => m.underline_px,
    };
    // The pattern period stays in integer arithmetic: no `as usize` round trip.
    let thick = usize::from(thickness);
    let (w, h) = m.cell_wh();
    let (position, thickness) = (f32::from(position), f32::from(thickness));

    match kind {
        // Continuous pattern: period 1, filled 1.
        RuleKind::Single | RuleKind::Strike => band(target, m, position, thickness, 1, 1),
        RuleKind::Double => {
            band(target, m, position, thickness, 1, 1);
            // The second line goes **down first**. The rows between the
            // underline and the cell's base are empty (13pt: line 14, cell 17
            // → 15-16 empty) and they are far from the glyph body. Moving it
            // up would reach the last body row of letters like `a e o`, and
            // the two lines would read as one thick line stuck to the letters'
            // feet instead of looking separate. If there is no room below, it
            // falls up.
            let below = position + 2.0 * thickness;
            let second = if below + thickness <= h as f32 {
                below
            } else {
                (position - 2.0 * thickness).max(0.0)
            };
            band(target, m, second, thickness, 1, 1);
        }
        // Dotted and dashed: the period depends on the thickness, so the
        // pattern grows with the point size and does not look cramped at @2x.
        // No lower bound is needed — `rules::rule_envelope` already ties the
        // thickness to `>= 1`.
        RuleKind::Dotted => {
            let p = dividing_period(2 * thick, w);
            band(target, m, position, thickness, p, (p / 2).max(1));
        }
        RuleKind::Dashed => {
            let p = dividing_period(6 * thick, w);
            band(target, m, position, thickness, p, (p * 2 / 3).max(1));
        }
        RuleKind::Curl => curl(target, m, position, thickness),
        RuleKind::Chevron => chevron(target, m),
    }
}

/// Prompt mark: a chevron whose two arms meet in the middle.
///
/// **Its vertical centre comes from the strikeout metric.** No new number
/// needs inventing: the strikeout line sits exactly at the middle of the
/// x-height, i.e. the optical centre of lowercase letters. With the mark
/// seated there it reads aligned with the text; the cell's geometric centre
/// falls below the baseline and the mark would look low against the text.
///
/// **Height is the x-height, width is half of it.** The first is derived too:
/// the distance between the strikeout centre and the baseline is half the
/// x-height, so the arms' vertical span comes straight from the font's own
/// measure. The 1:2 ratio is a chevron's usual typographic proportion and is a
/// single number — no second design constant arises.
///
/// **Thickness is the underline's thickness.** A second thickness number would
/// be a second source and would drift apart when the point size or scale
/// changes.
///
/// The ink is gathered at the horizontal centre of the cell and its width does
/// not exceed half the cell: the mark is drawn in the grid **inside the left
/// gutter** (`bt_gpu::Frame::push_block`) and the gutter can be narrower than
/// one cell. If it overflowed, it would land on the first letter of the
/// command text.
fn chevron(target: &mut [u8], m: Metrics) {
    let (w, _) = m.cell_wh();
    let (strike_top, strike_thick) = m.strikeout_px;
    let center_y = f32::from(strike_top) + f32::from(strike_thick) / 2.0;
    // Half the x-height; if the baseline were not below the centre (a
    // degenerate metric) the arms shrink to zero and the mark is not drawn at
    // all — a blank, not a panic.
    let half_h = (f32::from(m.baseline_px) - center_y).max(0.0);
    let half_w = half_h / 2.0;
    let center_x = w as f32 / 2.0;
    // Half thickness: coverage is computed from distance, i.e. the distance
    // between the line's **axis** and the pixel centre.
    let half_stroke = f32::from(m.underline_px.1).max(1.0) / 2.0;

    // The arms' tips and the apex. `>` opens to the left: tips left, apex right.
    let apex = (center_x + half_w, center_y);
    let upper = (center_x - half_w, center_y - half_h);
    let lower = (center_x - half_w, center_y + half_h);

    stamp(target, m, half_stroke, |px, py| {
        distance_to_segment(px, py, upper, apex).min(distance_to_segment(px, py, apex, lower))
    });
}

/// Stamps the distance field into coverage bytes: pixels whose `distance` is
/// near zero are full, those farther than `half_stroke + 0.5` are empty.
///
/// **The single owner of the antialiasing rule**, as [`coverage`] is for
/// axis-aligned drawing. The reasoning is the same and is written there: if two
/// drawers write the same rule twice, when one is tuned the other stays at the
/// old hardness and the symptom is silent — a test saying "their shapes should
/// differ" cannot see it. Today it has two consumers, [`chevron`] and
/// [`corner`], and the half-pixel transition band is the same in both: a wider
/// one blurs the shape, a narrower one makes it staircase.
///
/// There is **no** `join`: both consumers draw into an empty buffer and the
/// distance field covers the whole cell, so the write is direct. If a third
/// consumer wants a combiner, it goes here, not inside the caller.
fn stamp(target: &mut [u8], m: Metrics, half_stroke: f32, distance: impl Fn(f32, f32) -> f32) {
    let (w, h) = m.cell_wh();
    for y in 0..h {
        for x in 0..w {
            let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
            let value = (half_stroke + 0.5 - distance(px, py)).clamp(0.0, 1.0);
            // audit: `y < h` and `x < w`, so the index is below `w * h`.
            target[y * w + x] = (value * 255.0).round() as u8;
        }
    }
}

/// Distance from a point to a line segment; the chevron's antialiasing looks
/// at this.
///
/// It does **not** use CG, for the same reason as `band` and `curl`: the
/// drawing stays deterministic (tests can assert the exact structure), the
/// failure branch does not arise and the font is never asked.
fn distance_to_segment(px: f32, py: f32, a: (f32, f32), b: (f32, f32)) -> f32 {
    let (abx, aby) = (b.0 - a.0, b.1 - a.1);
    let (apx, apy) = (px - a.0, py - a.1);
    let length = abx * abx + aby * aby;
    // A degenerate segment (zero length) drops to the distance to the
    // endpoint: this branch runs when `half_h` is zero and no division is done.
    let t = if length > 0.0 {
        ((apx * abx + apy * aby) / length).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let (dx, dy) = (apx - t * abx, apy - t * aby);
    (dx * dx + dy * dy).sqrt()
}

/// Rounds the wanted period to the nearest value that **divides the cell
/// width exactly**.
///
/// The dotted/dashed counterpart of the constraint `WAVE_COUNT` carries for the
/// curl, and it exists for the same reason: the sprite is one cell wide, tiled
/// with its neighbours, and an `x % period` pattern breaks phase at the cell
/// boundary. Measured (this machine, Menlo 13pt@1x): `w = 8`, `Dashed` wants
/// period 6 → `8 % 6 = 2`, so the dash lengths would look different in two
/// neighbouring cells. Applying the constraint to the curl but not here was an
/// oversight.
pub(crate) fn dividing_period(wanted: usize, w: usize) -> usize {
    let wanted = wanted.clamp(1, w.max(1));
    (wanted..=w).find(|p| w % p == 0).unwrap_or(w.max(1))
}

/// Horizontal band: paints rows `[top, top + thickness)` in the columns the
/// `pattern` accepts. A partially covered row gets **partial alpha** — the
/// thickness need not be an integer and the antialiasing comes for free.
fn band(target: &mut [u8], m: Metrics, top: f32, thickness: f32, period: usize, filled: usize) {
    let (w, h) = m.cell_wh();
    let (y0, y1) = (top, top + thickness);
    for y in 0..h {
        let value = coverage(y, y0, y1);
        if value == 0 {
            continue;
        }
        for x in 0..w {
            if x % period < filled {
                // `max`: if the two bands of `Double` overlap, the darker wins.
                // audit: `y < h` and `x < w`, so the index is below `w * h`.
                target[y * w + x] = target[y * w + x].max(value);
            }
        }
    }
}

/// Intersection of pixel `[y, y+1)` with band `[y0, y1)` → alpha byte.
///
/// Single owner: `band` and `curl` must use the same antialiasing rule, or the
/// curl gets a different softness from the others and the symptom is silent —
/// a test comparing the five kinds cannot see it because it says "they should
/// differ".
fn coverage(y: usize, y0: f32, y1: f32) -> u8 {
    (overlap(y, y0, y1) * 255.0).round() as u8
}

/// Intersection of pixel `[i, i+1)` with range `[a, b)` — **as a ratio**.
///
/// Extracted from [`coverage`] because a rectangle overlaps on two axes at
/// once and the **product** of the two ratios must be rounded once: multiplying
/// two `coverage` bytes rounds twice and the saturating sum of `▀` and `▄`
/// would not stop at 255 — it would fall one or two short, so between two
/// stacked half blocks a faint copy of the seam procedural drawing exists to close would
/// appear.
fn overlap(i: usize, a: f32, b: f32) -> f32 {
    (b.min(i as f32 + 1.0) - a.max(i as f32)).clamp(0.0, 1.0)
}

/// Curly line: the centre of the band oscillates along the columns with a sine.
///
/// The wave band's **bottom is pinned below the underline**: `position +
/// CURL_FACTOR * thickness`, clipped to the cell base. The clipping is
/// **necessary** here — `rules::rule_envelope` only fits `position +
/// thickness` inside the cell, while the curl descends `CURL_FACTOR` times
/// that far and can overflow the base.
fn curl(target: &mut [u8], m: Metrics, position: f32, thickness: f32) {
    let (w, h) = m.cell_wh();
    // The wave band grows **downward** from the underline, not upward: the
    // rows between the underline and the cell base are empty and far from the
    // glyph body. If it grew upward, the wave's crest would merge with the
    // letters' bases (13pt: crest 12 = the last body row of `a e o`).
    // The amplitude is derived here, not in `Metrics`: it is not a measure that
    // comes from the font but a design constant of this drawer. `Metrics`
    // stays "cell geometry derived from the font" — putting a constant with a
    // single consumer into a `pub` field would expose it to `bt-gpu` as well.
    let top = position;
    let bottom = (position + CURL_FACTOR * thickness).min(h as f32);
    // Keep the centre axis inside the band: half the thickness is left as margin.
    let (y_bottom, y_top) = (bottom - thickness / 2.0, top + thickness / 2.0);
    let mid = (y_bottom + y_top) / 2.0;
    let amplitude = (y_bottom - y_top) / 2.0;
    for x in 0..w {
        // Sampled at the pixel **centre**, and exactly `WAVE_COUNT` waves fit
        // in one cell: when the sprite is tiled with its neighbours the phase
        // does not break (see `WAVE_COUNT`).
        let phase = core::f32::consts::TAU * (x as f32 + 0.5) * WAVE_COUNT / w as f32;
        let center = mid + amplitude * phase.sin();
        let (y0, y1) = (center - thickness / 2.0, center + thickness / 2.0);
        for y in 0..h {
            let value = coverage(y, y0, y1);
            if value > 0 {
                // audit: `y < h` and `x < w`.
                target[y * w + x] = value;
            }
        }
    }
}

/// Procedurally drawn character families.
///
/// The second set next to [`RuleKind`]: they do not come from the font, they
/// are computed from the cell measure and are independent of the face. What
/// differs is that they are **characters** — they live in the atlas as
/// [`crate::Sprite::Char`], so `bt-gpu` does not tell them apart from
/// ordinary letters and the boundary does not change at all.
enum Family {
    /// U+2580–U+259F — block elements: halves, eighth-steps, quadrants and
    /// three shades.
    Block,
    /// U+2800–U+28FF — Braille pattern; the low 8 bits are directly the dot
    /// mask.
    Braille,
    /// U+2500–U+257F — box drawing: four arms × {none, light, heavy, double},
    /// the dashed family and rounded corners. **Except the diagonals**, see
    /// [`family`].
    Line,
    /// U+23B8–U+23BF — the terminal's graphics set: two vertical box lines,
    /// four scan lines and two corners. A relative of [`Line`](Family::Line)
    /// but its axes are **on the edge**, not the centre; the reasoning is in
    /// [`technical`].
    Technical,
}

/// The character's procedural family — **the single owner of coverage**.
///
/// [`is_procedural`] and [`draw_procedural`] call the same function, because
/// they are read from **two separate** arms of `Atlas::slot`: one in
/// normalisation ("is this character face-insensitive"), the other in drawing
/// ("will we ask the font"). Two copies would drift silently, and the dangerous
/// direction of the drift is the silent one: were it present at the gate and
/// absent from normalisation, the same bitmap would hold four separate slots
/// for four faces (`Atlas::slot`'s own doc).
fn family(ch: char) -> Option<Family> {
    match ch {
        '\u{2580}'..='\u{259F}' => Some(Family::Block),
        '\u{2800}'..='\u{28FF}' => Some(Family::Braille),
        // The diagonals (`╱╲╳`) are a **deliberately left hole** inside the
        // coverage: the distance field could have drawn them too, but all three
        // are rare and the measure was to keep the coverage closed. This arm
        // must stand **above** the range below, otherwise the hole closes.
        //
        // The hole has a second job and that is deliberate too: these three
        // characters are the only remaining block in Menlo Regular that is
        // absent from Bold, i.e. the fixture of
        // `face_fallback_is_cached_under_the_requested_face` (the only guard on
        // this machine for the fallback's face ladder arm) lives here.
        '\u{2571}'..='\u{2573}' => None,
        '\u{2500}'..='\u{257F}' => Some(Family::Line),
        // U+23B7 (`⎷` RADICAL SYMBOL BOTTOM) stands **below** the range and
        // that too is a deliberately left hole: the radical sign's tail is not
        // a rail, so [`technical`]'s geometry cannot draw it. It comes from the
        // cascade and passes the gate (measured, this machine, Menlo 16pt:
        // Apple Symbols, ink 0.88 of the cell), so taking it into the coverage
        // would replace a working glyph, not fix a defect.
        '\u{23B8}'..='\u{23BF}' => Some(Family::Technical),
        _ => None,
    }
}

/// Does the character come from the terminal rather than the font?
///
/// **Procedural drawing beats the font unconditionally** and this is a
/// decision: even if the user picks a font that carries these characters,
/// procedural drawing wins. The reason is tiling — the font's em box is not
/// the cell box and there is no criterion that guarantees a font will provide
/// it. Measured (reported by the user with a screenshot): in
/// Menlo 13pt only rows 3–16 of the cell are painted (the cell was 8×18
/// then), so a stripe of several pixels remains between two stacked `█`. The same as the prompt
/// mark decision: *the mark is the terminal's own, not the user's font's.*
pub(crate) fn is_procedural(ch: char) -> bool {
    family(ch).is_some()
}

/// Draws the coverage bytes of a procedural character into `target`.
///
/// `m` is the cell the sprite *is*: the large **grid** cell, or in the small
/// class the small face's own cell (`Atlas::small_metrics`) — `target`
/// is that cell's buffer, not a slot, and the atlas places it into the slot
/// (below `1` the slot is larger than the cell).
///
/// The twin of [`draw_rule`], starting with the same two opening lines; the
/// reasons are the same too. It cannot fail — no font is asked, no context is
/// set up — so the caller's `Drawn` is not an assumption but the type itself.
///
/// An out-of-coverage character leaves an **empty slot**, not a panic: the
/// caller ([`is_procedural`]) already holds the gate, but a panic path in a
/// drawer has no counterpart of value — an empty cell is a visible and
/// diagnosable loss, a panic is the window itself.
pub(crate) fn draw_procedural(ch: char, m: Metrics, target: &mut [u8]) {
    // audit: the same precondition as `draw`/`draw_rule`, the same reasoning.
    assert_eq!(
        target.len(),
        m.slot_bytes(),
        "the buffer must be exactly one slot"
    );
    // The buffer is shared and holds the previous glyph's pixels.
    target.fill(0);

    match family(ch) {
        Some(Family::Block) => block(ch, m, target),
        Some(Family::Braille) => braille(ch, m, target),
        Some(Family::Line) => line(ch, m, target),
        Some(Family::Technical) => technical(ch, m, target),
        None => {}
    }
}

/// Fractional rectangle, coverage **summed** — for tiling parts.
///
/// The difference between summing and [`max_rect`] is not a taste but a
/// criterion: the union of parts that tile each other (disjoint) must give
/// **full** coverage. `▀` and `▄` meet at row 16.5 of 13pt@2x's h = 33 and both
/// leave 128 in that row; with `max` a 50% stripe would remain in the middle of
/// the cell — the defect procedural drawing exists to close, moved inside the cell.
/// Saturating addition closes it to 255.
fn add_rect(target: &mut [u8], m: Metrics, x0: f32, x1: f32, y0: f32, y1: f32) {
    rect(target, m, x0, x1, y0, y1, u8::saturating_add);
}

/// Fractional rectangle, coverage by **pixel-max** — overlapping ink.
///
/// Braille's dots are separate blobs of ink; when the geometry degenerates and
/// two dots touch the same pixel, summing would inflate them to a false
/// thickness. `max` keeps the blobs honest and makes the invariant "the
/// sprite of a mask = the pixel-max of the sprites of its set bits"
/// **structural**: it comes from the combiner itself, not from the dots being
/// disjoint.
fn max_rect(target: &mut [u8], m: Metrics, x0: f32, x1: f32, y0: f32, y1: f32) {
    rect(target, m, x0, x1, y0, y1, u8::max);
}

/// Fractional rectangle on two axes; antialiasing comes for free from
/// [`overlap`].
///
/// The ratios are **multiplied and rounded once** (see [`overlap`]).
fn rect(target: &mut [u8], m: Metrics, x0: f32, x1: f32, y0: f32, y1: f32, join: fn(u8, u8) -> u8) {
    let (w, h) = m.cell_wh();
    for y in 0..h {
        let ry = overlap(y, y0, y1);
        if ry == 0.0 {
            continue;
        }
        for x in 0..w {
            let value = (ry * overlap(x, x0, x1) * 255.0).round() as u8;
            if value > 0 {
                // audit: `y < h` and `x < w`, so the index is below `w * h`.
                target[y * w + x] = join(target[y * w + x], value);
            }
        }
    }
}

/// Coverage ratios of the three shades (`░▒▓`, U+2591–U+2593).
///
/// **A design constant, not a measurement** (like `CURL_FACTOR`), and the
/// numbers come from the characters' own definitions: quarter, half,
/// three-quarter density.
///
/// There is **no pattern, only flat coverage** and this is deliberate: CP437's
/// checkerboard was a density trick for one-bit displays, while the atlas is
/// eight-bit. Had a checkerboard been written, its phase would hold only if
/// the step divided **both** measures of the cell, and not every size gives
/// such a cell — on this machine 13pt@1x's cell is 8×17 and 17 is odd, so the
/// pattern would break
/// at every row boundary and horizontal stripes would appear in an area full
/// of `░`. Tiling is the reason procedural drawing exists; flat coverage gives it by
/// construction.
const SHADE_LEVELS: [f32; 3] = [0.25, 0.5, 0.75];

/// Bits of the quadrant mask: upper left, upper right, lower left, lower right.
const UL: u8 = 1;
const UR: u8 = 2;
const LL: u8 = 4;
const LR: u8 = 8;

/// Quadrant masks of U+2596–U+259F — **a real table**, no formula.
///
/// The order is Unicode's own and has no pattern (`▖▗▘▙▚▛▜▝▞▟`): single
/// quadrants are split into three, triples are sprinkled in between. The table
/// was written from the characters' names, not from a counter.
const QUADRANTS: [(char, u8); 10] = [
    ('\u{2596}', LL),           // ▖ QUADRANT LOWER LEFT
    ('\u{2597}', LR),           // ▗ QUADRANT LOWER RIGHT
    ('\u{2598}', UL),           // ▘ QUADRANT UPPER LEFT
    ('\u{2599}', UL | LL | LR), // ▙ UPPER LEFT AND LOWER LEFT AND LOWER RIGHT
    ('\u{259A}', UL | LR),      // ▚ UPPER LEFT AND LOWER RIGHT
    ('\u{259B}', UL | UR | LL), // ▛ UPPER LEFT AND UPPER RIGHT AND LOWER LEFT
    ('\u{259C}', UL | UR | LR), // ▜ UPPER LEFT AND UPPER RIGHT AND LOWER RIGHT
    ('\u{259D}', UR),           // ▝ QUADRANT UPPER RIGHT
    ('\u{259E}', UR | LL),      // ▞ UPPER RIGHT AND LOWER LEFT
    ('\u{259F}', UR | LL | LR), // ▟ UPPER RIGHT AND LOWER LEFT AND LOWER RIGHT
];

/// Block elements (U+2580–U+259F).
///
/// The family has three parts and only the last wants a table: **two
/// arithmetic runs** (eighth-steps from the bottom and from the left, halves
/// being their fourth step), **three shades** and **ten quadrants**.
///
/// Eighth slices are divided from the cell's own measure, not from a fixed
/// pixel count: `h / 8` stays fractional and the antialiasing comes from
/// [`rect`], so the staircase is monotonic at every point size and `█` is full
/// at every point size.
fn block(ch: char, m: Metrics, target: &mut [u8]) {
    let (w, h) = m.cell_wh();
    let (w, h) = (w as f32, h as f32);
    let cp = u32::from(ch);
    match ch {
        // ▀ upper half. Not the mirror of the lower staircase but its own
        // character: Unicode starts the lower staircase at 2581 and the left
        // staircase at 258F, and placed the upper half outside both, at the
        // head of the range.
        '\u{2580}' => add_rect(target, m, 0.0, w, 0.0, h / 2.0),
        // ▁▂▃▄▅▆▇█ — n/8 from the bottom; the eighth is the full block.
        '\u{2581}'..='\u{2588}' => {
            let n = (cp - 0x2580) as f32;
            add_rect(target, m, 0.0, w, h - h * n / 8.0, h);
        }
        // ▉▊▋▌▍▎▏ — n/8 from the left, but **decreasing**: 2589 is seven
        // eighths, 258F is one eighth. The slice thins as the code point grows,
        // so the counter counts down from 0x2590.
        '\u{2589}'..='\u{258F}' => {
            let n = (0x2590 - cp) as f32;
            add_rect(target, m, 0.0, w * n / 8.0, 0.0, h);
        }
        // ▐ right half.
        '\u{2590}' => add_rect(target, m, w / 2.0, w, 0.0, h),
        // ░▒▓ — flat coverage (see [`SHADE_LEVELS`]).
        '\u{2591}'..='\u{2593}' => {
            let level = SHADE_LEVELS[(cp - 0x2591) as usize];
            let value = (level * 255.0).round() as u8;
            target.fill(value);
        }
        // ▔ upper one eighth.
        '\u{2594}' => add_rect(target, m, 0.0, w, 0.0, h / 8.0),
        // ▕ right one eighth.
        '\u{2595}' => add_rect(target, m, w - w / 8.0, w, 0.0, h),
        // ▖▗▘▙▚▛▜▝▞▟ — quadrants, from the table.
        _ => {
            let mask = QUADRANTS
                .iter()
                .find(|&&(c, _)| c == ch)
                .map_or(0, |&(_, mask)| mask);
            // Quadrants **tile disjointly**: the union of all four is `█`. The
            // fractional row/column in the middle takes one share from each of
            // two quadrants and [`add_rect`] closes them to 255.
            for (bit, (x0, x1, y0, y1)) in [
                (UL, (0.0, w / 2.0, 0.0, h / 2.0)),
                (UR, (w / 2.0, w, 0.0, h / 2.0)),
                (LL, (0.0, w / 2.0, h / 2.0, h)),
                (LR, (w / 2.0, w, h / 2.0, h)),
            ] {
                if mask & bit != 0 {
                    add_rect(target, m, x0, x1, y0, y1);
                }
            }
        }
    }
}

/// The fraction of its own sub-cell a Braille dot fills.
///
/// **A design constant, not a measurement** (like `CURL_FACTOR`). It holds two
/// things at once: a dot big enough to be visible at small point sizes, small
/// enough to be separated from its neighbours. A ratio — not an absolute pixel
/// count — so the dot grows as the point size and scale grow; a fixed pixel
/// radius would turn into a pinhead at @2x.
///
/// The dot is a **rectangle**; the distance field a round one wants is a
/// separate primitive, and bringing it in just for this would be a
/// cost with no return — at 13pt the sub-cell is 4×4.25 pixels, and the
/// difference between a square and a circle there is under one pixel.
const BRAILLE_DOT_FILL: f32 = 0.7;

/// Braille pattern (U+2800–U+28FF).
///
/// **No table:** the low 8 bits of the code point are directly the dot mask —
/// the Unicode block is defined exactly this way. The bit's cell comes from
/// the definition too: the dots are on a 2×4 grid and the numbering
/// historically fills the 2×3 cell first, the fourth row having been added
/// later —
///
/// ```text
///   bit0  bit3        1 4
///   bit1  bit4   =    2 5
///   bit2  bit5        3 6
///   bit6  bit7        7 8
/// ```
///
/// that is, bits 0–2 are the first three rows of the left column, bits 3–5 the
/// first three rows of the right column, and bits 6 and 7 the left and right
/// of the fourth row.
fn braille(ch: char, m: Metrics, target: &mut [u8]) {
    let (w, h) = m.cell_wh();
    let (dot_w, dot_h) = (w as f32 / 2.0, h as f32 / 4.0);
    let mask = u32::from(ch) & 0xFF;
    for bit in 0..8u32 {
        if mask & (1 << bit) == 0 {
            continue;
        }
        let (col, row) = match bit {
            0..=2 => (0, bit),
            3..=5 => (1, bit - 3),
            6 => (0, 3),
            _ => (1, 3),
        };
        let (cx, cy) = ((col as f32 + 0.5) * dot_w, (row as f32 + 0.5) * dot_h);
        let (half_w, half_h) = (
            dot_w * BRAILLE_DOT_FILL / 2.0,
            dot_h * BRAILLE_DOT_FILL / 2.0,
        );
        max_rect(
            target,
            m,
            cx - half_w,
            cx + half_w,
            cy - half_h,
            cy + half_h,
        );
    }
}

/// Ratio of the heavy line to the light line.
///
/// **A design constant, not a measurement** ([`CURL_FACTOR`]'s counterpart).
/// Unicode puts light and heavy in separate code points (`─` U+2500 light, `━`
/// U+2501 heavy) but does not say *how much* thicker the heavy is: the light
/// comes from [`Metrics::underline_px`] (the repository already has an answer
/// to "line thickness"), and the heavy needs a second number. Double was
/// chosen — one and a half times is indistinguishable from light once rounded
/// to an integer at 13pt, three times eats a third of the cell width.
const HEAVY_FACTOR: f32 = 2.0;

/// Line style of an arm.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Stroke {
    None,
    Light,
    Heavy,
    /// Two light rails; a gap of one thickness between them.
    Double,
}

// Arm indices. The opposite arm is `dir ^ 1`: the pairs are deliberately adjacent.
const UP: usize = 0;
const DOWN: usize = 1;
const LEFT: usize = 2;
const RIGHT: usize = 3;

/// Geometry recipe of a box-drawing character.
///
/// The arms are in the order [`UP`], [`DOWN`], [`LEFT`], [`RIGHT`]; if
/// `dashes` is zero the line is solid, otherwise that many dashes per cell;
/// `arc` rounds the corner.
#[derive(Clone, Copy)]
struct Recipe {
    arms: [Stroke; 4],
    dashes: u8,
    arc: bool,
}

const N: Stroke = Stroke::None;
const L: Stroke = Stroke::Light;
const H: Stroke = Stroke::Heavy;
const D: Stroke = Stroke::Double;

const fn plain(arms: [Stroke; 4]) -> Recipe {
    Recipe {
        arms,
        dashes: 0,
        arc: false,
    }
}

const fn dashed(arms: [Stroke; 4], dashes: u8) -> Recipe {
    Recipe {
        arms,
        dashes,
        arc: false,
    }
}

const fn arc(arms: [Stroke; 4]) -> Recipe {
    Recipe {
        arms,
        dashes: 0,
        arc: true,
    }
}

/// Arm tables of U+2500–U+257F — **a real table**, no formula.
///
/// The order is the code point order (index = `cp - 0x2500`) and not a
/// counter: the heavy masks of `251C..2523` (up, down, right) are, in order,
/// 000, 001, 100, 010, 110, 101, 011, 111, i.e. a **permutation**; the same
/// family's `252C..2533` counterpart (left, right, down) is 000, 100, 010, 110,
/// 001, 101, 011, 111 — another permutation. Looking for a formula would
/// silently encode a wrong pattern; the table was written from the character
/// **names** and its guard is on the [`crate::tests`] side, which reads the
/// names from the Unicode database
/// (`the_arms_come_from_the_unicode_names`).
///
/// The rows of the diagonals (`╱╲╳`) are empty: [`family`] keeps them outside
/// the coverage, so these three rows are never read. The table is still 128
/// rows, because the index arithmetic cannot skip the hole.
// `rustfmt::skip`: one character per row and aligned name comments are the
// only reason the table can be scanned by eye; the formatter breaks the wraps.
#[rustfmt::skip]
const LINES: [Recipe; 128] = [
    plain([N, N, L, L]),      // ─ LIGHT HORIZONTAL
    plain([N, N, H, H]),      // ━ HEAVY HORIZONTAL
    plain([L, L, N, N]),      // │ LIGHT VERTICAL
    plain([H, H, N, N]),      // ┃ HEAVY VERTICAL
    dashed([N, N, L, L], 3),  // ┄ LIGHT TRIPLE DASH HORIZONTAL
    dashed([N, N, H, H], 3),  // ┅ HEAVY TRIPLE DASH HORIZONTAL
    dashed([L, L, N, N], 3),  // ┆ LIGHT TRIPLE DASH VERTICAL
    dashed([H, H, N, N], 3),  // ┇ HEAVY TRIPLE DASH VERTICAL
    dashed([N, N, L, L], 4),  // ┈ LIGHT QUADRUPLE DASH HORIZONTAL
    dashed([N, N, H, H], 4),  // ┉ HEAVY QUADRUPLE DASH HORIZONTAL
    dashed([L, L, N, N], 4),  // ┊ LIGHT QUADRUPLE DASH VERTICAL
    dashed([H, H, N, N], 4),  // ┋ HEAVY QUADRUPLE DASH VERTICAL
    plain([N, L, N, L]),      // ┌ LIGHT DOWN AND RIGHT
    plain([N, L, N, H]),      // ┍ DOWN LIGHT AND RIGHT HEAVY
    plain([N, H, N, L]),      // ┎ DOWN HEAVY AND RIGHT LIGHT
    plain([N, H, N, H]),      // ┏ HEAVY DOWN AND RIGHT
    plain([N, L, L, N]),      // ┐ LIGHT DOWN AND LEFT
    plain([N, L, H, N]),      // ┑ DOWN LIGHT AND LEFT HEAVY
    plain([N, H, L, N]),      // ┒ DOWN HEAVY AND LEFT LIGHT
    plain([N, H, H, N]),      // ┓ HEAVY DOWN AND LEFT
    plain([L, N, N, L]),      // └ LIGHT UP AND RIGHT
    plain([L, N, N, H]),      // ┕ UP LIGHT AND RIGHT HEAVY
    plain([H, N, N, L]),      // ┖ UP HEAVY AND RIGHT LIGHT
    plain([H, N, N, H]),      // ┗ HEAVY UP AND RIGHT
    plain([L, N, L, N]),      // ┘ LIGHT UP AND LEFT
    plain([L, N, H, N]),      // ┙ UP LIGHT AND LEFT HEAVY
    plain([H, N, L, N]),      // ┚ UP HEAVY AND LEFT LIGHT
    plain([H, N, H, N]),      // ┛ HEAVY UP AND LEFT
    plain([L, L, N, L]),      // ├ LIGHT VERTICAL AND RIGHT
    plain([L, L, N, H]),      // ┝ VERTICAL LIGHT AND RIGHT HEAVY
    plain([H, L, N, L]),      // ┞ UP HEAVY AND RIGHT DOWN LIGHT
    plain([L, H, N, L]),      // ┟ DOWN HEAVY AND RIGHT UP LIGHT
    plain([H, H, N, L]),      // ┠ VERTICAL HEAVY AND RIGHT LIGHT
    plain([H, L, N, H]),      // ┡ DOWN LIGHT AND RIGHT UP HEAVY
    plain([L, H, N, H]),      // ┢ UP LIGHT AND RIGHT DOWN HEAVY
    plain([H, H, N, H]),      // ┣ HEAVY VERTICAL AND RIGHT
    plain([L, L, L, N]),      // ┤ LIGHT VERTICAL AND LEFT
    plain([L, L, H, N]),      // ┥ VERTICAL LIGHT AND LEFT HEAVY
    plain([H, L, L, N]),      // ┦ UP HEAVY AND LEFT DOWN LIGHT
    plain([L, H, L, N]),      // ┧ DOWN HEAVY AND LEFT UP LIGHT
    plain([H, H, L, N]),      // ┨ VERTICAL HEAVY AND LEFT LIGHT
    plain([H, L, H, N]),      // ┩ DOWN LIGHT AND LEFT UP HEAVY
    plain([L, H, H, N]),      // ┪ UP LIGHT AND LEFT DOWN HEAVY
    plain([H, H, H, N]),      // ┫ HEAVY VERTICAL AND LEFT
    plain([N, L, L, L]),      // ┬ LIGHT DOWN AND HORIZONTAL
    plain([N, L, H, L]),      // ┭ LEFT HEAVY AND RIGHT DOWN LIGHT
    plain([N, L, L, H]),      // ┮ RIGHT HEAVY AND LEFT DOWN LIGHT
    plain([N, L, H, H]),      // ┯ DOWN LIGHT AND HORIZONTAL HEAVY
    plain([N, H, L, L]),      // ┰ DOWN HEAVY AND HORIZONTAL LIGHT
    plain([N, H, H, L]),      // ┱ RIGHT LIGHT AND LEFT DOWN HEAVY
    plain([N, H, L, H]),      // ┲ LEFT LIGHT AND RIGHT DOWN HEAVY
    plain([N, H, H, H]),      // ┳ HEAVY DOWN AND HORIZONTAL
    plain([L, N, L, L]),      // ┴ LIGHT UP AND HORIZONTAL
    plain([L, N, H, L]),      // ┵ LEFT HEAVY AND RIGHT UP LIGHT
    plain([L, N, L, H]),      // ┶ RIGHT HEAVY AND LEFT UP LIGHT
    plain([L, N, H, H]),      // ┷ UP LIGHT AND HORIZONTAL HEAVY
    plain([H, N, L, L]),      // ┸ UP HEAVY AND HORIZONTAL LIGHT
    plain([H, N, H, L]),      // ┹ RIGHT LIGHT AND LEFT UP HEAVY
    plain([H, N, L, H]),      // ┺ LEFT LIGHT AND RIGHT UP HEAVY
    plain([H, N, H, H]),      // ┻ HEAVY UP AND HORIZONTAL
    plain([L, L, L, L]),      // ┼ LIGHT VERTICAL AND HORIZONTAL
    plain([L, L, H, L]),      // ┽ LEFT HEAVY AND RIGHT VERTICAL LIGHT
    plain([L, L, L, H]),      // ┾ RIGHT HEAVY AND LEFT VERTICAL LIGHT
    plain([L, L, H, H]),      // ┿ VERTICAL LIGHT AND HORIZONTAL HEAVY
    plain([H, L, L, L]),      // ╀ UP HEAVY AND DOWN HORIZONTAL LIGHT
    plain([L, H, L, L]),      // ╁ DOWN HEAVY AND UP HORIZONTAL LIGHT
    plain([H, H, L, L]),      // ╂ VERTICAL HEAVY AND HORIZONTAL LIGHT
    plain([H, L, H, L]),      // ╃ LEFT UP HEAVY AND RIGHT DOWN LIGHT
    plain([H, L, L, H]),      // ╄ RIGHT UP HEAVY AND LEFT DOWN LIGHT
    plain([L, H, H, L]),      // ╅ LEFT DOWN HEAVY AND RIGHT UP LIGHT
    plain([L, H, L, H]),      // ╆ RIGHT DOWN HEAVY AND LEFT UP LIGHT
    plain([H, L, H, H]),      // ╇ DOWN LIGHT AND UP HORIZONTAL HEAVY
    plain([L, H, H, H]),      // ╈ UP LIGHT AND DOWN HORIZONTAL HEAVY
    plain([H, H, H, L]),      // ╉ RIGHT LIGHT AND LEFT VERTICAL HEAVY
    plain([H, H, L, H]),      // ╊ LEFT LIGHT AND RIGHT VERTICAL HEAVY
    plain([H, H, H, H]),      // ╋ HEAVY VERTICAL AND HORIZONTAL
    dashed([N, N, L, L], 2),  // ╌ LIGHT DOUBLE DASH HORIZONTAL
    dashed([N, N, H, H], 2),  // ╍ HEAVY DOUBLE DASH HORIZONTAL
    dashed([L, L, N, N], 2),  // ╎ LIGHT DOUBLE DASH VERTICAL
    dashed([H, H, N, N], 2),  // ╏ HEAVY DOUBLE DASH VERTICAL
    plain([N, N, D, D]),      // ═ DOUBLE HORIZONTAL
    plain([D, D, N, N]),      // ║ DOUBLE VERTICAL
    plain([N, L, N, D]),      // ╒ DOWN SINGLE AND RIGHT DOUBLE
    plain([N, D, N, L]),      // ╓ DOWN DOUBLE AND RIGHT SINGLE
    plain([N, D, N, D]),      // ╔ DOUBLE DOWN AND RIGHT
    plain([N, L, D, N]),      // ╕ DOWN SINGLE AND LEFT DOUBLE
    plain([N, D, L, N]),      // ╖ DOWN DOUBLE AND LEFT SINGLE
    plain([N, D, D, N]),      // ╗ DOUBLE DOWN AND LEFT
    plain([L, N, N, D]),      // ╘ UP SINGLE AND RIGHT DOUBLE
    plain([D, N, N, L]),      // ╙ UP DOUBLE AND RIGHT SINGLE
    plain([D, N, N, D]),      // ╚ DOUBLE UP AND RIGHT
    plain([L, N, D, N]),      // ╛ UP SINGLE AND LEFT DOUBLE
    plain([D, N, L, N]),      // ╜ UP DOUBLE AND LEFT SINGLE
    plain([D, N, D, N]),      // ╝ DOUBLE UP AND LEFT
    plain([L, L, N, D]),      // ╞ VERTICAL SINGLE AND RIGHT DOUBLE
    plain([D, D, N, L]),      // ╟ VERTICAL DOUBLE AND RIGHT SINGLE
    plain([D, D, N, D]),      // ╠ DOUBLE VERTICAL AND RIGHT
    plain([L, L, D, N]),      // ╡ VERTICAL SINGLE AND LEFT DOUBLE
    plain([D, D, L, N]),      // ╢ VERTICAL DOUBLE AND LEFT SINGLE
    plain([D, D, D, N]),      // ╣ DOUBLE VERTICAL AND LEFT
    plain([N, L, D, D]),      // ╤ DOWN SINGLE AND HORIZONTAL DOUBLE
    plain([N, D, L, L]),      // ╥ DOWN DOUBLE AND HORIZONTAL SINGLE
    plain([N, D, D, D]),      // ╦ DOUBLE DOWN AND HORIZONTAL
    plain([L, N, D, D]),      // ╧ UP SINGLE AND HORIZONTAL DOUBLE
    plain([D, N, L, L]),      // ╨ UP DOUBLE AND HORIZONTAL SINGLE
    plain([D, N, D, D]),      // ╩ DOUBLE UP AND HORIZONTAL
    plain([L, L, D, D]),      // ╪ VERTICAL SINGLE AND HORIZONTAL DOUBLE
    plain([D, D, L, L]),      // ╫ VERTICAL DOUBLE AND HORIZONTAL SINGLE
    plain([D, D, D, D]),      // ╬ DOUBLE VERTICAL AND HORIZONTAL
    arc([N, L, N, L]),        // ╭ LIGHT ARC DOWN AND RIGHT
    arc([N, L, L, N]),        // ╮ LIGHT ARC DOWN AND LEFT
    arc([L, N, L, N]),        // ╯ LIGHT ARC UP AND LEFT
    arc([L, N, N, L]),        // ╰ LIGHT ARC UP AND RIGHT
    plain([N, N, N, N]),      // ╱ diagonal — out of coverage, this row is never read
    plain([N, N, N, N]),      // ╲ diagonal — out of coverage
    plain([N, N, N, N]),      // ╳ diagonal — out of coverage
    plain([N, N, L, N]),      // ╴ LIGHT LEFT
    plain([L, N, N, N]),      // ╵ LIGHT UP
    plain([N, N, N, L]),      // ╶ LIGHT RIGHT
    plain([N, L, N, N]),      // ╷ LIGHT DOWN
    plain([N, N, H, N]),      // ╸ HEAVY LEFT
    plain([H, N, N, N]),      // ╹ HEAVY UP
    plain([N, N, N, H]),      // ╺ HEAVY RIGHT
    plain([N, H, N, N]),      // ╻ HEAVY DOWN
    plain([N, N, L, H]),      // ╼ LIGHT LEFT AND HEAVY RIGHT
    plain([L, H, N, N]),      // ╽ LIGHT UP AND HEAVY DOWN
    plain([N, N, H, L]),      // ╾ HEAVY LEFT AND LIGHT RIGHT
    plain([H, L, N, N]),      // ╿ HEAVY UP AND LIGHT DOWN
];

/// A rail's band **seated on the pixel grid**: `[start, start +
/// thickness)`.
///
/// The rounding is mandatory and its cost is visible to the eye: on this
/// machine 13pt@1x's cell is 8×17, so the vertical line's axis is x = 4.0 and
/// an unrounded light band `[3.5, 4.5)` would fall 50% on each of two columns
/// — every vertical line grey, horizontals (axis 9.0) crisp. The rule is not
/// new: the underline's position and thickness are already integers
/// ([`Metrics::underline_px`]) and [`rules::rule_envelope`] ties the thickness
/// to `>= 1`, so with an integer `start` the band ends on integer edges and
/// [`overlap`] produces no fractions.
fn rail(center: f32, thickness: f32) -> (f32, f32) {
    let start = (center - thickness / 2.0).round();
    (start, start + thickness)
}

/// The bands of an arm's rails on the **perpendicular axis**.
///
/// The double line's rail spacing comes from [`RuleKind::Double`]'s current
/// `position + 2.0 * thickness`: exactly one thickness of gap is left between
/// the two rails. No new design constant arises.
fn rails(stroke: Stroke, center: f32, thin: f32) -> [Option<(f32, f32)>; 2] {
    match stroke {
        Stroke::None => [None, None],
        Stroke::Light => [Some(rail(center, thin)), None],
        Stroke::Heavy => [Some(rail(center, HEAVY_FACTOR * thin)), None],
        Stroke::Double => [
            Some(rail(center - thin, thin)),
            Some(rail(center + thin, thin)),
        ],
    }
}

/// Axis switch: `along` is the arm's axis of travel, `across` the rail's band.
///
/// It stands on [`max_rect`] and its combiner is deliberately **pixel-max**:
/// the arms share the same ink (they overlap at the crossing), so `add_rect`'s
/// saturating sum would darken the junction artificially. A structural side
/// benefit: the union law (`┌ ∪ ┘ == ┼`) follows from the same arm giving the
/// **same rectangle** in every character.
fn stroke_rect(
    target: &mut [u8],
    m: Metrics,
    vertical: bool,
    along: (f32, f32),
    across: (f32, f32),
) {
    if vertical {
        max_rect(target, m, across.0, across.1, along.0, along.1);
    } else {
        max_rect(target, m, along.0, along.1, across.0, across.1);
    }
}

/// How far inward from the edge a rail goes.
///
/// There are three cases, and all three come from the real geometry of double
/// line junctions:
///
/// - **No perpendicular arm** → the rail passes the centre (seated on the grid
///   with [`f32::round`]) and meets the opposite arm's rail exactly there. The
///   two arms of `─` join this way.
/// - **Passing** → the rail cuts through all perpendicular rails and ends at
///   the far edge of the farthest one. This is what closes the corner: `╔`'s
///   upper rail goes up to the **left** edge of the left vertical rail,
///   otherwise a notch would remain in the corner.
/// - **Turning** → the rail stops at the far edge of the first perpendicular
///   rail it meets, i.e. it makes an elbow into that rail. The four elbows of
///   `╬` and the empty channel in its middle come from this.
///
/// The `turn` decision itself is in [`arm`]; this only measures the result.
fn reach(bands: [Option<(f32, f32)>; 4], forward: bool, turn: bool, center: f32) -> f32 {
    // The axis is flipped **once**: for an arm travelling in the backward
    // direction all coordinates are negated, so "near" is always smaller and
    // "far" always larger. One flip at entry and one at exit; asking `forward`
    // separately in the near/far choice, in min/max and in the turn would
    // require five separate places to stay consistent with each other, and
    // when one was written backwards the symptom would be a one-pixel notch at
    // the `╬` junction.
    let travel = |value: f32| if forward { value } else { -value };
    let mut nearest: Option<(f32, f32)> = None;
    let mut farthest: Option<f32> = None;
    for band in bands.into_iter().flatten() {
        let (near, far) = (
            travel(band.0).min(travel(band.1)),
            travel(band.0).max(travel(band.1)),
        );
        if nearest.is_none_or(|(current, _)| near < current) {
            nearest = Some((near, far));
        }
        farthest = Some(farthest.map_or(far, |edge: f32| edge.max(far)));
    }
    let stop = match (turn, nearest, farthest) {
        (true, Some((_, far)), _) => far,
        (false, _, Some(far)) => far,
        // No perpendicular arm: we meet at the centre.
        _ => travel(center.round()),
    };
    travel(stop)
}

/// A single arm: one or two rails from the edge toward the centre.
///
/// The **turning rule** ([`reach`]'s third case) lives in two sentences and
/// both follow from a double line being a "channel" — a double line is not a
/// line but a channel with two walls, and a wall opening at a junction does not
/// close the channel:
///
/// - If the perpendicular arm on the **double rail's** own side is also double,
///   the rail turns: in `╠` the right (inner) wall breaks and makes an elbow
///   into the horizontal rails, while the left (outer) wall passes
///   uninterrupted.
/// - A **single rail** (light/heavy) turns only when **both** perpendicular
///   arms are double **and** it has no opposite arm: `╤`'s stem stops at the
///   lower rail, while `╪`'s vertical line passes from end to end. Without the
///   opposite-arm condition `╪` would be split in two at the middle.
fn arm(spec: Recipe, dir: usize, m: Metrics, target: &mut [u8]) {
    let stroke = spec.arms[dir];
    if stroke == Stroke::None {
        return;
    }
    let (w, h) = m.cell_wh();
    let (w, h) = (w as f32, h as f32);
    let thin = f32::from(m.underline_px.1).max(1.0);
    let vertical = dir == UP || dir == DOWN;
    // Edge to centre: the up and left arms advance increasing from 0, the down
    // and right arms decreasing from the cell's end.
    let forward = dir == UP || dir == LEFT;
    let (along_extent, across_center) = if vertical { (h, w / 2.0) } else { (w, h / 2.0) };
    let along_center = along_extent / 2.0;
    // The directions of the perpendicular arms **and** the rail's own side are
    // the same pair: a vertical arm's rails are on the left/right, a horizontal
    // arm's on the top/bottom.
    let sides = if vertical { [LEFT, RIGHT] } else { [UP, DOWN] };
    let crossing = {
        let first = rails(spec.arms[sides[0]], along_center, thin);
        let second = rails(spec.arms[sides[1]], along_center, thin);
        [first[0], first[1], second[0], second[1]]
    };
    let single_turns = spec.arms[sides[0]] == Stroke::Double
        && spec.arms[sides[1]] == Stroke::Double
        // The opposite arm: `dir ^ 1` (UP↔DOWN, LEFT↔RIGHT).
        && spec.arms[dir ^ 1] == Stroke::None;

    for (index, band) in rails(stroke, across_center, thin).into_iter().enumerate() {
        let Some(band) = band else { continue };
        let turn = if stroke == Stroke::Double {
            spec.arms[sides[index]] == Stroke::Double
        } else {
            single_turns
        };
        let edge = reach(crossing, forward, turn, along_center);
        let along = if forward {
            (0.0, edge)
        } else {
            (edge, along_extent)
        };
        stroke_rect(target, m, vertical, along, band);
    }
}

/// The dashed line family (`┄┅┆┇┈┉┊┋╌╍╎╏`).
///
/// The period comes from [`dividing_period`], i.e. it is pulled to the nearest
/// value that **divides the cell exactly**; tiling is the reason procedural
/// drawing exists and the phase cannot break at the cell boundary. The cost is a visible
/// loss of information and it was accepted: on
/// this machine at `w = 8` the period the triple dash wants, 3, is pulled to
/// **4** and `┄` and `╌` collapse into **the same sprite**; at a measure whose
/// divisors are sparse (144pt@1x's `w = 87`: 1, 3, 29, 87) `╌` drops to **one**
/// dash per cell. The guard does not write the collapse into a list, it
/// **derives** it: two densities on the same axis are equal only if their
/// periods are equal (`dashed_densities_collapse_only_with_the_period`).
///
/// The fill ratio is [`RuleKind::Dashed`]'s ratio (two thirds); no second
/// design constant arises.
fn dashes(spec: Recipe, m: Metrics, target: &mut [u8]) {
    let (w, h) = m.cell_wh();
    let thin = f32::from(m.underline_px.1).max(1.0);
    let vertical = spec.arms[UP] != Stroke::None;
    let (along_extent, across_center) = if vertical {
        (h, w as f32 / 2.0)
    } else {
        (w, h as f32 / 2.0)
    };
    let stroke = spec.arms[if vertical { UP } else { LEFT }];
    // There is no double line in the dashed family: a single rail.
    let Some(band) = rails(stroke, across_center, thin)[0] else {
        return;
    };
    let period = dividing_period(
        along_extent.div_ceil(usize::from(spec.dashes).max(1)),
        along_extent,
    );
    let filled = (period * 2 / 3).max(1);
    for start in (0..along_extent).step_by(period) {
        let end = (start + filled).min(along_extent);
        stroke_rect(target, m, vertical, (start as f32, end as f32), band);
    }
}

/// Rounded corner (`╭╮╯╰`): two stems and the quarter arc joining them.
///
/// The arc is the twin of [`chevron`]'s distance field and goes through the
/// same [`stamp`] — there `distance_to_segment`, here `|hypot(x - ox, y - oy) -
/// r|`. [`curl`] is **not** a precedent: it samples a single `y` per column and
/// would tear the band at the quarter arc's vertical tangent.
///
/// The radius is the same at all four corners and is derived from the **seated
/// axes**: the smallest of the centre's distances to the cell edges, **one
/// pixel inward**. That one pixel is the seam itself: had the radius gone all
/// the way to the boundary, the tangent point would fall on the cell's edge,
/// the **arc** and not the stem would paint the edge column, and the arc's
/// coverage would fall short of the band's — measured (13pt@1x, `╭`'s right
/// edge): 246 instead of 255 in the rail's row, 13 instead of 0 in the row
/// below. The symptom would be the two corners of a box diverging, because
/// after the axis is seated it is not at the middle of the cell (4.5 versus
/// 4.0 at 13pt) and the smallest distance always comes from a **single** edge:
/// `╭` and `╰` would be left without a stem while `╮` and `╯` kept theirs. With
/// the inset radius the tangent point stays outside the edge pixel, the
/// quarter constraint never gives that pixel to the arc, and the seam is
/// **bit for bit** the same as `─`'s (`arms_tile_across_the_cell_edge` now
/// tests the arcs too).
///
/// Had it been derived from a single direction, the left corner of `╭──╮`
/// would be narrower than the right; had the tangent points been taken from the
/// unseated axis, a half-pixel break would remain between stem and arc.
fn corner(spec: Recipe, m: Metrics, target: &mut [u8]) {
    let (w, h) = m.cell_wh();
    let (w, h) = (w as f32, h as f32);
    let thin = f32::from(m.underline_px.1).max(1.0);
    let vertical_band = rail(w / 2.0, thin);
    let horizontal_band = rail(h / 2.0, thin);
    let axis_x = (vertical_band.0 + vertical_band.1) / 2.0;
    let axis_y = (horizontal_band.0 + horizontal_band.1) / 2.0;
    // One pixel inward and `max(0.0)`: in a narrow cell the radius drops to
    // zero, the corner sharpens and the stem fills the cell from end to end —
    // a lost roundness, not a broken seam.
    let radius = (axis_x.min(w - axis_x).min(axis_y).min(h - axis_y) - 1.0).max(0.0);
    let right = spec.arms[RIGHT] != Stroke::None;
    let down = spec.arms[DOWN] != Stroke::None;
    let ox = if right {
        axis_x + radius
    } else {
        axis_x - radius
    };
    let oy = if down {
        axis_y + radius
    } else {
        axis_y - radius
    };

    // The arc **first**: [`stamp`] writes the whole cell (it has no combiner)
    // and the buffer is clean at this point ([`draw_procedural`] zeroed it).
    // The stems then overlap on top with `max`; were the order reversed, the
    // arc would erase the stems.
    stamp(target, m, thin / 2.0, |px, py| {
        // Quarter constraint: the arc is on the side of the centre **opposite
        // the arms**. Outside is infinite distance, i.e. zero coverage — the
        // cut is exactly at the tangent point and the stem paints beyond it.
        let inside =
            if right { px <= ox } else { px >= ox } && if down { py <= oy } else { py >= oy };
        if inside {
            ((px - ox).hypot(py - oy) - radius).abs()
        } else {
            f32::INFINITY
        }
    });

    // Stems: from the tangent point to the cell edge.
    let (x0, x1) = if right { (ox, w) } else { (0.0, ox) };
    let (y0, y1) = if down { (oy, h) } else { (0.0, oy) };
    max_rect(target, m, x0, x1, horizontal_band.0, horizontal_band.1);
    max_rect(target, m, vertical_band.0, vertical_band.1, y0, y1);
}

/// Box-drawing character (U+2500–U+257F).
///
/// The recipe comes from [`LINES`]; the drawing splits into three arms and all
/// three use the same two primitives (rectangle and distance field). An
/// out-of-coverage index leaves an **empty slot**, not a panic: the gate
/// ([`family`]) already holds it.
fn line(ch: char, m: Metrics, target: &mut [u8]) {
    // The subtraction is **checked**: `.get()` holds only the upper bound, a
    // character below the range would overflow the `u32` subtraction and panic
    // in a debug build. It is unreachable today (the gate is in [`family`]) but
    // [`draw_procedural`]'s doc says there is **no** panic path, and the
    // defence must hold in both directions in which that promise applies.
    let Some(&spec) = u32::from(ch)
        .checked_sub(0x2500)
        .and_then(|index| LINES.get(index as usize))
    else {
        return;
    };
    if spec.dashes > 0 {
        dashes(spec, m, target);
    } else if spec.arc {
        corner(spec, m, target);
    } else {
        for dir in [UP, DOWN, LEFT, RIGHT] {
            arm(spec, dir, m, target);
        }
    }
}

/// Numbers of the scan lines (`⎺⎻⎼⎽`, U+23BA–U+23BD).
///
/// The names say it: "HORIZONTAL SCAN LINE-1/-3/-7/-9". The table is not a
/// counter — the order is 1, 3, 7, 9 and the 5 in between is **absent**,
/// because Unicode merged it with `─` (U+2500).
const SCAN_LINES: [f32; 4] = [1.0, 3.0, 7.0, 9.0];

/// The scan line's vertical centre in the cell.
///
/// The name says a **line**: DEC's character cell has nine scan lines and the
/// Nth line is the Nth ninth-wide band of the cell, so its centre is
/// `(N - 0.5) / 9`. The witness that the formula invents no second number is
/// the fifth line: `(5 - 0.5) / 9` is exactly `0.5`, i.e. the line Unicode
/// merged with `─` lands on the very spot where [`arm`] puts the horizontal
/// arm. The two families share the same grid; there is no shared constant, but
/// shared **geometry**.
fn scan_centre(line: f32, h: f32) -> f32 {
    (line - 0.5) / 9.0 * h
}

/// Draws the terminal's graphics set (U+23B8–U+23BF) into `target`.
///
/// A relative of [`line`] using the same rails ([`rail`], [`max_rect`]); the
/// one place it departs is **where the axis is**: the arms of the U+2500 family
/// meet in the middle of the cell, while this set's lines are by definition
/// **on the edge** — "LEFT/RIGHT VERTICAL BOX LINE" is the cell's left/right
/// edge, the scan lines are bands that divide the cell into nine, and the two
/// dentistry corners are an "L" following the edges. That is why no row is
/// added to the [`Recipe`] table: `LINES`'s index is `cp - 0x2500` and the
/// arms' axis is written as `w / 2` / `h / 2`.
///
/// **Why procedural.** All three came from the font and each had its own
/// defect (measured, this machine, Menlo 16pt@2x, cell 20×39):
///
/// - `⎾` `⎿` **came out as a box** — the cascade gives Hiragino Sans, its
///   advance is 1.66 times the cell and the half-width glyph leans on the
///   **right half** of that box (ink 15.36–32.00 px), so the ink gate was
///   rightly eliminating it. The symptom was seen by the user: Claude Code
///   starts tool results with `⎿`.
/// - `⎸` `⎹` were drawn **in the wrong place** — Apple Symbols' advance is 0.42
///   of the cell, so [`rules::centre_shift`] shifted them to the middle of the
///   cell and the "left edge line" did not stay on the left.
/// - `⎺⎻⎼⎽` **did not tile** — Monaco's ink spans 0.03–19.19 in a 20 px cell,
///   so a 0.8 px gap is left at the right end of every cell and scan lines
///   laid side by side look dashed. The block elements' thesis holds here too: there is no
///   guarantee that the font's em box will be the cell box.
///
/// The vertical extent is the **full cell** and this too is from measurement:
/// the eliminated candidate's ink spans from 0.047 to 0.868 of the cell
/// height, so the shape fills the cell — drawing `⎿` like `└` (axis at the
/// centre) would reduce it to a half-height corner.
fn technical(ch: char, m: Metrics, target: &mut [u8]) {
    let (w, h) = m.cell_wh();
    let (w, h) = (w as f32, h as f32);
    let thin = f32::from(m.underline_px.1).max(1.0);
    // The cell's first and last bands. Because `rail` seats on the grid, the
    // first is exactly `[0, thin)` and the last `[length - thin, length)`: the
    // edge line stays **inside** the edge, half of it is not clipped.
    let first = rail(thin / 2.0, thin);
    let last_col = rail(w - thin / 2.0, thin);
    let last_row = rail(h - thin / 2.0, thin);
    match ch {
        // ⎸ ⎹ — full-height vertical line on the left / right edge.
        '\u{23B8}' => max_rect(target, m, first.0, first.1, 0.0, h),
        '\u{23B9}' => max_rect(target, m, last_col.0, last_col.1, 0.0, h),
        // ⎺ ⎻ ⎼ ⎽ — a scan line running across the whole cell.
        '\u{23BA}'..='\u{23BD}' => {
            // audit: the range is four characters, the index is inside the table.
            let line = SCAN_LINES[u32::from(ch) as usize - 0x23BA];
            let band = rail(scan_centre(line, h), thin);
            max_rect(target, m, 0.0, w, band.0, band.1);
        }
        // ⎾ ⎿ — full-height vertical on the left edge, full-width horizontal on
        // the top / bottom edge. The arms join with `max_rect`, so the corner
        // pixel is not painted twice: the reasoning is the same as
        // [`stroke_rect`]'s.
        '\u{23BE}' | '\u{23BF}' => {
            max_rect(target, m, first.0, first.1, 0.0, h);
            let band = if ch == '\u{23BE}' { first } else { last_row };
            max_rect(target, m, 0.0, w, band.0, band.1);
        }
        _ => {}
    }
}
