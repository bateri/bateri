//! The UI kit's offscreen guards: the boxes' corners, border, shadow and clip
//! on the GPU, and the proportional text's place and colour.
//!
//! Colours are **mid-tones** wherever the claim is about the colour space
//! (`0.0` and `1.0` are the sRGB transfer function's fixed points and cannot
//! tell a missing linearisation from a present one): the box tests clear to
//! [`MIDTONE`], and the translucent fill is checked against the value the
//! blend must produce in linear space.

use bt_atlas::{Family, TextStyle};
use bt_core::LinearRgba;

use super::tests::{BACKGROUND, MIDTONE, WHITE, pixel_at, renderer};
use super::ui::{UiBitmap, UiImage, UiList, UiQuad, UiRect, tint};
use crate::Renderer;

const EDGE: usize = 64;

/// sRGB bytes of a colour, for comparing a read-back pixel with.
fn bytes(color: LinearRgba) -> (u8, u8, u8) {
    let [r, g, b, _] = color.to_array();
    let encode = |c: f32| {
        let c = c.clamp(0.0, 1.0);
        let s = if c <= 0.003_130_8 {
            c * 12.92
        } else {
            1.055 * c.powf(1.0 / 2.4) - 0.055
        };
        (s * 255.0).round() as u8
    };
    (encode(r), encode(g), encode(b))
}

fn near(a: (u8, u8, u8), b: (u8, u8, u8), by: u8) -> bool {
    a.0.abs_diff(b.0) <= by && a.1.abs_diff(b.1) <= by && a.2.abs_diff(b.2) <= by
}

fn paint(clear: LinearRgba, list: &UiList) -> Vec<u8> {
    renderer().render_ui_offscreen(EDGE as u32, clear, list)
}

fn at(pixels: &[u8], x: usize, y: usize) -> (u8, u8, u8) {
    pixel_at(pixels, EDGE, x, y)
}

#[test]
fn a_box_paints_its_fill_in_linear_space_and_leaves_the_rest() {
    // The fill is a mid-tone on a different mid-tone: the sRGB round trip of
    // both must come back as written.
    let fill = LinearRgba::from_srgb(0x5a, 0x5d, 0x65);
    let mut list = UiList::new();
    list.quad(UiQuad::new(UiRect::new(8.0, 8.0, 16.0, 16.0)).fill(tint(fill, 1.0)));
    let pixels = paint(MIDTONE, &list);
    assert!(near(at(&pixels, 16, 16), (0x5a, 0x5d, 0x65), 1));
    // The edge pixels belong to the box, the ones beside them do not.
    assert!(near(at(&pixels, 8, 8), (0x5a, 0x5d, 0x65), 1));
    assert!(near(at(&pixels, 23, 23), (0x5a, 0x5d, 0x65), 1));
    assert!(near(at(&pixels, 7, 16), (0x1a, 0x1c, 0x21), 1));
    assert!(near(at(&pixels, 24, 16), (0x1a, 0x1c, 0x21), 1));
    assert!(near(at(&pixels, 40, 40), (0x1a, 0x1c, 0x21), 1));
}

#[test]
fn a_translucent_fill_blends_in_linear_space() {
    // White at half alpha over black: the blend is 0.5 in LINEAR light, which
    // the target encodes to sRGB 0xbc — not the 0x80 a blend in encoded space
    // would give.
    let mut list = UiList::new();
    list.quad(UiQuad::new(UiRect::new(8.0, 8.0, 16.0, 16.0)).fill(tint(WHITE, 0.5)));
    let pixels = paint(BACKGROUND, &list);
    let expected = bytes(LinearRgba::from_srgb(0xff, 0xff, 0xff));
    assert_eq!(expected, (255, 255, 255));
    let got = at(&pixels, 16, 16);
    assert!(near(got, (0xbc, 0xbc, 0xbc), 2), "got {got:?}");
}

