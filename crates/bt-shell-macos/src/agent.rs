//! What a program on a server asks of the app, and the panes that come of it.
//!
//! `bateri split` on a server starts a terminal there (kablo's, numbered `p9`) and asks the Mac
//! where its pane goes, over the conversation kablo gives the app ([`Letter`]). The decision is
//! `bt-shell-common`'s ([`bt_shell_common::agent::serve`], with the rule of where a pane lands
//! in [`bt_shell_common::agent::place`]); this module lends it the windows ([`Shell`]) and
//! puts the pane where it says. **Nothing here runs a program or writes a byte to a shell**: the
//! pane is the pipe to a terminal that already exists on the server, asked for by name — the
//! request is a *place*, and an app that is asked cannot be made to run anything by it.
//!
//! - **The user's screen does not change.** The pane is born without the keyboard (`--focus`
//!   gives it under the keyboard rule below), in a zoomed tab it is born behind the zoom, and a
//!   tab is added where it was asked (beside the anchor's tab, in a group, in a window) without
//!   being selected ([`TerminalWindow::add_tab_lodged`]); a window made for a pane comes in front
//!   of bateri's others without the keyboard ([`TerminalWindow::show_apart`]): nothing the user
//!   was looking at moves, and the tab strip shows the new one running.
//! - **The program is the point of the pane.** Its pipe closes with its terminal
//!   (`ssh-hold --close-at-end`, [`Held::close_at_end`]) so the pane goes when the program does —
//!   unless `--keep-open` was asked, when the shell follows as it does for any pane. The pane
//!   keeps that, and who asked for it ([`Held::opener`], its card's "Opened by"), across a
//!   restart: session restore saves both with the terminal it shows.
//! - **A pane that cannot be placed waits; a place that is not there is refused.** The terminal
//!   runs on the server and is in the list of what runs in the background; a request whose
//!   anchor is on no tab of this Mac is kept here ([`Queue`]) until a pane shows the anchor — a pane opened
//!   from that list, restored, moved here, any way ([`terminal_shown`]) — and then goes beside
//!   it. A restart loses the pairing and not the terminal.
//! - **What was left while the Mac was away** is read when a connection to the server comes up
//!   ([`notes`]) and placed the same way, a terminal that ended meanwhile excepted unless it was
//!   asked to stay.
//! - **The user is told once.** Panes that open while bateri is behind are one notification
//!   per server that grows ("Claude opened 3 terminals on rasp"), not one for each; it is
//!   forgotten when bateri comes to the front ([`seen`]).
//! - **`ls` is what is on screen and what the server has, side by side.** The windows, tabs and
//!   split trees are read where they stand when the server's list arrives ([`windows_of`]); the
//!   terminals are the server's own words, asked for now ([`hold_service::survey`]), so the
//!   answer is the same table the script prints without a Mac, with the tabs filled in.
//! - **Nothing asks the user; the machine's level decides.** What the command may do is the
//!   server's [`CommandLevel`] ([`bt_shell_common::agent::level`]), read from the settings when
//!   the request arrives: `off` refuses everything, `own` shows and changes what is the
//!   server's, `screen` and `full` see every machine's panes in `ls` ([`pane_of`]). A program
//!   closes any terminal of its own server, opened by it or not, the way the user closing the
//!   pane does — the pane goes and the server's terminal ends with it — or, for a terminal with
//!   no pane, the card's End.
//! - **The keyboard has one rule** ([`bt_shell_common::agent::arrange`]). `focus`, `zoom`, `swap`,
//!   `move`, `resize` and `--focus` are done from the pane the cursor is in, while bateri is
//!   behind, or into a window other than the one being typed in; otherwise they are refused (77)
//!   or, for `--focus`, the pane is made and the answer says the keyboard stayed ([`arrange`]).
//! - **`notify` is one notification and a tick.** The notification when bateri is behind, or in
//!   front with no tab of this Mac showing the program (then nothing else could show it); the tab
//!   the program runs in is marked as having news unless it is the one on screen.
//!
//! Everything here runs on the main thread: the service's thread hands a letter over by hopping
//! to the main queue ([`letter`], [`notes`]), and the state that outlives a request ([`STATE`])
//! is read and written there only.

mod arrange;
mod reach;

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use bt_core::CommandLevel;
use bt_core::PaneUuid;
use bt_shell_common::agent::arrange::{self as rule, Go};
use bt_shell_common::agent::level::{self, Panes};
use bt_shell_common::agent::listing::{self, Node, Pane, Scene, Tab, Window};
use bt_shell_common::agent::message::{Failure, Format, Kind, Place, Placed, Reply, WindowHome};
use bt_shell_common::agent::notes::Note;
use bt_shell_common::agent::place::{Ask, Landed, Landing, Spot};
use bt_shell_common::agent::serve::{self, Answer, Placing, Server, Shell};
use bt_shell_common::agent::summary;
use bt_shell_common::agent::wait::{Queue, Waiting};
use bt_shell_common::agent::{Anchor, LOCAL_MACHINE, Terminal};
use bt_shell_common::kablo::SessionId;
use bt_shell_common::kablo::local;
use bt_shell_common::kablo::service::{Letter, Shown, Survey};
use dispatch2::DispatchQueue;
use objc2::rc::Retained;
use objc2::{MainThreadMarker, MainThreadOnly, Message};
use objc2_app_kit::{NSApplication, NSWindow, NSWindowNumberListOptions};

use crate::app::{self, AppDelegate, Opening, Start, pipe_installed};
use crate::embed;
use crate::hold_service;
use crate::ids::{Id, Kind as IdKind};
use crate::launch::{Held, Resume};
use crate::pane::TerminalPane;
use crate::tab::TerminalTab;
use crate::uploader;
use crate::window::{Lodging, TerminalWindow, initial_rect};
use bt_shell_common::child::PaneRuns;

/// What outlives a request.
#[derive(Default)]
struct State {
    /// The requests that wait for their anchor to have a pane.
    waiting: Queue,
    /// How many terminals were opened for the user per server since bateri was last in front:
    /// the number the notification says.
    told: HashMap<String, u32>,
    /// The panes made a moment ago for a terminal: the service does not say a pane shows its
    /// terminal until the pane's pipe has reached it, and a second request for the same
    /// terminal in between must not make a second pane.
    made: Vec<(String, Terminal, Instant)>,
}

