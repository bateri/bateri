//! Moves of panes and tabs between tabs and windows: the **pure** half that decides what a move
//! is, whether it may happen, what it leaves selected and focused, what Undo Move keeps of it and
//! what taking it back comes to. The platform shell carries the answer out with its appliers and
//! nothing else.
//!
//! **The question and the answer.** The shell hands over a picture of its windows ([`Window`]:
//! the strip, every tab's shape, whether the window holds a question of its own) and what the
//! user asked for ([`Move`]); [`plan`] answers with a refusal (and whether it beeps) or a
//! [`Plan`]: the steps that move the panes, the steps that follow once they have landed, and
//! the record Undo Move keeps. A pane here is an identity and nothing more: the picture says
//! which panes a tab holds and how they are laid out, never what they show, so a host whose
//! panes are not terminals carries out the same plans. What kind of pane it is stays the host's
//! until a rule here depends on it.
//!
//! **The steps are the appliers'.** Each [`Step`] is one thing the shell already does in one
//! place — a pane or a tab leaves, a tab is taken apart, panes join a tab, a pane becomes a tab,
//! a tab joins a strip or takes another place in it, an emptied window closes, a window comes
//! forward, a tab is selected, the keyboard goes to a pane, a chip glows — named by ids only.
//! Panes and tabs a step takes out are held by the shell until a later step of the same plan
//! puts them somewhere, so nothing is closed on the way.
//!
//! **Where it fits is the shell's, and so are new ids.** How much room a pane needs comes from
//! what it shows (a terminal's cells) and its window's screen; the shell answers that through
//! [`Host`], and only for a
//! move that asks to stand beside the target's focused pane. A move whose layout the pointer
//! already chose ([`Joins::Planned`]) is only checked to be exactly the panes it names. A tab a
//! move makes is given its id by the shell too, once the move is sure to make it.
//!
//! **Undo Move's record is written last.** A tab leaving its window drops the record the shell
//! held (the strip changed under it), so the plan's record is handed over to be written once
//! every step is done ([`Plan::undo`]); a move that closes a window leaves none, since a window
//! is not a picture Undo Move can paint again.
//!
//! **Whether an emptied window closes is the window's.** bateri's windows close when a move
//! takes their last tab; a host whose windows are places of their own (a workspace that stays
//! when its last terminal moves out) says so in the picture ([`Window::stays_empty`]), and the
//! same moves leave such a window open — and undoable.

use crate::split::{Direction, Tree};
use crate::tabs::{NewTab, Tabs, gap_to_index};
use crate::undo::{Now, Record, Scene, Shape};

/// A window as a move sees it.
#[derive(Clone, Debug, PartialEq)]
pub struct Window {
    pub id: u64,
    /// The strip: the tabs in order and the selected one.
    pub order: Tabs<u64>,
    /// Every tab of the strip as it stands: name, split tree with its ratios, focused pane.
    pub tabs: Vec<Shape>,
    /// The window holds a question of its own (the close question, the application's report),
    /// which blocks all of it: nothing moves into or out of it. A tab's own question does not
    /// count; it leaves the screen with its tab.
    pub asking: bool,
    /// The window outlives its tabs: a move that takes its last one leaves it open and empty,
    /// and Undo Move can take that move back. bateri's windows close instead (`false`); a host
    /// whose windows are places of their own — a workspace that stays when its last terminal
    /// moves out — keeps them (`true`).
    pub stays_empty: bool,
}

impl Window {
    /// Whether a move that takes `leaving` of its tabs closes this window: they are all it has,
    /// and it does not outlive them.
    fn closes_without(&self, leaving: usize) -> bool {
        !self.stays_empty && self.order.len() <= leaving
    }

    /// The step that closes this window once a move has left it without a tab — none for a
    /// window that stays.
    fn close_if_emptied(&self) -> Option<Step> {
        (!self.stays_empty).then_some(Step::CloseIfEmptied { window: self.id })
    }

    /// Tab `tab` as it stands, if it is here.
    pub fn tab(&self, tab: u64) -> Option<&Shape> {
        self.tabs.iter().find(|shape| shape.tab == tab)
    }

    /// What Undo Move writes down for this window before a move changes it: the strip and the
    /// tabs `touched`, in that order (one that is not here is left out).
    pub fn scene(&self, touched: &[u64]) -> Scene {
        Scene {
            window: self.id,
            order: self.order.clone(),
            shapes: touched
                .iter()
                .filter_map(|&tab| self.tab(tab).cloned())
                .collect(),
        }
    }
}

/// Where panes joining a tab stand.
#[derive(Clone, Debug, PartialEq)]
pub enum Joins {
    /// Beside the tab's focused pane on this side, room made by its neighbours ([`Host::beside`])
    /// — the menus' way and a pane let go on a chip.
    Beside(Direction),
    /// Exactly this tree of the tab's panes and the joining ones — the landing a pointer chose.
    /// One shown for a tab that has changed since is refused, not applied.
    Planned(Tree),
}

impl Joins {
    /// The tree tab `target` and `incoming` make once they have joined.
    fn tree(self, target: &Shape, incoming: &Tree, host: &dyn Host) -> Option<Tree> {
        match self {
            Self::Beside(side) => host.beside(target.tab, side, incoming),
            Self::Planned(tree) => {
                let mut wanted = tree.leaves();
                wanted.sort_unstable();
                let mut have = target.tree.leaves();
                have.extend(incoming.leaves());
                have.sort_unstable();
                (wanted == have).then_some(tree)
            }
        }
    }
}

/// What a plan asks of the shell that carries it out: the geometry the picture does not carry,
/// and the ids of what a move makes.
pub trait Host {
    /// The tree tab `tab`'s panes and `incoming` make once `incoming` stands beside the tab's
    /// focused pane on `side` — neighbours shrinking to their smallest, the drop climbing to a
    /// larger group when they cannot give enough (`split::Tree::plan_beside`). `None` where not
    /// even the whole tab has room. The panes of `incoming` may still be in another tab.
    fn beside(&self, tab: u64, side: Direction, incoming: &Tree) -> Option<Tree>;

    /// An id no window, tab or pane has had: the one a tab the move makes is given. Asked only
    /// once the move is sure to make one.
    fn fresh_id(&self) -> u64;
}

/// What the user asked for.
#[derive(Clone, Debug, PartialEq)]
pub enum Move {
    /// Pane `pane` joins tab `into` as `place` says, in its own window or another — Move Split
    /// to Tab, to Previous / Next Tab, a pane let go on a chip or in another tab's panes. A
    /// tab's only pane is the tab: it joins as [`Move::TabToTab`] does.
    PaneToTab { pane: u64, into: u64, place: Joins },
    /// Tab `tab` joins tab `into` as a block of panes with its own layout and ratios — a chip's
    /// Merge into Current Tab, a tab let go with ⌥⌘ on another tab's panes. The tab is thrown
    /// away, its name with it.
    TabToTab { tab: u64, into: u64, place: Joins },
    /// Pane `pane` becomes a tab of its own in window `window`'s strip, before tab number `gap`
    /// (the strip's length is the end) — Move Split to New Tab, the capsule's tab of its own, a
    /// pane let go between chips of its own window or another's. A tab's only pane is the tab:
    /// the tab itself moves there, name and identity and all.
    PaneToNewTab { pane: u64, window: u64, gap: usize },
    /// Tab `tab` becomes a window of its own — Move Tab to New Window, a tab dragged out of its
    /// strip and let go over nothing. `at` is where it was let go (a point on the screen, in
    /// the platform's coordinates): the new window's title row goes under it; without it the
    /// window takes its old window's size and place, cascaded. A window's only tab stays,
    /// unless the window outlives its tabs ([`Window::stays_empty`]).
    TabToNewWindow { tab: u64, at: Option<(f64, f64)> },
    /// Pane `pane` becomes a window of its own, as the one tab of it — Move Split to New
    /// Window, a pane let go over no window. A tab's only pane is the tab: it moves as in
    /// [`Move::TabToNewWindow`].
    PaneToNewWindow { pane: u64, at: Option<(f64, f64)> },
    /// Tab `tab` takes place `index` in window `window`'s strip — a tab dragged along its own
    /// strip, or let go on another window's, where it leaves its window whole and comes up
    /// selected.
    TabToStrip { tab: u64, window: u64, index: usize },
    /// Every other window's tabs join window `into`'s at its end, in strip order, the
    /// selection staying where it was; the windows they leave close.
    MergeAllWindows { into: u64 },
    /// Undo Move: the picture `record` keeps goes back, as one step — what the move made comes
    /// apart (a window it made closes, no program with it), then each tab and strip is put as
    /// it was, the selection and the focus with them. There is no redo.
    Undo { record: Record },
}

