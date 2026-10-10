//! The engine through its C functions alone. Every plan read through the getters is held against
//! the same move planned by `moves::plan` directly: a step kind, a field or a lent tree mapped wrong
//! shows here, whatever the move.

use std::ffi::{CStr, CString};
use std::ptr::{null, null_mut};

use super::*;
use crate::bt_string_free;
use crate::strip::*;

const PANES: [u64; 6] = [100, 101, 110, 200, 210, 211];

fn c(text: &str) -> CString {
    CString::new(text).expect("test text holds no NUL")
}

/// A split of two panes, side by side and even.
unsafe fn pair(a: u64, b: u64) -> *mut BtTree {
    // SAFETY: both leaves are fresh and owned.
    unsafe { bt_tree_split(axis::HORIZONTAL, 0.5, bt_tree_leaf(a), bt_tree_leaf(b)) }
}

/// Adds tab `tab` with `tree` (freed here) to `window`, in an 800×600 area at scale 2.
unsafe fn add(
    world: *mut BtWorld,
    window: u64,
    tab: u64,
    tree: *mut BtTree,
    focus: u64,
    selected: bool,
) {
    // SAFETY: the caller's live picture; the tree is owned and freed once.
    unsafe {
        assert!(bt_world_add_tab(
            world,
            window,
            tab,
            tree,
            focus,
            null(),
            selected
        ));
        bt_tree_free(tree);
        assert!(bt_world_set_area(world, tab, 0.0, 0.0, 800.0, 600.0, 2.0));
    }
}

/// Window 1: tab 10 holds 100 | 101 (101 focused), tab 11 holds 110, tab 10 on screen. Window 2:
/// tab 20 holds 200, tab 21 holds 210 | 211 (210 focused), tab 20 on screen. Every pane's smallest
/// size is 100 × 100. New identities start at `next`.
unsafe fn picture(next: u64) -> *mut BtWorld {
    // SAFETY: a fresh picture, built and returned.
    unsafe {
        let world = bt_world_new(next);
        assert!(bt_world_add_window(world, 1, false, false));
        assert!(bt_world_add_window(world, 2, false, false));
        add(world, 1, 10, pair(100, 101), 101, true);
        add(world, 1, 11, bt_tree_leaf(110), 110, false);
        add(world, 2, 20, bt_tree_leaf(200), 200, true);
        add(world, 2, 21, pair(210, 211), 210, false);
        for pane in PANES {
            assert!(bt_world_set_minimum(world, pane, 100.0, 100.0));
        }
        world
    }
}

/// A tree as text both sides can write: `(h 0.5 a b)`.
fn tree_words(tree: &Tree) -> String {
    match tree {
        Tree::Leaf(pane) => pane.to_string(),
        Tree::Split {
            axis,
            ratio,
            first,
            second,
        } => format!(
            "({} {ratio} {} {})",
            axis_code(*axis),
            tree_words(first),
            tree_words(second)
        ),
    }
}

/// The same, read through the C queries.
unsafe fn c_tree_words(tree: *const BtTree) -> String {
    // SAFETY: the caller's live tree.
    unsafe {
        if bt_tree_is_leaf(tree) {
            return bt_tree_pane(tree).to_string();
        }
        format!(
            "({} {} {} {})",
            bt_tree_axis(tree),
            bt_tree_ratio(tree),
            c_tree_words(bt_tree_first(tree)),
            c_tree_words(bt_tree_second(tree))
        )
    }
}

/// A strip as text: its tabs and the selected one.
fn strip_words(strip: &Tabs<u64>) -> String {
    format!("{:?}/{:?}", strip.ids(), strip.selected())
}

unsafe fn c_strip_words(strip: *const BtStrip) -> String {
    // SAFETY: the caller's live strip.
    unsafe {
        let ids: Vec<u64> = (0..bt_strip_count(strip))
            .map(|index| bt_strip_tab_at(strip, index))
            .collect();
        let mut selected = 0;
        let has = bt_strip_selected(strip, &raw mut selected);
        format!("{ids:?}/{:?}", has.then_some(selected))
    }
}