/// How long a pane just made stands in for the service's word that it shows its terminal.
const MADE_FOR: Duration = Duration::from_secs(30);

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

fn state<R>(edit: impl FnOnce(&mut State) -> R) -> R {
    STATE.with(|state| edit(&mut state.borrow_mut()))
}

// ─── The doors ───────────────────────────────────────────────────────────

/// A program on a server said something to the app (`Backend::mail`, on the service's thread):
/// the main queue takes it. Nobody to take it hangs up, which the script reads as no Mac.
pub(crate) fn letter(letter: Letter) {
    DispatchQueue::main().exec_async(move || {
        // audit: a block running on the main queue is on the main thread by definition.
        let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
        match app::delegate(mtm) {
            Some(app) => take(&app, letter),
            None => letter.hang_up(),
        }
    });
}

/// What a server left for the app while it was away has been read: the main queue places it.
pub(crate) fn notes(server: Server, notes: Vec<Note>) {
    DispatchQueue::main().exec_async(move || {
        // audit: a block running on the main queue is on the main thread by definition.
        let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
        if let Some(app) = app::delegate(mtm) {
            read(&app, &server, notes);
        }
    });
}

/// A pane began to show a terminal, or stopped (`Backend::terminal_changed`, on the main
/// queue): what waited for that terminal to have a pane goes beside it now, and what waited
/// for a pane of its own is done.
pub(crate) fn terminal_shown(pane: &PaneUuid) {
    let Some(shown) = hold_service::shown(pane) else {
        return;
    };
    let due = state(|state| {
        if let Some(terminal) = Terminal::parse(shown.terminal.as_str()) {
            state.waiting.forget(&shown.account, terminal);
            state
                .made
                .retain(|(account, made, _)| !(*account == shown.account && *made == terminal));
        }
        state
            .waiting
            .take_beside(&shown.account, shown.terminal.as_str())
    });
    if due.is_empty() {
        return;
    }
    // Whether a terminal can still be shown is asked of the server now: the cache of what runs
    // there is not told of a terminal the script on the server started, so it cannot say.
    let mut by_server: Vec<(String, Vec<Waiting>)> = Vec::new();
    for waiting in due {
        match by_server
            .iter_mut()
            .find(|(account, _)| *account == waiting.server.account)
        {
            Some((_, found)) => found.push(waiting),
            None => by_server.push((waiting.server.account.clone(), vec![waiting])),
        }
    }
    for (account, due) in by_server {
        hold_service::survey(&account, move |survey| {
            DispatchQueue::main().exec_async(move || {
                // audit: a block running on the main queue is on the main thread by definition.
                let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
                if let Some(app) = app::delegate(mtm) {
                    settle_due(&app, due, survey);
                }
            });
        });
    }
}

/// The requests that waited for an anchor that has a pane now, against the server's list: those
/// whose terminal can be shown go beside it. A server that could not be asked keeps them waiting
/// (the connection is down, and a pane could not be made for a terminal there).
fn settle_due(app: &AppDelegate, due: Vec<Waiting>, survey: Survey) {
    let Some(statuses) = survey.terminals else {
        state(|state| {
            for waiting in due {
                state.waiting.add(waiting);
            }
        });
        return;
    };
    let mut world = World::new(app);
    for mut waiting in due {
        // A machine whose commands were turned off while the request waited gets no pane.
        waiting.server.level = command_level(app, &waiting.server.account, &waiting.server.host);
        if waiting.server.level == CommandLevel::Off {
            continue;
        }
        let (again, _) = serve::settle_waiting(
            waiting,
            &|terminal, keep_open| serve::alive_in(&statuses, terminal, keep_open),
            &mut world,
        );
        if let Some(again) = again {
            state(|state| state.waiting.add(again));
        }
    }
    world.finish();
}

/// bateri came to the front: what it said while it was behind has been seen.
pub(crate) fn seen() {
    state(|state| state.told.clear());
}

// ─── A letter ────────────────────────────────────────────────────────────

/// What the `bateri` command may do on the machine of `account` just now: this Mac's own level, or
/// the host's entry. Read each time it is needed — a request that waited has the level of the
/// moment it is placed, not of the moment it was asked.
fn command_level(app: &AppDelegate, account: &str, host: &str) -> CommandLevel {
    if local::is_account(account) {
        app.settings().commands_level
    } else {
        app.settings().commands_for(host)
    }
}

fn take(app: &AppDelegate, mut letter: Letter) {
    let server = Server {
        account: letter.account.clone(),
        host: letter.host.clone(),
        argv: letter.argv.clone(),
        level: command_level(app, &letter.account, &letter.host),
    };
    let from = letter.from.as_ref().map(|id| id.as_str().to_owned());
    let bytes = std::mem::take(&mut letter.bytes);
    let Ok(text) = String::from_utf8(bytes) else {
        let failure = Failure::new(Kind::Usage, "the message is not text");
        letter.answer(Reply::Failed(failure).encode().into_bytes());
        return;
    };
    let mut world = World::new(app);
    let answer = serve::serve(&server, from.as_deref(), &text, &mut world);
    match answer {
        Answer::Reply(reply) => letter.answer(reply.encode().into_bytes()),
        Answer::Wait(reply, waiting) => {
            state(|state| state.waiting.add(*waiting));
            letter.answer(reply.encode().into_bytes());
        }
        Answer::List(format) => list(letter, server, from, format),
        Answer::Close(target) => match close_target(app, &server, target) {
            Ok((server, target)) => close(letter, server, target),
            Err(failure) => letter.answer(Reply::Failed(failure).encode().into_bytes()),
        },
        Answer::Notify(text) => notify(app, letter, &server, from.as_deref(), &text),
        Answer::Arrange(request) => {
            arrange::arranged(app, letter, &server, from.as_deref(), &request);
        }
        Answer::Ssh(name) => reach::ssh(app, letter, &server, from.as_deref(), name),
        Answer::PlaceOn(place) => reach::place_on(letter, server, from, place),
        Answer::Relay(relay) => reach::relay(app, letter, &server, from.as_deref(), relay),
    }
    world.finish();
}