#[test]
fn a_fill_from_one_colour_to_another_runs_along_its_angle_and_mixes_in_srgb() {
    // Black to white from the left to the right (90 degrees), over a red fill it covers.
    let black = LinearRgba::from_srgb(0, 0, 0);
    let red = LinearRgba::from_srgb(0xff, 0, 0);
    let mut list = UiList::new();
    list.quad(
        UiQuad::new(UiRect::new(0.0, 0.0, 64.0, 16.0))
            .fill(tint(red, 1.0))
            .gradient(90.0, tint(black, 1.0), tint(WHITE, 1.0)),
    );
    let pixels = paint(MIDTONE, &list);
    let left = at(&pixels, 0, 8);
    let middle = at(&pixels, 32, 8);
    let right = at(&pixels, 63, 8);
    assert!(near(left, (2, 2, 2), 3), "left {left:?}");
    assert!(near(right, (253, 253, 253), 3), "right {right:?}");
    // Half way is half of sRGB's white, as a style sheet mixes: 0x80, not the 0xbc a mix in
    // linear light would come to.
    assert!(near(middle, (0x80, 0x80, 0x80), 3), "middle {middle:?}");
    // From the top down (180), the same at every point of a row.
    let mut list = UiList::new();
    list.quad(UiQuad::new(UiRect::new(0.0, 0.0, 64.0, 64.0)).gradient(
        180.0,
        tint(black, 1.0),
        tint(WHITE, 1.0),
    ));
    let pixels = paint(MIDTONE, &list);
    assert_eq!(at(&pixels, 4, 32), at(&pixels, 60, 32));
    assert!(at(&pixels, 4, 4).0 < at(&pixels, 4, 60).0);
}

#[test]
fn a_rounded_corner_cuts_the_corner_and_keeps_the_edges() {
    let mut list = UiList::new();
    list.quad(
        UiQuad::new(UiRect::new(8.0, 8.0, 32.0, 32.0))
            .fill(tint(WHITE, 1.0))
            .radius(12.0),
    );
    let pixels = paint(MIDTONE, &list);
    let clear = (0x1a, 0x1c, 0x21);
    // The square corner's pixel is outside the rounded shape ...
    assert!(near(at(&pixels, 8, 8), clear, 1), "{:?}", at(&pixels, 8, 8));
    assert!(near(at(&pixels, 39, 8), clear, 1));
    assert!(near(at(&pixels, 8, 39), clear, 1));
    assert!(near(at(&pixels, 39, 39), clear, 1));
    // ... the middle of every side and the centre are in it.
    assert_eq!(at(&pixels, 8, 24), (255, 255, 255));
    assert_eq!(at(&pixels, 39, 24), (255, 255, 255));
    assert_eq!(at(&pixels, 24, 8), (255, 255, 255));
    assert_eq!(at(&pixels, 24, 39), (255, 255, 255));
    assert_eq!(at(&pixels, 24, 24), (255, 255, 255));
}

#[test]
fn each_corner_has_its_own_radius() {
    let mut list = UiList::new();
    // Only the bottom-right corner is round.
    list.quad(
        UiQuad::new(UiRect::new(8.0, 8.0, 32.0, 32.0))
            .fill(tint(WHITE, 1.0))
            .radii([0.0, 0.0, 14.0, 0.0]),
    );
    let pixels = paint(MIDTONE, &list);
    let clear = (0x1a, 0x1c, 0x21);
    assert_eq!(at(&pixels, 8, 8), (255, 255, 255));
    assert_eq!(at(&pixels, 39, 8), (255, 255, 255));
    assert_eq!(at(&pixels, 8, 39), (255, 255, 255));
    assert!(near(at(&pixels, 39, 39), clear, 1));
}

#[test]
fn a_border_is_drawn_inside_the_box_over_the_fill() {
    let fill = LinearRgba::from_srgb(0x5a, 0x5d, 0x65);
    let border = LinearRgba::from_srgb(0xc8, 0x40, 0x40);
    let mut list = UiList::new();
    list.quad(
        UiQuad::new(UiRect::new(8.0, 8.0, 32.0, 32.0))
            .fill(tint(fill, 1.0))
            .border(2.0, tint(border, 1.0)),
    );
    let pixels = paint(MIDTONE, &list);
    // The two outermost pixel columns are the border, the third is the fill.
    assert!(near(at(&pixels, 8, 24), (0xc8, 0x40, 0x40), 1));
    assert!(near(at(&pixels, 9, 24), (0xc8, 0x40, 0x40), 1));
    assert!(near(at(&pixels, 10, 24), (0x5a, 0x5d, 0x65), 1));
    assert!(near(at(&pixels, 24, 24), (0x5a, 0x5d, 0x65), 1));
    assert!(near(at(&pixels, 24, 39), (0xc8, 0x40, 0x40), 1));
    // Nothing outside the box.
    assert!(near(at(&pixels, 7, 24), (0x1a, 0x1c, 0x21), 1));
}

