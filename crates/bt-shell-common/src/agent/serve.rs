//! What the Mac does with what a program on a server says to it: the decision, with no window in
//! it. The shell (`bt-shell-macos`) lends its windows through [`Shell`] and puts the panes where
//! this says; a test lends a pretend one.
//!
//! A conversation is one request (`message`) and one answer. This reads the request, checks what
//! is the Mac's to check — the pane is for a terminal **of the server the letter came from**
//! (the letter arrives on that server's own connection, so nothing a request says can name another
//! server's terminal), and it goes beside a terminal that is named or, when none is, the one the
//! ticket says the request came from — and asks the shell to put it there ([`Shell::place`]).
//! Everything a pane could not be made for is a pane that **waits**, never a terminal that is
//! ended: the terminal is the server's and runs on, and the list of what runs in the background
//! (the server's card, ⌘P) is where a terminal with no pane is found.
//!
//! - The anchor is in no tab of this Mac ([`Placing::NoAnchor`]): the answer says `landed waiting`
//!   and the request is kept in a [`Queue`](super::wait::Queue) until a pane shows the anchor.
//! - A pane cannot be made at all now ([`Placing::Unable`]: a bateri with no pipe to a server,
//!   a timed run): the answer says it waits and names no anchor, and nothing is kept — the
//!   terminal is in the list.
//! - The terminal already has a pane: the request is done ([`Landed::AsAsked`]), not an error
//!   — an error would end a terminal the user is looking at.
//!
//! The other three requests need the Mac's windows, a question to the user or the server's own
//! list, none of which a decision with no window in it has: they are an [`Answer`] of their own
//! (`List`, `Close`, `Notify`) that the shell carries out — building `ls` from the live windows
//! and a [`survey`](crate::kablo::service::Service::survey), asking before it closes what the
//! asker did not open ([`may_close`]), showing a notification — and answers when it has.

use kablo_proto::v1::{State, Status};

use super::message::{Failure, Format, Kind, Place, Placed, Reply, Request};
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
}

/// What the Mac's windows are asked.
pub trait Shell {
    /// Whether a pane of this Mac shows `terminal` of the server already.
    fn shows(&self, server: &Server, terminal: Terminal) -> bool;

    /// Shows `place.terminal` in a pane beside the pane of `anchor` — wherever on this Mac that
    /// is — as `place` asks: where it goes is the rule's ([`super::place::place`]), the pane is
    /// born without the keyboard unless `place.focus` says so (and the shell finds that the person
    /// is where the pane goes: the window is the key one and the keyboard is in the anchor's pane),
    /// and a zoomed tab keeps its zoom. `place.focus` is set only where the terminal the request
    /// came from is the anchor. `opener` is the terminal the request came from, when it is known
    /// (a note read later does not say, and the pane is said to be opened from beside its anchor).
    fn place(
        &mut self,
        server: &Server,
        anchor: &Anchor,
        place: &Place,
        opener: Option<&str>,
    ) -> Placing;
}

/// What the shell did.
#[derive(Clone, Debug, PartialEq)]
pub enum Placing {
    /// The pane is made.
    Done(Placed),
    /// The anchor is in no tab of this Mac.
    NoAnchor,
    /// No pane can be made for it now.
    Unable,
}

/// What the Mac says to a letter.
#[derive(Clone, Debug, PartialEq)]
pub enum Answer {
    /// This, and that is all.
    Reply(Reply),
    /// This, and the request is kept until its anchor has a pane.
    Wait(Reply, Waiting),
    /// `ls`: the shell builds the text from the windows as they are and what the server says.
    List(Format),
    /// `close` of a terminal of this server: the shell decides with [`may_close`] whether to
    /// ask, closes it, and answers.
    Close(Anchor),
    /// `notify`: the shell shows it and answers.
    Notify(String),
}

/// Whether `caller` — the terminal a request runs in, as kablo says it — may close `terminal`
/// without the user being asked: its own, or one it opened (`opener`, what the server's record of
/// the terminal says). A program that runs in no terminal opened nothing. Anything else is the
/// user's to allow.
pub fn may_close(caller: Option<&str>, terminal: &Anchor, opener: Option<&Anchor>) -> bool {
    let Some(caller) = caller else {
        return false;
    };
    caller == terminal.name() || opener.is_some_and(|opener| opener.name() == caller)
}

