//! Where the pane of a terminal an agent opened goes in a tab.
//!
//! One rule, the drag's: a pane let go beside another asks for a share of it, takes what it
//! needs when the pane cannot give that, and moves up to the group the pane is part of when it
//! cannot give even that ([`Tree::plan_beside_sharing`]). The agent only chooses what a hand
//! chooses with the pointer — a side and how much — and when it chooses no side, the choice is
//! the palette's: right, else below ([`crate::palette::beside`]). A tab with no room left for it
//! is not an error: the pane opens in a tab of its own beside the anchor's, because an agent that
//! opens a terminal at night finds it there in the morning and nobody is asked.
//!
//! The answer says **how** the pane landed, not only where ([`Landed`]), since the agent and the
//! listing read it: as asked, beside the group the anchor is part of (the anchor was too small),
//! in a new tab (no room), or still waiting for a tab (decided by the Mac, which knows whether
//! the anchor is in any tab at all).
//!
//! A zoomed tab keeps its zoom: the pane is put in the tree as always and is born behind the
//! zoom ([`Landing::hidden`]); un-zooming it would change what the user is looking at.

use crate::palette::{Beside, beside};
use crate::split::{Direction, PANE_EDGE_SHARE, Rect, Room, Tree};

/// A side of the anchor an agent can ask for, or a tab of its own. The words are the ones the
/// drag's preview writes on the region it shows ([`crate::split::Zone::label`]), lower case.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
    Above,
    Below,
    /// A tab of its own, in the window of the anchor's tab.
    Tab,
}

impl Side {
    /// Every side, in the order the words are listed to a person.
    pub const ALL: [Side; 5] = [Side::Left, Side::Right, Side::Above, Side::Below, Side::Tab];

    /// The word the command line and the message carry.
    pub fn word(self) -> &'static str {
        match self {
            Side::Left => "left",
            Side::Right => "right",
            Side::Above => "above",
            Side::Below => "below",
            Side::Tab => "tab",
        }
    }

    /// The side a word names.
    pub fn from_word(word: &str) -> Option<Side> {
        Side::ALL.into_iter().find(|side| side.word() == word)
    }

    /// The side of a pane this is; `None` for a tab of its own.
    pub fn direction(self) -> Option<Direction> {
        match self {
            Side::Left => Some(Direction::Left),
            Side::Right => Some(Direction::Right),
            Side::Above => Some(Direction::Up),
            Side::Below => Some(Direction::Down),
            Side::Tab => None,
        }
    }
}

/// What the agent asked for.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Ask {
    /// A side of the anchor; none leaves the choice to the palette's rule.
    pub side: Option<Side>,
    /// The share of the anchor (of its group, once the pane has moved up to one) the new pane
    /// asks for, within `0..1`; none is half ([`PANE_EDGE_SHARE`]).
    pub share: Option<f64>,
}

/// How the pane came to be where it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Landed {
    /// What was asked for, or — when nothing was asked — the palette's choice.
    AsAsked,
    /// Beside the group the anchor is part of: the anchor itself was too small to share.
    BesideGroup,
    /// In a tab of its own because this tab had no room (a tab asked for is [`Landed::AsAsked`]).
    NewTab,
    /// In no tab yet: the anchor is in none, and the terminal waits in the list of what runs in
    /// the background until it is. Only the Mac can say so; [`place`] never answers it.
    Waiting,
}

impl Landed {
    /// The word the reply and the JSON carry.
    pub fn word(self) -> &'static str {
        match self {
            Landed::AsAsked => "as_asked",
            Landed::BesideGroup => "beside_group",
            Landed::NewTab => "new_tab",
            Landed::Waiting => "waiting",
        }
    }

    /// The case a word names.
    pub fn from_word(word: &str) -> Option<Landed> {
        [
            Landed::AsAsked,
            Landed::BesideGroup,
            Landed::NewTab,
            Landed::Waiting,
        ]
        .into_iter()
        .find(|landed| landed.word() == word)
    }
}

/// Where the pane goes.
#[derive(Clone, Debug, PartialEq)]
pub enum Spot {
    /// Into this tab: the tree it becomes and where the new pane stands, in points.
    Beside { tree: Tree, landing: Rect },
    /// Into a new tab, beside the anchor's tab and not selected.
    NewTab,
}

/// The decision.
#[derive(Clone, Debug, PartialEq)]
pub struct Landing {
    pub spot: Spot,
    pub landed: Landed,
    /// The pane did not get the share it asked for: a neighbour is held at its minimum.
    pub squeezed: bool,
    /// The pane is in a zoomed tab and so is born behind the zoom.
    pub hidden: bool,
}

