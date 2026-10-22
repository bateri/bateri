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
use super::ui::{UiList, UiQuad, UiRect, tint};
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
