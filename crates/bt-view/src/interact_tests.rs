//! The engine's guards for the pointer, the wheel, the keyboard and the names: what the point
//! hits, what a scrolling box reaches, how a wheel chains from one box to the box around it.
//! The host's measuring is a fixed-width stand-in.

use crate::{Font, Id, Laid, Measure, Metrics, Point, Rect, Size, Ui, div, layout, text};

struct Fixed;

impl Measure for Fixed {
    fn measure(&mut self, font: &Font, text: &str) -> Metrics {
        Metrics {
            width: text.chars().count() as f32 * font.size * 0.5,
            height: font.size * 1.25,
        }
    }
}

const LIST: Id = Id::new(1);
const OUTER: Id = Id::new(2);
const HEADER: Id = Id::new(3);

fn row(n: u64) -> crate::Element {
    div().h(20.0).no_shrink().press(Id::new(100 + n))
}

/// A header over a list of `rows` rows of 20 points that scrolls in what is left of a page of
/// 200 × 100, `offset` points down.
fn page(rows: u64, offset: f32, scale: f32) -> Laid {
    let list = div()
        .col()
        .id(LIST)
        .focusable()
        .grow(1.0)
        .min_h(0.0)
        .scroll_y(offset)
        .children((0..rows).map(row));
    let root = div()
        .col()
        .id(OUTER)
        .child(div().h(20.0).no_shrink().press(HEADER))
        .child(list);
    layout(
        root,
        Size::new(200.0 * scale, 100.0 * scale),
        scale,
        &mut Fixed,
    )
}

fn at(x: f32, y: f32) -> Point {
    Point::new(x, y)
}

#[test]
fn the_point_hits_the_topmost_element_that_answers_the_pointer() {
    let laid = page(10, 0.0, 1.0);
    assert_eq!(laid.hit(at(5.0, 5.0)), Some(HEADER));
    assert_eq!(laid.hit(at(5.0, 25.0)), Some(Id::new(100)));
    assert_eq!(laid.hit(at(5.0, 45.0)), Some(Id::new(101)));
    // Outside every pressable one: the list and the page are named but answer nothing.
    assert_eq!(laid.hit(at(5.0, 99.5)), Some(Id::new(103)));
    assert_eq!(laid.hit(at(250.0, 50.0)), None, "past the page");
}

#[test]
fn what_a_scrolled_box_has_pushed_out_of_sight_is_not_hit() {
    // The list is 80 points tall under the 20-point header; scrolled 30 down, the row that
    // stood at y 20..40 has gone under the header's edge and is not at its old place.
    let laid = page(10, 30.0, 1.0);
    assert_eq!(
        laid.hit(at(5.0, 21.0)),
        Some(Id::new(101)),
        "row 1 is 10 points in"
    );
    assert_eq!(
        laid.hit(at(5.0, 5.0)),
        Some(HEADER),
        "the header is still the header"
    );
    // Rows whose boxes lie above the list are cut off by it.
    let all: Vec<Rect> = laid.rects().collect();
    assert!(all.iter().any(|r| r.y < 20.0 && r.bottom() > 0.0));
    assert_eq!(laid.hit(at(5.0, 19.0)), Some(HEADER));
}

#[test]
fn points_are_points_at_any_scale() {
    for scale in [1.0, 2.0, 3.0] {
        let laid = page(10, 0.0, scale);
        assert_eq!(laid.hit(at(5.0, 25.0)), Some(Id::new(100)), "scale {scale}");
        assert_eq!(laid.bounds(HEADER), Some(Rect::new(0.0, 0.0, 200.0, 20.0)));
        assert_eq!(
            laid.bounds(Id::new(101)),
            Some(Rect::new(0.0, 40.0, 200.0, 20.0))
        );
    }
}

#[test]
fn a_scrolling_box_reports_its_content_and_its_own_length() {
    let laid = page(10, 0.0, 2.0);
    let extent = laid.scroll_extent(LIST).expect("the list scrolls");
    assert_eq!(extent.view, 80.0);
    assert_eq!(extent.content, 200.0, "ten rows of twenty");
    assert_eq!(extent.max_offset(), 120.0);
    // The same content seen after a scroll is the same length.
    let moved = page(10, 50.0, 2.0)
        .scroll_extent(LIST)
        .expect("the list scrolls");
    assert_eq!(moved.content, 200.0);
    // A list that fits has nothing to scroll.
    assert_eq!(
        page(2, 0.0, 1.0)
            .scroll_extent(LIST)
            .map(|e| e.max_offset()),
        Some(0.0)
    );
    // A box that does not scroll has no extent.
    assert_eq!(laid.scroll_extent(OUTER), None);
}

#[test]
fn the_wheel_moves_the_list_under_the_pointer_and_stops_at_its_ends() {
    let mut ui = Ui::default();
    let laid = page(10, 0.0, 1.0);
    assert!(ui.scroll_by(&laid, at(50.0, 60.0), 30.0));
    assert_eq!(ui.scroll_offset(LIST), 30.0);
    assert!(ui.scroll_by(&laid, at(50.0, 60.0), 500.0));
    assert_eq!(ui.scroll_offset(LIST), 120.0, "the end of the list");
    assert!(
        !ui.scroll_by(&laid, at(50.0, 60.0), 10.0),
        "nothing moved past the end"
    );
    assert!(ui.scroll_by(&laid, at(50.0, 60.0), -500.0));
    assert_eq!(ui.scroll_offset(LIST), 0.0);
    assert!(
        !ui.scroll_by(&laid, at(50.0, 60.0), -10.0),
        "nothing moved before the start"
    );
    // Over the header the list is not under the pointer.
    assert!(!ui.scroll_by(&laid, at(50.0, 5.0), 30.0));
    assert_eq!(ui.scroll_offset(LIST), 0.0);
}