#[test]
fn a_shadow_falls_off_with_distance_and_is_never_under_the_box() {
    let mut list = UiList::new();
    list.quad(
        UiQuad::new(UiRect::new(16.0, 12.0, 32.0, 20.0))
            .fill(tint(WHITE, 1.0))
            .shadow(0.0, 4.0, 10.0, tint(BACKGROUND, 0.9)),
    );
    let pixels = paint(MIDTONE, &list);
    let clear = at(&pixels, 2, 2);
    assert!(
        near(clear, (0x1a, 0x1c, 0x21), 1),
        "the corner is untouched"
    );
    // Below the box the shadow darkens the clear, and less the further it is.
    let below = |y: usize| usize::from(at(&pixels, 32, y).0);
    assert!(
        below(33) < usize::from(clear.0),
        "darker right below the box"
    );
    assert!(below(33) <= below(38));
    assert!(below(38) <= below(44));
    assert!(below(44) <= usize::from(clear.0));
    assert!(
        below(33) < below(44),
        "the falloff is real, not a flat tone"
    );
    // The offset moves it: the shadow is heavier below than above.
    let above = usize::from(at(&pixels, 32, 10).0);
    assert!(below(33) < above, "below {} above {}", below(33), above);
    // Inside the box the box is what shows.
    assert_eq!(at(&pixels, 32, 22), (255, 255, 255));
}

#[test]
fn a_transparent_box_does_not_show_its_own_shadow_through_itself() {
    let mut list = UiList::new();
    list.quad(UiQuad::new(UiRect::new(16.0, 16.0, 32.0, 32.0)).shadow(
        0.0,
        0.0,
        12.0,
        tint(BACKGROUND, 1.0),
    ));
    let pixels = paint(MIDTONE, &list);
    assert!(
        near(at(&pixels, 32, 32), (0x1a, 0x1c, 0x21), 1),
        "the box's own area is clear: {:?}",
        at(&pixels, 32, 32)
    );
    assert!(
        at(&pixels, 12, 32).0 < 0x1a,
        "the shadow shows outside the box"
    );
}

#[test]
fn a_clip_stops_the_box_at_its_edge() {
    let mut list = UiList::new();
    list.quad(
        UiQuad::new(UiRect::new(8.0, 8.0, 48.0, 48.0))
            .fill(tint(WHITE, 1.0))
            .clip(UiRect::new(0.0, 0.0, 30.0, 64.0)),
    );
    let pixels = paint(MIDTONE, &list);
    assert_eq!(at(&pixels, 29, 30), (255, 255, 255));
    assert!(near(at(&pixels, 31, 30), (0x1a, 0x1c, 0x21), 1));
}

#[test]
fn later_items_cover_earlier_ones() {
    let red = LinearRgba::from_srgb(0xc8, 0x40, 0x40);
    let mut list = UiList::new();
    list.quad(UiQuad::new(UiRect::new(8.0, 8.0, 24.0, 24.0)).fill(tint(WHITE, 1.0)));
    list.quad(UiQuad::new(UiRect::new(16.0, 16.0, 24.0, 24.0)).fill(tint(red, 1.0)));
    let pixels = paint(MIDTONE, &list);
    assert_eq!(at(&pixels, 10, 10), (255, 255, 255));
    assert!(near(at(&pixels, 24, 24), (0xc8, 0x40, 0x40), 1));
    assert!(near(at(&pixels, 38, 38), (0xc8, 0x40, 0x40), 1));
}

/// The proportional text tests need a backend that has proportional text; on
/// one that does not they say so and stop (the box tests above run on both).
fn proportional() -> bool {
    if !Renderer::ui_text_supported() {
        eprintln!("skipped: this backend has no proportional text");
    }
    Renderer::ui_text_supported()
}

const SHEET: usize = 128;

/// The rectangle of pixels that differ from `clear` inside `within` (left,
/// top, right, bottom; the right and bottom exclusive): left, top, right,
/// bottom of the ink (inclusive), or `None` for a blank area.
fn ink_box(
    pixels: &[u8],
    clear: (u8, u8, u8),
    within: (usize, usize, usize, usize),
) -> Option<(usize, usize, usize, usize)> {
    let mut found: Option<(usize, usize, usize, usize)> = None;
    for y in within.1..within.3 {
        for x in within.0..within.2 {
            if pixel_at(pixels, SHEET, x, y) != clear {
                found = Some(match found {
                    None => (x, y, x, y),
                    Some((l, t, r, b)) => (l.min(x), t.min(y), r.max(x), b.max(y)),
                });
            }
        }
    }
    found
}

fn style(family: Family, size_px: f64, weight: u16) -> TextStyle {
    TextStyle {
        family,
        size_px,
        weight,
        italic: false,
    }
}