/// What a server left while the Mac was away.
fn read(app: &AppDelegate, server: &Server, notes: Vec<Note>) {
    let mut world = World::new(app);
    let read = serve::settle_notes(
        server,
        notes,
        &|terminal, keep_open| alive(&server.account, terminal, keep_open),
        &mut world,
    );
    state(|state| {
        for waiting in read.waiting {
            state.waiting.add(waiting);
        }
    });
    world.finish();
}

/// Whether a pane can still be made for `terminal` of the server: it runs and nobody has it, or
/// it ended and was asked to stay open (the server still lists it).
fn alive(account: &str, terminal: Terminal, keep_open: bool) -> bool {
    let name = terminal.to_string();
    hold_service::background(account)
        .iter()
        .any(|found| found.terminal.as_str() == name)
        || (keep_open
            && hold_service::exited(account)
                .iter()
                .any(|found| found.as_str() == name))
}

// ─── The windows ─────────────────────────────────────────────────────────

/// A pane that was made, for the one line that tells the user.
struct Opened {
    account: String,
    host: String,
    /// The program that asked, as the anchor's terminal says.
    program: Option<String>,
}

/// Where the pane (or tab, or window) a program asked for goes, found among the windows and judged
/// by the level ([`World::whither`]).
enum Dest {
    /// Beside the anchor: in its tab (`split`) or as the tab beside its tab (`tab`).
    Anchor,
    /// Into this tab (`split --tab t3`), beside the pane that has its keyboard.
    Tab(Retained<TerminalTab>),
    /// As a tab of this window, standing as `Lodging` says (`tab --group`, `--window`,
    /// `--new-group`).
    Lodged(Retained<TerminalWindow>, Lodging),
    /// In a window of its own (`window`, `tab --window new`).
    Window,
}

/// The pane of this Mac a request is placed by: the anchor, with its tab and the name the server
/// knows it by.
struct Beside<'a> {
    tab: &'a TerminalTab,
    pane: &'a TerminalPane,
    name: &'a Anchor,
}

/// The Mac's windows, lent to the decision.
struct World<'a> {
    app: &'a AppDelegate,
    opened: Vec<Opened>,
}

