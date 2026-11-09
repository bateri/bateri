//! What the bateri app does with what a program on a server says to it: the decision, with no window in
//! it. The shell (`bt-shell-macos`) lends its windows through [`Shell`] and puts the panes where
//! this says; a test lends a pretend one.
//!
//! A conversation is one request (`message`) and one answer. This reads the request, checks what
//! is the bateri app's to check — the pane is for a terminal **of the server the letter came from**
//! (the letter arrives on that server's own connection, so nothing a request says can name another
//! server's terminal), and it goes beside a terminal that is named or, when none is, the one the
//! ticket says the request came from — and asks the shell to put it there ([`Shell::place`]).
//! Everything a pane could not be made for is a pane that **waits**, never a terminal that is
//! ended: the terminal is the server's and runs on, and the list of what runs in the background
//! (the server's card, ⌘P) is where a terminal with no pane is found.
//!
//! - The anchor is in no tab of this computer ([`Placing::NoAnchor`]): the answer says `landed waiting`
//!   and the request is kept in a [`Queue`](super::wait::Queue) until a pane shows the anchor.
//! - A pane cannot be made at all now ([`Placing::Unable`]: a bateri with no pipe to a server,
//!   a timed run): the answer says it waits and names no anchor, and nothing is kept — the
//!   terminal is in the list.
//! - The terminal already has a pane: the request is done ([`Landed::AsAsked`]), not an error
//!   — an error would end a terminal the user is looking at.
//!
//! **Another machine's terminal** (`-S`): the program starts it itself and asks for its pane with a
//! `place` that names the `server`. A program has no way to the other machine but the bateri app:
//! asked how to reach it, the app answers `relay`, and the program sends, in a `relay`, the
//! `bateri` command it means for the other machine — a server or, for a program on a server, this
//! computer (`@local`) — which the app runs over the connection it keeps and says how it ended
//! ([`Answer::Relay`], [`super::relay`]).
//! All of it is for a `full` machine alone ([`may_reach`], [`level::Act::Reach`]); the shell finds
//! the server, on a thread of its own, and [`place_on`] puts the pane beside a terminal of the
//! machine that asked while the pane itself is the pipe to the *target's* terminal.
//!
//! **This is where the contract's "the window writes no bytes to a shell and runs no program" is
//! relaxed, for `full` alone**: a relay is read like any request, then `off`, `own` and `screen` are
//! refused (77) before anything is run, and a relay runs only the commands of [`RELAYED`](super::message::RELAYED).
//!
//! The other requests need the bateri app's windows, a question to the user or the server's own
//! list, none of which a decision with no window in it has: they are an [`Answer`] of their own
//! (`List`, `Close`, `Notify`, `Message`) that the shell carries out — building `ls` from the live windows
//! and a [`survey`](crate::kablo::service::Service::survey), closing a terminal, showing a
//! notification — and answers when it has. **Nothing asks the user:** what a machine's commands
//! may do is its [`CommandLevel`] ([`super::level`]), decided here and by the shell from the
//! level the server carries, never in a sheet.

use bt_core::CommandLevel;
use kablo_proto::v1::{State, Status};

use super::level::{self, Act, Panes, Verdict, Why};
use super::message::{Arrange, Failure, Format, Kind, Place, Placed, Relay, Reply, Request};
use super::notes::Note;
use super::place::Landed;
use super::wait::Waiting;
use super::{Anchor, Terminal};

/// The server a letter, a note or a waiting request is about.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Server {
    /// The server's account.
    pub account: String,
    /// The server as the user typed it (`rasp`).
    pub host: String,
    /// The `ssh` arguments that reach it, the program's name first: what a pane that shows one of
    /// its terminals is started with.
    pub argv: Vec<String>,
    /// What the `bateri` command may do on it ([`Settings::commands_for`](bt_core::Settings::commands_for)
    /// at the moment of the request).
    pub level: CommandLevel,
}

impl Server {
    /// The name the user typed for the server, for a refusal to say; `None` for this computer, which
    /// has none (the setting that decides is `[commands] level`, not a host's entry).
    pub fn host_name(&self) -> Option<&str> {
        (!crate::kablo::local::is_account(&self.account) && !self.host.is_empty())
            .then_some(self.host.as_str())
    }
}

/// What the bateri app's windows are asked.
pub trait Shell {
    /// Whether a pane of this computer shows `terminal` of the server already.
    fn shows(&self, server: &Server, terminal: Terminal) -> bool;

    /// Shows `place.terminal` in a pane beside the pane of `anchor` — wherever on this computer that
    /// is — as `place` asks: where it goes is the rule's ([`super::place::place`]), the pane is
    /// born without the keyboard unless `place.focus` says so and the shell lets it
    /// ([`super::arrange::go`]: the cursor is in `opener`'s pane, or the person is not typing in
    /// bateri, or the pane is in another window than the one being typed in), and a zoomed tab
    /// keeps its zoom. `opener` is the terminal the request came from, when it is known (a note
    /// read later does not say, and the pane is said to be opened from beside its anchor).
    fn place(
        &mut self,
        server: &Server,
        anchor: &Anchor,
        place: &Place,
        opener: Option<&str>,
    ) -> Placing;

    /// [`Shell::place`] for a terminal that runs on `target`, a server other than the one that
    /// asked (`server`): the pane goes beside `anchor`, a terminal of the one that asked, whose
    /// level judges where it goes; the pane itself is the pipe to `target`'s terminal. A shell
    /// that cannot say so makes none.
    fn place_on(
        &mut self,
        _server: &Server,
        _target: &Server,
        _anchor: &Anchor,
        _place: &Place,
        _opener: Option<&str>,
    ) -> Placing {
        Placing::Unable
    }
}

/// What the shell did.
#[derive(Clone, Debug, PartialEq)]
pub enum Placing {
    /// The pane is made.
    Done(Placed),
    /// The anchor is in no tab of this computer.
    NoAnchor,
    /// No pane can be made for it now.
    Unable,
    /// The bateri app will not make it, and says why: a tab, group or window that is not there, or that
    /// the level does not let this machine change. The command ends the terminal it started.
    Refused(Failure),
}

