//! What a program on a server asks of the app, and the panes that come of it.
//!
//! `bateri split` on a server starts a terminal there (kablo's, numbered `t9`) and asks the Mac
//! where its pane goes, over the conversation kablo gives the app ([`Letter`]). The decision is
//! `bt-shell-common`'s ([`bt_shell_common::agent::serve`], with the rule of where a pane lands
//! in [`bt_shell_common::agent::place`]); this module lends it the windows ([`Shell`]) and
//! puts the pane where it says. **Nothing here runs a program or writes a byte to a shell**: the
//! pane is the pipe to a terminal that already exists on the server, asked for by name — the
//! request is a *place*, and an app that is asked cannot be made to run anything by it.
//!
//! - **The user's screen does not change.** The pane is born without the keyboard (`--focus`
//!   gives it), in a zoomed tab it is born behind the zoom, and a tab is added beside the
//!   anchor's tab without being selected ([`TerminalWindow::add_tab_behind`]): nothing the
//!   user was looking at moves, and the tab strip shows the new one running.
//! - **The program is the point of the pane.** Its pipe closes with its terminal
//!   (`ssh-hold --close-at-end`, [`Held::close_at_end`]) so the pane goes when the program does —
//!   unless `--keep-open` was asked, when the shell follows as it does for any pane. The pane
//!   keeps that, and who asked for it ([`Held::opener`], its card's "Opened by"), across a
//!   restart: session restore saves both with the terminal it shows.
//! - **A pane that cannot be placed waits, it is never refused.** The terminal runs on the
//!   server and is in the list of what runs in the background; a request whose anchor is on
//!   no tab of this Mac is kept here ([`Queue`]) until a pane shows the anchor — a pane opened
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
//! - **`close` asks about what is not the asker's.** A program closes without a question the
//!   terminal it runs in and the ones it opened ([`may_close`]); anything else is the user's to
//!   allow, in a sheet on the pane that shows it (or the asker's, or a window), and a "no" is
//!   the script's 77. Closing is what the user closing the pane does — the pane goes and the
//!   server's terminal ends with it — or, for a terminal with no pane, the card's End.
//! - **`notify` is one notification and a tick.** The notification only when bateri is behind;
//!   the tab the program runs in is marked as having news unless it is the one on screen.
//!
//! Everything here runs on the main thread: the service's thread hands a letter over by hopping
//! to the main queue ([`letter`], [`notes`]), and the state that outlives a request ([`STATE`])
//! is read and written there only.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use block2::RcBlock;
use bt_core::PaneUuid;
use bt_shell_common::agent::listing::{self, Node, Pane, Scene, Tab, Window};
use bt_shell_common::agent::message::{Failure, Format, Kind, Place, Placed, Reply};
use bt_shell_common::agent::notes::Note;
use bt_shell_common::agent::place::{Ask, Spot};
use bt_shell_common::agent::serve::{self, Answer, Placing, Server, Shell, may_close};
use bt_shell_common::agent::summary;
use bt_shell_common::agent::wait::{Queue, Waiting};
use bt_shell_common::agent::{Anchor, Terminal};
use bt_shell_common::kablo::SessionId;
use bt_shell_common::kablo::service::{Letter, Shown, Survey};
use dispatch2::DispatchQueue;
use objc2::rc::Retained;
use objc2::{MainThreadMarker, MainThreadOnly, Message};
use objc2_app_kit::{
    NSAlert, NSAlertSecondButtonReturn, NSApplication, NSModalResponse, NSWindow,
    NSWindowNumberListOptions,
};
use objc2_foundation::{NSString, ns_string};