/// Why nothing moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The move names nothing there is (a pane or tab that has gone, a tab joining itself).
    Quiet,
    /// The user asked for something that cannot be done now: a window holds a question of its
    /// own, or the panes do not fit. A beep says so.
    Beep,
    /// Undo Move's picture is no longer what is there (a pane closed or was born since, a
    /// window it names is gone): a beep, and the record is dropped.
    Stale,
}

/// One thing the shell does, by ids.
#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    /// Pane `pane` leaves tab `tab` of window `window` — moved, not closed. Never a tab's last.
    ReleasePane { window: u64, tab: u64, pane: u64 },
    /// Tab `tab` leaves window `window` whole — not closed: its panes go on. A selected tab
    /// hands the screen to its right neighbour (the left one at the end); the window's last tab
    /// leaves the window empty.
    ReleaseTab { window: u64, tab: u64 },
    /// Tab `tab`, released from window `window`, gives up its panes and is thrown away.
    Unpack { window: u64, tab: u64 },
    /// Tab `tab` of window `window` leaves the strip for tab `into` of the same window: if it was
    /// the selected one the screen goes to `into`, not to a neighbour. Its panes are taken, it is
    /// thrown away.
    Fold { window: u64, tab: u64, into: u64 },
    /// The pane the step before took becomes tab `tab` of window `window`, before tab number
    /// `gap` of its strip, **not** selected: the user stays where they were.
    NewTab { window: u64, tab: u64, gap: usize },
    /// The pane the step before took becomes tab `tab`, held in no strip yet; it is born in
    /// window `window`, the one it leaves.
    Wrap { window: u64, tab: u64 },
    /// The held tab `tab` joins window `window`'s strip at `slot`.
    AdoptTab { window: u64, tab: u64, slot: Slot },
    /// Tab `tab` takes place `index` in window `window`'s strip; what is on screen does not
    /// change.
    MoveTab { window: u64, tab: u64, index: usize },
    /// Tab `tab` of window `window`, one a move made, is taken apart: its panes are taken and it
    /// is thrown away. Its place in the strip stays until a [`Step::PutStrip`] puts the strip
    /// back.
    Dissolve { window: u64, tab: u64 },
    /// Tab `tab` of window `window` holds exactly the panes of `tree`, laid out as it says, and
    /// is named `name` again: the panes it lacks come from those the steps before took, and a
    /// tab that is gone is born again under its own id (not in the strip until
    /// [`Step::PutStrip`]). A question a pane brought opens again once its tab is on screen.
    Reshape {
        window: u64,
        tab: u64,
        tree: Tree,
        name: Option<String>,
    },
    /// Window `window`'s strip is `order` again, its selection with it; what was on screen
    /// leaves it first.
    PutStrip { window: u64, order: Tabs<u64> },
    /// Tab `tab` of window `window`, if its panes no longer fit their smallest on this screen,
    /// is evened out.
    Fit { window: u64, tab: u64 },
    /// Window `window` has been put back: its bar and title are written again, and the user is
    /// told the move was undone.
    Undone { window: u64 },
    /// The held tab `tab` is the one tab of a new window `window`, which comes up: window
    /// `from`'s size, at the screen point `at` if there is one (its title row under it, kept on
    /// a visible screen), else at `from`'s place, cascaded.
    OpenWindow {
        window: u64,
        from: u64,
        tab: u64,
        at: Option<(f64, f64)>,
    },
    /// The panes the steps before took join tab `tab` of window `window`, laid out as `tree`.
    /// The tab is not selected by it.
    AdoptPanes { window: u64, tab: u64, tree: Tree },
    /// Window `window` closes if the steps before left it without a tab; no program ends with it.
    CloseIfEmptied { window: u64 },
    /// Window `window` becomes the key window, in front.
    Raise { window: u64 },
    /// Tab `tab` comes on screen in window `window`.
    Select { window: u64, tab: u64 },
    /// The keyboard goes to pane `pane` of tab `tab`: into the pane when the tab is on screen,
    /// into the tab's memory of its focused pane when it is not.
    Focus { window: u64, tab: u64, pane: u64 },
    /// Tab `tab`'s chip in window `window` glows once: where the panes went.
    Pulse { window: u64, tab: u64 },
}

/// Where a tab joining a strip goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    /// Before tab number `n` (the strip's length is the end), and it comes up selected: the user
    /// carried it there to look at it.
    At(usize),
    /// At the end, the strip's selection where it was: it joined with others (Merge All
    /// Windows).
    End,
}

/// What a move comes to.
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    /// The steps that move the panes, in order. When panes fail to land, the steps after that
    /// one still run but [`Self::after`] and [`Self::undo`] do not.
    pub steps: Vec<Step>,
    /// The steps that follow once the panes have landed.
    pub after: Vec<Step>,
    /// The record Undo Move keeps, written after every step; `None` when the move cannot be
    /// taken back.
    pub undo: Option<Record>,
}

/// What `wanted` comes to among `world`'s windows, `host` answering where panes fit.
///
/// The order of the questions is the user's: a move naming nothing there is ends quietly, then
/// a window holding a question beeps, then a place with no room beeps.
pub fn plan(world: &[Window], wanted: Move, host: &dyn Host) -> Result<Plan, Refusal> {
    match wanted {
        Move::PaneToTab { pane, into, place } => pane_to_tab(world, pane, into, place, host),
        Move::TabToTab { tab, into, place } => tab_to_tab(world, tab, into, place, host),
        Move::PaneToNewTab { pane, window, gap } => pane_to_new_tab(world, pane, window, gap, host),
        Move::TabToNewWindow { tab, at } => tab_to_new_window(world, tab, at, host),
        Move::PaneToNewWindow { pane, at } => pane_to_new_window(world, pane, at, host),
        Move::TabToStrip { tab, window, index } => tab_to_strip(world, tab, window, index),
        Move::MergeAllWindows { into } => merge_all_windows(world, into),
        Move::Undo { record } => undo(world, &record),
    }
}

/// The window and the tab that hold pane `pane`.
fn holding_pane(world: &[Window], pane: u64) -> Option<(&Window, &Shape)> {
    world.iter().find_map(|window| {
        window
            .tabs
            .iter()
            .find(|shape| shape.tree.leaves().contains(&pane))
            .map(|shape| (window, shape))
    })
}

