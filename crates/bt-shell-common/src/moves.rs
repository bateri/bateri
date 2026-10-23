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
//! **Groups go with the strip.** A window's tabs may be gathered in named groups
//! ([`crate::groups`]): the picture carries them beside the strip ([`Window::groups`]), a plan
//! that moves a tab lets them follow the rules there (a group left empty is gone, a tab joining a
//! strip lands in the cluster of the place it was let go at), Undo Move's record puts them back
//! with the strip, and the moves of tabs *between* groups and of groups themselves
//! ([`Move::RegroupTabs`], [`Move::MoveGroup`], [`Move::GroupToWindow`],
//! [`Move::GroupToNewWindow`], [`Move::TabsToNewWindow`]) are planned here like any other.

use crate::groups::{GroupId, Groups, Target};
use crate::split::{Direction, Tree};
use crate::tabs::{NewTab, Tabs, gap_to_index};
use crate::undo::{Now, Record, Scene, Shape};

/// A window as a move sees it.
#[derive(Clone, Debug, PartialEq)]
pub struct Window {
    pub id: u64,
    /// The strip: the tabs in order and the selected one.
    pub order: Tabs<u64>,
    /// The strip's tab groups: who is gathered under which name. Empty for a plain list of tabs
    /// — and for a host that keeps no groups, whose pictures are all like that.
    pub groups: Groups<u64>,
    /// Every tab of the strip as it stands: name, split tree with its ratios, focused pane.
    pub tabs: Vec<Shape>,
    /// The window holds a question of its own (the close question, the application's report),
    /// which blocks all of it: nothing moves into or out of it. A tab's own question does not
    /// count; it leaves the screen with its tab.
    pub asking: bool,
    /// The window shows one cluster of its strip at a time — the group on screen, or the
    /// ungrouped tabs (bateri's tab bar in the title row) — and keeps its place when tabs are
    /// regrouped away from it: the next tab of the cluster comes forward instead of the screen
    /// following the tabs ([`Move::RegroupTabs`]). `false` for a window that lists every tab
    /// (the side column) and for a host that keeps no groups.
    pub one_cluster: bool,
}

impl Window {
    /// Whether a move that takes `leaving` of its tabs closes this window: they are all it has.
    fn closes_without(&self, leaving: usize) -> bool {
        self.order.len() <= leaving
    }