/// A step as the engine wrote it.
fn step_words(step: &Step) -> String {
    let kind = |code: u32| code.to_string();
    let f = fields(step);
    let extra = match step {
        Step::Reshape { tree, name, .. } => format!(" {} {name:?}", tree_words(tree)),
        Step::AdoptPanes { tree, .. } => format!(" {}", tree_words(tree)),
        Step::PutStrip { order, .. } => format!(" {}", strip_words(order)),
        _ => String::new(),
    };
    let code = match step {
        Step::ReleasePane { .. } => step::RELEASE_PANE,
        Step::ReleaseTab { .. } => step::RELEASE_TAB,
        Step::Unpack { .. } => step::UNPACK,
        Step::Fold { .. } => step::FOLD,
        Step::NewTab { .. } => step::NEW_TAB,
        Step::Wrap { .. } => step::WRAP,
        Step::AdoptTab { .. } => step::ADOPT_TAB,
        Step::MoveTab { .. } => step::MOVE_TAB,
        Step::Dissolve { .. } => step::DISSOLVE,
        Step::Reshape { .. } => step::RESHAPE,
        Step::PutStrip { .. } => step::PUT_STRIP,
        Step::Fit { .. } => step::FIT,
        Step::Undone { .. } => step::UNDONE,
        Step::OpenWindow { .. } => step::OPEN_WINDOW,
        Step::AdoptPanes { .. } => step::ADOPT_PANES,
        Step::CloseIfEmptied { .. } => step::CLOSE_IF_EMPTIED,
        Step::Raise { .. } => step::RAISE,
        Step::Select { .. } => step::SELECT,
        Step::Focus { .. } => step::FOCUS,
        Step::Pulse { .. } => step::PULSE,
    };
    format!(
        "{} w{} t{} p{} i{} f{} g{} x{} s{} at{:?}{extra}",
        kind(code),
        f.window,
        f.tab,
        f.pane,
        f.into,
        f.from,
        f.gap,
        f.index,
        f.slot,
        f.at
    )
}

/// A step as the host reads it.
unsafe fn c_step_words(plan: *const BtPlan, part: u32, index: usize) -> String {
    // SAFETY: the caller's live plan.
    unsafe {
        let code = bt_plan_step_kind(plan, part, index);
        let (mut x, mut y) = (0.0, 0.0);
        let at = bt_plan_step_at(plan, part, index, &raw mut x, &raw mut y).then_some((x, y));
        let tree = bt_plan_step_tree(plan, part, index);
        let name = bt_plan_step_name(plan, part, index);
        let strip = bt_plan_step_strip(plan, part, index);
        let extra = match code {
            step::RESHAPE => {
                let name = (!name.is_null())
                    .then(|| CStr::from_ptr(name).to_str().expect("UTF-8").to_owned());
                format!(" {} {name:?}", c_tree_words(tree))
            }
            step::ADOPT_PANES => format!(" {}", c_tree_words(tree)),
            step::PUT_STRIP => format!(" {}", c_strip_words(strip)),
            _ => {
                assert!(tree.is_null() && name.is_null() && strip.is_null());
                String::new()
            }
        };
        format!(
            "{} w{} t{} p{} i{} f{} g{} x{} s{} at{:?}{extra}",
            code,
            bt_plan_step_window(plan, part, index),
            bt_plan_step_tab(plan, part, index),
            bt_plan_step_pane(plan, part, index),
            bt_plan_step_into(plan, part, index),
            bt_plan_step_from(plan, part, index),
            bt_plan_step_gap(plan, part, index),
            bt_plan_step_index(plan, part, index),
            bt_plan_step_slot(plan, part, index),
            at
        )
    }
}

/// Plans `mv` (freed here) through C and directly, and holds the two against each other: the
/// refusal, every step of both parts, the identities used and whether a record came. The C plan
/// is returned (NULL on a refusal).
unsafe fn same_plan(world: *const BtWorld, mv: *mut BtMove) -> (*mut BtPlan, i32) {
    // SAFETY: the caller's live picture and owned move.
    unsafe {
        let picture = &*world;
        let engine = Engine {
            world: picture,
            next: Cell::new(picture.next_id),
        };
        let direct = moves::plan(&picture.windows, (*mv).0.clone(), &engine);
        let mut code = -1;
        let plan = bt_plan_new(world, mv, &raw mut code);
        bt_move_free(mv);
        match direct {
            Err(refused) => {
                assert!(plan.is_null());
                let expected = match refused {
                    Refusal::Quiet => refusal::QUIET,
                    Refusal::Beep => refusal::BEEP,
                    Refusal::Stale => refusal::STALE,
                };
                assert_eq!(code, expected);
            }
            Ok(direct) => {
                assert!(!plan.is_null());
                assert_eq!(code, 0);
                for (part, steps) in [(part::MAIN, &direct.steps), (part::AFTER, &direct.after)] {
                    let read: Vec<String> = (0..bt_plan_step_count(plan, part))
                        .map(|index| c_step_words(plan, part, index))
                        .collect();
                    let written: Vec<String> = steps.iter().map(step_words).collect();
                    assert_eq!(read, written);
                }
                assert_eq!(
                    bt_plan_ids_used(plan),
                    engine.next.get().wrapping_sub(picture.next_id)
                );
                assert_eq!((*plan).undo.is_some(), direct.undo.is_some());
            }
        }
        (plan, code)
    }
}

