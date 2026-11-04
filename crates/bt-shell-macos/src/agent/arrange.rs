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

use bt_shell_common::agent::Anchor;
use bt_shell_common::agent::arrange::{self as rule, Go, Keyboard};
use bt_shell_common::agent::level::{self, Act, Panes, Verdict};
use bt_shell_common::agent::message::{Arrange, Failure, Kind, Reply, Subject, TabHome};
use bt_shell_common::agent::place::Side;
use bt_shell_common::agent::serve::Server;
use bt_shell_common::groups::Target as Strip;
use bt_shell_common::kablo::service::Letter;
use bt_shell_common::split::Direction;
use objc2::MainThreadOnly;
use objc2::rc::Retained;
use objc2_app_kit::NSApplication;

use super::{find_address, find_pane, terminal_of};
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

pub(super) type Done = Result<(), Failure>;

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

/// The pane named — a terminal of the machine that asked, or of another by its address
/// (`p9@prod`, from `screen` up) — or the caller's own.
fn pane_or_own(
    app: &AppDelegate,
    server: &Server,
    from: Option<&str>,
    named: Option<&Anchor>,
) -> Result<Spot, Failure> {
    let Some(address) = named else {
        let name = from.ok_or_else(|| {
            Failure::new(
                Kind::Usage,
                "this was not asked from a terminal and names none",
            )
        })?;
        return spot(app, server, name)
            .ok_or_else(|| unknown(format!("{name} is no pane on screen on this server")));
    };
    let (tab, pane, _) = find_address(app, server, address)?.ok_or_else(|| {
        if address.machine().is_some() {
            unknown(format!("{address} is no pane on screen"))
        } else {
            unknown(format!("{address} is no pane on screen on this server"))
        }
    })?;
    let window = tab
        .window()
        .ok_or_else(|| unknown(format!("{address} is in no window")))?;
    Ok(Spot { window, tab, pane })
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
pub(super) fn panes_of(tab: &TerminalTab, server: &Server) -> Panes {
    tab.panes().iter().fold(Panes::default(), |sum, pane| {
        sum.and(if terminal_of(pane, &server.account).is_some() {
            Panes::own(1)
        } else {
            Panes::other(1)
        })
    })
}

/// The level's word on changing a layer of these panes: allowed, or the failure that says why.
pub(super) fn may_change(server: &Server, panes: Panes, what: &str) -> Done {
    match level::judge(server.level, Act::Change, panes) {
        Verdict::Allowed => Ok(()),
        Verdict::Refused(why) => Err(Failure::new(
            Kind::Refused,
            level::refusal(server.level, why, server.host_name()),
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
            let target = pane_or_own(app, server, from, named.as_ref())?;
            lays_out(app, caller.as_ref(), &target.window, &target.tab)?;
            target.tab.zoom_pane(&target.pane);
            Ok(())
        }
        Arrange::Swap { pane, other } => {
            let first = pane_or_own(app, server, from, Some(pane))?;
            let second = pane_or_own(app, server, from, Some(other))?;
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
            let moving = pane_or_own(app, server, from, Some(pane))?;
            let target = pane_or_own(app, server, from, Some(beside))?;
            lays_out(app, caller.as_ref(), &moving.window, &moving.tab)?;
            lays_out(app, caller.as_ref(), &target.window, &target.tab)?;
            move_beside(app, &moving, &target, *side)
        }
        Arrange::Close(subject) => close(app, server, caller.as_ref(), subject),
        Arrange::Rename { subject, label } => {
            rename(app, server, caller.as_ref(), subject, label.as_deref())
        }
        Arrange::MoveTab { tab, to } => move_tab(app, server, caller.as_ref(), *tab, to),
        Arrange::Resize { pane, side, steps } => {
            let target = pane_or_own(app, server, from, pane.as_ref())?;
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

// ─── close, rename, move tab ─────────────────────────────────────────────

/// The layer a command names, or the caller's own when it names none: its window and the tabs in
/// it. The level has judged it.
struct Layer {
    window: Retained<TerminalWindow>,
    tabs: Vec<Retained<TerminalTab>>,
    /// The group, when the layer is one.
    group: Option<GroupId>,
}

fn layer(
    app: &AppDelegate,
    server: &Server,
    caller: Option<&Spot>,
    subject: &Subject,
) -> Result<Layer, Failure> {
    let (window, tabs, group, what) = match subject {
        Subject::Tab(id) => {
            let tab = match id {
                Some(id) => tab_numbered(app, *id)?,
                None => own_tab(caller)?.tab.clone(),
            };
            let window = tab
                .window()
                .ok_or_else(|| failed("the tab has no window"))?;
            let what = format!("tab {}", tab_name(&tab));
            (window, vec![tab], None, what)
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
            let tabs = window.tabs_of_group(group);
            (window, tabs, Some(group), "group".to_owned())
        }
        Subject::Window(id) => {
            let window = match id {
                Some(id) => window_numbered(app, *id)?,
                None => own_tab(caller)?.window.clone(),
            };
            let tabs = window.tabs();
            (window, tabs, None, "window".to_owned())
        }
        Subject::Pane(_) => {
            return Err(Failure::new(
                Kind::Usage,
                "a pane is not a layer: a pane's terminal is closed, a pane's label is kablo's",
            ));
        }
    };
    let panes = tabs
        .iter()
        .fold(Panes::default(), |sum, tab| sum.and(panes_of(tab, server)));
    may_change(server, panes, &what)?;
    Ok(Layer {
        window,
        tabs,
        group,
    })
}

/// `close tab|group|window`: every tab of the layer closes without a question, the programs in
/// its panes end, and the window closes with its last tab. The level has judged the layer.
fn close(app: &AppDelegate, server: &Server, caller: Option<&Spot>, subject: &Subject) -> Done {
    let layer = layer(app, server, caller, subject)?;
    if matches!(subject, Subject::Window(_)) {
        layer.window.close();
        return Ok(());
    }
    // The tabs are named by id first: a closing tab takes its neighbours' places.
    let ids: Vec<u64> = layer.tabs.iter().map(|tab| tab.id()).collect();
    drop(layer.tabs);
    for id in ids {
        layer.window.close_tab_now(id);
    }
    Ok(())
}

/// `rename tab|group ID LABEL`: an empty label takes a tab's name away (its own title shows
/// again); a group always has one.
fn rename(
    app: &AppDelegate,
    server: &Server,
    caller: Option<&Spot>,
    subject: &Subject,
    label: Option<&str>,
) -> Done {
    if matches!(subject, Subject::Window(_)) {
        return Err(Failure::new(Kind::Usage, "a window has no label"));
    }
    let layer = layer(app, server, caller, subject)?;
    match (subject, layer.group) {
        (Subject::Group(_), Some(group)) => {
            let label = label
                .filter(|label| !label.trim().is_empty())
                .ok_or_else(|| {
                    Failure::new(Kind::Usage, "a group always has a label: say which")
                })?;
            layer.window.rename_group(group, label);
        }
        _ => {
            let tab = layer
                .tabs
                .first()
                .ok_or_else(|| failed("there is no tab"))?;
            layer.window.rename_tab(tab.id(), label.unwrap_or(""));
        }
    }
    Ok(())
}

/// `move tab t3 --group g1|--window w1`: the tab goes into the group, or into the window out of
/// any group, as the user's own move does, with its Undo Move and its id. The level judges the
/// tab and where it goes; the keyboard rule looks at the tab if it is on screen and at the window
/// it comes to.
fn move_tab(
    app: &AppDelegate,
    server: &Server,
    caller: Option<&Spot>,
    id: Id,
    to: &TabHome,
) -> Done {
    let tab = tab_numbered(app, id)?;
    may_change(server, panes_of(&tab, server), &format!("tab {id}"))?;
    let from = tab
        .window()
        .ok_or_else(|| failed("the tab has no window"))?;
    let (into, target) = match to {
        TabHome::Group(group_id) => {
            let (window, group) = group_numbered(app, *group_id)?;
            let panes = window
                .tabs_of_group(group)
                .iter()
                .fold(Panes::default(), |sum, tab| sum.and(panes_of(tab, server)));
            may_change(server, panes, &format!("group {group_id}"))?;
            (
                window,
                Strip::Group {
                    group,
                    before: None,
                },
            )
        }
        TabHome::Window(window_id) => {
            let window = window_numbered(app, *window_id)?;
            let panes = window
                .tabs()
                .iter()
                .fold(Panes::default(), |sum, tab| sum.and(panes_of(tab, server)));
            may_change(server, panes, &format!("window {window_id}"))?;
            (window, Strip::Loose { before: None })
        }
    };
    lays_out(app, caller, &from, &tab)?;
    let keyboard = keyboard(app, caller);
    let go = rule::go(keyboard, into.number());
    if from.id() != into.id() && go == Go::Refused {
        return Err(Failure::new(Kind::Refused, rule::REFUSAL));
    }
    let wanted = if from.id() == into.id() {
        Move::RegroupTabs {
            window: from.id(),
            tabs: vec![tab.id()],
            target,
        }
    } else {
        Move::TabsToWindow {
            window: from.id(),
            tabs: vec![tab.id()],
            into: into.id(),
            target,
        }
    };
    if app.make_move_raising(wanted, go == Go::Raise) {
        Ok(())
    } else {
        Err(failed("the tab could not move there"))
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
            let spot = pane_or_own(app, server, from, named.as_ref())?;
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

pub(super) fn tab_numbered(app: &AppDelegate, id: Id) -> Result<Retained<TerminalTab>, Failure> {
    app.tabs()
        .into_iter()
        .find(|tab| tab.number() == id.number())
        .ok_or_else(|| unknown(format!("there is no tab {id}")))
}

pub(super) fn window_numbered(
    app: &AppDelegate,
    id: Id,
) -> Result<Retained<TerminalWindow>, Failure> {
    app.windows()
        .into_iter()
        .find(|window| window.number() == id.number())
        .ok_or_else(|| unknown(format!("there is no window {id}")))
}

pub(super) fn group_numbered(
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

#[cfg(test)]
mod tests {
    /// What changes a tab, a group or a window passes the level before it touches a window; the
    /// keyboard rule looks at a tab that moves; and nothing here goes through a question (a
    /// program on a server cannot answer one, and nobody is asked). A source scan: the windows
    /// are AppKit's and need a window server.
    #[test]
    fn closing_renaming_and_moving_a_tab_pass_the_level_and_ask_nobody() {
        let source = include_str!("arrange.rs");
        let source = &source[..source.find("#[cfg(test)]").unwrap_or(source.len())];
        let body = |signature: &str| {
            let at = source.find(signature).expect(signature);
            &source[at..at + source[at..].find("\n}\n").expect("its end")]
        };
        let layer = body("fn layer(");
        assert!(layer.contains("may_change(server, panes,"), "{layer}");
        let close = body("fn close(");
        assert!(
            close.contains("layer(app, server, caller, subject)"),
            "{close}"
        );
        assert!(
            close.contains("close_tab_now(") && close.contains(".close()"),
            "{close}"
        );
        assert!(
            !close.contains("close_tabs_asking") && !close.contains("close_group("),
            "{close}"
        );
        let rename = body("fn rename(");
        assert!(
            rename.contains("layer(app, server, caller, subject)"),
            "{rename}"
        );
        let moved = body("fn move_tab(");
        // The tab, the group or the window it goes to: each judged.
        assert_eq!(moved.matches("may_change(").count(), 3, "{moved}");
        let judged = moved.find("may_change(").expect(moved);
        let lays = moved.find("lays_out(").expect(moved);
        let go = moved.find("rule::go(").expect(moved);
        let made = moved.find("make_move_raising(").expect(moved);
        assert!(judged < lays && lays < go && go < made, "{moved}");
    }
}