/// The window that holds tab `tab`, and the tab.
fn holding_tab(world: &[Window], tab: u64) -> Option<(&Window, &Shape)> {
    world
        .iter()
        .find_map(|window| window.tab(tab).map(|shape| (window, shape)))
}

fn pane_to_tab(
    world: &[Window],
    pane: u64,
    into: u64,
    place: Joins,
    host: &dyn Host,
) -> Result<Plan, Refusal> {
    let (from, source) = holding_pane(world, pane).ok_or(Refusal::Quiet)?;
    let (onto, target) = holding_tab(world, into).ok_or(Refusal::Quiet)?;
    if source.tab == into {
        return Err(Refusal::Quiet);
    }
    if source.tree.leaves().len() == 1 {
        return tab_to_tab(world, source.tab, into, place, host);
    }
    if from.asking || onto.asking {
        return Err(Refusal::Beep);
    }
    let tree = place
        .tree(target, &Tree::Leaf(pane), host)
        .ok_or(Refusal::Beep)?;
    let steps = vec![
        Step::ReleasePane {
            window: from.id,
            tab: source.tab,
            pane,
        },
        Step::AdoptPanes {
            window: onto.id,
            tab: into,
            tree,
        },
    ];
    if from.id == onto.id {
        // In its own window the target is not selected and the keyboard stays in the tab the
        // pane came from: the user goes on with what they were looking at.
        return Ok(Plan {
            steps,
            after: vec![Step::Pulse {
                window: onto.id,
                tab: into,
            }],
            undo: Some(Record {
                scenes: vec![from.scene(&[source.tab, into])],
                born: Vec::new(),
            }),
        });
    }
    // Into another window: that window comes forward with the tab it shows. The pane takes the
    // keyboard only if its new tab is the one on screen there; the tab is not brought up for a
    // single pane.
    let mut after = vec![Step::Raise { window: onto.id }];
    if onto.order.selected() == Some(into) {
        after.push(Step::Focus {
            window: onto.id,
            tab: into,
            pane,
        });
    }
    after.push(Step::Pulse {
        window: onto.id,
        tab: into,
    });
    Ok(Plan {
        steps,
        after,
        undo: Some(Record {
            scenes: vec![from.scene(&[source.tab]), onto.scene(&[into])],
            born: Vec::new(),
        }),
    })
}

fn tab_to_tab(
    world: &[Window],
    tab: u64,
    into: u64,
    place: Joins,
    host: &dyn Host,
) -> Result<Plan, Refusal> {
    if tab == into {
        return Err(Refusal::Quiet);
    }
    let (from, carried) = holding_tab(world, tab).ok_or(Refusal::Quiet)?;
    let (onto, target) = holding_tab(world, into).ok_or(Refusal::Quiet)?;
    if from.asking || onto.asking {
        return Err(Refusal::Beep);
    }
    let tree = place
        .tree(target, &carried.tree, host)
        .ok_or(Refusal::Beep)?;
    // The keyboard goes to the pane that had it in the tab that was carried.
    let focus = carried.focus;
    if from.id == onto.id {
        return Ok(Plan {
            steps: vec![
                Step::Fold {
                    window: from.id,
                    tab,
                    into,
                },
                Step::AdoptPanes {
                    window: from.id,
                    tab: into,
                    tree,
                },
            ],
            after: vec![
                Step::Focus {
                    window: from.id,
                    tab: into,
                    pane: focus,
                },
                Step::Pulse {
                    window: from.id,
                    tab: into,
                },
            ],
            undo: Some(Record {
                scenes: vec![from.scene(&[tab, into])],
                born: Vec::new(),
            }),
        });
    }
    // A whole tab is what the user was holding: in the window it went to, its tab comes up.
    // The window it left closes if that was its last tab, and then there is nothing to undo.
    let closes = from.closes_without(1);
    let mut steps = vec![
        Step::ReleaseTab {
            window: from.id,
            tab,
        },
        Step::Unpack {
            window: from.id,
            tab,
        },
        Step::AdoptPanes {
            window: onto.id,
            tab: into,
            tree,
        },
    ];
    steps.extend(from.close_if_emptied());
    Ok(Plan {
        steps,
        after: vec![
            Step::Raise { window: onto.id },
            Step::Select {
                window: onto.id,
                tab: into,
            },
            Step::Focus {
                window: onto.id,
                tab: into,
                pane: focus,
            },
            Step::Pulse {
                window: onto.id,
                tab: into,
            },
        ],
        undo: (!closes).then(|| Record {
            scenes: vec![from.scene(&[tab]), onto.scene(&[into])],
            born: Vec::new(),
        }),
    })
}

fn pane_to_new_tab(
    world: &[Window],
    pane: u64,
    window: u64,
    gap: usize,
    host: &dyn Host,
) -> Result<Plan, Refusal> {
    let (from, source) = holding_pane(world, pane).ok_or(Refusal::Quiet)?;
    let onto = world
        .iter()
        .find(|onto| onto.id == window)
        .ok_or(Refusal::Quiet)?;
    let panes = source.tree.leaves().len();
    if from.id == onto.id {
        if panes == 1 {
            // A tab's only pane is the tab: it takes that place in its strip, the way a tab
            // dragged along it does — nothing is shown or hidden, so a question up does not hold
            // it back.
            let at = from.order.index_of(source.tab).ok_or(Refusal::Quiet)?;
            let mut order = from.order.clone();
            if order.pane_to_new_tab(source.tab, panes, source.tab, gap) != NewTab::Reordered {
                return Err(Refusal::Quiet);
            }
            return Ok(Plan {
                steps: vec![Step::MoveTab {
                    window: from.id,
                    tab: source.tab,
                    index: gap_to_index(at, gap),
                }],
                after: vec![Step::Pulse {
                    window: from.id,
                    tab: source.tab,
                }],
                undo: Some(Record {
                    scenes: vec![from.scene(&[])],
                    born: Vec::new(),
                }),
            });
        }
        if from.asking {
            return Err(Refusal::Beep);
        }
        let new = host.fresh_id();
        if from
            .order
            .clone()
            .pane_to_new_tab(source.tab, panes, new, gap)
            != NewTab::Created
        {
            return Err(Refusal::Quiet);
        }
        return Ok(Plan {
            steps: vec![
                Step::ReleasePane {
                    window: from.id,
                    tab: source.tab,
                    pane,
                },
                Step::NewTab {
                    window: from.id,
                    tab: new,
                    gap,
                },
            ],
            after: vec![Step::Pulse {
                window: from.id,
                tab: new,
            }],
            undo: Some(Record {
                scenes: vec![from.scene(&[source.tab])],
                born: Vec::new(),
            }),
        });
    }
    if from.asking || onto.asking {
        return Err(Refusal::Beep);
    }
    // Into another window's strip the tab comes up selected, its window with it. The window the
    // pane's lone tab leaves closes if that was its last, and then there is nothing to undo.
    let (mut steps, tab) = if panes == 1 {
        (
            vec![Step::ReleaseTab {
                window: from.id,
                tab: source.tab,
            }],
            source.tab,
        )
    } else {
        let new = host.fresh_id();
        (
            vec![
                Step::ReleasePane {
                    window: from.id,
                    tab: source.tab,
                    pane,
                },
                Step::Wrap {
                    window: from.id,
                    tab: new,
                },
            ],
            new,
        )
    };
    steps.push(Step::AdoptTab {
        window: onto.id,
        tab,
        slot: Slot::At(gap),
    });
    steps.extend(from.close_if_emptied());
    let closes = panes == 1 && from.closes_without(1);
    Ok(Plan {
        steps,
        after: vec![
            Step::Raise { window: onto.id },
            Step::Pulse {
                window: onto.id,
                tab,
            },
        ],
        undo: (!closes).then(|| Record {
            scenes: vec![from.scene(&[source.tab]), onto.scene(&[])],
            born: Vec::new(),
        }),
    })
}