#[test]
fn every_move_reads_through_c_as_the_engine_planned_it() {
    // SAFETY: every pointer is live for the block and freed once.
    unsafe {
        let world = picture(1 << 62);
        let landing = pair(110, 101);
        let moves = [
            bt_move_pane_to_tab(101, 11, side::RIGHT),
            bt_move_pane_to_tab_planned(101, 11, landing),
            bt_move_tab_to_tab(11, 10, side::DOWN),
            bt_move_tab_to_tab_planned(11, 10, null()),
            bt_move_pane_to_new_tab(101, 1, 1),
            bt_move_pane_to_new_tab(110, 2, 0),
            bt_move_tab_to_new_window(11, true, 5.0, 6.0),
            bt_move_pane_to_new_window(211, false, 0.0, 0.0),
            bt_move_tab_to_strip(20, 1, 0),
            bt_move_tab_to_strip(11, 1, 0),
            bt_move_merge_all_windows(1),
            bt_move_pane_to_tab(999, 11, side::LEFT),
        ];
        bt_tree_free(landing);
        assert!(moves[3].is_null(), "a planned move needs its tree");
        let mut planned = 0;
        for mv in moves.into_iter().filter(|mv| !mv.is_null()) {
            let (plan, _) = same_plan(world, mv);
            if !plan.is_null() {
                planned += 1;
            }
            bt_plan_free(plan);
        }
        assert!(planned >= 9, "the moves are real ones, not refusals");
        bt_world_free(world);
    }
}

#[test]
fn a_new_tab_takes_the_hosts_next_identity_and_says_how_many_it_used() {
    // SAFETY: as above.
    unsafe {
        let next = (1 << 62) + 7;
        let world = picture(next);
        let (plan, _) = same_plan(world, bt_move_pane_to_new_tab(101, 1, 1));
        assert_eq!(bt_plan_step_kind(plan, part::MAIN, 1), step::NEW_TAB);
        assert_eq!(bt_plan_step_tab(plan, part::MAIN, 1), next);
        assert_eq!(bt_plan_ids_used(plan), 1);
        bt_plan_free(plan);
        bt_world_free(world);

        // At the very top the next ones wrap: nothing is assumed of the range.
        let world = picture(u64::MAX);
        let (plan, _) = same_plan(world, bt_move_pane_to_new_window(211, false, 0.0, 0.0));
        assert_eq!(bt_plan_ids_used(plan), 2);
        bt_plan_free(plan);
        bt_world_free(world);
    }
}

#[test]
fn a_window_asking_beeps_and_a_move_naming_nothing_is_quiet() {
    // SAFETY: as above.
    unsafe {
        let world = bt_world_new(1);
        assert!(bt_world_add_window(world, 1, false, true));
        add(world, 1, 10, pair(100, 101), 101, true);
        add(world, 1, 11, bt_tree_leaf(110), 110, false);
        let (_, beep) = same_plan(world, bt_move_pane_to_new_tab(101, 1, 1));
        assert_eq!(beep, refusal::BEEP);
        let (_, quiet) = same_plan(world, bt_move_pane_to_new_tab(999, 1, 1));
        assert_eq!(quiet, refusal::QUIET);
        bt_world_free(world);
    }
}