impl<'a> World<'a> {
    fn new(app: &'a AppDelegate) -> World<'a> {
        World {
            app,
            opened: Vec::new(),
        }
    }

    /// The user is told what was opened, once for each server.
    fn finish(self) {
        let mtm = self.app.mtm();
        let mut servers: Vec<(String, String, Option<String>, u32)> = Vec::new();
        for opened in self.opened {
            match servers
                .iter_mut()
                .find(|(account, ..)| *account == opened.account)
            {
                Some((_, _, program, count)) => {
                    *count += 1;
                    if program.is_none() {
                        *program = opened.program;
                    }
                }
                None => servers.push((opened.account, opened.host, opened.program, 1)),
            }
        }
        for (account, host, program, count) in servers {
            tell(mtm, &account, &host, program.as_deref(), count);
        }
    }

    /// Where the request puts the pane: the places it names are looked up and judged by the level
    /// (a tab, group or window is changed when a pane goes into it, so `own` may do it only to
    /// one made of its own machine's panes), and a place that is not there is a refusal.
    fn whither(
        &self,
        server: &Server,
        anchor: &TerminalTab,
        place: &Place,
    ) -> Result<Dest, Failure> {
        let app = self.app;
        let home = &place.home;
        if let Some(id) = home.tab {
            let tab = arrange::tab_numbered(app, id)?;
            arrange::may_change(
                server,
                arrange::panes_of(&tab, server),
                &format!("tab {id}"),
            )?;
            return Ok(Dest::Tab(tab));
        }
        if let Some(id) = home.group {
            let (window, group) = arrange::group_numbered(app, id)?;
            let panes = window
                .tabs_of_group(group)
                .iter()
                .fold(Panes::default(), |sum, tab| {
                    sum.and(arrange::panes_of(tab, server))
                });
            arrange::may_change(server, panes, &format!("group {id}"))?;
            return Ok(Dest::Lodged(window, Lodging::Group(group)));
        }
        let lodging = match &home.new_group {
            Some(name) => Lodging::NewGroup(name.clone()),
            None => Lodging::Loose,
        };
        match &home.window {
            Some(WindowHome::Existing(id)) => {
                let window = arrange::window_numbered(app, *id)?;
                let panes = window.tabs().iter().fold(Panes::default(), |sum, tab| {
                    sum.and(arrange::panes_of(tab, server))
                });
                arrange::may_change(server, panes, &format!("window {id}"))?;
                Ok(Dest::Lodged(window, lodging))
            }
            Some(WindowHome::New) => Ok(Dest::Window),
            // A group made in the window the anchor is in.
            None if home.new_group.is_some() => {
                let window = anchor
                    .window()
                    .ok_or_else(|| Failure::new(Kind::Failed, "the tab has no window"))?;
                Ok(Dest::Lodged(window, lodging))
            }
            None => Ok(Dest::Anchor),
        }
    }

    /// Makes the pane; `opener` is who asked for it, in the words its tab's card says them.
    /// `raise` is whether the keyboard may go to a window made for it. `Err` is "not now": the
    /// terminal stays in the list.
    fn put(
        &self,
        target: &Server,
        by: &Beside,
        place: &Place,
        opener: String,
        (dest, raise): (&Dest, bool),
    ) -> Result<Placed, String> {
        let app = self.app;
        let mtm = app.mtm();
        let Beside {
            tab,
            pane: anchor,
            name: anchor_name,
        } = *by;
        // The tab the pane is planned in, and the pane it stands beside there.
        let (into, beside) = match dest {
            Dest::Tab(target) => (target.clone(), target.focused_pane()),
            _ => (tab.retain(), anchor.retain()),
        };
        let window = into.window().ok_or("the tab has no window")?;
        let name = place.terminal.to_string();
        let terminal = SessionId::new(&name).map_err(|e| e.to_string())?;
        // A pane of this Mac is an ordinary one that is born with the identity its terminal was
        // written in the ledger for; a server's is the pipe to a terminal there.
        let uuid = local::is_account(&target.account).then(crate::pane::new_pane_uuid);
        let start = if uuid.is_some() {
            Start {
                // A pane with no program is started where the script was asked to start it; a
                // program's terminal is where its script put it.
                directory: place.fresh.as_ref().map(PathBuf::from),
                line: None,
                run: false,
                held: None,
                uuid: uuid.clone(),
                runs: place.runs(),
            }
        } else {
            if !pipe_installed(&app.shell_integration().env) {
                return Err("there is no pipe to a server in this process".to_owned());
            }
            let args = target
                .argv
                .get(1..)
                .filter(|args| !args.is_empty())
                .ok_or("the server is not named by arguments")?
                .to_vec();
            Start {
                directory: None,
                line: None,
                run: false,
                held: Some(Held {
                    args,
                    resume: Some(Resume {
                        account: target.account.clone(),
                        terminal: name,
                    }),
                    close_at_end: !place.keep_open,
                    opener: Some(opener),
                }),
                uuid: None,
                runs: PaneRuns::UserShell,
            }
        };
        let incoming = app.next_id();
        let ask = Ask {
            side: place.side,
            share: place.share(),
        };
        let landing = match dest {
            Dest::Anchor | Dest::Tab(_) => into
                .container()
                .plan_agent_pane(beside.id(), incoming, &ask)
                .ok_or("the anchor is not a pane of its tab")?,
            // A tab or a window of its own has no neighbour to share a room with.
            Dest::Lodged(..) | Dest::Window => Landing {
                spot: Spot::NewTab,
                landed: Landed::AsAsked,
                squeezed: false,
                hidden: false,
            },
        };
        // A terminal a pane is about to show is no longer one that was asked to end. For this
        // Mac the ledger is told which pane the terminal is for before the pane exists, so that
        // the pane finds it when it starts; one that is not started yet is made under its name.
        if let Some(uuid) = &uuid {
            if place.fresh.is_some() {
                hold_service::expect_name(uuid, terminal.clone());
            } else {
                hold_service::register_local(uuid, &terminal);
            }
        } else {
            hold_service::claim(&target.account, &terminal);
        }
        // `--focus` was decided by [`World::place`]: the keyboard goes to the new pane only if the
        // person is where the pane goes.
        let focus = place.focus;
        let label = place.home.label.as_deref();
        let (at_tab, at_group, at_window) = match (landing.spot, dest) {
            (Spot::Beside { tree, landing: at }, _) => {
                let (config, _) = app.pane_launch_as(
                    into.id(),
                    Some(anchor),
                    Opening::Split,
                    Some(&start),
                    incoming,
                );
                let pane = into.add_planned_pane(mtm, config, tree, at, false)?;
                if focus {
                    window.show_pane(into.id(), pane.id());
                }
                standing(&window, &into)
            }
            (Spot::NewTab, Dest::Window) => {
                let from = tab.window();
                let made =
                    app.open_window_for(from.as_deref(), anchor, &start, incoming, focus && raise)?;
                let born = made.selected_tab();
                standing(&made, &born)
            }
            (Spot::NewTab, dest) => {
                let (window, lodging) = match dest {
                    Dest::Lodged(window, lodging) => (window.clone(), lodging.clone()),
                    _ => (window, Lodging::Beside(into.id())),
                };
                let tab_id = app.next_id();
                let (config, theme) =
                    app.pane_launch_as(tab_id, Some(anchor), Opening::Tab, Some(&start), incoming);
                let pane = embed::open(mtm, initial_rect(), config).map_err(|e| e.to_string())?;
                let born = TerminalTab::new(mtm, tab_id, window.id(), &pane, None);
                window.add_tab_lodged(&born, (theme, app.settings().content_edge), &lodging);
                pane.observe_frame();
                if let Err(e) = born.start_hidden(mtm) {
                    window.close_tab_now(tab_id);
                    return Err(format!("could not start the shell: {e}"));
                }
                if let Some(label) = label {
                    window.rename_tab(tab_id, label);
                }
                window.refresh_title();
                if focus {
                    window.show_pane(tab_id, pane.id());
                }
                standing(&window, &born)
            }
        };
        Ok(Placed {
            landed: landing.landed,
            next_to: Some(anchor_name.clone()),
            squeezed: landing.squeezed,
            hidden: landing.hidden,
            tab: at_tab,
            group: at_group,
            window: at_window,
            keyboard_stayed: false,
        })
    }
}

/// Where `tab` stands, by the names a command takes: the tab, its group (none when it is in no
/// group) and its window.
fn standing(window: &TerminalWindow, tab: &TerminalTab) -> (Option<Id>, Option<Id>, Option<Id>) {
    let groups = window.groups();
    let group = groups
        .group_of(tab.id())
        .and_then(|group| groups.group(group))
        .and_then(|group| group.number);
    (
        Id::new(IdKind::Tab, tab.number()),
        group.and_then(|number| Id::new(IdKind::Group, number)),
        Id::new(IdKind::Window, window.number()),
    )
}

/// What `pane` shows of the account `account`: a terminal a server keeps, or — for this Mac's
/// account — the terminal of this Mac that carries the pane ([`hold_service::shown_local`]). The
/// service's own [`hold_service::shown`] is a server's alone.
fn shown_in(pane: &TerminalPane, account: &str) -> Option<Shown> {
    if local::is_account(account) {
        return hold_service::shown_local(pane.uuid());
    }
    hold_service::shown(pane.uuid()).filter(|shown| shown.account == account)
}

/// A pane of this Mac found by the terminal it shows: its tab, the pane, and what the service
/// says it shows.
type Found = (Retained<TerminalTab>, Retained<TerminalPane>, Shown);

/// The pane of this Mac that shows `terminal` of the server `account` (for this Mac's account,
/// the pane the terminal carries), with its tab and what the service says it shows. Any window,
/// any tab, on screen or not.
fn find_pane(app: &AppDelegate, account: &str, terminal: &str) -> Option<Found> {
    for tab in app.tabs() {
        for pane in tab.panes() {
            if let Some(shown) = shown_in(&pane, account)
                && shown.terminal.as_str() == terminal
            {
                return Some((tab, pane, shown));
            }
        }
    }
    None
}

/// The pane that shows the terminal `address` names — a terminal of the machine that asked, or
/// of another written with its machine (`p9@prod`, this Mac `p9@local`), as `ls` prints them. A
/// terminal of another machine is a pane the level lets the machine change (`screen` and up: it
/// is not its own), found among the panes on screen by the host the pane shows
/// ([`bt_core::bare_host`]); one name that two accounts of a host both have is a mistake the
/// answer says, not a guess.
fn find_address(
    app: &AppDelegate,
    server: &Server,
    address: &Anchor,
) -> Result<Option<Found>, Failure> {
    let own_machine = match address.machine() {
        None => true,
        Some(machine) => {
            if local::is_account(&server.account) {
                machine == LOCAL_MACHINE
            } else {
                server
                    .host_name()
                    .is_some_and(|host| bt_core::bare_host(host) == machine)
            }
        }
    };
    let machine = address.machine().unwrap_or_default();
    if own_machine {
        // A name with the machine written out is the machine's, not an account's: another account
        // on the same host may have a terminal of that name too, and which is meant is not told.
        if address.machine().is_some() && ambiguous(&panes_named(app, machine, address.name())) {
            return Err(two_accounts(address, machine));
        }
        return Ok(find_pane(app, &server.account, address.name()));
    }
    arrange::may_change(server, Panes::other(1), &format!("pane {address}"))?;
    let mut found = panes_named(app, machine, address.name());
    if ambiguous(&found) {
        return Err(two_accounts(address, machine));
    }
    Ok(found.drain(..).next())
}

/// The panes of this Mac that show the terminal `name` of `machine` (a host as `ls` writes it, or
/// this Mac): one for each account that has one.
fn panes_named(app: &AppDelegate, machine: &str, name: &str) -> Vec<Found> {
    let mut found = Vec::new();
    for tab in app.tabs() {
        for pane in tab.panes() {
            let shown = if machine == LOCAL_MACHINE {
                hold_service::shown_local(pane.uuid())
            } else {
                hold_service::shown(pane.uuid())
                    .filter(|shown| bt_core::bare_host(&shown.host) == machine)
            };
            if let Some(shown) = shown
                && shown.terminal.as_str() == name
            {
                found.push((tab.clone(), pane, shown));
            }
        }
    }
    found
}

/// Whether the panes show terminals of more than one account.
fn ambiguous(found: &[Found]) -> bool {
    let first = found.first().map(|(_, _, shown)| &shown.account);
    found
        .iter()
        .any(|(_, _, shown)| Some(&shown.account) != first)
}

fn two_accounts(address: &Anchor, machine: &str) -> Failure {
    Failure::new(
        Kind::Usage,
        format!("{address} is the name of terminals of two accounts on {machine}; none is meant"),
    )
}

/// The server and the terminal a `close` is about, once the machine in its address is told: the
/// server whose account has the pane, and the terminal's name there. The level is the one of the
/// machine that asked, which [`find_address`] has judged for the address.
fn close_target(
    app: &AppDelegate,
    server: &Server,
    target: Anchor,
) -> Result<(Server, Anchor), Failure> {
    if target.machine().is_none() {
        return Ok((server.clone(), target));
    }
    let Some((_, _, shown)) = find_address(app, server, &target)? else {
        return Err(Failure::new(
            Kind::Unknown,
            format!("{target} is no pane on screen"),
        ));
    };
    let named = Anchor::parse(target.name())
        .ok_or_else(|| Failure::new(Kind::Failed, "the terminal's name cannot be read"))?;
    let closing = Server {
        account: shown.account,
        host: shown.host,
        argv: shown.argv,
        level: server.level,
    };
    Ok((closing, named))
}

impl Shell for World<'_> {
    fn shows(&self, server: &Server, terminal: Terminal) -> bool {
        let just_made = state(|state| {
            state.made.retain(|(_, _, at)| at.elapsed() < MADE_FOR);
            state
                .made
                .iter()
                .any(|(account, made, _)| *account == server.account && *made == terminal)
        });
        just_made || find_pane(self.app, &server.account, &terminal.to_string()).is_some()
    }

