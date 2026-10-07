use bt_core::{
    Block, CaretShape, Cell, Cursor, SearchRun, SelectionRun, Theme, TrackMark, UnderlineStyle,
};

use super::*;
use crate::Renderer;
use crate::glyph_fx::{Effect, Fx, Kind};
use crate::scrollbar::Look;
use bt_atlas::fixture::{CHAIN_FAMILIES, ONE_CELL_WIDE_CHAR, PAIR_CHAR, PROPORTIONAL_FAMILY};
use bt_atlas::{Face, SizeClass};
use bt_core::CaretStyle;
use bt_core::{ButtonState, DockButton, Erase, Keypress};

/// The embedded theme's background and accent: in production the clear and cursor colours
/// come from these two roles (`link.rs`), and the tests read from the same source.
pub(crate) const BACKGROUND: LinearRgba = Theme::BATERI.background_linear();
pub(crate) const ACCENT: LinearRgba = Theme::BATERI.accent_linear();

/// The witness for sRGB linearisation **on the cell path**: a midtone.
///
/// Deliberately NOT from the theme. The guard's sensitivity cannot depend on a matter of
/// taste: `0.0` and `1.0` are the fixed points of the sRGB transfer function, so the day the
/// background was pulled to pure black (and that happened) this claim would have held with or
/// without linearisation, and the only guard would have gone silently blind. The value is
/// the old background itself.
pub(crate) const MIDTONE_SRGB: u32 = 0x1a1c21;
pub(crate) const MIDTONE: LinearRgba = {
    let (r, g, b) = (
        (MIDTONE_SRGB >> 16) as u8,
        (MIDTONE_SRGB >> 8) as u8,
        MIDTONE_SRGB as u8,
    );
    LinearRgba::from_srgb(r, g, b)
};

/// A grid metric whose left gutter is **zero**.
///
/// The offscreen tests' sampling point in `cell_rows` is `col * cw + x`, i.e. it assumes a
/// zero origin: a non-zero gutter would shift those points and the tests would read the
/// clear colour instead of the cell. Zero here is not a convenience but the **right
/// question**: what these tests are about is not the gutter's geometry but which colour the
/// GPU paints into which cell. That the gutter is added to the origin is held by the `pos`
/// tests on the `frame.rs` side.
pub(crate) fn grid(width: u16, height: u16) -> CellMetrics {
    CellMetrics::new(width, height, width, 0, 1, 1.0).expect("non-zero cell")
}

/// A grid with a **non-zero** gutter: the glow's margin derives from the left gutter
/// ([`Frame::glow_px`]), so on a gutterless grid the glow is never born and nothing that
/// tests it could see it.
pub(crate) fn grid_with_gutter(width: u16, height: u16, gutter: u16) -> CellMetrics {
    CellMetrics::new(width, height, width, gutter, 1, 1.0).expect("non-zero cell")
}

/// A cell with only a background; `ch: None` produces no glyph.
pub(crate) fn bg_cell(col: u16, row: u16, bg: LinearRgba) -> Cell {
    Cell {
        col,
        row,
        fg: BACKGROUND,
        bg: Some(bg),
        ..Default::default()
    }
}

#[test]
fn two_scales_give_two_metrics() {
    // The scale is part of the cache key and the metric is that key's
    // visible end: if the cell does not grow at @2x, the atlas is swallowing the scale and
    // glyphs go blurry without any error.
    let r = renderer();
    let one = r.cell_metrics(1.0);
    let two = r.cell_metrics(2.0);
    assert!(
        two.cell_px().0 > one.cell_px().0 && two.cell_px().1 > one.cell_px().1,
        "the @2x cell must be larger than the @1x one: {one:?} → {two:?}"
    );
    // Going back must work too: `ensure` is not a one-way gate. Otherwise, when the external
    // display is unplugged the metric would stay stuck at @2x and the window would show
    // half as many cells.
    assert_eq!(
        r.cell_metrics(1.0),
        one,
        "the same scale gives the same metric"
    );
}

#[test]
fn set_font_changes_the_metrics_on_the_next_ask() {
    let r = renderer();
    let base = r.cell_metrics(1.0);
    // The opening value is the settings model's default: the timed run's cell is the same as
    // that of a user without a file.
    assert!(
        !r.set_font(&FontOptions::default()),
        "the default must already be the requested one"
    );
    let large = FontOptions {
        size: 26.0,
        ..FontOptions::default()
    };
    assert!(r.set_font(&large), "the point size changed");
    assert!(!r.set_font(&large), "the same request is not a change");
    let bigger = r.cell_metrics(1.0);
    assert!(
        bigger.cell_px().0 > base.cell_px().0 && bigger.cell_px().1 > base.cell_px().1,
        "the 26pt cell must be larger than the 13pt one: {base:?} → {bigger:?}"
    );
    assert!(r.set_font(&FontOptions::default()));
    assert_eq!(r.cell_metrics(1.0), base, "return to the default");
}

#[test]
fn missing_family_becomes_a_notice_after_the_atlas_opens() {
    let r = renderer();
    assert_eq!(
        r.font_notice(),
        None,
        "nothing to say while there is no atlas"
    );
    r.cell_metrics(1.0);
    assert_eq!(r.font_notice(), None, "the chain is silent");
    assert!(r.set_font(&FontOptions {
        family: Some("No Such Family 12345".to_owned()),
        ..FontOptions::default()
    }));
    r.cell_metrics(1.0);
    let Some(FontNotice::FamilyNotFound { requested, using }) = r.font_notice() else {
        panic!("a not-found notice was expected: {:?}", r.font_notice());
    };
    assert_eq!(requested, "No Such Family 12345");
    assert!(
        CHAIN_FAMILIES.contains(&using.as_str()),
        "the chain's family: {using}"
    );
    assert!(r.set_font(&FontOptions {
        family: Some(PROPORTIONAL_FAMILY.to_owned()),
        ..FontOptions::default()
    }));
    r.cell_metrics(2.0);
    let proportional = Some(FontNotice::NotMonospaced {
        family: PROPORTIONAL_FAMILY.to_owned(),
    });
    assert_eq!(r.font_notice(), proportional);
    // A display change rebuilds the atlas but the notice stays the same: the font slot must
    // not move while the window is carried from one display to another.
    r.cell_metrics(1.0);
    assert_eq!(r.font_notice(), proportional, "the scale moved the notice");
}

#[test]
fn zero_component_metrics_cannot_be_built() {
    // This is the only guarantee the type carries. If it falls, `bt-shell`'s division yields
    // `inf`, `inf as u16` becomes 65535 and a 65535×65535 `TIOCSWINSZ` gets through without
    // hitting `Session::resize`'s zero gate.
    assert!(CellMetrics::new(0, 18, 0, 8, 1, 1.0).is_none());
    assert!(CellMetrics::new(9, 0, 9, 8, 1, 1.0).is_none());
    // The context width is a **divisor** too (`crate::frame::context_cols`), so it goes
    // through the same gate: if zero got through, the grid's would be caught while the dock's
    // context line would silently divide by zero.
    assert!(CellMetrics::new(9, 18, 0, 8, 1, 1.0).is_none());
    let metrics = CellMetrics::new(9, 18, 7, 8, 1, 1.0).expect("metrics");
    assert_eq!(metrics.cell_px(), (9, 18));
    assert_eq!(metrics.context_cell_px(), 7);
    assert_eq!(metrics.gutter_px(), 8);
    // The gutter is **carried, not filtered**: it is not a divisor but an outcome, and a zero
    // gutter means "the grid starts at the edge". Rejecting zero here too would force every
    // test that is not about the gutter to write a made-up value.
    assert_eq!(
        CellMetrics::new(9, 18, 9, 0, 1, 1.0)
            .expect("a zero gutter is legitimate")
            .gutter_px(),
        0
    );
}

#[test]
fn cell_metrics_are_never_zero() {
    // `bt-shell` uses these two numbers as **divisors**. `bt-atlas` gives the guarantee
    // (`rules::round_up` clamps to 1) and `CellMetrics`'s private field makes it structural on
    // this side of the boundary; this test says the clamp at the source is still in place.
    let r = renderer();
    for scale in [1.0, 2.0, 3.0] {
        let (w, h) = r.cell_metrics(scale).cell_px();
        assert!(w >= 1 && h >= 1, "scale {scale}: {w}×{h}");
    }
}

#[test]
fn cell_bg_pipeline_builds() {
    // If there is no device it is an explicit error, not `ignored`: this machine has Metal,
    // its absence is a defect. Building the pipeline proves the shader compiled and the
    // function names are found in the metallib.
    let r = renderer();
    assert_eq!(r.frames(), 0);
}

/// Byte order is B, G, R, A (the format's `_sRGB` suffix does not change the order).
pub(crate) fn pixel_at(pixels: &[u8], edge: usize, x: usize, y: usize) -> (u8, u8, u8) {
    let i = (y * edge + x) * 4;
    (pixels[i + 2], pixels[i + 1], pixels[i])
}

/// The saturated white used as the foreground.
///
/// Saturated: a pixel with full coverage yields the bytes `(0xff, 0xff, 0xff)` exactly, so
/// the claim "the rule was drawn in the foreground colour" can be asked with equality. Not
/// from the palette, because what the tests ask is not the colour but **where the colour
/// came from**.
pub(crate) const WHITE: LinearRgba = LinearRgba::from_srgb(0xff, 0xff, 0xff);

/// The renderer every guard draws with.
pub(crate) type TestRenderer = Renderer;

pub(crate) fn renderer() -> TestRenderer {
    Renderer::new()
}

/// Draws the frame into an offscreen texture and reads the pixels back —
/// the body every offscreen guard shares (setup, encode, submit, wait and
/// the error check live in `Renderer::render_offscreen`; copied, the
/// error check would be forgotten in one of them and that guard would read
/// an empty texture).
///
/// `edge` and `clear` stay parameters: both carry weight — edges 16 and 64
/// differ, and `cell_bg_paints_pixels_on_the_gpu`'s clear colour is
/// deliberately **different** from the others' (the cell path and the
/// clear path are proven with two distinct colours).
pub(crate) fn render_offscreen(
    r: &TestRenderer,
    edge: usize,
    clear: LinearRgba,
    frame: &Frame,
) -> Vec<u8> {
    r.render_offscreen(edge as u32, clear, frame)
}

/// The pixels of the cell in column `col`, **row by row** (top to bottom).
///
/// The row structure is kept because what the rule tests ask is precisely whether a row is
/// uniform along x; a flattened list cannot ask that question. Whoever wants a flat list
/// calls `.concat()`.
pub(crate) fn cell_rows(
    pixels: &[u8],
    edge: usize,
    cell_px: (u16, u16),
    col: usize,
) -> Vec<Vec<(u8, u8, u8)>> {
    let (cw, ch) = (usize::from(cell_px.0), usize::from(cell_px.1));
    (0..ch)
        .map(|y| {
            (0..cw)
                .map(|x| pixel_at(pixels, edge, col * cw + x, y))
                .collect()
        })
        .collect()
}

/// The sum of a pixel's three channels — the form of the "was it painted" question that
/// needs no colour table.
///
/// A shared helper, because all the caret tests ask the same question and each used to carry
/// its own copy (found in code review): a fix could be forgotten in one of the copies.
pub(crate) fn brightness(pixels: &[u8], edge: usize, x: usize, y: usize) -> u32 {
    let (r8, g8, b8) = pixel_at(pixels, edge, x, y);
    u32::from(r8) + u32::from(g8) + u32::from(b8)
}

/// The cell's **middle band**: pulled in from top and bottom by the radius, full width.
///
/// Because the caret's corner is rounded, the corner pixels are no longer the
/// block's colour; an equality claim that passes through there tests the **roundness**, not
/// the fill. Pulling in does not weaken the claim, it **separates** it: on the band the
/// equality is still bit for bit, and the corner has its own guard
/// ([`the_caret_corner_is_rounded`]).
///
/// **Only rows are pulled in, not columns:** the roundness is at the corners, and on every
/// row between `radius` and `ch - radius` the shape covers the cell's **full width**. Pulling
/// in the columns too would swallow the band entirely in a narrow cell (the radius derives
/// from the cell height and in a narrow cell can approach half the width).
fn cell_body(
    pixels: &[u8],
    edge: usize,
    cell_px: (u16, u16),
    col: usize,
    inset: usize,
) -> Vec<(u8, u8, u8)> {
    let (cw, ch) = (usize::from(cell_px.0), usize::from(cell_px.1));
    assert!(inset * 2 < ch, "the inset swallows the cell");
    (inset..ch - inset)
        .flat_map(|y| (0..cw).map(move |x| (x, y)))
        .map(|(x, y)| pixel_at(pixels, edge, col * cw + x, y))
        .collect()
}

/// How many pixels the caret's corner radius is at this cell size — the tests' inset
/// margin. It reads **from production's own function**, not from a copy: the formula used
/// to be written in three places and when one changed the guard would silently loosen
/// (found in code review).
fn caret_radius_px(cell_px: (u16, u16), ratio: f32) -> usize {
    crate::frame::caret_radius_px((f32::from(cell_px.0), f32::from(cell_px.1)), ratio).ceil()
        as usize
}

/// The offscreen tests' shared setup: cell size + fit check.
///
/// The scale is stated explicitly (`cell_metrics(1.0)`): the atlas's key comes from the
/// window, the test has no window and if it is not stated the frame fails with
/// `GpuError::NoAtlas`. The fit check is not dead: on a machine with a large default point
/// size the cell exceeds the texture and `cell_rows` would read outside the texture.
fn fitting_cell_px(r: &TestRenderer, edge: usize, cols: usize) -> (u16, u16) {
    let (cw, ch) = r.cell_metrics(1.0).cell_px();
    assert!(
        usize::from(cw) * cols <= edge && usize::from(ch) <= edge,
        "{cols}×({cw}×{ch}) does not fit the offscreen texture"
    );
    (cw, ch)
}

/// A cell with ink; the foreground is [`WHITE`] on every call. The third shape beside
/// `bg_cell` and `rule_cell` — three tests that draw glyphs used to build the same quad by
/// hand and when one changed the others would silently diverge.
fn glyph_cell(col: u16, ch: char, bg: Option<LinearRgba>) -> Cell {
    Cell {
        col,
        row: 0,
        ch: Some(ch),
        fg: WHITE,
        bg,
        ..Default::default()
    }
}

/// A cell carrying only a rule: `ch: None`, `bg: None` — the same as the smoke recipe's
/// seven rule cells. The foreground is [`WHITE`] on every call.
fn rule_cell(col: u16, underline: UnderlineStyle) -> Cell {
    Cell {
        col,
        row: 0,
        fg: WHITE,
        underline,
        ..Default::default()
    }
}

#[test]
fn cell_bg_paints_pixels_on_the_gpu() {
    // This test replaces the "the draw call went through the pipeline" proof that was
    // deleted, and says more: buffer indices, the NDC transform, the y flip, the instance
    // stride and the GPU reading the `Instance` layout correctly. The asserts on the two
    // sides bind the layout at compile time but never RUN it; this place runs it. It needs no
    // window, so it also runs in a headless environment.
    let r = renderer();
    const EDGE: usize = 16;

    // 8×8 cell, viewport 16×16 → four quadrants. Red top left, green bottom right, the
    // palette's background bottom left, top right empty. Two instances are required: a single
    // instance reads `inst[0]` independently of the stride, so a stride error (drifting from
    // 32) is INVISIBLE with one instance. The second is found only with the right stride. If
    // the y flip is broken, red and green swap places.
    //
    // The third is a **midtone** and the sRGB transition's only guard: pure 0.0/1.0 are the
    // fixed points of the sRGB transfer function, so red and green give the same byte with
    // or without linearisation.
    let mut frame = Frame::default();
    frame.clear(grid(8, 8), CaretStyle::default());
    frame.push(bg_cell(0, 0, LinearRgba::from_srgb(0xff, 0x00, 0x00)));
    frame.push(bg_cell(1, 1, LinearRgba::from_srgb(0x00, 0xff, 0x00)));
    frame.push(bg_cell(0, 1, MIDTONE));

    // The clear colour is a **midtone** too, and deliberately from the theme: in production
    // the whole of the window's visible background comes through this path (`frame()`
    // filters out cells with the default background, `link.rs` gives the clear the theme's
    // background).
    // If pure blue were left, the semantics of `MTLClearColor` on an sRGB target would remain
    // untested: someone who encodes it once more as "let me convert it to the target's space"
    // would darken the window background, leave the cells right and every test would pass
    // green.
    //
    // The clear is the **accent** (`ACCENT`), not the background: the background was used in
    // a cell and proving the two paths separately needs two distinct colours. If someone
    // "fixes" this to `BACKGROUND`, the test can no longer tell the cell path from the clear
    // path.
    let pixels = render_offscreen(&r, EDGE, ACCENT, &frame);
    let pixel = |x: usize, y: usize| pixel_at(&pixels, EDGE, x, y);
    assert_eq!(
        pixel(2, 2),
        (255, 0, 0),
        "the first cell is red at top left"
    );
    assert_eq!(
        pixel(12, 12),
        (0, 255, 0),
        "the second cell is green at bottom right"
    );
    // The expected byte is the byte the theme is **written** in: `Theme`'s fields are
    // `0xRRGGBB`. The values themselves are bound to a hand-written list by `bt-core`'s
    // palette guard; the claim here is not the value but the round trip.
    let srgb = |hex: u32| ((hex >> 16) as u8, (hex >> 8) as u8, hex as u8);
    let close_to = |seen: (u8, u8, u8), expected: (u8, u8, u8), what: &str| {
        // ±1: 8-bit sRGB encoding carries rounding and the Metal spec gives an accuracy
        // bound, not bit-exactness. If bit equality were demanded, the gate would be
        // hostage to the driver version.
        assert!(
            seen.0.abs_diff(expected.0) <= 1
                && seen.1.abs_diff(expected.1) <= 1
                && seen.2.abs_diff(expected.2) <= 1,
            "{what}: {seen:02x?} ≠ {expected:02x?}"
        );
    };
    // sRGB round trip: `linear_rgba`'s linearisation and the encoding the hardware does when
    // writing must invert each other, i.e. the byte that goes to the screen must be the byte
    // that was written. If linearisation is dropped, `MIDTONE` (`0x1a1c21`) brightens to the
    // grey `0x5a5d65` — this is the only place where the transition could stay silent; pure
    // red, green **and now the background too** cannot see it, all three being fixed points
    // of the sRGB transfer function.
    close_to(pixel(2, 12), srgb(MIDTONE_SRGB), "the cell midtone");
    close_to(
        pixel(12, 2),
        srgb(Theme::BATERI.accent),
        "the empty quadrant is the clear colour",
    );
}