fn tab_to_new_window(
    world: &[Window],
    tab: u64,
    at: Option<(f64, f64)>,
    host: &dyn Host,
) -> Result<Plan, Refusal> {
    let (from, _) = holding_tab(world, tab).ok_or(Refusal::Quiet)?;
    // A window's only tab stays where it is: the window would close and another just like it
    // open. A window that outlives its tabs lets it go.
    if from.closes_without(1) || from.asking {
        return Err(Refusal::Beep);
    }
    // Nothing to undo: a window is not a picture Undo Move can paint again, and the tab leaving
    // its strip drops the record there was.
    Ok(Plan {
        steps: vec![
            Step::ReleaseTab {
                window: from.id,
                tab,
            },
            Step::OpenWindow {
                window: host.fresh_id(),
                from: from.id,
                tab,
                at,
            },
        ],
        after: Vec::new(),
        undo: None,
    })
}

fn pane_to_new_window(
    world: &[Window],
    pane: u64,
    at: Option<(f64, f64)>,
    host: &dyn Host,
) -> Result<Plan, Refusal> {
    let (from, source) = holding_pane(world, pane).ok_or(Refusal::Quiet)?;
    if source.tree.leaves().len() == 1 {
        return tab_to_new_window(world, source.tab, at, host);
    }
    if from.asking {
        return Err(Refusal::Beep);
    }
    let tab = host.fresh_id();
    let window = host.fresh_id();
    // Undo Move takes the pane back from the window it made, and closes that window.
    Ok(Plan {
        steps: vec![
            Step::ReleasePane {
                window: from.id,
                tab: source.tab,
                pane,
            },
            Step::Wrap {
                window: from.id,
                tab,
            },
            Step::OpenWindow {
                window,
                from: from.id,
                tab,
                at,
            },
        ],
        after: Vec::new(),
        undo: Some(Record {
            scenes: vec![from.scene(&[source.tab])],
            born: vec![window],
        }),
    })
}

fn tab_to_strip(world: &[Window], tab: u64, window: u64, index: usize) -> Result<Plan, Refusal> {
    let (from, _) = holding_tab(world, tab).ok_or(Refusal::Quiet)?;
    let onto = world
        .iter()
        .find(|onto| onto.id == window)
        .ok_or(Refusal::Quiet)?;
    if from.id == onto.id {
        // Along its own strip nothing is shown or hidden, so a question up does not hold it back;
        // nothing to undo either: the strip is all it changed.
        if !from.order.clone().move_to(tab, index) {
            return Err(Refusal::Quiet);
        }
        return Ok(Plan {
            steps: vec![Step::MoveTab {
                window: from.id,
                tab,
                index,
            }],
            after: Vec::new(),
            undo: None,
        });
    }
    if from.asking || onto.asking {
        return Err(Refusal::Beep);
    }
    Ok(Plan {
        steps: vec![
            Step::ReleaseTab {
                window: from.id,
                tab,
            },
            Step::AdoptTab {
                window: onto.id,
                tab,
                slot: Slot::At(index),
            },
        ]
        .into_iter()
        .chain(from.close_if_emptied())
        .collect(),
        after: vec![Step::Raise { window: onto.id }],
        undo: None,
    })
}

fn merge_all_windows(world: &[Window], into: u64) -> Result<Plan, Refusal> {
    let target = world
        .iter()
        .find(|window| window.id == into)
        .ok_or(Refusal::Quiet)?;
    let others: Vec<&Window> = world.iter().filter(|window| window.id != into).collect();
    if others.is_empty() {
        return Err(Refusal::Quiet);
    }
    if target.asking || others.iter().any(|window| window.asking) {
        return Err(Refusal::Beep);
    }
    let mut steps = Vec::new();
    for other in others {
        // Its selected tab leaves last: it is the one that leaves the screen, and the window it
        // leaves is closing anyway. They join in strip order.
        let selected = other.order.selected();
        let ids = other.order.ids();
        for &tab in ids.iter().filter(|&&tab| Some(tab) != selected) {
            steps.push(Step::ReleaseTab {
                window: other.id,
                tab,
            });
        }
        steps.extend(selected.map(|tab| Step::ReleaseTab {
            window: other.id,
            tab,
        }));
        for &tab in ids {
            steps.push(Step::AdoptTab {
                window: into,
                tab,
                slot: Slot::End,
            });
        }
        steps.extend(other.close_if_emptied());
    }
    Ok(Plan {
        steps,
        after: vec![Step::Raise { window: into }],
        undo: None,
    })
}

/// What window `window` holds now, as Undo Move's record asks it ([`Record::holds`]).
fn now(window: &Window) -> Now {
    Now {
        window: window.id,
        tabs: window
            .tabs
            .iter()
            .map(|shape| (shape.tab, shape.tree.leaves()))
            .collect(),
    }
}

/// Whether Undo Move's `record` is still a picture of what `world` holds: every window it names
/// is there and the panes it accounts for are exactly the ones there are ([`Record::holds`]).
pub fn standing(world: &[Window], record: &Record) -> bool {
    record.holds(&world.iter().map(now).collect::<Vec<_>>())
}