/// What the bateri app says to a letter.
#[derive(Clone, Debug, PartialEq)]
pub enum Answer {
    /// This, and that is all.
    Reply(Reply),
    /// This, and the request is kept until its anchor has a pane.
    Wait(Reply, Box<Waiting>),
    /// `ls`: the shell builds the text from the windows as they are and what the server says.
    List(Format),
    /// `close` of a terminal of this server: the shell closes it and answers (the level has been
    /// judged: it is not `off`, and a terminal of this server is the server's own).
    Close(Anchor),
    /// `notify`: the shell shows it and answers.
    Notify(String),
    /// `message`: the last thing an agent said. The shell keeps it for the pane the letter came
    /// from, to be shown beside the pane's tab, and answers; it asks nothing and starts nothing,
    /// so the level has only judged that it is not `off`.
    Message(String),
    /// `focus`, `zoom`, `swap`, `move`, `resize`: the shell decides by what the keyboard is
    /// doing ([`super::arrange`]) and by the level over the layers it touches, then does it.
    Arrange(Arrange),
    /// `ssh`: the shell finds the server the name stands for — on a thread of its own, for it
    /// waits for a person's password at times — and answers `relay`: the bateri app runs what the
    /// program means over the connection it keeps. The level has been judged: a `full` one.
    Ssh(String),
    /// A `place` whose terminal runs on another server (`server`): the shell finds that server,
    /// then asks [`place_on`] to put the pane. The level has been judged as for [`Answer::Ssh`].
    PlaceOn(Place),
    /// `relay`: the shell runs the command on the machine it names over the connection it keeps
    /// — on a thread of its own — and answers how it ended ([`super::relay`]). The level has been
    /// judged.
    Relay(Relay),
}

fn waiting_reply(anchor: &Anchor) -> Reply {
    Reply::Placed(Placed {
        landed: Landed::Waiting,
        next_to: Some(anchor.clone()),
        squeezed: false,
        hidden: false,
        tab: None,
        group: None,
        window: None,
        keyboard_stayed: false,
    })
}

/// A pane that could not be made now: the terminal is in the list of what runs in the background
/// and nothing is kept to place it — no anchor in the answer, which is how the command knows it will
/// not take its place by itself.
fn unable_reply() -> Reply {
    Reply::Placed(Placed {
        landed: Landed::Waiting,
        next_to: None,
        squeezed: false,
        hidden: false,
        tab: None,
        group: None,
        window: None,
        keyboard_stayed: false,
    })
}

/// The answer to a letter: `text` is what the program said, `from` the terminal kablo says it
/// runs in (never what it says of itself), `server` the server it came on.
pub fn serve(server: &Server, from: Option<&str>, text: &str, shell: &mut dyn Shell) -> Answer {
    // `off`: nothing is done for this server, whatever the request is — a command that is still
    // there (one that was running when the level changed) is told so, once.
    if server.level == CommandLevel::Off {
        return Answer::Reply(Reply::Failed(Failure::new(
            Kind::Refused,
            level::refusal(server.level, Why::Off, server.host_name()),
        )));
    }
    match Request::parse(text) {
        Err(failure) => Answer::Reply(Reply::Failed(failure)),
        Ok(Request::Place(place)) if place.server.is_some() => {
            match may_reach_to(server, place.server.as_deref().unwrap_or_default()) {
                Ok(()) => Answer::PlaceOn(place),
                Err(failure) => Answer::Reply(Reply::Failed(failure)),
            }
        }
        Ok(Request::Place(place)) => place_for(server, from, place, shell),
        Ok(Request::Ls(format)) => Answer::List(format),
        Ok(Request::Close(terminal)) => Answer::Close(terminal),
        Ok(Request::Notify(text)) => Answer::Notify(text),
        Ok(Request::Message(text)) => Answer::Message(text),
        Ok(Request::Arrange(arrange)) => Answer::Arrange(arrange),
        // The machine that asks is no other machine: nothing is reached, and the answer is `here`,
        // which needs no level (the command does it itself, as it does what is its own).
        Ok(Request::Ssh(name)) if is_asker(server, &name) => Answer::Ssh(name),
        Ok(Request::Ssh(name)) => match may_reach_to(server, &name) {
            Ok(()) => Answer::Ssh(name),
            Err(failure) => Answer::Reply(Reply::Failed(failure)),
        },
        Ok(Request::Relay(relay)) => match may_reach_to(server, &relay.server) {
            Ok(()) => Answer::Relay(relay),
            Err(failure) => Answer::Reply(Reply::Failed(failure)),
        },
    }
}

/// Whether the machine that asked may start a terminal on another and work with it: only a
/// `full` one ([`Act::Reach`]). The bateri app does the reaching, over the connection it keeps,
/// whichever machine asked.
fn may_reach(server: &Server) -> Result<(), Failure> {
    match level::judge(server.level, Act::Reach, Panes::default()) {
        Verdict::Allowed => Ok(()),
        Verdict::Refused(why) => Err(Failure::new(
            Kind::Refused,
            level::refusal(server.level, why, server.host_name()),
        )),
        Verdict::Unknown => Err(Failure::new(Kind::Failed, "there is no such machine")),
    }
}

/// Whether `name` is the machine the asker runs on: this computer for a program on this computer (`@local`),
/// a server's own name for a program on that server.
fn is_asker(server: &Server, name: &str) -> bool {
    if crate::kablo::local::is_account(&server.account) {
        name == super::LOCAL_SERVER
    } else {
        server.host_name() == Some(name)
    }
}

/// [`may_reach`] for the machine `name` stands for. This computer is not a machine a program on this
/// computer reaches: it is where it runs, and the word for it is for a program on a server.
fn may_reach_to(server: &Server, name: &str) -> Result<(), Failure> {
    may_reach(server)?;
    if name == super::LOCAL_SERVER && crate::kablo::local::is_account(&server.account) {
        return Err(Failure::new(
            Kind::Usage,
            "`@local` names this computer for a program on a server; a program on this computer is on it already",
        ));
    }
    Ok(())
}