#[test]
fn a_selection_run_paints_between_the_ground_and_the_glyph() {
    // The GPU witness of the selection: the run is drawn **after the background,
    // before the glyph**. A run of three cells: in column 0 a cell with a red background (the
    // run must cover it), in column 1 a white `M` (it must stay in its own colour above the
    // run), column 2 empty (the bridge). Column 3 is outside the run and must stay the clear
    // colour.
    //
    // The run's colour is not from the theme but a **midtone** ([`MIDTONE`]): sRGB's fixed
    // points could not see linearisation being forgotten. The clear is `ACCENT` — the run and
    // the clear are two distinct colours, otherwise "painted" and "not painted" cannot be
    // told apart.
    let r = renderer();
    const EDGE: usize = 64;
    let (cw, ch) = fitting_cell_px(&r, EDGE, 4);
    let mut frame = Frame::default();
    frame.clear(grid(cw, ch), CaretStyle::default());
    frame.push(bg_cell(0, 0, LinearRgba::from_srgb(0xff, 0x00, 0x00)));
    frame.push(glyph_cell(1, 'M', None));
    frame.push_selection(
        &[SelectionRun {
            row: 0,
            first: 0,
            last: 2,
        }],
        MIDTONE,
    );
    assert_eq!(
        frame.bg_count(),
        1,
        "the run must not enter the `cells=` counter"
    );

    let pixels = render_offscreen(&r, EDGE, ACCENT, &frame);
    let srgb = |hex: u32| ((hex >> 16) as u8, (hex >> 8) as u8, hex as u8);
    let near = |seen: (u8, u8, u8), expected: (u8, u8, u8)| {
        seen.0.abs_diff(expected.0) <= 1
            && seen.1.abs_diff(expected.1) <= 1
            && seen.2.abs_diff(expected.2) <= 1
    };
    let midtone = srgb(MIDTONE_SRGB);
    let accent = srgb(Theme::BATERI.accent);
    let cell = |col: usize| cell_rows(&pixels, EDGE, (cw, ch), col).concat();
    // The run's four corners are round: those closer than `inset` pixels to a
    // corner are outside the question, the corner guards ask about them.
    let inset = caret_radius_px((cw, ch), crate::frame::SELECTION_RADIUS);
    let body = |col: usize, left: bool| -> Band {
        let rows = cell_rows(&pixels, EDGE, (cw, ch), col);
        let (w, h) = (usize::from(cw), usize::from(ch));
        let mut out = Vec::new();
        for (y, row) in rows.into_iter().enumerate() {
            for (x, p) in row.into_iter().enumerate() {
                let edge_x = if left { x < inset } else { x >= w - inset };
                if edge_x && (y < inset || y >= h - inset) {
                    continue;
                }
                out.push(p);
            }
        }
        out
    };
    // The cell with a background is under the run: the red is not visible except at the
    // corners.
    assert!(
        body(0, true).iter().all(|&p| near(p, midtone)),
        "the run stayed under the background: {:02x?}",
        body(0, true)
    );
    // The glyph is above the run, in its own colour; the letter's gaps are in the run's
    // colour — the clear colour is nowhere inside the run.
    let glyph = cell(1);
    // The exact byte is not demanded: at 1x even vertical stems can fall on half a pixel and
    // saturated white needs a full pixel of coverage. What is asked is that the letter is
    // **lighter** than the run — if it were underneath, it would not be visible at all.
    let brightest = glyph
        .iter()
        .map(|p| u32::from(p.0) + u32::from(p.1) + u32::from(p.2))
        .max()
        .unwrap_or(0);
    assert!(
        brightest > 3 * 0xc0,
        "the glyph stayed under the run: {brightest}"
    );
    assert!(
        glyph.iter().any(|&p| near(p, midtone)),
        "the letter's surroundings were not painted"
    );
    assert!(!glyph.iter().any(|&p| near(p, accent)), "a hole in the run");
    // The bridge: the column without ink is in the run's colour too.
    assert!(
        body(2, false).iter().all(|&p| near(p, midtone)),
        "the bridge was not painted"
    );
    // Outside the run is clear.
    assert!(
        cell(3).iter().all(|&p| near(p, accent)),
        "the run overflowed"
    );
}

/// The selection guards' shared setup: a **large** artificial cell (40×80 → radius 17.6) so
/// the corner radius is a few pixels, no atlas needed — there is no glyph in the frame. What
/// it returns is a reader that says whether a pixel is closer to the midtone (selection) or
/// to the clear.
fn render_selection(runs: &[SelectionRun]) -> impl Fn(usize, usize) -> &'static str {
    const EDGE: usize = 256;
    let r = renderer();
    let mut frame = Frame::default();
    frame.clear(grid(40, 80), CaretStyle::default());
    assert!(
        (frame.selection_radius() - 17.6).abs() < 1e-4,
        "radius assumption: {}",
        frame.selection_radius()
    );
    frame.push_selection(runs, MIDTONE);
    let pixels = render_offscreen(&r, EDGE, ACCENT, &frame);
    let srgb = |hex: u32| ((hex >> 16) as u8, (hex >> 8) as u8, hex as u8);
    let (midtone, accent) = (srgb(MIDTONE_SRGB), srgb(Theme::BATERI.accent));
    let near = |seen: (u8, u8, u8), expected: (u8, u8, u8)| {
        seen.0.abs_diff(expected.0) <= 1
            && seen.1.abs_diff(expected.1) <= 1
            && seen.2.abs_diff(expected.2) <= 1
    };
    move |x, y| {
        let p = pixel_at(&pixels, EDGE, x, y);
        if near(p, midtone) {
            "selection"
        } else if near(p, accent) {
            "clear"
        } else {
            "blend"
        }
    }
}

#[test]
fn a_lone_selection_run_has_round_corners_and_a_solid_body() {
    // A run of two cells: x 0..80, y 0..80. The corner pixel's centre is far outside the arc
    // of radius 17.6 → clear; the middle of the edge and the inside of the arc are the
    // selection colour — the edges sit on the pixel grid, i.e. there is no half alpha on a
    // straight edge.
    let at = render_selection(&[SelectionRun {
        row: 0,
        first: 0,
        last: 1,
    }]);
    for (x, y) in [(0, 0), (79, 0), (79, 79), (0, 79)] {
        assert_eq!(at(x, y), "clear", "corner ({x},{y}) was not rounded");
    }
    for (x, y) in [(40, 0), (0, 40), (79, 40), (40, 79), (40, 40), (8, 8)] {
        assert_eq!(at(x, y), "selection", "body ({x},{y}) was not painted");
    }
    assert_eq!(at(80, 40), "clear", "the run overflowed");
}

#[test]
fn a_selection_step_fills_its_concave_corner() {
    // 2..=3 on top (x 80..160), 0..=3 below (x 0..160): the upper run's bottom-left corner is
    // concave. The fill is in [62.4,80]×[62.4,80], painting the outside of the circle whose
    // centre is (62.4,62.4): the pixel at the foot of the step is the selection colour, the
    // one inside the circle (72,72) is clear.
    let at = render_selection(&[
        SelectionRun {
            row: 0,
            first: 2,
            last: 3,
        },
        SelectionRun {
            row: 1,
            first: 0,
            last: 3,
        },
    ]);
    assert_eq!(at(79, 79), "selection", "the concave corner was not filled");
    assert_eq!(
        at(72, 72),
        "clear",
        "the fill painted the inside of the circle"
    );
    // The exposed corners are round, the covered corners are square.
    assert_eq!(at(80, 0), "clear", "the upper run's top left");
    assert_eq!(at(0, 80), "clear", "the lower run's top left");
    assert_eq!(at(159, 79), "selection", "aligned right edge seam");
    assert_eq!(at(159, 80), "selection", "aligned right edge seam");
    assert_eq!(at(100, 79), "selection", "the two rows' seam");
    assert_eq!(at(100, 80), "selection", "the two rows' seam");
    assert_eq!(at(159, 159), "clear", "the lower run's bottom right");
}

/// The two colours of the search guards: midtones (not fixed points), distinct from the
/// selection's [`MIDTONE`] and from the clear's `ACCENT` — all four must be distinguishable
/// in a pixel read.
const MATCH_SRGB: u32 = 0x3c6e5a;
const CURRENT_SRGB: u32 = 0x8a5a2c;

fn srgb_linear(hex: u32) -> LinearRgba {
    LinearRgba::from_srgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
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

/// The search guards' shared setup, following [`render_selection`]: a 40×80 artificial cell
/// (radius 17.6), no atlas. If `fill` is non-zero the band is that many rows and the grid is
/// one row lower ([`Frame::set_origin_rows`]), i.e. the band's row 0 is the window's 0..80.
/// The reader says which surface a pixel belongs to.
fn render_search(
    grid_runs: &[SearchRun],
    fill_runs: &[SearchRun],
    selection: &[SelectionRun],
) -> impl Fn(usize, usize) -> &'static str + use<> {
    const EDGE: usize = 256;
    let r = renderer();
    let mut frame = Frame::default();
    frame.clear(grid(40, 80), CaretStyle::default());
    if !fill_runs.is_empty() {
        frame.set_fill_rows(1);
    }
    frame.push_search(
        grid_runs,
        srgb_linear(MATCH_SRGB),
        srgb_linear(CURRENT_SRGB),
    );
    frame.push_fill_search(fill_runs);
    frame.push_selection(selection, MIDTONE);
    if !fill_runs.is_empty() {
        frame.set_origin_rows(1.0);
    }
    let pixels = render_offscreen(&r, EDGE, ACCENT, &frame);
    let srgb = |hex: u32| ((hex >> 16) as u8, (hex >> 8) as u8, hex as u8);
    let near = |seen: (u8, u8, u8), expected: (u8, u8, u8)| {
        seen.0.abs_diff(expected.0) <= 1
            && seen.1.abs_diff(expected.1) <= 1
            && seen.2.abs_diff(expected.2) <= 1
    };
    move |x, y| {
        let p = pixel_at(&pixels, EDGE, x, y);
        [
            (MATCH_SRGB, "match"),
            (CURRENT_SRGB, "current"),
            (MIDTONE_SRGB, "selection"),
            (Theme::BATERI.accent, "clear"),
        ]
        .into_iter()
        .find(|&(hex, _)| near(p, srgb(hex)))
        .map_or("blend", |(_, name)| name)
    }
}

#[test]
fn search_roles_paint_their_colors_under_the_selection() {
    // The search highlight's order on the GPU: background → `search_match` → `search_current` →
    // selection. The match is 0..=2 of row 0, the current match is 0..=1 of row 2; the
    // selection is 2..=3 of row 0 and covers the match's last cell — the user's selection is
    // above the search.
    let at = render_search(
        &[
            search_run(0, 0, 2, false, false),
            search_run(2, 0, 1, true, false),
        ],
        &[],
        &[SelectionRun {
            row: 0,
            first: 2,
            last: 3,
        }],
    );
    assert_eq!(at(40, 40), "match", "the match is not in its own colour");
    assert_eq!(
        at(40, 200),
        "current",
        "the current match is not in its own colour"
    );
    assert_eq!(
        at(100, 40),
        "selection",
        "the selection stayed under the search"
    );
    assert_eq!(at(140, 40), "selection");
    // The round corner and the outside of the run.
    assert_eq!(at(0, 0), "clear", "the match's corner was not rounded");
    assert_eq!(at(0, 160), "clear", "the current match's corner");
    assert_eq!(at(40, 120), "clear", "the gap between rows was painted");
}

#[test]
fn adjacent_matches_are_two_shapes_and_a_wrapped_match_is_one() {
    // Two aligned runs on consecutive rows: if they are two separate matches, four round
    // corners at the seam (rows 79/80 of the left edge are clear); if it is one match wrapping,
    // a straight edge (painted).
    let runs = |continues| {
        [
            search_run(0, 0, 1, false, false),
            search_run(1, 0, 1, false, continues),
        ]
    };
    let at = render_search(&runs(false), &[], &[]);
    assert_eq!(
        at(0, 79),
        "clear",
        "the upper match's bottom corner is square"
    );
    assert_eq!(at(0, 80), "clear", "the lower match's top corner is square");
    assert_eq!(at(40, 40), "match");
    assert_eq!(at(40, 120), "match");
    let at = render_search(&runs(true), &[], &[]);
    assert_eq!(at(0, 79), "match", "the wrapped match was split in two");
    assert_eq!(at(0, 80), "match", "the wrapped match was split in two");
}

#[test]
fn the_fill_band_highlights_its_matches() {
    // The band's rows are real history and their matches are highlighted too —
    // in the band's own viewport. The band is at 0..80 (the grid is one row lower); nothing
    // must land on the grid's row with the same number (80..160).
    let at = render_search(&[], &[search_run(0, 0, 1, true, false)], &[]);
    assert_eq!(at(40, 40), "current", "no highlight in the band");
    assert_eq!(
        at(40, 120),
        "clear",
        "the band's highlight landed on the grid"
    );
}

#[test]
fn a_frame_without_search_draws_todays_picture() {
    // While search is off (both lists empty) the encoder never sees the highlight and the
    // frame is **bit for bit** the same as today's; the same pattern as the fill's rollback
    // strip. When on it must differ, otherwise the equality says nothing.
    let r = renderer();
    const EDGE: usize = 64;
    let draw = |search: Option<&[SearchRun]>| {
        let mut frame = Frame::default();
        frame.clear(grid(16, 32), CaretStyle::default());
        frame.push(bg_cell(0, 0, MIDTONE));
        frame.set_fill_rows(1);
        frame.push_fill(bg_cell(1, 0, MIDTONE));
        if let Some(runs) = search {
            frame.push_search(runs, srgb_linear(MATCH_SRGB), srgb_linear(CURRENT_SRGB));
            frame.push_fill_search(&[]);
        }
        frame.set_origin_rows(1.0);
        render_offscreen(&r, EDGE, ACCENT, &frame)
    };
    let today = draw(None);
    assert!(
        today == draw(Some(&[])),
        "an empty search changed the frame"
    );
    assert!(
        today != draw(Some(&[search_run(0, 1, 2, false, false)])),
        "the search was never drawn"
    );
}

/// The pixels of a rectangular region, as [`pixel_at`]'s triple.
///
/// The name is for readability, not for the type's complexity: both guards ask "which colour
/// does this region carry" and the `Vec<(u8, u8, u8)>` pair in the signature did not say
/// that question.
type Band = Vec<(u8, u8, u8)>;

/// In an offset frame, whether row 0's cell is painted where it was moved to and whether its
/// **old place** stayed the clear colour.
///
/// The shared body of two guards: the setup (8 px cell, 16 px texture, one row of offset) and
/// the reading of the two regions. If copied, the "the upper region stayed empty" half could
/// be forgotten in one — and without that half the test would also pass code that **ignores**
/// the offset: it would not look, since there is no cell in the lower region anyway.
///
/// The return is `(upper region, lower region)`, row by row: the claim is which region
/// carries which colour.
fn origin_shifted_halves(r: &TestRenderer, frame: &mut Frame, clear: LinearRgba) -> (Band, Band) {
    const EDGE: usize = 16;
    const CELL: usize = 8;
    // One row of offset: row 0's cell must land in y ∈ [8, 16) instead of [0, 8).
    // `set_origin_rows` converts to pixels with the cell height set by `clear`, so the metric
    // must have been set **before** this call.
    frame.set_origin_rows(1.0);
    assert_eq!(
        frame.origin_px(),
        CELL as f32,
        "the offset was not converted to pixels"
    );
    let pixels = render_offscreen(r, EDGE, clear, frame);
    let band = |y0: usize| {
        (y0..y0 + CELL)
            .flat_map(|y| (0..CELL).map(move |x| (x, y)))
            .map(|(x, y)| pixel_at(&pixels, EDGE, x, y))
            .collect::<Vec<_>>()
    };
    (band(0), band(CELL))
}

#[test]
fn content_sticks_to_the_bottom_for_cell_bg() {
    // **This set's CPU→GPU seam.** The offset is applied on the GPU with `setViewport`, so a
    // test that measured two CPU lists against each other would test something that is right
    // by construction. What is asked is that
    // the painted **pixel** moved.
    //
    // It is also the `setViewport` canary: the viewport overflows below the texture (origin 8
    // + height 16 = 24 > 16) and Metal must clip the overflowing fragment. If it did not clip,
    // either a validation error would fire or the lower region would wrap; both show up here.
    let r = renderer();
    let red = LinearRgba::from_srgb(0xff, 0x00, 0x00);
    let mut frame = Frame::default();
    frame.clear(grid(8, 8), CaretStyle::default());
    frame.push(bg_cell(0, 0, red));

    // The clear is the **accent**: it must be distinct from the cell's colour, otherwise "the
    // cell moved" and "everywhere is clear" cannot be told apart.
    let (top, bottom) = origin_shifted_halves(&r, &mut frame, ACCENT);
    assert!(
        bottom.iter().all(|&p| p == (255, 0, 0)),
        "row 0's cell was not painted one row lower: {bottom:02x?}"
    );
    assert!(
        top.iter().all(|&p| p != (255, 0, 0)),
        "the cell also stayed in its old row: {top:02x?}"
    );
}

