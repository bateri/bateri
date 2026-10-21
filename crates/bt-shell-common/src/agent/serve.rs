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
//!   a timed run): the same answer, and nothing is kept — the terminal is in the list.
//! - The terminal already has a pane: the request is done ([`Landed::AsAsked`]), not an error
//!   — an error would end a terminal the user is looking at.
//!
//! A request this version does not answer (`ls`, `close`, `notify`) is [`Answer::Pass`]: the
//! conversation is hung up on and the script does what it does when there is no Mac.

use super::message::{Failure, Kind, Place, Placed, Reply, Request};
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
    /// born without the keyboard unless `place.focus` says so, and a zoomed tab keeps its zoom.
    fn place(&mut self, server: &Server, anchor: &Anchor, place: &Place) -> Placing;
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
    /// Not answered: the conversation is hung up on.
    Pass,
}

fn waiting_reply(anchor: &Anchor) -> Reply {
    Reply::Placed(Placed {
        landed: Landed::Waiting,
        next_to: Some(anchor.clone()),
        squeezed: false,
        hidden: false,
    })
}

/// The answer to a letter: `text` is what the program said, `from` the terminal kablo says it
/// runs in (never what it says of itself), `server` the server it came on.
pub fn serve(server: &Server, from: Option<&str>, text: &str, shell: &mut dyn Shell) -> Answer {
    match Request::parse(text) {
        Err(failure) => Answer::Reply(Reply::Failed(failure)),
        Ok(Request::Place(place)) => place_for(server, from, place, shell),
        Ok(Request::Ls(_) | Request::Close(_) | Request::Notify(_)) => Answer::Pass,
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
    match shell.place(server, &anchor, &place) {
        Placing::Done(placed) => Answer::Reply(Reply::Placed(placed)),
        Placing::NoAnchor => {
            let reply = waiting_reply(&anchor);
            Answer::Wait(
                reply,
                Waiting {
                    server: server.clone(),
                    anchor,
                    place,
                },
            )
        }
        Placing::Unable => Answer::Reply(waiting_reply(&anchor)),
    }
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
        match shell.place(server, &anchor, &place) {
            Placing::Done(_) => read.placed += 1,
            Placing::NoAnchor => read.waiting.push(Waiting {
                server: server.clone(),
                anchor,
                place,
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
    } = waiting;
    if !alive(place.terminal, place.keep_open) || shell.shows(&server, place.terminal) {
        return (None, false);
    }
    match shell.place(&server, &anchor, &place) {
        Placing::Done(_) => (None, true),
        Placing::NoAnchor => (
            Some(Waiting {
                server,
                anchor,
                place,
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
    }

    impl Shell for Pretend {
        fn shows(&self, _: &Server, terminal: Terminal) -> bool {
            self.shown.contains(&terminal)
        }

        fn place(&mut self, _: &Server, anchor: &Anchor, place: &Place) -> Placing {
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
        format!("bateri 1\nplace\nterminal t9\n{extra}")
    }

    #[test]
    fn a_ticket_a_place_a_landing_and_the_answer() {
        let mut mac = Pretend {
            anchors: vec!["t4".to_owned()],
            ..Pretend::default()
        };
        // Named anchor, a side and a focus: the shell is asked for exactly that.
        let answer = serve(
            &server(),
            Some("t4"),
            &place_text("anchor t4\nside below\nratio 30\nfocus yes\n"),
            &mut mac,
        );
        let Answer::Reply(Reply::Placed(placed)) = answer else {
            panic!("{answer:?}");
        };
        assert_eq!(placed.landed, Landed::AsAsked);
        assert_eq!(placed.next_to.as_ref().map(Anchor::name), Some("t4"));
        assert_eq!(
            mac.placed,
            [("t4".to_owned(), Terminal(9), Some(Side::Below), true)]
        );
    }

    #[test]
    fn with_no_anchor_the_pane_goes_beside_the_terminal_the_ticket_names() {
        let made = format!("b-{}", "0123456789abcdef".repeat(2));
        let mut mac = Pretend {
            anchors: vec!["t4".to_owned()],
            ..Pretend::default()
        };
        // The terminal the Mac opened before the numbers existed is the caller too.
        let mut older = Pretend {
            anchors: vec![made.clone()],
            ..Pretend::default()
        };
        assert!(matches!(
            serve(&server(), Some("t4"), &place_text(""), &mut mac),
            Answer::Reply(Reply::Placed(_))
        ));
        assert_eq!(mac.placed[0].0, "t4");
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
            anchors: vec!["t4".to_owned()],
            shown: vec![Terminal(9)],
            ..Pretend::default()
        };
        let answer = serve(&server(), Some("t4"), &place_text("anchor t4\n"), &mut mac);
        let Answer::Reply(Reply::Placed(placed)) = answer else {
            panic!("{answer:?}");
        };
        assert_eq!(placed.landed, Landed::AsAsked);
        assert!(mac.placed.is_empty());
    }

    #[test]
    fn an_anchor_in_no_tab_waits_and_the_answer_says_so() {
        let mut mac = Pretend::default();
        let answer = serve(&server(), Some("t4"), &place_text("anchor t4\n"), &mut mac);
        let Answer::Wait(Reply::Placed(placed), waiting) = answer else {
            panic!("{answer:?}");
        };
        assert_eq!(placed.landed, Landed::Waiting);
        assert_eq!(placed.next_to.as_ref().map(Anchor::name), Some("t4"));
        assert_eq!(waiting.anchor.name(), "t4");
        assert_eq!(waiting.place.terminal, Terminal(9));
        assert_eq!(waiting.server, server());

        // The anchor gets a pane; the queue hands the request back and it is placed.
        let mut queue = Queue::default();
        queue.add(waiting);
        mac.anchors.push("t4".to_owned());
        let due = queue.take_beside("ada@rasp:22", "t4");
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
        let answer = serve(&server(), Some("t4"), &place_text("anchor t4\n"), &mut mac);
        let Answer::Reply(Reply::Placed(placed)) = answer else {
            panic!("{answer:?}");
        };
        assert_eq!(placed.landed, Landed::Waiting);
    }

    #[test]
    fn a_request_that_is_wrong_is_answered_with_the_mistake_and_the_rest_is_passed() {
        let mut mac = Pretend::default();
        for text in [
            "",
            "hello\n",
            "bateri 1\nplace\n",
            "bateri 2\nplace\nterminal t9\n",
        ] {
            let Answer::Reply(Reply::Failed(_)) = serve(&server(), None, text, &mut mac) else {
                panic!("{text:?}");
            };
        }
        let Answer::Reply(Reply::Failed(failure)) =
            serve(&server(), None, "bateri 2\nplace\nterminal t9\n", &mut mac)
        else {
            panic!("a later version");
        };
        assert_eq!(failure.kind, Kind::Version);
        for text in [
            "bateri 1\nls\n",
            "bateri 1\nclose\nterminal t3\n",
            "bateri 1\nnotify\ntext hi\n",
        ] {
            assert_eq!(
                serve(&server(), None, text, &mut mac),
                Answer::Pass,
                "{text:?}"
            );
        }
        assert!(mac.placed.is_empty());
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
            anchors: vec!["t4".to_owned()],
            shown: vec![Terminal(3)],
            ..Pretend::default()
        };
        // t5 runs, t6 ended and was not asked to stay, t7 ended and was, t3 has a pane, t8's
        // anchor is in no tab.
        let alive = |terminal: Terminal, keep_open: bool| match terminal.0 {
            5 | 3 | 8 => true,
            7 => keep_open,
            _ => false,
        };
        let read = settle_notes(
            &server(),
            vec![
                note(5, "t4", false),
                note(6, "t4", false),
                note(7, "t4", true),
                note(3, "t4", false),
                note(8, "t1", false),
            ],
            &alive,
            &mut mac,
        );
        assert_eq!(read.placed, 2, "t5 and t7");
        assert_eq!(
            mac.placed.iter().map(|p| p.1).collect::<Vec<_>>(),
            [Terminal(5), Terminal(7)]
        );
        assert_eq!(read.waiting.len(), 1);
        assert_eq!(read.waiting[0].place.terminal, Terminal(8));
        assert_eq!(read.waiting[0].anchor.name(), "t1");
    }

    #[test]
    fn a_waiting_request_whose_terminal_is_gone_or_shown_is_dropped_and_one_whose_anchor_is_not_in_a_tab_yet_stays()
     {
        let mut mac = Pretend::default();
        let waiting = |terminal: u64| Waiting {
            server: server(),
            anchor: Anchor::parse("t4").expect("a name"),
            place: note(terminal, "t4", false).place,
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