#[test]
fn text_lands_inside_the_line_it_was_measured_as_and_in_its_colour() {
    if !proportional() {
        return;
    }
    let r = renderer();
    let font = r
        .ui_font(&style(Family::System, 32.0, 400))
        .expect("the system face opens");
    let line = r.ui_shape(font, "HHHH").expect("shapes");
    let origin = [10.0_f32, 12.0];
    let mut list = UiList::new();
    let width = r.ui_text(
        &mut list,
        &line,
        origin,
        tint(WHITE, 1.0),
        UiRect::UNBOUNDED,
    );
    assert!((f64::from(width) - line.width).abs() < 0.01);
    let pixels = r.render_ui_offscreen(SHEET as u32, BACKGROUND, &list);
    let (left, top, right, bottom) =
        ink_box(&pixels, (0, 0, 0), (0, 0, SHEET, SHEET)).expect("the text painted something");
    // The ink stays inside the line box ...
    assert!(left as f64 >= f64::from(origin[0]) - 2.0, "left {left}");
    assert!(
        right as f64 <= f64::from(origin[0]) + line.width + 2.0,
        "right {right} for width {}",
        line.width
    );
    assert!(top as f64 >= f64::from(origin[1]) - 1.0, "top {top}");
    assert!(
        bottom as f64 <= f64::from(origin[1]) + line.height() + 1.0,
        "bottom {bottom} for height {}",
        line.height()
    );
    // ... and fills the width it was measured at (a cap letter has little
    // side bearing), so measuring and drawing are one font.
    assert!(
        (right - left) as f64 >= line.width * 0.8,
        "ink {} wide for a line measured {}",
        right - left,
        line.width
    );
    // A stem is solid: some pixel is exactly the text colour.
    let solid = (0..SHEET)
        .flat_map(|y| (0..SHEET).map(move |x| (x, y)))
        .any(|(x, y)| pixel_at(&pixels, SHEET, x, y) == (255, 255, 255));
    assert!(solid, "no pixel reached the text colour");
}

#[test]
fn text_is_drawn_over_the_box_behind_it() {
    if !proportional() {
        return;
    }
    let r = renderer();
    let font = r
        .ui_font(&style(Family::System, 32.0, 700))
        .expect("the system face opens");
    let line = r.ui_shape(font, "II").expect("shapes");
    let mut list = UiList::new();
    list.quad(UiQuad::new(UiRect::new(4.0, 4.0, 120.0, 56.0)).fill(tint(MIDTONE, 1.0)));
    r.ui_text(
        &mut list,
        &line,
        [10.0, 10.0],
        tint(WHITE, 1.0),
        UiRect::UNBOUNDED,
    );
    let pixels = r.render_ui_offscreen(SHEET as u32, BACKGROUND, &list);
    let behind = pixel_at(&pixels, SHEET, 100, 50);
    assert!(
        near(behind, (0x1a, 0x1c, 0x21), 1),
        "the box shows: {behind:?}"
    );
    let (left, top, right, bottom) =
        ink_box(&pixels, (0x1a, 0x1c, 0x21), (4, 4, 124, 60)).expect("text over the box");
    let _ = (top, bottom);
    assert!(left >= 8 && right < 60, "the text is where it was put");
    let solid = (4..60)
        .flat_map(|y| (4..120).map(move |x| (x, y)))
        .any(|(x, y)| pixel_at(&pixels, SHEET, x, y) == (255, 255, 255));
    assert!(solid, "the text colour is on top of the box");
}

#[test]
fn text_is_clipped() {
    if !proportional() {
        return;
    }
    let r = renderer();
    let font = r
        .ui_font(&style(Family::System, 32.0, 400))
        .expect("the system face opens");
    let line = r.ui_shape(font, "HHHHHH").expect("shapes");
    let mut list = UiList::new();
    r.ui_text(
        &mut list,
        &line,
        [4.0, 4.0],
        tint(WHITE, 1.0),
        UiRect::new(0.0, 0.0, 40.0, 128.0),
    );
    let pixels = r.render_ui_offscreen(SHEET as u32, BACKGROUND, &list);
    let (_, _, right, _) = ink_box(&pixels, (0, 0, 0), (0, 0, SHEET, SHEET)).expect("some ink");
    assert!(right < 40, "nothing past the clip: {right}");
}

#[test]
fn a_heavier_weight_paints_more_ink() {
    if !proportional() {
        return;
    }
    let r = renderer();
    let ink = |weight: u16| {
        let font = r
            .ui_font(&style(Family::System, 40.0, weight))
            .expect("opens");
        let line = r.ui_shape(font, "Il").expect("shapes");
        let mut list = UiList::new();
        r.ui_text(
            &mut list,
            &line,
            [8.0, 8.0],
            tint(WHITE, 1.0),
            UiRect::UNBOUNDED,
        );
        let pixels = r.render_ui_offscreen(SHEET as u32, BACKGROUND, &list);
        (0..SHEET)
            .flat_map(|y| (0..SHEET).map(move |x| (x, y)))
            .map(|(x, y)| u32::from(pixel_at(&pixels, SHEET, x, y).0))
            .sum::<u32>()
    };
    let (light, bold) = (ink(300), ink(800));
    assert!(bold > light, "bold {bold} against light {light}");
}

