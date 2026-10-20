//! The pictures programs send, drawn: where they land, what colour they
//! come out, which layer they are in, and what is cut. Offscreen, the way the
//! cell tests read what the GPU really painted.

use std::sync::Arc;

use bt_core::{CaretStyle, LinearRgba, Picture, Pictures};

use super::tests::{
    ACCENT, bg_cell, fitting_cell_px, glyph_cell, grid, grid_with_gutter, pixel_at,
    render_offscreen, renderer,
};
use super::*;

const CELL: u16 = 8;
const RED: [u8; 4] = [0xff, 0x00, 0x00, 0xff];
const GREEN: [u8; 4] = [0x00, 0xff, 0x00, 0xff];
const BLUE: [u8; 4] = [0x00, 0x00, 0xff, 0xff];

/// The pixel the clear paints: the accent, a midtone, so ±1.
fn clear() -> (u8, u8, u8) {
    let hex = bt_core::Theme::BATERI.accent;
    ((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

fn near(seen: (u8, u8, u8), want: (u8, u8, u8)) -> bool {
    seen.0.abs_diff(want.0) <= 1 && seen.1.abs_diff(want.1) <= 1 && seen.2.abs_diff(want.2) <= 1
}

/// A picture of `texels` (`size` wide and high) drawn `dest` pixels big with
/// its first cell at (`col`, `row`).
fn picture(
    image: u64,
    texels: &[[u8; 4]],
    size: (u32, u32),
    (col, row): (u16, i32),
    dest: (u32, u32),
    z: i32,
) -> Picture {
    Picture {
        col,
        row,
        x_offset: 0,
        y_offset: 0,
        width: dest.0,
        height: dest.1,
        source: [0, 0, size.0, size.1],
        z,
        image,
        size,
        pixels: Arc::new(texels.concat()),
    }
}

/// A one-colour picture.
fn solid(image: u64, rgba: [u8; 4], at: (u16, i32), dest: (u32, u32), z: i32) -> Picture {
    picture(image, &[rgba], (1, 1), at, dest, z)
}

/// A frame of `cols` × `rows` cells of `cell` pixels with these pictures.
fn frame_of(cell: (u16, u16), (cols, rows): (u16, u16), grid_pictures: Vec<Picture>) -> Frame {
    let mut frame = Frame::default();
    frame.clear(grid(cell.0, cell.1), CaretStyle::default());
    frame.set_grid(cols, rows);
    frame.put_pictures(Pictures::new(grid_pictures, Vec::new()));
    frame
}

fn cached(r: &Renderer) -> usize {
    r.state.borrow().images.textures.len()
}

#[test]
fn a_picture_paints_its_rectangle_in_the_colour_it_was_sent() {
    let r = renderer();
    const EDGE: usize = 32;
    // Two cells wide, one tall, from the second column of the second row.
    // A picture's generation names its pixels: another colour is another
    // generation, as it is when a program sends an id again.
    let draw = |image: u64, texel: [u8; 4]| {
        let frame = frame_of(
            (CELL, CELL),
            (4, 4),
            vec![solid(image, texel, (1, 1), (16, 8), 0)],
        );
        render_offscreen(&r, EDGE, ACCENT, &frame)
    };
    let pixels = draw(1, RED);
    let at = |x, y| pixel_at(&pixels, EDGE, x, y);
    for (x, y) in [(8, 8), (23, 15), (15, 12)] {
        assert_eq!(at(x, y), (0xff, 0, 0), "inside, at ({x}, {y})");
    }
    for (x, y) in [(7, 12), (24, 12), (12, 7), (12, 16), (0, 0)] {
        assert!(
            near(at(x, y), clear()),
            "outside, at ({x}, {y}): {:?}",
            at(x, y)
        );
    }

    // **A midtone**: the picture's bytes are sRGB, the texture decodes them
    // and the target encodes them again. A texture that skipped the decode
    // would come out lighter, and red and the primaries — the fixed points of
    // the transfer — would not show it.
    let pixels = draw(2, [128, 128, 128, 255]);
    let seen = pixel_at(&pixels, EDGE, 12, 12);
    assert!(near(seen, (128, 128, 128)), "{seen:?}");

    // Straight alpha: no coverage leaves the ground as it was.
    let pixels = draw(3, [255, 0, 0, 0]);
    let seen = pixel_at(&pixels, EDGE, 12, 12);
    assert!(near(seen, clear()), "{seen:?}");
}

#[test]
fn a_picture_rides_the_origin_with_the_text() {
    let r = renderer();
    const EDGE: usize = 16;
    // Two columns, the picture in the first: a picture across the grid would
    // lend its edges to the padding above, which is not this test's subject.
    let mut frame = frame_of((CELL, CELL), (2, 1), vec![solid(1, RED, (0, 0), (8, 8), 0)]);
    // The content is one row pinned to the bottom: the row is the second.
    frame.set_origin_rows(1.0);
    let pixels = render_offscreen(&r, EDGE, ACCENT, &frame);
    assert_eq!(pixel_at(&pixels, EDGE, 4, 12), (0xff, 0, 0));
    assert!(near(pixel_at(&pixels, EDGE, 4, 4), clear()));
    // The motion frame: the list is kept, only the origin moves.
    frame.set_origin_rows(0.5);
    let pixels = render_offscreen(&r, EDGE, ACCENT, &frame);
    assert_eq!(pixel_at(&pixels, EDGE, 4, 6), (0xff, 0, 0));
    assert!(near(pixel_at(&pixels, EDGE, 4, 2), clear()));
    assert!(near(pixel_at(&pixels, EDGE, 4, 13), clear()));
}

#[test]
fn a_picture_is_cut_to_the_grid_and_not_stretched_into_it() {
    let r = renderer();
    const EDGE: usize = 32;
    // Starts a row above the grid, in its second column, and runs past its
    // right edge and its bottom: only the 16 × 16 pixels of the last 2 × 2
    // cells show. Not from the first column: a picture across the grid lends
    // its edges to the padding ([`Frame::picture_padding`]), and the cut is
    // this test's subject.
    let frame = frame_of(
        (CELL, CELL),
        (3, 2),
        vec![solid(1, RED, (1, -1), (32, 32), 0)],
    );
    let pixels = render_offscreen(&r, EDGE, ACCENT, &frame);
    for (x, y) in [(8, 0), (23, 15), (16, 8)] {
        assert_eq!(pixel_at(&pixels, EDGE, x, y), (0xff, 0, 0), "({x}, {y})");
    }
    for (x, y) in [(4, 4), (24, 4), (12, 16), (28, 20), (31, 31)] {
        assert!(
            near(pixel_at(&pixels, EDGE, x, y), clear()),
            "({x}, {y}) is outside the grid"
        );
    }

    // The cut takes its share of the texture, it does not squeeze the whole
    // texture into what is left: four texels, red, red, red, green, over 32
    // pixels; the grid is one cell (8 pixels) wide, so only the first quarter
    // shows — red to the last pixel. Squeezed, green would be at its edge.
    let texels = [RED, RED, RED, GREEN];
    let frame = frame_of(
        (CELL, CELL),
        (1, 1),
        vec![picture(2, &texels, (4, 1), (0, 0), (32, 8), 0)],
    );
    let pixels = render_offscreen(&r, EDGE, ACCENT, &frame);
    for x in [0, 4, 7] {
        assert_eq!(pixel_at(&pixels, EDGE, x, 4), (0xff, 0, 0), "x = {x}");
    }
}

#[test]
fn a_picture_across_the_grid_paints_the_padding_with_its_edges() {
    let r = renderer();
    const EDGE: usize = 32;
    // Two columns of 8 after a 4-pixel gutter, two rows from the top: the
    // grid spans x 4..20 and y 0..16, the padding is the gutter, the 12
    // pixels at the right and the 16 below. The picture is red | green.
    let mut frame = Frame::default();
    frame.clear(grid_with_gutter(CELL, CELL, 4), CaretStyle::default());
    frame.set_grid(2, 2);
    frame.put_pictures(Pictures::new(
        vec![picture(1, &[RED, GREEN], (2, 1), (0, 0), (16, 16), 0)],
        Vec::new(),
    ));
    let pixels = render_offscreen(&r, EDGE, ACCENT, &frame);
    let at = |x, y| pixel_at(&pixels, EDGE, x, y);
    for (x, y, want, what) in [
        (1, 8, (0xff, 0, 0), "the gutter, from the first column"),
        (28, 8, (0, 0xff, 0), "the right, from the last column"),
        (1, 30, (0xff, 0, 0), "the corner below the gutter"),
        (30, 30, (0, 0xff, 0), "the corner at the right"),
    ] {
        assert_eq!(at(x, y), want, "{what}, at ({x}, {y})");
    }
    // Below, each column is the grid's last pixel row: two texels over 16
    // pixels are a ramp from red to green (the sampler is linear), and the
    // strip carries the ramp, not one colour.
    for x in 4..20 {
        assert_eq!(at(x, 24), at(x, 15), "below the grid, at x = {x}");
    }
    assert_ne!(at(4, 24), at(19, 24), "the strip is the picture's row");
}

#[test]
fn the_padding_stands_in_the_layers_order() {
    let r = renderer();
    const EDGE: usize = 32;
    // Both rows painted blue end to end, and a red picture across them: under
    // the backgrounds the cells lend the padding, over them the picture does.
    let blue = LinearRgba::from_srgb(0, 0, 0xff);
    let gutter = |z: i32| {
        let mut frame = Frame::default();
        frame.clear(grid_with_gutter(CELL, CELL, 4), CaretStyle::default());
        frame.set_grid(2, 2);
        for (col, row) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            frame.push(bg_cell(col, row, blue));
        }
        frame.put_pictures(Pictures::new(
            vec![solid(1, RED, (0, 0), (16, 16), z)],
            Vec::new(),
        ));
        let pixels = render_offscreen(&r, EDGE, ACCENT, &frame);
        pixel_at(&pixels, EDGE, 1, 8)
    };
    assert_eq!(gutter(-1_073_741_825), (0, 0, 0xff), "the ground layer");
    assert_eq!(
        gutter(-1),
        (0xff, 0, 0),
        "between the backgrounds and the text"
    );
    assert_eq!(gutter(0), (0xff, 0, 0), "over the text");
}

#[test]
fn the_part_of_a_picture_that_shows_is_the_source_rectangle() {
    let r = renderer();
    const EDGE: usize = 16;
    // Two texels, red and green; the source is the second, drawn one cell big.
    let mut shown = picture(1, &[RED, GREEN], (2, 1), (0, 0), (8, 8), 0);
    shown.source = [1, 0, 1, 1];
    let frame = frame_of((CELL, CELL), (2, 2), vec![shown]);
    let pixels = render_offscreen(&r, EDGE, ACCENT, &frame);
    assert_eq!(pixel_at(&pixels, EDGE, 4, 4), (0, 0xff, 0));
}

#[test]
fn the_layers_stand_between_the_backgrounds_and_over_the_text() {
    let r = renderer();
    const EDGE: usize = 64;
    let (cw, ch) = fitting_cell_px(&r, EDGE, 2);
    let red = LinearRgba::from_srgb(0xff, 0x00, 0x00);
    let cell = |z: i32| {
        let picture = solid(1, BLUE, (0, 0), (u32::from(cw), u32::from(ch)), z);
        let mut frame = frame_of((cw, ch), (2, 1), vec![picture]);
        // A red cell with an `M` in it: a background and ink.
        frame.push(glyph_cell(0, 'M', Some(red)));
        let pixels = render_offscreen(&r, EDGE, ACCENT, &frame);
        super::tests::cell_rows(&pixels, EDGE, (cw, ch), 0).concat()
    };
    let blue = (0, 0, 0xff);
    let has = |cell: &[(u8, u8, u8)], want: (u8, u8, u8)| cell.contains(&want);

    // Below `-1 073 741 824`: under the background, so the red shows and the
    // picture does not.
    let under = cell(-1_073_741_825);
    assert!(has(&under, (0xff, 0, 0)), "the background is covered");
    assert!(!has(&under, blue), "the picture is over the background");

    // The other negatives: over the background, under the ink — blue where
    // the cell is empty, the letter still there, no red left.
    let between = cell(-1);
    assert!(
        has(&between, blue),
        "the picture is not over the background"
    );
    assert!(!has(&between, (0xff, 0, 0)), "the background shows through");
    assert!(
        between.iter().any(|&(r, g, b)| g > 0 && r > 0 && b > 0),
        "the letter is covered"
    );

    // Zero and above: over everything. The cell is blue, bare.
    let over = cell(0);
    assert!(over.iter().all(|&pixel| pixel == blue), "{over:?}");
}

#[test]
fn the_fill_band_has_its_own_pictures_cut_at_its_own_edge() {
    let r = renderer();
    const EDGE: usize = 16;
    // One row of grid, pushed to the bottom, and one row of band above it.
    let mut frame = Frame::default();
    frame.clear(grid(CELL, CELL), CaretStyle::default());
    frame.set_grid(1, 1);
    frame.set_fill_rows(1);
    frame.set_origin_rows(1.0);
    // The band's picture is two cells tall: the second would lie over the
    // grid's row, and it is not the band's to draw there.
    frame.put_pictures(Pictures::new(
        vec![solid(1, RED, (0, 0), (8, 8), 0)],
        vec![solid(2, GREEN, (0, 0), (8, 16), 0)],
    ));
    let pixels = render_offscreen(&r, EDGE, ACCENT, &frame);
    assert_eq!(
        pixel_at(&pixels, EDGE, 4, 4),
        (0, 0xff, 0),
        "the band's row"
    );
    assert_eq!(
        pixel_at(&pixels, EDGE, 4, 12),
        (0xff, 0, 0),
        "the grid's row, with the band's picture past its edge"
    );
}

#[test]
fn a_texture_lives_as_long_as_its_picture_shows() {
    let r = renderer();
    const EDGE: usize = 16;
    let with = frame_of((CELL, CELL), (2, 2), vec![solid(1, RED, (0, 0), (8, 8), 0)]);
    let without = frame_of((CELL, CELL), (2, 2), Vec::new());
    assert_eq!(cached(&r), 0);
    render_offscreen(&r, EDGE, ACCENT, &with);
    assert_eq!(cached(&r), 1);
    // Drawn again — a motion frame, say — it is the same texture.
    render_offscreen(&r, EDGE, ACCENT, &with);
    assert_eq!(cached(&r), 1);
    // Cut out of every row on screen, it is let go; so is one the frame no
    // longer lists.
    let off = frame_of((CELL, CELL), (2, 2), vec![solid(1, RED, (0, 8), (8, 8), 0)]);
    render_offscreen(&r, EDGE, ACCENT, &off);
    assert_eq!(cached(&r), 0);
    render_offscreen(&r, EDGE, ACCENT, &with);
    render_offscreen(&r, EDGE, ACCENT, &without);
    assert_eq!(cached(&r), 0);
}

#[test]
fn a_picture_the_device_cannot_hold_is_left_out_not_failed() {
    let r = renderer();
    const EDGE: usize = 16;
    // Past any device's texture side, and bytes that are not 4 × w × h: each
    // would be a validation error that fails the frame if it reached the
    // device. The picture beside them is drawn.
    let wide = u32::from(u16::MAX) * 4;
    let mut huge = picture(1, &[RED], (1, 1), (0, 0), (8, 8), 0);
    huge.size = (wide, 1);
    huge.source = [0, 0, wide, 1];
    huge.pixels = Arc::new(vec![0xff; wide as usize * 4]);
    let mut short = picture(2, &[RED], (1, 1), (0, 0), (8, 8), 0);
    short.size = (2, 2);
    short.source = [0, 0, 2, 2];
    let fine = solid(3, GREEN, (1, 1), (8, 8), 0);
    let frame = frame_of((CELL, CELL), (2, 2), vec![huge, short, fine]);
    let pixels = render_offscreen(&r, EDGE, ACCENT, &frame);
    assert!(near(pixel_at(&pixels, EDGE, 4, 4), clear()));
    assert_eq!(pixel_at(&pixels, EDGE, 12, 12), (0, 0xff, 0));
    assert_eq!(cached(&r), 1);
}

#[test]
fn a_frame_without_pictures_plans_what_it_did_before_them() {
    let r = renderer();
    let mut frame = Frame::default();
    frame.clear(grid(CELL, CELL), CaretStyle::default());
    frame.set_grid(2, 2);
    frame.push(bg_cell(0, 0, LinearRgba::from_srgb(0, 0xff, 0)));
    let mut state = r.state.borrow_mut();
    let state = &mut *state;
    r.plan(
        &frame,
        [16.0, 16.0],
        &mut state.atlas,
        &mut state.images,
        &mut state.plan,
    )
    .expect("a plan");
    assert!(
        !state
            .plan
            .ops
            .iter()
            .any(|op| matches!(op, Op::Image { .. })),
        "{:?}",
        state.plan.ops
    );
}