#[test]
fn content_sticks_to_the_bottom_for_glyphs() {
    // The twin of its sibling for the `cell` pipeline and **this set's real risk**: the two
    // pipelines are separate `setRenderPipelineState` calls and separate shader pairs, so one
    // being offset while the other is not is a representable state — and moreover a state that
    // leaves `make check` green. A single-line `setViewport` shifts both; this test binds that
    // to the code, not to a comment sentence.
    //
    // The glyph size is **not** tied to the atlas's cell: what is asked is the quad's
    // position, and with an 8 px cell the atlas slot stretches, it does not break.
    let r = renderer();
    // "Ask the metric first": the scale half of the atlas's key comes from the window, this
    // test has no window and if it is not stated the frame fails with `GpuError::NoAtlas`. The
    // returned metric is **not used** — the quad is 8 px, so the atlas slot stretches; what is
    // asked is position, not resolution.
    r.cell_metrics(1.0);
    let mut frame = Frame::default();
    frame.clear(grid(8, 8), CaretStyle::default());
    // An `M` without a background: so that the claim belongs only to the `cell` pipeline. The
    // `cell_bg` list stays empty, so the only witness in the upper region is the clear.
    frame.push(glyph_cell(0, 'M', None));
    assert_eq!(
        frame.bg_count(),
        0,
        "the background must not get mixed into the claim"
    );

    let (top, bottom) = origin_shifted_halves(&r, &mut frame, BACKGROUND);
    let clear = {
        let hex = Theme::BATERI.background;
        ((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
    };
    // The glyph's exact byte is not demanded (precedent `glyphs_paint_pixels_on_the_gpu`):
    // what is asked is "in which region is there a pixel different from the clear".
    assert!(
        bottom.iter().any(|&p| p.0.abs_diff(clear.0) > 1
            || p.1.abs_diff(clear.1) > 1
            || p.2.abs_diff(clear.2) > 1),
        "the glyph was not drawn one row lower: {bottom:02x?}"
    );
    assert!(
        top.iter().all(|&p| p.0.abs_diff(clear.0) <= 1
            && p.1.abs_diff(clear.1) <= 1
            && p.2.abs_diff(clear.2) <= 1),
        "the glyph also stayed in its old row: {top:02x?}"
    );
}

/// **A measurement, pinned into a test.**
///
/// The fill band lands **above** the grid and its candidate is a third `setViewport`:
/// `originY = origin_px − fill_px`. That number goes negative in the middle of the slide,
/// whereas the clamp in [`MetalRenderer::encode_dock`] said *"a negative `originY` would fall
/// into Metal's validation — an exception that kills the process"* and that sentence had
/// **not been measured**. This test measures it.
///
/// The only thing asked is the `originY` field of `MTLViewport`: which list is drawn is of no
/// concern to Metal, so the witness goes through the grid's **own** viewport
/// ([`Frame::set_origin_rows`] accepts a negative row) and invents no second encode path.
///
/// **Four of the five values are not settled** and in three `originY` is negative, i.e. the
/// middle of the slide: if the settled frame alone were asked, the canary would pass at rest
/// and fall in the middle of the 150 ms slide.
#[test]
fn a_negative_viewport_origin_draws_and_clips_from_the_top() {
    let r = renderer();
    const EDGE: usize = 16;
    const CELL: u16 = 8;
    // The fill is **one row**: `fill_px` is one cell, i.e. in the settled frame `origin_px` is
    // twice it and `originY` is positive; during the slide `origin_px` falls and the
    // difference passes zero and goes negative.
    const FILL_PX: f32 = CELL as f32;
    let red = LinearRgba::from_srgb(0xff, 0x00, 0x00);
    let green = LinearRgba::from_srgb(0x00, 0xff, 0x00);
    let blue = LinearRgba::from_srgb(0x00, 0x00, 0xff);
    // The clear is the **accent**, same reasoning as its siblings: it must be distinct from the
    // cells' colours, otherwise "the band moved" and "everywhere is clear" cannot be told
    // apart. ±1 because the accent is a midtone and 8-bit sRGB encoding carries rounding
    // (precedent `cell_bg_paints_pixels_on_the_gpu`).
    let clear = {
        let hex = Theme::BATERI.accent;
        ((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
    };
    let near = |seen: (u8, u8, u8), want: (u8, u8, u8)| {
        seen.0.abs_diff(want.0) <= 1 && seen.1.abs_diff(want.1) <= 1 && seen.2.abs_diff(want.2) <= 1
    };

    let mut frame = Frame::default();
    frame.clear(grid(CELL, CELL), CaretStyle::default());
    // Row 0 is the fill's, row 1 is the content's first row. Two distinct colours are
    // required: "the upper band was clipped" and "both bands slid together" can be told apart
    // only with colours distinct from each other.
    frame.push(bg_cell(0, 0, red));
    frame.push(bg_cell(0, 1, green));

    // The viewport's height stays the texture's height (as in production) and the two rows
    // cover exactly that: viewport-local y ∈ [0, EDGE) is inside, the outside is what is
    // clipped.
    for origin_px in [2.0 * FILL_PX, FILL_PX, 5.0, 3.0, 0.0] {
        let origin_y = origin_px - FILL_PX;
        frame.set_origin_rows(origin_y / f32::from(CELL));
        assert_eq!(
            frame.origin_px(),
            origin_y,
            "the offset was not converted to pixels"
        );
        let pixels = render_offscreen(&r, EDGE, ACCENT, &frame);
        for y in 0..EDGE {
            let local = y as f32 - origin_y;
            let want = if !(0.0..EDGE as f32).contains(&local) {
                // Outside the viewport — "Fragments that lie outside of the viewport are
                // clipped". The half that the negative origin tests is the **top** clipping:
                // the part of the fill band that does not fit on the screen must stay the
                // clear colour, it must not wrap.
                clear
            } else if local < FILL_PX {
                (255, 0, 0)
            } else {
                (0, 255, 0)
            };
            let seen = pixel_at(&pixels, EDGE, 2, y);
            assert!(
                near(seen, want),
                "originY={origin_y}, y={y}: {seen:02x?} ≠ {want:02x?}"
            );
        }
    }

    // **A second one after the negative viewport in the same encoder**: this is exactly 2b-i's
    // shape (grid → fill → dock). If the encoder fell at the negative origin the dock would
    // not be drawn either, so the dock's background is the witness that the whole pass came
    // out alive.
    frame.set_origin_rows(-FILL_PX / f32::from(CELL));
    frame.set_dock_rows(1);
    frame.open_dock(blue, WHITE, WHITE);
    let pixels = render_offscreen(&r, EDGE, ACCENT, &frame);
    assert!(
        near(pixel_at(&pixels, EDGE, 14, 12), (0, 0, 255)),
        "the dock's background was not drawn after the negative viewport"
    );
    // And the grid itself: with `originY = -8` row 0 was clipped entirely, row 1 sat at the
    // top of the texture.
    assert!(
        near(pixel_at(&pixels, EDGE, 2, 4), (0, 255, 0)),
        "at the negative origin the content's row did not sit at the top"
    );
}

/// The fill band's size: one row, as tall as a cell.
///
/// The two guards' shared setup is read through this number; if written by hand, one could
/// change while the other silently stayed old.
const FILL_ROWS: u16 = 1;

#[test]
fn the_fill_band_draws_above_the_content_and_rides_the_origin() {
    // **The fill band's only visible claim, the pixel half.** The band is drawn above
    // the offset (a third `setViewport`, `originY = origin_px − fill_px`) and **in a motion
    // frame** — the lists are kept, only `origin_px` changes — it slides together with the
    // grid. A position baked at push time would drop the second half: the band would freeze in
    // place, the grid would glide and the seam between them would be visible.
    let r = renderer();
    const EDGE: usize = 16;
    const CELL: u16 = 8;
    let red = LinearRgba::from_srgb(0xff, 0x00, 0x00);
    let green = LinearRgba::from_srgb(0x00, 0xff, 0x00);
    // The clear is the **accent**, same reasoning as its siblings: it must be distinct from both
    // bands' colours. ±1 because the accent is a midtone.
    let clear = {
        let hex = Theme::BATERI.accent;
        ((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
    };
    let near = |seen: (u8, u8, u8), want: (u8, u8, u8)| {
        seen.0.abs_diff(want.0) <= 1 && seen.1.abs_diff(want.1) <= 1 && seen.2.abs_diff(want.2) <= 1
    };

    let mut frame = Frame::default();
    frame.clear(grid(CELL, CELL), CaretStyle::default());
    // The content is a single row and pinned to the bottom: the offset is one row, one row of
    // space is left above it and the fill lands exactly there.
    frame.push(bg_cell(0, 0, green));
    frame.set_fill_rows(FILL_ROWS);
    // The row is **fill-local**: `0` is the band's single row, not the grid's row 0. Both
    // carry the same number and are drawn in separate spaces — the claim is exactly this.
    frame.push_fill(bg_cell(0, 0, red));
    frame.set_origin_rows(1.0);

    let cell_px = f32::from(CELL);
    let band = |pixels: &[u8], origin_px: f32| {
        for y in 0..EDGE {
            let from_fill = y as f32 - (origin_px - cell_px * f32::from(FILL_ROWS));
            let from_grid = y as f32 - origin_px;
            let want = if (0.0..cell_px).contains(&from_fill) {
                (255, 0, 0)
            } else if (0.0..cell_px).contains(&from_grid) {
                (0, 255, 0)
            } else {
                // Neither band nor content: the overflowing part of the fill is clipped (at the
                // top) or there is nothing below the grid.
                clear
            };
            let seen = pixel_at(pixels, EDGE, 2, y);
            assert!(
                near(seen, want),
                "origin_px={origin_px}, y={y}: {seen:02x?} ≠ {want:02x?}"
            );
        }
    };

    // The settled frame: band [0, 8), content [8, 16).
    band(&render_offscreen(&r, EDGE, ACCENT, &frame), 8.0);

    // **The motion frame**: in this arm `link.rs` calls neither `clear` nor `frame()`, it only
    // rewrites the offset. At half a row down the band's origin goes **negative** (−4): its
    // upper half must be clipped, its lower half must sit right above the content.
    frame.set_origin_rows(0.5);
    assert_eq!(
        frame.fill_origin_px(),
        -4.0,
        "the band's origin did not go negative"
    );
    band(&render_offscreen(&r, EDGE, ACCENT, &frame), 4.0);
}
#[test]
fn a_frame_without_fill_draws_todays_picture() {
    // **Rollback lane**, the caret's "radius 0, glow 0" pattern: with the fill off, the
    // frame drawn must be **bit for bit** identical to today's, and only the GPU can say so. Had
    // the third viewport been set up unconditionally (or had `clear` forgotten the band's
    // height), the third read would diverge from the first — the one defect that could stay
    // silent.
    let r = renderer();
    const EDGE: usize = 16;
    const CELL: u16 = 8;
    let red = LinearRgba::from_srgb(0xff, 0x00, 0x00);
    let green = LinearRgba::from_srgb(0x00, 0xff, 0x00);

    let mut frame = Frame::default();
    let today = |frame: &mut Frame| {
        frame.clear(grid(CELL, CELL), CaretStyle::default());
        frame.push(bg_cell(0, 0, green));
        frame.set_origin_rows(1.0);
    };

    today(&mut frame);
    let before = render_offscreen(&r, EDGE, ACCENT, &frame);

    // The same frame, with one fill row: it **must** diverge, otherwise the equality below
    // says nothing.
    frame.clear(grid(CELL, CELL), CaretStyle::default());
    frame.push(bg_cell(0, 0, green));
    frame.set_fill_rows(FILL_ROWS);
    frame.push_fill(bg_cell(0, 0, red));
    frame.set_origin_rows(1.0);
    let filled = render_offscreen(&r, EDGE, ACCENT, &frame);
    assert!(before != filled, "the fill band was never drawn");

    // And once the fill closes: `clear` also resets the band's height, so `encode_fill` returns
    // early and the encoder never sees the fill.
    today(&mut frame);
    let after = render_offscreen(&r, EDGE, ACCENT, &frame);
    let diff = before.iter().zip(&after).position(|(a, b)| a != b);
    assert!(
        diff.is_none(),
        "the frame diverged from today's picture once the fill closed, first difference at byte {diff:?}"
    );
}

#[test]
fn command_marks_paint_the_gutter_on_the_gpu() {
    // The GPU side of the mark: `Frame::stripes` is a **CPU** list and, unlike its sibling
    // counters, has not even a smoke token — if this test fails, no other guard is left to say
    // the mark was drawn.
    //
    // **The mark is now a sprite**, not a rectangle: the same shape as the dock's chevron
    // (user: "the result color boxes will be this new one too"). The shape's own guard
    // lives in `bt-atlas`; this test's job is the pipeline — the right row, the right color,
    // inside the gutter.
    let r = renderer();
    const EDGE: usize = 64;
    // The cell is at the **atlas's own size**: shrinking the sprite to a quarter scale would
    // melt the coverage and the test would measure the scaling, not the shape.
    let (cw, ch) = fitting_cell_px(&r, EDGE, 2);
    assert!(
        usize::from(ch) * 3 <= EDGE,
        "three rows do not fit the texture"
    );
    // The gutter is one cell; the mark is no longer in the gutter but in **column 0**,
    // so the gutter is a pure left margin.
    let gutter = cw;

    let mut frame = Frame::default();
    frame.clear(
        CellMetrics::new(cw, ch, cw, gutter, 1, 1.0).expect("metrics"),
        CaretStyle::default(),
    );
    // Two marks, two status colors: row 0 succeeded, row 2 failed. The row in between (output)
    // must stay **unmarked**.
    frame.push_block(Block {
        row: 0,
        stripe: Theme::BATERI.success_linear(),
    });
    frame.push_block(Block {
        row: 2,
        stripe: Theme::BATERI.error_linear(),
    });
    // The cell of the command's **first letter**: column 2, because the prompt really is two
    // columns wide (`__bateri_ps1`). A witness that the mark does not touch it.
    frame.push(bg_cell(2, 0, WHITE));

    // Clear is outside all three colors: every pixel without a mark must read it, so that a
    // "the mark overflowed the gutter" error can be told apart from clear.
    let pixels = render_offscreen(&r, EDGE, ACCENT, &frame);
    let pixel = |x: usize, y: usize| pixel_at(&pixels, EDGE, x, y);
    let srgb = |hex: u32| ((hex >> 16) as u8, (hex >> 8) as u8, hex as u8);

    // Because of anti-aliasing, **equality cannot be asked**: only the sprite's core gives full
    // coverage. The claim therefore looks at distance — which mark color the band's pixel
    // farthest from clear is closest to.
    let distance = |a: (u8, u8, u8), b: (u8, u8, u8)| {
        i32::from(a.0).abs_diff(i32::from(b.0)).pow(2)
            + i32::from(a.1).abs_diff(i32::from(b.1)).pow(2)
            + i32::from(a.2).abs_diff(i32::from(b.2)).pow(2)
    };
    let clear = srgb(Theme::BATERI.accent);
    let band = usize::from(ch);
    // The mark's band is **column 0**, not the gutter: the gutter is now empty.
    let mark_x = usize::from(gutter)..usize::from(gutter) + usize::from(cw);
    let boldest = |row: usize| {
        mark_x
            .clone()
            .flat_map(|x| (row * band..(row + 1) * band).map(move |y| (x, y)))
            .map(|(x, y)| pixel(x, y))
            .max_by_key(|&seen| distance(seen, clear))
            .expect("band is empty")
    };
    let (success, error) = (srgb(Theme::BATERI.success), srgb(Theme::BATERI.error));
    let first = boldest(0);
    assert!(
        distance(first, success) < distance(first, error),
        "the first command is not in the success color: {first:02x?}"
    );
    let second = boldest(2);
    assert!(
        distance(second, error) < distance(second, success),
        "the second command is not in the error color: {second:02x?}"
    );
    // **The output row is unmarked** (user decision): its gutter must stay the
    // **same** as the clear color, not a single ink pixel. This is the one-pixel proof that the
    // mark stays one cell tall — if the height turned into the line spacing, this would fail.
    assert_eq!(boldest(1), clear, "the output row received a mark");

    // **The command's letter is untouched:** column 2 stayed white. The mark is one cell tall
    // and in column 0; since the prompt is two columns wide, column 1 in between is empty too,
    // so the mark touches the text at no scale.
    assert_eq!(
        pixel(
            usize::from(gutter) + 2 * usize::from(cw) + usize::from(cw) / 2,
            band / 2
        ),
        (255, 255, 255),
        "the command's first letter is not to the right of the mark"
    );
    // **The left gutter is empty.** The mark moved from there to column 0; had ink remained in
    // the gutter, the alignment between the two marks would be off again.
    for x in 0..usize::from(gutter) {
        assert_eq!(pixel(x, band / 2), clear, "there is ink in the left gutter");
    }
}

#[test]
fn the_dock_paints_the_bottom_band_and_the_sliding_grid_cannot_reach_it() {
    // **Phase-3's CPU-to-GPU seam.** The second `setViewport` claims two things at once and
    // both can only be read from pixels: the dock sticks to the **bottom** of the texture, and
    // the grid's offset does not move it. Asking either by comparing two CPU lists would be
    // testing what is correct by construction (same reasoning as
    // `content_sticks_to_the_bottom_*`).
    //
    // The third claim is the slide's overflow: the doc of `LinkDelegate::set_origin` says "the
    // offset during the slide is larger than its target, part of the bottom row stays below the
    // window" — once the dock arrives, that part lands **on top of** the dock and the only
    // thing covering it is the opaque ground. The offset frame must therefore still yield the
    // dock's colors in the same band.
    let r = renderer();
    const EDGE: usize = 16;
    const CELL: u16 = 8;
    let red = LinearRgba::from_srgb(0xff, 0x00, 0x00);
    let green = LinearRgba::from_srgb(0x00, 0xff, 0x00);
    let blue = LinearRgba::from_srgb(0x00, 0x00, 0xff);
    // All three colors are **saturated**: fixed points of the sRGB transfer function, so they
    // can be asked with byte equality. The color space's own guard is
    // `cell_bg_paints_pixels_on_the_gpu` and it has a midtone.

    // A one-row dock: 8 pixels, so the texture splits in two — grid on top, dock below.
    // `DOCK_ROWS` is **not used** here, deliberately: the renderer does not know how many rows
    // there are, it only draws the gutter it is given.
    let mut frame = Frame::default();
    frame.clear(grid(CELL, CELL), CaretStyle::default());
    frame.push(bg_cell(0, 0, red));
    frame.push_dock(bg_cell(0, 0, blue));
    frame.set_dock_rows(1);
    frame.open_dock(green, WHITE, WHITE);

    let pixels = render_offscreen(&r, EDGE, ACCENT, &frame);
    let pixel = |x: usize, y: usize| pixel_at(&pixels, EDGE, x, y);

    // The top half is the grid's: row 0's cell is there.
    assert_eq!(
        pixel(2, 2),
        (255, 0, 0),
        "the grid cell is not in the top half"
    );
    // The bottom half is the dock's: its cell on the left, its ground to the right of it **as
    // wide as the texture**. Had the ground covered only the grid's columns, the leftover
    // stripe on the right would have stayed the clear color.
    assert_eq!(pixel(2, 12), (0, 0, 255), "the dock cell was not painted");
    assert_eq!(
        pixel(14, 12),
        (0, 255, 0),
        "the dock ground did not cover the texture"
    );
    // The separator is at the dock's topmost pixel and distinct from the ground: had the two
    // collapsed into a single rectangle, the boundary would vanish.
    assert_eq!(
        pixel(14, 8),
        (255, 255, 255),
        "the separator is not at the top of the dock"
    );

    // **The same frame, offset by one row.** The grid's cell spills into the bottom half (the
    // setup of `content_sticks_to_the_bottom_for_cell_bg`) and lands right on top of the dock.
    // Because the dock is drawn **after** it and its ground is opaque, the bottom half must
    // see no red at all; the dock itself must not move either.
    frame.set_origin_rows(1.0);
    let pixels = render_offscreen(&r, EDGE, ACCENT, &frame);
    let pixel = |x: usize, y: usize| pixel_at(&pixels, EDGE, x, y);
    assert_eq!(
        pixel(2, 12),
        (0, 0, 255),
        "the dock slid along with the offset"
    );
    assert_eq!(
        pixel(14, 12),
        (0, 255, 0),
        "the dock ground slid with the offset"
    );
    assert!(
        (8..EDGE).all(|y| (0..EDGE).all(|x| pixel(x, y) != (255, 0, 0))),
        "the sliding grid showed up on top of the dock"
    );
    // The grid's old place emptied: the offset really was applied, otherwise the claim above
    // would also pass with code that **ignores** the offset.
    assert_ne!(pixel(2, 2), (255, 0, 0), "the grid was not offset");
}

#[test]
fn no_band_paints_no_dock_pixel_and_the_grid_reaches_the_bottom() {
    // **No band** (a program reading the keyboard itself): the surface is open but the
    // band's height is zero — the ground and both hairlines must not reach a single pixel,
    // and the grid, the whole share lower, owns the window's bottom. Asked of pixels, not of
    // the zero-size instances: a viewport set at the texture's bottom edge is the renderer's
    // to clip.
    let r = renderer();
    const EDGE: usize = 16;
    const CELL: u16 = 8;
    let red = LinearRgba::from_srgb(0xff, 0x00, 0x00);
    let green = LinearRgba::from_srgb(0x00, 0xff, 0x00);
    let mut frame = Frame::default();
    frame.clear(grid(CELL, CELL), CaretStyle::default());
    frame.push(bg_cell(0, 0, red));
    frame.set_dock_input_rows(None);
    // A one-row share, settled at no band: the excess is the whole share.
    frame.set_dock_share(1);
    frame.set_dock_band(EDGE as f32, -1.0);
    frame.open_dock(green, WHITE, WHITE);
    assert_eq!(frame.dock_band_px(), 0.0);

    let pixels = render_offscreen(&r, EDGE, ACCENT, &frame);
    let pixel = |x: usize, y: usize| pixel_at(&pixels, EDGE, x, y);
    assert_eq!(
        pixel(2, 12),
        (255, 0, 0),
        "the grid's row is not at the bottom"
    );
    assert_eq!(
        pixel(14, 12),
        pixel(14, 2),
        "something painted beside the grid's cell"
    );
    for y in 0..EDGE {
        for x in 0..EDGE {
            assert_ne!(pixel(x, y), (0, 255, 0), "the dock's ground at {x},{y}");
            assert_ne!(pixel(x, y), (255, 255, 255), "a hairline at {x},{y}");
        }
    }
}

#[test]
fn a_growing_band_reveals_its_rows_from_the_bottom() {
    // **The growing band, the pixel half.** A dock with three input rows (four-row layout): the
    // cells are bottom-anchored and come from the layout's viewport, the ground from the band's
    // current height. While the band has not yet risen (extra 0), an input row spilling over
    // the top of the band **must not be drawn** — it would be text without a ground, on top of
    // the grid's bottom rows. The scissor does the clipping (`scissor_below`) and only on a
    // growing band.
    //
    // 48 px texture, 8 px cell, no gutter: the layout is 32 px (16..48), the PTY gutter is two
    // rows (32..48). Row 0 → 16..24, row 2 → 32..40, context row → 40..48.
    let r = renderer();
    const EDGE: usize = 48;
    const CELL: u16 = 8;
    let blue = LinearRgba::from_srgb(0x00, 0x00, 0xff);
    let red = LinearRgba::from_srgb(0xff, 0x00, 0x00);
    let green = LinearRgba::from_srgb(0x00, 0xff, 0x00);
    let draw = |extra: f32| {
        let mut frame = Frame::default();
        frame.clear(grid(CELL, CELL), CaretStyle::default());
        frame.set_dock_rows(4);
        frame.push_dock(bg_cell(0, 0, blue));
        frame.push_dock(bg_cell(0, 2, blue));
        frame.push_dock(bg_cell(0, 3, red));
        frame.set_dock_band(EDGE as f32, extra);
        frame.open_dock(green, WHITE, WHITE);
        render_offscreen(&r, EDGE, ACCENT, &frame)
    };

    // A resting band: all three rows in place, the ground covers the layout.
    let pixels = draw(2.0);
    let pixel = |x: usize, y: usize| pixel_at(&pixels, EDGE, x, y);
    assert_eq!(
        pixel(2, 20),
        (0, 0, 255),
        "the first input row was not drawn"
    );
    assert_eq!(
        pixel(2, 36),
        (0, 0, 255),
        "the last input row is not in place"
    );
    assert_eq!(
        pixel(2, 44),
        (255, 0, 0),
        "the context row is not at the bottom"
    );
    assert_eq!(
        pixel(2, 28),
        (0, 255, 0),
        "the ground did not cover the input block"
    );

    // The start of growth: the band is as tall as the PTY gutter. The context row and the last
    // input row are at the **same pixel** (bottom-anchored), and the row above the band is
    // absent.
    let pixels = draw(0.0);
    let pixel = |x: usize, y: usize| pixel_at(&pixels, EDGE, x, y);
    assert_eq!(
        pixel(2, 44),
        (255, 0, 0),
        "the context row slid with the band"
    );
    assert_eq!(
        pixel(2, 36),
        (0, 0, 255),
        "the last input row slid with the band"
    );
    assert_ne!(
        pixel(2, 20),
        (0, 0, 255),
        "a row spilling over the top of the band was drawn"
    );
    assert_ne!(
        pixel(2, 28),
        (0, 255, 0),
        "the ground spilled over the top of the band"
    );
    // The top of the band is the hairline: 48 − 16 = 32.
    assert_eq!(
        pixel(20, 32),
        (255, 255, 255),
        "the top hairline did not rise with the band"
    );
}

#[test]
fn the_dock_draws_glyphs_and_its_own_caret() {
    // The dock's second pipeline: glyphs and the caret. The caret's rectangle is compared with
    // the fragment's `[[position]]`, and that coordinate is **after** the viewport transform,
    // while the dock lists are dock-local — `Frame::dock_caret(origin_y)` joins the two. Had
    // the shift been forgotten, the letter under the caret would be painted in the ground color
    // **in the grid**, on a row above the dock: a defect that leaves `make check` green and is
    // noticed by eye as "a cell became invisible".
    let r = renderer();
    const EDGE: usize = 64;
    let (cw, ch) = fitting_cell_px(&r, EDGE, 2);

    let red = LinearRgba::from_srgb(0xff, 0x00, 0x00);
    let mut frame = Frame::default();
    frame.clear(grid(cw, ch), CaretStyle::default());
    // The grid's last row stays above the dock; the dock is **one** row and at the bottom of
    // the texture.
    frame.push_dock(glyph_cell(0, 'M', Some(red)));
    // The caret is **single** and in window space: we give the top of the dock band and put it
    // on the band's first row. `Frame` picks the slot (`Frame::push_caret`) and the encode
    // draws it after the dock's ground — the pixel this test sees is exactly the witness of that
    // order.
    let dock_top = (EDGE - usize::from(ch)) as f32;
    frame.set_dock_top(dock_top);
    frame.push_caret(
        [0.0, dock_top / f32::from(ch)],
        BACKGROUND,
        WHITE,
        1.0,
        CaretShape::Block,
        true,
    );
    frame.set_dock_rows(1);
    frame.open_dock(red, WHITE, WHITE);

    let pixels = render_offscreen(&r, EDGE, ACCENT, &frame);
    // The dock is anchored to the **bottom** of the texture, not counted in cells from the top:
    // the offset is `height − dock gutter`. Had it been counted from the top, it would shift by
    // the leftover stripe between the grid's last row and the dock (the pixels left over from
    // dividing by the cell height) and the band would read the wrong place.
    let top = EDGE - usize::from(ch);
    let cell: Vec<_> = (0..usize::from(ch))
        .flat_map(|y| (0..usize::from(cw)).map(move |x| (x, y)))
        .map(|(x, y)| pixel_at(&pixels, EDGE, x, top + y))
        .collect();

    // Two claims, and the second is **exactly the guard of the shift**: the caret draws its
    // opaque white block onto the dock's row, while the `M` under it is painted in the ground
    // color. Had the shift been forgotten, the rectangle would stay dock-local, i.e. be
    // compared against the grid's first row: the block would still be drawn here (that instance
    // goes through the viewport) but the letter would be drawn with its own foreground, i.e.
    // **white**, and the cell would stay uniformly white. The letter would vanish and no
    // counter would see it.
    const WHITE_PX: (u8, u8, u8) = (0xff, 0xff, 0xff);
    assert!(
        cell.contains(&WHITE_PX),
        "the caret block was not drawn on the dock's row: {cell:?}"
    );
    // No exact byte is looked for: glyph coverage is partial at the edges and even the darkest
    // pixel only approaches the ground (same reasoning as `glyph_differs_from_cell_background`
    // — the gate must not be held hostage to the system font's version). What is asked is the
    // **direction** of the difference: the ground (black) is darker than white.
    let darkest = cell
        .iter()
        .map(|p| p.0)
        .min()
        .expect("the cell is not empty");
    assert!(
        darkest < 0x80,
        "the glyph under the caret was not painted in the ground color: {cell:?}"
    );
}

#[test]
fn an_upload_button_paints_a_fill_and_a_brighter_edge_in_the_dock() {
    // The button's fill and frame come from the caret's fragment, in the dock's
    // viewport. The core must be in window space: had it stayed dock-local, the SDF would
    // measure outside the quad and no pixel would be painted — a class the counters cannot see.
    let r = renderer();
    const EDGE: usize = 64;
    let (cw, ch) = fitting_cell_px(&r, EDGE, 4);
    let black = LinearRgba::from_srgb(0, 0, 0);
    let red = LinearRgba::from_srgb(0xff, 0, 0);
    let render = |state: ButtonState| {
        let mut frame = Frame::default();
        frame.clear(grid(cw, ch), CaretStyle::default());
        frame.set_dock_rows(1);
        frame.open_dock(black, black, black);
        frame.set_dock_buttons([
            None,
            Some(DockButton {
                start: 0,
                end: 3,
                color: red,
                state,
            }),
        ]);
        let origin_y = EDGE as f32 - frame.dock_layout_px();
        let core = frame.dock_button_draws(origin_y).next().expect("fill").core;
        (render_offscreen(&r, EDGE, ACCENT, &frame), core)
    };
    let (idle, core) = render(ButtonState::Idle);
    let (hover, _) = render(ButtonState::Hover);
    let mid_y = ((core[1] + core[3]) / 2.0) as usize;
    let mid_x = ((core[0] + core[2]) / 2.0) as usize;
    let inside = pixel_at(&idle, EDGE, mid_x, mid_y);
    let edge = pixel_at(&idle, EDGE, core[0] as usize, mid_y);
    let outside = pixel_at(&idle, EDGE, core[2] as usize + 2, mid_y);
    assert!(
        inside.0 > 0x10 && inside.1 < 0x08,
        "the fill was not drawn: {inside:02x?}"
    );
    assert!(
        edge.0 > inside.0,
        "the frame is not more prominent than the fill: {edge:02x?} ≤ {inside:02x?}"
    );
    assert!(
        outside.0 < 0x08,
        "the area outside the button's range was painted: {outside:02x?}"
    );
    let hovered = pixel_at(&hover, EDGE, mid_x, mid_y);
    assert!(
        hovered.0 > inside.0,
        "it did not darken while the mouse was over it: {hovered:02x?}"
    );
}

/// The sRGB byte a linear channel encodes to — the transfer the target's
/// `_sRGB` format applies on write. The reference the blend tests compare
/// with, not a production path: the GPU does the encoding.
fn srgb_byte(linear: f32) -> u8 {
    let encoded = if linear <= 0.003_130_8 {
        12.92 * linear
    } else {
        1.055 * linear.powf(1.0 / 2.4) - 0.055
    };
    (encoded * 255.0).round() as u8
}

/// The scroll bar marks' two colours in the pixel tests: a match's, then
/// the current match's — pure channels, so a pixel says which one it is.
const MARK_COLORS: [LinearRgba; 2] = [
    LinearRgba::from_srgb(0x00, 0xff, 0x00),
    LinearRgba::from_srgb(0x00, 0x00, 0xff),
];

/// A frame with a dock and a scroll bar at the bottom of its travel, drawn
/// with `look` (`None` → the bar is never set) and `marks`: the frame, the
/// layout and the floor the track ends at.
fn scroll_bar_marked(
    edge: usize,
    look: Option<Look>,
    marks: &[TrackMark],
) -> (Frame, crate::scrollbar::ScrollbarLayout, f32) {
    let cell = grid(8, 16);
    let mut frame = Frame::default();
    frame.clear(cell, CaretStyle::default());
    frame.set_dock_rows(1);
    let ground = LinearRgba::from_srgb(0xff, 0x00, 0x00);
    frame.open_dock(ground, ground, ground);
    frame.set_dock_band(edge as f32, 0.0);
    let floor = frame.band_top_px(edge as f32);
    let position = bt_core::ScrollPosition {
        room: 100,
        top: 100.0,
        visible: 2,
    };
    let layout = crate::scrollbar::ScrollbarLayout::new(Some(position), edge as f32, floor, cell);
    if let Some(look) = look {
        frame.set_scrollbar(layout, look, WHITE, marks, MARK_COLORS);
    }
    (frame, layout, floor)
}

/// [`scroll_bar_marked`] with no marks: the frame, the thumb and the floor.
fn scroll_bar_frame(edge: usize, look: Option<Look>) -> (Frame, [f32; 4], f32) {
    let (frame, layout, floor) = scroll_bar_marked(edge, look, &[]);
    let wide = look.map_or(0.0, |look| look.wide);
    (frame, layout.thumb(wide), floor)
}

/// The pixel at a rectangle's centre.
fn centre_of(pixels: &[u8], edge: usize, [x0, y0, x1, y1]: [f32; 4]) -> (u8, u8, u8) {
    pixel_at(
        pixels,
        edge,
        ((x0 + x1) / 2.0) as usize,
        ((y0 + y1) / 2.0) as usize,
    )
}

#[test]
fn a_thin_bar_draws_its_search_marks_over_the_thumb_in_its_column() {
    // The window at the bottom shows the history's last two rows (100, 101):
    // their marks land on the thumb, in its column, opaque over it — the
    // current one in its own colour. A row far up the history lands on the
    // bare track above the thumb (a texture tall enough for a track longer
    // than the thumb's minimum).
    const EDGE: usize = 128;
    let r = renderer();
    let marks = [
        TrackMark {
            position: 10.0,
            current: false,
        },
        TrackMark {
            position: 101.0,
            current: true,
        },
    ];
    let (frame, layout, _) = scroll_bar_marked(
        EDGE,
        Some(Look::auto(1.0, 0.0, crate::scrollbar::THUMB_ALPHA)),
        &marks,
    );
    let pixels = render_offscreen(&r, EDGE, BACKGROUND, &frame);
    let thumb = layout.thumb(0.0);
    let current = layout.search_mark(101.0, 0.0);
    assert_eq!(
        (current[0], current[2]),
        (thumb[0], thumb[2]),
        "the mark left the thumb's column"
    );
    assert!(
        current[1] >= thumb[1] && current[3] <= thumb[3],
        "the window's row is not on the thumb: {current:?} / {thumb:?}"
    );
    assert_eq!(
        centre_of(&pixels, EDGE, current),
        (0x00, 0x00, 0xff),
        "the current mark is not over the thumb"
    );
    let far = layout.search_mark(10.0, 0.0);
    assert!(far[3] < thumb[1], "{far:?} / {thumb:?}");
    assert_eq!(centre_of(&pixels, EDGE, far), (0x00, 0xff, 0x00));
    // The thumb around the mark is still the thumb.
    let x = ((thumb[0] + thumb[2]) / 2.0) as usize;
    let above = pixel_at(&pixels, EDGE, x, current[1] as usize - 2);
    assert!(
        above.0 == above.1 && above.1 == above.2 && above.0 > 0,
        "the thumb beside the mark: {above:02x?}"
    );
}

#[test]
fn a_wide_bar_draws_its_search_marks_in_the_right_lane() {
    // Wide, the mark leaves the thumb's column for the strip's right lane
    // (9–14 points of 16): the lane is the mark's, the wide thumb's left
    // part at the same height stays the thumb's grey.
    const EDGE: usize = 64;
    let r = renderer();
    let marks = [TrackMark {
        position: 10.0,
        current: false,
    }];
    let (frame, layout, _) = scroll_bar_marked(EDGE, Some(Look::ALWAYS), &marks);
    let pixels = render_offscreen(&r, EDGE, BACKGROUND, &frame);
    let mark = layout.search_mark(10.0, 1.0);
    let strip = layout.strip_x();
    assert_eq!((mark[0] - strip, mark[2] - strip), (9.0, 14.0));
    assert_eq!(centre_of(&pixels, EDGE, mark), (0x00, 0xff, 0x00));
    let left = pixel_at(
        &pixels,
        EDGE,
        strip as usize + 4,
        ((mark[1] + mark[3]) / 2.0) as usize,
    );
    assert!(
        left.0 == left.1 && left.1 == left.2 && left.0 > 0,
        "the mark spilled out of its lane: {left:02x?}"
    );
}

#[test]
fn a_hidden_bar_draws_no_marks() {
    // Marks are the bar's: a faded bar is no op at all, its marks included.
    const EDGE: usize = 64;
    let r = renderer();
    let marks = [TrackMark {
        position: 101.0,
        current: true,
    }];
    let (hidden, ..) = scroll_bar_marked(
        EDGE,
        Some(Look::auto(0.0, 0.0, crate::scrollbar::THUMB_ALPHA)),
        &marks,
    );
    assert!(
        hidden
            .scrollbar_marks()
            .iter()
            .all(|(marks, _)| marks.is_empty())
    );
    let (never, ..) = scroll_bar_frame(EDGE, None);
    assert_eq!(
        render_offscreen(&r, EDGE, BACKGROUND, &hidden),
        render_offscreen(&r, EDGE, BACKGROUND, &never),
        "the hidden bar's marks changed pixels"
    );
}

#[test]
fn a_shown_scroll_bar_blends_the_foreground_and_stops_above_the_dock() {
    // The thumb is the foreground at the thumb's opacity over the ground, in
    // **linear** space (the target encodes on write): white at 36 % over
    // black is 0.36 linear. It ends above the dock — the track's margin is
    // ground, the dock's band is the dock's.
    const EDGE: usize = 64;
    let r = renderer();
    let (frame, [x0, y0, x1, y1], floor) = scroll_bar_frame(
        EDGE,
        Some(Look::auto(1.0, 0.0, crate::scrollbar::THUMB_ALPHA)),
    );
    assert!(
        y1 < floor,
        "the thumb reaches into the dock: {y1} ≥ {floor}"
    );
    let pixels = render_offscreen(&r, EDGE, BACKGROUND, &frame);
    let x = ((x0 + x1) / 2.0) as usize;
    let mid = pixel_at(&pixels, EDGE, x, ((y0 + y1) / 2.0) as usize);
    let expected = srgb_byte(crate::scrollbar::THUMB_ALPHA);
    assert!(
        mid.0.abs_diff(expected) <= 2 && mid.0 == mid.1 && mid.1 == mid.2,
        "the thumb is not the foreground at its opacity: {mid:02x?} ≠ ~{expected:02x}"
    );
    let margin = pixel_at(&pixels, EDGE, x, (floor - 1.0) as usize);
    assert_eq!(margin, (0, 0, 0), "the bar painted the track's margin");
    let dock = pixel_at(&pixels, EDGE, x, (floor + 1.0) as usize);
    assert_eq!(dock, (0xff, 0, 0), "the dock's band is not the dock's");
    // Left of the thumb is untouched ground.
    assert_eq!(
        pixel_at(&pixels, EDGE, x0 as usize - 2, ((y0 + y1) / 2.0) as usize),
        (0, 0, 0)
    );
}

#[test]
fn an_always_up_scroll_bar_paints_its_track_inside_the_reserve() {
    // The wide form: a faint track the full height down to the dock, a
    // brighter hairline at its left edge, the thumb at its quieter opacity
    // over the track — and nothing left of the track, the room the grid
    // gives up.
    const EDGE: usize = 64;
    let r = renderer();
    let (frame, [x0, y0, x1, y1], floor) = scroll_bar_frame(EDGE, Some(Look::ALWAYS));
    let pixels = render_offscreen(&r, EDGE, BACKGROUND, &frame);
    let near = |(r, g, b): (u8, u8, u8), linear: f32, what: &str| {
        let expected = srgb_byte(linear);
        assert!(
            r.abs_diff(expected) <= 2 && r == g && g == b,
            "{what}: {:02x?} ≠ ~{expected:02x}",
            (r, g, b)
        );
    };
    let strip_x = EDGE as f32 - crate::scrollbar::Mode::Always.reserve_px(grid(8, 16));
    let mid_y = ((y0 + y1) / 2.0) as usize;
    let track = crate::scrollbar::TRACK_ALPHA;
    // Over the track the thumb blends twice: the track, then the thumb.
    let thumb = crate::scrollbar::ALWAYS_THUMB_ALPHA;
    near(
        pixel_at(&pixels, EDGE, ((x0 + x1) / 2.0) as usize, mid_y),
        thumb + track * (1.0 - thumb),
        "the thumb",
    );
    near(
        pixel_at(&pixels, EDGE, strip_x as usize, mid_y),
        crate::scrollbar::HAIRLINE_ALPHA,
        "the hairline",
    );
    near(
        pixel_at(&pixels, EDGE, strip_x as usize + 1, mid_y),
        track,
        "the track",
    );
    near(
        pixel_at(&pixels, EDGE, strip_x as usize + 1, (floor - 1.0) as usize),
        track,
        "the track's foot",
    );
    assert_eq!(
        pixel_at(&pixels, EDGE, strip_x as usize - 1, mid_y),
        (0, 0, 0),
        "the bar painted left of its reserve"
    );
    assert_eq!(
        pixel_at(&pixels, EDGE, strip_x as usize + 1, (floor + 1.0) as usize),
        (0xff, 0, 0),
        "the track ran into the dock"
    );
}

#[test]
fn a_hidden_scroll_bar_changes_no_pixel() {
    // A fully faded bar is not a transparent quad, it is no op at all: the
    // frame is byte for byte the one that never had a bar.
    const EDGE: usize = 64;
    let r = renderer();
    let (hidden, ..) = scroll_bar_frame(
        EDGE,
        Some(Look::auto(0.0, 0.0, crate::scrollbar::THUMB_ALPHA)),
    );
    assert!(hidden.scrollbar().is_none(), "a hidden bar planned a draw");
    let (never, ..) = scroll_bar_frame(EDGE, None);
    assert_eq!(
        render_offscreen(&r, EDGE, BACKGROUND, &hidden),
        render_offscreen(&r, EDGE, BACKGROUND, &never),
        "the hidden bar changed pixels"
    );
}

#[test]
fn glyph_differs_from_cell_background() {
    // `make smoke`'s `glyphs=G` token is a CPU counter: it would print G > 0 even if the atlas
    // were empty and the glyph pipeline never drew. This is the place that proves the GPU side
    // — and **no exact byte is looked for**: the inside of the cell is NOT uniform with its
    // background, that is all. Had bytes been looked for, the gate would be held hostage to the
    // system font's version.
    let r = renderer();
    // The scale is stated explicitly: the atlas's key comes from the window, this test has no
    // window and the `MetalRenderer` atlas is born `None`. Had it not been stated, the frame
    // would fail with `GpuError::NoAtlas` — instead of silently drawing at @1x. The cell size
    // should also be the atlas's own so that the slot fits the quad exactly.
    const EDGE: usize = 64;
    let (cw, ch) = fitting_cell_px(&r, EDGE, 2);

    // A white `M` and `.` on a pure red background: both are saturated, the difference is
    // whatever the coverage is.
    //
    // **Two glyphs are required**, same reasoning as `cell_bg`'s two instances: with a single
    // glyph, a break in the uv arithmetic is INVISIBLE. Had uv0 stayed pinned to slot 0 (e.g.
    // if `slot_origin` were ignored), the shader would sample the resident tofu box and all
    // three claims "the background is there + a different pixel is there + it carries the
    // foreground color" would pass. Asking whether two different slots draw two different things
    // closes that door: `M` fills the cell, `.` puts only a small dot at its baseline.
    let red = LinearRgba::from_srgb(0xff, 0x00, 0x00);
    let mut frame = Frame::default();
    frame.clear(grid(cw, ch), CaretStyle::default());
    for (col, glyph) in [(0u16, 'M'), (1, '.')] {
        frame.push(glyph_cell(col, glyph, Some(red)));
    }
    assert_eq!(frame.glyph_count(), 2);

    let pixels = render_offscreen(&r, EDGE, BACKGROUND, &frame);
    let (m, dot) = (
        cell_rows(&pixels, EDGE, (cw, ch), 0).concat(),
        cell_rows(&pixels, EDGE, (cw, ch), 1).concat(),
    );

    // Four claims, four separate defects: the background is still visible (the glyph quad did
    // not paint the cell entirely), at least one pixel differs from it (the glyph really was
    // drawn), the direction of the difference is the foreground color (red has no green, white
    // has — so the difference comes from `rgba`, not from random garbage) and two slots draw
    // two different things (the uv arithmetic).
    let bg = (0xff, 0x00, 0x00);
    assert!(
        m.contains(&bg),
        "no background is left inside the cell: {m:?}"
    );
    assert!(
        m.iter().any(|&p| p != bg),
        "the inside of the cell is uniform with the background: the glyph was not drawn"
    );
    assert!(
        m.iter().any(|&(_, g, _)| g > 0),
        "there is a different pixel but it does not carry the foreground color"
    );
    assert_ne!(
        m, dot,
        "two slots drew the same thing: the uv does not depend on the slot"
    );

    // Alpha: the fragment outputs non-premultiplied and the blend's **alpha** factor must be
    // `One`. Had it been `SourceAlpha`, the target's alpha would drop below 1 on a
    // half-covered edge; since `CAMetalLayer` is not opaque, the compositor would honor that
    // hole and whatever is behind the window would leak through the letter's edge. No color
    // claim can see this.
    let alphas: Vec<u8> = (0..usize::from(ch))
        .flat_map(|y| (0..usize::from(cw)).map(move |x| (x, y)))
        .map(|(x, y)| pixels[(y * EDGE + x) * 4 + 3])
        .collect();
    assert!(
        alphas.iter().all(|&a| a == 0xff),
        "alpha hole at the glyph edge: {alphas:?}"
    );
}

#[test]
fn atlas_occupancy_is_republished() {
    // `bt-shell` has no edge to `bt-atlas` and must not; occupancy passes through here using
    // the `cell_metrics` pattern.
    //
    // The criterion is **an increase**, not equality: calling `Atlas::occupancy()` a second
    // time here would copy the body of the function under test into the test, and that
    // `assert_eq!` could never fail under any condition. An `atlas_occupancy` returning a
    // constant pair would pass it too. Two different glyphs raising the slot count **one by
    // one** says the value really comes from the atlas itself.
    let r = renderer();
    const EDGE: usize = 64;
    // The atlas is born at the first metrics query; before that, occupancy is (0, 0).
    assert_eq!(
        r.atlas_occupancy(),
        (0, 0),
        "no atlas before metrics are asked"
    );
    let (cw, ch) = fitting_cell_px(&r, EDGE, 2);
    // A fresh atlas is **not empty**: the resident tofu slot is already open ([`TOFU`]). The
    // base is therefore read, not assumed to be zero.
    let base = r.atlas_occupancy();
    assert!(base.1 > 0, "capacity cannot be zero: {base:?}");
    assert!(
        base.0 < base.1,
        "the base must not fill the capacity: {base:?}"
    );

    let mut used = base.0;
    for (col, glyph) in [(0u16, 'M'), (1, '.')] {
        let mut frame = Frame::default();
        frame.clear(grid(cw, ch), CaretStyle::default());
        frame.push(glyph_cell(col, glyph, None));
        render_offscreen(&r, EDGE, BACKGROUND, &frame);

        let now = r.atlas_occupancy();
        assert_eq!(
            now.0,
            used + 1,
            "{glyph:?} should have opened one slot: {now:?}"
        );
        assert_eq!(now.1, base.1, "capacity must not change: {now:?}");
        used = now.0;
    }
}

#[test]
fn rule_band_is_not_uniform_along_x() {
    // `bt-atlas` proves that the curl's **bitmap** is a wave (`curl_is_really_a_wave`, without
    // the GPU). What this proves is that the wave **survives the GPU path**: with the right
    // slot's uv, through the `cell` pipeline, in the rule list's own pass. Code that pins the
    // slot or draws the rule as a straight line would get lost between the two — the `kural=R`
    // counter cannot see the style distinction (see `Frame::rule_count`).
    //
    // **No exact byte is looked for**: the claim is "the band's row is not uniform along x".
    // Had bytes been looked for, the gate would be held hostage to the font's `underline_px`
    // and to `CURL_FACTOR`.
    let r = renderer();
    const EDGE: usize = 64;
    let (cw, ch) = fitting_cell_px(&r, EDGE, 2);

    // The straight line is the **control**: the claim "a row is not uniform" alone would also
    // hold for code that fills the cell with garbage. Together the two say "the curl is wavy
    // **and** the straight line is straight".
    let mut frame = Frame::default();
    frame.clear(grid(cw, ch), CaretStyle::default());
    frame.push(rule_cell(0, UnderlineStyle::Single));
    frame.push(rule_cell(1, UnderlineStyle::Curl));
    assert_eq!(frame.rule_count(), 2);
    assert_eq!(frame.glyph_count(), 0, "a rule cell produces no ink");

    let pixels = render_offscreen(&r, EDGE, BACKGROUND, &frame);
    let uniform = |row: &Vec<(u8, u8, u8)>| row.iter().all(|p| *p == row[0]);
    let single = cell_rows(&pixels, EDGE, (cw, ch), 0);
    let curl = cell_rows(&pixels, EDGE, (cw, ch), 1);

    let clear = single[0][0];
    assert!(
        single.iter().flatten().any(|&p| p != clear),
        "the straight underline was not drawn: {single:?}"
    );
    assert!(
        single.iter().all(uniform),
        "the straight line's band is not uniform along x: {single:?}"
    );
    assert!(
        curl.iter().any(|row| !uniform(row)),
        "no row of the curl varies along x: the wave has degraded into a straight line"
    );
}

#[test]
fn sgr58_color_differs_from_foreground() {
    // SGR 58 arrives from `bt-core` as `Cell::underline_color` and `Frame::push` puts it **in
    // place of** `fg`. If it is dropped the symptom is silent: the line is drawn, only its color
    // is wrong, and no counter moves.
    let r = renderer();
    const EDGE: usize = 64;
    let (cw, ch) = fitting_cell_px(&r, EDGE, 2);

    let mut frame = Frame::default();
    frame.clear(grid(cw, ch), CaretStyle::default());
    // The left cell is the control: the same line, **no** SGR 58 → foreground color.
    frame.push(rule_cell(0, UnderlineStyle::Single));
    frame.push(Cell {
        underline_color: Some(LinearRgba::from_srgb(0xff, 0x00, 0x00)),
        ..rule_cell(1, UnderlineStyle::Single)
    });

    let pixels = render_offscreen(&r, EDGE, BACKGROUND, &frame);
    let plain = cell_rows(&pixels, EDGE, (cw, ch), 0).concat();
    let colored = cell_rows(&pixels, EDGE, (cw, ch), 1).concat();

    // A fully covered row yields the foreground exactly: coverage 1 → the blend writes the
    // source as is.
    let fg = (0xff, 0xff, 0xff);
    assert!(
        plain.contains(&fg),
        "the rule without SGR 58 was not drawn in the foreground color: {plain:?}"
    );
    assert!(
        !colored.contains(&fg),
        "the pixels of the rule with SGR 58 carry the foreground color: {colored:?}"
    );
    // The direction must be asked too: "different" alone is also true of a rule that was never
    // drawn. A red-dominant pixel says the color came from `underline_color`.
    //
    // The comparison is in `u16`: with `u8`, a light clear color (or a green/blue rule) would
    // overflow at `+ 64` and the test would die with "attempt to add with overflow" instead of
    // an assert pointing at the wrong pixel — `make check` runs the tests in debug.
    assert!(
        colored
            .iter()
            .any(|&(red, green, blue)| { u16::from(red) > u16::from(green.max(blue)) + 64 }),
        "no red-dominant pixel in the rule with SGR 58: {colored:?}"
    );
}

#[test]
fn bold_and_regular_draw_differently() {
    // The `(bold, italic)` → `Face` translation is `bt-gpu`'s single place and no counter can
    // see a state that silently returns `Face::Regular`: `glyphs=G` is the same, `bt-core`'s flag
    // is the same, the atlas still hands out the slot. The same character yielding two
    // different pixel sets in two faces is the only proof.
    //
    // This test relies on the font **carrying a bold face**. If it does not, `Faces::effective`
    // collapses to the regular face, the two cells are drawn identically and the test fails red
    // — it does not give a false green.
    let r = renderer();
    const EDGE: usize = 64;
    let (cw, ch) = fitting_cell_px(&r, EDGE, 2);

    let mut frame = Frame::default();
    frame.clear(grid(cw, ch), CaretStyle::default());
    for (col, bold) in [(0u16, false), (1, true)] {
        frame.push(Cell {
            col,
            row: 0,
            ch: Some('M'),
            fg: WHITE,
            bold,
            ..Default::default()
        });
    }

    let pixels = render_offscreen(&r, EDGE, BACKGROUND, &frame);
    let plain = cell_rows(&pixels, EDGE, (cw, ch), 0).concat();
    let bold = cell_rows(&pixels, EDGE, (cw, ch), 1).concat();
    assert_ne!(
        plain, bold,
        "the bold `M` was drawn the same as the regular `M`"
    );
}

/// A visible cursor; the text color under the block is stated at the call because each test
/// picks it for a separate claim.
pub(crate) fn cursor_at(col: u16, text: LinearRgba) -> Cursor {
    Cursor {
        next_tick: None,
        col,
        row: 0,
        visible: true,
        // These tests ask about the pixel; the handover is `link`'s question.
        caret_in_dock: false,
        input_rows: 1,
        band_hidden: false,
        shape: CaretShape::Block,
        blink: false,
        text,
        // The scroll decision is motion's job (`motion.rs`); here the drawn pixel is queried and
        // the position is already the target itself through `push_settled`.
        display_offset: 0,
        // The offset is `set_origin_rows`'s job and these two fields are its input; the tests
        // that concern the origin (`content_sticks_to_the_bottom_*`) state it directly. A full
        // grid, i.e. zero offset.
        content_rows: 1,
        // Drawing the fill is another test's job; these tests do not consume it yet.
        fill: 0,
        // Fractional scrolling does not concern this list either: on a whole row, no top
        // row.
        top_row: 0,
        scrolled: 0,
        scroll_frac: 0.0,
        scroll_generation: 0,
        rows: 1,
        // No scrollback: the scroll bar is not drawn and these tests ask about cells.
        history: 0,
        resting_fill: 0,
    }
}

/// Draws the cursor in its **own** cell: all these tests look at the settled block, not at an
/// intermediate position (that one is tested in `frame.rs`).
pub(crate) fn push_settled(frame: &mut Frame, cursor: Cursor, rgba: LinearRgba) {
    if cursor.visible {
        frame.push_caret(
            [f32::from(cursor.col), f32::from(cursor.row)],
            cursor.text,
            rgba,
            1.0,
            cursor.shape,
            true,
        );
    }
}

#[test]
fn glyph_under_the_cursor_takes_the_cursor_text_color() {
    // The claim descending from `bt-core` (`char_under_cursor_is_drawn_inverted`), now on the
    // pixels: the letter under the block is drawn with `Cursor::text`, not with its own
    // foreground.
    //
    // The criterion is **equality**, not "different": the cursor cell (A) must be **bit for
    // bit** identical to the same letter drawn in the text color on top of a block-colored
    // background (B). Both pass through the same two passes with the same parameters, so
    // equality is a legitimate demand — and it also covers the edge pixels where coverage is
    // partial: had the alpha path been overwritten (RGBA written instead of RGB), the edges
    // would diverge.
    //
    // Arm C is the negative control: the same letter without the cursor, drawn with its own
    // foreground. Had A == C, the "it is overridden" claim would be empty.
    let r = renderer();
    const EDGE: usize = 64;
    let (cw, ch) = fitting_cell_px(&r, EDGE, 3);

    let mut frame = Frame::default();
    frame.clear(grid(cw, ch), CaretStyle::default());
    // A: under the cursor, the letter's own foreground is white.
    frame.push(glyph_cell(0, 'M', None));
    // B: without the cursor but the letter is already in the text color, its background the
    // block color.
    frame.push(Cell {
        fg: BACKGROUND,
        ..glyph_cell(1, 'M', Some(ACCENT))
    });
    // C: without the cursor, with its own foreground, on top of the same block-colored ground.
    frame.push(glyph_cell(2, 'M', Some(ACCENT)));
    push_settled(&mut frame, cursor_at(0, BACKGROUND), ACCENT);

    let pixels = render_offscreen(&r, EDGE, BACKGROUND, &frame);
    // **The claim was split in two, not turned into a tolerance**: the caret's
    // corner is now round, so the corner pixels are not the block's color and an equality that
    // passed through there would test the roundness. On the body the equality is still **bit
    // for bit**; the corner and the glow have their own separate guards
    // ([`the_caret_corner_is_rounded`], [`the_caret_glow_spills_but_stops`]).
    let inset = caret_radius_px((cw, ch), bt_core::CURSOR_RADIUS as f32);
    let cell = |col| cell_body(&pixels, EDGE, (cw, ch), col, inset);
    let (a, b, c) = (cell(0), cell(1), cell(2));

    assert_eq!(
        a, b,
        "the letter under the cursor was not drawn in the text color (A ≠ B)"
    );
    assert_ne!(
        a, c,
        "the cursor rectangle never overrode the letter's color (A = C)"
    );
}

#[test]
fn a_degenerate_caret_shape_paints_the_old_rectangle() {
    // **The rollback path's guard**: "radius 0, glow 0" must be a supported and tested
    // state, i.e. its output is bit for bit identical to the old plain rectangle. Only the GPU
    // can say so — in the degenerate arm the fragment uses `step`, in the open arm
    // `smoothstep`, and their edge pixels diverge. If `smoothstep` leaks into that arm, this
    // goes red.
    let r = renderer();
    const EDGE: usize = 64;
    let (cw, ch) = fitting_cell_px(&r, EDGE, 2);

    let mut frame = Frame::default();
    // **The degenerate arm is now driven by the setting**: `cursor_radius = 0` and
    // `cursor_glow = 0` are a supported user setting, i.e. the rollback path is not a test hook
    // but a **real path**.
    frame.clear(
        grid(cw, ch),
        CaretStyle {
            radius_ratio: 0.0,
            glow: 0.0,
            ..CaretStyle::default()
        },
    );
    // A: the caret, with the degenerate shape. B: a plain background of the same color — i.e.
    // the state before the caret's own pipeline.
    push_settled(&mut frame, cursor_at(0, BACKGROUND), ACCENT);
    frame.push(bg_cell(1, 0, ACCENT));

    let pixels = render_offscreen(&r, EDGE, BACKGROUND, &frame);
    let cell = |col| cell_rows(&pixels, EDGE, (cw, ch), col).concat();
    assert_eq!(
        cell(0),
        cell(1),
        "the degenerate caret diverged from the plain rectangle: the rollback path is broken"
    );
}

#[test]
fn the_caret_corner_is_rounded() {
    // The radius's own guard. `glyph_under_the_cursor_...` deliberately leaves the corners out
    // (middle band); this is the only place that says the roundness **really** exists.
    //
    // **The reference is from OUTSIDE the caret** (found in code review): the earlier version made
    // the comparison with the caret's own opposite corner and, since the SDF is symmetric, the
    // `d` of the two corners is always equal — the claim was a tautology that could not fail at
    // any radius value. The measure is also no longer "equal/different" but the **paint
    // ratio**: the corner must be markedly dimmer than the center.
    let r = renderer();
    const EDGE: usize = 64;
    let (cw, ch) = fitting_cell_px(&r, EDGE, 1);

    let mut frame = Frame::default();
    // **The radius is driven by the setting**, without overriding: `cursor_radius` is
    // now a user key and the guard must pass through the real path. The ratio is given
    // **explicitly**, not from the production default — the default is a matter of taste and
    // corresponds to ~1.6 px on a 1x cell, i.e. most of the corner pixel would still be
    // painted and the threshold would turn red depending on the cell height.
    frame.clear(
        grid(cw, ch),
        CaretStyle {
            // Half of the narrow edge: the block turns into a stadium.
            radius_ratio: 0.5,
            glow: 0.0,
            ..CaretStyle::default()
        },
    );
    push_settled(&mut frame, cursor_at(0, BACKGROUND), ACCENT);
    let pixels = render_offscreen(&r, EDGE, BACKGROUND, &frame);

    let sum = |x, y| brightness(&pixels, EDGE, x, y);
    let clear = sum(EDGE - 1, EDGE - 1);
    let middle = sum(usize::from(cw) / 2, usize::from(ch) / 2);
    assert!(middle > clear, "the caret's middle is not painted");
    assert_eq!(
        sum(0, 0),
        clear,
        "the corner is painted: the shader does not round"
    );
}

#[test]
fn a_hollow_caret_paints_only_its_edge() {
    // **The edge arm must not ship dead** (found in code review). `caret_shape()` gives `stroke` a
    // constant 0 in this version, so the shader's `stroke > 0` branch would never have run and
    // a later change would open it believing it "already written and passing". The test drives that
    // branch **now**.
    //
    // The second job: the `body -= inner` subtraction can zero the body entirely on a thick
    // edge. Here the edge is a quarter of the cell, so the middle must really stay empty but
    // the caret must not become invisible.
    let r = renderer();
    const EDGE: usize = 64;
    let (cw, ch) = fitting_cell_px(&r, EDGE, 1);

    let mut frame = Frame::default();
    frame.clear(grid(cw, ch), CaretStyle::default());
    push_settled(&mut frame, cursor_at(0, BACKGROUND), ACCENT);
    // Radius and glow are off; the only thing under test is the edge band.
    let stroke = (f32::from(cw) / 4.0).max(1.0);
    frame.force_caret_sdf([0.0, stroke, 0.0, 0.0]);
    let pixels = render_offscreen(&r, EDGE, BACKGROUND, &frame);

    let sum = |x, y| brightness(&pixels, EDGE, x, y);
    let clear = sum(EDGE - 1, EDGE - 1);
    let edge = sum(0, usize::from(ch) / 2);
    let middle = sum(usize::from(cw) / 2, usize::from(ch) / 2);
    assert!(edge > clear, "the hollow caret's edge was not drawn either");
    assert_eq!(middle, clear, "the hollow caret's middle is painted");
}

#[test]
fn the_caret_glow_spills_but_stops() {
    // **The glow is sampled OUTSIDE the rectangle**: a test that looks from the inside
    // cannot see the glow, because the body is already opaque there.
    let r = renderer();
    const EDGE: usize = 64;
    // The gutter in the test is **wider than production's** (default ~8): as the glow fades
    // (0.35 → 0.10) the difference drowns in quantization on an 8-bit target and the guard goes
    // blind. A wide gutter brings the sampled point closer to the top of the glow; what is
    // tested is the ratio, not the absolute pixel.
    const GUTTER: u16 = 16;
    let (cw, ch) = fitting_cell_px(&r, EDGE, 1);

    let mut frame = Frame::default();
    frame.clear(grid_with_gutter(cw, ch, GUTTER), CaretStyle::default());
    push_settled(&mut frame, cursor_at(0, BACKGROUND), ACCENT);
    let pixels = render_offscreen(&r, EDGE, BACKGROUND, &frame);

    // The caret starts to the right of the gutter: x ∈ [GUTTER, GUTTER + cw].
    let right = usize::from(GUTTER) + usize::from(cw);
    // The pad is **the same** as production's derivation: when the ratio changes the guard
    // shifts with it, otherwise when the glow shrank the test would look at an empty point.
    let pad = (f32::from(GUTTER) * crate::frame::CARET_GLOW_RATIO) as usize;
    let y = usize::from(ch) / 2;
    assert!(
        right + pad + 2 < EDGE,
        "the sample points do not fit the texture"
    );

    // The reference is **from afar**: a corner the glow cannot reach. Taking the point right
    // beyond the boundary as the reference would be circular — that point is itself what is
    // being tested.
    let clear = pixel_at(&pixels, EDGE, EDGE - 1, EDGE - 1);
    let inside_glow = pixel_at(&pixels, EDGE, right + pad / 2, y);
    assert_ne!(inside_glow, clear, "there is no glow outside the rectangle");
    assert_eq!(
        pixel_at(&pixels, EDGE, right + pad + 2, y),
        clear,
        "the glow paints beyond the pad too: unbounded"
    );
}

#[test]
fn the_glow_setting_reaches_the_pixels() {
    // **The only proof that the setting really lands.** `cursor_glow` scales both the pad and
    // the alpha (one feel, not two numbers), so turning it off must return the outside of the
    // rectangle to the ground and turning it up must brighten it. The `Settings` tests show the
    // value is **read**; only this one shows it is painted.
    let r = renderer();
    const EDGE: usize = 64;
    const GUTTER: u16 = 16;
    let (cw, ch) = fitting_cell_px(&r, EDGE, 1);
    let y = usize::from(ch) / 2;
    let at = usize::from(GUTTER) + usize::from(cw) + 2;
    assert!(at < EDGE, "the sample point does not fit the texture");

    let sample = |glow: f64| {
        let mut frame = Frame::default();
        frame.clear(
            grid_with_gutter(cw, ch, GUTTER),
            CaretStyle {
                radius_ratio: 0.0,
                glow,
                ..CaretStyle::default()
            },
        );
        push_settled(&mut frame, cursor_at(0, BACKGROUND), ACCENT);
        let pixels = render_offscreen(&r, EDGE, BACKGROUND, &frame);
        brightness(&pixels, EDGE, at, y)
    };

    let (off, on, strong) = (sample(0.0), sample(1.0), sample(2.0));
    assert!(
        off < on && on < strong,
        "the glow setting does not reach the pixels: {off} / {on} / {strong}"
    );
}

#[test]
fn the_caret_glow_fades_with_the_caret() {
    // The glow is **multiplied** by the caret's own alpha, so as the blink fades out the glow
    // fades too. The guard again samples **from outside**:
    // `cursor_alpha_is_blended_on_the_gpu` looks only at the caret's own cell and cannot see
    // this symptom.
    let r = renderer();
    const EDGE: usize = 64;
    // A wide gutter: same reasoning as the sibling guard (quantization).
    const GUTTER: u16 = 16;
    let (cw, ch) = fitting_cell_px(&r, EDGE, 1);
    let y = usize::from(ch) / 2;
    let pad = (f32::from(GUTTER) * crate::frame::CARET_GLOW_RATIO) as usize;
    let at = usize::from(GUTTER) + usize::from(cw) + pad / 2;
    // `pixel_at` does not bound x: an overflowing index does not panic but reads a pixel of
    // **the next row down**, i.e. the test silently makes a wrong claim (found in code review).
    // `fitting_cell_px` never sees the gutter.
    assert!(at < EDGE, "the sample point does not fit the texture");

    let sample = |alpha: f32| {
        let mut frame = Frame::default();
        frame.clear(grid_with_gutter(cw, ch, GUTTER), CaretStyle::default());
        frame.push_caret(
            [0.0, 0.0],
            BACKGROUND,
            ACCENT,
            alpha,
            CaretShape::Block,
            true,
        );
        let pixels = render_offscreen(&r, EDGE, BACKGROUND, &frame);
        brightness(&pixels, EDGE, at, y)
    };

    // A faded-out caret is not drawn at all, so the clear color remains at that point; the
    // ordering is exact at all three ends and needs no color table.
    let (dark, half, full) = (sample(0.0), sample(0.5), sample(1.0));
    assert!(
        dark < half && half < full,
        "the glow does not follow the caret's alpha: {dark} / {half} / {full}"
    );
}
#[test]
fn a_hollow_caret_leaves_the_glyph_its_own_color() {
    // **The unfocused sibling of `glyph_under_the_cursor_takes_the_cursor_text_color`, and it says
    // the opposite.** Inversion depends on a painted ground: in the middle of a hollow caret nothing
    // is painted, so the letter must keep its own foreground. If it did not, it would be drawn in
    // the ground colour and become **invisible** — a hollow caret would swallow the text.
    //
    // Sampling is the cell's **interior**: the edge band (`rule_px`) is the caret itself and
    // equality is not expected there.
    let r = renderer();
    const EDGE: usize = 64;
    let (cw, ch) = fitting_cell_px(&r, EDGE, 2);

    let mut frame = Frame::default();
    frame.clear(grid(cw, ch), CaretStyle::default());
    // A: under the unfocused caret. C: no caret, same letter and ground.
    frame.push(glyph_cell(0, 'M', None));
    frame.push(glyph_cell(1, 'M', None));
    frame.push_caret(
        [0.0, 0.0],
        BACKGROUND,
        ACCENT,
        1.0,
        CaretShape::Block,
        false,
    );

    let pixels = render_offscreen(&r, EDGE, BACKGROUND, &frame);
    // **The inset is derived from production**, not a constant (found in code review): the ring's
    // thickness is `rule_px` and the radius eats the corner; a fixed 3 would either pull the ring
    // into the sample on a large point size or, in a narrow cell, empty the range and fall into an
    // equality that claims nothing.
    let inset = caret_radius_px((cw, ch), bt_core::CURSOR_RADIUS as f32)
        + usize::from(r.cell_metrics(1.0).rule_px()).max(1);
    assert!(
        inset * 2 < usize::from(cw).min(usize::from(ch)),
        "the inset swallowed the cell"
    );
    let interior = |col: usize| {
        let (cwu, chu) = (usize::from(cw), usize::from(ch));
        (inset..chu - inset)
            .flat_map(|y| (inset..cwu - inset).map(move |x| (x, y)))
            .map(|(x, y)| pixel_at(&pixels, EDGE, col * cwu + x, y))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        interior(0),
        interior(1),
        "the hollow caret overrode the letter's colour: the text would become invisible"
    );
}

#[test]
fn an_unfocused_caret_paints_a_ring_through_the_production_path() {
    // The ring's guard from the **production path**: `a_hollow_caret_paints_only_
    // its_edge` drives the shader's arm with `force_caret_sdf`, whereas here what turns the edge on
    // is the focus itself (`push_caret(.., focused=false)`). Without both, "the arm works but focus
    // never turns it on" would go unnoticed.
    let r = renderer();
    const EDGE: usize = 64;
    let (cw, ch) = fitting_cell_px(&r, EDGE, 1);

    let mut frame = Frame::default();
    frame.clear(grid(cw, ch), CaretStyle::default());
    frame.push_caret(
        [0.0, 0.0],
        BACKGROUND,
        ACCENT,
        1.0,
        CaretShape::Block,
        false,
    );
    let pixels = render_offscreen(&r, EDGE, BACKGROUND, &frame);

    let sum = |x, y| brightness(&pixels, EDGE, x, y);
    let clear = sum(EDGE - 1, EDGE - 1);
    assert!(
        sum(0, usize::from(ch) / 2) > clear,
        "the unfocused caret's edge was not drawn"
    );
    assert_eq!(
        sum(usize::from(cw) / 2, usize::from(ch) / 2),
        clear,
        "the unfocused caret's middle is filled"
    );
}

#[test]
fn cursor_alpha_is_blended_on_the_gpu() {
    // Reduce Motion's fade-in is blended **on the GPU**: the `cell_bg` pipeline became
    // blended for it and the `cell` fragment does a `mix` instead of an overwrite. The
    // counter in `frame.rs` shows the alpha was written to the list but cannot show it was painted
    // — exactly the repo rule's counterpart ("a CPU counter does not prove what the GPU painted").
    //
    // The criterion is **equality** at the two ends and **order** in the middle: computing the
    // expected colour would mean encoding a linear blend to sRGB, and that table is not here. The
    // ends are a sharper claim anyway — had alpha never been read, all three frames would have come
    // out the same.
    let r = renderer();
    const EDGE: usize = 64;
    let (cw, ch) = fitting_cell_px(&r, EDGE, 1);

    // One cell, one caret: the uniform is **one** value per frame, so three opacities mean three
    // separate frames.
    let render = |alpha: Option<f32>| {
        let mut frame = Frame::default();
        frame.clear(grid(cw, ch), CaretStyle::default());
        frame.push(glyph_cell(0, 'M', None));
        if let Some(alpha) = alpha {
            frame.push_caret(
                [0.0, 0.0],
                BACKGROUND,
                ACCENT,
                alpha,
                CaretShape::Block,
                true,
            );
        }
        cell_rows(
            &render_offscreen(&r, EDGE, BACKGROUND, &frame),
            EDGE,
            (cw, ch),
            0,
        )
        .concat()
    };
    let (none, clear, half, opaque) = (
        render(None),
        render(Some(0.0)),
        render(Some(0.5)),
        render(Some(1.0)),
    );

    // Alpha zero = **no caret at all**: both the block and the letter under it must stay untouched.
    // With blending off, this would have been an opaque rectangle.
    assert_eq!(clear, none, "alpha 0 still drew the caret opaquely");
    // Alpha one = the state from **before** the fade-in: the visual result did not change for a
    // settled caret.
    assert_ne!(opaque, none, "alpha 1 never drew the caret");

    let mut strictly_between = 0usize;
    for (i, (&mid, (&off, &on))) in half.iter().zip(clear.iter().zip(opaque.iter())).enumerate() {
        for c in 0..3 {
            let (m, a, b) = (
                [mid.0, mid.1, mid.2][c],
                [off.0, off.1, off.2][c],
                [on.0, on.1, on.2][c],
            );
            let (lo, hi) = (a.min(b), a.max(b));
            assert!(
                (lo..=hi).contains(&m),
                "pixel {i} component {c}: {m} is outside the two ends ({lo}, {hi})"
            );
            if m > lo && m < hi {
                strictly_between += 1;
            }
        }
    }
    // The middle frame must **not equal** either end: if it did, alpha would behave like a binary
    // flag and not really blend.
    assert!(
        strictly_between > 0,
        "alpha 0.5 fell to one of the two ends: no blending"
    );
}

#[test]
fn cursor_rect_stops_at_its_own_cell() {
    // The rectangle's **outside** must stay untouched: the overwrite is bounded by the cell itself,
    // not the frame. The defect it catches is the rectangle's **size** — the frame instead of the
    // cell, or two cells instead of one. What it does not catch is the `<` versus `<=` difference:
    // `[[position]]` gives the fragment centre (x + 0.5) and no fragment lands exactly on the
    // boundary (written in the shader).
    //
    // **The in-between position is asked too** (`/audit` finding): since motion arrived `at` can be
    // fractional, and the arm left as "asked on its own" in fact never ran — both offscreen tests
    // gave integer positions, so only the CPU unit tests saw a fractional rectangle.
    let r = renderer();
    const EDGE: usize = 64;
    let (cw, ch) = fitting_cell_px(&r, EDGE, 2);

    // What asks about the boundary is the **rule**, not a glyph: the band also covers the cell's
    // first column fully, so the question "was that column overwritten" does not depend on which
    // pixel the font painted (a glyph's first column can be empty and the test would silently ask
    // nothing).
    let mut frame = Frame::default();
    frame.clear(grid(cw, ch), CaretStyle::default());
    frame.push(rule_cell(0, UnderlineStyle::Single));
    frame.push(rule_cell(1, UnderlineStyle::Single));
    push_settled(&mut frame, cursor_at(0, BACKGROUND), ACCENT);

    let pixels = render_offscreen(&r, EDGE, BACKGROUND, &frame);
    // The neighbour's **first column**: the rectangle's `x1` falls exactly there.
    let first_column: Vec<(u8, u8, u8)> = cell_rows(&pixels, EDGE, (cw, ch), 1)
        .iter()
        .map(|row| row[0])
        .collect();
    assert!(
        first_column.contains(&(0xff, 0xff, 0xff)),
        "the neighbour's first column was overwritten too: the rectangle overflows its cell — {first_column:?}"
    );

    // A caret sitting **halfway between** two cells: the rectangle now spills into both cells and
    // that is right — but its width is still one cell, so the second cell's **last** column must
    // stay untouched. A defect where the size is the frame or two cells is caught here too, and at
    // a fractional position at that.
    let mut frame = Frame::default();
    frame.clear(grid(cw, ch), CaretStyle::default());
    frame.push(rule_cell(0, UnderlineStyle::Single));
    frame.push(rule_cell(1, UnderlineStyle::Single));
    let mut cursor = cursor_at(0, BACKGROUND);
    cursor.col = 0;
    frame.push_caret([0.5, 0.0], cursor.text, ACCENT, 1.0, cursor.shape, true);

    let pixels = render_offscreen(&r, EDGE, BACKGROUND, &frame);
    let last_column: Vec<(u8, u8, u8)> = cell_rows(&pixels, EDGE, (cw, ch), 1)
        .iter()
        .map(|row| row[usize::from(cw) - 1])
        .collect();
    assert!(
        last_column.contains(&(0xff, 0xff, 0xff)),
        "a caret shifted by half a cell overwrote both cells at once: {last_column:?}"
    );
}

#[test]
fn rule_under_the_cursor_takes_the_cursor_text_color() {
    // The second claim that comes down from `bt-core` (`cursor_cell_drops_the_underline_color`),
    // through pixels. `frame()` used to **drop** the cursor cell's SGR 58 colour; now the cell keeps
    // its colour and the rectangle overwrites it as pixels. The result is the same: the line under
    // the block also returns to the text colour, so an underline and a strikeout in the same cell
    // behave the same.
    let r = renderer();
    const EDGE: usize = 64;
    let (cw, ch) = fitting_cell_px(&r, EDGE, 2);

    let red = LinearRgba::from_srgb(0xff, 0x00, 0x00);
    let mut frame = Frame::default();
    frame.clear(grid(cw, ch), CaretStyle::default());
    // The same SGR 58 line in two cells; the caret in only one.
    for col in [0, 1] {
        frame.push(Cell {
            underline_color: Some(red),
            ..rule_cell(col, UnderlineStyle::Single)
        });
    }
    push_settled(&mut frame, cursor_at(0, BACKGROUND), ACCENT);

    let pixels = render_offscreen(&r, EDGE, BACKGROUND, &frame);
    let under = cell_rows(&pixels, EDGE, (cw, ch), 0).concat();
    let plain = cell_rows(&pixels, EDGE, (cw, ch), 1).concat();

    let srgb = |hex: u32| ((hex >> 16) as u8, (hex >> 8) as u8, hex as u8);
    let background = srgb(Theme::BATERI.background);
    // Control: in the caretless cell the line is still SGR 58's red.
    assert!(
        plain.contains(&(0xff, 0x00, 0x00)),
        "the SGR 58 colour vanished in the caretless cell: {plain:?}"
    );
    assert!(
        !under.contains(&(0xff, 0x00, 0x00)),
        "the line under the block kept the SGR 58 colour: {under:?}"
    );
    // The direction must be asked too: "no red" alone also holds for a rule that was never drawn.
    // The fully covering band yields the text colour — ±1 tolerance, because the ground is a
    // **midtone** and 8-bit sRGB encoding carries rounding (precedent
    // `cell_bg_paints_pixels_on_the_gpu`).
    assert!(
        under.iter().any(|p| {
            p.0.abs_diff(background.0) <= 1
                && p.1.abs_diff(background.1) <= 1
                && p.2.abs_diff(background.2) <= 1
        }),
        "the line under the block was not drawn in the text colour: {under:?}"
    );
}

#[test]
fn rule_over_cursor_stays_visible() {
    // Draw order: backgrounds **and caret**, then glyphs, then rules. The caret block is opaque and
    // covers everything under it; if the rule does not come after it, the underline disappears in
    // the cell above the caret and the symptom shows only in the single cell where the caret stands.
    let r = renderer();
    const EDGE: usize = 64;
    let (cw, ch) = fitting_cell_px(&r, EDGE, 1);

    let red = LinearRgba::from_srgb(0xff, 0x00, 0x00);
    let mut frame = Frame::default();
    frame.clear(grid(cw, ch), CaretStyle::default());
    frame.push(rule_cell(0, UnderlineStyle::Single));
    // The text colour is deliberately the **same** as the rule's own (white): what this test asks
    // is **order**, not colour, and the rectangle's overwrite must not muddy it. The place that asks
    // whether the colour was overwritten is `rule_under_the_cursor_takes_the_cursor_text_color`.
    push_settled(&mut frame, cursor_at(0, WHITE), red);

    let pixels = render_offscreen(&r, EDGE, BACKGROUND, &frame);
    let cell = cell_rows(&pixels, EDGE, (cw, ch), 0).concat();
    assert!(
        cell.contains(&(0xff, 0xff, 0xff)),
        "the rule over the caret was covered: {cell:?}"
    );
    // The second claim is the guard's other half: the rule must not paint the caret entirely,
    // otherwise the "visible" claim would hold for code that erased the caret too.
    assert!(
        cell.contains(&(0xff, 0x00, 0x00)),
        "the rule covered the whole caret block: {cell:?}"
    );
}

#[test]
fn renderer_without_atlas_refuses_glyphs() {
    // The "ask for the metrics first" contract, testable: the renderer's
    // atlas is born `None`; a path drawing glyphs without ever saying the
    // scale must drop the frame rather than draw silently @1x. Silent, the
    // symptom would be "half-size letters on a retina Mac" and no test
    // would see it.
    let r = renderer();
    const EDGE: u32 = 32;
    let mut frame = Frame::default();
    frame.clear(grid(8, 16), CaretStyle::default());
    frame.push(Cell {
        col: 0,
        row: 0,
        ch: Some('x'),
        fg: ACCENT,
        bg: None,
        ..Default::default()
    });
    let result = r.try_submit_offscreen(EDGE, BACKGROUND, &frame);
    assert!(
        matches!(result, Err(GpuError::NoAtlas)),
        "a frame without an atlas went through silently: {result:?}"
    );
}

/// A [`SlotUpload`] that only records which planes were written: the
/// list-building guards below ask the shared `slots::glyph_lists` (the
/// Metal `AtlasTexture` they used to build by hand went away with
/// Metal), so no texture is involved.
#[derive(Default)]
struct RecordingUpload {
    planes: Vec<Plane>,
}

impl SlotUpload for RecordingUpload {
    fn upload(&mut self, plane: Plane, _: (u16, u16), metrics: Metrics, bytes: &[u8]) {
        assert_eq!(
            bytes.len(),
            slots::slot_layout(metrics, plane).0,
            "a full slot"
        );
        self.planes.push(plane);
    }
}

/// An atlas and the two lists `slots::glyph_lists` fills — what the
/// renderers' `prepare` produce, minus the texture.
struct Lists {
    atlas: Atlas,
    upload: RecordingUpload,
    mask: Vec<GlyphInstance>,
    color: Vec<GlyphInstance>,
}

impl Lists {
    fn new(scale: f64) -> Self {
        Self {
            atlas: Atlas::new(None, 13.0, scale, bt_atlas::Spacing::default()),
            upload: RecordingUpload::default(),
            mask: Vec::new(),
            color: Vec::new(),
        }
    }

    fn prepare(&mut self, glyphs: &[GlyphCell], clusters: &Clusters) {
        slots::glyph_lists(
            &mut self.atlas,
            &mut self.upload,
            glyphs,
            clusters,
            &[],
            &mut self.mask,
            &mut self.color,
        );
    }
}

/// A wide cell yields **two quads**: the left half in place, the right half one cell to the right
/// and from two separate slots.
///
/// The fan-out is **not** in `Frame::push` but here, and the reason is borrowing: "one slot or
/// two" is decided by the ink gate, that is `Atlas::slot` — while the sink never sees the atlas.
/// The guard shows that decision actually reaches the drawing.
#[test]
fn a_wide_cell_becomes_two_quads() {
    let mut tex = Lists::new(1.0);
    let cell_w = tex.atlas.metrics().cell_px.0;
    // The fixture's pair character: its ink yields a candidate that wants two cells; it is rejected by
    // the one-cell gate and passes the two-cell gate.
    let glyphs = [GlyphCell {
        pos: [0.0, 0.0],
        ch: PAIR_CHAR,
        face: Face::Regular,
        size: SizeClass::Normal,
        rgba: [1.0, 1.0, 1.0, 1.0],
        wide: true,
        cluster: None,
    }];
    tex.prepare(&glyphs, &Clusters::default());
    assert_eq!(
        tex.mask.len(),
        2,
        "a wide cell must yield two quads: {:?}",
        tex.mask.len()
    );
    assert_eq!(tex.mask[0].pos, [0.0, 0.0], "left half in the cell's place");
    assert_eq!(
        tex.mask[1].pos,
        [f32::from(cell_w), 0.0],
        "right half exactly one cell to the right"
    );
    assert_ne!(
        tex.mask[0].uv0, tex.mask[1].uv0,
        "the two halves must be read from two separate slots"
    );
}

/// A character declared wide but whose ink fits one cell yields **one** quad.
///
/// An empty quad dropped to its right would be a wasted draw and would be paid on each of the 65
/// measured characters ("drawings that work today"). The owner of the decision is the gate, not
/// the caller — and the guard shows exactly that.
#[test]
fn a_wide_cell_that_fits_one_cell_stays_one_quad() {
    let mut tex = Lists::new(1.0);
    // The base font's own glyph, two columns according to Unicode: in the base font the advance is
    // the cell's advance itself, that is one cell (the fixture's `ONE_CELL_WIDE_CHAR`).
    let glyphs = [GlyphCell {
        pos: [0.0, 0.0],
        ch: ONE_CELL_WIDE_CHAR,
        face: Face::Regular,
        size: SizeClass::Normal,
        rgba: [1.0, 1.0, 1.0, 1.0],
        wide: true,
        cluster: None,
    }];
    tex.prepare(&glyphs, &Clusters::default());
    assert_eq!(
        tex.mask.len(),
        1,
        "a wide character that fits one cell must not produce a second quad"
    );
}

/// The colour texture's format is **`RGBA8Unorm_sRGB`** and this is a contract, not a taste.
///
/// A plain `RGBA8Unorm` texture would be silently wrong: the hardware does **not** decode sRGB
/// when sampling, the fragment treats the values as linear and the target (`BGRA8Unorm_sRGB`)
/// encodes once more on write — the palette washes out. The very same silent defect as "the
/// colour space crosses the boundary", and the only place it is asked
/// directly.
#[test]
fn the_color_plane_is_an_srgb_texture() {
    use crate::renderer::{COLOR_FORMAT, MASK_FORMAT, plane_format};
    assert_eq!(
        plane_format(Plane::Color),
        COLOR_FORMAT,
        "the colour plane's texture is not the colour format"
    );
    assert_eq!(
        COLOR_FORMAT,
        wgpu::TextureFormat::Rgba8UnormSrgb,
        "the colour plane must be sRGB"
    );
    // The mask plane **did not change**: the one-channel coverage contract
    // stands.
    assert_eq!(plane_format(Plane::Mask), MASK_FORMAT);
    assert_eq!(MASK_FORMAT, wgpu::TextureFormat::R8Unorm);
}

/// A cluster (`🇹🇷`) landing as **one** colour glyph on all three surfaces: the grid,
/// the fill band and the dock carry the cell with their own table, `prepare` asks the atlas for
/// it as `Sprite::Cluster`, and a wide glyph puts two quads from the colour plane — not the box
/// slot, not two RIs. On a frame where the lists are kept (motion) the second `prepare` gives
/// the same slots: the table lives with the lists.
#[test]
fn a_cluster_is_one_color_glyph_on_every_surface() {
    // Retina: at 13pt@1x the flag's ink exceeds two cells and the
    // cluster falls back to its base character (the scale of `bt-atlas`'s
    // cluster tests).
    let mut tex = Lists::new(2.0);
    let mut frame = Frame::default();
    frame.clear(grid(16, 32), CaretStyle::default());
    let mut clusters = frame.take_clusters();
    let cell = Cell {
        ch: Some('🇹'),
        wide: true,
        cluster: clusters.push("🇹🇷"),
        ..Default::default()
    };
    frame.put_clusters(clusters);
    frame.push(cell);
    frame.set_fill_rows(1);
    frame.push_fill(cell);
    let mut dock = frame.take_dock_clusters();
    let dock_cell = Cell {
        cluster: dock.push("🇹🇷"),
        ..cell
    };
    frame.put_dock_clusters(dock);
    frame.push_dock(dock_cell);

    let surfaces = [
        ("grid", frame.glyphs(), frame.clusters()),
        ("band", frame.fill_glyphs(), frame.clusters()),
        ("dock", frame.dock_glyphs(), frame.dock_clusters()),
    ];
    let mut first = None;
    for (name, glyphs, clusters) in surfaces {
        for pass in ["content", "motion"] {
            tex.prepare(glyphs, clusters);
            // Without a colour flag font the test has no subject; precedent `🎉`.
            if tex.color.is_empty() && tex.mask.is_empty() {
                return;
            }
            assert_eq!(
                tex.color.len(),
                2,
                "{name}/{pass}: the flag must be two quads from the colour plane"
            );
            assert!(
                tex.mask.is_empty(),
                "{name}/{pass}: fell into the mask list (box or a lone RI)"
            );
            let uvs: Vec<[f32; 2]> = tex.color.iter().map(|part| part.uv0).collect();
            assert_eq!(*first.get_or_insert(uvs.clone()), uvs, "{name}/{pass}");
        }
    }
    // A lone `🇹` (the same cell without the cluster) gets **different** slots: the quads above
    // belong to the cluster, not the base character.
    let lone = [GlyphCell {
        cluster: None,
        ..frame.glyphs()[0]
    }];
    tex.prepare(&lone, frame.clusters());
    let lone: Vec<[f32; 2]> = tex
        .color
        .iter()
        .chain(&tex.mask)
        .map(|part| part.uv0)
        .collect();
    assert_ne!(
        first,
        Some(lone),
        "the cluster was drawn from the base character's slot"
    );
}

/// A colour candidate goes to the **colour plane** and never enters the mask list.
///
/// The two lists must stay separate: different pipeline, different texture, different blend.
/// Were they mixed, a single draw call could not ask for both fragments at once and the emoji
/// would be painted in the text's foreground colour.
#[test]
fn a_color_glyph_goes_to_the_color_list() {
    let mut tex = Lists::new(1.0);
    // A code point whose default presentation is emoji; the grid gives it two columns, so `wide`
    // arrives set.
    let glyphs = [GlyphCell {
        pos: [0.0, 0.0],
        ch: '🎉',
        face: Face::Regular,
        size: SizeClass::Normal,
        rgba: [1.0, 1.0, 1.0, 1.0],
        wide: true,
        cluster: None,
    }];
    tex.prepare(&glyphs, &Clusters::default());
    // If no colour font carrying the character is installed the test has no subject — but the
    // escape branch **must see the regression**: if `has_color_glyphs` breaks, `🎉` falls to the
    // mask plane and `color_instances` stays empty anyway. In that case the mask list must carry
    // nothing but the tofu; if it does, a colour glyph was drawn as a mask.
    if tex.color.is_empty() {
        assert_eq!(
            tex.atlas.occupancy().0,
            1,
            "a colour candidate opened a slot in the mask plane: `has_color_glyphs` missed the plane"
        );
        return;
    }
    assert_eq!(tex.color.len(), 2, "a wide emoji must produce two quads");
    assert!(
        tex.mask.is_empty(),
        "a colour candidate entered the mask list: {:?}",
        tex.mask.len()
    );
    // The colour texture is created by the first colour upload
    // (`SlotUpload`'s contract); here that upload is the witness.
    assert!(
        tex.upload.planes.contains(&Plane::Color),
        "the first colour slot was not uploaded to the colour plane"
    );
    assert_eq!(
        tex.atlas.color_occupancy().0,
        2,
        "the colour plane must spend two slots"
    );
    assert_eq!(
        tex.atlas.occupancy().0,
        1,
        "the mask plane must hold only the tofu"
    );
}

/// Rebuilding the atlas **also drops the colour texture**.
///
/// `Atlas::ensure` builds the atlas from scratch: `color_next` is reset and the texture edge can
/// change (the edge derives from `SLOT_TARGET` and the cell size). If a colour texture left at
/// the old edge is written with the new grid's corners, `replaceRegion` overflows **outside** the
/// texture — growing the point size with Cmd+ triggers this path while an emoji is on screen.
/// The same line for the mask texture has existed for longer; this guard keeps the two together.
#[test]
fn rebuilding_the_atlas_drops_both_textures() {
    let r = renderer();
    const EDGE: usize = 32;
    // Run the emoji through the real frame path: the texture is only born with the first colour
    // slot.
    let mut frame = Frame::default();
    let metrics = r.cell_metrics(1.0);
    frame.clear(metrics, CaretStyle::default());
    frame.push(Cell {
        col: 0,
        row: 0,
        ch: Some('🎉'),
        wide: true,
        ..Default::default()
    });
    render_offscreen(&r, EDGE, BACKGROUND, &frame);
    let (mask_before, color_before) = r.plane_textures();
    // Without a colour font installed the texture is never born and the
    // test has no subject.
    if !color_before {
        return;
    }
    assert!(mask_before, "the mask texture must exist too");
    // A size change changes the atlas key, so `ensure` rebuilds.
    assert!(
        r.set_font(&FontOptions {
            size: 31.0,
            ..FontOptions::default()
        }),
        "size changed"
    );
    r.cell_metrics(1.0);
    let (mask, color) = r.plane_textures();
    assert!(!mask, "the mask texture was not dropped");
    assert!(
        !color,
        "the colour texture was not dropped: one left at the old edge would take an \
         out-of-bounds write"
    );
}

// ---- The dock's typing effects: hermetic invariants ----
//
// The correctness of the in-between frames is by eye only; here, for each effect, what is
// tested by loop is the ends and the boundaries: an arrival at `t = 1` is the static glyph
// itself, a ghost at `t = 1` is bare ground, the neighbour slot is never sampled and a wide
// glyph transforms as a single box.

/// `heat`'s hot colour: distinct from both the foreground ([`WHITE`]) and the ground, so
/// `heat`'s in-between frame mixes with neither.
const HEAT: LinearRgba = LinearRgba::from_srgb(0xff, 0x80, 0x20);

/// A one-row frame with a dock: ground [`BACKGROUND`], cells printed to the dock, the effects
/// above them.
fn dock_fx_frame(cell_px: (u16, u16), cells: &[Cell], fx: &[Fx]) -> Frame {
    let mut frame = Frame::default();
    frame.clear(grid(cell_px.0, cell_px.1), CaretStyle::default());
    for &cell in cells {
        frame.push_dock(cell);
    }
    frame.set_dock_fx(fx.iter().copied(), &Clusters::default(), HEAT);
    frame.set_dock_rows(1);
    frame.open_dock(BACKGROUND, BACKGROUND, BACKGROUND);
    frame
}

fn effect(cell: Cell, kind: Kind, effect: u32, t: f32) -> Fx {
    Fx {
        cell,
        kind,
        effect,
        t,
        seed: 0.0,
    }
}

fn wide_cell(col: u16, ch: char) -> Cell {
    Cell {
        wide: true,
        ..glyph_cell(col, ch, None)
    }
}

/// The top of the dock row, in pixels: the dock is pinned to the bottom of the texture.
fn dock_row_top(edge: usize, ch: u16) -> usize {
    edge - usize::from(ch)
}

#[test]
fn an_arrival_at_its_end_is_the_static_glyph_pixel_for_pixel() {
    // On the handover from the last effect frame to the static drawing the letter must not jump:
    // the `t = 1` branch lands on the static path's arithmetic. Three layouts: a single cell, a
    // wide glyph's two halves and the colour plane (emoji; if no colour font is installed it is
    // tofu and the claim stays in the mask plane).
    let r = renderer();
    const EDGE: usize = 64;
    let cell_px = fitting_cell_px(&r, EDGE, 4);
    for &keypress in &Keypress::effects() {
        let id = keypress.id().expect("a drawing effect");
        for cell in [
            glyph_cell(2, 'M', None),
            wide_cell(2, PAIR_CHAR),
            wide_cell(2, '🎉'),
        ] {
            let still = dock_fx_frame(cell_px, &[cell], &[]);
            // The cache is warmed: if no colour font is installed `🎉` falls to tofu and the atlas gives
            // the negative answer as one cell on first ask and two halves from the cache
            // (`bt_atlas::Atlas::slot`) — what is compared is the effect's path, not the atlas's first
            // question.
            render_offscreen(&r, EDGE, BACKGROUND, &still);
            let moving = dock_fx_frame(cell_px, &[cell], &[effect(cell, Kind::Arrival, id, 1.0)]);
            assert!(
                moving.dock_glyphs().is_empty() && moving.dock_arrivals().len() == 1,
                "the arrival is not drawn through the effect's path ({keypress:?}, {:?})",
                cell.ch
            );
            let a = render_offscreen(&r, EDGE, BACKGROUND, &still);
            let b = render_offscreen(&r, EDGE, BACKGROUND, &moving);
            let top = dock_row_top(EDGE, cell_px.1);
            assert!(
                (0..EDGE).any(|x| brightness(&a, EDGE, x, top + usize::from(cell_px.1) / 2) > 0)
                    || (top..EDGE).any(|y| (0..EDGE).any(|x| brightness(&a, EDGE, x, y) > 0)),
                "the static glyph was never drawn — the equality would claim nothing ({:?})",
                cell.ch
            );
            assert!(
                a == b,
                "at t = 1 the arrival differs from the static glyph ({keypress:?}, {:?})",
                cell.ch
            );
        }
    }
}

#[test]
fn a_ghost_starts_as_the_glyph_and_ends_as_bare_ground() {
    // The ghost at `t = 0` is the deleted glyph itself (the letter does not jump at the moment of
    // deletion), at `t = 1` it is nothing: after the last effect frame the ground is bare.
    let r = renderer();
    const EDGE: usize = 64;
    let cell_px = fitting_cell_px(&r, EDGE, 4);
    let bare = render_offscreen(&r, EDGE, BACKGROUND, &dock_fx_frame(cell_px, &[], &[]));
    for &erase in &Erase::effects() {
        let id = erase.id().expect("a drawing effect");
        for cell in [glyph_cell(2, 'M', None), wide_cell(2, PAIR_CHAR)] {
            let glyph =
                render_offscreen(&r, EDGE, BACKGROUND, &dock_fx_frame(cell_px, &[cell], &[]));
            assert_ne!(glyph, bare, "the glyph was not drawn ({:?})", cell.ch);
            let start = dock_fx_frame(cell_px, &[], &[effect(cell, Kind::Ghost, id, 0.0)]);
            let end = dock_fx_frame(cell_px, &[], &[effect(cell, Kind::Ghost, id, 1.0)]);
            assert!(
                render_offscreen(&r, EDGE, BACKGROUND, &start) == glyph,
                "at t = 0 the ghost is not the deleted glyph ({erase:?}, {:?})",
                cell.ch
            );
            assert!(
                render_offscreen(&r, EDGE, BACKGROUND, &end) == bare,
                "at t = 1 the ghost does not leave the ground bare ({erase:?}, {:?})",
                cell.ch
            );
        }
    }
}

#[test]
fn an_effect_never_samples_its_neighbour_slot() {
    // The quad swells by the effect's margin, the inverse transform maps outside the cell to
    // outside the slot and the scaling arms sample linearly; without the bounds test and the clamp
    // to the texel centre, the neighbour slot's glyph (no margin between slots) would show up
    // inside the effect.
    //
    // **The criterion is independent of amplitude**: the same `.` is drawn on two atlases — in one
    // its slot has both sides filled (`@` takes a slot before, `#` after), in the other `.` is
    // alone. If the output does not look at the neighbour the two frames are identical. The
    // previous version said "every pixel outside the cell is a leak" and that tied the effect's
    // amplitude to the empty space of `.` inside its cell — when the user found the effects "not
    // noticeable at all" the amplitudes exceeded that envelope.
    //
    // Tolerance 2/255: `uv0` differs between the two atlases and the linear filter's lower texel
    // weight can move with the rounding of that difference; a leak, on the other hand, would bring
    // the neighbour's ink, that is a much larger difference.
    const EDGE: usize = 128;
    let crowded = renderer();
    let alone = renderer();
    let cell_px = fitting_cell_px(&crowded, EDGE, 8);
    // The atlas is built from the metrics: the second renderer must have the same metrics too.
    assert_eq!(fitting_cell_px(&alone, EDGE, 8), cell_px);
    let neighbours = [
        glyph_cell(2, '@', None),
        glyph_cell(3, '.', None),
        glyph_cell(4, '#', None),
    ];
    render_offscreen(
        &crowded,
        EDGE,
        BACKGROUND,
        &dock_fx_frame(cell_px, &neighbours, &[]),
    );
    let dot = glyph_cell(5, '.', None);
    render_offscreen(
        &alone,
        EDGE,
        BACKGROUND,
        &dock_fx_frame(cell_px, &[dot], &[]),
    );
    let kinds = Keypress::effects()
        .into_iter()
        .map(|fx| (Kind::Arrival, fx.id().expect("a drawing effect")))
        .chain(
            Erase::effects()
                .into_iter()
                .map(|fx| (Kind::Ghost, fx.id().expect("a drawing effect"))),
        );
    for (kind, id) in kinds {
        for t in [0.1, 0.25, 0.5, 0.75, 0.9] {
            // It must be the arrival's static glyph (otherwise it is not drawn).
            let statics: &[Cell] = if kind == Kind::Arrival { &[dot] } else { &[] };
            let frame = dock_fx_frame(cell_px, statics, &[effect(dot, kind, id, t)]);
            let a = render_offscreen(&crowded, EDGE, BACKGROUND, &frame);
            let b = render_offscreen(&alone, EDGE, BACKGROUND, &frame);
            let worst = a
                .iter()
                .zip(&b)
                .map(|(x, y)| x.abs_diff(*y))
                .max()
                .unwrap_or(0);
            assert!(
                worst <= 2,
                "the effect looks at the neighbour slot: the atlas with a filled neighbour and the lone \
                 atlas differ by {worst} ({kind:?} {id}, t = {t})"
            );
        }
    }
}

#[test]
fn heat_starts_in_the_cursor_color() {
    // `heat`'s colour comes from the uniform, not the instance (`Frame::dock_fx_heat`); had it not
    // been bound, or bound to the wrong slot, the letter would be born in the foreground's colour
    // and the `t = 1` equality could not see it. The foreground is [`WHITE`] (its red equals its
    // blue), [`HEAT`] is orange: at the brightest pixel red must clearly exceed blue.
    let r = renderer();
    const EDGE: usize = 64;
    let cell_px = fitting_cell_px(&r, EDGE, 4);
    let cell = glyph_cell(2, 'M', None);
    let id = Keypress::Heat.id().expect("a drawing effect");
    let pixels = render_offscreen(
        &r,
        EDGE,
        BACKGROUND,
        &dock_fx_frame(cell_px, &[cell], &[effect(cell, Kind::Arrival, id, 0.0)]),
    );
    let top = dock_row_top(EDGE, cell_px.1);
    let (red, _, blue) = (top..EDGE)
        .flat_map(|y| (0..EDGE).map(move |x| (x, y)))
        .map(|(x, y)| pixel_at(&pixels, EDGE, x, y))
        .max_by_key(|&(r8, g8, b8)| u32::from(r8) + u32::from(g8) + u32::from(b8))
        .expect("there is a pixel");
    assert!(
        u32::from(red) > u32::from(blue) + 64,
        "heat was not born in the hot colour: brightest pixel r={red} b={blue}"
    );
}

#[test]
fn a_shattered_glyph_breaks_the_same_way_every_frame() {
    // `shatter`'s pieces come from the seed (`FxInstance`'s `fx[2]`): the same input must give the
    // same pieces on two frames — otherwise the pieces jitter on a motion frame —, while another
    // seed must give another break. The second claim is the only witness that the shader really
    // reads the seed.
    let r = renderer();
    const EDGE: usize = 64;
    let cell_px = fitting_cell_px(&r, EDGE, 4);
    let cell = glyph_cell(2, 'M', None);
    let id = Erase::Shatter.id().expect("a drawing effect");
    let draw = |seed: f32| {
        let fx = Fx {
            seed,
            ..effect(cell, Kind::Ghost, id, 0.5)
        };
        render_offscreen(&r, EDGE, BACKGROUND, &dock_fx_frame(cell_px, &[], &[fx]))
    };
    let first = draw(7.0);
    assert!(
        first == draw(7.0),
        "the same seed broke differently on two frames"
    );
    assert!(first != draw(8.0), "the seed does not change the break");
}

#[test]
fn a_wide_glyph_transforms_as_one_box() {
    // A wide glyph is split across two slots but must transform as a single box: had the halves
    // shrunk to their own centres (or, in `extrude`, grown from their own left edges), an empty
    // stripe would open between the two — at the seam. In the columns on both sides of the seam the
    // static glyph has ink; the box's centre is the scales' fixed point, while the shifts carry the
    // seam through an inked row — so ink must remain there in every effect's in-between frame too.
    let r = renderer();
    const EDGE: usize = 64;
    let cell_px = fitting_cell_px(&r, EDGE, 4);
    let (cw, ch) = (usize::from(cell_px.0), usize::from(cell_px.1));
    let han = wide_cell(1, PAIR_CHAR);
    let seam = 2 * cw;
    let top = dock_row_top(EDGE, cell_px.1);
    let inked = |pixels: &[u8]| {
        (top..top + ch).any(|y| {
            brightness(pixels, EDGE, seam - 1, y) > 0 || brightness(pixels, EDGE, seam, y) > 0
        })
    };
    let still = render_offscreen(&r, EDGE, BACKGROUND, &dock_fx_frame(cell_px, &[han], &[]));
    assert!(
        inked(&still),
        "precondition: static `{PAIR_CHAR}` has no ink at the seam"
    );
    let kinds = Keypress::effects()
        .into_iter()
        .map(|fx| (Kind::Arrival, fx.id().expect("a drawing effect")))
        .chain(
            Erase::effects()
                .into_iter()
                .map(|fx| (Kind::Ghost, fx.id().expect("a drawing effect"))),
        );
    for (kind, id) in kinds {
        // It must be the arrival's static glyph (otherwise it is not drawn).
        let statics: &[Cell] = if kind == Kind::Arrival { &[han] } else { &[] };
        let frame = dock_fx_frame(cell_px, statics, &[effect(han, kind, id, 0.5)]);
        let pixels = render_offscreen(&r, EDGE, BACKGROUND, &frame);
        assert!(
            inked(&pixels),
            "the wide glyph split at the seam: the two halves transformed like separate boxes ({kind:?} {id})"
        );
    }
}

// **The slot quad.** Below `line_height = 1` the glyph's slot is taller than the
// grid cell and the glyph spills into its neighbours instead of being cut. `< 1` is reachable
// only from here: `set_font` takes the struct as is, the settings parser still
// clamps at `1`.

/// The line height the overflow guards draw at: low enough that `g`'s tail leaves its cell and
/// `É`'s accent rises above it, at both scales.
const TIGHT_LINE: f64 = 0.6;

fn tight_renderer() -> TestRenderer {
    let r = renderer();
    r.set_font(&FontOptions {
        line_height: TIGHT_LINE,
        ..FontOptions::default()
    });
    r
}

/// The open atlas's slot geometry (the renderer must have been asked for its metrics).
fn slot_quad_of(r: &TestRenderer) -> SlotQuad {
    let state = r.state.borrow();
    SlotQuad::of(&state.atlas.as_ref().expect("an open atlas").atlas)
}

/// The atlas's grid cell **without a gutter**, so column `n` starts at `n * cell width`.
fn flush(m: CellMetrics) -> CellMetrics {
    let (cw, ch) = m.cell_px();
    CellMetrics::new(cw, ch, m.context_cell_px(), 0, m.rule_px(), m.scale())
        .expect("non-zero metrics")
}

#[test]
fn slot_quad_is_the_cell_at_or_above_one() {
    // The GPU half: at `>= 1` the immediates describe today's quad — no offset, the slot is
    // the cell, no viewport lift. Below `1` the same function opens all three, or the claim
    // above would be vacuous.
    for scale in [1.0, 2.0] {
        for (line, letter) in [(1.0, 1.0), (1.2, 1.0), (1.0, 1.3), (1.2, 1.3)] {
            let atlas = Atlas::new(None, 13.0, scale, bt_atlas::Spacing { line, letter });
            let quad = SlotQuad::of(&atlas);
            let (cw, ch) = atlas.metrics().cell_px;
            let at = format!("{line}/{letter} @{scale}x");
            assert_eq!(quad.slot_offset, [0.0, 0.0], "offset, {at}");
            assert_eq!(quad.slot_px, [f32::from(cw), f32::from(ch)], "slot, {at}");
            assert_eq!(quad.overflow(), 0.0, "lift, {at}");
            let imm = quad.glyph_immediates(CursorBlock::default(), [64.0; 2], quad.overflow());
            assert_eq!(imm.slot_offset, [0.0, 0.0], "immediate offset, {at}");
            assert_eq!(
                imm.slot_px,
                [f32::from(cw), f32::from(ch)],
                "immediate slot, {at}"
            );
            assert_eq!(imm.lift, 0.0, "immediate lift, {at}");
            let fx = quad.fx_immediates([0.0; 4], [64.0; 2], [f32::from(cw), f32::from(ch)]);
            assert_eq!(
                (fx.slot_px, fx.slot_offset),
                (fx.cell_px, [0.0; 2]),
                "fx, {at}"
            );
        }
        let atlas = Atlas::new(
            None,
            13.0,
            scale,
            bt_atlas::Spacing {
                line: 0.6,
                letter: 0.7,
            },
        );
        let quad = SlotQuad::of(&atlas);
        let (cw, ch) = atlas.metrics().cell_px;
        assert!(
            quad.overflow() > 0.0
                && quad.slot_offset[0] > 0.0
                && quad.slot_px[0] > f32::from(cw)
                && quad.slot_px[1] > f32::from(ch),
            "below 1 the slot must be larger than the cell @{scale}x: {quad:?}"
        );
    }
}

#[test]
fn descender_paints_over_the_next_rows_background() {
    // `g`'s tail leaves row 0 and lands on row 1's red ground in the foreground colour —
    // the grid's grounds are all drawn before its glyphs and the quad is the slot. @2x only:
    // the cut below `1` is proportional to ascent:descent, and at 13pt@1x it takes a single
    // pixel off the bottom — the descent's own slack, so the tail still fits its cell there
    // (measured: no ink on row 1 for `g j y p _ , ( Q }`).
    const EDGE: usize = 64;
    let r = tight_renderer();
    let red = LinearRgba::from_srgb(0xff, 0x00, 0x00);
    let m = flush(r.cell_metrics(2.0));
    let (cw, ch) = (usize::from(m.cell_px().0), usize::from(m.cell_px().1));
    let mut frame = Frame::default();
    frame.clear(m, CaretStyle::default());
    frame.push(bg_cell(0, 1, red));
    frame.push(Cell {
        fg: WHITE,
        ..glyph_cell(0, 'g', None)
    });
    let pixels = render_offscreen(&r, EDGE, BACKGROUND, &frame);
    // Red has no green: any green on row 1 is the white tail over it.
    let tail = (ch..2 * ch)
        .flat_map(|y| (0..cw).map(move |x| (x, y)))
        .filter(|&(x, y)| pixel_at(&pixels, EDGE, x, y).1 > 0x80)
        .count();
    assert!(tail > 0, "the tail of `g` is not on row 1's ground");
    assert!(
        (ch..2 * ch).any(|y| pixel_at(&pixels, EDGE, cw - 1, y) == (0xff, 0x00, 0x00)),
        "row 1's red ground was not drawn"
    );
}

#[test]
fn grid_top_accent_survives_the_fill_band() {
    // The seam between the grid and the fill band: the grid's top row's accent rises
    // into the band, and the band's ground is drawn **before** the grid's glyphs, so it shows.
    const EDGE: usize = 64;
    let r = tight_renderer();
    let red = LinearRgba::from_srgb(0xff, 0x00, 0x00);
    for scale in [1.0, 2.0] {
        let m = flush(r.cell_metrics(scale));
        let (cw, ch) = (usize::from(m.cell_px().0), usize::from(m.cell_px().1));
        assert!(slot_quad_of(&r).overflow() > 0.0, "no overflow @{scale}x");
        let mut frame = Frame::default();
        frame.clear(m, CaretStyle::default());
        frame.push(Cell {
            fg: WHITE,
            ..glyph_cell(0, 'É', None)
        });
        frame.set_fill_rows(1);
        frame.push_fill(bg_cell(0, 0, red));
        frame.set_origin_rows(1.0);
        let pixels = render_offscreen(&r, EDGE, BACKGROUND, &frame);
        assert_eq!(
            pixel_at(&pixels, EDGE, 0, 0),
            (0xff, 0x00, 0x00),
            "the band's ground was not drawn @{scale}x"
        );
        let accent = (0..ch)
            .flat_map(|y| (0..cw).map(move |x| (x, y)))
            .any(|(x, y)| pixel_at(&pixels, EDGE, x, y).1 > 0x80);
        assert!(
            accent,
            "the accent of `É` is hidden under the band @{scale}x"
        );
    }
}

#[test]
fn caret_stays_under_the_fill_band() {
    // The draw order's other half: a grid caret sliding into the band is
    // drawn **before** the band's ground and is covered by it. Raising the grid's glyphs over the
    // band must not raise the caret with them.
    let r = renderer();
    const EDGE: usize = 16;
    const CELL: u16 = 8;
    let red = LinearRgba::from_srgb(0xff, 0x00, 0x00);
    let mut frame = Frame::default();
    frame.clear(grid(CELL, CELL), CaretStyle::default());
    frame.set_fill_rows(1);
    frame.push_fill(bg_cell(0, 0, red));
    frame.set_origin_rows(1.0);
    // Half a row into the band, in window rows.
    frame.push_caret([0.0, 0.5], BACKGROUND, ACCENT, 1.0, CaretShape::Block, true);
    assert!(
        frame.grid_caret().is_some(),
        "the caret is not in the grid slot"
    );
    let pixels = render_offscreen(&r, EDGE, BACKGROUND, &frame);
    let accent = {
        let hex = Theme::BATERI.accent;
        ((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
    };
    let near = |seen: (u8, u8, u8), want: (u8, u8, u8)| {
        seen.0.abs_diff(want.0) <= 1 && seen.1.abs_diff(want.1) <= 1 && seen.2.abs_diff(want.2) <= 1
    };
    let x = usize::from(CELL) / 2;
    assert_eq!(
        pixel_at(&pixels, EDGE, x, 6),
        (0xff, 0x00, 0x00),
        "the caret is drawn over the band's ground"
    );
    let below = pixel_at(&pixels, EDGE, x, 10);
    assert!(
        near(below, accent),
        "the caret's grid half is missing: {below:02x?}"
    );
}

#[test]
fn dock_glyph_stays_inside_its_band() {
    // The dock is a separate panel. With no breathing margin (gutter zero) its input row
    // starts at the band's top and the accent rising above it is cut at the band's top — the
    // grid's area above stays the clear colour.
    const EDGE: usize = 64;
    let r = tight_renderer();
    for scale in [1.0, 2.0] {
        let m = flush(r.cell_metrics(scale));
        let ch = usize::from(m.cell_px().1);
        assert!(slot_quad_of(&r).overflow() > 0.0, "no overflow @{scale}x");
        let mut frame = Frame::default();
        frame.clear(m, CaretStyle::default());
        frame.push_dock(Cell {
            fg: WHITE,
            ..glyph_cell(1, 'É', None)
        });
        frame.set_dock_rows(1);
        frame.open_dock(BACKGROUND, BACKGROUND, BACKGROUND);
        let band_top = EDGE - frame.dock_layout_px() as usize;
        assert_eq!(band_top, EDGE - ch, "the band is one row @{scale}x");
        let pixels = render_offscreen(&r, EDGE, ACCENT, &frame);
        let cw = usize::from(m.cell_px().0);
        assert!(
            (cw..2 * cw).any(|x| pixel_at(&pixels, EDGE, x, band_top).1 > 0x80),
            "the accent does not reach the band's top — nothing to clip @{scale}x"
        );
        let clear = pixel_at(&pixels, EDGE, EDGE - 1, 0);
        for y in 0..band_top {
            for x in 0..EDGE {
                assert_eq!(
                    pixel_at(&pixels, EDGE, x, y),
                    clear,
                    "the dock's glyph spilled above its band at ({x}, {y}) @{scale}x"
                );
            }
        }
    }
}

#[test]
fn arrival_effect_matches_static_glyph_below_one() {
    // Below `1` the arrival at `t = 1` is still the static glyph pixel for pixel —
    // the effect samples the slot (bound, texel), and the static glyph's accent, spilling into
    // the breathing margin above the input row, is not cut by the dock's glyph viewport.
    const EDGE: usize = 96;
    let r = tight_renderer();
    let m = r.cell_metrics(2.0);
    let gutter = usize::from(m.gutter_px());
    assert!(
        slot_quad_of(&r).overflow() > 0.0 && slot_quad_of(&r).overflow() < gutter as f32,
        "the overflow must be inside the breathing margin to be tested"
    );
    let still_frame = |fx: &[Fx], cell: Cell| {
        let mut frame = Frame::default();
        frame.clear(m, CaretStyle::default());
        frame.push_dock(cell);
        frame.set_dock_fx(fx.iter().copied(), &Clusters::default(), HEAT);
        frame.set_dock_rows(1);
        frame.open_dock(BACKGROUND, BACKGROUND, BACKGROUND);
        frame
    };
    let cell = Cell {
        fg: WHITE,
        ..glyph_cell(1, 'É', None)
    };
    let still = still_frame(&[], cell);
    let band_top = EDGE - still.dock_layout_px() as usize;
    let a = render_offscreen(&r, EDGE, BACKGROUND, &still);
    assert!(
        (band_top..band_top + gutter).any(|y| (0..EDGE).any(|x| pixel_at(&a, EDGE, x, y).1 > 0x80)),
        "the static accent does not spill into the breathing margin — the claim is vacuous"
    );
    for &keypress in &Keypress::effects() {
        let id = keypress.id().expect("a drawing effect");
        let moving = still_frame(&[effect(cell, Kind::Arrival, id, 1.0)], cell);
        assert!(
            moving.dock_glyphs().is_empty() && moving.dock_arrivals().len() == 1,
            "the arrival is not drawn through the effect's path ({keypress:?})"
        );
        let b = render_offscreen(&r, EDGE, BACKGROUND, &moving);
        assert!(
            a == b,
            "at t = 1 the arrival differs from the static glyph ({keypress:?})"
        );
    }
}

#[test]
fn a_lifted_viewport_keeps_the_window_bottom() {
    // The glyph viewport is raised by the overflow and **taller by it** (`Op::Lifted`): a grid
    // row sitting on the window's bottom edge (no dock to cover it — vim, `blocks`) keeps the
    // ink of its cell's last pixels. Raised but not taller, the viewport's bottom would end
    // `lift` pixels above the window's (found in code review). The same glyph on the top row and on
    // the bottom row of a grid whose origin leaves less than a row: the cell's pixels match.
    const EDGE: usize = 64;
    let r = tight_renderer();
    let m = flush(r.cell_metrics(2.0));
    let (cw, ch) = (usize::from(m.cell_px().0), usize::from(m.cell_px().1));
    let lift = slot_quad_of(&r).overflow() as usize;
    let last = EDGE / ch - 1;
    let origin = EDGE - (last + 1) * ch;
    assert!(
        lift > origin,
        "the case needs a lift larger than the origin"
    );
    let draw = |row: u16| {
        let mut frame = Frame::default();
        frame.clear(m, CaretStyle::default());
        frame.push(Cell {
            row,
            fg: WHITE,
            ..glyph_cell(0, 'g', None)
        });
        frame.set_origin_rows(origin as f32 / ch as f32);
        let pixels = render_offscreen(&r, EDGE, BACKGROUND, &frame);
        let top = origin + usize::from(row) * ch;
        (0..ch)
            .map(|y| {
                (0..cw)
                    .map(|x| pixel_at(&pixels, EDGE, x, top + y))
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>()
    };
    let first = draw(0);
    let bottom = draw(last as u16);
    assert!(
        first[ch - lift..].concat().iter().any(|p| p.1 > 0x80),
        "no ink in the cell's last `lift` rows — nothing to lose"
    );
    assert_eq!(
        first, bottom,
        "the bottom row's glyph lost ink at the window's edge"
    );
}
