//! The font's own rules on the screen: a masked key and an alternate letter
//! on every surface that draws text, through the typing effects, and the
//! fonts and cells the pass must leave exactly as they were.
//!
//! The fonts are the repository's test fonts (`bt_atlas::fixture`): no
//! installed font carries a rule every Mac has.

use bt_atlas::fixture::{JETBRAINS_MONO, OBEL_MONO, register_test_fonts};
use bt_core::{CaretStyle, Cell, FontOptions};

use super::tests::{BACKGROUND, WHITE, glyph_cell, grid, pixel_at};
use crate::Renderer;
use crate::frame::Frame;
use crate::glyph_fx::{Effect, Fx, Kind};
use bt_core::Clusters;

/// A key the test font masks under `ss01`: some of its characters change.
const KEY: &str = "sk-ant-abc123";
const EDGE: usize = 128;
const HEAT: bt_core::LinearRgba = bt_core::LinearRgba::from_srgb(0xff, 0x80, 0x20);

/// The renderer with `family` and its feature choices, the atlas opened.
fn renderer_with(family: &str, features: &[(&str, bool)]) -> Renderer {
    register_test_fonts();
    let r = Renderer::new();
    let options = FontOptions {
        family: Some(family.to_owned()),
        features: [(
            family.to_lowercase(),
            features
                .iter()
                .map(|&(tag, on)| (tag.to_owned(), on))
                .collect(),
        )]
        .into(),
        ..FontOptions::default()
    };
    r.set_font(&options);
    let _ = r.cell_metrics(1.0);
    r
}

/// The renderer on the default chain, no choice made: a font without rules.
fn plain_renderer() -> Renderer {
    let r = Renderer::new();
    let _ = r.cell_metrics(1.0);
    r
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Surface {
    Grid,
    Band,
    Dock,
}

/// `text` on one surface, one character per column, shaped by `r` unless
/// `shaped` is off. The grid's row is the top of the texture, the band's is
/// the one above the content, the dock's the bottom.
fn frame_of(r: &Renderer, surface: Surface, text: &str, bold: bool, shaped: bool) -> Frame {
    let (cw, ch) = r.cell_metrics(1.0).cell_px();
    assert!(
        usize::from(cw) * text.chars().count() <= EDGE && usize::from(ch) * 2 <= EDGE,
        "the row does not fit the offscreen texture"
    );
    let mut frame = Frame::default();
    frame.clear(grid(cw, ch), CaretStyle::default());
    let cells = text.chars().enumerate().map(|(at, c)| Cell {
        bold,
        ..glyph_cell(u16::try_from(at).expect("a short row"), c, None)
    });
    match surface {
        Surface::Grid => cells.for_each(|cell| frame.push(cell)),
        Surface::Band => {
            frame.set_fill_rows(1);
            cells.for_each(|cell| frame.push_fill(cell));
            frame.set_origin_rows(1.0);
        }
        Surface::Dock => {
            cells.for_each(|cell| frame.push_dock(cell));
            frame.set_dock_fx(std::iter::empty(), &Clusters::default(), HEAT);
            frame.set_dock_rows(1);
            frame.open_dock(BACKGROUND, BACKGROUND, BACKGROUND, BACKGROUND);
        }
    }
    if shaped {
        match surface {
            Surface::Dock => frame.shape_dock(|cells| r.shape(cells)),
            _ => frame.shape_grid(|cells| r.shape(cells)),
        }
    }
    frame
}

/// The glyphs of the frame's surface, as the shaper left them.
fn shaped_places(frame: &Frame, surface: Surface) -> Vec<Option<u32>> {
    let cells = match surface {
        Surface::Grid => frame.glyphs(),
        Surface::Band => frame.fill_glyphs(),
        Surface::Dock => frame.dock_glyphs(),
    };
    cells.iter().map(|cell| cell.glyph).collect()
}

/// A cell's pixels. Its `Debug` is a summary: a failed comparison of two
/// cells would otherwise print a hundred identical black pixels each.
#[derive(PartialEq)]
struct Pixels(Vec<(u8, u8, u8)>);

impl std::fmt::Debug for Pixels {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let ink = self.0.iter().filter(|px| px.0 > 0).count();
        write!(f, "{} pixels, {ink} of them lit", self.0.len())
    }
}

