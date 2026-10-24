//! The container list in use: the keys, the pointer, the filter, the menu, the scroll, and the
//! fades — each through the same `Input` the host sends, on a page laid out with a stand-in for
//! the host's text.

use super::*;
use pencere_view::{
    Button, Cursor, Font, Input, Key, Keyboard, Laid, Measure, MenuEntry, Metrics, Mods, Point,
    Reply, layout,
};

struct Mono;

impl Measure for Mono {
    fn measure(&mut self, font: &Font, text: &str) -> Metrics {
        Metrics {
            width: text.chars().count() as f32 * font.size * 0.55,
            height: (font.size * 1.3).round(),
        }
    }
}

/// The page laid out as a pane of 1000 × `height` points would have it, and settled.
fn lay(page: &mut Containers, height: f32) -> Laid {
    let size = Size::new(1000.0, height);
    let mut laid = layout(page.view(size), size, 1.0, &mut Mono);
    while page.settle(&laid) {
        laid = layout(page.view(size), size, 1.0, &mut Mono);
    }
    laid
}

fn key(page: &mut Containers, key: Key) -> Reply {
    let laid = lay(page, 600.0);
    page.input(
        Input::Key {
            key,
            mods: Mods::default(),
        },
        &laid,
    )
}

fn chars(page: &mut Containers, keys: &str) {
    for c in keys.chars() {
        key(page, Key::Char(c));
    }
}

fn cursor(page: &Containers) -> Option<RowKey> {
    page.cursor
}

fn center(laid: &Laid, id: Id) -> Point {
    let b = laid.bounds(id).expect("the element is on the page");
    Point::new(b.x + b.w / 2.0, b.y + b.h / 2.0)
}

fn click(page: &mut Containers, laid: &Laid, id: Id, button: Button) -> Reply {
    let at = center(laid, id);
    page.input(
        Input::Press {
            at,
            button,
            mods: Mods::default(),
        },
        laid,
    );
    page.input(Input::Release { at, button }, laid)
}

fn names(page: &Containers) -> Vec<&'static str> {
    page.rows()
        .iter()
        .filter_map(|row| match row.key {
            RowKey::Container(name) => Some(name),
            RowKey::Group(_) => None,
        })
        .collect()
}

/// A list of `count` containers in projects of ten.
fn many(count: usize) -> Containers {
    let mut page = Containers::fixture();
    let template = page.containers[0].clone();
    page.containers = (0..count)
        .map(|at| Container {
            name: Box::leak(format!("c{at:05}").into_boxed_str()),
            project: Some(Box::leak(format!("p{:04}", at / 10).into_boxed_str())),
            ..template.clone()
        })
        .collect();
    page.collapsed.clear();
    page.cursor = None;
    page
}

#[test]
fn the_arrow_keys_and_j_k_g_and_capital_g_move_the_cursor_within_the_list() {
    let mut page = Containers::fixture();
    assert_eq!(cursor(&page), Some(RowKey::Container("api")));
    assert!(key(&mut page, Key::Down).redraw);
    assert_eq!(cursor(&page), Some(RowKey::Container("cache")));
    key(&mut page, Key::Char('j'));
    assert_eq!(cursor(&page), Some(RowKey::Container("db")));
    key(&mut page, Key::Up);
    key(&mut page, Key::Char('k'));
    assert_eq!(cursor(&page), Some(RowKey::Container("api")));
    key(&mut page, Key::Char('k'));
    assert_eq!(cursor(&page), Some(RowKey::Group(Some("shop"))));
    assert!(
        !key(&mut page, Key::Up).redraw,
        "the first row is the end of the list"
    );
    key(&mut page, Key::Char('G'));
    assert_eq!(cursor(&page), Some(RowKey::Container("toolbox")));
    assert!(!key(&mut page, Key::Char('j')).redraw, "and so is the last");
    key(&mut page, Key::Char('g'));
    assert_eq!(cursor(&page), Some(RowKey::Group(Some("shop"))));
    key(&mut page, Key::End);
    assert_eq!(cursor(&page), Some(RowKey::Container("toolbox")));
    key(&mut page, Key::Home);
    key(&mut page, Key::PageDown);
    assert_ne!(cursor(&page), Some(RowKey::Group(Some("shop"))));
}