#[test]
fn undo_move_puts_a_named_tab_back_and_a_stale_record_beeps() {
    // SAFETY: as above.
    unsafe {
        let before = bt_world_new(1);
        assert!(bt_world_add_window(before, 1, false, false));
        let tree = pair(100, 101);
        assert!(bt_world_add_tab(
            before,
            1,
            10,
            tree,
            101,
            c("build").as_ptr(),
            true
        ));
        bt_tree_free(tree);
        let record = bt_record_of_tab(before, 10);
        assert!(!record.is_null());
        assert!(bt_record_standing(before, record));

        // The host swapped the two panes: the record takes them back, name and all.
        let after = bt_world_new(1);
        assert!(bt_world_add_window(after, 1, false, false));
        let swapped = pair(101, 100);
        assert!(bt_world_add_tab(
            after,
            1,
            10,
            swapped,
            101,
            c("build").as_ptr(),
            true
        ));
        bt_tree_free(swapped);
        assert!(bt_record_standing(after, record));
        let (plan, code) = same_plan(after, bt_move_undo(record));
        assert_eq!(code, 0);
        let reshape = (0..bt_plan_step_count(plan, part::MAIN))
            .find(|&index| bt_plan_step_kind(plan, part::MAIN, index) == step::RESHAPE)
            .expect("the tab is reshaped");
        assert_eq!(
            CStr::from_ptr(bt_plan_step_name(plan, part::MAIN, reshape)).to_str(),
            Ok("build")
        );
        let kept = bt_tree_copy(bt_plan_step_tree(plan, part::MAIN, reshape));
        bt_plan_free(plan);
        assert_eq!(
            c_tree_words(kept),
            "(1 0.5 100 101)",
            "a copy outlives its plan"
        );
        bt_tree_free(kept);

        // A pane born since: the picture no longer holds.
        let grown = bt_world_new(1);
        assert!(bt_world_add_window(grown, 1, false, false));
        let three = bt_tree_split(axis::VERTICAL, 0.5, pair(100, 101), bt_tree_leaf(102));
        assert!(bt_world_add_tab(grown, 1, 10, three, 101, null(), true));
        bt_tree_free(three);
        assert!(!bt_record_standing(grown, record));
        let (_, stale) = same_plan(grown, bt_move_undo(record));
        assert_eq!(stale, refusal::STALE);

        bt_record_free(record);
        for world in [before, after, grown] {
            bt_world_free(world);
        }
    }
}

#[test]
fn a_verdict_reads_through_c_as_the_tree_judged_it_and_its_landing_plans() {
    // SAFETY: as above.
    unsafe {
        let world = picture(1);
        let shape = (*world).shape(10).expect("tab 10").clone();
        let min = |pane: u64| (*world).min(pane);
        let mut kinds = Vec::new();
        for (carried, x, y) in [
            (100, 700.0, 300.0),
            (100, 600.0, 300.0),
            (110, 700.0, 300.0),
            (110, 600.0, 580.0),
            (100, 200.0, 300.0),
            (100, 900.0, 900.0),
        ] {
            let tree = bt_tree_leaf(carried);
            let foreign = usize::from(!shape.tree.leaves().contains(&carried));
            let room = (*world)
                .room(10, shape.tree.leaves().len() + foreign, &min)
                .expect("an area");
            let judged = shape.tree.verdict(&Tree::Leaf(carried), &room, (x, y));
            let read = bt_verdict_new(world, 10, tree, x, y);
            assert!(!read.is_null());
            let kind = bt_verdict_kind(read);
            kinds.push(kind);
            let (mut rx, mut ry, mut rw, mut rh) = (0.0, 0.0, 0.0, 0.0);
            let has_rect =
                bt_verdict_rect(read, &raw mut rx, &raw mut ry, &raw mut rw, &raw mut rh);
            match &judged {
                Verdict::Lands { zone, placement } => {
                    assert_eq!(kind, verdict::LANDS);
                    assert!(has_rect);
                    assert_eq!(Rect::new(rx, ry, rw, rh), placement.landing);
                    assert_eq!(bt_verdict_made_room(read), placement.made_room);
                    assert_eq!(
                        c_tree_words(bt_verdict_tree(read)),
                        tree_words(&placement.tree)
                    );
                    assert_eq!(
                        CStr::from_ptr(bt_verdict_label(read)).to_str().ok(),
                        zone.label()
                    );
                    // A block from another tab lands through a planned move: the drop is the
                    // preview's answer.
                    if foreign == 1 {
                        let (plan, code) = same_plan(
                            world,
                            bt_move_pane_to_tab_planned(carried, 10, bt_verdict_tree(read)),
                        );
                        assert_eq!(code, 0);
                        bt_plan_free(plan);
                    }
                }
                Verdict::Swaps {
                    target,
                    frame,
                    fits,
                } => {
                    assert_eq!(kind, verdict::SWAPS);
                    assert_eq!(bt_verdict_zone(read), zone::SWAP);
                    assert_eq!(bt_verdict_target(read), *target);
                    assert_eq!(bt_verdict_fits(read), *fits);
                    assert_eq!(Rect::new(rx, ry, rw, rh), *frame);
                }
                Verdict::TooSmall { edges, .. } => {
                    assert_eq!(kind, verdict::TOO_SMALL);
                    assert_eq!(bt_verdict_edge_count(read), edges.len());
                }
                Verdict::Nothing => assert_eq!(kind, verdict::NOTHING),
                Verdict::NoRoom => assert_eq!(kind, verdict::NO_ROOM),
            }
            bt_verdict_free(read);
            bt_tree_free(tree);
        }
        assert!(kinds.contains(&verdict::LANDS) && kinds.contains(&verdict::SWAPS));
        assert!(
            bt_verdict_new(world, 99, bt_tree_leaf(1), 0.0, 0.0).is_null(),
            "no tab, no verdict"
        );
        bt_world_free(world);
    }
}