fn waiting_reply(anchor: &Anchor) -> Reply {
    Reply::Placed(Placed {
        landed: Landed::Waiting,
        next_to: Some(anchor.clone()),
        squeezed: false,
        hidden: false,
    })
}

/// A pane that could not be made now: the terminal is in the list of what runs in the background
/// and nothing is kept to place it — no anchor in the answer, which is how the script knows it will
/// not take its place by itself.
fn unable_reply() -> Reply {
    Reply::Placed(Placed {
        landed: Landed::Waiting,
        next_to: None,
        squeezed: false,
        hidden: false,
    })
}

/// `place` as the shell is asked for it: the keyboard (`--focus`) is the person's word when they say
/// it in the pane they are in, and a program can say it for them. It is asked of the shell only
/// where the terminal that asked is the anchor, the pane the pane goes beside; a program in one
/// terminal that names another as the anchor (the person's, which `ls` shows it), a request whose
/// sender is not known (one that waited and was kept, a note read later) is placed without it. The
/// shell adds what only it knows: the window is the key one and the keyboard is in the anchor's pane.
fn without_a_focus_nobody_spoke_for(place: &Place, anchor: &Anchor, opener: Option<&str>) -> Place {
    let mut place = place.clone();
    place.focus = place.focus && opener == Some(anchor.name());
    place
}