/// The pixels of column `col` of the surface's row, top to bottom.
fn column(r: &Renderer, frame: &Frame, surface: Surface, col: usize) -> Pixels {
    let pixels = r.render_offscreen(EDGE as u32, BACKGROUND, frame);
    let (cw, ch) = r.cell_metrics(1.0).cell_px();
    let (cw, ch) = (usize::from(cw), usize::from(ch));
    let top = if surface == Surface::Dock {
        EDGE - ch
    } else {
        0
    };
    Pixels(
        (0..ch)
            .flat_map(|y| (0..cw).map(move |x| (x, y)))
            .map(|(x, y)| pixel_at(&pixels, EDGE, col * cw + x, top + y))
            .collect(),
    )
}

const SURFACES: [Surface; 3] = [Surface::Grid, Surface::Band, Surface::Dock];

#[test]
fn a_masked_key_changes_the_same_cells_on_every_surface_and_only_when_it_is_on() {
    let on = renderer_with(OBEL_MONO, &[("ss01", true)]);
    let off = renderer_with(OBEL_MONO, &[("ss01", false)]);
    let unchosen = renderer_with(OBEL_MONO, &[]);
    let mut masked_pixels = Vec::new();
    for surface in SURFACES {
        let masked = frame_of(&on, surface, KEY, false, true);
        let places = shaped_places(&masked, surface);
        let masked_cols: Vec<usize> = (0..places.len())
            .filter(|&at| places[at].is_some())
            .collect();
        assert!(!masked_cols.is_empty(), "{surface:?}: nothing was masked");
        assert!(
            masked_cols.len() < places.len(),
            "{surface:?}: all of it was"
        );
        let plain = frame_of(&off, surface, KEY, false, true);
        let never = frame_of(&unchosen, surface, KEY, false, true);
        assert!(
            shaped_places(&plain, surface).iter().all(Option::is_none),
            "{surface:?}: ss01 off still shapes"
        );
        for col in 0..places.len() {
            let (a, b) = (
                column(&on, &masked, surface, col),
                column(&off, &plain, surface, col),
            );
            if masked_cols.contains(&col) {
                assert_ne!(
                    a, b,
                    "{surface:?}: column {col} is masked and draws the letter"
                );
            } else {
                assert_eq!(a, b, "{surface:?}: column {col} is not masked and moved");
            }
            // No line in the file reads as ss01 off.
            assert_eq!(
                column(&unchosen, &never, surface, col),
                b,
                "{surface:?}: {col}"
            );
        }
        masked_pixels.push((surface, column(&on, &masked, surface, masked_cols[0])));
    }
    // The same glyph on the three surfaces, pixel for pixel.
    for (surface, pixels) in &masked_pixels[1..] {
        assert_eq!(
            *pixels, masked_pixels[0].1,
            "{surface:?} drew another glyph"
        );
    }
}

#[test]
fn the_bold_face_is_masked_too() {
    let on = renderer_with(OBEL_MONO, &[("ss01", true)]);
    let off = renderer_with(OBEL_MONO, &[("ss01", false)]);
    for surface in SURFACES {
        let masked = frame_of(&on, surface, KEY, true, true);
        let places = shaped_places(&masked, surface);
        // The last one: the key's separators keep their look under the mask.
        let col = places
            .iter()
            .rposition(Option::is_some)
            .expect("a masked key");
        let plain = frame_of(&off, surface, KEY, true, true);
        assert_ne!(
            column(&on, &masked, surface, col),
            column(&off, &plain, surface, col),
            "{surface:?}: a bold key is open"
        );
    }
}

#[test]
fn an_alternate_letter_draws_in_its_own_shape_and_the_rest_from_todays_slots() {
    let on = renderer_with(OBEL_MONO, &[("cv02", true)]);
    let off = renderer_with(OBEL_MONO, &[]);
    for surface in SURFACES {
        let alternate = frame_of(&on, surface, "abca", false, true);
        let plain = frame_of(&off, surface, "abca", false, true);
        assert_eq!(
            shaped_places(&alternate, surface)
                .iter()
                .map(Option::is_some)
                .collect::<Vec<_>>(),
            [true, false, false, true],
            "{surface:?}"
        );
        for col in 0..4 {
            let changed = col == 0 || col == 3;
            assert_eq!(
                column(&on, &alternate, surface, col) != column(&off, &plain, surface, col),
                changed,
                "{surface:?}: column {col}"
            );
        }
    }
}