#[test]
fn enter_and_the_arrows_fold_a_project_and_left_goes_back_to_it() {
    let mut page = Containers::fixture();
    key(&mut page, Key::Char('g'));
    assert!(names(&page).contains(&"api"));
    assert!(key(&mut page, Key::Enter).redraw);
    assert!(!names(&page).contains(&"api"), "folded");
    key(&mut page, Key::Enter);
    assert!(names(&page).contains(&"api"), "and unfolded");
    key(&mut page, Key::Left);
    assert!(!names(&page).contains(&"api"));
    assert!(!key(&mut page, Key::Left).redraw, "already folded");
    key(&mut page, Key::Right);
    assert!(names(&page).contains(&"api"));
    // From a container, ← goes back to the project it is under.
    key(&mut page, Key::Down);
    key(&mut page, Key::Left);
    assert_eq!(cursor(&page), Some(RowKey::Group(Some("shop"))));
    // The folded project of the fixture opens from its row.
    key(&mut page, Key::Char('G'));
    key(&mut page, Key::Up);
    assert_eq!(cursor(&page), Some(RowKey::Group(None)));
    key(&mut page, Key::Up);
    assert_eq!(cursor(&page), Some(RowKey::Group(Some("stats"))));
    assert!(!names(&page).contains(&"grafana"));
    key(&mut page, Key::Right);
    assert!(names(&page).contains(&"grafana"));
}

#[test]
fn space_marks_and_goes_on_and_capital_a_clears() {
    let mut page = Containers::fixture();
    key(&mut page, Key::Char(' '));
    assert_eq!(page.marked, vec!["api"]);
    assert_eq!(
        cursor(&page),
        Some(RowKey::Container("cache")),
        "on to the next"
    );
    key(&mut page, Key::Char(' '));
    assert_eq!(page.marked, vec!["api", "cache"]);
    key(&mut page, Key::Char('k'));
    key(&mut page, Key::Char(' '));
    assert_eq!(page.marked, vec!["api"], "marking a marked one clears it");
    key(&mut page, Key::Char('a'));
    assert_eq!(page.marked.len(), names(&page).len());
    assert!(key(&mut page, Key::Char('A')).redraw);
    assert!(page.marked.is_empty());
    assert!(!key(&mut page, Key::Char('A')).redraw);
}

#[test]
fn the_filter_narrows_the_list_and_the_cursor_goes_to_the_nearest_row_left() {
    let mut page = Containers::fixture();
    let laid = lay(&mut page, 600.0);
    let _ = laid;
    // By image, by port, by name, by project.
    for (words, found) in [
        ("redis", vec!["cache"]),
        ("5432", vec!["db"]),
        ("GRAF", vec!["grafana"]),
        ("blog", vec!["ghost", "mysql"]),
    ] {
        assert!(page.set_query(words).redraw);
        assert_eq!(names(&page), found, "{words}");
        assert!(
            page.rows().iter().any(|row| Some(row.key) == page.cursor),
            "the cursor is on a row that is shown ({words})"
        );
    }
    // Nothing matches: no rows, no cursor, and the page says so.
    page.set_query("zzz");
    assert!(page.rows().is_empty());
    assert_eq!(cursor(&page), None);
    let laid = lay(&mut page, 600.0);
    assert!(
        pencere_view::layout(
            page.view(Size::new(1000.0, 600.0)),
            Size::new(1000.0, 600.0),
            1.0,
            &mut Mono
        )
        .paint(&mut Mono)
        .items
        .iter()
        .any(|item| matches!(item, pencere_view::Paint::Text(run) if run.text.contains("zzz")))
    );
    let _ = laid;
    // The same words again change nothing, and clearing brings the list back.
    assert!(!page.set_query("zzz").redraw);
    assert!(page.set_query("").redraw);
    assert_eq!(
        names(&page).len(),
        10,
        "the folded project's three are not shown"
    );
    assert!(page.cursor.is_some());
}

