//! What Undo Move takes back: the record a move of panes leaves, and the question
//! whether the world is still the one the record would restore.
//!
//! **The record is a picture, not an inverse.** A move that carries a pane (or all of a tab's
//! panes) to another tab, makes a tab of it, swaps two panes or reorders a lone pane's tab
//! writes down what it is about to change — for each window that takes part, the strip as it
//! stood ([`Scene::order`]: order and selection) and each tab it touches as it stood
//! ([`Shape`]: name, split tree with its ratios, the focused pane; the strip's tab groups go with
//! the strip) — and putting that picture back is planned like any move (`moves::Move::Undo`), through the same steps the moves use. A
//! picture covers every move the same
//! way (a tab that the move closed is born again from its shape; a tab the move made is taken
//! apart), where one inverse per move would be five code paths that drift apart.
//!
//! **One step, and only while nothing else happened.** The shell keeps one record. Every
//! other change of the tab list drops it (adding, closing, naming, reordering, a tab leaving or
//! joining a window — not selecting: the picture carries the selection and puts it back), and
//! what no applier announces is found out here:
//! [`Record::holds`] compares the panes the picture accounts for with the panes the windows
//! hold now, so a pane that closed or was born since (a split, an exiting shell) makes the
//! picture false and the record goes with it, and so does a window that closed.
//!
//! Panes and tabs are named by the in-process identities, which mean nothing in another
//! process; a record never reaches the disk.

use crate::floating::{Card, Layers};
use crate::groups::{GroupId, Groups};
use crate::split::Tree;
use crate::tabs::Tabs;

/// One tab as it stood before a move.
#[derive(Clone, Debug, PartialEq)]
pub struct Shape {
    pub tab: u64,
    /// The name the user gave it, if any (a tab that was merged away takes it with it).
    pub name: Option<String>,
    /// The flat area's split tree, ratios and all.
    pub tree: Tree,
    /// The floating cards, back to front, each with its own tree, share and whether it was a
    /// capsule: Undo Move of a float or a dock puts them back with the tree.
    pub cards: Vec<Card>,
    /// The pane the keyboard was in (a floating pane too).
    pub focus: u64,
}

impl Shape {
    /// Every pane of the tab: the flat area's, then the cards' ([`Layers::panes`]). What the
    /// tab holds, where `tree` is only what the flat area holds.
    pub fn panes(&self) -> Vec<u64> {
        self.layers().panes()
    }

    /// The tab's two layers.
    pub fn layers(&self) -> Layers {
        Layers {
            flat: self.tree.clone(),
            cards: self.cards.clone(),
        }
    }
}

/// One window's part of a move: its strip and the tabs the move touched.
#[derive(Clone, Debug, PartialEq)]
pub struct Scene {
    pub window: u64,
    /// The strip before the move: the tabs in order and the selected one.
    pub order: Tabs<u64>,
    /// The groups before the move: which tabs were gathered under which name, which were closed
    /// and which tab each was last on. Putting the strip back puts them back with it.
    pub groups: Groups<u64>,
    /// The touched tabs, each as it stood. Every one is in [`Self::order`].
    pub shapes: Vec<Shape>,
}

impl Scene {
    /// The shape of tab `tab`, if the move touched it.
    pub fn shape(&self, tab: u64) -> Option<&Shape> {
        self.shapes.iter().find(|shape| shape.tab == tab)
    }

    /// Whether the move made tab `tab`: it was not in the strip before.
    pub fn made(&self, tab: u64) -> bool {
        self.order.index_of(tab).is_none()
    }
}

/// Everything one move changed, to be taken back as one step.
#[derive(Clone, Debug, PartialEq)]
pub struct Record {
    pub scenes: Vec<Scene>,
    /// Windows the move made (a pane that became a window): taking the move back closes them,
    /// their panes going back to the scenes.
    pub born: Vec<u64>,
}

/// What a window holds now: its tabs in strip order, each with its panes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Now {
    pub window: u64,
    pub tabs: Vec<(u64, Vec<u64>)>,
}

impl Record {
    /// Group `group` of window `window` has been called `name` since the move: the pictures the
    /// record puts back carry the name too, so that taking the move back does not give the group
    /// its old name again. A group the move made is in no picture — taking the move back removes
    /// it — and there is nothing to say; windows number their groups alone, so the window is part
    /// of the question. `name` is a name the window's groups already took
    /// ([`Groups::rename`]'s trimmed one).
    pub fn named(&mut self, window: u64, group: GroupId, name: &str) {
        for scene in self
            .scenes
            .iter_mut()
            .filter(|scene| scene.window == window)
        {
            scene.groups.rename(group, name);
        }
    }

