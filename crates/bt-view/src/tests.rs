//! The engine's guards: where boxes stand, what a line of text does when it does not fit,
//! and what the draw list holds. The host's measuring is a fixed-width stand-in.

use crate::{
    Align, DrawList, Family, Font, Measure, Metrics, Paint, Rect, Size, Tone, Track, div, layout,
    spacer, spark, text,
};

/// Every character is `0.5 × size` wide and the line `1.25 × size` tall: round numbers.
struct Fixed;

impl Measure for Fixed {
    fn measure(&mut self, font: &Font, text: &str) -> Metrics {
        Metrics {
            width: text.chars().count() as f32 * font.size * 0.5,
            height: font.size * 1.25,
        }
    }
}

fn rects(root: crate::Element, w: f32, h: f32, scale: f32) -> Vec<Rect> {
    layout(root, Size::new(w * scale, h * scale), scale, &mut Fixed)
        .rects()
        .collect()
}

fn paint(root: crate::Element, w: f32, h: f32, scale: f32) -> DrawList {
    layout(root, Size::new(w * scale, h * scale), scale, &mut Fixed).paint(&mut Fixed)
}

fn runs(list: &DrawList) -> Vec<&crate::Run> {
    list.items
        .iter()
        .filter_map(|item| match item {
            Paint::Text(run) => Some(run),
            Paint::Quad(_) => None,
        })
        .collect()
}

fn quads(list: &DrawList) -> Vec<&crate::Quad> {
    list.items
        .iter()
        .filter_map(|item| match item {
            Paint::Quad(quad) => Some(quad),
            Paint::Text(_) => None,
        })
        .collect()
}

#[test]
fn a_row_puts_its_children_side_by_side_inside_its_padding() {
    let root = div()
        .row()
        .p(10.0)
        .gap(4.0)
        .child(div().size(20.0, 30.0))
        .child(div().size(40.0, 10.0));
    let boxes = rects(root, 200.0, 100.0, 1.0);
    assert_eq!(boxes[0], Rect::new(0.0, 0.0, 200.0, 100.0));
    assert_eq!(boxes[1], Rect::new(10.0, 10.0, 20.0, 30.0));
    assert_eq!(boxes[2], Rect::new(34.0, 10.0, 40.0, 10.0));
}

#[test]
fn a_column_is_the_default_and_stretches_its_children() {
    let root = div().child(div().h(10.0)).child(div().h(20.0));
    let boxes = rects(root, 80.0, 100.0, 1.0);
    assert_eq!(boxes[1], Rect::new(0.0, 0.0, 80.0, 10.0));
    assert_eq!(boxes[2], Rect::new(0.0, 10.0, 80.0, 20.0));
}

#[test]
fn a_spacer_pushes_what_follows_it_to_the_end() {
    let root = div()
        .row()
        .child(div().size(10.0, 10.0))
        .child(spacer())
        .child(div().size(10.0, 10.0));
    let boxes = rects(root, 100.0, 10.0, 1.0);
    assert_eq!(boxes[1].x, 0.0);
    assert_eq!(boxes[3].x, 90.0);
}

#[test]
fn items_are_centred_on_the_cross_axis() {
    let root = div().row().items_center().child(div().size(10.0, 20.0));
    let boxes = rects(root, 100.0, 100.0, 1.0);
    assert_eq!(boxes[1], Rect::new(0.0, 40.0, 10.0, 20.0));
}

#[test]
fn a_grid_shares_what_the_fixed_tracks_leave_by_fr_and_keeps_its_minimum() {
    let cell = || div().h(10.0);
    let root = div()
        .grid([
            Track::px(20.0),
            Track::range(30.0, 1.0),
            Track::range(30.0, 3.0),
        ])
        .gap_x(10.0)
        .child(cell())
        .child(cell())
        .child(cell());
    // 200 − 20 − 2×10 = 160, shared 1:3.
    let boxes = rects(root, 200.0, 50.0, 1.0);
    assert_eq!(boxes[1], Rect::new(0.0, 0.0, 20.0, 10.0));
    assert_eq!(boxes[2], Rect::new(30.0, 0.0, 40.0, 10.0));
    assert_eq!(boxes[3], Rect::new(80.0, 0.0, 120.0, 10.0));

    // Too narrow for the shares: each range keeps its minimum.
    let root = div()
        .grid([
            Track::px(20.0),
            Track::range(30.0, 1.0),
            Track::range(30.0, 3.0),
        ])
        .gap_x(10.0)
        .child(cell())
        .child(cell())
        .child(cell());
    let boxes = rects(root, 100.0, 50.0, 1.0);
    assert_eq!(boxes[2].w, 30.0);
    assert_eq!(boxes[3].w, 30.0);
}