#[test]
fn a_filter_at_work_unfolds_the_projects_it_finds_things_in() {
    let mut page = Containers::fixture();
    assert!(!names(&page).contains(&"grafana"), "stats is folded");
    page.set_query("grafana");
    assert_eq!(names(&page), vec!["grafana"]);
}

#[test]
fn the_chips_filter_by_state_and_f_goes_round_them() {
    let mut page = Containers::fixture();
    let laid = lay(&mut page, 600.0);
    assert!(click(&mut page, &laid, Status::Issues.id(), Button::Left).redraw);
    assert_eq!(page.status, Status::Issues);
    assert_eq!(names(&page), vec!["mailer", "search"]);
    let laid = lay(&mut page, 600.0);
    click(&mut page, &laid, Status::Stopped.id(), Button::Left);
    assert_eq!(names(&page), vec!["mailer", "ghost", "mysql", "alerts"]);
    let laid = lay(&mut page, 600.0);
    click(&mut page, &laid, Status::All.id(), Button::Left);
    assert_eq!(page.status, Status::All);
    chars(&mut page, "f");
    assert_eq!(page.status, Status::Running);
    chars(&mut page, "fff");
    assert_eq!(page.status, Status::All);
    // The chip's count follows the list.
    let counts = page.counts();
    assert_eq!(
        (counts.all, counts.running, counts.stopped, counts.issues),
        (13, 9, 4, 2)
    );
}

#[test]
fn the_group_button_and_capital_p_flatten_the_list_by_name() {
    let mut page = Containers::fixture();
    let laid = lay(&mut page, 600.0);
    assert!(click(&mut page, &laid, GROUP_BUTTON, Button::Left).redraw);
    assert!(!page.grouped);
    assert!(
        page.rows()
            .iter()
            .all(|row| matches!(row.key, RowKey::Container(_)))
    );
    let flat = names(&page);
    let mut sorted = flat.clone();
    sorted.sort_unstable();
    assert_eq!(flat, sorted);
    chars(&mut page, "P");
    assert!(page.grouped);
}

#[test]
fn a_click_chooses_the_row_and_a_click_on_a_project_folds_it() {
    let mut page = Containers::fixture();
    let laid = lay(&mut page, 600.0);
    click(&mut page, &laid, RowKey::Container("db").id(), Button::Left);
    assert_eq!(cursor(&page), Some(RowKey::Container("db")));
    let laid = lay(&mut page, 600.0);
    click(
        &mut page,
        &laid,
        RowKey::Group(Some("shop")).id(),
        Button::Left,
    );
    assert_eq!(cursor(&page), Some(RowKey::Group(Some("shop"))));
    assert!(!names(&page).contains(&"api"), "folded by the click");
    // A click that marks (⌘) does not fold.
    let laid = lay(&mut page, 600.0);
    let at = center(&laid, RowKey::Group(Some("blog")).id());
    page.input(
        Input::Press {
            at,
            button: Button::Left,
            mods: Mods {
                command: true,
                ..Mods::default()
            },
        },
        &laid,
    );
    page.input(
        Input::Release {
            at,
            button: Button::Left,
        },
        &laid,
    );
    assert!(names(&page).contains(&"ghost"));
    assert_eq!(page.marked, vec!["ghost", "mysql"]);
}