/// The pane for a terminal that runs on `target`, asked by a program on `server` (this computer):
/// beside a terminal of `server`, as [`Place`] says, and a pane that shows it already is done.
/// Nothing is kept for later — the terminal is the target's and runs on — and a pane that cannot be
/// made says why.
pub fn place_on(
    server: &Server,
    target: &Server,
    from: Option<&str>,
    place: &Place,
    shell: &mut dyn Shell,
) -> Reply {
    let Some(anchor) = place
        .anchor
        .clone()
        .or_else(|| from.and_then(Anchor::parse))
    else {
        return Reply::Failed(Failure::new(
            Kind::Usage,
            "no terminal for the pane to go beside: it was not asked from a terminal and names none",
        ));
    };
    if shell.shows(target, place.terminal) {
        return Reply::Placed(Placed {
            landed: Landed::AsAsked,
            next_to: Some(anchor),
            squeezed: false,
            hidden: false,
            tab: None,
            group: None,
            window: None,
            keyboard_stayed: false,
        });
    }
    match shell.place_on(server, target, &anchor, place, from) {
        Placing::Done(placed) => Reply::Placed(placed),
        Placing::NoAnchor | Placing::Unable => Reply::Failed(Failure::new(
            Kind::Failed,
            "the pane to go beside is not on screen, or no pane can be made for the server now; \
             the terminal runs on the server and is in its list",
        )),
        Placing::Refused(failure) => Reply::Failed(failure),
    }
}

fn place_for(server: &Server, from: Option<&str>, place: Place, shell: &mut dyn Shell) -> Answer {
    let Some(anchor) = place
        .anchor
        .clone()
        .or_else(|| from.and_then(Anchor::parse))
    else {
        return Answer::Reply(Reply::Failed(Failure::new(
            Kind::Usage,
            "no terminal for the pane to go beside: it was not asked from a terminal and names none",
        )));
    };
    // A terminal that is not started is this computer's to open when it is a pane of this computer's own; a
    // server's pane is the pipe to a terminal there, which only the server can start: the answer
    // is `ok start`, and the command starts it with the bootstrap the server has and asks again
    // for the pane, with no `fresh` this time.
    if place.fresh.is_some() && !crate::kablo::local::is_account(&server.account) {
        return Answer::Reply(Reply::Start);
    }
    if shell.shows(server, place.terminal) {
        return Answer::Reply(Reply::Placed(Placed {
            landed: Landed::AsAsked,
            next_to: Some(anchor),
            squeezed: false,
            hidden: false,
            // Nothing here says where a pane stands that was already shown.
            tab: None,
            group: None,
            window: None,
            keyboard_stayed: false,
        }));
    }
    match shell.place(server, &anchor, &place, from) {
        Placing::Done(placed) => Answer::Reply(Reply::Placed(placed)),
        // A pane that has no terminal yet is not kept for later: nothing is running to wait
        // for, and the command has not started anything to leave behind.
        Placing::NoAnchor | Placing::Unable if place.fresh.is_some() => {
            Answer::Reply(Reply::Failed(Failure::new(
                Kind::Failed,
                "the pane to go beside is not on screen; a pane with no program is opened beside \
                 one that is",
            )))
        }
        Placing::NoAnchor => {
            let reply = waiting_reply(&anchor);
            Answer::Wait(
                reply,
                Box::new(Waiting {
                    server: server.clone(),
                    anchor,
                    place,
                    opener: from.map(str::to_owned),
                }),
            )
        }
        Placing::Unable => Answer::Reply(unable_reply()),
        Placing::Refused(failure) => Answer::Reply(Reply::Failed(failure)),
    }
}

/// Whether the server's list (a [`survey`](crate::kablo::service::Service::survey), asked for
/// now) says a pane can be made for `terminal`: it runs, or it ended and was asked to stay open.
pub fn alive_in(terminals: &[Status], terminal: Terminal, keep_open: bool) -> bool {
    let name = terminal.to_string();
    terminals.iter().any(|status| {
        status.id.as_str() == name
            && (status.state == State::RUNNING || (keep_open && status.state == State::ENDED))
    })
}

/// What reading a server's notes came to.
#[derive(Debug, Default, PartialEq)]
pub struct Read {
    /// How many panes were made.
    pub placed: u32,
    /// The notes whose anchor is in no tab: kept until it is.
    pub waiting: Vec<Waiting>,
}

/// Places what a server left while the bateri app was away. A terminal is placed only if `alive` says it
/// can be shown (`alive(terminal, keep_open)`: it runs, or it ended and was asked to stay) and no
/// pane shows it yet; every other note is dropped — the terminal that ended is gone, the one a
/// pane shows or another computer has needs nothing.
pub fn settle_notes(
    server: &Server,
    notes: Vec<Note>,
    alive: &dyn Fn(Terminal, bool) -> bool,
    shell: &mut dyn Shell,
) -> Read {
    let mut read = Read::default();
    // A note was written by a command a server had; at `off` nothing of it is done any more.
    if server.level == CommandLevel::Off {
        return read;
    }
    for Note { terminal, place } in notes {
        let Some(anchor) = place.anchor.clone() else {
            continue;
        };
        if !alive(terminal, place.keep_open) || shell.shows(server, terminal) {
            continue;
        }
        match shell.place(server, &anchor, &place, None) {
            Placing::Done(_) => read.placed += 1,
            Placing::NoAnchor => read.waiting.push(Waiting {
                server: server.clone(),
                anchor,
                place,
                opener: None,
            }),
            // A note read later has nobody to tell: what is refused is not made.
            Placing::Unable | Placing::Refused(_) => {}
        }
    }
    read
}