    fn place(
        &mut self,
        server: &Server,
        anchor: &Anchor,
        place: &Place,
        opener: Option<&str>,
    ) -> Placing {
        self.place_on(server, server, anchor, place, opener)
    }

    fn place_on(
        &mut self,
        server: &Server,
        target: &Server,
        anchor: &Anchor,
        place: &Place,
        opener: Option<&str>,
    ) -> Placing {
        let Some((tab, pane, shown)) = find_pane(self.app, &server.account, anchor.name()) else {
            // A server's anchor may come later (a pane shows it, any way); this Mac's terminals
            // are its panes' and nothing else shows one.
            if local::is_account(&server.account) {
                return Placing::Refused(Failure::new(
                    Kind::Unknown,
                    format!("no pane of this Mac shows {anchor} to go beside"),
                ));
            }
            return Placing::NoAnchor;
        };
        // The terminal that asked; a note that was read later does not say, and the pane is said
        // to come from the terminal it stands beside.
        let asker = opener.unwrap_or(anchor.name());
        let program = find_pane(self.app, &server.account, asker)
            .and_then(|(_, _, found)| found.program)
            .or_else(|| shown.program.clone());
        let who = summary::opener(program.as_deref(), asker);
        // Where it goes: a place that is not there, or that the level keeps from this machine, is
        // a refusal and nothing is made.
        let dest = match self.whither(server, &tab, place) {
            Ok(dest) => dest,
            Err(failure) => return Placing::Refused(failure),
        };
        // `--focus` is the keyboard moving, so it passes the rule every such command passes
        // ([`bt_shell_common::agent::arrange`]): the cursor is in the pane that asked, or bateri is
        // behind, or the pane goes to another window than the one being typed in. Otherwise the
        // pane is made all the same and the answer says the keyboard stayed. A window made for
        // the pane is nobody's yet (no window has number 0): it takes the keyboard only from the
        // pane that has it.
        let go = place.focus.then(|| {
            let caller = opener.and_then(|name| arrange::spot(self.app, server, name));
            let keyboard = arrange::keyboard(self.app, caller.as_ref());
            let target = match &dest {
                Dest::Anchor => tab.window(),
                Dest::Tab(target) => target.window(),
                Dest::Lodged(window, _) => Some(window.clone()),
                Dest::Window => None,
            };
            rule::go(keyboard, target.map_or(0, |window| window.number()))
        });
        let stayed = go == Some(Go::Refused);
        let raise = go == Some(Go::Raise);
        let place = &Place {
            focus: place.focus && !stayed,
            ..place.clone()
        };
        let by = Beside {
            tab: &tab,
            pane: &pane,
            name: anchor,
        };
        match self.put(target, &by, place, who, (&dest, raise)) {
            Ok(mut placed) => {
                placed.keyboard_stayed = stayed;
                state(|state| {
                    state
                        .made
                        .push((target.account.clone(), place.terminal, Instant::now()));
                });
                self.opened.push(Opened {
                    account: target.account.clone(),
                    host: target.host.clone(),
                    program: shown.program,
                });
                Placing::Done(placed)
            }
            Err(why) => {
                eprintln!(
                    "bateri: no pane for {} on {}: {why}",
                    place.terminal, target.host
                );
                Placing::Unable
            }
        }
    }
}