#[test]
fn the_pointer_over_a_row_lights_it_and_a_pointer_that_stays_in_it_costs_no_frame() {
    let mut page = Containers::fixture();
    let laid = lay(&mut page, 600.0);
    let on = center(&laid, RowKey::Container("db").id());
    let first = page.input(Input::Move(on), &laid);
    assert!(first.redraw);
    assert_eq!(first.cursor, Cursor::Pointer);
    // The same row, another point of it: nothing changed, nothing to draw.
    let again = page.input(Input::Move(Point::new(on.x + 6.0, on.y)), &laid);
    assert!(!again.redraw);
    assert_eq!(again.cursor, Cursor::Pointer);
    assert!(page.animating(), "the highlight is coming up");
    // It comes up over the fade's time and then the page rests.
    let mut now = 5.0;
    let mut frames = 0;
    while page.advance(now, false) {
        now += 1.0 / 60.0;
        frames += 1;
        assert!(frames < 40, "the fade never ended");
    }
    assert!(!page.animating());
    assert!(page.glow(RowKey::Container("db").id()) > 0.99);
    // Off the rows: the hand goes and the highlight fades out.
    let gone = page.input(Input::Move(Point::new(990.0, 2.0)), &laid);
    assert!(gone.redraw);
    assert_eq!(gone.cursor, Cursor::Arrow);
    assert!(page.animating());
    assert!(!page.input(Input::Leave, &laid).redraw);
}

#[test]
fn reduce_motion_lights_a_row_at_once_and_asks_for_no_more_frames() {
    let mut page = Containers::fixture();
    page.advance(1.0, true);
    let laid = lay(&mut page, 600.0);
    let on = center(&laid, RowKey::Container("db").id());
    assert!(page.input(Input::Move(on), &laid).redraw);
    assert!(
        page.glow(RowKey::Container("db").id()) > 0.99,
        "already at full"
    );
    assert!(!page.animating());
    key(&mut page, Key::Down);
    assert!(!page.animating(), "the choice snaps too");
}

#[test]
fn the_choice_fades_from_one_row_to_the_next() {
    let mut page = Containers::fixture();
    let api = RowKey::Container("api").id().role(CHOSEN);
    let cache = RowKey::Container("cache").id().role(CHOSEN);
    assert_eq!(page.ui.fades.level(api), 1.0);
    key(&mut page, Key::Down);
    assert!(page.animating());
    let mut now = 1.0;
    while page.advance(now, false) {
        now += 1.0 / 60.0;
    }
    assert_eq!(page.ui.fades.level(api), 0.0);
    assert_eq!(page.ui.fades.level(cache), 1.0);
}

#[test]
fn a_right_click_chooses_the_row_and_opens_its_menu_whose_entries_act_on_the_list() {
    let mut page = Containers::fixture();
    let laid = lay(&mut page, 600.0);
    let at = center(&laid, RowKey::Container("db").id());
    let reply = page.input(
        Input::Press {
            at,
            button: Button::Right,
            mods: Mods::default(),
        },
        &laid,
    );
    assert_eq!(cursor(&page), Some(RowKey::Container("db")));
    let menu = reply.menu.expect("the row has a menu");
    assert_eq!(menu.at, at);
    let titles: Vec<&str> = menu
        .entries
        .iter()
        .filter_map(|entry| match entry {
            MenuEntry::Item { title, .. } => Some(title.as_str()),
            MenuEntry::Separator => None,
        })
        .collect();
    assert_eq!(
        titles,
        [
            "Stop",
            "Restart",
            "Pause",
            "Open a shell",
            "Logs",
            "Inspect",
            "Copy id",
            "Kill",
            "Remove"
        ]
    );
    // What needs a program is shown and grey.
    assert!(menu.entries.iter().any(|entry| matches!(entry,
        MenuEntry::Item { title, enabled: false, .. } if title == "Logs")));
    // Never two separators in a row, never one at an end.
    assert!(!matches!(menu.entries.first(), Some(MenuEntry::Separator)));
    assert!(!matches!(menu.entries.last(), Some(MenuEntry::Separator)));
    // Picking Stop stops it.
    let stop = menu
        .entries
        .iter()
        .position(|entry| matches!(entry, MenuEntry::Item { title, .. } if title == "Stop"))
        .unwrap();
    let done = page.pick(stop);
    assert!(done.redraw);
    let db = page.containers.iter().find(|c| c.name == "db").unwrap();
    assert_eq!(db.state, State::Exited(0));
    // The menu is spent: the same pick does nothing.
    assert!(!page.pick(stop).redraw);
}

