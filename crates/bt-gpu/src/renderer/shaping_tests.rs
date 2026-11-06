//! The font's own rules on the screen: a masked key and an alternate letter
//! on every surface that draws text, through the typing effects, and the
//! fonts and cells the pass must leave exactly as they were.
//!
//! The fonts are the repository's test fonts (`bt_atlas::fixture`): no
//! installed font carries a rule every Mac has.

use bt_atlas::Shaped;
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
    renderer_spaced(family, features, 1.0)
}

/// [`renderer_with`] at a letter spacing.
fn renderer_spaced(family: &str, features: &[(&str, bool)], letter: f64) -> Renderer {
    register_test_fonts();
    let r = Renderer::new();
    let options = FontOptions {
        family: Some(family.to_owned()),
        letter_spacing: letter,
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
            Surface::Dock => frame.shape_dock(|cells, above| r.shape(cells, above)),
            _ => frame.shape_grid(|cells, above| r.shape(cells, above)),
        }
    }
    frame
}

/// The glyphs of the frame's surface, as the shaper left them.
fn shaped_places(frame: &Frame, surface: Surface) -> Vec<Option<Shaped>> {
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
    place(r, frame, surface, 0, col)
}

/// The pixels of the cell at `row` and `col` of the grid or the band, top to
/// bottom (the dock's rows are read at the glyph, [`shaped_places`]).
fn place(r: &Renderer, frame: &Frame, surface: Surface, row: usize, col: usize) -> Pixels {
    let pixels = r.render_offscreen(EDGE as u32, BACKGROUND, frame);
    let (cw, ch) = r.cell_metrics(1.0).cell_px();
    let (cw, ch) = (usize::from(cw), usize::from(ch));
    let top = if surface == Surface::Dock {
        assert_eq!(row, 0, "the dock's second row is read at the glyph");
        EDGE - ch
    } else {
        row * ch
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
            frame.shape_grid(|cells, above| r.shape(cells, above));
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

/// A frame whose dock is [`KEY`] minus the character at `gone`, built after a
/// frame that had it: the deleted cell is what a ghost is born from.
fn dock_after_a_delete(r: &Renderer, gone: usize) -> (Frame, Cell) {
    dock_of_after_a_delete(r, KEY, gone)
}

/// [`dock_after_a_delete`] for any `text`.
fn dock_of_after_a_delete(r: &Renderer, text: &str, gone: usize) -> (Frame, Cell) {
    let (cw, ch) = r.cell_metrics(1.0).cell_px();
    let mut frame = Frame::default();
    frame.clear(grid(cw, ch), CaretStyle::default());
    let cells: Vec<Cell> = text
        .chars()
        .enumerate()
        .map(|(at, c)| glyph_cell(u16::try_from(at).expect("a short row"), c, None))
        .collect();
    cells.iter().for_each(|&cell| frame.push_dock(cell));
    frame.shape_dock(|cells, above| r.shape(cells, above));
    // The next content frame, with the character gone.
    frame.clear(grid(cw, ch), CaretStyle::default());
    cells
        .iter()
        .filter(|cell| usize::from(cell.col) != gone)
        .for_each(|&cell| frame.push_dock(cell));
    frame.shape_dock(|cells, above| r.shape(cells, above));
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
fn a_ghost_of_one_slice_of_a_ligature_fades_as_its_character() {
    let r = renderer_with(JETBRAINS_MONO, &[]);
    // `a->b`: the `>` was the second slice of the shape `->` drew. A ghost is
    // one cell and half a shape alone would be a cut-off bar.
    let (frame, cell) = dock_of_after_a_delete(&r, "a->b", 2);
    assert_eq!(cell.ch, Some('>'));
    assert_eq!(frame.recalled(&cell), None);
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
    frame.shape_dock(|cells, above| on.shape(cells, above));
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

/// One row of a wrapped line for [`wrapped_frame`].
#[derive(Clone, Copy)]
struct Line<'a> {
    text: &'a str,
    /// Whether the terminal wrapped onto this row from the one above it.
    continues: bool,
    bold: bool,
}

fn line(text: &str, continues: bool) -> Line<'_> {
    Line {
        text,
        continues,
        bold: false,
    }
}

/// The first and the second part of [`KEY`]: the second is what a narrow
/// grid wraps onto the next row.
const HEAD: &str = "sk-ant-";
const TAIL: &str = "abc123";

/// `lines` as rows of one surface, as `Session` hands them over, and `lead`
/// as the text above the first row, shaped by `r`. The dock's rows start
/// after the mark.
fn wrapped_frame(r: &Renderer, surface: Surface, lines: &[Line<'_>], lead: &str) -> Frame {
    let (cw, ch) = r.cell_metrics(1.0).cell_px();
    let mut frame = Frame::default();
    frame.clear(grid(cw, ch), CaretStyle::default());
    let margin = if surface == Surface::Dock {
        bt_core::DOCK_TEXT_COL
    } else {
        0
    };
    let cells = lines.iter().enumerate().flat_map(|(row, line)| {
        line.text.chars().enumerate().map(move |(at, c)| Cell {
            row: u16::try_from(row).expect("a few rows"),
            bold: line.bold,
            continues: line.continues,
            ..glyph_cell(margin + u16::try_from(at).expect("a short row"), c, None)
        })
    });
    let rows = u16::try_from(lines.len()).expect("a few rows");
    match surface {
        Surface::Grid => cells.for_each(|cell| frame.push(cell)),
        Surface::Band => {
            frame.set_fill_rows(rows);
            cells.for_each(|cell| frame.push_fill(cell));
            frame.set_origin_rows(f32::from(rows));
        }
        Surface::Dock => cells.for_each(|cell| frame.push_dock(cell)),
    }
    let mut leads = frame.take_leads();
    match surface {
        Surface::Grid => leads.grid = lead.to_owned(),
        Surface::Band => leads.fill = lead.to_owned(),
        Surface::Dock => frame.put_dock_lead(lead.to_owned()),
    }
    frame.put_leads(leads);
    match surface {
        Surface::Dock => frame.shape_dock(|cells, above| r.shape(cells, above)),
        _ => frame.shape_grid(|cells, above| r.shape(cells, above)),
    }
    frame
}

#[test]
fn a_key_the_terminal_wrapped_is_masked_in_both_rows() {
    let on = renderer_with(OBEL_MONO, &[("ss01", true)]);
    for surface in [Surface::Grid, Surface::Band] {
        let whole = frame_of(&on, surface, KEY, false, true);
        let wrapped = wrapped_frame(&on, surface, &[line(HEAD, false), line(TAIL, true)], "");
        let places = shaped_places(&wrapped, surface);
        assert_eq!(places, shaped_places(&whole, surface), "{surface:?}");
        assert!(
            places[HEAD.len()..].iter().any(Option::is_some),
            "{surface:?}: nothing on the second row is masked"
        );
        for at in 0..KEY.len() {
            let (row, col) = if at < HEAD.len() {
                (0, at)
            } else {
                (1, at - HEAD.len())
            };
            assert_eq!(
                place(&on, &wrapped, surface, row, col),
                column(&on, &whole, surface, at),
                "{surface:?}: character {at} is drawn another way wrapped"
            );
        }
    }
}

#[test]
fn a_line_break_and_a_new_face_end_what_the_font_reads() {
    let on = renderer_with(OBEL_MONO, &[("ss01", true)]);
    for surface in SURFACES {
        let alone = shaped_places(&frame_of(&on, surface, TAIL, false, true), surface);
        assert!(alone.iter().all(Option::is_none), "{surface:?}: {alone:?}");
        // The second row begins a line of its own: the rows above are not its text.
        let broken = wrapped_frame(&on, surface, &[line(HEAD, false), line(TAIL, false)], "");
        assert_eq!(shaped_places(&broken, surface)[HEAD.len()..], alone[..]);
        // The row wraps on from a row of another face: a bold tail is read as itself.
        let bold = Line {
            bold: true,
            ..line(TAIL, true)
        };
        let faced = wrapped_frame(&on, surface, &[line(HEAD, false), bold], "");
        let alone = shaped_places(&frame_of(&on, surface, TAIL, true, true), surface);
        assert_eq!(shaped_places(&faced, surface)[HEAD.len()..], alone[..]);
    }
}

#[test]
fn a_row_that_begins_mid_line_is_masked_from_the_text_above_it() {
    let on = renderer_with(OBEL_MONO, &[("ss01", true)]);
    for surface in SURFACES {
        let whole = frame_of(&on, surface, KEY, false, true);
        let expected = shaped_places(&whole, surface)[HEAD.len()..].to_vec();
        assert!(expected.iter().any(Option::is_some));
        // The first row on screen is the tail: the head is scrolled away, in
        // the lead.
        let tail = wrapped_frame(&on, surface, &[line(TAIL, true)], HEAD);
        assert_eq!(shaped_places(&tail, surface), expected, "{surface:?}");
        if surface != Surface::Dock {
            for at in HEAD.len()..KEY.len() {
                assert_eq!(
                    column(&on, &tail, surface, at - HEAD.len()),
                    column(&on, &whole, surface, at),
                    "{surface:?}: character {at}"
                );
            }
        }
        // No lead, or a row that does not continue it: nothing above it.
        let bare = wrapped_frame(&on, surface, &[line(TAIL, true)], "");
        assert!(shaped_places(&bare, surface).iter().all(Option::is_none));
        let apart = wrapped_frame(&on, surface, &[line(TAIL, false)], HEAD);
        assert!(shaped_places(&apart, surface).iter().all(Option::is_none));
    }
}

#[test]
fn the_grid_reads_the_band_above_it_as_its_text_and_each_list_answers_for_its_own_cells() {
    let on = renderer_with(OBEL_MONO, &[("ss01", true)]);
    let (cw, ch) = on.cell_metrics(1.0).cell_px();
    let mut frame = Frame::default();
    frame.clear(grid(cw, ch), CaretStyle::default());
    frame.set_fill_rows(1);
    for (at, c) in HEAD.chars().enumerate() {
        frame.push_fill(glyph_cell(u16::try_from(at).expect("short"), c, None));
    }
    for (at, c) in TAIL.chars().enumerate() {
        frame.push(Cell {
            continues: true,
            ..glyph_cell(u16::try_from(at).expect("short"), c, None)
        });
    }
    frame.set_origin_rows(1.0);
    // What `Session::frame` writes: the grid's first row runs on from the
    // band's rows, which are the band's own cells.
    let mut leads = frame.take_leads();
    leads.grid = HEAD.to_owned();
    frame.put_leads(leads);
    frame.shape_grid(|cells, above| on.shape(cells, above));
    let whole = shaped_places(
        &frame_of(&on, Surface::Grid, KEY, false, true),
        Surface::Grid,
    );
    assert_eq!(shaped_places(&frame, Surface::Grid), whole[HEAD.len()..]);
    assert_eq!(shaped_places(&frame, Surface::Band), whole[..HEAD.len()]);
}

#[test]
fn a_start_further_back_than_the_bound_is_the_fonts_own_result() {
    let on = renderer_with(OBEL_MONO, &[("ss01", true)]);
    // The key began 400 characters before the row: more than is handed over.
    let far = format!("{HEAD}{}", "q".repeat(400));
    for surface in SURFACES {
        let near = wrapped_frame(&on, surface, &[line(TAIL, true)], HEAD);
        assert!(
            shaped_places(&near, surface).iter().any(Option::is_some),
            "{surface:?}: the head in reach masks the tail"
        );
        let beyond = wrapped_frame(&on, surface, &[line(TAIL, true)], &far);
        let unrelated = wrapped_frame(&on, surface, &[line(TAIL, true)], "qqqqq");
        assert_eq!(
            shaped_places(&beyond, surface),
            shaped_places(&unrelated, surface),
            "{surface:?}: the head out of reach still masks the tail"
        );
    }
}

#[test]
fn rows_that_were_not_wrapped_are_what_they_were_and_a_font_without_rules_reads_nothing() {
    let on = renderer_with(OBEL_MONO, &[("ss01", true)]);
    for surface in [Surface::Grid, Surface::Band] {
        let one = frame_of(&on, surface, KEY, false, true);
        let two = wrapped_frame(&on, surface, &[line(KEY, false), line(KEY, false)], "");
        let (once, twice) = (shaped_places(&one, surface), shaped_places(&two, surface));
        assert_eq!(twice, [once.clone(), once].concat(), "{surface:?}");
        for at in 0..KEY.len() {
            assert_eq!(
                place(&on, &two, surface, 1, at),
                column(&on, &one, surface, at),
                "{surface:?}: column {at} of the second row"
            );
        }
    }
    let menlo = plain_renderer();
    for surface in SURFACES {
        let frame = wrapped_frame(
            &menlo,
            surface,
            &[line(HEAD, false), line(TAIL, true)],
            HEAD,
        );
        assert!(shaped_places(&frame, surface).iter().all(Option::is_none));
    }
    assert_eq!(menlo.shape_calls(), 0, "the shaper was asked for Menlo");
}

#[test]
fn a_ligature_font_joins_its_pairs_unless_the_user_turned_it_off() {
    let on = renderer_with(JETBRAINS_MONO, &[]);
    let off = renderer_with(JETBRAINS_MONO, &[("calt", false)]);
    // `a->b != c`: `->` is columns 1 and 2, `!=` columns 5 and 6.
    let text = "a->b != c";
    let joined_cols = [1, 2, 5, 6];
    for surface in SURFACES {
        let joined = frame_of(&on, surface, text, false, true);
        let unshaped = frame_of(&on, surface, text, false, false);
        let places = shaped_places(&joined, surface);
        assert_eq!(places.len(), text.len(), "{surface:?}");
        for (col, place) in places.iter().enumerate() {
            let is_joined = joined_cols.contains(&col);
            assert_eq!(place.is_some(), is_joined, "{surface:?}: column {col}");
            assert_eq!(
                column(&on, &joined, surface, col) != column(&on, &unshaped, surface, col),
                is_joined,
                "{surface:?}: column {col}"
            );
        }
        // The cells of one pair are the two slices of one glyph.
        let (first, second) = (places[1].expect("a slice"), places[2].expect("a slice"));
        assert_eq!(
            (first.glyph, first.slice, first.cells),
            (second.glyph, 0, 2),
            "{surface:?}"
        );
        assert_eq!((second.slice, second.cells), (1, 2), "{surface:?}");
        // `calt` off, in the file: the characters.
        let apart = frame_of(&off, surface, text, false, true);
        assert!(
            shaped_places(&apart, surface).iter().all(Option::is_none),
            "{surface:?}: calt off still joins"
        );
        for col in 0..text.len() {
            assert_eq!(
                column(&off, &apart, surface, col),
                column(&on, &unshaped, surface, col),
                "{surface:?}: column {col}"
            );
        }
    }
}

/// Whether every row of `frame`'s surface that is lit across most of `cells`
/// cells is lit in one piece: a bar that crosses a cell's edge has no gap in
/// it. `None` when no row is lit that wide.
fn bars_are_unbroken(r: &Renderer, frame: &Frame, cells: usize) -> Option<bool> {
    let pixels = r.render_offscreen(EDGE as u32, BACKGROUND, frame);
    let (cw, ch) = r.cell_metrics(1.0).cell_px();
    let (cw, ch) = (usize::from(cw), usize::from(ch));
    let ground = pixel_at(&pixels, EDGE, EDGE - 1, EDGE - 1);
    let mut wide = false;
    for y in 0..ch {
        let lit: Vec<usize> = (0..cw * cells)
            .filter(|&x| pixel_at(&pixels, EDGE, x, y) != ground)
            .collect();
        if lit.len() < 2 * cw {
            continue;
        }
        wide = true;
        if lit[lit.len() - 1] - lit[0] + 1 != lit.len() {
            return Some(false);
        }
    }
    wide.then_some(true)
}

#[test]
fn a_joined_bar_has_no_seam_at_the_cell_edge_whatever_the_letter_spacing() {
    for letter in [1.0, 1.2, 0.9] {
        let on = renderer_spaced(JETBRAINS_MONO, &[], letter);
        let joined = frame_of(&on, Surface::Grid, "===", false, true);
        assert!(
            shaped_places(&joined, Surface::Grid)
                .iter()
                .all(Option::is_some),
            "{letter}: the three cells are one shape"
        );
        assert_eq!(
            bars_are_unbroken(&on, &joined, 3),
            Some(true),
            "{letter}: a bar of the joined shape has a gap"
        );
        // The same cells drawn as characters have gaps between the bars: the
        // check sees a seam.
        let apart = frame_of(&on, Surface::Grid, "===", false, false);
        assert_eq!(
            bars_are_unbroken(&on, &apart, 3),
            Some(false),
            "{letter}: three separate bars are not one"
        );
    }
}

#[test]
fn each_cell_of_a_ligature_keeps_its_own_colour() {
    let r = renderer_with(JETBRAINS_MONO, &[]);
    let (cw, ch) = r.cell_metrics(1.0).cell_px();
    let red = bt_core::LinearRgba::from_srgb(0xff, 0x00, 0x00);
    let blue = bt_core::LinearRgba::from_srgb(0x00, 0x00, 0xff);
    let mut frame = Frame::default();
    frame.clear(grid(cw, ch), CaretStyle::default());
    let (cw, ch) = (usize::from(cw), usize::from(ch));
    for (at, (c, fg)) in [('-', red), ('>', blue)].into_iter().enumerate() {
        frame.push(Cell {
            fg,
            ..glyph_cell(u16::try_from(at).expect("two cells"), c, None)
        });
    }
    frame.shape_grid(|cells, above| r.shape(cells, above));
    assert!(frame.glyphs().iter().all(|cell| cell.glyph.is_some()));
    let pixels = r.render_offscreen(EDGE as u32, BACKGROUND, &frame);
    let ground = pixel_at(&pixels, EDGE, EDGE - 1, EDGE - 1);
    let tones = |col: usize| -> Vec<(u8, u8, u8)> {
        (0..ch)
            .flat_map(|y| (0..cw).map(move |x| (x, y)))
            .map(|(x, y)| pixel_at(&pixels, EDGE, col * cw + x, y))
            .filter(|px| *px != ground)
            .collect()
    };
    let (left, right) = (tones(0), tones(1));
    assert!(!left.is_empty() && !right.is_empty(), "both cells have ink");
    assert!(left.iter().all(|px| px.0 > px.2), "the first cell is red");
    assert!(
        right.iter().all(|px| px.2 > px.0),
        "the second cell is blue"
    );
}

#[test]
fn the_cursor_on_a_ligature_shows_its_characters_and_leaves_the_rest() {
    let r = renderer_with(JETBRAINS_MONO, &[]);
    for surface in [Surface::Grid, Surface::Dock] {
        let unshaped = frame_of(&r, surface, "a->b", false, false);
        let unjoin = |frame: &mut Frame, col: u16| match surface {
            Surface::Dock => frame.unjoin_dock(0, col),
            _ => frame.unjoin_grid(0, col),
        };
        let reference = shaped_places(&frame_of(&r, surface, "a->b", false, true), surface);
        assert!(reference[1].is_some() && reference[2].is_some());
        // Either cell of the pair: both are the characters, the pixels are
        // what no shaping draws.
        for col in [1, 2] {
            let mut frame = frame_of(&r, surface, "a->b", false, true);
            unjoin(&mut frame, col);
            assert!(
                shaped_places(&frame, surface).iter().all(Option::is_none),
                "{surface:?}: cursor on {col}"
            );
            for at in 0..4 {
                assert_eq!(
                    column(&r, &frame, surface, at),
                    column(&r, &unshaped, surface, at),
                    "{surface:?}: cursor on {col}, column {at}"
                );
            }
        }
        // Beside it, past the text: the pair stays joined.
        for col in [0, 3, 9] {
            let mut frame = frame_of(&r, surface, "a->b", false, true);
            unjoin(&mut frame, col);
            assert_eq!(
                shaped_places(&frame, surface),
                reference,
                "{surface:?}: {col}"
            );
        }
    }
    // A masked letter is one cell: the cursor does not open it.
    let obel = renderer_with(OBEL_MONO, &[("ss01", true)]);
    let mut frame = frame_of(&obel, Surface::Grid, KEY, false, true);
    let before = shaped_places(&frame, Surface::Grid);
    for col in 0..KEY.len() {
        frame.unjoin_grid(0, u16::try_from(col).expect("a short key"));
    }
    assert_eq!(shaped_places(&frame, Surface::Grid), before);
}

#[test]
fn a_pair_split_by_a_wrap_is_not_joined() {
    let r = renderer_with(JETBRAINS_MONO, &[]);
    for surface in [Surface::Grid, Surface::Band] {
        // `-` ends one row and `>` begins the row the terminal wrapped onto.
        let wrapped = wrapped_frame(&r, surface, &[line("a-", false), line(">b", true)], "");
        assert!(
            shaped_places(&wrapped, surface).iter().all(Option::is_none),
            "{surface:?}"
        );
        // The same pair inside one row is joined.
        let inside = wrapped_frame(&r, surface, &[line("a->b", false)], "");
        assert!(shaped_places(&inside, surface)[1].is_some(), "{surface:?}");
        // The row begins mid-line and the pair is the row's own.
        let after = wrapped_frame(&r, surface, &[line("->b", true)], "xx");
        assert!(shaped_places(&after, surface)[0].is_some(), "{surface:?}");
        // The pair's first cell is in the text above the row.
        let above = wrapped_frame(&r, surface, &[line(">b", true)], "a-");
        assert!(
            shaped_places(&above, surface).iter().all(Option::is_none),
            "{surface:?}"
        );
    }
}

#[test]
fn a_chosen_tag_the_font_lacks_is_a_notice_until_it_is_fixed() {
    use crate::FontNotice;
    let gaps = |r: &Renderer| -> Vec<FontNotice> {
        r.font_notices()
            .into_iter()
            .filter(|notice| matches!(notice, FontNotice::NoFeature { .. }))
            .collect()
    };
    let typo = renderer_with(OBEL_MONO, &[("ss01", true), ("ss99", true)]);
    assert_eq!(
        gaps(&typo),
        [FontNotice::NoFeature {
            family: OBEL_MONO.to_owned(),
            tag: "ss99".to_owned()
        }]
    );
    // Fixing the line is another atlas and says nothing.
    assert!(gaps(&renderer_with(OBEL_MONO, &[("ss01", true)])).is_empty());
    // The default chain without a choice has nothing to say, and nothing
    // before an atlas exists.
    assert!(plain_renderer().font_notices().is_empty());
    assert!(Renderer::new().font_notices().is_empty());
}

/// The glyph numbers of `text` in a page's line set in the terminal's face (`Obel Mono`).
fn page_glyphs(r: &Renderer, text: &str) -> Vec<u32> {
    let font = r
        .ui_font(&bt_atlas::TextStyle {
            family: bt_atlas::Family::Terminal(Some(OBEL_MONO.to_owned())),
            size_px: 26.0,
            weight: 400,
            italic: false,
        })
        .expect("the terminal's face opens");
    let line = r.ui_shape(font, text).expect("shapes");
    line.glyphs.iter().map(|glyph| glyph.glyph).collect()
}

/// A program's page set in the terminal's face is drawn by the person's choices for that face,
/// as the grid is: Hide Secrets masks a key there too, whether the page opened before the choice
/// was made or after, and the change tells whoever keeps lines or font ids to build them again.
#[test]
fn a_pages_text_in_the_terminals_face_follows_its_rules() {
    let r = renderer_with(OBEL_MONO, &[]);
    let shown = page_glyphs(&r, KEY);
    let _ = r.ui_flushed();
    let generation = r.ui_fonts_generation();

    let masking = |r: &Renderer| {
        let options = FontOptions {
            family: Some(OBEL_MONO.to_owned()),
            features: [(OBEL_MONO.to_lowercase(), [("ss01".to_owned(), true)].into())].into(),
            ..FontOptions::default()
        };
        assert!(r.set_font(&options));
    };
    masking(&r);
    assert!(r.ui_flushed(), "a list built before holds the plain key");
    assert_ne!(
        r.ui_fonts_generation(),
        generation,
        "a kept font id is stale"
    );
    let masked = page_glyphs(&r, KEY);
    assert_ne!(masked, shown, "the key in the page is masked");

    // A renderer that opens its page's text only after the choice gets it as well.
    let late = renderer_with(OBEL_MONO, &[("ss01", true)]);
    assert_eq!(page_glyphs(&late, KEY), masked);
}
