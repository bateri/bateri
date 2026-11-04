//! `bateri focus`, `zoom`, `swap`, `move` and `resize`: what moves the keyboard or the layout of
//! what is open.
//!
//! Two decisions come before anything is done, both pure and tested where they live:
//!
//! - **the level** ([`level::judge`]): a pane of the server is its machine's own; a tab, a group
//!   and a window are made of panes, so `own` changes one only when every pane in it is the
//!   server's. A layer none of whose panes is the server's is not there for that level (the same
//!   answer as for an id nobody has);
//! - **the keyboard** ([`rule`]): whatever the level, a command that would change where the person
//!   types is done only from the pane the cursor is in, or while bateri is behind, or in a window
//!   other than the one being typed in.
//!
//! Everything runs on the main thread, in the window code's own functions (`select_tab`,
//! `show_pane`, `swap_panes`, `move_pane`, the moves' planner): the layout is changed the way the
//! user's own keys and menus change it, with the same undo and the same refusals.

use bt_shell_common::agent::arrange::{self as rule, Go, Keyboard};
use bt_shell_common::agent::level::{self, Act, Panes, Verdict};
use bt_shell_common::agent::message::{Arrange, Failure, Kind, Reply, Subject};
use bt_shell_common::agent::place::Side;
use bt_shell_common::agent::serve::Server;
use bt_shell_common::kablo::service::Letter;
use bt_shell_common::split::Direction;
use objc2::MainThreadOnly;
use objc2::rc::Retained;
use objc2_app_kit::NSApplication;

use super::{find_pane, terminal_of};
use crate::app::AppDelegate;
use crate::groups::GroupId;
use crate::ids::{Id, Kind as IdKind};
use crate::moves::{Joins, Move};
use crate::pane::TerminalPane;
use crate::split::Tree;
use crate::tab::TerminalTab;
use crate::window::TerminalWindow;

/// Answers the request: it is done or it is a [`Failure`] with the words the script prints.
pub(super) fn arranged(
    app: &AppDelegate,
    letter: Letter,
    server: &Server,
    from: Option<&str>,
    request: &Arrange,
) {
    let reply = match carry_out(app, server, from, request) {
        Ok(()) => Reply::Done,
        Err(failure) => Reply::Failed(failure),
    };
    letter.answer(reply.encode().into_bytes());
}

type Done = Result<(), Failure>;

fn unknown(text: impl Into<String>) -> Failure {
    Failure::new(Kind::Unknown, text)
}

fn failed(text: impl Into<String>) -> Failure {
    Failure::new(Kind::Failed, text)
}

/// A pane of this server, with the tab and window it stands in.
pub(super) struct Spot {
    window: Retained<TerminalWindow>,
    tab: Retained<TerminalTab>,
    pane: Retained<TerminalPane>,
}

/// The pane that shows `name` of the server, in a window.
pub(super) fn spot(app: &AppDelegate, server: &Server, name: &str) -> Option<Spot> {
    let (tab, pane, _) = find_pane(app, &server.account, name)?;
    let window = tab.window()?;
    Some(Spot { window, tab, pane })
}

/// The pane named, or the caller's own.
fn pane_or_own(
    app: &AppDelegate,
    server: &Server,
    from: Option<&str>,
    named: Option<&str>,
) -> Result<Spot, Failure> {
    let name = named.or(from).ok_or_else(|| {
        Failure::new(
            Kind::Usage,
            "this was not asked from a terminal and names none",
        )
    })?;
    spot(app, server, name)
        .ok_or_else(|| unknown(format!("{name} is no pane on screen on this server")))
}

/// Where the keyboard is: bateri in front, the window typed in, whether the cursor is in the pane
/// that asked.
pub(super) fn keyboard(app: &AppDelegate, caller: Option<&Spot>) -> Keyboard {
    let front = NSApplication::sharedApplication(app.mtm()).isActive();
    let window = if front {
        app.windows()
            .into_iter()
            .find(|window| window.ns_window().isKeyWindow())
            .map(|window| window.number())
    } else {
        None
    };
    Keyboard {
        front,
        window,
        caller_holds: front && caller.is_some_and(|caller| holds(&caller.pane)),
    }
}

/// The cursor is in `pane` of the window being typed in (a web view's included).
fn holds(pane: &TerminalPane) -> bool {
    pane.is_active() && pane.holds_keyboard()
}

/// The panes of a tab by whose they are: this server's or anything else's.
fn panes_of(tab: &TerminalTab, server: &Server) -> Panes {
    tab.panes().iter().fold(Panes::default(), |sum, pane| {
        sum.and(if terminal_of(pane, &server.account).is_some() {
            Panes::own(1)
        } else {
            Panes::other(1)
        })
    })
}

/// The level's word on changing a layer of these panes: allowed, or the failure that says why.
fn may_change(server: &Server, panes: Panes, what: &str) -> Done {
    match level::judge(server.level, Act::Change, panes) {
        Verdict::Allowed => Ok(()),
        Verdict::Refused(why) => Err(Failure::new(
            Kind::Refused,
            level::refusal(server.level, why, Some(&server.host)),
        )),
        Verdict::Unknown => Err(unknown(format!("there is no {what}"))),
    }
}