#[test]
fn the_carets_of_a_line_are_where_its_shaping_puts_its_characters() {
    if !proportional() {
        return;
    }
    let r = renderer();
    let font = r
        .ui_font(&style(Family::System, 20.0, 400))
        .expect("the system face opens");
    // Before each character and at the end; a character of two UTF-16 units and one of three
    // bytes are counted as the line counts them.
    let text = "Ab😀ç";
    let at = [0, 1, 2, 6, text.len()];
    let carets = r.ui_carets(font, text, &at).expect("carets");
    let whole = r.ui_shape(font, text).expect("shapes").width;
    assert_eq!(carets.len(), at.len());
    assert!(carets[0].abs() < 0.01, "{carets:?}");
    assert!(
        carets.windows(2).all(|pair| pair[0] < pair[1]),
        "{carets:?}"
    );
    assert!(
        (carets[4] - whole).abs() < 0.5,
        "{carets:?} against {whole}"
    );
    // The caret after `A` is where the line of `A` alone ends (no kerning across one letter).
    let one = r.ui_shape(font, "A").expect("shapes").width;
    assert!((carets[1] - one).abs() < 0.5, "{carets:?} against {one}");
    // A place inside a character is not one: the line's end.
    let inside = r.ui_carets(font, text, &[3]).expect("carets");
    assert!((inside[0] - whole).abs() < 0.5, "{inside:?}");
}

// ---------------------------------------------------------------------------
// Images
// ---------------------------------------------------------------------------

/// One pixel as a decoder hands it over: sRGB, premultiplied by alpha.
fn texel(r: u8, g: u8, b: u8, a: u8) -> [u8; 4] {
    let times = |c: u8| ((u16::from(c) * u16::from(a) + 127) / 255) as u8;
    [times(r), times(g), times(b), a]
}

fn bitmap(id: u64, width: u32, pixels: &[[u8; 4]]) -> (u64, u32, u32, Vec<u8>) {
    (id, width, pixels.len() as u32 / width, pixels.concat())
}

/// Puts `(id, width, height, pixels)` into `list` as `image`.
fn put(r: &Renderer, list: &mut UiList, bitmap: &(u64, u32, u32, Vec<u8>), image: UiImage) -> bool {
    r.ui_image(
        list,
        &UiBitmap {
            id: bitmap.0,
            width: bitmap.1,
            height: bitmap.2,
            pixels: &bitmap.3,
        },
        image,
    )
}

const RED: (u8, u8, u8) = (0xc8, 0x40, 0x40);
const GREEN: (u8, u8, u8) = (0x40, 0xa0, 0x50);
const BLUE: (u8, u8, u8) = (0x40, 0x60, 0xd0);
const CREAM: (u8, u8, u8) = (0xe0, 0xd0, 0x90);
const GROUND: (u8, u8, u8) = (0x1a, 0x1c, 0x21);

fn opaque((r, g, b): (u8, u8, u8)) -> [u8; 4] {
    texel(r, g, b, 255)
}

#[test]
fn an_image_lands_in_its_rectangle_in_its_own_colours() {
    // Mid-tones throughout: a missing sRGB decode (or a double one) moves every one of them.
    let r = renderer();
    let image = bitmap(
        1,
        2,
        &[opaque(RED), opaque(GREEN), opaque(BLUE), opaque(CREAM)],
    );
    let mut list = UiList::new();
    // Pixel for texel: each pixel centre is a texel centre, so nothing is filtered.
    assert!(put(
        &r,
        &mut list,
        &image,
        UiImage::new(UiRect::new(10.0, 12.0, 2.0, 2.0))
    ));
    let pixels = r.render_ui_offscreen(EDGE as u32, MIDTONE, &list);
    assert!(
        near(at(&pixels, 10, 12), RED, 1),
        "{:?}",
        at(&pixels, 10, 12)
    );
    assert!(near(at(&pixels, 11, 12), GREEN, 1));
    assert!(near(at(&pixels, 10, 13), BLUE, 1));
    assert!(near(at(&pixels, 11, 13), CREAM, 1));
    // And only there.
    for (x, y) in [(9, 12), (12, 12), (10, 11), (11, 14), (40, 40)] {
        assert!(near(at(&pixels, x, y), GROUND, 1), "({x}, {y})");
    }
}