/// A request that waited for its anchor, which has a pane now: placed if the terminal can still
/// be shown (`alive`) and has no pane yet. `Some` when it must wait on (the anchor's pane is not
/// in a tab yet), `None` when it is done with.
pub fn settle_waiting(
    waiting: Waiting,
    alive: &dyn Fn(Terminal, bool) -> bool,
    shell: &mut dyn Shell,
) -> (Option<Waiting>, bool) {
    let Waiting {
        server,
        anchor,
        place,
        opener,
    } = waiting;
    if !alive(place.terminal, place.keep_open) || shell.shows(&server, place.terminal) {
        return (None, false);
    }
    match shell.place(&server, &anchor, &place, opener.as_deref()) {
        Placing::Done(_) => (None, true),
        Placing::NoAnchor => (
            Some(Waiting {
                server,
                anchor,
                place,
                opener,
            }),
            false,
        ),
        Placing::Unable | Placing::Refused(_) => (None, false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::place::Side;
    use crate::agent::wait::Queue;
    use crate::ids::{Id, Kind as IdKind};

    fn server() -> Server {
        Server {
            account: "ada@rasp:22".into(),
            host: "rasp".into(),
            argv: vec!["ssh".into(), "rasp".into()],
            level: CommandLevel::Own,
        }
    }

    /// A pretend bateri app: which anchors have a pane, which terminals have one, what it did.
    #[derive(Default)]
    struct Pretend {
        anchors: Vec<String>,
        shown: Vec<Terminal>,
        unable: bool,
        /// The bateri app will not do it, for this reason.
        refuse: Option<Failure>,
        placed: Vec<(String, Terminal, Option<Side>, bool)>,
        /// Who each ask said it came from, in order.
        openers: Vec<Option<String>>,
    }

    impl Shell for Pretend {
        fn shows(&self, _: &Server, terminal: Terminal) -> bool {
            self.shown.contains(&terminal)
        }

        fn place(
            &mut self,
            _: &Server,
            anchor: &Anchor,
            place: &Place,
            opener: Option<&str>,
        ) -> Placing {
            self.openers.push(opener.map(str::to_owned));
            if self.unable {
                return Placing::Unable;
            }
            if let Some(failure) = self.refuse.clone() {
                return Placing::Refused(failure);
            }
            if !self.anchors.iter().any(|known| known == anchor.name()) {
                return Placing::NoAnchor;
            }
            self.placed.push((
                anchor.name().to_owned(),
                place.terminal,
                place.side,
                place.focus,
            ));
            self.shown.push(place.terminal);
            Placing::Done(Placed {
                landed: Landed::AsAsked,
                next_to: Some(anchor.clone()),
                squeezed: false,
                hidden: false,
                tab: Id::new(IdKind::Tab, 3),
                group: None,
                window: Id::new(IdKind::Window, 1),
                keyboard_stayed: false,
            })
        }
    }

    fn place_text(extra: &str) -> String {
        format!("bateri 2\nplace\nterminal p9\n{extra}")
    }

    /// Whether the pane that was placed for `text` asked for the keyboard, from `from`, when the
    /// anchors the bateri app shows are `anchors`: what the shell is told, which is the whole of
    /// what it can act on.
    fn focus_for(from: Option<&str>, text: &str, anchors: &[&str]) -> Option<bool> {
        let mut app = Pretend {
            anchors: anchors.iter().map(|name| (*name).to_owned()).collect(),
            ..Pretend::default()
        };
        serve(&server(), from, &place_text(text), &mut app);
        app.placed.first().map(|placed| placed.3)
    }

    fn this_computer() -> Server {
        Server {
            account: crate::kablo::local::ACCOUNT.into(),
            host: String::new(),
            argv: Vec::new(),
            level: CommandLevel::Own,
        }
    }

    /// A pane with no terminal yet is this computer's to open; a server's is started by the server (`ok
    /// start`), and one that has nothing to stand beside is not kept for later (nothing runs to
    /// wait for).
    #[test]
    fn a_pane_with_no_terminal_yet_is_this_computers_and_is_not_kept_waiting() {
        let ask = place_text("anchor p4\nfresh /Users/ada/api\n");
        let mut app = Pretend {
            anchors: vec!["p4".into()],
            ..Pretend::default()
        };
        let Answer::Reply(Reply::Placed(placed)) =
            serve(&this_computer(), Some("p4"), &ask, &mut app)
        else {
            panic!("placed");
        };
        assert_eq!(placed.landed, Landed::AsAsked);
        assert_eq!(app.placed.len(), 1);

        // The anchor is on no pane: refused at once, not queued.
        let mut app = Pretend::default();
        let Answer::Reply(Reply::Failed(failure)) =
            serve(&this_computer(), Some("p4"), &ask, &mut app)
        else {
            panic!("refused");
        };
        assert_eq!(failure.kind, Kind::Failed);
        assert!(failure.text.contains("not on screen"), "{}", failure.text);
        // And not for a server, whose pane is the pipe to a terminal there: the server is told to
        // start it, and nothing is placed.
        let mut app = Pretend {
            anchors: vec!["p4".into()],
            ..Pretend::default()
        };
        let Answer::Reply(reply) = serve(&server(), Some("p4"), &ask, &mut app) else {
            panic!("answered");
        };
        assert_eq!(reply, Reply::Start);
        assert!(app.placed.is_empty() && app.openers.is_empty());
        // Naming a shell does not change it: nothing of it runs here.
        let ask = place_text("anchor p4\nfresh /tmp\nshell /bin/zsh\n");
        let mut app = Pretend {
            anchors: vec!["p4".into()],
            ..Pretend::default()
        };
        let Answer::Reply(reply) = serve(&server(), Some("p4"), &ask, &mut app) else {
            panic!("answered");
        };
        assert_eq!(reply, Reply::Start);
        assert!(app.placed.is_empty() && app.openers.is_empty());
        // Asked again with no `fresh`, for the terminal the server started, it is a pane like any.
        let again = place_text("anchor p4\n");
        let mut app = Pretend {
            anchors: vec!["p4".into()],
            ..Pretend::default()
        };
        let Answer::Reply(Reply::Placed(placed)) = serve(&server(), Some("p4"), &again, &mut app)
        else {
            panic!("placed");
        };
        assert_eq!(placed.landed, Landed::AsAsked);
    }

    /// Reaching another machine — starting a terminal there, or working with one — is a `full`
    /// machine's, a server's as well as this computer's: the others are refused (77) with the setting
    /// that changes it, `off` first of all. Nothing is asked of the windows before this is
    /// decided, and a relay that is refused runs nothing.
    #[test]
    fn reaching_another_machine_is_for_a_full_machine_alone() {
        let relay = |name: &str| {
            Request::Relay(Relay {
                server: name.to_owned(),
                words: vec!["send".into(), "p9".into(), "q".into()],
            })
            .encode()
            .expect("a relay")
        };
        let asks = [
            "bateri 2\nssh\nserver prod\n".to_owned(),
            place_text("anchor p4\nserver prod\n"),
            relay("prod"),
            relay("@local"),
        ];
        for text in &asks {
            for level in [CommandLevel::Own, CommandLevel::Screen] {
                for mut server in [this_computer(), self::server()] {
                    server.level = level;
                    let mut app = Pretend {
                        anchors: vec!["p4".into()],
                        ..Pretend::default()
                    };
                    let Answer::Reply(Reply::Failed(failure)) =
                        serve(&server, Some("p4"), text, &mut app)
                    else {
                        panic!("{level:?}: refused");
                    };
                    assert_eq!(failure.kind, Kind::Refused, "{level:?} {text:?}");
                    assert!(failure.text.contains("only \"full\""), "{}", failure.text);
                    assert!(app.placed.is_empty() && app.openers.is_empty());
                }
            }
            // `off` is refused before the request is read.
            for mut off in [this_computer(), self::server()] {
                off.level = CommandLevel::Off;
                let Answer::Reply(Reply::Failed(failure)) =
                    serve(&off, Some("p4"), text, &mut Pretend::default())
                else {
                    panic!("off");
                };
                assert_eq!(failure.kind, Kind::Refused);
            }
        }

        // A `full` server goes on to the window: the connection is found on a thread, or the
        // command is run on one; nothing is placed here.
        let mut full = self::server();
        full.level = CommandLevel::Full;
        for (text, name) in [(&asks[0], "prod"), (&asks[1], "prod")] {
            let mut app = Pretend {
                anchors: vec!["p4".into()],
                ..Pretend::default()
            };
            match serve(&full, Some("p4"), text, &mut app) {
                Answer::Ssh(asked) => assert_eq!(asked, name),
                Answer::PlaceOn(place) => assert_eq!(place.server.as_deref(), Some(name)),
                other => panic!("{other:?}"),
            }
            assert!(app.placed.is_empty() && app.openers.is_empty());
        }
        for (text, name) in [(&asks[2], "prod"), (&asks[3], "@local")] {
            match serve(&full, Some("p4"), text, &mut Pretend::default()) {
                Answer::Relay(relay) => {
                    assert_eq!(relay.server, name);
                    assert_eq!(relay.words, ["send", "p9", "q"]);
                }
                other => panic!("{other:?}"),
            }
        }
        for text in [
            "bateri 2\nssh\nserver @local\n".to_owned(),
            place_text("anchor p4\nserver @local\n"),
        ] {
            let mut app = Pretend {
                anchors: vec!["p4".into()],
                ..Pretend::default()
            };
            assert!(
                matches!(
                    serve(&full, Some("p4"), &text, &mut app),
                    Answer::Ssh(_) | Answer::PlaceOn(_)
                ),
                "{text:?}"
            );
        }

        // This computer's `full` reaches a server the same way; `@local` is the word for this
        // computer to a server, not to itself.
        let mut full = this_computer();
        full.level = CommandLevel::Full;
        for text in &asks[..3] {
            let mut app = Pretend {
                anchors: vec!["p4".into()],
                ..Pretend::default()
            };
            match serve(&full, Some("p4"), text, &mut app) {
                Answer::Ssh(_) | Answer::PlaceOn(_) | Answer::Relay(_) => {}
                other => panic!("{text:?}: {other:?}"),
            }
            assert!(app.placed.is_empty() && app.openers.is_empty());
        }
        for text in [asks[3].clone(), place_text("anchor p4\nserver @local\n")] {
            let Answer::Reply(Reply::Failed(failure)) =
                serve(&full, Some("p4"), &text, &mut Pretend::default())
            else {
                panic!("{text:?}");
            };
            assert_eq!(failure.kind, Kind::Usage, "{text:?}");
        }
    }

    /// The machine that asks is no other machine: asked how to reach it, the window passes the
    /// question on (to be answered `here`) whatever the level, and a different machine is still a
    /// `full` one's.
    #[test]
    fn the_machine_that_asks_is_reached_by_nothing_and_needs_no_level() {
        for level in [CommandLevel::Own, CommandLevel::Screen, CommandLevel::Full] {
            let mut this = this_computer();
            this.level = level;
            match serve(
                &this,
                Some("p4"),
                "bateri 2\nssh\nserver @local\n",
                &mut Pretend::default(),
            ) {
                Answer::Ssh(name) => assert_eq!(name, "@local", "{level:?}"),
                other => panic!("{level:?}: {other:?}"),
            }
            let mut rasp = self::server();
            rasp.level = level;
            match serve(
                &rasp,
                Some("p4"),
                "bateri 2\nssh\nserver rasp\n",
                &mut Pretend::default(),
            ) {
                Answer::Ssh(name) => assert_eq!(name, "rasp", "{level:?}"),
                other => panic!("{level:?}: {other:?}"),
            }
        }
        // For a program on a server, `@local` is this computer: another machine, a `full` one's.
        let mut rasp = self::server();
        rasp.level = CommandLevel::Own;
        let Answer::Reply(Reply::Failed(failure)) = serve(
            &rasp,
            Some("p4"),
            "bateri 2\nssh\nserver @local\n",
            &mut Pretend::default(),
        ) else {
            panic!("refused");
        };
        assert_eq!(failure.kind, Kind::Refused);
    }

    /// The pane for a terminal that runs on another server goes beside a terminal of the machine
    /// that asked and is the pipe to the target's: a pane that shows it already is done, one that
    /// cannot go beside its anchor says so and keeps nothing.
    #[test]
    fn a_pane_for_a_terminal_on_another_server_is_placed_beside_the_askers_anchor() {
        let mut asker = this_computer();
        asker.level = CommandLevel::Full;
        let mut target = server();
        target.level = CommandLevel::Own;
        let place = Place {
            terminal: Terminal(9),
            anchor: Anchor::parse("p4"),
            side: None,
            ratio: None,
            keep_open: false,
            focus: false,
            float: None,
            home: crate::agent::message::Home::default(),
            fresh: None,
            shell: None,
            server: Some("rasp".into()),
        };

        struct OnTarget(Pretend, Vec<String>);
        impl Shell for OnTarget {
            fn shows(&self, server: &Server, terminal: Terminal) -> bool {
                self.0.shows(server, terminal)
            }
            fn place(
                &mut self,
                server: &Server,
                anchor: &Anchor,
                place: &Place,
                opener: Option<&str>,
            ) -> Placing {
                self.0.place(server, anchor, place, opener)
            }
            fn place_on(
                &mut self,
                server: &Server,
                target: &Server,
                anchor: &Anchor,
                place: &Place,
                opener: Option<&str>,
            ) -> Placing {
                self.1
                    .push(format!("{} -> {}", server.account, target.account));
                self.0.place(server, anchor, place, opener)
            }
        }

        let mut app = OnTarget(
            Pretend {
                anchors: vec!["p4".into()],
                ..Pretend::default()
            },
            Vec::new(),
        );
        let Reply::Placed(placed) = place_on(&asker, &target, Some("p4"), &place, &mut app) else {
            panic!("placed");
        };
        assert_eq!(placed.landed, Landed::AsAsked);
        assert_eq!(app.1, ["local -> ada@rasp:22"]);
        // Asked again, the pane is there: done, nothing is made twice.
        let Reply::Placed(again) = place_on(&asker, &target, Some("p4"), &place, &mut app) else {
            panic!("placed");
        };
        assert_eq!(again.landed, Landed::AsAsked);
        assert_eq!(app.1.len(), 1);

        // No anchor: named by nobody. An anchor on no pane: said, and nothing is kept for later.
        let mut lonely = place.clone();
        lonely.anchor = None;
        let mut app = OnTarget(Pretend::default(), Vec::new());
        let Reply::Failed(failure) = place_on(&asker, &target, None, &lonely, &mut app) else {
            panic!("no anchor");
        };
        assert_eq!(failure.kind, Kind::Usage);
        let Reply::Failed(failure) = place_on(&asker, &target, Some("p4"), &place, &mut app) else {
            panic!("not on screen");
        };
        assert_eq!(failure.kind, Kind::Failed);
        assert!(failure.text.contains("not on screen"), "{}", failure.text);
    }

    /// A refusal names the host the user typed for a server, and for this computer the setting that
    /// is its own.
    #[test]
    fn a_refusal_names_the_setting_of_the_machine_it_is_for() {
        let off = |mut server: Server| {
            server.level = CommandLevel::Off;
            let Answer::Reply(Reply::Failed(failure)) = serve(
                &server,
                Some("p4"),
                &place_text("anchor p4\n"),
                &mut Pretend::default(),
            ) else {
                panic!("refused");
            };
            failure.text
        };
        let text = off(server());
        assert!(
            text.contains("on rasp") && text.contains("[remote] hosts"),
            "{text}"
        );
        let text = off(this_computer());
        assert!(
            text.contains("on this computer") && text.contains("[commands]"),
            "{text}"
        );
    }

    /// `--focus` reaches the shell as it was asked, with who asked: whether the keyboard moves is
    /// the shell's to decide by where the cursor is ([`super::super::arrange`]), not the server's
    /// to guess by whom the pane goes beside.
    #[test]
    fn the_focus_reaches_the_shell_as_asked_with_the_terminal_that_asked() {
        let ask = "anchor p4\nfocus yes\n";
        assert_eq!(focus_for(Some("p4"), ask, &["p4"]), Some(true));
        assert_eq!(
            focus_for(Some("p4"), "focus yes\n", &["p4"]),
            Some(true),
            "no anchor named: the sender's own"
        );
        // A program in p9 that names p4: the shell sees p9 asked, and decides.
        assert_eq!(focus_for(Some("p9"), ask, &["p4"]), Some(true));
        assert_eq!(focus_for(None, ask, &["p4"]), Some(true));
        // Without the word, never.
        assert_eq!(focus_for(Some("p4"), "anchor p4\n", &["p4"]), Some(false));
        let mut app = Pretend {
            anchors: vec!["p4".to_owned()],
            ..Pretend::default()
        };
        serve(&server(), Some("p9"), &place_text(ask), &mut app);
        assert_eq!(app.openers, [Some("p9".to_owned())]);
    }

    /// A request that waited keeps who asked, so the shell decides when the pane is placed; a
    /// note that was read later does not say, and the shell decides without a caller.
    #[test]
    fn the_focus_of_a_request_that_waited_and_of_a_note_reaches_the_shell_too() {
        let text = "anchor p4\nfocus yes\n";
        for from in [Some("p4"), Some("p9"), None] {
            let mut idle = Pretend::default();
            let Answer::Wait(_, waiting) = serve(&server(), from, &place_text(text), &mut idle)
            else {
                panic!("waits: {from:?}");
            };
            idle.anchors.push("p4".to_owned());
            settle_waiting(*waiting, &|_, _| true, &mut idle);
            assert!(idle.placed[0].3, "waited, from {from:?}");
            assert_eq!(
                idle.openers.last().cloned().flatten().as_deref(),
                from,
                "who asked is kept"
            );
        }
        let Request::Place(place) = Request::parse(&place_text(text)).expect("a place") else {
            panic!("a place");
        };
        let mut app = Pretend {
            anchors: vec!["p4".to_owned()],
            ..Pretend::default()
        };
        settle_notes(
            &server(),
            vec![Note {
                terminal: Terminal(9),
                place,
            }],
            &|_, _| true,
            &mut app,
        );
        assert!(app.placed[0].3, "a note");
        assert_eq!(app.openers, [None]);
    }

    #[test]
    fn a_ticket_a_place_a_landing_and_the_answer() {
        let mut app = Pretend {
            anchors: vec!["p4".to_owned()],
            ..Pretend::default()
        };
        // Named anchor, a side and a focus: the shell is asked for exactly that.
        let answer = serve(
            &server(),
            Some("p4"),
            &place_text("anchor p4\nside below\nratio 30\nfocus yes\n"),
            &mut app,
        );
        let Answer::Reply(Reply::Placed(placed)) = answer else {
            panic!("{answer:?}");
        };
        assert_eq!(placed.landed, Landed::AsAsked);
        assert_eq!(placed.next_to.as_ref().map(Anchor::name), Some("p4"));
        // Where the pane stands is the shell's word, passed on as it was given.
        assert_eq!(placed.tab.map(|id| id.to_string()).as_deref(), Some("t3"));
        assert_eq!(placed.group, None);
        assert_eq!(
            placed.window.map(|id| id.to_string()).as_deref(),
            Some("w1")
        );
        assert_eq!(
            app.placed,
            [("p4".to_owned(), Terminal(9), Some(Side::Below), true)]
        );
    }

    #[test]
    fn the_shell_is_told_who_asked_and_a_request_that_waits_remembers_it() {
        let mut app = Pretend {
            anchors: vec!["p4".to_owned()],
            ..Pretend::default()
        };
        // Named anchor and a different sender: the sender is the opener.
        serve(&server(), Some("p9"), &place_text("anchor p4\n"), &mut app);
        assert_eq!(app.openers, [Some("p9".to_owned())]);
        // Waiting for its anchor, it keeps who asked and says it when it is placed.
        let mut idle = Pretend::default();
        let Answer::Wait(_, waiting) =
            serve(&server(), Some("p9"), &place_text("anchor p4\n"), &mut idle)
        else {
            panic!("waits");
        };
        assert_eq!(waiting.opener.as_deref(), Some("p9"));
        idle.anchors.push("p4".to_owned());
        idle.openers.clear();
        settle_waiting(*waiting, &|_, _| true, &mut idle);
        assert_eq!(idle.openers, [Some("p9".to_owned())]);
    }

    #[test]
    fn with_no_anchor_the_pane_goes_beside_the_terminal_the_ticket_names() {
        let made = format!("b-{}", "0123456789abcdef".repeat(2));
        let mut app = Pretend {
            anchors: vec!["p4".to_owned()],
            ..Pretend::default()
        };
        // The terminal the bateri app opened before the numbers existed is the caller too.
        let mut older = Pretend {
            anchors: vec![made.clone()],
            ..Pretend::default()
        };
        assert!(matches!(
            serve(&server(), Some("p4"), &place_text(""), &mut app),
            Answer::Reply(Reply::Placed(_))
        ));
        assert_eq!(app.placed[0].0, "p4");
        assert!(matches!(
            serve(&server(), Some(&made), &place_text(""), &mut older),
            Answer::Reply(Reply::Placed(_))
        ));
        // Neither a ticket's terminal nor a named anchor: a mistake in the request, and the
        // answer says so (the command ends the terminal it started for it).
        let Answer::Reply(Reply::Failed(failure)) =
            serve(&server(), None, &place_text(""), &mut app)
        else {
            panic!("no anchor");
        };
        assert_eq!(failure.kind, Kind::Usage);
        assert_eq!(app.placed.len(), 1, "nothing was placed for it");
    }

    #[test]
    fn a_terminal_that_has_a_pane_is_done_and_is_not_placed_twice() {
        let mut app = Pretend {
            anchors: vec!["p4".to_owned()],
            shown: vec![Terminal(9)],
            ..Pretend::default()
        };
        let answer = serve(&server(), Some("p4"), &place_text("anchor p4\n"), &mut app);
        let Answer::Reply(Reply::Placed(placed)) = answer else {
            panic!("{answer:?}");
        };
        assert_eq!(placed.landed, Landed::AsAsked);
        assert!(app.placed.is_empty());
    }

    #[test]
    fn an_anchor_in_no_tab_waits_and_the_answer_says_so() {
        let mut app = Pretend::default();
        let answer = serve(&server(), Some("p4"), &place_text("anchor p4\n"), &mut app);
        let Answer::Wait(Reply::Placed(placed), waiting) = answer else {
            panic!("{answer:?}");
        };
        assert_eq!(placed.landed, Landed::Waiting);
        assert_eq!(placed.next_to.as_ref().map(Anchor::name), Some("p4"));
        assert_eq!(waiting.anchor.name(), "p4");
        assert_eq!(waiting.place.terminal, Terminal(9));
        assert_eq!(waiting.server, server());

        // The anchor gets a pane; the queue hands the request back and it is placed.
        let mut queue = Queue::default();
        queue.add(*waiting);
        app.anchors.push("p4".to_owned());
        let due = queue.take_beside("ada@rasp:22", "p4");
        assert_eq!(due.len(), 1);
        let (again, made) =
            settle_waiting(due.into_iter().next().expect("one"), &|_, _| true, &mut app);
        assert!(again.is_none() && made);
        assert_eq!(app.placed.len(), 1);
        assert!(queue.is_empty());
    }

    #[test]
    fn a_pane_that_cannot_be_made_leaves_the_terminal_in_the_list_and_keeps_nothing() {
        let mut app = Pretend {
            unable: true,
            ..Pretend::default()
        };
        let answer = serve(&server(), Some("p4"), &place_text("anchor p4\n"), &mut app);
        let Answer::Reply(Reply::Placed(placed)) = answer else {
            panic!("{answer:?}");
        };
        assert_eq!(placed.landed, Landed::Waiting);
        assert_eq!(placed.next_to, None, "nothing is kept to place it by");
    }

    #[test]
    fn a_pane_the_mac_refuses_is_answered_with_the_reason_and_kept_by_nobody() {
        let refusal = Failure::new(Kind::Refused, "that tab has a pane of another machine");
        let mut app = Pretend {
            anchors: vec!["p4".into()],
            refuse: Some(refusal.clone()),
            ..Pretend::default()
        };
        let text = place_text("anchor p4\nside tab\ngroup g1\n");
        assert_eq!(
            serve(&server(), Some("p4"), &text, &mut app),
            Answer::Reply(Reply::Failed(refusal.clone()))
        );
        assert!(app.placed.is_empty());
        // A note read later, and a request that waited, have nobody to tell: they are dropped.
        let read = settle_notes(
            &server(),
            vec![note(5, "p4", false)],
            &|_, _| true,
            &mut app,
        );
        assert_eq!(read, Read::default());
        let waiting = Waiting {
            server: server(),
            anchor: Anchor::parse("p4").expect("a name"),
            place: note(6, "p4", false).place,
            opener: None,
        };
        assert_eq!(
            settle_waiting(waiting, &|_, _| true, &mut app),
            (None, false)
        );
    }

    #[test]
    fn a_request_that_is_wrong_is_answered_with_the_mistake() {
        let mut app = Pretend::default();
        for text in ["", "hello\n", "bateri 2\nplace\n"] {
            let Answer::Reply(Reply::Failed(_)) = serve(&server(), None, text, &mut app) else {
                panic!("{text:?}");
            };
        }
        // The version before this one and a later one are not read, each by its own error.
        for text in [
            "bateri 1\nplace\nterminal p9\n",
            "bateri 3\nplace\nterminal p9\n",
        ] {
            let Answer::Reply(Reply::Failed(failure)) = serve(&server(), None, text, &mut app)
            else {
                panic!("{text:?}");
            };
            assert_eq!(failure.kind, Kind::Version, "{text:?}");
        }
        assert!(app.placed.is_empty());
    }

    #[test]
    fn ls_close_and_notify_are_for_the_shell_to_carry_out() {
        let mut app = Pretend::default();
        assert_eq!(
            serve(&server(), None, "bateri 2\nls\n", &mut app),
            Answer::List(Format::Text)
        );
        assert_eq!(
            serve(&server(), None, "bateri 2\nls\nformat json\n", &mut app),
            Answer::List(Format::Json)
        );
        assert_eq!(
            serve(&server(), None, "bateri 2\nclose\nterminal p3\n", &mut app),
            Answer::Close(Anchor::of(Terminal(3)))
        );
        assert_eq!(
            serve(&server(), None, "bateri 2\nnotify\ntext hi\n", &mut app),
            Answer::Notify("hi".to_owned())
        );
        assert_eq!(
            serve(&server(), None, "bateri 2\nmessage\ntext hi\n", &mut app),
            Answer::Message("hi".to_owned())
        );
        // A target that is no name kablo has is no request.
        let Answer::Reply(Reply::Failed(failure)) =
            serve(&server(), None, "bateri 2\nclose\nterminal a b\n", &mut app)
        else {
            panic!("not a terminal name");
        };
        assert_eq!(failure.kind, Kind::Usage);
        assert!(app.placed.is_empty());
    }

    #[test]
    fn a_pane_can_be_made_for_a_terminal_the_server_says_runs_or_that_ended_and_was_to_stay() {
        use kablo_proto::SessionId;
        let status = |id: &str, state: State| {
            let mut status = Status::new(SessionId::new(id).expect("a name"));
            status.state = state;
            status
        };
        let list = [
            status("p5", State::RUNNING),
            status("p6", State::ENDED),
            status("p7", State::BROKEN),
        ];
        assert!(alive_in(&list, Terminal(5), false));
        assert!(!alive_in(&list, Terminal(6), false));
        assert!(alive_in(&list, Terminal(6), true));
        assert!(!alive_in(&list, Terminal(7), true));
        assert!(
            !alive_in(&list, Terminal(8), true),
            "the server has no such terminal"
        );
    }

    #[test]
    fn a_close_is_the_shells_to_carry_out_for_every_level_but_off_and_nobody_is_asked() {
        // The terminal need not be the asker's own or one it opened: the level decides.
        for level in [CommandLevel::Own, CommandLevel::Screen, CommandLevel::Full] {
            let server = Server { level, ..server() };
            let mut app = Pretend::default();
            for from in [Some("p4"), Some("p9"), None] {
                assert_eq!(
                    serve(&server, from, "bateri 2\nclose\nterminal p7\n", &mut app),
                    Answer::Close(Anchor::of(Terminal(7))),
                    "{level:?} {from:?}"
                );
            }
        }
    }

    #[test]
    fn a_server_at_off_gets_a_refusal_for_every_request_and_nothing_is_done() {
        let server = Server {
            level: CommandLevel::Off,
            ..server()
        };
        let mut app = Pretend {
            anchors: vec!["p4".to_owned()],
            ..Pretend::default()
        };
        for text in [
            place_text("anchor p4\n"),
            "bateri 2\nls\n".to_owned(),
            "bateri 2\nclose\nterminal p7\n".to_owned(),
            "bateri 2\nnotify\ntext hi\n".to_owned(),
            "bateri 2\nmessage\ntext hi\n".to_owned(),
            "not a message".to_owned(),
        ] {
            let Answer::Reply(Reply::Failed(failure)) = serve(&server, Some("p4"), &text, &mut app)
            else {
                panic!("{text:?}");
            };
            assert_eq!(failure.kind, Kind::Refused, "{text:?}");
            assert!(failure.text.contains("rasp"), "{failure:?}");
            assert!(failure.text.contains("off"), "{failure:?}");
            assert!(failure.text.contains("[remote] hosts"), "{failure:?}");
        }
        assert!(app.placed.is_empty() && app.openers.is_empty());
    }

    fn note(terminal: u64, anchor: &str, keep_open: bool) -> Note {
        Note {
            terminal: Terminal(terminal),
            place: Place {
                terminal: Terminal(terminal),
                anchor: Anchor::parse(anchor),
                side: None,
                ratio: None,
                keep_open,
                focus: false,
                float: None,
                fresh: None,
                shell: None,
                server: None,
                home: crate::agent::message::Home::default(),
            },
        }
    }

    #[test]
    fn what_was_left_while_the_mac_was_away_is_placed_when_the_terminal_can_still_be_shown() {
        let mut app = Pretend {
            anchors: vec!["p4".to_owned()],
            shown: vec![Terminal(3)],
            ..Pretend::default()
        };
        // p5 runs, p6 ended and was not asked to stay, p7 ended and was, p3 has a pane, p8's
        // anchor is in no tab.
        let alive = |terminal: Terminal, keep_open: bool| match terminal.0 {
            5 | 3 | 8 => true,
            7 => keep_open,
            _ => false,
        };
        let read = settle_notes(
            &server(),
            vec![
                note(5, "p4", false),
                note(6, "p4", false),
                note(7, "p4", true),
                note(3, "p4", false),
                note(8, "p1", false),
            ],
            &alive,
            &mut app,
        );
        assert_eq!(read.placed, 2, "p5 and p7");
        assert_eq!(
            app.placed.iter().map(|p| p.1).collect::<Vec<_>>(),
            [Terminal(5), Terminal(7)]
        );
        assert_eq!(read.waiting.len(), 1);
        assert_eq!(read.waiting[0].place.terminal, Terminal(8));
        assert_eq!(read.waiting[0].anchor.name(), "p1");
    }

    #[test]
    fn a_waiting_request_whose_terminal_is_gone_or_shown_is_dropped_and_one_whose_anchor_is_not_in_a_tab_yet_stays()
     {
        let mut app = Pretend::default();
        let waiting = |terminal: u64| Waiting {
            server: server(),
            anchor: Anchor::parse("p4").expect("a name"),
            place: note(terminal, "p4", false).place,
            opener: None,
        };
        // Gone.
        assert_eq!(
            settle_waiting(waiting(9), &|_, _| false, &mut app),
            (None, false)
        );
        // Shown by the user meanwhile.
        app.shown.push(Terminal(9));
        assert_eq!(
            settle_waiting(waiting(9), &|_, _| true, &mut app),
            (None, false)
        );
        // The anchor's pane is not in a tab yet: it stays.
        let (again, made) = settle_waiting(waiting(10), &|_, _| true, &mut app);
        assert!(again.is_some() && !made);
    }
}
