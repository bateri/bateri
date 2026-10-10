//! What Undo Move takes back: the record a move of panes leaves, and the question
//! whether the world is still the one the record would restore.
//!
//! **The record is a picture, not an inverse.** A move that carries a pane (or all of a tab's
//! panes) to another tab, makes a tab of it, swaps two panes or reorders a lone pane's tab
//! writes down what it is about to change — for each window that takes part, the strip as it
//! stood ([`Scene::order`]: order and selection) and each tab it touches as it stood
//! ([`Shape`]: name, split tree with its ratios, the focused pane) — and the shell puts that
//! picture back through the same steps the move used. A picture covers every move the same
//! way (a tab that the move closed is born again from its shape; a tab the move made is taken
//! apart), where one inverse per move would be five code paths that drift apart.
//!
//! **One step, and only while nothing else happened.** The shell keeps one record. Every
//! other change of the tab list drops it (selecting, adding, closing, naming, reordering, a tab
//! leaving or joining a window), and what no applier announces is found out here:
//! [`Record::holds`] compares the panes the picture accounts for with the panes the windows
//! hold now, so a pane that closed or was born since (a split, an exiting shell) makes the
//! picture false and the record goes with it, and so does a window that closed.
//!
//! Panes and tabs are named by the in-process identities, which mean nothing in another
//! process; a record never reaches the disk.

use crate::split::Tree;
use crate::tabs::Tabs;

/// One tab as it stood before a move.
#[derive(Clone, Debug, PartialEq)]
pub struct Shape {
    pub tab: u64,
    /// The name the user gave it, if any (a tab that was merged away takes it with it).
    pub name: Option<String>,
    /// The split tree, ratios and all.
    pub tree: Tree,
    /// The pane the keyboard was in.
    pub focus: u64,
}

/// One window's part of a move: its strip and the tabs the move touched.
#[derive(Clone, Debug, PartialEq)]
pub struct Scene {
    pub window: u64,
    /// The strip before the move: the tabs in order and the selected one.
    pub order: Tabs<u64>,
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
            .flat_map(|shape| shape.tree.leaves())
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
                    let own = shape.tree.leaves();
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
}