#[test]
fn the_dot_key_opens_the_menu_under_the_cursors_row() {
    let mut page = Containers::fixture();
    let laid = lay(&mut page, 600.0);
    let row = laid.bounds(RowKey::Container("api").id()).unwrap();
    let reply = page.input(
        Input::Key {
            key: Key::Char('.'),
            mods: Mods::default(),
        },
        &laid,
    );
    let menu = reply.menu.expect("a menu");
    assert_eq!(menu.at, Point::new(row.x + 40.0, row.y + row.h - 4.0));
}

#[test]
fn the_command_keys_change_the_list_the_way_the_menu_does() {
    let mut page = Containers::fixture();
    let state = |page: &Containers, name: &str| {
        page.containers
            .iter()
            .find(|c| c.name == name)
            .unwrap()
            .state
    };
    chars(&mut page, "s");
    assert_eq!(state(&page, "api"), State::Exited(0), "stops what runs");
    chars(&mut page, "s");
    assert_eq!(
        state(&page, "api"),
        State::Running,
        "and starts what does not"
    );
    chars(&mut page, "p");
    assert_eq!(state(&page, "api"), State::Paused);
    chars(&mut page, "p");
    assert_eq!(state(&page, "api"), State::Running);
    chars(&mut page, "K");
    assert_eq!(state(&page, "api"), State::Exited(137));
    let reply = {
        key(&mut page, Key::Char('r'));
        key(&mut page, Key::Char('y'))
    };
    assert_eq!(state(&page, "api"), State::Running, "restarted");
    assert_eq!(reply.copy.as_deref().map(str::len), Some(12));
    // Removing takes it off the list and the cursor stays near.
    chars(&mut page, "d");
    assert!(!names(&page).contains(&"api"));
    assert_eq!(cursor(&page), Some(RowKey::Container("cache")));
}

#[test]
fn a_command_acts_on_the_marked_containers_and_not_the_cursors() {
    let mut page = Containers::fixture();
    chars(&mut page, " ");
    chars(&mut page, " ");
    assert_eq!(page.marked, vec!["api", "cache"]);
    chars(&mut page, "s");
    let down: Vec<&str> = page
        .containers
        .iter()
        .filter(|c| !c.is_up())
        .map(|c| c.name)
        .collect();
    assert!(down.contains(&"api") && down.contains(&"cache"));
    assert!(
        !down.contains(&"db"),
        "the cursor's row is not the target while others are marked"
    );
}

#[test]
fn escape_closes_the_slash_and_tab_go_to_the_filter_and_chords_are_not_read() {
    let mut page = Containers::fixture();
    assert!(key(&mut page, Key::Escape).close);
    assert_eq!(
        key(&mut page, Key::Char('/')).keyboard,
        Some(Keyboard::Filter)
    );
    assert_eq!(key(&mut page, Key::Tab).keyboard, Some(Keyboard::Filter));
    let laid = lay(&mut page, 600.0);
    for mods in [
        Mods {
            command: true,
            ..Mods::default()
        },
        Mods {
            control: true,
            ..Mods::default()
        },
        Mods {
            option: true,
            ..Mods::default()
        },
    ] {
        let reply = page.input(
            Input::Key {
                key: Key::Char('d'),
                mods,
            },
            &laid,
        );
        assert_eq!(reply, Reply::default());
    }
    assert!(names(&page).contains(&"api"));
}