#[test]
fn a_page_written_in_points_is_laid_out_in_pixels() {
    let root = div()
        .row()
        .p(10.0)
        .child(div().size(20.0, 30.0).rounded(4.0).border(1.0, Tone::Line));
    let list = {
        let laid = layout(root, Size::new(400.0, 200.0), 2.0, &mut Fixed);
        assert_eq!(laid.rects().nth(1), Some(Rect::new(20.0, 20.0, 40.0, 60.0)));
        laid.paint(&mut Fixed)
    };
    let quad = quads(&list)[0];
    assert_eq!(quad.radii, [8.0; 4]);
    assert_eq!(quad.border.map(|b| b.width), Some(2.0));
}

#[test]
fn text_takes_what_the_host_says_and_inherits_the_boxes_around_it() {
    let root = div()
        .text_size(10.0)
        .weight(600)
        .color(Tone::Dim)
        .family(Family::System)
        .child(text("abcd"))
        .child(text("ef").text_size(20.0).weight(400));
    let list = paint(root, 100.0, 100.0, 2.0);
    let runs = runs(&list);
    assert_eq!(runs.len(), 2);
    // 10 pt at 2 px a point.
    assert_eq!(runs[0].font.size, 20.0);
    assert_eq!(runs[0].font.weight, 600);
    assert_eq!(runs[0].font.family, Family::System);
    assert_eq!(runs[0].color.tone, Tone::Dim);
    assert_eq!(runs[1].font.size, 40.0);
    assert_eq!(runs[1].font.weight, 400);
    assert_eq!(
        runs[1].color.tone,
        Tone::Dim,
        "the colour is still the box's"
    );
    // The second line sits under the first, whose box is 1.25 × 20 px tall.
    assert_eq!(runs[0].origin.y, 0.0);
    assert_eq!(runs[1].origin.y, 25.0);
}

#[test]
fn a_line_that_may_be_cut_ends_in_an_ellipsis_where_it_does_not_fit() {
    // 12.5 pt at one px a point: 6.25 px a character, a 50 px box fits 8 characters.
    let root = div()
        .row()
        .child(text("abcdefghijklmnop").truncate().w(50.0));
    let list = paint(root, 200.0, 20.0, 1.0);
    let run = runs(&list)[0];
    assert_eq!(run.text, "abcdefg\u{2026}");
    assert_eq!(run.clip.w, 50.0, "and it stays in its box");
}

#[test]
fn a_line_that_fits_is_left_whole_and_one_that_may_not_be_cut_is_not() {
    let root = div()
        .row()
        .child(text("abc").truncate().w(50.0))
        .child(text("abcdefghijklmnop").w(50.0));
    let list = paint(root, 200.0, 20.0, 1.0);
    let runs = runs(&list);
    assert_eq!(runs[0].text, "abc");
    assert_eq!(runs[1].text, "abcdefghijklmnop");
}

#[test]
fn a_line_that_may_be_cut_gives_its_room_away_before_the_others() {
    let root = div()
        .row()
        .w(100.0)
        .child(text("a very long name indeed").truncate())
        .child(div().w(40.0).no_shrink());
    let boxes = rects(root, 100.0, 20.0, 1.0);
    assert_eq!(boxes[1].w, 60.0, "what the fixed box leaves");
    assert_eq!(boxes[2].w, 40.0);
}

#[test]
fn nothing_of_a_line_is_drawn_where_not_even_the_ellipsis_fits() {
    let root = div()
        .row()
        .child(text("abcdefghijklmnop").truncate().w(3.0));
    let list = paint(root, 200.0, 20.0, 1.0);
    assert!(runs(&list).is_empty());
}

#[test]
fn a_line_stands_at_the_end_or_the_middle_of_its_box() {
    let root = div()
        .w(100.0)
        .child(text("abcd").align(Align::End))
        .child(text("abcd").align(Align::Center))
        .child(text("abcd"));
    let list = paint(root, 100.0, 100.0, 1.0);
    let runs = runs(&list);
    // Four characters, 25 px.
    assert_eq!(runs[0].origin.x, 75.0);
    assert_eq!(runs[1].origin.x, 37.5);
    assert_eq!(runs[2].origin.x, 0.0);
}