#[test]
fn a_turned_image_is_the_image_turned_clockwise_about_the_middle_of_its_rectangle() {
    let r = renderer();
    // Two texels side by side: red on the left, green on the right, in a rectangle 24 wide and 8
    // high about (32, 32).
    let image = bitmap(7, 2, &[opaque(RED), opaque(GREEN)]);
    let draw = |image_in: UiImage| {
        let mut list = UiList::new();
        assert!(put(&r, &mut list, &image, image_in));
        r.render_ui_offscreen(EDGE as u32, MIDTONE, &list)
    };
    let rect = UiRect::new(20.0, 28.0, 24.0, 8.0);
    let plain = draw(UiImage::new(rect));
    assert!(near(at(&plain, 24, 32), RED, 1));
    assert!(near(at(&plain, 40, 32), GREEN, 1));
    assert!(near(at(&plain, 32, 22), GROUND, 1));
    // A quarter turn clockwise: what was on the right is below, what was on the left above; the
    // rectangle stands up, 8 wide and 24 high, about the same middle.
    let turned = draw(UiImage::new(rect).turn(90.0));
    assert!(
        near(at(&turned, 32, 22), RED, 1),
        "{:?}",
        at(&turned, 32, 22)
    );
    assert!(
        near(at(&turned, 32, 42), GREEN, 1),
        "{:?}",
        at(&turned, 32, 42)
    );
    for (x, y) in [(22, 32), (42, 32), (26, 22), (38, 42), (32, 18), (32, 46)] {
        assert!(
            near(at(&turned, x, y), GROUND, 1),
            "({x}, {y}) is outside it"
        );
    }
    // A turn of a half is the image the other way round, in the same place.
    let half = draw(UiImage::new(rect).turn(180.0));
    assert!(near(at(&half, 24, 32), GREEN, 1));
    assert!(near(at(&half, 40, 32), RED, 1));
    // The clip does not turn: what is outside it is not drawn, turned or not.
    let cut = draw(
        UiImage::new(rect)
            .turn(90.0)
            .clip(UiRect::new(0.0, 0.0, 64.0, 32.0)),
    );
    assert!(near(at(&cut, 32, 22), RED, 1));
    assert!(near(at(&cut, 32, 42), GROUND, 1));
}

#[test]
fn an_image_that_is_half_transparent_blends_in_linear_light_not_darker() {
    // Premultiplied sRGB: the stored colour is `encoded × alpha`. Read as an sRGB texture it
    // would be decoded as if it were an encoded colour and every soft edge would come out
    // dark; the blend must be the colour at half alpha over the ground, in linear light.
    let r = renderer();
    let half = texel(RED.0, RED.1, RED.2, 0x80);
    let image = bitmap(2, 1, &[half]);
    let mut list = UiList::new();
    assert!(put(
        &r,
        &mut list,
        &image,
        UiImage::new(UiRect::new(8.0, 8.0, 8.0, 8.0))
    ));
    let pixels = r.render_ui_offscreen(EDGE as u32, MIDTONE, &list);
    let expected = bytes(LinearRgba::from_srgb(RED.0, RED.1, RED.2).over(128.0 / 255.0, MIDTONE));
    let got = at(&pixels, 12, 12);
    assert!(near(got, expected, 3), "got {got:?}, expected {expected:?}");
    // The wrong reading (decode the product) is further than that from the right one.
    let wrong =
        bytes(LinearRgba::from_srgb(half[0], half[1], half[2]).over(128.0 / 255.0, MIDTONE));
    assert!(!near(wrong, expected, 8), "the guard cannot tell {wrong:?}");
}

#[test]
fn a_tint_paints_the_shape_with_the_images_alpha() {
    let r = renderer();
    // Opaque, clear, half, opaque: the colours the file had are of no matter.
    let image = bitmap(
        3,
        4,
        &[
            opaque(RED),
            texel(0, 0, 0, 0),
            texel(GREEN.0, GREEN.1, GREEN.2, 0x80),
            opaque(BLUE),
        ],
    );
    let paint_in = LinearRgba::from_srgb(CREAM.0, CREAM.1, CREAM.2);
    let mut list = UiList::new();
    assert!(put(
        &r,
        &mut list,
        &image,
        UiImage::new(UiRect::new(8.0, 8.0, 4.0, 1.0)).tint(tint(paint_in, 1.0))
    ));
    let pixels = r.render_ui_offscreen(EDGE as u32, MIDTONE, &list);
    assert!(near(at(&pixels, 8, 8), CREAM, 1), "{:?}", at(&pixels, 8, 8));
    assert!(
        near(at(&pixels, 9, 8), GROUND, 1),
        "a clear texel paints nothing"
    );
    let half = bytes(paint_in.over(128.0 / 255.0, MIDTONE));
    assert!(
        near(at(&pixels, 10, 8), half, 3),
        "{:?}",
        at(&pixels, 10, 8)
    );
    assert!(near(at(&pixels, 11, 8), CREAM, 1));
}