use crate::app::{self, AppDelegate, Opening, Start, pipe_installed};
use crate::embed;
use crate::hold_service;
use crate::launch::{Held, Resume};
use crate::pane::TerminalPane;
use crate::sheets::{self, Asker};
use crate::tab::TerminalTab;
use crate::uploader::{self, ESCAPE, add_key_monitor, remove_monitor};
use crate::window::{TerminalWindow, initial_rect};

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
    for waiting in due {
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

fn take(app: &AppDelegate, mut letter: Letter) {
    let server = Server {
        account: letter.account.clone(),
        host: letter.host.clone(),
        argv: letter.argv.clone(),
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
            state(|state| state.waiting.add(waiting));
            letter.answer(reply.encode().into_bytes());
        }
        Answer::List(format) => list(letter, server, from, format),
        Answer::Close(target) => close(letter, server, from, target),
        Answer::Notify(text) => notify(app, letter, &server, from.as_deref(), &text),
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

    /// Makes the pane; `opener` is who asked for it, in the words its tab's card says them. `Err`
    /// is "not now": the terminal stays in the list.
    fn put(
        &self,
        server: &Server,
        tab: &TerminalTab,
        anchor: &TerminalPane,
        anchor_name: &Anchor,
        place: &Place,
        opener: String,
    ) -> Result<Placed, String> {
        let app = self.app;
        let mtm = app.mtm();
        let window = tab.window().ok_or("the tab has no window")?;
        if !pipe_installed(&app.shell_integration().env) {
            return Err("there is no pipe to a server in this process".to_owned());
        }
        let args = server
            .argv
            .get(1..)
            .filter(|args| !args.is_empty())
            .ok_or("the server is not named by arguments")?
            .to_vec();
        let name = place.terminal.to_string();
        let terminal = SessionId::new(&name).map_err(|e| e.to_string())?;
        let start = Start {
            directory: None,
            line: None,
            run: false,
            held: Some(Held {
                args,
                resume: Some(Resume {
                    account: server.account.clone(),
                    terminal: name,
                }),
                close_at_end: !place.keep_open,
                opener: Some(opener),
            }),
        };
        let incoming = app.next_id();
        let ask = Ask {
            side: place.side,
            share: place.share(),
        };
        let landing = tab
            .container()
            .plan_agent_pane(anchor.id(), incoming, &ask)
            .ok_or("the anchor is not a pane of its tab")?;
        // A terminal a pane is about to show is no longer one that was asked to end.
        hold_service::claim(&server.account, &terminal);
        match landing.spot {
            Spot::Beside { tree, landing: at } => {
                let (config, _) = app.pane_launch_as(
                    tab.id(),
                    Some(anchor),
                    Opening::Split,
                    Some(&start),
                    incoming,
                );
                let pane = tab.add_planned_pane(mtm, config, tree, at, false)?;
                if place.focus {
                    window.show_pane(tab.id(), pane.id());
                }
            }
            Spot::NewTab => {
                let tab_id = app.next_id();
                let (config, theme) =
                    app.pane_launch_as(tab_id, Some(anchor), Opening::Tab, Some(&start), incoming);
                let pane = embed::open(mtm, initial_rect(), config).map_err(|e| e.to_string())?;
                let born = TerminalTab::new(mtm, tab_id, window.id(), &pane);
                window.add_tab_behind(&born, (theme, app.settings().content_edge), tab.id());
                pane.observe_frame();
                if let Err(e) = born.start_hidden(mtm) {
                    window.close_tab_now(tab_id);
                    return Err(format!("could not start the shell: {e}"));
                }
                window.refresh_title();
                if place.focus {
                    window.show_pane(tab_id, pane.id());
                }
            }
        }
        Ok(Placed {
            landed: landing.landed,
            next_to: Some(anchor_name.clone()),
            squeezed: landing.squeezed,
            hidden: landing.hidden,
        })
    }
}

/// The pane of this Mac that shows `terminal` of the server `account`, with its tab and what the
/// service says it shows. Any window, any tab, on screen or not.
fn find_pane(
    app: &AppDelegate,
    account: &str,
    terminal: &str,
) -> Option<(Retained<TerminalTab>, Retained<TerminalPane>, Shown)> {
    for tab in app.tabs() {
        for pane in tab.panes() {
            if let Some(shown) = hold_service::shown(pane.uuid())
                && shown.account == account
                && shown.terminal.as_str() == terminal
            {
                return Some((tab, pane, shown));
            }
        }
    }
    None
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
        let Some((tab, pane, shown)) = find_pane(self.app, &server.account, anchor.name()) else {
            return Placing::NoAnchor;
        };
        // The terminal that asked; a note that was read later does not say, and the pane is said
        // to come from the terminal it stands beside.
        let asker = opener.unwrap_or(anchor.name());
        let program = find_pane(self.app, &server.account, asker)
            .and_then(|(_, _, found)| found.program)
            .or_else(|| shown.program.clone());
        let who = summary::opener(program.as_deref(), asker);
        match self.put(server, &tab, &pane, anchor, place, who) {
            Ok(placed) => {
                state(|state| {
                    state
                        .made
                        .push((server.account.clone(), place.terminal, Instant::now()));
                });
                self.opened.push(Opened {
                    account: server.account.clone(),
                    host: server.host.clone(),
                    program: shown.program,
                });
                Placing::Done(placed)
            }
            Err(why) => {
                eprintln!(
                    "bateri: no pane for {} on {}: {why}",
                    place.terminal, server.host
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
    let (windows, typing) = windows_of(app, &server.account);
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
    hold_service::shown(pane.uuid())
        .filter(|shown| shown.account == account)
        .map(|shown| shown.terminal.as_str().to_owned())
}

/// The windows as `ls` tells them for the server `account`, and the terminal of it the user is
/// typing in, if one. A pane of anything else is `another machine` and nothing more.
fn windows_of(app: &AppDelegate, account: &str) -> (Vec<Window>, Option<String>) {
    let mut typing = None;
    let mut windows = Vec::new();
    for window in windows_front_to_back(app) {
        // ⌘1… reach the key window's tabs alone.
        let keys = if window.ns_window().isKeyWindow() {
            window.tab_keys()
        } else {
            Vec::new()
        };
        let mut tabs = Vec::new();
        for tab in window.tabs() {
            let panes = tab.panes();
            let named = |id: u64| -> Pane {
                panes
                    .iter()
                    .find(|pane| pane.id() == id)
                    .and_then(|pane| terminal_of(pane, account))
                    .map_or(Pane::Elsewhere, Pane::Terminal)
            };
            let focused = tab.focused_pane();
            let own = terminal_of(&focused, account);
            if typing.is_none() && focused.is_active() {
                typing.clone_from(&own);
            }
            tabs.push(Tab {
                keys: keys
                    .iter()
                    .find(|(id, _)| *id == tab.id())
                    .map(|(_, hint)| hint.clone()),
                // The user's name for it, else the title of a terminal of this server.
                name: tab
                    .name()
                    .or_else(|| own.as_ref().and_then(|_| tab.automatic_title())),
                selected: window.is_selected(tab.id()),
                tree: Node::of(&tab.container().tree(), &named),
                zoomed: tab.zoomed().map(named),
            });
        }
        windows.push(Window {
            visible: window.window_visible(),
            tabs,
        });
    }
    (windows, typing)
}

// ─── close ───────────────────────────────────────────────────────────────

/// `bateri close`: the server says what it has and who opened it, then the terminal is closed —
/// without a question when it is the asker's own or one it opened, with one when it is not.
fn close(letter: Letter, server: Server, from: Option<String>, target: Anchor) {
    let account = server.account.clone();
    hold_service::survey(&account, move |survey| {
        DispatchQueue::main().exec_async(move || {
            // audit: a block running on the main queue is on the main thread by definition.
            let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
            match app::delegate(mtm) {
                Some(app) => closing(&app, letter, server, from, target, survey),
                None => letter.hang_up(),
            }
        });
    });
}

fn closing(
    app: &AppDelegate,
    letter: Letter,
    server: Server,
    from: Option<String>,
    target: Anchor,
    survey: Survey,
) {
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
            &format!("{target} is no terminal on this server"),
        );
        return;
    }
    let opener = target
        .terminal()
        .and_then(|terminal| survey.records.iter().find(|(found, _)| *found == terminal))
        .and_then(|(_, record)| record.opener.clone());
    if may_close(from.as_deref(), &target, opener.as_ref()) {
        letter.answer(Reply::Done.encode().into_bytes());
        carry_out_close(app, &server, &target);
        return;
    }
    ask_to_close(app, letter, server, from, target);
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

/// The user is asked whether a program may close a terminal it did not open: a sheet on the pane
/// that shows the terminal, else on the pane the program runs in, else on a window. "Cancel" is
/// the default and Esc, and the sheet cannot be answered by a Return typed to the terminal.
fn ask_to_close(
    app: &AppDelegate,
    letter: Letter,
    server: Server,
    from: Option<String>,
    target: Anchor,
) {
    let mtm = app.mtm();
    let account = server.account.clone();
    let program = from
        .as_deref()
        .and_then(|name| find_pane(app, &account, name).and_then(|(_, _, shown)| shown.program));
    let pane = find_pane(app, &account, target.name())
        .or_else(|| {
            from.as_deref()
                .and_then(|name| find_pane(app, &account, name))
        })
        .map(|(_, pane, _)| pane);
    let window = windows_front_to_back(app)
        .into_iter()
        .find(|window| window.ns_window().isKeyWindow())
        .or_else(|| windows_front_to_back(app).into_iter().next());
    // The seat can be asked for again when the Esc monitor needs it: a monitor that outlived
    // its sheet must not pin the pane or the window.
    let reseat: Box<dyn Fn(MainThreadMarker) -> Option<sheets::Seat>> = match (&pane, &window) {
        (Some(pane), _) => {
            let (id, lookup) = (pane.id(), pane.lookup());
            Box::new(move |mtm| lookup(mtm, id).and_then(|pane| sheets::seat(Asker::Pane(&pane))))
        }
        (None, Some(window)) => {
            let window = window.ns_window().retain();
            Box::new(move |_| sheets::seat(Asker::Window(&window)))
        }
        (None, None) => {
            refuse(letter, Kind::Refused, "there is no window to ask in");
            return;
        }
    };
    let Some(seat) = reseat(mtm) else {
        refuse(letter, Kind::Refused, "there is no window to ask in");
        return;
    };
    if seat.is_taken() {
        refuse(
            letter,
            Kind::Failed,
            "the user is answering another question in bateri; ask again",
        );
        return;
    }
    let (text, under) = summary::close_question(
        program.as_deref(),
        from.as_deref(),
        target.name(),
        &server.host,
    );
    let alert = NSAlert::new(mtm);
    alert.setMessageText(&NSString::from_str(&text));
    alert.setInformativeText(&NSString::from_str(&under));
    alert.addButtonWithTitle(ns_string!("Cancel"));
    alert.addButtonWithTitle(ns_string!("Close"));
    let monitor: Rc<Cell<Option<Retained<objc2::runtime::AnyObject>>>> = Rc::default();
    let sheet_window = alert.window();
    let installed = add_key_monitor(move |event| {
        if event.keyCode() != ESCAPE {
            return false;
        }
        // audit: the local event monitor runs on the main thread.
        let mtm = MainThreadMarker::new().expect("the event monitor is on the main thread");
        let on_sheet = event
            .window(mtm)
            .is_some_and(|window| Retained::as_ptr(&window) == Retained::as_ptr(&sheet_window));
        if on_sheet && let Some(seat) = reseat(mtm) {
            seat.end(&sheet_window, objc2_app_kit::NSAlertFirstButtonReturn);
        }
        on_sheet
    });
    monitor.set(installed);
    let slot = Rc::clone(&monitor);
    let waiting = Cell::new(Some(letter));
    let answered = RcBlock::new(move |response: NSModalResponse| {
        remove_monitor(slot.take());
        let Some(letter) = waiting.take() else {
            return;
        };
        // audit: AppKit calls the sheet's completion on the main thread.
        let mtm = MainThreadMarker::new().expect("a sheet answers on the main thread");
        match app::delegate(mtm) {
            Some(app) if response == NSAlertSecondButtonReturn => {
                letter.answer(Reply::Done.encode().into_bytes());
                carry_out_close(&app, &server, &target);
            }
            Some(_) => refuse(letter, Kind::Refused, "the user said no"),
            None => letter.hang_up(),
        }
    });
    seat.begin(&alert, &answered);
}

// ─── notify ──────────────────────────────────────────────────────────────

/// The longest a notification's words are, in characters.
const NOTICE_LIMIT: usize = 1000;

/// `bateri notify`: a notification when bateri is behind, and the tab the program runs in is
/// marked as having news unless the user is looking at it. Answered at once: the words are
/// delivered, what the system does with them is its own — except that a program no tab of this
/// Mac shows, asking while bateri is in front, is told nothing could be shown.
fn notify(app: &AppDelegate, letter: Letter, server: &Server, from: Option<&str>, text: &str) {
    let found = from.and_then(|name| find_pane(app, &server.account, name));
    if let Some((tab, pane, _)) = &found {
        tab.news_from(pane.id());
    }
    if NSApplication::sharedApplication(app.mtm()).isActive() {
        // The system silences a notification delivered while bateri is in front, so the tab is
        // what shows it; with no tab of the asker here, nothing was shown and the program is told.
        if found.is_none() {
            refuse(
                letter,
                Kind::Failed,
                "bateri is in front and has no tab of this terminal to mark, so nothing was shown",
            );
            return;
        }
    } else {
        let program = found
            .as_ref()
            .and_then(|(_, _, shown)| shown.program.as_deref());
        let body: String = text
            .chars()
            .filter(|c| !c.is_control())
            .take(NOTICE_LIMIT)
            .collect();
        uploader::deliver_notification(&summary::notified(program, &server.host), &body);
    }
    letter.answer(Reply::Done.encode().into_bytes());
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