// ─── ls ──────────────────────────────────────────────────────────────────

/// `bateri ls`: the server is asked for its terminals now, and when they come the windows are read
/// on the main queue, so the two are of the same moment.
fn list(letter: Letter, server: Server, from: Option<String>, format: Format) {
    let account = server.account.clone();
    hold_service::survey(&account, move |survey| {
        DispatchQueue::main().exec_async(move || {
            // audit: a block running on the main queue is on the main thread by definition.
            let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
            match app::delegate(mtm) {
                Some(app) => listed(&app, letter, &server, from.as_deref(), format, survey),
                None => letter.hang_up(),
            }
        });
    });
}

fn listed(
    app: &AppDelegate,
    letter: Letter,
    server: &Server,
    from: Option<&str>,
    format: Format,
    survey: Survey,
) {
    let Some(statuses) = survey.terminals else {
        refuse(
            letter,
            Kind::Failed,
            "the server did not give its terminals in time",
        );
        return;
    };
    let (windows, typing) = windows_of(app, &server.account, server.level);
    let rows = state(|state| {
        listing::rows(statuses, &survey.records, &|terminal| {
            state.waiting.waits(&server.account, terminal)
        })
    });
    let scene = Scene {
        caller: from.map(str::to_owned),
        typing,
        windows: Some(windows),
        terminals: rows,
    };
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    let mut text = match format {
        Format::Text => scene.text(now),
        Format::Json => scene.json(),
    };
    if !text.ends_with('\n') {
        text.push('\n');
    }
    letter.answer(Reply::Text(text).encode().into_bytes());
}

/// Answers a request with a mistake or a refusal.
fn refuse(letter: Letter, kind: Kind, text: &str) {
    letter.answer(
        Reply::Failed(Failure::new(kind, text))
            .encode()
            .into_bytes(),
    );
}

/// Our windows front to back (`windowNumbersWithOptions:`, every space), those the system does
/// not list after them in the order the application keeps them.
fn windows_front_to_back(app: &AppDelegate) -> Vec<Retained<TerminalWindow>> {
    let mut rest = app.windows();
    let mut ordered = Vec::with_capacity(rest.len());
    if let Some(numbers) =
        NSWindow::windowNumbersWithOptions(NSWindowNumberListOptions::AllSpaces, app.mtm())
    {
        for number in numbers.iter() {
            if let Some(at) = rest
                .iter()
                .position(|window| window.ns_window().windowNumber() == number.integerValue())
            {
                ordered.push(rest.remove(at));
            }
        }
    }
    ordered.extend(rest);
    ordered
}

/// The terminal of the server `account` that `pane` shows, by its name there.
fn terminal_of(pane: &TerminalPane, account: &str) -> Option<String> {
    shown_in(pane, account).map(|shown| shown.terminal.as_str().to_owned())
}

/// How `ls` tells `pane` to the server `account` at `level`: a terminal of that server by its
/// name there; a pane of another machine by that machine's name when the level sees every
/// machine ([`level::shown`]); else `another machine` and nothing more.
fn pane_of(pane: &TerminalPane, account: &str, level: CommandLevel) -> Pane {
    if let Some(name) = terminal_of(pane, account) {
        return Pane::Terminal(name);
    }
    if !level::shown(level, Panes::other(1)) {
        return Pane::Elsewhere;
    }
    // A pane that shows a terminal of another server is told by that server; any other pane
    // (a shell of this Mac) is this Mac's, by the number its terminal has when it has one
    // (`p3@local`) and by nothing else when it has not.
    match hold_service::shown(pane.uuid()) {
        Some(shown) => Pane::Remote {
            machine: bt_core::bare_host(&shown.host).to_owned(),
            name: Some(shown.terminal.as_str().to_owned()),
        },
        None => Pane::Remote {
            machine: LOCAL_MACHINE.to_owned(),
            name: hold_service::shown_local(pane.uuid())
                .map(|shown| shown.terminal.as_str().to_owned())
                .filter(|name| Terminal::parse(name).is_some()),
        },
    }
}