/// A tab is on screen when it is the selected one of a window that shows.
fn on_screen(window: &TerminalWindow, tab: &TerminalTab) -> bool {
    window.is_selected(tab.id()) && window.window_visible()
}

/// A layout change in `tab` of `window`: free where nobody types, refused where it would take
/// the keyboard from under the person's hands.
fn lays_out(
    app: &AppDelegate,
    caller: Option<&Spot>,
    window: &TerminalWindow,
    tab: &TerminalTab,
) -> Done {
    if rule::lays_out(
        keyboard(app, caller),
        window.number(),
        on_screen(window, tab),
    ) {
        Ok(())
    } else {
        Err(Failure::new(Kind::Refused, rule::REFUSAL))
    }
}

fn carry_out(app: &AppDelegate, server: &Server, from: Option<&str>, request: &Arrange) -> Done {
    let caller = from.and_then(|name| spot(app, server, name));
    match request {
        Arrange::Focus(subject) => focus(app, server, caller.as_ref(), from, subject),
        Arrange::Zoom(named) => {
            let target = pane_or_own(app, server, from, named.as_ref().map(|name| name.name()))?;
            lays_out(app, caller.as_ref(), &target.window, &target.tab)?;
            target.tab.zoom_pane(&target.pane);
            Ok(())
        }
        Arrange::Swap { pane, other } => {
            let first = pane_or_own(app, server, from, Some(pane.name()))?;
            let second = pane_or_own(app, server, from, Some(other.name()))?;
            if first.tab.id() != second.tab.id() {
                return Err(Failure::new(
                    Kind::Usage,
                    format!(
                        "{pane} and {other} are not in the same tab; move one beside the other first"
                    ),
                ));
            }
            lays_out(app, caller.as_ref(), &first.window, &first.tab)?;
            if first
                .window
                .swap_panes(first.tab.id(), first.pane.id(), second.pane.id())
            {
                Ok(())
            } else {
                Err(failed(format!("{pane} and {other} could not trade places")))
            }
        }
        Arrange::Move { pane, beside, side } => {
            let moving = pane_or_own(app, server, from, Some(pane.name()))?;
            let target = pane_or_own(app, server, from, Some(beside.name()))?;
            lays_out(app, caller.as_ref(), &moving.window, &moving.tab)?;
            lays_out(app, caller.as_ref(), &target.window, &target.tab)?;
            move_beside(app, &moving, &target, *side)
        }
        Arrange::Resize { pane, side, steps } => {
            let target = pane_or_own(app, server, from, pane.as_ref().map(|name| name.name()))?;
            lays_out(app, caller.as_ref(), &target.window, &target.tab)?;
            let direction = direction_of(*side)?;
            if target.tab.resize_pane(&target.pane, direction, *steps) {
                Ok(())
            } else {
                Err(failed("the divider is at its limit"))
            }
        }
    }
}

fn direction_of(side: Side) -> Result<Direction, Failure> {
    side.direction()
        .ok_or_else(|| Failure::new(Kind::Usage, "`side` is left, right, above or below"))
}

/// `move pane p9 --beside p3 --right`: within a tab the tree takes the pane back beside the other;
/// into another tab (of this window or another) the move planner joins it as the menus' do.
fn move_beside(app: &AppDelegate, moving: &Spot, target: &Spot, side: Side) -> Done {
    let direction = direction_of(side)?;
    if moving.pane.id() == target.pane.id() {
        return Err(Failure::new(Kind::Usage, "a pane cannot go beside itself"));
    }
    if moving.tab.id() == target.tab.id() {
        let placement = moving
            .tab
            .plan_move_within(moving.pane.id(), target.pane.id(), direction)
            .ok_or_else(|| failed("there is no room for it there"))?;
        return if moving
            .window
            .move_pane(moving.tab.id(), moving.pane.id(), placement.tree)
        {
            Ok(())
        } else {
            Err(failed("the pane could not move there"))
        };
    }
    let placement = target
        .tab
        .container()
        .plan_beside(
            target.pane.id(),
            direction,
            &Tree::Leaf(moving.pane.id()),
            std::slice::from_ref(&moving.pane),
        )
        .ok_or_else(|| failed("there is no room for it there"))?;
    let done = app.make_move(Move::PaneToTab {
        pane: moving.pane.id(),
        into: target.tab.id(),
        place: Joins::Planned(placement.tree),
    });
    if done {
        Ok(())
    } else {
        Err(failed("the pane could not move there"))
    }
}

// ─── focus ───────────────────────────────────────────────────────────────

/// What `focus` reaches: the tab to select, the pane to take the keyboard if one is named, the
/// window it is in.
struct Target {
    window: Retained<TerminalWindow>,
    /// `None` for a window: it only comes forward.
    tab: Option<Retained<TerminalTab>>,
    pane: Option<Retained<TerminalPane>>,
    /// A group to arrive at: the tab it was last left on is chosen by the window.
    group: Option<GroupId>,
}