#[test]
fn a_press_on_the_filter_box_asks_for_the_keyboard_for_the_field() {
    let mut page = Containers::fixture();
    let laid = lay(&mut page, 600.0);
    let at = center(&laid, SEARCH);
    let reply = page.input(
        Input::Press {
            at,
            button: Button::Left,
            mods: Mods::default(),
        },
        &laid,
    );
    assert_eq!(reply.keyboard, Some(Keyboard::Filter));
    // The field has a place inside the box.
    let field = laid.text_field().expect("the field's room");
    let outer = laid.bounds(SEARCH).unwrap();
    assert!(field.x >= outer.x && field.right() <= outer.right());
    assert!(field.y >= outer.y && field.bottom() <= outer.bottom());
}

#[test]
fn a_long_list_in_a_short_pane_scrolls_and_stays_within_its_reach() {
    let mut page = Containers::fixture();
    page.set_query("");
    let laid = lay(&mut page, 330.0);
    let extent = laid.scroll_extent(LIST).expect("the list scrolls");
    assert!(
        extent.content > extent.view,
        "{extent:?}: the list is longer than its window"
    );
    let over = center(&laid, RowKey::Container("api").id());
    assert!(
        page.input(Input::Scroll { at: over, dy: 60.0 }, &laid)
            .redraw
    );
    assert_eq!(page.ui.scroll_offset(LIST), 60.0);
    // To the end: no further.
    assert!(
        page.input(
            Input::Scroll {
                at: over,
                dy: 10_000.0
            },
            &laid
        )
        .redraw
    );
    assert_eq!(page.ui.scroll_offset(LIST), extent.max_offset());
    let at_end = lay(&mut page, 330.0);
    assert!(
        !page
            .input(Input::Scroll { at: over, dy: 10.0 }, &at_end)
            .redraw
    );
    // The last row is seen at the end.
    let last = at_end
        .bounds(RowKey::Container("toolbox").id())
        .expect("built");
    let window = at_end.bounds(LIST).unwrap();
    assert!(last.bottom() <= window.bottom() + 0.5);
}

#[test]
fn the_cursor_is_kept_in_sight_by_the_keys() {
    let mut page = Containers::fixture();
    let _ = lay(&mut page, 330.0);
    key_in(&mut page, Key::Char('G'), 330.0);
    assert!(
        page.ui.scroll_offset(LIST) > 0.0,
        "the last row is below the window"
    );
    let laid = lay(&mut page, 330.0);
    let row = laid.bounds(RowKey::Container("toolbox").id()).unwrap();
    let window = laid.bounds(LIST).unwrap();
    assert!(row.y >= window.y && row.bottom() <= window.bottom() + 0.5);
    key_in(&mut page, Key::Char('g'), 330.0);
    assert_eq!(
        page.ui.scroll_offset(LIST),
        0.0,
        "the first row, with its room above"
    );
}

fn key_in(page: &mut Containers, key: Key, height: f32) -> Reply {
    let laid = lay(page, height);
    page.input(
        Input::Key {
            key,
            mods: Mods::default(),
        },
        &laid,
    )
}

#[test]
fn a_list_that_shrinks_under_a_scrolled_window_comes_back_into_reach() {
    let mut page = Containers::fixture();
    let laid = lay(&mut page, 330.0);
    let over = center(&laid, RowKey::Container("api").id());
    page.input(
        Input::Scroll {
            at: over,
            dy: 10_000.0,
        },
        &laid,
    );
    assert!(page.ui.scroll_offset(LIST) > 0.0);
    page.set_query("redis");
    let laid = lay(&mut page, 330.0);
    assert_eq!(page.ui.scroll_offset(LIST), 0.0);
    assert!(laid.bounds(RowKey::Container("cache").id()).is_some());
}