/// The windows as `ls` tells them for the server `account`, and the terminal of it the user is
/// typing in, if one. A pane of anything else is `another machine` and nothing more.
fn windows_of(
    app: &AppDelegate,
    account: &str,
    level: CommandLevel,
) -> (Vec<Window>, Option<String>) {
    let mut typing = None;
    let mut windows = Vec::new();
    for window in windows_front_to_back(app) {
        // ⌘1… reach the key window's tabs alone.
        let keys = if window.ns_window().isKeyWindow() {
            window.tab_keys()
        } else {
            Vec::new()
        };
        let groups = window.groups();
        let mut tabs = Vec::new();
        for tab in window.tabs() {
            let panes = tab.panes();
            let named = |id: u64| -> Pane {
                panes
                    .iter()
                    .find(|pane| pane.id() == id)
                    .map_or(Pane::Elsewhere, |pane| pane_of(pane, account, level))
            };
            let focused = tab.focused_pane();
            let own = terminal_of(&focused, account);
            if typing.is_none() && focused.is_active() {
                typing.clone_from(&own);
            }
            tabs.push(Tab {
                id: tab.number(),
                group: groups
                    .group_of(tab.id())
                    .and_then(|group| groups.group(group))
                    .and_then(|group| group.number),
                keys: keys
                    .iter()
                    .find(|(id, _)| *id == tab.id())
                    .map(|(_, hint)| hint.clone()),
                // The name it was given, and apart from it the title of a terminal of this server.
                label: tab.name(),
                title: own.as_ref().and_then(|_| tab.automatic_title()),
                selected: window.is_selected(tab.id()),
                tree: Node::of(&tab.container().tree(), &named),
                zoomed: tab.zoomed().map(named),
            });
        }
        windows.push(Window {
            id: window.number(),
            visible: window.window_visible(),
            groups: groups
                .groups()
                .iter()
                .filter_map(|group| {
                    Some(listing::Group {
                        id: group.number?,
                        label: group.name.clone(),
                    })
                })
                .collect(),
            tabs,
        });
    }
    (windows, typing)
}

// ─── close ───────────────────────────────────────────────────────────────

/// `bateri close`: the server says what it has, then the terminal is closed. Nobody is asked:
/// the level of the server's machine decided before this ([`serve::serve`]) and a terminal of the
/// server is that machine's own, so `own` and above close it, whoever opened it.
fn close(letter: Letter, server: Server, target: Anchor) {
    let account = server.account.clone();
    hold_service::survey(&account, move |survey| {
        DispatchQueue::main().exec_async(move || {
            // audit: a block running on the main queue is on the main thread by definition.
            let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
            match app::delegate(mtm) {
                Some(app) => closing(&app, letter, server, target, survey),
                None => letter.hang_up(),
            }
        });
    });
}

fn closing(app: &AppDelegate, letter: Letter, server: Server, target: Anchor, survey: Survey) {
    let Some(statuses) = survey.terminals else {
        refuse(letter, Kind::Failed, "the server did not answer");
        return;
    };
    let known = statuses
        .iter()
        .any(|status| status.id.as_str() == target.name());
    if !known && find_pane(app, &server.account, target.name()).is_none() {
        refuse(
            letter,
            Kind::Unknown,
            &format!("{target} is no pane on this server"),
        );
        return;
    }
    letter.answer(Reply::Done.encode().into_bytes());
    carry_out_close(app, &server, &target);
}

/// The terminal ends. With a pane that shows it that is the user closing the pane — the pane
/// goes, the server's terminal ends with it; with none it is the card's End.
fn carry_out_close(app: &AppDelegate, server: &Server, target: &Anchor) {
    if let Some(terminal) = target.terminal() {
        state(|state| state.waiting.forget(&server.account, terminal));
    }
    match find_pane(app, &server.account, target.name()) {
        Some((tab, pane, _)) => tab.close_pane(pane.id()),
        None => {
            if let Ok(terminal) = SessionId::new(target.name()) {
                hold_service::end_terminal(&server.account, &terminal);
            }
        }
    }
}

// ─── notify ──────────────────────────────────────────────────────────────

/// The longest a notification's words are, in characters.
const NOTICE_LIMIT: usize = 1000;

/// `bateri notify`, told as every program that asks to be noticed is ([`notice`]). Answered at
/// once: the words are delivered, what the system does with them is its own.
fn notify(app: &AppDelegate, letter: Letter, server: &Server, from: Option<&str>, text: &str) {
    let found = from.and_then(|name| find_pane(app, &server.account, name));
    let program = found
        .as_ref()
        .and_then(|(_, _, shown)| shown.program.as_deref());
    let title = summary::notified(program, &server.host);
    let body: String = text
        .chars()
        .filter(|c| !c.is_control())
        .take(NOTICE_LIMIT)
        .collect();
    let at = found.as_ref().map(|(tab, pane, _)| (&**tab, pane.id()));
    notice(app.mtm(), at, &title, &body, None);
    letter.answer(Reply::Done.encode().into_bytes());
}

/// How a program that asks to be noticed is told — a server's `bateri notify` and a terminal's
/// `OSC 9` / `OSC 777` alike. The tab it runs in (`at`: the tab and the pane) is marked as having
/// news unless the user is looking at it. With bateri behind, the words come as a notification;
/// one with a `replacing` name takes the place of the last one of that name. In front, the tab is
/// what shows it — and when no tab of this Mac shows the program, the notification is shown in
/// front instead, the one kind of notification that is
/// ([`uploader::deliver_notification_in_front`]).
pub(crate) fn notice(
    mtm: MainThreadMarker,
    at: Option<(&TerminalTab, u64)>,
    title: &str,
    body: &str,
    replacing: Option<&str>,
) {
    if let Some((tab, pane)) = at {
        tab.news_from(pane);
    }
    if !NSApplication::sharedApplication(mtm).isActive() {
        match replacing {
            Some(name) => uploader::deliver_notification_replacing(name, title, body),
            None => uploader::deliver_notification(title, body),
        }
    } else if at.is_none() {
        // The system silences a notification delivered while bateri is in front and the tab is
        // what shows it there; with no tab of the asker here this one is shown in front.
        uploader::deliver_notification_in_front(mtm, title, body);
    }
}

// ─── Telling the user ────────────────────────────────────────────────────

/// `count` more terminals were opened for the user on a server. In front, they are on the tab
/// strip and nothing more is said; behind, a notification says how many were opened since the
/// user last looked — the same notification, replaced, as more come.
fn tell(mtm: MainThreadMarker, account: &str, host: &str, program: Option<&str>, count: u32) {
    if NSApplication::sharedApplication(mtm).isActive() {
        state(|state| state.told.remove(account));
        return;
    }
    let total = state(|state| {
        let told = state.told.entry(account.to_owned()).or_insert(0);
        *told += count;
        *told
    });
    uploader::deliver_notification_replacing(
        &format!("bateri.agent.{account}"),
        &summary::opened(program, host, total),
        summary::found_in(total),
    );
}