fn undo(world: &[Window], record: &Record) -> Result<Plan, Refusal> {
    if !standing(world, record) {
        return Err(Refusal::Stale);
    }
    let named = |id: u64| world.iter().find(|window| window.id == id);
    let windows: Vec<&Window> = record
        .scenes
        .iter()
        .map(|scene| scene.window)
        .chain(record.born.iter().copied())
        .filter_map(named)
        .collect();
    if windows.iter().any(|window| window.asking) {
        return Err(Refusal::Beep);
    }
    let mut steps = Vec::new();
    // What the move made comes apart: each touched tab gives back the panes that are not its own
    // in the picture (it keeps at least one, or the picture would not stand), each tab the move
    // made is taken apart, each window it made is emptied and closes.
    for scene in &record.scenes {
        let Some(window) = named(scene.window) else {
            continue;
        };
        for now in &window.tabs {
            if let Some(shape) = scene.shape(now.tab) {
                let own = shape.tree.leaves();
                for pane in now.tree.leaves() {
                    if !own.contains(&pane) {
                        steps.push(Step::ReleasePane {
                            window: window.id,
                            tab: now.tab,
                            pane,
                        });
                    }
                }
            } else if scene.made(now.tab) {
                steps.push(Step::Dissolve {
                    window: window.id,
                    tab: now.tab,
                });
            }
        }
    }
    for &born in &record.born {
        let Some(window) = named(born) else {
            continue;
        };
        for &tab in window.order.ids() {
            steps.push(Step::ReleaseTab { window: born, tab });
            steps.push(Step::Unpack { window: born, tab });
        }
        steps.push(Step::CloseIfEmptied { window: born });
    }
    // Then each tab and strip as it was: the keyboard goes to the pane that had it, and a tree
    // that no longer fits the window's smallest panes is evened out, as a restored one is.
    for scene in &record.scenes {
        let window = scene.window;
        for shape in &scene.shapes {
            steps.push(Step::Reshape {
                window,
                tab: shape.tab,
                tree: shape.tree.clone(),
                name: shape.name.clone(),
            });
        }
        steps.push(Step::PutStrip {
            window,
            order: scene.order.clone(),
        });
        for shape in &scene.shapes {
            steps.push(Step::Focus {
                window,
                tab: shape.tab,
                pane: shape.focus,
            });
            steps.push(Step::Fit {
                window,
                tab: shape.tab,
            });
        }
        steps.push(Step::Undone { window });
    }
    // The window the move made was the key one and is gone: the first it touched comes forward.
    let after = match record.scenes.first() {
        Some(scene) if !record.born.is_empty() => vec![Step::Raise {
            window: scene.window,
        }],
        _ => Vec::new(),
    };
    Ok(Plan {
        steps,
        after,
        undo: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::cell::{Cell, RefCell};

    use crate::split::Axis;

    /// Places beside a tab's focused pane by putting the incoming tree to its right at an even
    /// share — the arithmetic is `split`'s and is tested there; here only "it fits or not" and
    /// what was asked matter.
    struct Roomy {
        world: Vec<Window>,
        asked: RefCell<Vec<(u64, Direction, Tree)>>,
        /// The next fresh id: [`NEW`], then counting up.
        next: Cell<u64>,
    }

    impl Host for Roomy {
        fn beside(&self, tab: u64, side: Direction, incoming: &Tree) -> Option<Tree> {
            self.asked.borrow_mut().push((tab, side, incoming.clone()));
            let (_, shape) = holding_tab(&self.world, tab)?;
            Some(split(shape.tree.clone(), incoming.clone()))
        }

        fn fresh_id(&self) -> u64 {
            let id = self.next.get();
            self.next.set(id + 1);
            id
        }
    }

    /// Never room.
    struct Cramped;

    impl Host for Cramped {
        fn beside(&self, _: u64, _: Direction, _: &Tree) -> Option<Tree> {
            None
        }

        fn fresh_id(&self) -> u64 {
            NEW
        }
    }

    /// The id a tab a move makes is given.
    const NEW: u64 = 50;

    fn split(first: Tree, second: Tree) -> Tree {
        Tree::Split {
            axis: Axis::Horizontal,
            ratio: 0.5,
            first: Box::new(first),
            second: Box::new(second),
        }
    }

    fn shape(tab: u64, tree: Tree, focus: u64) -> Shape {
        Shape {
            tab,
            name: None,
            tree,
            focus,
        }
    }

    fn window(id: u64, tabs: Vec<Shape>, selected: u64) -> Window {
        let mut order = Tabs::new(tabs[0].tab);
        for shape in &tabs[1..] {
            order.append(shape.tab);
        }
        order.select(selected);
        Window {
            id,
            order,
            tabs,
            asking: false,
            stays_empty: false,
        }
    }

    /// Window 1: tab 10 holds panes 100 and 101 (101 focused), tab 11 holds pane 110, tab 10 on
    /// screen. Window 2: tab 20 holds pane 200, tab 21 holds 210 and 211, tab 20 on screen.
    fn world() -> Vec<Window> {
        vec![
            window(
                1,
                vec![
                    shape(10, split(Tree::Leaf(100), Tree::Leaf(101)), 101),
                    shape(11, Tree::Leaf(110), 110),
                ],
                10,
            ),
            window(
                2,
                vec![
                    shape(20, Tree::Leaf(200), 200),
                    shape(21, split(Tree::Leaf(210), Tree::Leaf(211)), 210),
                ],
                20,
            ),
        ]
    }

    fn roomy(world: &[Window]) -> Roomy {
        Roomy {
            world: world.to_vec(),
            asked: RefCell::new(Vec::new()),
            next: Cell::new(NEW),
        }
    }

    fn beside() -> Joins {
        Joins::Beside(Direction::Right)
    }

    #[test]
    fn a_pane_joins_another_tab_of_its_window_and_the_user_stays_where_they_were() {
        let world = world();
        let host = roomy(&world);
        let plan = plan(
            &world,
            Move::PaneToTab {
                pane: 101,
                into: 11,
                place: beside(),
            },
            &host,
        )
        .unwrap();
        assert_eq!(
            host.asked.borrow().as_slice(),
            &[(11, Direction::Right, Tree::Leaf(101))]
        );
        assert_eq!(
            plan.steps,
            vec![
                Step::ReleasePane {
                    window: 1,
                    tab: 10,
                    pane: 101
                },
                Step::AdoptPanes {
                    window: 1,
                    tab: 11,
                    tree: split(Tree::Leaf(110), Tree::Leaf(101)),
                },
            ]
        );
        assert_eq!(plan.after, vec![Step::Pulse { window: 1, tab: 11 }]);
        assert_eq!(
            plan.undo,
            Some(Record {
                scenes: vec![world[0].scene(&[10, 11])],
                born: Vec::new(),
            })
        );
    }

    #[test]
    fn a_tabs_only_pane_joins_as_its_tab_and_the_screen_follows_it() {
        let mut world = world();
        world[0].order.select(11);
        let host = roomy(&world);
        let plan = plan(
            &world,
            Move::PaneToTab {
                pane: 110,
                into: 10,
                place: beside(),
            },
            &host,
        )
        .unwrap();
        assert_eq!(
            plan.steps,
            vec![
                Step::Fold {
                    window: 1,
                    tab: 11,
                    into: 10
                },
                Step::AdoptPanes {
                    window: 1,
                    tab: 10,
                    tree: split(split(Tree::Leaf(100), Tree::Leaf(101)), Tree::Leaf(110)),
                },
            ]
        );
        assert_eq!(
            plan.after,
            vec![
                Step::Focus {
                    window: 1,
                    tab: 10,
                    pane: 110
                },
                Step::Pulse { window: 1, tab: 10 },
            ]
        );
        assert_eq!(
            plan.undo,
            Some(Record {
                scenes: vec![world[0].scene(&[11, 10])],
                born: Vec::new(),
            })
        );
        // The chip's merge is the same move by the tab's name.
        let merged = super::plan(
            &world,
            Move::TabToTab {
                tab: 11,
                into: 10,
                place: beside(),
            },
            &host,
        );
        assert_eq!(merged, Ok(plan));
    }

    #[test]
    fn a_pane_crossing_windows_raises_the_window_and_takes_the_keyboard_only_on_screen() {
        let world = world();
        let host = roomy(&world);
        let off_screen = plan(
            &world,
            Move::PaneToTab {
                pane: 101,
                into: 21,
                place: beside(),
            },
            &host,
        )
        .unwrap();
        assert_eq!(
            off_screen.steps,
            vec![
                Step::ReleasePane {
                    window: 1,
                    tab: 10,
                    pane: 101
                },
                Step::AdoptPanes {
                    window: 2,
                    tab: 21,
                    tree: split(split(Tree::Leaf(210), Tree::Leaf(211)), Tree::Leaf(101)),
                },
            ]
        );
        assert_eq!(
            off_screen.after,
            vec![
                Step::Raise { window: 2 },
                Step::Pulse { window: 2, tab: 21 },
            ]
        );
        assert_eq!(
            off_screen.undo,
            Some(Record {
                scenes: vec![world[0].scene(&[10]), world[1].scene(&[21])],
                born: Vec::new(),
            })
        );

        let on_screen = plan(
            &world,
            Move::PaneToTab {
                pane: 101,
                into: 20,
                place: beside(),
            },
            &host,
        )
        .unwrap();
        assert_eq!(
            on_screen.after,
            vec![
                Step::Raise { window: 2 },
                Step::Focus {
                    window: 2,
                    tab: 20,
                    pane: 101
                },
                Step::Pulse { window: 2, tab: 20 },
            ]
        );
    }

    #[test]
    fn a_tab_crossing_windows_comes_up_there_and_an_emptied_window_cannot_be_undone() {
        let world = world();
        let host = roomy(&world);
        let plan = plan(
            &world,
            Move::TabToTab {
                tab: 10,
                into: 21,
                place: beside(),
            },
            &host,
        )
        .unwrap();
        assert_eq!(
            plan.steps,
            vec![
                Step::ReleaseTab { window: 1, tab: 10 },
                Step::Unpack { window: 1, tab: 10 },
                Step::AdoptPanes {
                    window: 2,
                    tab: 21,
                    tree: split(
                        split(Tree::Leaf(210), Tree::Leaf(211)),
                        split(Tree::Leaf(100), Tree::Leaf(101))
                    ),
                },
                Step::CloseIfEmptied { window: 1 },
            ]
        );
        assert_eq!(
            plan.after,
            vec![
                Step::Raise { window: 2 },
                Step::Select { window: 2, tab: 21 },
                Step::Focus {
                    window: 2,
                    tab: 21,
                    pane: 101
                },
                Step::Pulse { window: 2, tab: 21 },
            ]
        );
        assert_eq!(
            plan.undo,
            Some(Record {
                scenes: vec![world[0].scene(&[10]), world[1].scene(&[21])],
                born: Vec::new(),
            })
        );

        // Window 2's tab 20 is its only pane's tab: the pane crosses as the tab, and the window
        // it leaves keeps tab 21 — still undoable. Alone in its window, the move closes it.
        let crossing = |world: &[Window]| {
            super::plan(
                world,
                Move::PaneToTab {
                    pane: 200,
                    into: 11,
                    place: beside(),
                },
                &host,
            )
            .unwrap()
        };
        assert!(crossing(&world).undo.is_some());
        let mut alone = world.clone();
        alone[1] = window(2, vec![shape(20, Tree::Leaf(200), 200)], 20);
        let plan = crossing(&alone);
        assert_eq!(plan.steps[0], Step::ReleaseTab { window: 2, tab: 20 });
        assert_eq!(plan.steps[3], Step::CloseIfEmptied { window: 2 });
        assert_eq!(plan.undo, None);
    }

    #[test]
    fn a_landing_the_pointer_chose_is_taken_only_for_exactly_these_panes() {
        let world = world();
        let landing = split(Tree::Leaf(101), Tree::Leaf(110));
        let plan = plan(
            &world,
            Move::PaneToTab {
                pane: 101,
                into: 11,
                place: Joins::Planned(landing.clone()),
            },
            &Cramped,
        )
        .unwrap();
        assert_eq!(
            plan.steps[1],
            Step::AdoptPanes {
                window: 1,
                tab: 11,
                tree: landing
            }
        );
        // Shown before tab 11 gained or lost a pane: refused.
        let stale = super::plan(
            &world,
            Move::PaneToTab {
                pane: 101,
                into: 11,
                place: Joins::Planned(split(Tree::Leaf(101), Tree::Leaf(999))),
            },
            &Cramped,
        );
        assert_eq!(stale, Err(Refusal::Beep));
    }

    #[test]
    fn a_window_holding_a_question_or_no_room_beeps() {
        let mut world = world();
        let pane = Move::PaneToTab {
            pane: 101,
            into: 21,
            place: beside(),
        };
        assert_eq!(plan(&world, pane.clone(), &Cramped), Err(Refusal::Beep));
        for asking in [0, 1] {
            world[asking].asking = true;
            let host = roomy(&world);
            assert_eq!(plan(&world, pane.clone(), &host), Err(Refusal::Beep));
            // Asked before the room is: nothing is measured for a move that cannot happen.
            assert!(host.asked.borrow().is_empty());
            world[asking].asking = false;
        }
    }

    #[test]
    fn a_pane_becomes_a_tab_of_its_window_and_the_user_stays_where_they_were() {
        let world = world();
        let host = roomy(&world);
        let plan = plan(
            &world,
            Move::PaneToNewTab {
                pane: 101,
                window: 1,
                gap: 1,
            },
            &host,
        )
        .unwrap();
        assert_eq!(
            plan.steps,
            vec![
                Step::ReleasePane {
                    window: 1,
                    tab: 10,
                    pane: 101
                },
                Step::NewTab {
                    window: 1,
                    tab: NEW,
                    gap: 1
                },
            ]
        );
        assert_eq!(
            plan.after,
            vec![Step::Pulse {
                window: 1,
                tab: NEW
            }]
        );
        assert_eq!(
            plan.undo,
            Some(Record {
                scenes: vec![world[0].scene(&[10])],
                born: Vec::new(),
            })
        );

        let mut asking = world.clone();
        asking[0].asking = true;
        let refused = super::plan(
            &asking,
            Move::PaneToNewTab {
                pane: 101,
                window: 1,
                gap: 1,
            },
            &host,
        );
        assert_eq!(refused, Err(Refusal::Beep));
    }

    #[test]
    fn a_tabs_only_pane_let_go_between_its_windows_tabs_moves_its_tab_there() {
        let mut world = world();
        let host = roomy(&world);
        let to_front = Move::PaneToNewTab {
            pane: 110,
            window: 1,
            gap: 0,
        };
        let plan = plan(&world, to_front.clone(), &host).unwrap();
        assert_eq!(
            plan.steps,
            vec![Step::MoveTab {
                window: 1,
                tab: 11,
                index: 0
            }]
        );
        assert_eq!(plan.after, vec![Step::Pulse { window: 1, tab: 11 }]);
        assert_eq!(
            plan.undo,
            Some(Record {
                scenes: vec![world[0].scene(&[])],
                born: Vec::new(),
            })
        );
        // Its own place, on either side of it, is no move.
        for gap in [1, 2] {
            assert_eq!(
                super::plan(
                    &world,
                    Move::PaneToNewTab {
                        pane: 110,
                        window: 1,
                        gap
                    },
                    &host
                ),
                Err(Refusal::Quiet),
                "gap {gap}"
            );
        }
        // A reorder shows and hides nothing: a question up does not hold it back.
        world[0].asking = true;
        assert_eq!(super::plan(&world, to_front, &host), Ok(plan));
    }

    #[test]
    fn a_pane_let_go_in_another_windows_strip_comes_up_there_as_a_tab() {
        let world = world();
        let host = roomy(&world);
        let plan = plan(
            &world,
            Move::PaneToNewTab {
                pane: 101,
                window: 2,
                gap: 1,
            },
            &host,
        )
        .unwrap();
        assert_eq!(
            plan.steps,
            vec![
                Step::ReleasePane {
                    window: 1,
                    tab: 10,
                    pane: 101
                },
                Step::Wrap {
                    window: 1,
                    tab: NEW
                },
                Step::AdoptTab {
                    window: 2,
                    tab: NEW,
                    slot: Slot::At(1)
                },
                Step::CloseIfEmptied { window: 1 },
            ]
        );
        assert_eq!(
            plan.after,
            vec![
                Step::Raise { window: 2 },
                Step::Pulse {
                    window: 2,
                    tab: NEW
                },
            ]
        );
        assert_eq!(
            plan.undo,
            Some(Record {
                scenes: vec![world[0].scene(&[10]), world[1].scene(&[])],
                born: Vec::new(),
            })
        );
    }

    #[test]
    fn a_tabs_only_pane_crosses_as_its_tab_and_an_emptied_window_cannot_be_undone() {
        let mut world = world();
        let host = roomy(&world);
        let crossing = Move::PaneToNewTab {
            pane: 110,
            window: 2,
            gap: 0,
        };
        let plan = plan(&world, crossing.clone(), &host).unwrap();
        assert_eq!(
            plan.steps,
            vec![
                Step::ReleaseTab { window: 1, tab: 11 },
                Step::AdoptTab {
                    window: 2,
                    tab: 11,
                    slot: Slot::At(0)
                },
                Step::CloseIfEmptied { window: 1 },
            ]
        );
        assert_eq!(
            plan.after,
            vec![
                Step::Raise { window: 2 },
                Step::Pulse { window: 2, tab: 11 },
            ]
        );
        assert_eq!(
            plan.undo,
            Some(Record {
                scenes: vec![world[0].scene(&[11]), world[1].scene(&[])],
                born: Vec::new(),
            })
        );

        world[0] = window(1, vec![shape(11, Tree::Leaf(110), 110)], 11);
        assert_eq!(
            super::plan(&world, crossing.clone(), &host).unwrap().undo,
            None
        );
        world[1].asking = true;
        assert_eq!(super::plan(&world, crossing, &host), Err(Refusal::Beep));
    }

    #[test]
    fn a_tab_becomes_a_window_of_its_own_where_it_was_let_go() {
        let mut world = world();
        let host = roomy(&world);
        let at = Some((640.0, 480.0));
        let plan = plan(&world, Move::TabToNewWindow { tab: 10, at }, &host).unwrap();
        assert_eq!(
            plan.steps,
            vec![
                Step::ReleaseTab { window: 1, tab: 10 },
                Step::OpenWindow {
                    window: NEW,
                    from: 1,
                    tab: 10,
                    at
                },
            ]
        );
        assert!(plan.after.is_empty());
        assert_eq!(plan.undo, None, "a window is not a picture to paint again");

        // A window's only tab stays, and a question holds the window.
        let lone = Move::TabToNewWindow { tab: 20, at: None };
        let mut alone = world.clone();
        alone[1] = window(2, vec![shape(20, Tree::Leaf(200), 200)], 20);
        assert_eq!(super::plan(&alone, lone, &host), Err(Refusal::Beep));
        world[0].asking = true;
        assert_eq!(
            super::plan(&world, Move::TabToNewWindow { tab: 10, at: None }, &host),
            Err(Refusal::Beep)
        );
    }

    #[test]
    fn a_pane_becomes_a_window_undo_move_takes_back() {
        let world = world();
        let host = roomy(&world);
        let plan = plan(
            &world,
            Move::PaneToNewWindow {
                pane: 101,
                at: None,
            },
            &host,
        )
        .unwrap();
        assert_eq!(
            plan.steps,
            vec![
                Step::ReleasePane {
                    window: 1,
                    tab: 10,
                    pane: 101
                },
                Step::Wrap {
                    window: 1,
                    tab: NEW
                },
                Step::OpenWindow {
                    window: NEW + 1,
                    from: 1,
                    tab: NEW,
                    at: None
                },
            ]
        );
        assert_eq!(
            plan.undo,
            Some(Record {
                scenes: vec![world[0].scene(&[10])],
                born: vec![NEW + 1],
            })
        );

        // A tab's only pane takes its tab, as Move Tab to New Window does.
        assert_eq!(
            super::plan(
                &world,
                Move::PaneToNewWindow {
                    pane: 110,
                    at: None
                },
                &roomy(&world)
            ),
            super::plan(
                &world,
                Move::TabToNewWindow { tab: 11, at: None },
                &roomy(&world)
            )
        );
        let mut asking = world.clone();
        asking[0].asking = true;
        assert_eq!(
            super::plan(
                &asking,
                Move::PaneToNewWindow {
                    pane: 101,
                    at: None
                },
                &host
            ),
            Err(Refusal::Beep)
        );
    }

    #[test]
    fn a_tab_let_go_on_a_strip_takes_its_place_there() {
        let mut world = world();
        let along = Move::TabToStrip {
            tab: 10,
            window: 1,
            index: 1,
        };
        let plan = plan(&world, along.clone(), &Cramped).unwrap();
        assert_eq!(
            plan.steps,
            vec![Step::MoveTab {
                window: 1,
                tab: 10,
                index: 1
            }]
        );
        assert!(plan.after.is_empty());
        assert_eq!(plan.undo, None);
        assert_eq!(
            super::plan(
                &world,
                Move::TabToStrip {
                    tab: 10,
                    window: 1,
                    index: 0
                },
                &Cramped
            ),
            Err(Refusal::Quiet),
            "its own place"
        );

        let across = Move::TabToStrip {
            tab: 10,
            window: 2,
            index: 1,
        };
        let plan = super::plan(&world, across.clone(), &Cramped).unwrap();
        assert_eq!(
            plan.steps,
            vec![
                Step::ReleaseTab { window: 1, tab: 10 },
                Step::AdoptTab {
                    window: 2,
                    tab: 10,
                    slot: Slot::At(1)
                },
                Step::CloseIfEmptied { window: 1 },
            ]
        );
        assert_eq!(plan.after, vec![Step::Raise { window: 2 }]);
        assert_eq!(plan.undo, None);

        world[1].asking = true;
        assert_eq!(super::plan(&world, across, &Cramped), Err(Refusal::Beep));
        world[1].asking = false;
        world[0].asking = true;
        assert!(
            super::plan(&world, along, &Cramped).is_ok(),
            "a reorder shows and hides nothing"
        );
    }

    #[test]
    fn merging_all_windows_brings_their_tabs_to_the_end_and_closes_them() {
        let mut world = world();
        world.push(window(
            3,
            vec![
                shape(30, Tree::Leaf(300), 300),
                shape(31, Tree::Leaf(310), 310),
                shape(32, Tree::Leaf(320), 320),
            ],
            31,
        ));
        let plan = plan(&world, Move::MergeAllWindows { into: 2 }, &Cramped).unwrap();
        let end = |tab| Step::AdoptTab {
            window: 2,
            tab,
            slot: Slot::End,
        };
        assert_eq!(
            plan.steps,
            vec![
                Step::ReleaseTab { window: 1, tab: 11 },
                Step::ReleaseTab { window: 1, tab: 10 },
                end(10),
                end(11),
                Step::CloseIfEmptied { window: 1 },
                Step::ReleaseTab { window: 3, tab: 30 },
                Step::ReleaseTab { window: 3, tab: 32 },
                Step::ReleaseTab { window: 3, tab: 31 },
                end(30),
                end(31),
                end(32),
                Step::CloseIfEmptied { window: 3 },
            ]
        );
        assert_eq!(plan.after, vec![Step::Raise { window: 2 }]);
        assert_eq!(plan.undo, None);

        assert_eq!(
            super::plan(&world[1..2], Move::MergeAllWindows { into: 2 }, &Cramped),
            Err(Refusal::Quiet),
            "no other window"
        );
        world[2].asking = true;
        assert_eq!(
            super::plan(&world, Move::MergeAllWindows { into: 2 }, &Cramped),
            Err(Refusal::Beep)
        );
    }

    /// Tab `tab` of `window` holds `tree` now, its focus on `focus`.
    fn reshaped(window: &mut Window, tab: u64, tree: Tree, focus: u64) {
        let shape = window
            .tabs
            .iter_mut()
            .find(|shape| shape.tab == tab)
            .expect("the tab is in the window");
        shape.tree = tree;
        shape.focus = focus;
    }

    /// What taking `record` back from `world` comes to.
    fn undoing(world: &[Window], record: Record) -> Result<Plan, Refusal> {
        plan(world, Move::Undo { record }, &Cramped)
    }

    #[test]
    fn undo_move_takes_a_crossed_pane_back_and_puts_both_strips_as_they_were() {
        let before = world();
        let record = plan(
            &before,
            Move::PaneToTab {
                pane: 101,
                into: 21,
                place: beside(),
            },
            &roomy(&before),
        )
        .unwrap()
        .undo
        .unwrap();
        let mut after = before.clone();
        reshaped(&mut after[0], 10, Tree::Leaf(100), 100);
        reshaped(
            &mut after[1],
            21,
            split(split(Tree::Leaf(210), Tree::Leaf(211)), Tree::Leaf(101)),
            210,
        );
        after[1].order.select(21);
        assert!(standing(&after, &record));

        let plan = undoing(&after, record).unwrap();
        let put_back = |window: &Window, tab: u64, focus: u64| {
            let shape = window.tab(tab).unwrap();
            vec![
                Step::Reshape {
                    window: window.id,
                    tab,
                    tree: shape.tree.clone(),
                    name: None,
                },
                Step::PutStrip {
                    window: window.id,
                    order: window.order.clone(),
                },
                Step::Focus {
                    window: window.id,
                    tab,
                    pane: focus,
                },
                Step::Fit {
                    window: window.id,
                    tab,
                },
                Step::Undone { window: window.id },
            ]
        };
        let mut expected = vec![Step::ReleasePane {
            window: 2,
            tab: 21,
            pane: 101,
        }];
        expected.extend(put_back(&before[0], 10, 101));
        expected.extend(put_back(&before[1], 21, 210));
        assert_eq!(plan.steps, expected);
        assert!(plan.after.is_empty());
        assert_eq!(plan.undo, None, "there is no redo");
    }

    #[test]
    fn undo_move_takes_apart_the_tab_and_the_window_a_move_made() {
        let before = world();
        let host = roomy(&before);
        let record = plan(
            &before,
            Move::PaneToNewTab {
                pane: 101,
                window: 1,
                gap: 1,
            },
            &host,
        )
        .unwrap()
        .undo
        .unwrap();
        let mut after = before.clone();
        reshaped(&mut after[0], 10, Tree::Leaf(100), 100);
        after[0].tabs.insert(1, shape(NEW, Tree::Leaf(101), 101));
        after[0].order.place_new(NEW, 1);
        let plan = undoing(&after, record).unwrap();
        assert_eq!(
            plan.steps[0],
            Step::Dissolve {
                window: 1,
                tab: NEW
            }
        );
        assert_eq!(
            plan.steps[2],
            Step::PutStrip {
                window: 1,
                order: before[0].order.clone()
            }
        );

        let record = super::plan(
            &before,
            Move::PaneToNewWindow {
                pane: 101,
                at: None,
            },
            &host,
        )
        .unwrap()
        .undo
        .unwrap();
        let born = record.born[0];
        let mut after = before.clone();
        reshaped(&mut after[0], 10, Tree::Leaf(100), 100);
        after.push(window(born, vec![shape(77, Tree::Leaf(101), 101)], 77));
        let plan = undoing(&after, record).unwrap();
        assert_eq!(
            plan.steps[..3],
            [
                Step::ReleaseTab {
                    window: born,
                    tab: 77
                },
                Step::Unpack {
                    window: born,
                    tab: 77
                },
                Step::CloseIfEmptied { window: born },
            ]
        );
        assert_eq!(
            plan.after,
            vec![Step::Raise { window: 1 }],
            "the window the move made was the key one and is gone"
        );
    }

    #[test]
    fn undo_move_beeps_while_a_question_holds_and_drops_a_picture_no_longer_true() {
        let before = world();
        let record = plan(
            &before,
            Move::PaneToTab {
                pane: 101,
                into: 11,
                place: beside(),
            },
            &roomy(&before),
        )
        .unwrap()
        .undo
        .unwrap();
        let mut after = before.clone();
        reshaped(&mut after[0], 10, Tree::Leaf(100), 100);
        reshaped(
            &mut after[0],
            11,
            split(Tree::Leaf(110), Tree::Leaf(101)),
            110,
        );
        after[0].asking = true;
        assert_eq!(undoing(&after, record.clone()), Err(Refusal::Beep));
        after[0].asking = false;
        // Pane 110 closed since: the picture would bring back a pane that is not there.
        reshaped(&mut after[0], 11, Tree::Leaf(101), 101);
        assert!(!standing(&after, &record));
        assert_eq!(undoing(&after, record), Err(Refusal::Stale));
    }

    #[test]
    fn a_window_that_outlives_its_tabs_stays_open_and_its_move_can_be_taken_back() {
        let mut world = world();
        world[0] = window(
            1,
            vec![shape(10, split(Tree::Leaf(100), Tree::Leaf(101)), 101)],
            10,
        );
        world[0].stays_empty = true;
        let host = roomy(&world);

        // Its only tab joins another window's tab: no step closes it, and Undo Move keeps a
        // record, since the window is there to put the tab back in.
        let plan = plan(
            &world,
            Move::TabToTab {
                tab: 10,
                into: 21,
                place: beside(),
            },
            &host,
        )
        .unwrap();
        assert!(!plan.steps.contains(&Step::CloseIfEmptied { window: 1 }));
        let record = plan.undo.clone().expect("the window it left stays");

        // Its only tab may leave for a window of its own too.
        assert!(super::plan(&world, Move::TabToNewWindow { tab: 10, at: None }, &host).is_ok());

        // Taken back, the tab is born again in the window that stayed empty.
        let mut after = world.clone();
        after[0].tabs.clear();
        after[0].order = Tabs::empty();
        reshaped(
            &mut after[1],
            21,
            split(
                split(Tree::Leaf(210), Tree::Leaf(211)),
                split(Tree::Leaf(100), Tree::Leaf(101)),
            ),
            101,
        );
        let undo = undoing(&after, record).unwrap();
        assert!(undo.steps.contains(&Step::Reshape {
            window: 1,
            tab: 10,
            tree: world[0].tabs[0].tree.clone(),
            name: None,
        }));
        assert!(undo.steps.contains(&Step::PutStrip {
            window: 1,
            order: world[0].order.clone(),
        }));
    }

    #[test]
    fn a_move_naming_nothing_there_is_ends_quietly() {
        let world = world();
        let host = roomy(&world);
        for wanted in [
            Move::PaneToTab {
                pane: 999,
                into: 11,
                place: beside(),
            },
            Move::PaneToTab {
                pane: 101,
                into: 99,
                place: beside(),
            },
            Move::PaneToTab {
                pane: 101,
                into: 10,
                place: beside(),
            },
            Move::TabToTab {
                tab: 10,
                into: 10,
                place: beside(),
            },
            Move::TabToTab {
                tab: 99,
                into: 10,
                place: beside(),
            },
            Move::PaneToNewTab {
                pane: 101,
                window: 9,
                gap: 0,
            },
            Move::TabToNewWindow { tab: 99, at: None },
            Move::PaneToNewWindow {
                pane: 999,
                at: None,
            },
        ] {
            assert_eq!(
                plan(&world, wanted.clone(), &host),
                Err(Refusal::Quiet),
                "{wanted:?}"
            );
        }
    }
}