/// Where pane `incoming` goes beside pane `anchor` of `tree` — the tab's tree, laid out in `room`
/// — when the agent asked `ask`; `zoomed` is whether the tab is zoomed. `None` when `anchor` is
/// not a pane of the tree or `incoming` already is (the caller put the wrong tab or pane in).
pub fn place(
    tree: &Tree,
    anchor: u64,
    incoming: u64,
    ask: &Ask,
    zoomed: bool,
    room: &Room<'_>,
) -> Option<Landing> {
    let panes = tree.leaves();
    if !panes.contains(&anchor) || panes.contains(&incoming) {
        return None;
    }
    let new_tab = |landed| Landing {
        spot: Spot::NewTab,
        landed,
        squeezed: false,
        hidden: false,
    };
    let leaf = Tree::Leaf(incoming);
    let direction = match ask.side {
        Some(Side::Tab) => return Some(new_tab(Landed::AsAsked)),
        Some(side) => side.direction(),
        // The palette's rule asks whether each plain half fits: a share that was squeezed or a
        // pane that moved up to a group is not a split of this pane.
        None => {
            let fits = |side| {
                tree.plan_beside(anchor, side, &leaf, room)
                    .is_some_and(|plan| !plan.made_room)
            };
            match beside(fits(Direction::Right), fits(Direction::Down)) {
                Beside::Right => Some(Direction::Right),
                Beside::Down => Some(Direction::Down),
                Beside::NewTab => None,
            }
        }
    };
    let Some(direction) = direction else {
        return Some(new_tab(Landed::NewTab));
    };
    let share = ask
        .share
        .filter(|share| share.is_finite())
        .map_or(PANE_EDGE_SHARE, |share| share.clamp(0.0, 1.0));
    let Some(plan) = tree.plan_beside_sharing(anchor, direction, &leaf, share, room) else {
        return Some(new_tab(Landed::NewTab));
    };
    Some(Landing {
        spot: Spot::Beside {
            tree: plan.tree,
            landing: plan.landing,
        },
        landed: if plan.climbed {
            Landed::BesideGroup
        } else {
            Landed::AsAsked
        },
        squeezed: plan.squeezed,
        hidden: zoomed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::split::{Axis, Size, Spacing, Zone};

    fn min_100_by_60(_: u64) -> Size {
        Size::new(100.0, 60.0)
    }

    fn room_in<'a>(width: f64, height: f64, min: &'a dyn Fn(u64) -> Size) -> Room<'a> {
        Room {
            bounds: Rect::new(0.0, 0.0, width, height),
            scale: 1.0,
            spacing: Spacing::DIVIDED,
            min,
        }
    }

    fn pair(axis: Axis, ratio: f64, first: u64, second: u64) -> Tree {
        Tree::Split {
            axis,
            ratio,
            first: Box::new(Tree::Leaf(first)),
            second: Box::new(Tree::Leaf(second)),
        }
    }

    /// `tree` is two panes along `axis` with `first`'s share near `ratio`: the plan lands on
    /// whole pixels, so the share is the nearest one a pixel allows.
    #[track_caller]
    fn assert_pair(tree: &Tree, axis: Axis, ratio: f64, first: u64, second: u64) {
        let Tree::Split {
            axis: seen,
            ratio: share,
            first: one,
            second: two,
        } = tree
        else {
            panic!("a split was expected: {tree:?}");
        };
        assert_eq!(*seen, axis, "{tree:?}");
        assert!((share - ratio).abs() < 0.01, "{share} is not near {ratio}");
        assert_eq!(
            (&**one, &**two),
            (&Tree::Leaf(first), &Tree::Leaf(second)),
            "{tree:?}"
        );
    }

    fn frame_of(tree: &Tree, room: &Room<'_>, pane: u64) -> Rect {
        tree.layout(room.bounds, room.scale)
            .panes
            .iter()
            .find(|(id, _)| *id == pane)
            .map(|(_, frame)| *frame)
            .expect("the pane is in the layout")
    }

    fn asking(side: Side, share: Option<f64>) -> Ask {
        Ask {
            side: Some(side),
            share,
        }
    }

    fn tree_of(landing: &Landing) -> &Tree {
        match &landing.spot {
            Spot::Beside { tree, .. } => tree,
            Spot::NewTab => panic!("the pane was expected in the tab"),
        }
    }

    #[test]
    fn the_words_of_a_side_are_the_ones_the_drags_preview_writes() {
        for side in [Side::Left, Side::Right, Side::Above, Side::Below] {
            let direction = side.direction().expect("a side of a pane");
            let label = Zone::Beside {
                target: 1,
                side: direction,
            }
            .label()
            .expect("a zone beside a pane has a label");
            assert_eq!(label.to_lowercase(), side.word());
        }
        assert_eq!(Side::Tab.direction(), None);
        for side in Side::ALL {
            assert_eq!(Side::from_word(side.word()), Some(side));
        }
        assert_eq!(Side::from_word("Left"), None);
        assert_eq!(Side::from_word("top"), None);
        for landed in [
            Landed::AsAsked,
            Landed::BesideGroup,
            Landed::NewTab,
            Landed::Waiting,
        ] {
            assert_eq!(Landed::from_word(landed.word()), Some(landed));
        }
    }

    #[test]
    fn a_side_and_a_share_are_what_the_pane_gets() {
        let min = min_100_by_60;
        let room = room_in(1001.0, 600.0, &min);
        let tree = Tree::Leaf(1);
        let landing = place(&tree, 1, 2, &asking(Side::Below, Some(0.3)), false, &room)
            .expect("the anchor is in the tree");
        assert_eq!(landing.landed, Landed::AsAsked);
        assert!(!landing.squeezed && !landing.hidden);
        let Spot::Beside {
            tree: after,
            landing: rect,
        } = &landing.spot
        else {
            panic!("room for it");
        };
        assert_pair(after, Axis::Vertical, 0.7, 1, 2);
        assert_eq!(frame_of(after, &room, 2), *rect, "the preview is the drop");
        assert_eq!(rect.height, 180.0, "a share of 30% of the 599 pixels");
        // The left side puts the newcomer first.
        let landing = place(&tree, 1, 2, &asking(Side::Left, None), false, &room).expect("anchor");
        assert_pair(tree_of(&landing), Axis::Horizontal, 0.5, 2, 1);
    }

    #[test]
    fn a_share_a_neighbour_cannot_give_is_squeezed_and_said_so() {
        let min = min_100_by_60;
        let room = room_in(1001.0, 600.0, &min);
        let landing = place(
            &Tree::Leaf(1),
            1,
            2,
            &asking(Side::Right, Some(0.95)),
            false,
            &room,
        )
        .expect("anchor");
        assert_eq!(landing.landed, Landed::AsAsked);
        assert!(landing.squeezed, "the neighbour keeps its minimum");
        let after = tree_of(&landing);
        assert_eq!(frame_of(after, &room, 1).width, 100.0);
    }

    #[test]
    fn a_pane_too_small_to_share_has_the_pane_land_beside_its_group() {
        let min = min_100_by_60;
        let room = room_in(1001.0, 600.0, &min);
        // Pane 1 is 150 wide: half of it is not 100.
        let tree = pair(Axis::Horizontal, 0.15, 1, 2);
        let landing = place(&tree, 1, 3, &asking(Side::Right, None), false, &room).expect("anchor");
        assert_eq!(landing.landed, Landed::BesideGroup);
        let after = tree_of(&landing);
        let Tree::Split { first, second, .. } = after else {
            panic!("a split");
        };
        assert_eq!(**first, tree, "the group keeps its own ratio");
        assert_eq!(**second, Tree::Leaf(3));
        // Every pane stays at or above its minimum.
        for (_, frame) in &after.layout(room.bounds, room.scale).panes {
            assert!(frame.width >= 100.0 && frame.height >= 60.0, "{frame:?}");
        }
    }

    #[test]
    fn a_side_that_does_not_fit_anywhere_in_the_tab_is_a_new_tab() {
        let min = min_100_by_60;
        // 150 wide: nothing goes beside anything.
        let room = room_in(150.0, 600.0, &min);
        let landing = place(
            &Tree::Leaf(1),
            1,
            2,
            &asking(Side::Right, None),
            false,
            &room,
        )
        .expect("anchor");
        assert_eq!(landing.spot, Spot::NewTab);
        assert_eq!(landing.landed, Landed::NewTab);
        assert!(!landing.hidden && !landing.squeezed);
        // Asked for a tab, got a tab: that is as asked, even where there was room.
        let wide = room_in(1001.0, 600.0, &min);
        let landing =
            place(&Tree::Leaf(1), 1, 2, &asking(Side::Tab, None), false, &wide).expect("anchor");
        assert_eq!(
            (landing.spot, landing.landed),
            (Spot::NewTab, Landed::AsAsked)
        );
    }

    #[test]
    fn with_no_side_asked_it_is_right_then_below_then_a_new_tab() {
        let min = min_100_by_60;
        let ask = Ask::default();
        let tree = Tree::Leaf(1);
        let wide = room_in(1001.0, 600.0, &min);
        let landing = place(&tree, 1, 2, &ask, false, &wide).expect("anchor");
        assert_pair(tree_of(&landing), Axis::Horizontal, 0.5, 1, 2);
        assert_eq!(landing.landed, Landed::AsAsked);
        // Too narrow to split in two, tall enough for two.
        let narrow = room_in(150.0, 600.0, &min);
        let landing = place(&tree, 1, 2, &ask, false, &narrow).expect("anchor");
        assert_pair(tree_of(&landing), Axis::Vertical, 0.5, 1, 2);
        assert_eq!(landing.landed, Landed::AsAsked);
        // Neither.
        let small = room_in(150.0, 100.0, &min);
        let landing = place(&tree, 1, 2, &ask, false, &small).expect("anchor");
        assert_eq!(
            (landing.spot, landing.landed),
            (Spot::NewTab, Landed::NewTab)
        );
    }

    #[test]
    fn the_palettes_rule_looks_at_the_plain_half_not_at_a_climb() {
        let min = min_100_by_60;
        // Pane 1 is 150 wide and a hundred tall: half of it fits neither way, and a pane
        // that went up to its group would not be a split of pane 1.
        let room = room_in(1001.0, 100.0, &min);
        let tree = pair(Axis::Horizontal, 0.15, 1, 2);
        let landing = place(&tree, 1, 3, &Ask::default(), false, &room).expect("anchor");
        assert_eq!(
            (landing.spot, landing.landed),
            (Spot::NewTab, Landed::NewTab)
        );
    }

    #[test]
    fn a_zoomed_tab_keeps_its_zoom_and_the_pane_is_born_behind_it() {
        let min = min_100_by_60;
        let room = room_in(1001.0, 600.0, &min);
        let landing = place(&Tree::Leaf(1), 1, 2, &Ask::default(), true, &room).expect("anchor");
        assert!(landing.hidden);
        assert_pair(tree_of(&landing), Axis::Horizontal, 0.5, 1, 2);
        // A pane in a new tab is not behind anything.
        let narrow = room_in(150.0, 100.0, &min);
        let landing = place(&Tree::Leaf(1), 1, 2, &Ask::default(), true, &narrow).expect("anchor");
        assert!(!landing.hidden);
    }

    #[test]
    fn ten_panes_in_a_row_all_land_and_when_the_room_is_gone_it_is_a_new_tab() {
        let min = min_100_by_60;
        let room = room_in(1001.0, 600.0, &min);
        let mut tree = Tree::Leaf(1);
        let (mut in_tab, mut new_tabs) = (0, 0);
        for incoming in 2..12 {
            let landing = place(&tree, 1, incoming, &Ask::default(), false, &room)
                .expect("the anchor stays and every newcomer is new");
            match landing.spot {
                Spot::Beside {
                    tree: after,
                    landing: rect,
                } => {
                    assert_eq!(frame_of(&after, &room, incoming), rect);
                    for (id, frame) in &after.layout(room.bounds, room.scale).panes {
                        assert!(
                            frame.width >= 100.0 && frame.height >= 60.0,
                            "pane {id} fell below its minimum: {frame:?}"
                        );
                    }
                    assert_eq!(new_tabs, 0, "once the room is gone it stays gone");
                    tree = after;
                    in_tab += 1;
                }
                Spot::NewTab => {
                    assert_eq!(landing.landed, Landed::NewTab);
                    new_tabs += 1;
                }
            }
        }
        assert!(in_tab >= 3, "right, right, then below: {in_tab}");
        assert!(new_tabs >= 1, "ten do not fit in one tab");
        assert_eq!(in_tab + new_tabs, 10);
    }

    #[test]
    fn a_pane_the_tab_does_not_hold_cannot_be_the_anchor_and_one_it_holds_is_not_new() {
        let min = min_100_by_60;
        let room = room_in(1001.0, 600.0, &min);
        let tree = pair(Axis::Horizontal, 0.5, 1, 2);
        assert_eq!(place(&tree, 7, 3, &Ask::default(), false, &room), None);
        assert_eq!(place(&tree, 1, 2, &Ask::default(), false, &room), None);
    }
}