#[test]
fn a_tree_is_built_queried_and_kept_as_versioned_text() {
    // SAFETY: as above.
    unsafe {
        let tree = bt_tree_split(axis::VERTICAL, 0.25, bt_tree_leaf(7), pair(1 << 60, 0));
        assert_eq!(
            c_tree_words(tree),
            "(2 0.25 7 (1 0.5 1152921504606846976 0))"
        );
        assert_eq!(bt_tree_pane_count(tree), 3);
        assert_eq!(bt_tree_pane_at(tree, 1), 1 << 60);
        assert_eq!(bt_tree_pane_at(tree, 3), 0);

        let text = bt_tree_encode(tree);
        assert!(
            CStr::from_ptr(text)
                .to_str()
                .is_ok_and(|text| text.starts_with("bateri-tree 1\n"))
        );
        let back = bt_tree_decode(text);
        bt_string_free(text);
        assert_eq!(c_tree_words(back), c_tree_words(tree));
        assert!(bt_tree_decode(c("bateri-tree 2\nP 1\nT L 0\n").as_ptr()).is_null());

        let even = bt_tree_equalized(tree);
        assert!(
            (bt_tree_ratio(even) - 1.0 / 2.0).abs() < 1e-9,
            "one pane over a row of two"
        );

        assert!(bt_tree_split(axis::HORIZONTAL, 0.5, bt_tree_leaf(1), bt_tree_leaf(1)).is_null());
        assert!(bt_tree_split(axis::HORIZONTAL, 1.0, bt_tree_leaf(1), bt_tree_leaf(2)).is_null());
        assert!(bt_tree_split(9, 0.5, bt_tree_leaf(1), bt_tree_leaf(2)).is_null());
        assert!(bt_tree_split(axis::HORIZONTAL, 0.5, null_mut(), bt_tree_leaf(2)).is_null());
        for tree in [tree, back, even] {
            bt_tree_free(tree);
        }
    }
}

#[test]
fn the_picture_refuses_a_tab_it_could_not_hold() {
    // SAFETY: as above.
    unsafe {
        let world = picture(1);
        let lone = bt_tree_leaf(300);
        assert!(
            !bt_world_add_tab(world, 9, 30, lone, 300, null(), false),
            "no such window"
        );
        assert!(
            !bt_world_add_tab(world, 1, 10, lone, 300, null(), false),
            "the tab is there"
        );
        assert!(
            !bt_world_add_tab(world, 1, 30, lone, 301, null(), false),
            "focus not in it"
        );
        let taken = bt_tree_leaf(100);
        assert!(
            !bt_world_add_tab(world, 1, 30, taken, 100, null(), false),
            "pane in another tab"
        );
        assert!(bt_world_add_tab(world, 1, 30, lone, 300, null(), true));
        assert!(
            !bt_world_add_window(world, 1, false, false),
            "the window is there"
        );
        assert!(
            !bt_world_set_spacing(world, 77, spacing::LONE, 0.0, 0.0, 0.0),
            "no area"
        );
        assert!(
            !bt_world_set_area(world, 30, 0.0, 0.0, 10.0, 10.0, 0.0),
            "no scale"
        );
        let held = &*world;
        assert_eq!(held.windows[0].order.selected(), Some(30));
        bt_tree_free(lone);
        bt_tree_free(taken);
        bt_world_free(world);
    }
}