#[test]
fn a_hidden_box_takes_no_room_and_draws_nothing_nor_do_its_children() {
    let root = div()
        .child(div().h(10.0).bg(Tone::Accent))
        .child(
            div()
                .hidden()
                .h(10.0)
                .bg(Tone::Error)
                .child(text("gone").color(Tone::Error)),
        )
        .child(div().h(10.0).bg(Tone::Success));
    let laid = layout(root, Size::new(50.0, 50.0), 1.0, &mut Fixed);
    // The root, the first box, the hidden one and its line, the last box.
    assert_eq!(laid.rects().nth(4), Some(Rect::new(0.0, 10.0, 50.0, 10.0)));
    let list = laid.paint(&mut Fixed);
    assert_eq!(quads(&list).len(), 2);
    assert!(runs(&list).is_empty());
}

#[test]
fn a_box_that_clips_cuts_what_is_drawn_inside_it() {
    let root = div().child(
        div()
            .size(30.0, 20.0)
            .clip()
            .child(div().size(100.0, 100.0).bg(Tone::Accent)),
    );
    let list = paint(root, 100.0, 100.0, 1.0);
    let quad = quads(&list)[0];
    assert_eq!(quad.rect.w, 100.0, "the child is as big as it was written");
    assert_eq!(quad.clip, Rect::new(0.0, 0.0, 30.0, 20.0));
}

#[test]
fn a_scrolling_box_moves_its_content_behind_it() {
    let rows = |offset: f32| {
        div()
            .size(50.0, 40.0)
            .scroll_y(offset)
            .child(div().h(30.0).no_shrink().bg(Tone::Accent))
            .child(div().h(30.0).no_shrink().bg(Tone::Success))
            .child(div().h(30.0).no_shrink().bg(Tone::Error))
    };
    let at = |offset: f32| {
        let list = paint(div().child(rows(offset)), 100.0, 100.0, 1.0);
        quads(&list).iter().map(|q| q.rect.y).collect::<Vec<_>>()
    };
    assert_eq!(at(0.0), [0.0, 30.0, 60.0]);
    assert_eq!(at(20.0), [-20.0, 10.0, 40.0]);
    let list = paint(div().child(rows(20.0)), 100.0, 100.0, 1.0);
    assert!(
        quads(&list)
            .iter()
            .all(|q| q.clip == Rect::new(0.0, 0.0, 50.0, 40.0)),
        "all of it is cut at the box"
    );
}

#[test]
fn an_absolute_box_is_placed_against_its_parent_and_takes_no_room() {
    let root = div()
        .p(8.0)
        .child(div().h(10.0))
        .child(
            div()
                .absolute()
                .size(20.0, 20.0)
                .inset(Some(0.0), Some(0.0), None, None),
        )
        .child(div().h(10.0));
    let boxes = rects(root, 100.0, 100.0, 1.0);
    assert_eq!(boxes[2], Rect::new(80.0, 0.0, 20.0, 20.0));
    assert_eq!(boxes[3], Rect::new(8.0, 18.0, 84.0, 10.0));
}

#[test]
fn a_sparkline_stays_in_its_box_and_ends_at_the_newest_value() {
    let values = [1.0, 4.0, 2.0, 8.0, 3.0, 5.0];
    let root = div().child(spark(&values, Tone::Accent).size(60.0, 20.0));
    let list = paint(root, 100.0, 100.0, 1.0);
    let quads = quads(&list);
    assert!(!quads.is_empty());
    let right = quads.iter().map(|q| q.rect.right()).fold(0.0_f32, f32::max);
    assert!(
        (right - 60.0).abs() < 0.01,
        "the newest value is at the right edge"
    );
    for quad in &quads {
        assert!(quad.rect.x >= 0.0 && quad.rect.bottom() <= 21.0);
        assert_eq!(quad.clip, Rect::new(0.0, 0.0, 60.0, 20.0));
    }
}

#[test]
fn a_host_with_no_text_gets_the_boxes_and_no_words() {
    struct Mute;
    impl Measure for Mute {
        fn measure(&mut self, _: &Font, _: &str) -> Metrics {
            Metrics::default()
        }
    }
    let root = div()
        .row()
        .child(div().size(10.0, 10.0).bg(Tone::Accent))
        .child(text("words"));
    let laid = layout(root, Size::new(100.0, 20.0), 1.0, &mut Mute);
    let list = laid.paint(&mut Mute);
    assert_eq!(quads(&list).len(), 1);
    assert!(runs(&list).is_empty());
}