fn focus(
    app: &AppDelegate,
    server: &Server,
    caller: Option<&Spot>,
    from: Option<&str>,
    subject: &Subject,
) -> Done {
    let target = match subject {
        Subject::Pane(named) => {
            let spot = pane_or_own(app, server, from, named.as_ref().map(|name| name.name()))?;
            // A pane of this server is its machine's own.
            Target {
                window: spot.window,
                tab: Some(spot.tab),
                pane: Some(spot.pane),
                group: None,
            }
        }
        Subject::Tab(id) => {
            let tab = match id {
                Some(id) => tab_numbered(app, *id)?,
                None => own_tab(caller)?.tab.clone(),
            };
            may_change(
                server,
                panes_of(&tab, server),
                &format!("tab {}", tab_name(&tab)),
            )?;
            let window = tab
                .window()
                .ok_or_else(|| failed("the tab has no window"))?;
            Target {
                window,
                tab: Some(tab),
                pane: None,
                group: None,
            }
        }
        Subject::Group(id) => {
            let (window, group) = match id {
                Some(id) => group_numbered(app, *id)?,
                None => {
                    let own = own_tab(caller)?;
                    let group = own
                        .window
                        .groups()
                        .group_of(own.tab.id())
                        .ok_or_else(|| unknown("this tab is in no group"))?;
                    (own.window.clone(), group)
                }
            };
            let panes = window
                .tabs_of_group(group)
                .iter()
                .fold(Panes::default(), |sum, tab| sum.and(panes_of(tab, server)));
            may_change(server, panes, "group")?;
            Target {
                window,
                tab: None,
                pane: None,
                group: Some(group),
            }
        }
        Subject::Window(id) => {
            let window = match id {
                Some(id) => window_numbered(app, *id)?,
                None => own_tab(caller)?.window.clone(),
            };
            let panes = window
                .tabs()
                .iter()
                .fold(Panes::default(), |sum, tab| sum.and(panes_of(tab, server)));
            may_change(server, panes, "window")?;
            Target {
                window,
                tab: None,
                pane: None,
                group: None,
            }
        }
    };
    let go = rule::go(keyboard(app, caller), target.window.number());
    if go == Go::Refused {
        return Err(Failure::new(Kind::Refused, rule::REFUSAL));
    }
    reach(&target, go)
}

fn tab_name(tab: &TerminalTab) -> String {
    Id::new(IdKind::Tab, tab.number()).map_or_else(String::new, |id| id.to_string())
}

fn own_tab(caller: Option<&Spot>) -> Result<&Spot, Failure> {
    caller.ok_or_else(|| {
        Failure::new(
            Kind::Usage,
            "this was not asked from a pane on screen on this server; name an id",
        )
    })
}

fn tab_numbered(app: &AppDelegate, id: Id) -> Result<Retained<TerminalTab>, Failure> {
    app.tabs()
        .into_iter()
        .find(|tab| tab.number() == id.number())
        .ok_or_else(|| unknown(format!("there is no tab {id}")))
}

fn window_numbered(app: &AppDelegate, id: Id) -> Result<Retained<TerminalWindow>, Failure> {
    app.windows()
        .into_iter()
        .find(|window| window.number() == id.number())
        .ok_or_else(|| unknown(format!("there is no window {id}")))
}

fn group_numbered(
    app: &AppDelegate,
    id: Id,
) -> Result<(Retained<TerminalWindow>, GroupId), Failure> {
    for window in app.windows() {
        let found = window
            .groups()
            .groups()
            .iter()
            .find(|group| group.number == Some(id.number()))
            .map(|group| group.id);
        if let Some(group) = found {
            return Ok((window, group));
        }
    }
    Err(unknown(format!("there is no group {id}")))
}

/// Takes the target as `go` says: raised and keyboard-taking, ordered in front of bateri's other
/// windows with bateri left behind, or chosen inside its window with everything else where it was.
fn reach(target: &Target, go: Go) -> Done {
    let window = &target.window;
    let chosen = if let Some(group) = target.group {
        if !window.reach_group_id(group) {
            return Err(failed("the group's tab cannot be selected now"));
        }
        window.try_selected_tab()
    } else if let Some(tab) = &target.tab {
        if !window.select_tab(tab.id()) {
            return Err(failed("the tab cannot be selected now"));
        }
        Some(tab.clone())
    } else {
        window.try_selected_tab()
    };
    if let (Some(tab), Some(pane)) = (&chosen, &target.pane) {
        tab.focus_pane(pane);
    }
    match go {
        Go::Raise => {
            if let Some(tab) = chosen {
                let pane = target.pane.clone().unwrap_or_else(|| tab.focused_pane());
                window.bring_to_front(&tab, &pane);
            } else {
                window.ns_window().makeKeyAndOrderFront(None);
            }
        }
        Go::Order => window.ns_window().orderFront(None),
        Go::Within | Go::Refused => {}
    }
    Ok(())
}