#[test]
fn layout_dividers_and_swaps_keep_every_pane_at_its_smallest() {
    // SAFETY: as above.
    unsafe {
        let world = picture(1);
        assert!(bt_world_set_spacing(
            world,
            10,
            spacing::SPLIT,
            8.0,
            8.0,
            0.0
        ));
        let layout = bt_layout_new(world, 10);
        assert_eq!(bt_layout_pane_count(layout), 2);
        assert_eq!(bt_layout_divider_count(layout), 1);
        let (mut pane, mut x, mut width) = (0, 0.0, 0.0);
        assert!(bt_layout_pane(
            layout,
            1,
            &raw mut pane,
            &raw mut x,
            null_mut(),
            &raw mut width,
            null_mut()
        ));
        assert_eq!(pane, 101);
        assert!(
            x > 400.0 && x + width <= 800.0 - 8.0 + 1e-9,
            "the margin is kept"
        );
        let mut divider_axis = 0;
        assert!(bt_layout_divider(
            layout,
            0,
            &raw mut divider_axis,
            null_mut(),
            null_mut(),
            null_mut(),
            null_mut()
        ));
        assert_eq!(divider_axis, axis::HORIZONTAL);
        assert!(!bt_layout_pane(
            layout,
            2,
            null_mut(),
            null_mut(),
            null_mut(),
            null_mut(),
            null_mut()
        ));
        bt_layout_free(layout);

        let dragged = bt_world_tree_dragged(world, 10, 0, 200.0);
        assert!(!dragged.is_null() && bt_tree_ratio(dragged) < 0.5);
        bt_tree_free(dragged);
        let pinned = bt_world_tree_dragged(world, 10, 0, 10.0);
        assert!(
            !pinned.is_null(),
            "dragged as far as the smallest size allows"
        );
        bt_tree_free(pinned);

        let resized = bt_world_tree_resized(world, 10, 100, side::RIGHT, 50.0);
        assert!(!resized.is_null() && bt_tree_ratio(resized) > 0.5);
        bt_tree_free(resized);

        let swapped = bt_world_tree_swapped(world, 10, 100, 101);
        assert_eq!(c_tree_words(swapped), "(1 0.5 101 100)");
        bt_tree_free(swapped);
        assert!(bt_world_set_minimum(world, 100, 700.0, 100.0));
        assert!(
            bt_world_tree_swapped(world, 10, 100, 101).is_null(),
            "a pane would not fit"
        );
        assert!(bt_world_tree_swapped(world, 10, 100, 999).is_null());
        bt_world_free(world);
    }
}

#[test]
fn a_strip_follows_bateris_rules() {
    // SAFETY: as above.
    unsafe {
        let strip = bt_strip_new();
        let mut tab = 0;
        assert!(!bt_strip_selected(strip, &raw mut tab));
        assert!(bt_strip_append(strip, 1));
        assert!(bt_strip_append(strip, 2));
        assert!(bt_strip_append(strip, 3));
        assert_eq!(c_strip_words(strip), "[1, 2, 3]/Some(1)");
        assert!(bt_strip_insert(strip, 9));
        assert_eq!(
            c_strip_words(strip),
            "[1, 9, 2, 3]/Some(9)",
            "right of the selected one"
        );
        assert!(bt_strip_close(strip, 9));
        assert_eq!(
            c_strip_words(strip),
            "[1, 2, 3]/Some(2)",
            "the right neighbour comes up"
        );
        assert!(bt_strip_select(strip, 3));
        assert!(bt_strip_close(strip, 3));
        assert_eq!(
            c_strip_words(strip),
            "[1, 2]/Some(2)",
            "the left one at the end"
        );
        assert!(bt_strip_adjacent(strip, true, &raw mut tab));
        assert_eq!(tab, 1, "wrapping");
        assert!(bt_strip_place_new(strip, 5, 0));
        assert_eq!(c_strip_words(strip), "[5, 1, 2]/Some(2)");
        assert!(bt_strip_by_shortcut(strip, 9, &raw mut tab));
        assert_eq!(tab, 2, "⌘9 is the last");
        assert!(!bt_strip_by_shortcut(strip, 7, &raw mut tab));
        assert!(bt_strip_move_to(strip, 2, 0));
        let mut index = 9;
        assert!(bt_strip_index_of(strip, 2, &raw mut index));
        assert_eq!(index, 0);
        assert!(bt_strip_insert_at(strip, 8, 1));
        assert_eq!(c_strip_words(strip), "[2, 8, 5, 1]/Some(8)");
        let copy = bt_strip_copy(strip);
        bt_strip_free(strip);
        assert_eq!(c_strip_words(copy), "[2, 8, 5, 1]/Some(8)");
        bt_strip_free(copy);
    }
}