#[test]
fn a_list_that_got_shorter_comes_back_within_its_reach() {
    let mut ui = Ui::default();
    let tall = page(10, 0.0, 1.0);
    ui.scroll_to(&tall, LIST, 120.0);
    assert_eq!(ui.scroll_offset(LIST), 120.0);
    // Four rows now: 80 points of content in 80 of box, nothing to scroll.
    let short = page(4, 120.0, 1.0);
    assert!(ui.settle(&short), "the offset had to move");
    assert_eq!(ui.scroll_offset(LIST), 0.0);
    assert!(!ui.settle(&short), "and once is enough");
}

#[test]
fn the_pointer_changes_the_hover_only_when_it_changes_the_element() {
    let mut ui = Ui::default();
    let laid = page(10, 0.0, 1.0);
    assert!(ui.point(&laid, Some(at(5.0, 25.0))));
    assert_eq!(ui.hover(), Some(Id::new(100)));
    assert!(!ui.point(&laid, Some(at(40.0, 30.0))), "still the same row");
    assert!(ui.point(&laid, Some(at(40.0, 45.0))));
    assert!(ui.point(&laid, None), "the pointer left");
    assert_eq!(ui.hover(), None);
    assert!(!ui.point(&laid, None));
}

#[test]
fn a_click_is_a_press_and_a_release_on_the_same_element() {
    let mut ui = Ui::default();
    let laid = page(10, 0.0, 1.0);
    assert_eq!(ui.press(&laid, at(5.0, 25.0)), Some(Id::new(100)));
    assert_eq!(ui.pressed(), Some(Id::new(100)));
    assert_eq!(ui.release(&laid, at(8.0, 28.0)), Some(Id::new(100)));
    assert_eq!(ui.pressed(), None);
    // Pressed on one row and released over another is not a click.
    ui.press(&laid, at(5.0, 25.0));
    assert_eq!(ui.release(&laid, at(5.0, 45.0)), None);
    // A release with no press is nothing.
    assert_eq!(ui.release(&laid, at(5.0, 45.0)), None);
}

#[test]
fn a_press_puts_the_keyboard_on_the_nearest_element_that_takes_it() {
    let mut ui = Ui::default();
    let laid = page(10, 0.0, 1.0);
    ui.press(&laid, at(5.0, 45.0));
    assert_eq!(
        ui.focus(),
        Some(LIST),
        "the row is in the list, and the list takes it"
    );
    // The header does not take the keyboard: it stays where it was.
    ui.press(&laid, at(5.0, 5.0));
    assert_eq!(ui.focus(), Some(LIST));
}

#[test]
fn the_keyboard_steps_around_the_elements_that_take_it() {
    let mut ui = Ui::default();
    let both = div()
        .child(div().h(10.0).id(Id::new(10)).focusable())
        .child(div().h(10.0).id(Id::new(11)).focusable())
        .child(div().h(10.0).id(Id::new(12)).hidden().focusable())
        .child(div().h(10.0).id(Id::new(13)));
    let laid = layout(both, Size::new(100.0, 100.0), 1.0, &mut Fixed);
    assert_eq!(laid.focus_order(), vec![Id::new(10), Id::new(11)]);
    assert_eq!(
        ui.focus_step(&laid, false),
        Some(Id::new(10)),
        "from nowhere: the first"
    );
    assert_eq!(ui.focus_step(&laid, false), Some(Id::new(11)));
    assert_eq!(ui.focus_step(&laid, false), Some(Id::new(10)), "around");
    assert_eq!(ui.focus_step(&laid, true), Some(Id::new(11)), "and back");
    let mut fresh = Ui::default();
    assert_eq!(
        fresh.focus_step(&laid, true),
        Some(Id::new(11)),
        "from nowhere back: the last"
    );
}

#[test]
fn a_key_walks_from_the_focused_element_out_to_the_boxes_around_it() {
    let mut ui = Ui::default();
    let root = div()
        .id(Id::new(1))
        .child(
            div()
                .id(Id::new(2))
                .child(div().id(Id::new(3)).focusable().child(text("x"))),
        )
        .child(div().id(Id::new(4)));
    let laid = layout(root, Size::new(100.0, 100.0), 1.0, &mut Fixed);
    assert!(
        ui.key_path(&laid).is_empty(),
        "no focus, no one to take the key"
    );
    ui.focus_on(Some(Id::new(3)));
    assert_eq!(ui.key_path(&laid), vec![Id::new(3), Id::new(2), Id::new(1)]);
}

#[test]
fn a_hidden_element_and_what_is_in_it_answers_nothing() {
    let root = div().child(div().h(20.0).hidden().press(Id::new(5)));
    let laid = layout(root, Size::new(100.0, 100.0), 1.0, &mut Fixed);
    assert_eq!(laid.hit(at(5.0, 5.0)), None);
    assert!(laid.ids_at(at(5.0, 5.0)).is_empty());
}