#[test]
fn a_ligature_font_draws_the_arrow_as_two_characters() {
    let r = renderer_with(JETBRAINS_MONO, &[]);
    for surface in SURFACES {
        let text = "a->b != c";
        let shaped = frame_of(&r, surface, text, false, true);
        let unshaped = frame_of(&r, surface, text, false, false);
        assert!(
            shaped_places(&shaped, surface).iter().all(Option::is_none),
            "{surface:?}: the font joined a pair into one glyph"
        );
        for col in 0..text.len() {
            assert_eq!(
                column(&r, &shaped, surface, col),
                column(&r, &unshaped, surface, col),
                "{surface:?}: column {col} moved"
            );
        }
    }
}

#[test]
fn a_font_without_rules_is_not_asked_and_a_row_seen_twice_is_asked_once() {
    let menlo = plain_renderer();
    for surface in SURFACES {
        let frame = frame_of(&menlo, surface, KEY, false, true);
        assert!(shaped_places(&frame, surface).iter().all(Option::is_none));
    }
    assert_eq!(
        menlo.shape_calls(),
        0,
        "the shaper was asked for a font without rules"
    );

    let obel = renderer_with(OBEL_MONO, &[("ss01", true)]);
    let first = frame_of(&obel, Surface::Grid, KEY, false, true);
    assert_eq!(obel.shape_calls(), 1);
    // The grid is rebuilt every frame: the same row is the cache's.
    let again = frame_of(&obel, Surface::Grid, KEY, false, true);
    assert_eq!(obel.shape_calls(), 1, "the same row was shaped twice");
    assert_eq!(
        shaped_places(&first, Surface::Grid),
        shaped_places(&again, Surface::Grid)
    );
    // The dock's row is the same text in the same face: the same answer.
    let _ = frame_of(&obel, Surface::Dock, KEY, false, true);
    assert_eq!(obel.shape_calls(), 1);
}

#[test]
fn a_wide_character_and_a_blank_stay_where_the_grid_put_them() {
    // `a`, a wide emoji (its second column is blank), `b`, a wide CJK
    // character: only the single-cell letters are the shaper's.
    let r = renderer_with(OBEL_MONO, &[("cv02", true)]);
    let (cw, ch) = r.cell_metrics(1.0).cell_px();
    let build = |shaped: bool| {
        let mut frame = Frame::default();
        frame.clear(grid(cw, ch), CaretStyle::default());
        for (col, c, wide) in [
            (0, 'a', false),
            (1, '🎉', true),
            (3, 'a', false),
            (4, '世', true),
        ] {
            frame.push(Cell {
                wide,
                ..glyph_cell(col, c, None)
            });
        }
        if shaped {
            frame.shape_grid(|cells| r.shape(cells));
        }
        frame
    };
    let (shaped, unshaped) = (build(true), build(false));
    let places = shaped_places(&shaped, Surface::Grid);
    assert_eq!(
        places.iter().map(Option::is_some).collect::<Vec<_>>(),
        [true, false, true, false],
        "an alternate `a` on each side of a wide character, nothing on the wide ones"
    );
    for col in [1, 2, 4, 5] {
        assert_eq!(
            column(&r, &shaped, Surface::Grid, col),
            column(&r, &unshaped, Surface::Grid, col),
            "column {col} of the wide characters moved"
        );
    }
}