#[cfg(test)]
mod tests {
    /// A place the request names is looked up and judged by the level before anything is made, a
    /// place that is not there is a refusal and not a pane in the wrong tab, and a window made for
    /// a pane takes the keyboard only where the rule gives it. A source scan: the windows are
    /// AppKit's and need a window server.
    /// What a request runs reaches the pane by one word (`Place::runs`), and the pane's integration
    /// and first command follow from it (`launch::integration_for`, `PaneRuns::shell`), each tested
    /// where it is written: only the joining is left to read here, since it needs a window.
    #[test]
    fn what_a_request_runs_reaches_the_pane_it_opens() {
        let source = include_str!("agent.rs");
        let source = &source[..source.find("#[cfg(test)]").unwrap_or(source.len())];
        assert!(source.contains("runs: place.runs(),"));
        let app = include_str!("app.rs");
        assert!(
            app.contains(
                "launch::integration_for(&runs, |shell| self.shell_integration_of(shell))"
            )
        );
        assert!(
            include_str!("pane.rs").contains("child::shell_command_for(asked_shell, &shell_args)")
        );
    }

    #[test]
    fn a_place_is_judged_before_the_pane_is_made_and_a_new_window_waits_for_the_rule() {
        let source = include_str!("agent.rs");
        let source = &source[..source.find("#[cfg(test)]").unwrap_or(source.len())];
        let body = |signature: &str| {
            let at = source.find(signature).expect(signature);
            &source[at..at + source[at..].find("\n    }\n").expect("its end")]
        };
        let place = body("    fn place_on(");
        let judged = place.find("self.whither(").expect(place);
        let refused = place
            .find("Err(failure) => return Placing::Refused(failure)")
            .expect(place);
        let made = place.find("self.put(").expect(place);
        assert!(judged < refused && refused < made, "{place}");
        let whither = body("    fn whither(");
        // The tab, the group and the window a pane goes into are each changed by it.
        assert_eq!(
            whither.matches("arrange::may_change(").count(),
            3,
            "{whither}"
        );
        let put = body("    fn put(");
        assert!(put.contains("add_tab_lodged("), "{put}");
        assert!(put.contains("open_window_for("), "{put}");
        assert!(put.contains("focus && raise"), "{put}");
    }

    /// What a program asks of a server (`-S`) waits for a program (`ssh -G`) and for a person's
    /// password, so it never runs on the main queue: the backend is asked on a thread of its own,
    /// the letter is answered from there, and the pane is made on the main queue afterwards. The
    /// pane of such a terminal is the pipe to the server's terminal (the target's `Held`), beside
    /// a terminal of the machine that asked (the sender's level judges where). A source scan.
    #[test]
    fn a_program_reaching_a_server_never_waits_on_the_main_queue() {
        let reach = include_str!("agent/reach.rs");
        let reach = &reach[..reach.find("#[cfg(test)]").unwrap_or(reach.len())];
        let body = |signature: &str| {
            let at = reach.find(signature).expect(signature);
            &reach[at..at + reach[at..].find("\n}\n").expect("its end")]
        };
        let ssh = body("pub(super) fn ssh(");
        let thread = ssh.find("thread::Builder::new()").expect(ssh);
        let reached = ssh.find("backend.reach(").expect(ssh);
        assert!(
            thread < reached,
            "the connection is made on a thread: {ssh}"
        );
        // A relayed command runs a program, perhaps for as long as a job: on a thread of its own,
        // and the letter is answered from there.
        let relay = body("pub(super) fn relay(");
        let thread = relay.find("thread::Builder::new()").expect(relay);
        assert!(!relay[..thread].contains("run::execute("), "{relay}");
        assert!(relay[thread..].contains("letter.answer("), "{relay}");
        let relayed = body("fn relayed(");
        assert!(relayed.contains("run::execute("), "{relayed}");
        let place = body("pub(super) fn place_on(");
        let thread = place.find("thread::Builder::new()").expect(place);
        let route = place.find("backend.route(").expect(place);
        let main = place.find("DispatchQueue::main()").expect(place);
        assert!(
            thread < route && route < main,
            "the server is found on a thread, the pane made on the main queue: {place}"
        );
        // The target's account and arguments are the pane's pipe; the sender's level judges it.
        let source = include_str!("agent.rs");
        let source = &source[..source.find("#[cfg(test)]").unwrap_or(source.len())];
        let at = source.find("    fn put(").expect("put");
        let put = &source[at..at + source[at..].find("\n    }\n").expect("its end")];
        let put: String = put.split_whitespace().collect();
        assert!(put.contains("target.argv"), "{put}");
        assert!(put.contains("account:target.account.clone()"), "{put}");
    }

    /// A pane of this Mac is an ordinary pane born with the identity its terminal was written in
    /// the ledger for (`register_local`), or made under the number it was told (`expect_name`);
    /// it holds no pipe to a server (no `Held`, no `ssh-hold`), and an anchor that no pane shows
    /// is refused and not kept waiting. A source scan, like the one above.
    #[test]
    fn a_pane_of_this_mac_is_registered_before_it_exists_and_holds_no_pipe() {
        let source = include_str!("agent.rs");
        let source = &source[..source.find("#[cfg(test)]").unwrap_or(source.len())];
        let body = |signature: &str| {
            let at = source.find(signature).expect(signature);
            &source[at..at + source[at..].find("\n    }\n").expect("its end")]
        };
        let put = body("    fn put(");
        let local = put.find("local::is_account(&target.account)").expect(put);
        let pipe = put.find("pipe_installed(").expect(put);
        assert!(
            local < pipe,
            "the server's pipe is not looked at for this Mac: {put}"
        );
        assert!(put.contains("hold_service::register_local("), "{put}");
        assert!(put.contains("hold_service::expect_name("), "{put}");
        let register = put.find("hold_service::register_local(").expect(put);
        let launch = put.find("app.pane_launch_as(").expect(put);
        assert!(register < launch, "the ledger first, the pane after: {put}");
        let place = body("    fn place_on(");
        assert!(
            place.contains("if local::is_account(&server.account)")
                && place.contains("Kind::Unknown"),
            "{place}"
        );
    }
}