    /// Whether window `window` takes part: a window where the step can be taken from.
    pub fn involves(&self, window: u64) -> bool {
        self.born.contains(&window) || self.scenes.iter().any(|scene| scene.window == window)
    }

    /// Whether `world` — the windows the record names, as they are now — is what the move left.
    ///
    /// True when every window is there, every tab the move did not touch is still in its strip,
    /// no touched tab that is still there has lost all of its own panes, and the panes of the
    /// touched tabs, of the tabs the move made and of the windows it made are **exactly** the
    /// panes the shapes account for. The last is the one that finds a pane that closed or was
    /// born since.
    pub fn holds(&self, world: &[Now]) -> bool {
        let find = |window: u64| world.iter().find(|now| now.window == window);
        let mut then: Vec<u64> = self
            .scenes
            .iter()
            .flat_map(|scene| scene.shapes.iter())
            .flat_map(|shape| shape.panes())
            .collect();
        let mut now: Vec<u64> = Vec::new();
        for scene in &self.scenes {
            let Some(held) = find(scene.window) else {
                return false;
            };
            let present = |tab: u64| held.tabs.iter().any(|(id, _)| *id == tab);
            if scene
                .order
                .ids()
                .iter()
                .any(|&tab| scene.shape(tab).is_none() && !present(tab))
            {
                return false;
            }
            for (tab, panes) in &held.tabs {
                if let Some(shape) = scene.shape(*tab) {
                    let own = shape.panes();
                    if !panes.iter().any(|pane| own.contains(pane)) {
                        return false;
                    }
                    now.extend(panes);
                } else if scene.made(*tab) {
                    now.extend(panes);
                }
            }
        }
        for window in &self.born {
            let Some(held) = find(*window) else {
                return false;
            };
            if held.tabs.is_empty() {
                return false;
            }
            for (_, panes) in &held.tabs {
                now.extend(panes);
            }
        }
        then.sort_unstable();
        now.sort_unstable();
        then == now
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::split::Axis;

    fn pair(a: u64, b: u64) -> Tree {
        Tree::Split {
            axis: Axis::Horizontal,
            ratio: 0.5,
            first: Box::new(Tree::Leaf(a)),
            second: Box::new(Tree::Leaf(b)),
        }
    }

    fn strip(ids: &[u64]) -> Tabs<u64> {
        let mut tabs = Tabs::new(ids[0]);
        for &id in &ids[1..] {
            tabs.append(id);
        }
        tabs
    }

    fn shape(tab: u64, tree: Tree) -> Shape {
        let focus = tree.leaves()[0];
        Shape {
            tab,
            name: None,
            tree,
            cards: Vec::new(),
            focus,
        }
    }

    fn now(window: u64, tabs: &[(u64, &[u64])]) -> Now {
        Now {
            window,
            tabs: tabs
                .iter()
                .map(|(id, panes)| (*id, panes.to_vec()))
                .collect(),
        }
    }

    /// Tab 1 (pane 10) was merged into tab 2 (panes 20, 21): the picture holds while tab 2
    /// holds exactly the three and tab 3 is where it was.
    fn merged() -> Record {
        Record {
            scenes: vec![Scene {
                window: 1,
                order: strip(&[1, 2, 3]),
                groups: Groups::new(),
                shapes: vec![shape(1, Tree::Leaf(10)), shape(2, pair(20, 21))],
            }],
            born: Vec::new(),
        }
    }

    #[test]
    fn a_merge_holds_while_the_panes_are_all_there() {
        let record = merged();
        let world = [now(1, &[(2, &[20, 21, 10]), (3, &[30])])];
        assert!(record.holds(&world));
        // The tab the merge closed may be there or not: it is only restored if it is gone.
        let world = [now(1, &[(1, &[]), (2, &[20, 21, 10]), (3, &[30])])];
        assert!(
            !record.holds(&world),
            "a tab that kept nothing of its own is not a tab"
        );
    }

    #[test]
    fn a_pane_born_or_closed_since_makes_the_picture_false() {
        let record = merged();
        assert!(
            !record.holds(&[now(1, &[(2, &[20, 21, 10, 99]), (3, &[30])])]),
            "a split"
        );
        assert!(
            !record.holds(&[now(1, &[(2, &[20, 10]), (3, &[30])])]),
            "a shell that exited"
        );
    }

    #[test]
    fn an_untouched_tab_that_closed_or_a_window_that_went_makes_it_false() {
        let record = merged();
        assert!(
            !record.holds(&[now(1, &[(2, &[20, 21, 10])])]),
            "tab 3 closed"
        );
        assert!(!record.holds(&[]), "the window closed");
    }

    #[test]
    fn a_tab_the_move_made_is_accounted_for() {
        // Pane 11 of tab 1 became tab 7.
        let record = Record {
            scenes: vec![Scene {
                window: 1,
                order: strip(&[1, 2]),
                groups: Groups::new(),
                shapes: vec![shape(1, pair(10, 11))],
            }],
            born: Vec::new(),
        };
        assert!(record.holds(&[now(1, &[(1, &[10]), (7, &[11]), (2, &[20])])]));
        assert!(!record.holds(&[now(1, &[(1, &[10]), (7, &[11, 12]), (2, &[20])])]));
        assert!(record.scenes[0].made(7));
        assert!(!record.scenes[0].made(2));
    }

    #[test]
    fn a_reorder_has_no_panes_to_account_for() {
        let record = Record {
            scenes: vec![Scene {
                window: 1,
                order: strip(&[1, 2, 3]),
                groups: Groups::new(),
                shapes: Vec::new(),
            }],
            born: Vec::new(),
        };
        assert!(record.holds(&[now(1, &[(2, &[20]), (3, &[30]), (1, &[10])])]));
    }

    #[test]
    fn a_window_the_move_made_holds_the_panes_that_left() {
        // Pane 11 of tab 1 became window 5 (tab 8).
        let record = Record {
            scenes: vec![Scene {
                window: 1,
                order: strip(&[1]),
                groups: Groups::new(),
                shapes: vec![shape(1, pair(10, 11))],
            }],
            born: vec![5],
        };
        let world = [now(1, &[(1, &[10])]), now(5, &[(8, &[11])])];
        assert!(record.holds(&world));
        assert!(record.involves(1) && record.involves(5) && !record.involves(2));
        assert!(
            !record.holds(&[now(1, &[(1, &[10])])]),
            "the new window closed"
        );
        assert!(
            !record.holds(&[now(1, &[(1, &[10])]), now(5, &[])]),
            "and left empty"
        );
    }

    #[test]
    fn a_name_given_since_the_move_is_put_back_with_the_groups_that_knew_it() {
        // Window 1 and window 2 each have a group `api` and number their groups alone: both are
        // group 0.
        let grouped = |window: u64| {
            let mut order = strip(&[1, 2]);
            let mut groups = Groups::new();
            groups.place(
                &mut order,
                &[2],
                crate::groups::Target::New("api".to_owned()),
            );
            Scene {
                window,
                order,
                groups,
                shapes: Vec::new(),
            }
        };
        let (one, two) = (grouped(1), grouped(2));
        let id = one.groups.group_of(2).expect("tab 2 is in a group");
        assert_eq!(Some(id), two.groups.group_of(2), "the ids are the same");
        let mut record = Record {
            scenes: vec![one, two],
            born: Vec::new(),
        };
        record.named(1, id, "  backend ");
        let name = |scene: usize| {
            record.scenes[scene]
                .groups
                .group(id)
                .map(|g| g.name.clone())
        };
        assert_eq!(
            name(0),
            Some("backend".to_owned()),
            "trimmed, as the window's own"
        );
        assert_eq!(
            name(1),
            Some("api".to_owned()),
            "another window's group is its own"
        );
        // A group that no picture knows — the move made it — is not made up.
        let mut order = strip(&[1, 2, 3]);
        let mut born = Groups::new();
        for tab in [2, 3] {
            born.place(
                &mut order,
                &[tab],
                crate::groups::Target::New(format!("g{tab}")),
            );
        }
        let made = born.group_of(3).expect("tab 3 is in a group");
        assert_ne!(Some(made), record.scenes[0].groups.group_of(2));
        let mut unchanged = record.clone();
        unchanged.named(1, made, "x");
        assert_eq!(unchanged, record);
    }
}