/// A frame whose dock is `text` minus the character at `gone`, built after a
/// frame that had it: the deleted cell is what a ghost is born from.
fn dock_after_a_delete(r: &Renderer, gone: usize) -> (Frame, Cell) {
    let (cw, ch) = r.cell_metrics(1.0).cell_px();
    let mut frame = Frame::default();
    frame.clear(grid(cw, ch), CaretStyle::default());
    let cells: Vec<Cell> = KEY
        .chars()
        .enumerate()
        .map(|(at, c)| glyph_cell(u16::try_from(at).expect("a short row"), c, None))
        .collect();
    cells.iter().for_each(|&cell| frame.push_dock(cell));
    frame.shape_dock(|cells| r.shape(cells));
    // The next content frame, with the character gone.
    frame.clear(grid(cw, ch), CaretStyle::default());
    cells
        .iter()
        .filter(|cell| usize::from(cell.col) != gone)
        .for_each(|&cell| frame.push_dock(cell));
    frame.shape_dock(|cells| r.shape(cells));
    (frame, cells[gone])
}

#[test]
fn a_ghost_of_a_masked_character_fades_as_the_dot_not_the_letter() {
    let on = renderer_with(OBEL_MONO, &[("ss01", true)]);
    let off = renderer_with(OBEL_MONO, &[("ss01", false)]);
    // The last masked column: nothing after it moves the answer.
    let masked = frame_of(&on, Surface::Dock, KEY, false, true);
    let gone = shaped_places(&masked, Surface::Dock)
        .iter()
        .rposition(Option::is_some)
        .expect("a masked key");
    let (mut frame, cell) = dock_after_a_delete(&on, gone);
    let remembered = frame.recalled(&cell);
    assert!(
        remembered.is_some(),
        "the dock before this one knew the glyph"
    );
    let ghost = Fx {
        cell,
        kind: Kind::Ghost,
        effect: bt_core::Erase::effects()[0].id().expect("a drawing effect"),
        t: 0.0,
        seed: 0.0,
        glyph: remembered,
    };
    frame.set_dock_fx([ghost], &Clusters::default(), HEAT);
    frame.set_dock_rows(1);
    frame.open_dock(BACKGROUND, BACKGROUND, BACKGROUND, BACKGROUND);
    assert_eq!(frame.dock_ghosts()[0].glyph.glyph, remembered);
    // At `t = 0` the ghost is the glyph that was on screen: the dot the full
    // key drew there, and not the letter the font draws with the mask off.
    let ghost_at = column(&on, &frame, Surface::Dock, gone);
    assert_eq!(ghost_at, column(&on, &masked, Surface::Dock, gone));
    let open = frame_of(&off, Surface::Dock, KEY, false, true);
    assert_ne!(ghost_at, column(&off, &open, Surface::Dock, gone));
}

#[test]
fn an_arrival_carries_the_glyph_and_ends_as_the_static_one() {
    let on = renderer_with(OBEL_MONO, &[("ss01", true)]);
    let (cw, ch) = on.cell_metrics(1.0).cell_px();
    let col = shaped_places(
        &frame_of(&on, Surface::Dock, KEY, false, true),
        Surface::Dock,
    )
    .iter()
    .position(Option::is_some)
    .expect("a masked key");
    let static_frame = frame_of(&on, Surface::Dock, KEY, false, true);
    // The same dock with the masked character arriving: it is drawn by the
    // effect, from the answer it was copied with.
    let mut frame = Frame::default();
    frame.clear(grid(cw, ch), CaretStyle::default());
    let cells: Vec<Cell> = KEY
        .chars()
        .enumerate()
        .map(|(at, c)| glyph_cell(u16::try_from(at).expect("a short row"), c, None))
        .collect();
    cells.iter().for_each(|&cell| frame.push_dock(cell));
    frame.shape_dock(|cells| on.shape(cells));
    let arrival = Fx {
        cell: cells[col],
        kind: Kind::Arrival,
        effect: bt_core::Keypress::effects()[0]
            .id()
            .expect("a drawing effect"),
        t: 1.0,
        seed: 0.0,
        glyph: None,
    };
    frame.set_dock_fx([arrival], &Clusters::default(), HEAT);
    frame.set_dock_rows(1);
    frame.open_dock(BACKGROUND, BACKGROUND, BACKGROUND, BACKGROUND);
    assert_eq!(frame.dock_arrivals().len(), 1);
    assert!(frame.dock_arrivals()[0].glyph.glyph.is_some());
    assert_eq!(
        column(&on, &frame, Surface::Dock, col),
        column(&on, &static_frame, Surface::Dock, col),
        "at t = 1 the arrival differs from the static glyph"
    );
    let _ = WHITE;
}