/// The answer to a letter: `text` is what the program said, `from` the terminal kablo says it
/// runs in (never what it says of itself), `server` the server it came on.
pub fn serve(server: &Server, from: Option<&str>, text: &str, shell: &mut dyn Shell) -> Answer {
    match Request::parse(text) {
        Err(failure) => Answer::Reply(Reply::Failed(failure)),
        Ok(Request::Place(place)) => place_for(server, from, place, shell),
        Ok(Request::Ls(format)) => Answer::List(format),
        Ok(Request::Close(terminal)) => Answer::Close(terminal),
        Ok(Request::Notify(text)) => Answer::Notify(text),
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
    if shell.shows(server, place.terminal) {
        return Answer::Reply(Reply::Placed(Placed {
            landed: Landed::AsAsked,
            next_to: Some(anchor),
            squeezed: false,
            hidden: false,
        }));
    }
    let asked = without_a_focus_nobody_spoke_for(&place, &anchor, from);
    match shell.place(server, &anchor, &asked, from) {
        Placing::Done(placed) => Answer::Reply(Reply::Placed(placed)),
        Placing::NoAnchor => {
            let reply = waiting_reply(&anchor);
            Answer::Wait(
                reply,
                Waiting {
                    server: server.clone(),
                    anchor,
                    place,
                    opener: from.map(str::to_owned),
                },
            )
        }
        Placing::Unable => Answer::Reply(unable_reply()),
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

/// Places what a server left while the Mac was away. A terminal is placed only if `alive` says it
/// can be shown (`alive(terminal, keep_open)`: it runs, or it ended and was asked to stay) and no
/// pane shows it yet; every other note is dropped — the terminal that ended is gone, the one a
/// pane shows or another Mac has needs nothing.
pub fn settle_notes(
    server: &Server,
    notes: Vec<Note>,
    alive: &dyn Fn(Terminal, bool) -> bool,
    shell: &mut dyn Shell,
) -> Read {
    let mut read = Read::default();
    for Note { terminal, place } in notes {
        let Some(anchor) = place.anchor.clone() else {
            continue;
        };
        if !alive(terminal, place.keep_open) || shell.shows(server, terminal) {
            continue;
        }
        let asked = without_a_focus_nobody_spoke_for(&place, &anchor, None);
        match shell.place(server, &anchor, &asked, None) {
            Placing::Done(_) => read.placed += 1,
            Placing::NoAnchor => read.waiting.push(Waiting {
                server: server.clone(),
                anchor,
                place,
                opener: None,
            }),
            Placing::Unable => {}
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
    let asked = without_a_focus_nobody_spoke_for(&place, &anchor, opener.as_deref());
    match shell.place(&server, &anchor, &asked, opener.as_deref()) {
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
        Placing::Unable => (None, false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::place::Side;
    use crate::agent::wait::Queue;

    fn server() -> Server {
        Server {
            account: "ada@rasp:22".into(),
            host: "rasp".into(),
            argv: vec!["ssh".into(), "rasp".into()],
        }
    }

    /// A pretend Mac: which anchors have a pane, which terminals have one, what it did.
    #[derive(Default)]
    struct Pretend {
        anchors: Vec<String>,
        shown: Vec<Terminal>,
        unable: bool,
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
            })
        }
    }

    fn place_text(extra: &str) -> String {
        format!("bateri 1\nplace\nterminal p9\n{extra}")
    }

    /// Whether the pane that was placed for `text` asked for the keyboard, from `from`, on a Mac
    /// whose anchors are `anchors`: what the shell is told, which is the whole of what it can act on.
    fn focus_for(from: Option<&str>, text: &str, anchors: &[&str]) -> Option<bool> {
        let mut mac = Pretend {
            anchors: anchors.iter().map(|name| (*name).to_owned()).collect(),
            ..Pretend::default()
        };
        serve(&server(), from, &place_text(text), &mut mac);
        mac.placed.first().map(|placed| placed.3)
    }

    /// `--focus` is the person's word when they say it in the pane they are in; a program that
    /// names another terminal as the anchor (the person's, which it can see in `ls`) asks to take
    /// the keyboard from where the person is. The shell is told the focus only where the terminal
    /// that asked is the anchor: a program in p9 naming p4, a request that says who it is from
    /// nowhere, is placed behind.
    #[test]
    fn the_focus_is_asked_of_the_shell_only_by_the_terminal_the_pane_goes_beside() {
        let ask = "anchor p4\nfocus yes\n";
        // The person in p4, at p4's pane: as asked.
        assert_eq!(focus_for(Some("p4"), ask, &["p4"]), Some(true));
        assert_eq!(
            focus_for(Some("p4"), "focus yes\n", &["p4"]),
            Some(true),
            "no anchor named: the sender's own"
        );
        // A program in p9 names p4: the pane goes beside p4, behind.
        assert_eq!(focus_for(Some("p9"), ask, &["p4"]), Some(false));
        // A request that says who it is from nowhere.
        assert_eq!(focus_for(None, ask, &["p4"]), Some(false));
        // Without the word, never.
        assert_eq!(focus_for(Some("p4"), "anchor p4\n", &["p4"]), Some(false));
    }

    /// The same for a request that waited for its anchor and for a note that was read later (which
    /// does not say who wrote it): neither is the person speaking in the pane they are in.
    #[test]
    fn the_focus_of_a_request_that_waited_or_of_a_note_is_not_asked_of_the_shell() {
        let text = "anchor p4\nfocus yes\n";
        for (from, want) in [(Some("p4"), true), (Some("p9"), false), (None, false)] {
            let mut idle = Pretend::default();
            let Answer::Wait(_, waiting) = serve(&server(), from, &place_text(text), &mut idle)
            else {
                panic!("waits: {from:?}");
            };
            idle.anchors.push("p4".to_owned());
            settle_waiting(waiting, &|_, _| true, &mut idle);
            assert_eq!(idle.placed[0].3, want, "waited, from {from:?}");
        }
        let Request::Place(place) = Request::parse(&place_text(text)).expect("a place") else {
            panic!("a place");
        };
        let mut mac = Pretend {
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
            &mut mac,
        );
        assert!(!mac.placed[0].3, "a note");
    }

    #[test]
    fn a_ticket_a_place_a_landing_and_the_answer() {
        let mut mac = Pretend {
            anchors: vec!["p4".to_owned()],
            ..Pretend::default()
        };
        // Named anchor, a side and a focus: the shell is asked for exactly that.
        let answer = serve(
            &server(),
            Some("p4"),
            &place_text("anchor p4\nside below\nratio 30\nfocus yes\n"),
            &mut mac,
        );
        let Answer::Reply(Reply::Placed(placed)) = answer else {
            panic!("{answer:?}");
        };
        assert_eq!(placed.landed, Landed::AsAsked);
        assert_eq!(placed.next_to.as_ref().map(Anchor::name), Some("p4"));
        assert_eq!(
            mac.placed,
            [("p4".to_owned(), Terminal(9), Some(Side::Below), true)]
        );
    }

    #[test]
    fn the_shell_is_told_who_asked_and_a_request_that_waits_remembers_it() {
        let mut mac = Pretend {
            anchors: vec!["p4".to_owned()],
            ..Pretend::default()
        };
        // Named anchor and a different sender: the sender is the opener.
        serve(&server(), Some("p9"), &place_text("anchor p4\n"), &mut mac);
        assert_eq!(mac.openers, [Some("p9".to_owned())]);
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
        settle_waiting(waiting, &|_, _| true, &mut idle);
        assert_eq!(idle.openers, [Some("p9".to_owned())]);
    }

    #[test]
    fn with_no_anchor_the_pane_goes_beside_the_terminal_the_ticket_names() {
        let made = format!("b-{}", "0123456789abcdef".repeat(2));
        let mut mac = Pretend {
            anchors: vec!["p4".to_owned()],
            ..Pretend::default()
        };
        // The terminal the Mac opened before the numbers existed is the caller too.
        let mut older = Pretend {
            anchors: vec![made.clone()],
            ..Pretend::default()
        };
        assert!(matches!(
            serve(&server(), Some("p4"), &place_text(""), &mut mac),
            Answer::Reply(Reply::Placed(_))
        ));
        assert_eq!(mac.placed[0].0, "p4");
        assert!(matches!(
            serve(&server(), Some(&made), &place_text(""), &mut older),
            Answer::Reply(Reply::Placed(_))
        ));
        // Neither a ticket's terminal nor a named anchor: a mistake in the request, and the
        // answer says so (the script ends the terminal it started for it).
        let Answer::Reply(Reply::Failed(failure)) =
            serve(&server(), None, &place_text(""), &mut mac)
        else {
            panic!("no anchor");
        };
        assert_eq!(failure.kind, Kind::Usage);
        assert_eq!(mac.placed.len(), 1, "nothing was placed for it");
    }

    #[test]
    fn a_terminal_that_has_a_pane_is_done_and_is_not_placed_twice() {
        let mut mac = Pretend {
            anchors: vec!["p4".to_owned()],
            shown: vec![Terminal(9)],
            ..Pretend::default()
        };
        let answer = serve(&server(), Some("p4"), &place_text("anchor p4\n"), &mut mac);
        let Answer::Reply(Reply::Placed(placed)) = answer else {
            panic!("{answer:?}");
        };
        assert_eq!(placed.landed, Landed::AsAsked);
        assert!(mac.placed.is_empty());
    }

    #[test]
    fn an_anchor_in_no_tab_waits_and_the_answer_says_so() {
        let mut mac = Pretend::default();
        let answer = serve(&server(), Some("p4"), &place_text("anchor p4\n"), &mut mac);
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
        queue.add(waiting);
        mac.anchors.push("p4".to_owned());
        let due = queue.take_beside("ada@rasp:22", "p4");
        assert_eq!(due.len(), 1);
        let (again, made) =
            settle_waiting(due.into_iter().next().expect("one"), &|_, _| true, &mut mac);
        assert!(again.is_none() && made);
        assert_eq!(mac.placed.len(), 1);
        assert!(queue.is_empty());
    }

    #[test]
    fn a_pane_that_cannot_be_made_leaves_the_terminal_in_the_list_and_keeps_nothing() {
        let mut mac = Pretend {
            unable: true,
            ..Pretend::default()
        };
        let answer = serve(&server(), Some("p4"), &place_text("anchor p4\n"), &mut mac);
        let Answer::Reply(Reply::Placed(placed)) = answer else {
            panic!("{answer:?}");
        };
        assert_eq!(placed.landed, Landed::Waiting);
        assert_eq!(placed.next_to, None, "nothing is kept to place it by");
    }

    #[test]
    fn a_request_that_is_wrong_is_answered_with_the_mistake() {
        let mut mac = Pretend::default();
        for text in [
            "",
            "hello\n",
            "bateri 1\nplace\n",
            "bateri 2\nplace\nterminal p9\n",
        ] {
            let Answer::Reply(Reply::Failed(_)) = serve(&server(), None, text, &mut mac) else {
                panic!("{text:?}");
            };
        }
        let Answer::Reply(Reply::Failed(failure)) =
            serve(&server(), None, "bateri 2\nplace\nterminal p9\n", &mut mac)
        else {
            panic!("a later version");
        };
        assert_eq!(failure.kind, Kind::Version);
        assert!(mac.placed.is_empty());
    }

    #[test]
    fn ls_close_and_notify_are_for_the_shell_to_carry_out() {
        let mut mac = Pretend::default();
        assert_eq!(
            serve(&server(), None, "bateri 1\nls\n", &mut mac),
            Answer::List(Format::Text)
        );
        assert_eq!(
            serve(&server(), None, "bateri 1\nls\nformat json\n", &mut mac),
            Answer::List(Format::Json)
        );
        assert_eq!(
            serve(&server(), None, "bateri 1\nclose\nterminal p3\n", &mut mac),
            Answer::Close(Anchor::of(Terminal(3)))
        );
        assert_eq!(
            serve(&server(), None, "bateri 1\nnotify\ntext hi\n", &mut mac),
            Answer::Notify("hi".to_owned())
        );
        // A target that is no name kablo has is no request.
        let Answer::Reply(Reply::Failed(failure)) =
            serve(&server(), None, "bateri 1\nclose\nterminal a b\n", &mut mac)
        else {
            panic!("not a terminal name");
        };
        assert_eq!(failure.kind, Kind::Usage);
        assert!(mac.placed.is_empty());
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
    fn a_terminal_is_closed_unasked_by_itself_and_by_the_one_that_opened_it_and_by_no_other() {
        let opener = Anchor::parse("p4");
        let made = format!("b-{}", "0123456789abcdef".repeat(2));
        // The opener, and the terminal itself (an agent ending the terminal it runs in).
        let seven = Anchor::of(Terminal(7));
        assert!(may_close(Some("p4"), &seven, opener.as_ref()));
        assert!(may_close(Some("p7"), &seven, None));
        // A terminal the Mac opened before the numbers existed is its own, whatever its name,
        // and is one that opened others.
        let older = Anchor::parse(&made).expect("a name");
        assert!(may_close(Some(&made), &older, None));
        assert!(may_close(Some(&made), &seven, Some(&older)));
        // Anybody else, a program that runs in no terminal, a terminal nobody opened.
        assert!(!may_close(Some("p5"), &seven, opener.as_ref()));
        assert!(!may_close(None, &seven, opener.as_ref()));
        assert!(!may_close(Some("p4"), &seven, None));
        assert!(!may_close(Some("p7"), &Anchor::of(Terminal(70)), None));
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
            },
        }
    }

    #[test]
    fn what_was_left_while_the_mac_was_away_is_placed_when_the_terminal_can_still_be_shown() {
        let mut mac = Pretend {
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
            &mut mac,
        );
        assert_eq!(read.placed, 2, "p5 and p7");
        assert_eq!(
            mac.placed.iter().map(|p| p.1).collect::<Vec<_>>(),
            [Terminal(5), Terminal(7)]
        );
        assert_eq!(read.waiting.len(), 1);
        assert_eq!(read.waiting[0].place.terminal, Terminal(8));
        assert_eq!(read.waiting[0].anchor.name(), "p1");
    }

    #[test]
    fn a_waiting_request_whose_terminal_is_gone_or_shown_is_dropped_and_one_whose_anchor_is_not_in_a_tab_yet_stays()
     {
        let mut mac = Pretend::default();
        let waiting = |terminal: u64| Waiting {
            server: server(),
            anchor: Anchor::parse("p4").expect("a name"),
            place: note(terminal, "p4", false).place,
            opener: None,
        };
        // Gone.
        assert_eq!(
            settle_waiting(waiting(9), &|_, _| false, &mut mac),
            (None, false)
        );
        // Shown by the user meanwhile.
        mac.shown.push(Terminal(9));
        assert_eq!(
            settle_waiting(waiting(9), &|_, _| true, &mut mac),
            (None, false)
        );
        // The anchor's pane is not in a tab yet: it stays.
        let (again, made) = settle_waiting(waiting(10), &|_, _| true, &mut mac);
        assert!(again.is_some() && !made);
    }
}