#[test]
fn only_the_rows_in_view_are_built_however_long_the_list_is() {
    let mut page = many(10_000);
    let size = Size::new(1000.0, 600.0);
    let built = |page: &Containers| layout(page.view(size), size, 1.0, &mut Mono).len();
    let at_top = built(&page);
    assert!(at_top < 600, "{at_top} elements for 11 000 rows");
    // Far down the list the page is no bigger, and the rows there are the ones built.
    let laid = layout(page.view(size), size, 1.0, &mut Mono);
    let extent = laid.scroll_extent(LIST).unwrap();
    assert!(
        extent.content > 300_000.0,
        "the list is as long as its rows: {extent:?}"
    );
    page.ui.set_scroll(LIST, 150_000.0);
    let deep = layout(page.view(size), size, 1.0, &mut Mono);
    assert!(deep.len() < 600);
    let rows = page.rows();
    let places = Containers::places(&rows);
    let first = places.at(150_000.0).unwrap();
    let id = rows[first].key.id();
    assert!(deep.bounds(id).is_some(), "the row at the scroll is built");
    assert!(
        deep.bounds(rows[0].key.id()).is_none(),
        "and the first row, far above, is not"
    );
    // The row at the scroll stands where the scroll puts it.
    let at = deep.bounds(id).unwrap();
    let window = deep.bounds(LIST).unwrap();
    assert!(at.y <= window.y + window.h && at.bottom() >= window.y - ROW * 3.0);
    // The wheel, and the end of the list.
    let over = Point::new(500.0, 400.0);
    assert!(
        page.input(
            Input::Scroll {
                at: over,
                dy: 1.0e9
            },
            &deep
        )
        .redraw
    );
    let end = lay(&mut page, 600.0);
    let last = rows.last().unwrap().key.id();
    assert!(end.bounds(last).is_some());
}

#[test]
fn a_page_at_rest_asks_for_nothing() {
    let mut page = Containers::fixture();
    let laid = lay(&mut page, 600.0);
    assert!(!page.advance(1.0, false));
    assert!(!page.animating());
    assert!(!page.input(Input::Leave, &laid).redraw);
    assert!(
        !page
            .input(
                Input::Release {
                    at: Point::new(5.0, 5.0),
                    button: Button::Left
                },
                &laid
            )
            .redraw
    );
}

#[test]
fn the_pointer_takes_the_shape_of_what_it_is_over() {
    let mut page = Containers::fixture();
    let laid = lay(&mut page, 600.0);
    let over = |page: &mut Containers, id: Id| {
        let at = center(&laid, id);
        page.input(Input::Move(at), &laid).cursor
    };
    assert_eq!(
        over(&mut page, RowKey::Container("db").id()),
        Cursor::Pointer
    );
    assert_eq!(over(&mut page, Status::Running.id()), Cursor::Pointer);
    assert_eq!(over(&mut page, GROUP_BUTTON), Cursor::Pointer);
    // The host's text field is over the filter's box, and the button that sorts does nothing.
    assert_eq!(over(&mut page, SEARCH), Cursor::Text);
    assert_eq!(over(&mut page, SORT_BUTTON), Cursor::Arrow);
}

#[test]
fn a_marked_row_carries_a_bar_at_its_edge_and_takes_no_room_for_it() {
    let mut page = Containers::fixture();
    let before = lay(&mut page, 600.0);
    let row = before.bounds(RowKey::Container("api").id()).unwrap();
    chars(&mut page, " ");
    let after = lay(&mut page, 600.0);
    assert_eq!(after.bounds(RowKey::Container("api").id()), Some(row));
    let bar = after
        .rects()
        .find(|r| r.w == 3.0 && r.h > 10.0 && r.y > row.y && r.bottom() < row.bottom())
        .expect("the marker is in the row");
    assert!(
        bar.x >= row.x && bar.x < row.x + 8.0,
        "at the row's edge: {bar:?}"
    );
    assert_eq!(bar.h, row.h - 16.0);
}