    /// The step that closes this window once a move has left it without a tab.
    fn close_if_emptied(&self) -> Step {
        Step::CloseIfEmptied { window: self.id }
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
            groups: self.groups.clone(),
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
    /// the tab itself moves there, name and identity and all. In a window that has groups the
    /// place of the strip cannot say which side of a group's border the pane was let go on, so
    /// the one who let it go says where it goes (`target`, [`Groups::gap_target`]); without it the
    /// tab joins the group of the tab it goes before ([`Groups::insert_at`]).
    PaneToNewTab {
        pane: u64,
        window: u64,
        gap: usize,
        target: Option<Target<u64>>,
    },
    /// Tab `tab` becomes a window of its own — Move Tab to New Window, a tab dragged out of its
    /// strip and let go over nothing. `at` is where it was let go (a point on the screen, in
    /// the platform's coordinates): the new window's title row goes under it; without it the
    /// window takes its old window's size and place, cascaded. A window's only tab stays.
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
    /// selection staying where it was; the windows they leave close. Ungrouped tabs join the
    /// ungrouped ones; a group comes with its tabs under its own name, made unique where a group
    /// of that name is there already ([`Groups::unique_name`]). Nothing is undone: the windows
    /// that close are no picture.
    MergeAllWindows { into: u64 },
    /// Tabs `tabs` of window `window` go where `target` says ([`Groups::place`]): into a group
    /// (before one of its tabs, or last), into a group made for them with a name, or out of any
    /// group among the ungrouped ones — Move to Group, Move to New Group, Remove from Group,
    /// Ungroup, a row let go between two rows. What is on screen stays. A question the window
    /// holds does not hold it back: nothing is shown or hidden.
    RegroupTabs {
        window: u64,
        tabs: Vec<u64>,
        target: Target<u64>,
    },
    /// Group `group` of window `window` takes place `to` among the groups (past the end is the
    /// end), its tabs with it; the ungrouped tabs stay first.
    MoveGroup {
        window: u64,
        group: GroupId,
        to: usize,
    },
    /// Group `group` of window `window` joins window `into` as a group of its own, with its name
    /// (made unique there) and at place `to` among that window's groups; its tabs leave their
    /// window whole and the window comes forward on the tab the user was on in the group. The
    /// window they leave closes if they were all it had. In its own window it is
    /// [`Move::MoveGroup`].
    GroupToWindow {
        window: u64,
        group: GroupId,
        into: u64,
        to: usize,
    },
    /// Tabs `tabs` of window `window` join window `into` where `target` says (a group of it, or
    /// among its ungrouped tabs, before one of its tabs or last) — a row, or several picked
    /// together, let go on another window's bar or column. They leave their window whole and the
    /// window they leave closes if they were all it had; `into` comes forward on the one that was
    /// on screen, else the first. In its own window it is [`Move::RegroupTabs`].
    TabsToWindow {
        window: u64,
        tabs: Vec<u64>,
        into: u64,
        target: Target<u64>,
    },
    /// Group `group` of window `window` becomes a window of its own with the group's name, as
    /// [`Move::TabToNewWindow`] says for `at`. A window that is only that group stays.
    GroupToNewWindow {
        window: u64,
        group: GroupId,
        at: Option<(f64, f64)>,
    },
    /// Tabs `tabs` of window `window` become a window of their own, ungrouped, in the order they
    /// had; `at` as in [`Move::TabToNewWindow`]. A window that is only these tabs stays.
    TabsToNewWindow {
        window: u64,
        tabs: Vec<u64>,
        at: Option<(f64, f64)>,
    },
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
    /// Window `window`'s strip is `order` again, its selection and its `groups` with it; what was
    /// on screen leaves it first.
    PutStrip {
        window: u64,
        order: Tabs<u64>,
        groups: Groups<u64>,
    },
    /// Tabs `tabs` of window `window` go where `target` says ([`Groups::place`]); a group made
    /// for them is given its identity by the window's own count, which the plan reads from the
    /// picture. What is on screen does not change.
    Regroup {
        window: u64,
        tabs: Vec<u64>,
        target: Target<u64>,
    },
    /// Group `group` of window `window` takes place `to` among the groups, its tabs with it.
    MoveGroup {
        window: u64,
        group: GroupId,
        to: usize,
    },
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
        Move::PaneToNewTab {
            pane,
            window,
            gap,
            target,
        } => pane_to_new_tab(world, pane, window, gap, target, host),
        Move::TabToNewWindow { tab, at } => tab_to_new_window(world, tab, at, host),
        Move::PaneToNewWindow { pane, at } => pane_to_new_window(world, pane, at, host),
        Move::TabToStrip { tab, window, index } => tab_to_strip(world, tab, window, index),
        Move::MergeAllWindows { into } => merge_all_windows(world, into),
        Move::RegroupTabs {
            window,
            tabs,
            target,
        } => regroup_tabs(world, window, &tabs, target),
        Move::MoveGroup { window, group, to } => move_group(world, window, group, to),
        Move::GroupToWindow {
            window,
            group,
            into,
            to,
        } => group_to_window(world, window, group, into, to),
        Move::GroupToNewWindow { window, group, at } => {
            group_to_new_window(world, window, group, at, host)
        }
        Move::TabsToNewWindow { window, tabs, at } => {
            tabs_to_new_window(world, window, &tabs, at, host)
        }
        Move::TabsToWindow {
            window,
            tabs,
            into,
            target,
        } => tabs_to_window(world, window, &tabs, into, target),
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
    steps.push(from.close_if_emptied());
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
    target: Option<Target<u64>>,
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
            if let Some(target) = target {
                // The place was named by a group's border, not by a number of the strip.
                let mut plan = regroup_tabs(world, from.id, &[source.tab], target)?;
                plan.after.push(Step::Pulse {
                    window: from.id,
                    tab: source.tab,
                });
                return Ok(plan);
            }
            let at = from.order.index_of(source.tab).ok_or(Refusal::Quiet)?;
            let (mut order, mut groups) = (from.order.clone(), from.groups.clone());
            if !groups.move_to(&mut order, source.tab, gap_to_index(at, gap)) {
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
        let mut steps = vec![
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
        ];
        steps.extend(target.map(|target| Step::Regroup {
            window: from.id,
            tabs: vec![new],
            target,
        }));
        return Ok(Plan {
            steps,
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
    steps.extend(target.map(|target| Step::Regroup {
        window: onto.id,
        tabs: vec![tab],
        target,
    }));
    steps.push(from.close_if_emptied());
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
    // open.
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
        let (mut order, mut groups) = (from.order.clone(), from.groups.clone());
        if !groups.move_to(&mut order, tab, index) {
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
        .chain([from.close_if_emptied()])
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
        // They joined among the ungrouped; each group's tabs go on into a group of their own,
        // in the groups' order, so the window keeps the user's grouping and its sequence.
        for group in other.groups.groups() {
            steps.push(Step::Regroup {
                window: into,
                tabs: other.groups.cluster(&other.order, Some(group.id)),
                target: Target::New(group.name.clone()),
            });
        }
        steps.push(other.close_if_emptied());
    }
    Ok(Plan {
        steps,
        after: vec![Step::Raise { window: into }],
        undo: None,
    })
}

/// The window `id`.
fn window_named(world: &[Window], id: u64) -> Result<&Window, Refusal> {
    world
        .iter()
        .find(|window| window.id == id)
        .ok_or(Refusal::Quiet)
}

/// What a strip and its groups say, short of the tabs each group last remembers: the order, who is
/// in which group, how many groups there are. A move that leaves it as it was moved nothing.
fn arrangement(order: &Tabs<u64>, groups: &Groups<u64>) -> (Vec<u64>, Vec<Option<GroupId>>, usize) {
    (
        order.ids().to_vec(),
        order
            .ids()
            .iter()
            .map(|&tab| groups.group_of(tab))
            .collect(),
        groups.len(),
    )
}

/// The tabs of `from` that `tabs` names, in the order of the strip.
fn in_strip(from: &Window, tabs: &[u64]) -> Vec<u64> {
    from.order
        .ids()
        .iter()
        .copied()
        .filter(|tab| tabs.contains(tab))
        .collect()
}

/// The tab on screen once tabs `tabs` of `from` have gone to another place: the one the user was
/// on if it is among them, else the one `prefer` names (the group's own) if it is, else the first.
fn shown_among(from: &Window, tabs: &[u64], prefer: Option<u64>) -> u64 {
    from.order
        .selected()
        .filter(|tab| tabs.contains(tab))
        .or_else(|| prefer.filter(|tab| tabs.contains(tab)))
        .unwrap_or(tabs[0])
}

/// The steps that take `tabs` out of `from`, the one on screen last (it is the one that leaves the
/// screen, and the window it leaves may be closing anyway).
fn releases(from: &Window, tabs: &[u64]) -> Vec<Step> {
    let selected = from.order.selected();
    tabs.iter()
        .filter(|&&tab| Some(tab) != selected)
        .chain(tabs.iter().filter(|&&tab| Some(tab) == selected))
        .map(|&tab| Step::ReleaseTab {
            window: from.id,
            tab,
        })
        .collect()
}

fn regroup_tabs(
    world: &[Window],
    window: u64,
    tabs: &[u64],
    target: Target<u64>,
) -> Result<Plan, Refusal> {
    let from = window_named(world, window)?;
    let tabs = in_strip(from, tabs);
    let (mut order, mut groups) = (from.order.clone(), from.groups.clone());
    let into = groups
        .place(&mut order, &tabs, target.clone())
        .ok_or(Refusal::Quiet)?;
    if arrangement(&order, &groups) == arrangement(&from.order, &from.groups) {
        return Err(Refusal::Quiet);
    }
    let after = keep_cluster(from, &tabs, &target, into)
        .into_iter()
        .collect();
    Ok(Plan {
        steps: vec![Step::Regroup {
            window,
            tabs,
            target,
        }],
        after,
        undo: Some(Record {
            scenes: vec![from.scene(&[])],
            born: Vec::new(),
        }),
    })
}

/// Where a window that shows one cluster at a time ([`Window::one_cluster`]) stays when `moved`
/// are regrouped out of the cluster on screen into `into` (what [`Groups::place`] answered): the
/// tab the cluster comes forward on, if the cluster keeps one — the window does not follow the
/// tabs, it stays where it is on the next tab of its group ([`Groups::fronting`]). It follows
/// them when the cluster is left empty (**a group does not stay empty**: the window goes with
/// the tabs, and the ungrouped tabs, which have no header to be left empty, are no different)
/// and when they go into a group made for them, which is where the user is going.
fn keep_cluster(
    from: &Window,
    moved: &[u64],
    target: &Target<u64>,
    into: Option<GroupId>,
) -> Option<Step> {
    if !from.one_cluster || matches!(target, Target::New(_)) {
        return None;
    }
    let shown = from
        .order
        .selected()
        .filter(|shown| moved.contains(shown))?;
    let here = from.groups.group_of(shown);
    if into == here {
        return None;
    }
    let next = from.groups.fronting(&from.order, moved)?;
    (from.groups.group_of(next) == here).then_some(Step::Select {
        window: from.id,
        tab: next,
    })
}

fn move_group(world: &[Window], window: u64, group: GroupId, to: usize) -> Result<Plan, Refusal> {
    let from = window_named(world, window)?;
    let (mut order, mut groups) = (from.order.clone(), from.groups.clone());
    if !groups.move_group(&mut order, group, to) {
        return Err(Refusal::Quiet);
    }
    Ok(Plan {
        steps: vec![Step::MoveGroup { window, group, to }],
        after: Vec::new(),
        undo: Some(Record {
            scenes: vec![from.scene(&[])],
            born: Vec::new(),
        }),
    })
}

fn group_to_window(
    world: &[Window],
    window: u64,
    group: GroupId,
    into: u64,
    to: usize,
) -> Result<Plan, Refusal> {
    let from = window_named(world, window)?;
    let onto = window_named(world, into)?;
    if from.id == onto.id {
        return move_group(world, window, group, to);
    }
    let name = from.groups.group(group).ok_or(Refusal::Quiet)?.name.clone();
    let tabs = from.groups.cluster(&from.order, Some(group));
    if from.asking || onto.asking {
        return Err(Refusal::Beep);
    }
    // What the window it joins makes of the tabs, played on a copy: the group's identity, which the
    // step that places it and the one that moves it name, is the window's own count.
    let (mut order, mut groups) = (onto.order.clone(), onto.groups.clone());
    for &tab in &tabs {
        groups.append(&mut order, tab);
    }
    let made = groups
        .place(&mut order, &tabs, Target::New(name.clone()))
        .flatten()
        .ok_or(Refusal::Quiet)?;
    let at = groups.position(made);
    let mut steps = releases(from, &tabs);
    steps.extend(tabs.iter().map(|&tab| Step::AdoptTab {
        window: onto.id,
        tab,
        slot: Slot::End,
    }));
    steps.push(Step::Regroup {
        window: onto.id,
        tabs: tabs.clone(),
        target: Target::New(name),
    });
    if at.is_some_and(|at| at != to.min(groups.len() - 1)) {
        steps.push(Step::MoveGroup {
            window: onto.id,
            group: made,
            to,
        });
    }
    steps.push(from.close_if_emptied());
    // The window it leaves closes if the group was all it had, and then there is nothing to undo.
    let closes = from.closes_without(tabs.len());
    let resident = from.groups.resident(Some(group));
    Ok(Plan {
        steps,
        after: vec![
            Step::Raise { window: onto.id },
            Step::Select {
                window: onto.id,
                tab: shown_among(from, &tabs, resident),
            },
        ],
        undo: (!closes).then(|| Record {
            scenes: vec![from.scene(&tabs), onto.scene(&[])],
            born: Vec::new(),
        }),
    })
}

fn tabs_to_window(
    world: &[Window],
    window: u64,
    tabs: &[u64],
    into: u64,
    target: Target<u64>,
) -> Result<Plan, Refusal> {
    let from = window_named(world, window)?;
    let onto = window_named(world, into)?;
    let tabs = in_strip(from, tabs);
    if tabs.is_empty() {
        return Err(Refusal::Quiet);
    }
    if from.id == onto.id {
        return regroup_tabs(world, window, &tabs, target);
    }
    if from.asking || onto.asking {
        return Err(Refusal::Beep);
    }
    // The target is the window's own group and tab: played on a copy, a group that is gone or a
    // place that is not there ends quietly before anything has left its window.
    let (mut order, mut groups) = (onto.order.clone(), onto.groups.clone());
    for &tab in &tabs {
        groups.append(&mut order, tab);
    }
    groups
        .place(&mut order, &tabs, target.clone())
        .ok_or(Refusal::Quiet)?;
    let mut steps = releases(from, &tabs);
    steps.extend(tabs.iter().map(|&tab| Step::AdoptTab {
        window: onto.id,
        tab,
        slot: Slot::End,
    }));
    steps.push(Step::Regroup {
        window: onto.id,
        tabs: tabs.clone(),
        target,
    });
    steps.push(from.close_if_emptied());
    // The window they leave closes if they were all it had, and then there is nothing to undo.
    let closes = from.closes_without(tabs.len());
    Ok(Plan {
        steps,
        after: vec![
            Step::Raise { window: onto.id },
            Step::Select {
                window: onto.id,
                tab: shown_among(from, &tabs, None),
            },
        ],
        undo: (!closes).then(|| Record {
            scenes: vec![from.scene(&tabs), onto.scene(&[])],
            born: Vec::new(),
        }),
    })
}

fn group_to_new_window(
    world: &[Window],
    window: u64,
    group: GroupId,
    at: Option<(f64, f64)>,
    host: &dyn Host,
) -> Result<Plan, Refusal> {
    let from = window_named(world, window)?;
    let name = from.groups.group(group).ok_or(Refusal::Quiet)?.name.clone();
    let tabs = from.groups.cluster(&from.order, Some(group));
    let resident = from.groups.resident(Some(group));
    new_window_of(from, tabs, Some(name), resident, at, host)
}

fn tabs_to_new_window(
    world: &[Window],
    window: u64,
    tabs: &[u64],
    at: Option<(f64, f64)>,
    host: &dyn Host,
) -> Result<Plan, Refusal> {
    let from = window_named(world, window)?;
    new_window_of(from, in_strip(from, tabs), None, None, at, host)
}

/// A window made of `tabs` of `from`, grouped under `group` if a name is given, ungrouped if not.
/// Like a lone tab's, it takes the old window's size and place (or goes under `at`); a window that
/// is only these tabs stays where it is. Undo Move takes the tabs back and closes the window.
fn new_window_of(
    from: &Window,
    tabs: Vec<u64>,
    group: Option<String>,
    resident: Option<u64>,
    at: Option<(f64, f64)>,
    host: &dyn Host,
) -> Result<Plan, Refusal> {
    if tabs.is_empty() {
        return Err(Refusal::Quiet);
    }
    if from.closes_without(tabs.len()) || from.asking {
        return Err(Refusal::Beep);
    }
    let window = host.fresh_id();
    let shown = shown_among(from, &tabs, resident);
    let mut steps = releases(from, &tabs);
    steps.push(Step::OpenWindow {
        window,
        from: from.id,
        tab: tabs[0],
        at,
    });
    steps.extend(tabs[1..].iter().map(|&tab| Step::AdoptTab {
        window,
        tab,
        slot: Slot::End,
    }));
    if let Some(name) = group {
        steps.push(Step::Regroup {
            window,
            tabs: tabs.clone(),
            target: Target::New(name),
        });
    }
    Ok(Plan {
        steps,
        after: (shown != tabs[0])
            .then_some(Step::Select { window, tab: shown })
            .into_iter()
            .collect(),
        undo: Some(Record {
            scenes: vec![from.scene(&tabs)],
            born: vec![window],
        }),
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
            groups: scene.groups.clone(),
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
            groups: Groups::new(),
            tabs,
            asking: false,
            one_cluster: false,
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
                target: None,
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
                target: None,
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
            target: None,
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
                        gap,
                        target: None,
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
                target: None,
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
            target: None,
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

    #[test]
    fn merging_all_windows_brings_the_groups_with_their_names() {
        let mut world = grouped();
        let mut three = window(
            3,
            vec![
                shape(30, Tree::Leaf(300), 300),
                shape(31, Tree::Leaf(310), 310),
            ],
            30,
        );
        three
            .groups
            .place(&mut three.order, &[31], Target::New("api".to_owned()));
        world.push(three);
        let plan = plan(&world, Move::MergeAllWindows { into: 2 }, &Cramped).unwrap();
        let end = |tab| Step::AdoptTab {
            window: 2,
            tab,
            slot: Slot::End,
        };
        let regroup = |tabs: Vec<u64>, name: &str| Step::Regroup {
            window: 2,
            tabs,
            target: Target::New(name.to_owned()),
        };
        assert_eq!(
            plan.steps,
            vec![
                Step::ReleaseTab { window: 1, tab: 11 },
                Step::ReleaseTab { window: 1, tab: 12 },
                Step::ReleaseTab { window: 1, tab: 13 },
                Step::ReleaseTab { window: 1, tab: 10 },
                end(10),
                end(11),
                end(12),
                end(13),
                regroup(vec![11, 12], "api"),
                regroup(vec![13], "web"),
                Step::CloseIfEmptied { window: 1 },
                Step::ReleaseTab { window: 3, tab: 31 },
                Step::ReleaseTab { window: 3, tab: 30 },
                end(30),
                end(31),
                // The same name: the window gives it its own number when the step is played.
                regroup(vec![31], "api"),
                Step::CloseIfEmptied { window: 3 },
            ],
            "the ungrouped tabs adopt as they did; each group follows them under its name, in \
             the groups' order"
        );
        assert_eq!(plan.undo, None, "the windows that close are no picture");

        // Played on a copy of window 2: the second `api` is made unique where it lands.
        let mut two = world[1].clone();
        for tab in [10, 11, 12, 13, 30, 31] {
            two.groups.append(&mut two.order, tab);
        }
        for step in &plan.steps {
            if let Step::Regroup { tabs, target, .. } = step {
                two.groups.place(&mut two.order, tabs, target.clone());
            }
        }
        let names: Vec<&str> = two
            .groups
            .groups()
            .iter()
            .map(|group| group.name.as_str())
            .collect();
        assert_eq!(names, ["api", "web", "api 2"]);
        assert_eq!(two.groups.check(&two.order), Ok(()));
        assert_eq!(
            two.groups.cluster(&two.order, None).len(),
            world[1].order.len() + 2,
            "the ungrouped tabs of the windows joined the ungrouped"
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
                    groups: window.groups.clone(),
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
                target: None,
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
                order: before[0].order.clone(),
                groups: Groups::new()
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
                target: None,
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

    // ─── Groups ──────────────────────────────────────────────────────────

    /// [`world`] with window 1 grouped. Its strip reads `10 | 11 12 | 13` — tab 10 ungrouped,
    /// `api` holding 11 and 12, `web` holding 13 — with 10 on screen; tabs 12 and 13 are one pane
    /// each (120, 130). Window 2 is as it was.
    fn grouped() -> Vec<Window> {
        let mut world = world();
        let one = &mut world[0];
        for (tab, pane) in [(12, 120), (13, 130)] {
            one.tabs.push(shape(tab, Tree::Leaf(pane), pane));
            one.order.append(tab);
        }
        one.groups
            .place(&mut one.order, &[11, 12], Target::New("api".to_owned()));
        one.groups
            .place(&mut one.order, &[13], Target::New("web".to_owned()));
        world
    }

    fn gid(window: &Window, name: &str) -> GroupId {
        window
            .groups
            .groups()
            .iter()
            .find(|group| group.name == name)
            .expect("the group is there")
            .id
    }

    /// A group identity no window of [`grouped`] has.
    fn stray() -> GroupId {
        let mut tabs = Tabs::new(1u64);
        tabs.append(2);
        tabs.append(3);
        let mut groups = Groups::new();
        for tab in 1..=3 {
            groups.place(&mut tabs, &[tab], Target::New(format!("g{tab}")));
        }
        groups.groups()[2].id
    }

    /// Window 3: its one tab, 30, is all of group `solo`.
    fn solo() -> Window {
        let mut window = window(3, vec![shape(30, Tree::Leaf(300), 300)], 30);
        window
            .groups
            .place(&mut window.order, &[30], Target::New("solo".to_owned()));
        window
    }

    #[test]
    fn tabs_regroup_in_their_window_and_the_picture_comes_back_with_undo() {
        let before = grouped();
        let api = gid(&before[0], "api");
        let record = |wanted| {
            plan(&before, wanted, &Cramped)
                .map(|plan| (plan.steps, plan.after, plan.undo))
                .unwrap()
        };
        // Into a group, before one of its tabs.
        let (steps, after, undo) = record(Move::RegroupTabs {
            window: 1,
            tabs: vec![10],
            target: Target::Group {
                group: api,
                before: Some(12),
            },
        });
        assert_eq!(
            steps,
            [Step::Regroup {
                window: 1,
                tabs: vec![10],
                target: Target::Group {
                    group: api,
                    before: Some(12)
                }
            }]
        );
        assert_eq!(after, []);
        let undo = undo.expect("a regrouping can be taken back");
        assert_eq!(undo.scenes, [before[0].scene(&[])]);
        assert_eq!(undo.scenes[0].groups, before[0].groups);

        // Played on the picture, then taken back: the strip and the groups as they were, named.
        let mut now = before.clone();
        let one = &mut now[0];
        one.groups.place(
            &mut one.order,
            &[10],
            Target::Group {
                group: api,
                before: Some(12),
            },
        );
        assert_eq!(
            now[0].groups.cluster(&now[0].order, Some(api)),
            [11, 10, 12]
        );
        assert!(standing(&now, &undo));
        let taken_back = undoing(&now, undo).unwrap();
        assert!(taken_back.steps.contains(&Step::PutStrip {
            window: 1,
            order: before[0].order.clone(),
            groups: before[0].groups.clone(),
        }));
        assert_eq!(
            taken_back.steps.last(),
            Some(&Step::Undone { window: 1 }),
            "and the window says so"
        );
        assert_eq!(taken_back.undo, None, "there is no redo");
    }

    #[test]
    fn the_name_a_new_group_is_born_asking_for_is_part_of_its_move() {
        let before = grouped();
        let record = |wanted| plan(&before, wanted, &Cramped).unwrap().undo.unwrap();
        let regroup = |name: &str| Move::RegroupTabs {
            window: 1,
            tabs: vec![10],
            target: Target::New(name.to_owned()),
        };
        // Tab 10 is made a group of its own, named for its folder; the name field it opens is
        // answered "ten": the picture the move left has the group by that name now.
        let mut undo = record(regroup("home"));
        let mut now = before.clone();
        let one = &mut now[0];
        one.groups
            .place(&mut one.order, &[10], Target::New("home".to_owned()));
        let made = one.groups.group_of(10).expect("tab 10 is in a group");
        assert!(one.groups.rename(made, "ten"));
        undo.named(1, made, "ten");
        // Undo Move still takes the move back: the group goes, and no name comes back for it.
        assert!(standing(&now, &undo));
        let taken_back = undoing(&now, undo).expect("the move can still be taken back");
        assert!(taken_back.steps.contains(&Step::PutStrip {
            window: 1,
            order: before[0].order.clone(),
            groups: before[0].groups.clone(),
        }));

        // A move that follows records the picture with the group in it; the name typed after it
        // is what Undo Move puts back, not the name the picture was taken with.
        let mut later = plan(
            &now,
            Move::RegroupTabs {
                window: 1,
                tabs: vec![13],
                target: Target::Group {
                    group: made,
                    before: None,
                },
            },
            &Cramped,
        )
        .unwrap()
        .undo
        .unwrap();
        let called = |record: &Record| {
            record.scenes[0]
                .groups
                .group(made)
                .map(|group| group.name.clone())
        };
        assert_eq!(called(&later), Some("ten".to_owned()));
        later.named(1, made, "tenth");
        assert_eq!(
            called(&later),
            Some("tenth".to_owned()),
            "the picture says what the group is called now"
        );
    }

    /// What a regrouping of `tabs` of window 1 into `target` asks to come forward, in a window
    /// that shows one cluster at a time and has tab `shown` on screen.
    fn kept(shown: u64, tabs: &[u64], target: Target<u64>, one_cluster: bool) -> Vec<Step> {
        let mut world = grouped();
        world[0].one_cluster = one_cluster;
        let one = &mut world[0];
        one.groups.select(&mut one.order, shown);
        plan(
            &world,
            Move::RegroupTabs {
                window: 1,
                tabs: tabs.to_vec(),
                target,
            },
            &Cramped,
        )
        .unwrap()
        .after
    }

    #[test]
    fn a_bar_that_shows_one_group_stays_where_it_is_when_its_tab_goes_to_another() {
        let world = grouped();
        let (api, web) = (gid(&world[0], "api"), gid(&world[0], "web"));
        let to_web = || Target::Group {
            group: web,
            before: None,
        };
        let next = |tab| vec![Step::Select { window: 1, tab }];
        // api = 11 12, web = 13, ungrouped = 10. Tab 11 on screen goes to web: api keeps 12, the
        // window stays in api on it, and web's new tab is not on screen.
        assert_eq!(kept(11, &[11], to_web(), true), next(12));
        // A window that lists every tab does not move, and follows the tab as it always did.
        assert_eq!(kept(11, &[11], to_web(), false), []);
        // Out of the group, among the ungrouped tabs: the same.
        assert_eq!(
            kept(11, &[11], Target::Loose { before: None }, true),
            next(12)
        );
        // The one that stays is the neighbour the closing rule names: the next, else the one before.
        assert_eq!(kept(12, &[12], to_web(), true), next(11));
        // A tab that is not on screen moves and the screen is where it was.
        assert_eq!(kept(12, &[11], to_web(), true), []);
        // Into the group it is in: only the order changes, and the tab stays on screen.
        assert_eq!(
            kept(
                11,
                &[11],
                Target::Group {
                    group: api,
                    before: None
                },
                true
            ),
            []
        );
        // Into a group made for it, the window goes with it.
        assert_eq!(kept(11, &[11], Target::New("db".to_owned()), true), []);
        // The group is left empty (a group does not stay empty): the window follows its tabs.
        assert_eq!(kept(11, &[11, 12], to_web(), true), []);
        assert_eq!(kept(13, &[13], Target::Loose { before: None }, true), []);
        assert_eq!(
            kept(10, &[10], to_web(), true),
            [],
            "the ungrouped tab was alone"
        );
    }

    #[test]
    fn the_bar_that_stays_is_undone_with_the_tab_back_on_screen() {
        let mut world = grouped();
        world[0].one_cluster = true;
        let web = gid(&world[0], "web");
        let one = &mut world[0];
        one.groups.select(&mut one.order, 11);
        let moved = plan(
            &world,
            Move::RegroupTabs {
                window: 1,
                tabs: vec![11],
                target: Target::Group {
                    group: web,
                    before: None,
                },
            },
            &Cramped,
        )
        .unwrap();
        let record = moved.undo.expect("a regrouping can be taken back");
        assert_eq!(
            record.scenes[0].order.selected(),
            Some(11),
            "the picture it keeps has the tab on screen, in its group"
        );
        assert_eq!(
            record.scenes[0].groups.group_of(11),
            world[0].groups.group_of(11)
        );
    }

    #[test]
    fn tabs_move_to_a_new_group_with_the_name_asked_and_out_of_a_group() {
        let before = grouped();
        let made = plan(
            &before,
            Move::RegroupTabs {
                window: 1,
                tabs: vec![13, 10],
                target: Target::New("db".to_owned()),
            },
            &Cramped,
        )
        .unwrap();
        // The tabs go in the strip's order, whatever order they were named in.
        assert_eq!(
            made.steps,
            [Step::Regroup {
                window: 1,
                tabs: vec![10, 13],
                target: Target::New("db".to_owned())
            }]
        );
        // `web` was only tab 13: the group it leaves empty is gone, the strip is as the planner
        // played it.
        let mut now = before.clone();
        let one = &mut now[0];
        one.groups
            .place(&mut one.order, &[10, 13], Target::New("db".to_owned()));
        assert_eq!(one.groups.len(), 2);
        assert!(standing(&now, made.undo.as_ref().unwrap()));
        let back = undoing(&now, made.undo.unwrap()).unwrap();
        assert!(back.steps.contains(&Step::PutStrip {
            window: 1,
            order: before[0].order.clone(),
            groups: before[0].groups.clone(),
        }));

        // Remove from Group: ungrouped, after the last ungrouped tab.
        let out = plan(
            &before,
            Move::RegroupTabs {
                window: 1,
                tabs: vec![12],
                target: Target::Loose { before: None },
            },
            &Cramped,
        )
        .unwrap();
        assert_eq!(out.steps.len(), 1);
    }

    #[test]
    fn a_regrouping_that_changes_nothing_or_names_nothing_ends_quietly() {
        let world = grouped();
        let api = gid(&world[0], "api");
        let regroup = |window, tabs: Vec<u64>, target| {
            plan(
                &world,
                Move::RegroupTabs {
                    window,
                    tabs,
                    target,
                },
                &Cramped,
            )
        };
        for (tabs, target) in [
            // Already there, last in `api`.
            (
                vec![12],
                Target::Group {
                    group: api,
                    before: None,
                },
            ),
            // Already ungrouped, and last of the ungrouped.
            (vec![10], Target::Loose { before: None }),
            // A group that is not there; tabs that are not there; no tab.
            (
                vec![10],
                Target::Group {
                    group: stray(),
                    before: None,
                },
            ),
            (vec![99], Target::New("x".to_owned())),
            (vec![], Target::New("x".to_owned())),
        ] {
            assert_eq!(
                regroup(1, tabs.clone(), target.clone()),
                Err(Refusal::Quiet),
                "{tabs:?} {target:?}"
            );
        }
        assert_eq!(
            regroup(9, vec![10], Target::New("x".to_owned())),
            Err(Refusal::Quiet),
            "a window that is not there"
        );
        // A tab named to go before one that is not in the group goes last in it.
        assert!(
            regroup(
                1,
                vec![10],
                Target::Group {
                    group: api,
                    before: Some(99)
                }
            )
            .is_ok()
        );
        // A question the window holds does not hold it back: nothing is shown or hidden.
        let mut asking = grouped();
        asking[0].asking = true;
        assert!(
            plan(
                &asking,
                Move::RegroupTabs {
                    window: 1,
                    tabs: vec![10],
                    target: Target::New("x".to_owned()),
                },
                &Cramped
            )
            .is_ok()
        );
    }

    #[test]
    fn a_group_moves_among_the_groups_and_undo_puts_them_back() {
        let before = grouped();
        let web = gid(&before[0], "web");
        let moved = plan(
            &before,
            Move::MoveGroup {
                window: 1,
                group: web,
                to: 0,
            },
            &Cramped,
        )
        .unwrap();
        assert_eq!(
            moved.steps,
            [Step::MoveGroup {
                window: 1,
                group: web,
                to: 0
            }]
        );
        let mut now = before.clone();
        let one = &mut now[0];
        one.groups.move_group(&mut one.order, web, 0);
        assert_eq!(now[0].order.ids(), [10, 13, 11, 12]);
        assert!(standing(&now, moved.undo.as_ref().unwrap()));
        let back = undoing(&now, moved.undo.unwrap()).unwrap();
        assert!(back.steps.contains(&Step::PutStrip {
            window: 1,
            order: before[0].order.clone(),
            groups: before[0].groups.clone(),
        }));
        // Where it already is, or a group that is not there: nothing.
        for (group, to) in [(web, 1), (web, 9), (stray(), 0)] {
            assert_eq!(
                plan(
                    &before,
                    Move::MoveGroup {
                        window: 1,
                        group,
                        to
                    },
                    &Cramped
                ),
                Err(Refusal::Quiet),
                "{to}"
            );
        }
    }

    #[test]
    fn a_group_crossing_windows_comes_up_there_under_its_name_and_undo_takes_it_back() {
        let before = grouped();
        let api = gid(&before[0], "api");
        let moved = plan(
            &before,
            Move::GroupToWindow {
                window: 1,
                group: api,
                into: 2,
                to: 0,
            },
            &Cramped,
        )
        .unwrap();
        let adopt = |tab| Step::AdoptTab {
            window: 2,
            tab,
            slot: Slot::End,
        };
        assert_eq!(
            moved.steps,
            [
                Step::ReleaseTab { window: 1, tab: 11 },
                Step::ReleaseTab { window: 1, tab: 12 },
                adopt(11),
                adopt(12),
                Step::Regroup {
                    window: 2,
                    tabs: vec![11, 12],
                    target: Target::New("api".to_owned())
                },
                // The window closes only if the steps before left it without a tab.
                Step::CloseIfEmptied { window: 1 },
            ],
            "it is the window's only group, so it is at place 0 already"
        );
        assert_eq!(
            moved.after,
            [
                Step::Raise { window: 2 },
                Step::Select { window: 2, tab: 11 }
            ],
            "the window comes forward on the group's first tab"
        );
        let undo = moved.undo.expect("window 1 stays");
        assert_eq!(undo.scenes.len(), 2);
        assert_eq!(undo.scenes[0].shapes.len(), 2, "the tabs that crossed");

        // Played: window 1 without them, window 2 with them in a group of its own.
        let mut now = before.clone();
        {
            let one = &mut now[0];
            one.groups.close(&mut one.order, &[11, 12]);
            one.tabs.retain(|shape| shape.tab != 11 && shape.tab != 12);
            let two = &mut now[1];
            for tab in [11, 12] {
                two.groups.append(&mut two.order, tab);
            }
            two.groups
                .place(&mut two.order, &[11, 12], Target::New("api".to_owned()));
            two.tabs.push(shape(11, Tree::Leaf(110), 110));
            two.tabs.push(shape(12, Tree::Leaf(120), 120));
        }
        assert!(standing(&now, &undo));
        let back = undoing(&now, undo).unwrap();
        for tab in [11, 12] {
            assert!(back.steps.contains(&Step::Dissolve { window: 2, tab }));
        }
        assert!(back.steps.contains(&Step::PutStrip {
            window: 1,
            order: before[0].order.clone(),
            groups: before[0].groups.clone(),
        }));
        assert!(back.steps.contains(&Step::PutStrip {
            window: 2,
            order: before[1].order.clone(),
            groups: Groups::new(),
        }));
    }

    #[test]
    fn a_group_let_go_among_another_windows_groups_stands_where_it_was_let_go() {
        let mut world = grouped();
        let two = &mut world[1];
        two.groups
            .place(&mut two.order, &[21], Target::New("x".to_owned()));
        let api = gid(&world[0], "api");
        let steps = plan(
            &world,
            Move::GroupToWindow {
                window: 1,
                group: api,
                into: 2,
                to: 0,
            },
            &Cramped,
        )
        .unwrap()
        .steps;
        // Made after `x`, it is then moved before it: the group it names is the window's next.
        let moved = steps
            .iter()
            .find(|step| matches!(step, Step::MoveGroup { .. }))
            .expect("a step that moves it");
        assert!(
            matches!(moved, Step::MoveGroup { window: 2, to: 0, group } if *group != gid(&world[1], "x")),
            "{moved:?}"
        );
        // Past the end is the end: no move at all.
        let steps = plan(
            &world,
            Move::GroupToWindow {
                window: 1,
                group: api,
                into: 2,
                to: 9,
            },
            &Cramped,
        )
        .unwrap()
        .steps;
        assert!(
            !steps
                .iter()
                .any(|step| matches!(step, Step::MoveGroup { .. }))
        );
        // In its own window it is a move among its groups.
        assert_eq!(
            plan(
                &world,
                Move::GroupToWindow {
                    window: 1,
                    group: api,
                    into: 1,
                    to: 1
                },
                &Cramped
            ),
            plan(
                &world,
                Move::MoveGroup {
                    window: 1,
                    group: api,
                    to: 1
                },
                &Cramped
            )
        );
    }

    #[test]
    fn a_group_that_is_all_a_window_has_closes_it_when_it_crosses_and_cannot_be_undone() {
        let mut world = grouped();
        world.push(solo());
        let moved = plan(
            &world,
            Move::GroupToWindow {
                window: 3,
                group: gid(&world[2], "solo"),
                into: 1,
                to: 9,
            },
            &Cramped,
        )
        .unwrap();
        assert_eq!(
            moved.steps.last(),
            Some(&Step::CloseIfEmptied { window: 3 })
        );
        assert_eq!(moved.undo, None);
    }

    #[test]
    fn a_group_becomes_a_window_that_keeps_its_name_and_undo_closes_it() {
        let before = grouped();
        let api = gid(&before[0], "api");
        let moved = plan(
            &before,
            Move::GroupToNewWindow {
                window: 1,
                group: api,
                at: Some((10.0, 20.0)),
            },
            &roomy(&before),
        )
        .unwrap();
        assert_eq!(
            moved.steps,
            [
                Step::ReleaseTab { window: 1, tab: 11 },
                Step::ReleaseTab { window: 1, tab: 12 },
                Step::OpenWindow {
                    window: NEW,
                    from: 1,
                    tab: 11,
                    at: Some((10.0, 20.0))
                },
                Step::AdoptTab {
                    window: NEW,
                    tab: 12,
                    slot: Slot::End
                },
                Step::Regroup {
                    window: NEW,
                    tabs: vec![11, 12],
                    target: Target::New("api".to_owned())
                },
            ]
        );
        assert_eq!(moved.after, []);
        let undo = moved.undo.expect("the window it made is closed by undo");
        assert_eq!(undo.born, [NEW]);

        // Played: window 1 without the group, the window it made holding it.
        let mut now = before.clone();
        {
            let one = &mut now[0];
            one.groups.close(&mut one.order, &[11, 12]);
            one.tabs.retain(|shape| shape.tab != 11 && shape.tab != 12);
        }
        let mut born = window(
            NEW,
            vec![
                shape(11, Tree::Leaf(110), 110),
                shape(12, Tree::Leaf(120), 120),
            ],
            11,
        );
        born.groups
            .place(&mut born.order, &[11, 12], Target::New("api".to_owned()));
        now.push(born);
        assert!(standing(&now, &undo));
        let back = undoing(&now, undo).unwrap();
        for tab in [11, 12] {
            assert!(back.steps.contains(&Step::Unpack { window: NEW, tab }));
        }
        assert!(back.steps.contains(&Step::CloseIfEmptied { window: NEW }));
        assert!(back.steps.contains(&Step::PutStrip {
            window: 1,
            order: before[0].order.clone(),
            groups: before[0].groups.clone(),
        }));
        assert_eq!(back.after, [Step::Raise { window: 1 }]);
    }

    #[test]
    fn the_tab_on_screen_in_the_group_is_the_one_the_new_window_shows() {
        let mut world = grouped();
        world[0].order.select(12);
        let moved = plan(
            &world,
            Move::GroupToNewWindow {
                window: 1,
                group: gid(&world[0], "api"),
                at: None,
            },
            &roomy(&world),
        )
        .unwrap();
        // It leaves last, and comes up on screen in the window that takes it.
        assert_eq!(
            &moved.steps[..2],
            [
                Step::ReleaseTab { window: 1, tab: 11 },
                Step::ReleaseTab { window: 1, tab: 12 }
            ]
        );
        assert_eq!(
            moved.after,
            [Step::Select {
                window: NEW,
                tab: 12
            }]
        );
    }

    #[test]
    fn tabs_become_an_ungrouped_window_in_the_order_they_had() {
        let before = grouped();
        let moved = plan(
            &before,
            Move::TabsToNewWindow {
                window: 1,
                tabs: vec![13, 11],
                at: None,
            },
            &roomy(&before),
        )
        .unwrap();
        assert_eq!(
            moved.steps,
            [
                Step::ReleaseTab { window: 1, tab: 11 },
                Step::ReleaseTab { window: 1, tab: 13 },
                Step::OpenWindow {
                    window: NEW,
                    from: 1,
                    tab: 11,
                    at: None
                },
                Step::AdoptTab {
                    window: NEW,
                    tab: 13,
                    slot: Slot::End
                },
            ],
            "no group in the window they make"
        );
        assert_eq!(moved.undo.unwrap().born, [NEW]);
        // All of a window's tabs: it stays. Nothing named: quietly nothing. A window asking: a beep.
        let all = [10, 11, 12, 13].to_vec();
        for (tabs, answer) in [
            (all, Refusal::Beep),
            (vec![99], Refusal::Quiet),
            (vec![], Refusal::Quiet),
        ] {
            assert_eq!(
                plan(
                    &before,
                    Move::TabsToNewWindow {
                        window: 1,
                        tabs,
                        at: None
                    },
                    &roomy(&before)
                )
                .map(|plan| plan.steps),
                Err(answer)
            );
        }
        let mut asking = grouped();
        asking[0].asking = true;
        assert_eq!(
            plan(
                &asking,
                Move::TabsToNewWindow {
                    window: 1,
                    tabs: vec![10],
                    at: None
                },
                &roomy(&asking)
            )
            .map(|plan| plan.steps),
            Err(Refusal::Beep)
        );
    }

    /// A record of a regrouping or of a group that crossed is a picture like any: a tab that
    /// closed since, or a pane that was born in one the move carried, makes it false.
    #[test]
    fn a_group_record_that_the_windows_have_outgrown_is_stale() {
        let before = grouped();
        let api = gid(&before[0], "api");
        let regrouped = plan(
            &before,
            Move::RegroupTabs {
                window: 1,
                tabs: vec![10],
                target: Target::Group {
                    group: api,
                    before: None,
                },
            },
            &Cramped,
        )
        .unwrap()
        .undo
        .unwrap();
        let mut now = before.clone();
        assert!(standing(&now, &regrouped));
        // Tab 13 closed since.
        let one = &mut now[0];
        one.groups.close(&mut one.order, &[13]);
        one.tabs.retain(|shape| shape.tab != 13);
        assert!(!standing(&now, &regrouped));
        assert_eq!(undoing(&now, regrouped), Err(Refusal::Stale));

        let crossed = plan(
            &before,
            Move::GroupToWindow {
                window: 1,
                group: api,
                into: 2,
                to: 0,
            },
            &Cramped,
        )
        .unwrap()
        .undo
        .unwrap();
        // The tabs are in window 2 as the move left them, but one of them has split since.
        let mut now = before.clone();
        {
            let one = &mut now[0];
            one.groups.close(&mut one.order, &[11, 12]);
            one.tabs.retain(|shape| shape.tab != 11 && shape.tab != 12);
            let two = &mut now[1];
            for tab in [11, 12] {
                two.groups.append(&mut two.order, tab);
            }
            two.tabs.push(shape(11, Tree::Leaf(110), 110));
            two.tabs
                .push(shape(12, split(Tree::Leaf(120), Tree::Leaf(121)), 120));
        }
        assert!(!standing(&now, &crossed));
        assert_eq!(undoing(&now, crossed), Err(Refusal::Stale));
    }

    #[test]
    fn the_moves_that_were_there_before_groups_keep_the_groups_in_their_record() {
        let before = grouped();
        // A tab folded into another of its window; its group goes with its last tab.
        let folded = plan(
            &before,
            Move::TabToTab {
                tab: 13,
                into: 12,
                place: beside(),
            },
            &roomy(&before),
        )
        .unwrap();
        assert_eq!(
            folded.undo.as_ref().unwrap().scenes[0].groups,
            before[0].groups
        );
        // A tab dragged along the strip lands in the cluster of the place it was let go at.
        let strip = |index| {
            plan(
                &before,
                Move::TabToStrip {
                    tab: 10,
                    window: 1,
                    index,
                },
                &Cramped,
            )
        };
        assert_eq!(
            strip(1).unwrap().steps,
            [Step::MoveTab {
                window: 1,
                tab: 10,
                index: 1
            }]
        );
        // `11` let go before `12` is where it is.
        assert_eq!(
            plan(
                &before,
                Move::TabToStrip {
                    tab: 11,
                    window: 1,
                    index: 1
                },
                &Cramped
            ),
            Err(Refusal::Quiet)
        );
        // A tab that crosses to a window with groups takes its place by the same rule.
        let mut world = grouped();
        world.swap(0, 1);
        let crossed = plan(
            &world,
            Move::TabToStrip {
                tab: 20,
                window: 1,
                index: 2,
            },
            &Cramped,
        )
        .unwrap();
        assert!(crossed.steps.contains(&Step::AdoptTab {
            window: 1,
            tab: 20,
            slot: Slot::At(2)
        }));
    }

    /// [`grouped`] with window 2's tab 21 in a group of its own, `x`.
    fn two_grouped() -> Vec<Window> {
        let mut world = grouped();
        let two = &mut world[1];
        two.groups
            .place(&mut two.order, &[21], Target::New("x".to_owned()));
        world
    }

    #[test]
    fn tabs_let_go_on_another_windows_group_join_it_where_they_were_let_go() {
        let world = two_grouped();
        let x = gid(&world[1], "x");
        let target = Target::Group {
            group: x,
            before: Some(21),
        };
        let moved = plan(
            &world,
            Move::TabsToWindow {
                window: 1,
                tabs: vec![12, 11],
                into: 2,
                target: target.clone(),
            },
            &Cramped,
        )
        .unwrap();
        // They leave in the order of the strip, join at the end and are then put where the target
        // says; the window they leave closes only if they were all it had.
        assert_eq!(
            moved.steps,
            [
                Step::ReleaseTab { window: 1, tab: 11 },
                Step::ReleaseTab { window: 1, tab: 12 },
                Step::AdoptTab {
                    window: 2,
                    tab: 11,
                    slot: Slot::End
                },
                Step::AdoptTab {
                    window: 2,
                    tab: 12,
                    slot: Slot::End
                },
                Step::Regroup {
                    window: 2,
                    tabs: vec![11, 12],
                    target
                },
                Step::CloseIfEmptied { window: 1 },
            ]
        );
        // The window comes forward on the tab that was on screen if it is among them, else the
        // first.
        assert_eq!(
            moved.after,
            [
                Step::Raise { window: 2 },
                Step::Select { window: 2, tab: 11 }
            ]
        );
        let undo = moved
            .undo
            .expect("a move between windows can be taken back");
        assert_eq!(
            undo.scenes,
            [world[0].scene(&[11, 12]), world[1].scene(&[])]
        );
        assert_eq!(undo.scenes[1].groups, world[1].groups);
    }

    #[test]
    fn tabs_let_go_on_their_own_window_are_regrouped_there() {
        let world = two_grouped();
        let api = gid(&world[0], "api");
        let target = Target::Group {
            group: api,
            before: None,
        };
        assert_eq!(
            plan(
                &world,
                Move::TabsToWindow {
                    window: 1,
                    tabs: vec![10],
                    into: 1,
                    target: target.clone()
                },
                &Cramped
            ),
            plan(
                &world,
                Move::RegroupTabs {
                    window: 1,
                    tabs: vec![10],
                    target
                },
                &Cramped
            )
        );
    }

    #[test]
    fn tabs_let_go_where_nothing_is_end_quietly_or_beep() {
        let mut world = two_grouped();
        let go = |world: &[Window], target| {
            plan(
                world,
                Move::TabsToWindow {
                    window: 1,
                    tabs: vec![11],
                    into: 2,
                    target,
                },
                &Cramped,
            )
        };
        // A group the window has not, a tab it has not: nothing has left its window.
        assert_eq!(
            go(
                &world,
                Target::Group {
                    group: stray(),
                    before: None
                }
            ),
            Err(Refusal::Quiet)
        );
        // None of the tabs is there.
        assert_eq!(
            plan(
                &world,
                Move::TabsToWindow {
                    window: 1,
                    tabs: vec![99],
                    into: 2,
                    target: Target::Loose { before: None }
                },
                &Cramped
            ),
            Err(Refusal::Quiet)
        );
        // A question held by either window holds them back.
        world[1].asking = true;
        assert_eq!(
            go(&world, Target::Loose { before: None }),
            Err(Refusal::Beep)
        );
    }

    #[test]
    fn tabs_that_are_all_a_window_has_close_it_when_they_cross_and_cannot_be_undone() {
        let mut world = two_grouped();
        world.push(solo());
        let moved = plan(
            &world,
            Move::TabsToWindow {
                window: 3,
                tabs: vec![30],
                into: 1,
                target: Target::Loose { before: None },
            },
            &Cramped,
        )
        .unwrap();
        assert_eq!(
            moved.steps.last(),
            Some(&Step::CloseIfEmptied { window: 3 })
        );
        assert_eq!(moved.undo, None);
    }

    #[test]
    fn a_pane_let_go_at_a_groups_border_goes_where_the_target_says() {
        let world = two_grouped();
        let x = gid(&world[1], "x");
        let api = gid(&world[0], "api");
        // Another window: the tab (110's was the only pane of 11) joins it, then goes to the end
        // of x — the gap before 21 would have been the head of x.
        let across = plan(
            &world,
            Move::PaneToNewTab {
                pane: 110,
                window: 2,
                gap: 1,
                target: Some(Target::Group {
                    group: x,
                    before: None,
                }),
            },
            &Cramped,
        )
        .unwrap();
        assert!(across.steps.contains(&Step::Regroup {
            window: 2,
            tabs: vec![11],
            target: Target::Group {
                group: x,
                before: None
            }
        }));
        let adopted = across
            .steps
            .iter()
            .position(|step| matches!(step, Step::AdoptTab { .. }))
            .unwrap();
        let regrouped = across
            .steps
            .iter()
            .position(|step| matches!(step, Step::Regroup { .. }))
            .unwrap();
        assert!(adopted < regrouped, "it is put where it goes once it is in");

        // Its own window, a tab's only pane: the tab is regrouped, and glows once.
        let target = Target::Group {
            group: api,
            before: None,
        };
        let own = plan(
            &world,
            Move::PaneToNewTab {
                pane: 130,
                window: 1,
                gap: 3,
                target: Some(target.clone()),
            },
            &Cramped,
        )
        .unwrap();
        assert_eq!(
            own.steps,
            [Step::Regroup {
                window: 1,
                tabs: vec![13],
                target: target.clone()
            }]
        );
        assert_eq!(own.after, [Step::Pulse { window: 1, tab: 13 }]);

        // One pane of several becomes a tab, then goes where the target says.
        let host = Cramped;
        let made = plan(
            &world,
            Move::PaneToNewTab {
                pane: 101,
                window: 1,
                gap: 1,
                target: Some(target.clone()),
            },
            &host,
        )
        .unwrap();
        assert_eq!(
            made.steps.last(),
            Some(&Step::Regroup {
                window: 1,
                tabs: vec![NEW],
                target
            })
        );
        // Without a target nothing is asked of the groups: today's plan.
        let plain = plan(
            &world,
            Move::PaneToNewTab {
                pane: 101,
                window: 1,
                gap: 1,
                target: None,
            },
            &host,
        )
        .unwrap();
        assert!(
            !plain
                .steps
                .iter()
                .any(|step| matches!(step, Step::Regroup { .. }))
        );
    }
}