#[test]
fn a_cover_crop_shows_the_middle_of_the_image_and_leaves_the_rest_out() {
    let r = renderer();
    // Four texels across; the rectangle is two pixels, cut to the middle two.
    let image = bitmap(
        4,
        4,
        &[opaque(RED), opaque(GREEN), opaque(BLUE), opaque(CREAM)],
    );
    let mut list = UiList::new();
    assert!(put(
        &r,
        &mut list,
        &image,
        UiImage::new(UiRect::new(8.0, 8.0, 2.0, 1.0)).crop(UiRect::new(0.25, 0.0, 0.5, 1.0))
    ));
    let pixels = r.render_ui_offscreen(EDGE as u32, MIDTONE, &list);
    assert!(near(at(&pixels, 8, 8), GREEN, 1), "{:?}", at(&pixels, 8, 8));
    assert!(near(at(&pixels, 9, 8), BLUE, 1), "{:?}", at(&pixels, 9, 8));
    // Neither end of the file is anywhere: not in the rectangle, not at its antialiased edge.
    assert!(near(at(&pixels, 7, 8), GROUND, 1));
    assert!(near(at(&pixels, 10, 8), GROUND, 1));
}

#[test]
fn an_image_has_the_corners_clip_and_opacity_of_a_box() {
    let r = renderer();
    let white = bitmap(5, 1, &[opaque((255, 255, 255))]);
    let rounded = |image: UiImage| {
        let mut list = UiList::new();
        assert!(put(&r, &mut list, &white, image));
        r.render_ui_offscreen(EDGE as u32, MIDTONE, &list)
    };
    // Corners: the square corner's pixel is outside, the middle of a side and the centre in.
    let pixels = rounded(UiImage::new(UiRect::new(8.0, 8.0, 32.0, 32.0)).radii([12.0; 4]));
    assert!(
        near(at(&pixels, 8, 8), GROUND, 1),
        "{:?}",
        at(&pixels, 8, 8)
    );
    assert!(near(at(&pixels, 39, 39), GROUND, 1));
    assert_eq!(at(&pixels, 24, 8), (255, 255, 255));
    assert_eq!(at(&pixels, 24, 24), (255, 255, 255));
    // Clip: nothing past it.
    let pixels = rounded(
        UiImage::new(UiRect::new(8.0, 8.0, 32.0, 32.0)).clip(UiRect::new(0.0, 0.0, 20.0, 64.0)),
    );
    assert_eq!(at(&pixels, 19, 24), (255, 255, 255));
    assert!(near(at(&pixels, 21, 24), GROUND, 1));
    // Opacity: half of white over the ground, in linear light.
    let pixels = rounded(UiImage::new(UiRect::new(8.0, 8.0, 32.0, 32.0)).opacity(0.5));
    let expected = bytes(WHITE.over(0.5, MIDTONE));
    assert!(
        near(at(&pixels, 24, 24), expected, 2),
        "{:?}",
        at(&pixels, 24, 24)
    );
}

#[test]
fn an_images_texture_lives_with_the_lists_that_name_it() {
    let r = renderer();
    let held = |r: &Renderer| r.ui_images_held();
    let one = bitmap(6, 1, &[opaque(RED)]);
    let other = bitmap(7, 1, &[opaque(GREEN)]);
    let rect = UiRect::new(8.0, 8.0, 4.0, 4.0);

    let mut list = UiList::new();
    assert!(put(&r, &mut list, &one, UiImage::new(rect)));
    assert!(put(&r, &mut list, &other, UiImage::new(rect)));
    r.render_ui_offscreen(EDGE as u32, MIDTONE, &list);
    assert_eq!(held(&r), 2);

    // A frame that names one of them lets the other go (an SVG at a new size is a new id).
    let mut list = UiList::new();
    assert!(put(&r, &mut list, &other, UiImage::new(rect)));
    r.render_ui_offscreen(EDGE as u32, MIDTONE, &list);
    assert_eq!(held(&r), 1);

    // The page going away lets go of the rest, drawn or not.
    r.ui_release_images();
    assert_eq!(held(&r), 0);
}

#[test]
fn pixels_that_are_not_a_texture_are_left_out_of_the_list() {
    let r = renderer();
    let mut list = UiList::new();
    let short = UiBitmap {
        id: 8,
        width: 2,
        height: 2,
        pixels: &[0; 12],
    };
    assert!(!r.ui_image(
        &mut list,
        &short,
        UiImage::new(UiRect::new(0.0, 0.0, 4.0, 4.0))
    ));
    let empty = UiBitmap {
        id: 9,
        width: 0,
        height: 0,
        pixels: &[],
    };
    assert!(!r.ui_image(
        &mut list,
        &empty,
        UiImage::new(UiRect::new(0.0, 0.0, 4.0, 4.0))
    ));
    assert!(list.is_empty());
}

