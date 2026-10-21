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
//!   unless `--keep-open` was asked, when the shell follows as it does for any pane.
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
//!
//! Everything here runs on the main thread: the service's thread hands a letter over by hopping
//! to the main queue ([`letter`], [`notes`]), and the state that outlives a request ([`STATE`])
//! is read and written there only.

use std::cell::RefCell;
use std::collections::HashMap;
use std::time::{Duration, Instant};

use bt_core::PaneUuid;
use bt_shell_common::agent::message::{Failure, Kind, Place, Placed, Reply};
use bt_shell_common::agent::notes::Note;
use bt_shell_common::agent::place::{Ask, Spot};
use bt_shell_common::agent::serve::{self, Answer, Placing, Server, Shell};
use bt_shell_common::agent::summary;
use bt_shell_common::agent::wait::Queue;
use bt_shell_common::agent::{Anchor, Terminal};
use bt_shell_common::kablo::SessionId;
use bt_shell_common::kablo::service::{Letter, Shown};
use dispatch2::DispatchQueue;
use objc2::rc::Retained;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::NSApplication;

use crate::app::{self, AppDelegate, Opening, Start, pipe_installed};
use crate::embed;
use crate::hold_service;
use crate::launch::{Held, Resume};
use crate::pane::TerminalPane;
use crate::tab::TerminalTab;
use crate::uploader;
use crate::window::initial_rect;

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
pub(crate) fn terminal_shown(app: &AppDelegate, pane: &PaneUuid) {
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
    let mut world = World::new(app);
    for waiting in due {
        let account = waiting.server.account.clone();
        let (again, _) = serve::settle_waiting(
            waiting,
            &|terminal, keep_open| alive(&account, terminal, keep_open),
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
        Answer::Pass => letter.hang_up(),
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

    /// Makes the pane. `Err` is "not now": the terminal stays in the list.
    fn put(
        &self,
        server: &Server,
        tab: &TerminalTab,
        anchor: &TerminalPane,
        anchor_name: &Anchor,
        place: &Place,
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

    fn place(&mut self, server: &Server, anchor: &Anchor, place: &Place) -> Placing {
        let Some((tab, pane, shown)) = find_pane(self.app, &server.account, anchor.name()) else {
            return Placing::NoAnchor;
        };
        match self.put(server, &tab, &pane, anchor, place) {
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