#[test]
fn a_crop_blown_up_reads_nothing_beyond_its_edge() {
    let r = renderer();
    // Four texels across, the rectangle cut to the middle two and drawn eight pixels to a texel:
    // the filter at its ends is half a texel from the red and the cream on the other side of the
    // cut, and must not take any of them.
    let image = bitmap(
        10,
        4,
        &[opaque(RED), opaque(GREEN), opaque(BLUE), opaque(CREAM)],
    );
    let mut list = UiList::new();
    assert!(put(
        &r,
        &mut list,
        &image,
        UiImage::new(UiRect::new(8.0, 8.0, 16.0, 2.0)).crop(UiRect::new(0.25, 0.0, 0.5, 1.0))
    ));
    let pixels = r.render_ui_offscreen(EDGE as u32, MIDTONE, &list);
    assert!(near(at(&pixels, 8, 8), GREEN, 1), "{:?}", at(&pixels, 8, 8));
    assert!(
        near(at(&pixels, 23, 8), BLUE, 1),
        "{:?}",
        at(&pixels, 23, 8)
    );
    // Across the cut's middle the two blend, and the colours of the file's ends are in none.
    for x in 8..24 {
        let (red, _, blue) = at(&pixels, x, 8);
        assert!(
            red <= GREEN.0 + 1 || blue >= GREEN.2,
            "({x}) leans toward the red end: {:?}",
            at(&pixels, x, 8)
        );
        assert!(
            at(&pixels, x, 8).0 <= RED.0.min(0xa0),
            "({x}) took the cream: {:?}",
            at(&pixels, x, 8)
        );
    }
}

#[test]
fn an_image_drawn_far_smaller_than_its_file_is_its_average_and_not_a_sample_of_it() {
    let r = renderer();
    // One-pixel black and white checks, 63 to a side, drawn 9 to a side: seven texels a pixel,
    // and every pixel's centre is a texel's centre, so read at the file's finest level each
    // pixel would be one check — black or white, and the other when the image moves by a
    // pixel. Its average is grey.
    let black = opaque((0, 0, 0));
    let white = opaque((255, 255, 255));
    let side = 63u32;
    let mut pixels = Vec::new();
    for y in 0..side {
        for x in 0..side {
            pixels.push(if (x + y) % 2 == 0 { black } else { white });
        }
    }
    let image = bitmap(11, side, &pixels);
    let mut list = UiList::new();
    assert!(put(
        &r,
        &mut list,
        &image,
        UiImage::new(UiRect::new(8.0, 8.0, 9.0, 9.0))
    ));
    let painted = r.render_ui_offscreen(EDGE as u32, MIDTONE, &list);
    for y in 8..17 {
        for x in 8..17 {
            // About half black and half white, which the shader decodes and the target
            // encodes again.
            assert!(
                near(at(&painted, x, y), (0x80, 0x80, 0x80), 24),
                "({x}, {y}): {:?}",
                at(&painted, x, y)
            );
        }
    }
    // At its own size the same file is the checks themselves: the chain is not read.
    let mut list = UiList::new();
    assert!(put(
        &r,
        &mut list,
        &image,
        UiImage::new(UiRect::new(0.0, 0.0, 63.0, 63.0))
    ));
    let painted = r.render_ui_offscreen(EDGE as u32, MIDTONE, &list);
    assert_eq!(at(&painted, 10, 10), (0, 0, 0));
    assert_eq!(at(&painted, 11, 10), (255, 255, 255));
}

#[test]
fn a_rectangle_with_no_area_paints_nothing_and_harms_nothing() {
    let r = renderer();
    let image = bitmap(12, 1, &[opaque(RED)]);
    for rect in [
        UiRect::new(20.0, 20.0, 0.0, 8.0),
        UiRect::new(20.0, 20.0, 8.0, 0.0),
        UiRect::new(20.0, 20.0, 0.0, 0.0),
    ] {
        let mut list = UiList::new();
        assert!(put(&r, &mut list, &image, UiImage::new(rect)));
        // A real one after it, to see that the empty one broke nothing.
        assert!(put(
            &r,
            &mut list,
            &image,
            UiImage::new(UiRect::new(40.0, 40.0, 4.0, 4.0))
        ));
        let pixels = r.render_ui_offscreen(EDGE as u32, MIDTONE, &list);
        for (x, y) in [(19, 20), (20, 20), (20, 24), (21, 21), (28, 28)] {
            assert!(near(at(&pixels, x, y), GROUND, 1), "{rect:?} ({x}, {y})");
        }
        assert!(near(at(&pixels, 42, 42), RED, 1));
    }
}
