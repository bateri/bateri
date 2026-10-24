//! The hub against the bridge of [`super::super::fake`], over bytes: a holder
//! speaks the wire of `ssh-hold` on one side, the fake server on the other, and
//! what each gets is looked at.

use std::time::Instant;

use kablo_proto::v1::Exit;

use super::*;
use crate::hold::wire::{HoldReader, Target};
use crate::kablo::fake::{Age, Bridge};
use crate::kablo::{Cause, Link, Size};

const SIZE: Size = Size {
    cols: 80,
    rows: 24,
    xpixel: 0,
    ypixel: 0,
};

/// What a picture is written after: not `RIS`, which would take the pane's
/// history with it, but the reset that keeps it, for the width of the terminal.
fn reset() -> Vec<u8> {
    bt_core::reset_keeping_history(SIZE.cols)
}

const WIDE: Size = Size {
    cols: 120,
    rows: 40,
    xpixel: 0,
    ypixel: 0,
};

const ACCOUNT: &str = "rasp";

fn id(name: &str) -> SessionId {
    SessionId::new(name).expect("a session id")
}

fn new_request(terminal: &str) -> Open {
    Open {
        terminal: id(terminal),
        size: SIZE,
        target: Target::New {
            args: vec![b"rasp".to_vec()],
        },
    }
}

fn resume_request(terminal: &str, offset: Option<u64>) -> Open {
    Open {
        terminal: id(terminal),
        size: SIZE,
        target: Target::Resume {
            account: ACCOUNT.into(),
            offset,
        },
    }
}

/// A hub with one client of `ACCOUNT`, the bridge it talks to, and the pipe
/// between them, stepped by hand; and holders, each a wire reader.
struct Rig {
    hub: Hub,
    bridge: Bridge,
    now: Instant,
    /// What each holder has been sent, in order.
    heard: Vec<(HoldId, Vec<ToHold>)>,
    readers: Vec<(HoldId, HoldReader)>,
    /// What the hub told the owner that was not a request.
    events: Vec<HubEvent>,
}

impl Rig {
    fn new() -> Rig {
        Rig::with(Bridge::new(Age::Current))
    }

    fn with(bridge: Bridge) -> Rig {
        let now = Instant::now();
        let mut hub = Hub::new();
        hub.add_client(ACCOUNT, Client::new(Some("MacBook"), now));
        let mut rig = Rig {
            hub,
            bridge,
            now,
            heard: Vec::new(),
            readers: Vec::new(),
            events: Vec::new(),
        };
        rig.connect();
        rig
    }

    fn client(&mut self) -> &mut Client {
        self.hub.client_mut(ACCOUNT).expect("the client")
    }

    fn connect(&mut self) {
        let now = self.now;
        self.client().connected(now);
        self.pump();
    }

    /// Moves bytes between the client and the bridge and between the hub and
    /// the holders until nothing moves.
    fn pump(&mut self) {
        loop {
            let mut moved = false;
            let out = self.client().take_outgoing();
            if !out.is_empty() {
                self.bridge.feed(&out);
                moved = true;
            }
            let back = self.bridge.take_output();
            if !back.is_empty() {
                let now = self.now;
                self.client().push(&back, now);
                moved = true;
            }
            self.hub.pump(ACCOUNT);
            let holds: Vec<HoldId> = self.readers.iter().map(|(hold, _)| *hold).collect();
            for hold in holds {
                let bytes = self.hub.take_outgoing(hold);
                if bytes.is_empty() {
                    continue;
                }
                moved = true;
                let at = self
                    .readers
                    .iter()
                    .position(|(h, _)| *h == hold)
                    .expect("a reader");
                self.readers[at].1.push(&bytes);
                while let Some(message) = self.readers[at].1.read().expect("whole frames") {
                    match self.heard.iter_mut().find(|(h, _)| *h == hold) {
                        Some((_, list)) => list.push(message),
                        None => self.heard.push((hold, vec![message])),
                    }
                }
            }
            if !moved {
                break;
            }
        }
    }

    /// A holder connects.
    fn hold(&mut self) -> HoldId {
        let hold = self.hub.open();
        self.readers.push((hold, HoldReader::new()));
        hold
    }

    /// The holder says `message`.
    fn say(&mut self, hold: HoldId, message: &ToApp) {
        let mut bytes = Vec::new();
        message.encode(&mut bytes);
        self.hub.push(hold, &bytes);
        self.pump();
    }

    fn greet(&mut self, hold: HoldId) {
        self.say(
            hold,
            &ToApp::Hello {
                newest: wire::VERSION,
                oldest: wire::OLDEST,
            },
        );
    }

    /// What the owner is asked, taken; everything else is kept.
    fn requests(&mut self) -> Vec<(HoldId, Open)> {
        let mut asked = Vec::new();
        while let Some(event) = self.hub.next_event() {
            match event {
                HubEvent::Wants { hold, open } => asked.push((hold, open)),
                other => self.events.push(other),
            }
        }
        asked
    }

    /// A holder greets and asks for a new terminal, which the owner starts.
    fn started(&mut self, terminal: &str) -> (HoldId, SessionId) {
        let hold = self.hold();
        self.greet(hold);
        self.say(hold, &ToApp::Open(new_request(terminal)));
        let asked = self.requests();
        assert_eq!(asked, vec![(hold, new_request(terminal))]);
        self.hub
            .start(hold, ACCOUNT, NewSession::default(), None)
            .expect("started");
        self.pump();
        (hold, id(terminal))
    }

    fn heard_by(&self, hold: HoldId) -> &[ToHold] {
        self.heard
            .iter()
            .find(|(h, _)| *h == hold)
            .map_or(&[], |(_, list)| list)
    }

    /// The bytes a holder was given as output, in order, and where the stream
    /// stood after the last.
    fn output(&self, hold: HoldId) -> (Vec<u8>, Option<u64>) {
        let mut bytes = Vec::new();
        let mut end = None;
        for message in self.heard_by(hold) {
            if let ToHold::Output { end: at, data } = message {
                bytes.extend_from_slice(data);
                end = Some(*at);
            }
        }
        (bytes, end)
    }

    fn drop_connection(&mut self) {
        self.bridge.disconnect();
        let now = self.now;
        self.client().lost(now, Cause::Dropped);
        self.pump();
    }

    fn redial(&mut self) {
        let Link::Retrying { at, .. } = self.client().link() else {
            panic!("not retrying");
        };
        self.now = self.now.max(at);
        self.connect();
    }
}

#[test]
fn a_new_terminal_is_bound_and_its_output_reaches_the_holder_exactly() {
    let mut rig = Rig::new();
    let (hold, terminal) = rig.started("b-1");
    assert_eq!(
        rig.heard_by(hold)[0],
        ToHold::Welcome {
            version: wire::VERSION
        }
    );
    // A holder of this version is told the name the terminal has.
    assert!(rig.heard_by(hold).contains(&ToHold::Bound {
        account: ACCOUNT.into(),
        terminal: Some(terminal.clone()),
    }));
    assert_eq!(rig.hub.terminal_of(hold), Some((ACCOUNT, &terminal)));
    assert_eq!(rig.hub.hold_of(ACCOUNT, &terminal), Some(hold));

    rig.bridge
        .write(&terminal, b"hello \x1b[31mworld\x1b[0m\r\n");
    rig.bridge
        .write(&terminal, &(0..=255u8).collect::<Vec<_>>());
    rig.pump();
    let (bytes, end) = rig.output(hold);
    let mut expected = b"hello \x1b[31mworld\x1b[0m\r\n".to_vec();
    expected.extend(0..=255u8);
    assert_eq!(bytes, expected);
    assert_eq!(
        end,
        Some(expected.len() as u64),
        "the stream stands after the bytes"
    );
}

#[test]
fn what_the_holder_types_and_the_pane_size_reach_the_program() {
    let mut rig = Rig::new();
    let (hold, terminal) = rig.started("b-1");
    rig.say(hold, &ToApp::Input(b"ls -la\r".to_vec()));
    rig.say(hold, &ToApp::Input(vec![0x03, 0x1b, b'[', b'A']));
    assert_eq!(rig.bridge.typed(&terminal), b"ls -la\r\x03\x1b[A");
    assert_eq!(
        rig.bridge.size_of(&terminal),
        Some(SIZE),
        "the size it asked with"
    );
    rig.say(hold, &ToApp::Resize(WIDE));
    assert_eq!(rig.bridge.size_of(&terminal), Some(WIDE));
}

#[test]
fn a_lost_connection_to_the_server_loses_nothing_and_repeats_nothing() {
    let mut rig = Rig::new();
    let (hold, terminal) = rig.started("b-1");
    rig.bridge.write(&terminal, b"one ");
    rig.pump();
    // The server's stream breaks, and the program writes while nobody listens.
    rig.drop_connection();
    rig.bridge.write(&terminal, b"two ");
    rig.redial();
    rig.bridge.write(&terminal, b"three");
    rig.pump();
    let (bytes, end) = rig.output(hold);
    assert_eq!(bytes, b"one two three");
    assert_eq!(end, Some(13));
    assert!(
        !rig.heard_by(hold)
            .iter()
            .any(|m| matches!(m, ToHold::End { .. })),
        "the holder is not told the connection was lost: it goes on when the stream does"
    );
}

#[test]
fn what_is_typed_while_the_server_is_away_is_dropped_with_a_bell() {
    let mut rig = Rig::new();
    let (hold, terminal) = rig.started("b-1");
    rig.say(hold, &ToApp::Input(b"a".to_vec()));
    rig.drop_connection();
    let before = rig.heard_by(hold).len();
    rig.say(hold, &ToApp::Input(b"b".to_vec()));
    assert_eq!(rig.heard_by(hold)[before..], [ToHold::Bell]);
    rig.redial();
    rig.pump();
    assert_eq!(
        rig.bridge.typed(&terminal),
        b"a",
        "a line typed while away is not sent when the connection comes back"
    );
}

#[test]
fn a_picture_replaces_the_bytes_the_server_no_longer_holds() {
    let mut rig = Rig::new();
    let (hold, terminal) = rig.started("b-1");
    rig.bridge.write(&terminal, b"0123456789");
    rig.pump();
    rig.drop_connection();
    rig.bridge.write(&terminal, b"abcdefghij");
    // The journal keeps only the last four bytes: what the holder missed is gone.
    rig.bridge.retain_last(&terminal, 4);
    rig.redial();
    rig.bridge.write(&terminal, b"!");
    rig.pump();

    let (bytes, end) = rig.output(hold);
    let picture = Bridge::picture_of(20);
    let mut expected = b"0123456789".to_vec();
    expected.extend_from_slice(&reset());
    expected.extend_from_slice(&picture);
    expected.push(b'!');
    assert_eq!(
        bytes, expected,
        "the screen is reset, then drawn, then the stream goes on"
    );
    assert_eq!(
        end,
        Some(21),
        "the stream stands after the picture and the byte after it"
    );

    // The stream's position moves only when the picture is whole: the frames
    // before the last piece say where the stream stood before.
    let ends: Vec<u64> = rig
        .heard_by(hold)
        .iter()
        .filter_map(|m| match m {
            ToHold::Output { end, .. } => Some(*end),
            _ => None,
        })
        .collect();
    assert_eq!(ends, [10, 10, 20, 21]);
}

#[test]
fn a_program_that_ends_ends_the_holder_with_its_status() {
    let mut rig = Rig::new();
    let (hold, terminal) = rig.started("b-1");
    rig.bridge.write(&terminal, b"bye\r\n");
    rig.bridge.exit(&terminal, Exit::Code(3));
    rig.pump();
    let heard = rig.heard_by(hold);
    assert_eq!(
        heard[heard.len() - 1],
        ToHold::End {
            code: 3,
            note: None
        }
    );
    assert_eq!(
        rig.output(hold).0,
        b"bye\r\n",
        "all of its output comes first"
    );
    assert!(rig.hub.is_finished(hold));
    assert_eq!(rig.hub.terminal_of(hold), None);

    let (second, terminal) = rig.started("b-2");
    rig.bridge.exit(&terminal, Exit::Signal(9));
    rig.pump();
    assert_eq!(
        rig.heard_by(second).last(),
        Some(&ToHold::End {
            code: 137,
            note: None
        })
    );
    assert!(rig.hub.is_finished(second));
}

#[test]
fn a_restarted_application_resumes_from_what_the_holder_wrote() {
    // The server outlives the first application: its terminal and its journal.
    let mut first = Rig::new();
    let (hold, terminal) = first.started("b-1");
    first.bridge.write(&terminal, b"abc");
    first.pump();
    let (_, end) = first.output(hold);
    assert_eq!(end, Some(3));

    // The application is gone; the program goes on writing. A new application
    // has a new client with no memory of the terminal.
    first.drop_connection();
    first.bridge.write(&terminal, b"def");
    let mut second = Rig::with(first.bridge);
    let hold = second.hold();
    second.greet(hold);
    second.say(hold, &ToApp::Open(resume_request("b-1", end)));
    assert_eq!(second.requests(), vec![(hold, resume_request("b-1", end))]);
    second.hub.resume(hold, ACCOUNT, false).expect("resumed");
    second.pump();
    second.bridge.write(&terminal, b"ghi");
    second.pump();
    let (bytes, end) = second.output(hold);
    assert_eq!(
        bytes, b"defghi",
        "exactly what the holder had not been given"
    );
    assert_eq!(end, Some(9));
}

/// The application numbers the terminals of a server: the holder asked with its own name and the
/// terminal is `t7`. The server has it under the number, the holder is told it, and the terminal
/// is found by it afterwards — the holder resumes by the name it was told.
#[test]
fn a_terminal_started_under_a_number_is_told_to_the_holder_and_found_by_it() {
    let mut rig = Rig::new();
    let hold = rig.hold();
    rig.greet(hold);
    rig.say(hold, &ToApp::Open(new_request("b-1")));
    assert_eq!(rig.requests(), vec![(hold, new_request("b-1"))]);
    rig.hub
        .start(hold, ACCOUNT, NewSession::default(), Some(id("t7")))
        .expect("started");
    rig.pump();
    assert!(rig.bridge.has(&id("t7")));
    assert!(
        !rig.bridge.has(&id("b-1")),
        "nothing is made under the holder's name"
    );
    assert_eq!(rig.hub.terminal_of(hold), Some((ACCOUNT, &id("t7"))));
    assert_eq!(rig.hub.hold_of(ACCOUNT, &id("t7")), Some(hold));
    assert_eq!(rig.hub.hold_of(ACCOUNT, &id("b-1")), None);
    assert!(rig.heard_by(hold).contains(&ToHold::Bound {
        account: ACCOUNT.into(),
        terminal: Some(id("t7")),
    }));
    rig.bridge.write(&id("t7"), b"abc");
    rig.pump();
    let (bytes, end) = rig.output(hold);
    assert_eq!(bytes, b"abc");

    // The application is gone and comes back: the holder asks for the name it was told.
    rig.drop_connection();
    rig.bridge.write(&id("t7"), b"def");
    let mut second = Rig::with(rig.bridge);
    let hold = second.hold();
    second.greet(hold);
    second.say(hold, &ToApp::Open(resume_request("t7", end)));
    assert_eq!(second.requests(), vec![(hold, resume_request("t7", end))]);
    second.hub.resume(hold, ACCOUNT, false).expect("resumed");
    second.pump();
    assert_eq!(second.output(hold).0, b"def");
}

/// A holder of version 2 reads no name in `Bound` (it would take the frame for a broken peer), so
/// it is sent the bytes it always was, and the terminal keeps the name it made.
#[test]
fn a_holder_that_cannot_be_told_a_name_is_sent_the_bound_it_always_was() {
    for version in [1, 2] {
        let mut rig = Rig::new();
        let hold = rig.hold();
        rig.say(
            hold,
            &ToApp::Hello {
                newest: version,
                oldest: 1,
            },
        );
        rig.say(hold, &ToApp::Open(new_request("b-1")));
        rig.requests();
        assert_eq!(rig.hub.version_of(hold), version);
        // A name given in spite of that is not used: the holder could not be told it, and a
        // terminal its holder does not know the name of is one it cannot find again.
        rig.hub
            .start(hold, ACCOUNT, NewSession::default(), Some(id("t7")))
            .expect("started");
        rig.pump();
        assert!(rig.bridge.has(&id("b-1")), "version {version}");
        assert!(!rig.bridge.has(&id("t7")), "version {version}");
        assert!(rig.heard_by(hold).contains(&ToHold::Bound {
            account: ACCOUNT.into(),
            terminal: None,
        }));
    }
}

/// A start that the client refuses leaves the request as it was asked, name included, so the
/// owner can answer it another way.
#[test]
fn a_start_that_is_refused_leaves_the_request_with_the_name_it_came_with() {
    let mut rig = Rig::new();
    let (_, terminal) = rig.started("b-1");
    let hold = rig.hold();
    rig.greet(hold);
    rig.say(hold, &ToApp::Open(new_request("b-2")));
    rig.requests();
    // `b-1` is in use on this client: the numbered start for b-2 under that name is refused.
    assert!(
        rig.hub
            .start(hold, ACCOUNT, NewSession::default(), Some(terminal))
            .is_err()
    );
    rig.hub
        .start(hold, ACCOUNT, NewSession::default(), None)
        .expect("the request is still there, as b-2");
    rig.pump();
    assert!(rig.bridge.has(&id("b-2")));
}

#[test]
fn a_holder_that_never_saw_the_terminal_is_shown_the_screen() {
    let mut bridge = Bridge::new(Age::Current);
    let terminal = bridge.add("b-1", SIZE);
    bridge.write(&terminal, b"already there");
    let mut rig = Rig::with(bridge);
    let hold = rig.hold();
    rig.greet(hold);
    rig.say(hold, &ToApp::Open(resume_request("b-1", None)));
    rig.requests();
    rig.hub.resume(hold, ACCOUNT, false).expect("resumed");
    rig.pump();
    rig.bridge.write(&terminal, b"!");
    rig.pump();
    let (bytes, end) = rig.output(hold);
    let mut expected = reset();
    expected.extend_from_slice(&Bridge::picture_of(13));
    expected.push(b'!');
    assert_eq!(bytes, expected);
    assert_eq!(end, Some(14));
    // The pane's scrollback is more than this terminal's: nothing that wipes
    // it (`RIS`, `ED 3`) comes before a picture.
    assert!(!bytes.windows(2).any(|pair| pair == b"\x1bc"));
    assert!(!bytes.windows(4).any(|pair| pair == b"\x1b[3J"));
}

/// A relay from before pictures cannot make the one a holder with no position asks for: it goes
/// on with bytes from where the terminal stands. The holder is left with the position of those
/// bytes, not with the count of what it was given — it asks for the place it keeps after the next
/// restart, and a place counted from zero is the wrong one.
#[test]
fn a_relay_that_cannot_make_a_picture_leaves_the_holder_the_position_of_its_bytes() {
    let mut bridge = Bridge::new(Age::Old);
    let terminal = bridge.add("b-1", SIZE);
    bridge.write(&terminal, b"already there");
    let mut rig = Rig::with(bridge);
    let hold = rig.hold();
    rig.greet(hold);
    rig.say(hold, &ToApp::Open(resume_request("b-1", None)));
    rig.requests();
    rig.hub.resume(hold, ACCOUNT, false).expect("resumed");
    rig.pump();
    rig.bridge.write(&terminal, b"!?");
    rig.pump();
    let (bytes, end) = rig.output(hold);
    assert_eq!(bytes, b"!?", "no picture, and no reset for one");
    assert_eq!(end, Some(15), "13 bytes were there, two came");
}

/// The holder asked for a terminal it had shown before (to the bateri that is gone): whatever
/// the connection to the server is doing, what its pane shows is a screen, and a screen that
/// cannot be brought up to date is a passive one — not a first connection still being made.
#[test]
fn a_holder_that_asks_for_a_terminal_again_counts_as_shown_before_the_server_answers() {
    let mut bridge = Bridge::new(Age::Current);
    bridge.add("b-1", SIZE);
    let mut rig = Rig::with(bridge);
    rig.drop_connection();
    let hold = rig.hold();
    rig.greet(hold);
    rig.say(hold, &ToApp::Open(resume_request("b-1", Some(0))));
    rig.requests();
    rig.hub.resume(hold, ACCOUNT, false).expect("resumed");
    assert!(rig.hub.is_shown(hold));
    // A new terminal is not: its pane shows the user's own `ssh` until the terminal is up.
    let other = rig.hold();
    rig.greet(other);
    rig.say(other, &ToApp::Open(new_request("b-2")));
    rig.requests();
    rig.hub
        .start(other, ACCOUNT, NewSession::default(), None)
        .expect("started");
    assert!(!rig.hub.is_shown(other));
}

/// A terminal asked for again that the server no longer has — it ended while bateri was closed
/// and the server forgot it — ends the holder with one line that says so, not with the
/// server's words, and the pane that showed it is left with a shell.
#[test]
fn a_terminal_the_server_no_longer_has_ends_the_holder_that_asks_for_it_again_with_a_line() {
    let mut rig = Rig::new();
    let hold = rig.hold();
    rig.greet(hold);
    rig.say(hold, &ToApp::Open(resume_request("b-gone", Some(5))));
    rig.requests();
    rig.hub.resume(hold, ACCOUNT, false).expect("resumed");
    rig.pump();
    assert_eq!(
        rig.heard_by(hold).last(),
        Some(&ToHold::End {
            code: 1,
            note: Some("bateri: the terminal that ran here is no longer on the server".into()),
        })
    );
    assert!(rig.hub.is_finished(hold));
}

#[test]
fn a_holder_that_goes_lets_the_terminal_live_on() {
    let mut rig = Rig::new();
    let (hold, terminal) = rig.started("b-1");
    assert!(rig.bridge.is_attached(&terminal));
    rig.hub.closed(hold);
    rig.pump();
    assert!(!rig.bridge.is_attached(&terminal), "the stream stops");
    assert!(rig.bridge.has(&terminal), "the terminal does not");
    assert_eq!(rig.hub.hold_of(ACCOUNT, &terminal), None);
    assert!(rig.requests().is_empty());
    // The owner is told which terminal the holder showed: this Mac was attached to it a moment
    // ago, and what the server counted of who is attached has it in.
    assert!(rig.events.iter().any(|e| matches!(
        e,
        HubEvent::Gone { hold: h, showed: Some((account, shown)) }
            if *h == hold && account == ACCOUNT && *shown == terminal
    )));
    // A later write finds nobody to give it to.
    rig.bridge.write(&terminal, b"nobody");
    rig.pump();
}

#[test]
fn the_newer_holder_of_a_terminal_wins() {
    let mut rig = Rig::new();
    let (old, terminal) = rig.started("b-1");
    rig.bridge.write(&terminal, b"abc");
    rig.pump();

    let new = rig.hold();
    rig.greet(new);
    rig.say(new, &ToApp::Open(resume_request("b-1", Some(3))));
    rig.requests();
    rig.hub.resume(new, ACCOUNT, false).expect("resumed");
    rig.pump();
    assert!(matches!(
        rig.heard_by(old).last(),
        Some(ToHold::End {
            code: 1,
            note: Some(_)
        })
    ));
    assert_eq!(rig.hub.hold_of(ACCOUNT, &terminal), Some(new));
    rig.bridge.write(&terminal, b"def");
    rig.pump();
    assert_eq!(rig.output(new).0, b"def");
    // The old holder going away does not let go of the new one's terminal.
    rig.hub.closed(old);
    rig.pump();
    assert_eq!(rig.hub.hold_of(ACCOUNT, &terminal), Some(new));
    assert!(rig.bridge.is_attached(&terminal));
}

#[test]
fn the_versions_are_agreed_or_refused() {
    let mut rig = Rig::new();
    for (newest, oldest, expected) in [
        (1, 1, Some(1)),
        (2, 1, Some(2)),
        (3, 1, Some(3)),
        (7, 1, Some(3)),
        (9, 4, None),
        (0, 0, None),
    ] {
        let hold = rig.hold();
        rig.say(hold, &ToApp::Hello { newest, oldest });
        let reply = &rig.heard_by(hold)[0];
        match expected {
            Some(version) => assert_eq!(reply, &ToHold::Welcome { version }),
            None => {
                assert!(
                    matches!(
                        reply,
                        ToHold::Refused {
                            fall_back: true,
                            ..
                        }
                    ),
                    "{reply:?}"
                );
                assert!(rig.hub.is_finished(hold));
            }
        }
    }
}

#[test]
fn a_holder_that_speaks_out_of_turn_or_garbles_the_wire_is_ended() {
    let mut rig = Rig::new();
    // A request before the greeting.
    let hold = rig.hold();
    rig.say(hold, &ToApp::Open(new_request("b-1")));
    assert!(matches!(
        rig.heard_by(hold).last(),
        Some(ToHold::Refused {
            fall_back: true,
            ..
        })
    ));
    assert!(rig.requests().is_empty(), "the owner is not asked");

    // Bytes that are not frames of this protocol.
    let hold = rig.hold();
    rig.hub.push(hold, &[0x03, 0xff, 0xff, 0xff, 0xff]);
    rig.pump();
    assert!(matches!(
        rig.heard_by(hold).last(),
        Some(ToHold::Refused {
            fall_back: true,
            ..
        })
    ));
    // Nothing more is read from it.
    rig.say(hold, &ToApp::Input(b"x".to_vec()));
    assert_eq!(rig.heard_by(hold).len(), 1);

    // Two requests.
    let (second, _) = rig.started("b-2");
    rig.say(second, &ToApp::Open(new_request("b-3")));
    assert!(matches!(
        rig.heard_by(second).last(),
        Some(ToHold::End { code: 1, .. })
    ));
}

#[test]
fn the_owners_answers_that_do_not_fit_are_declined_and_change_nothing() {
    let mut rig = Rig::new();
    let hold = rig.hold();
    rig.greet(hold);
    assert_eq!(
        rig.hub.start(hold, ACCOUNT, NewSession::default(), None),
        Err(Declined::NoRequest)
    );
    rig.say(hold, &ToApp::Open(resume_request("b-1", Some(0))));
    rig.requests();
    assert_eq!(
        rig.hub.start(hold, ACCOUNT, NewSession::default(), None),
        Err(Declined::WrongKind)
    );
    assert_eq!(
        rig.hub.resume(hold, "nowhere", false),
        Err(Declined::NoClient)
    );
    // The request is still there to be answered.
    rig.hub.refuse(hold, true, "no kablo here");
    rig.pump();
    assert_eq!(
        rig.heard_by(hold).last(),
        Some(&ToHold::Refused {
            fall_back: true,
            text: "no kablo here".into()
        })
    );
    assert!(rig.hub.is_finished(hold));
    assert_eq!(
        rig.hub.resume(hold, ACCOUNT, false),
        Err(Declined::NoRequest)
    );
}

#[test]
fn a_terminal_the_server_will_not_make_falls_back_before_it_was_shown() {
    let mut bridge = Bridge::new(Age::Current);
    bridge.add("b-1", SIZE);
    let mut rig = Rig::with(bridge);
    // The name is taken on the server: `new` fails and nothing was shown.
    let hold = rig.hold();
    rig.greet(hold);
    rig.say(hold, &ToApp::Open(new_request("b-1")));
    rig.requests();
    rig.hub
        .start(hold, ACCOUNT, NewSession::default(), None)
        .expect("asked");
    rig.pump();
    assert!(matches!(
        rig.heard_by(hold).last(),
        Some(ToHold::Refused {
            fall_back: true,
            ..
        })
    ));
    assert_eq!(rig.hub.terminal_of(hold), None);
}

/// What the holder's pane shows after another computer took the terminal is the pane's: its last
/// screen, dimmed, until ⏎ takes the terminal back. The holder is not ended (it used to be, with a
/// line, and the prompt came back under the screen).
#[test]
fn a_terminal_taken_by_another_computer_leaves_the_holder_bound_and_says_by_whom() {
    let mut rig = Rig::new();
    let (hold, terminal) = rig.started("b-1");
    rig.bridge.write(&terminal, b"abc");
    rig.pump();
    let heard = rig.heard_by(hold).len();
    rig.bridge.take_by(&terminal, "iMac");
    rig.pump();
    assert_eq!(
        rig.heard_by(hold).len(),
        heard,
        "the holder is told nothing"
    );
    assert!(
        !rig.heard_by(hold)
            .iter()
            .any(|message| matches!(message, ToHold::End { .. })),
        "and is not ended"
    );
    assert_eq!(rig.hub.terminal_of(hold), Some((ACCOUNT, &terminal)));
    assert!(rig.client().is_taken(&terminal));
    assert_eq!(rig.client().taken_by(&terminal), Some("iMac"));
    // The owner hears of it, once.
    rig.requests();
    assert!(rig.events.iter().any(|event| matches!(
        event,
        HubEvent::Client { account, event }
            if account == ACCOUNT
                && matches!(&**event, Event::Taken { by: Some(by), .. } if by == "iMac")
    )));
    // What is typed into the dimmed pane goes nowhere.
    assert!(!rig.client().input(&terminal, b"x"));
}

/// ⏎ in the dimmed pane takes the terminal back: attached again with the take bit, from the byte
/// the pane was owed, under the same name, and what the program wrote meanwhile arrives.
#[test]
fn a_terminal_taken_back_is_attached_taking_from_where_the_pane_left_off() {
    let mut rig = Rig::new();
    let (hold, terminal) = rig.started("b-1");
    rig.bridge.write(&terminal, b"abc");
    rig.pump();
    rig.bridge.take_by(&terminal, "iMac");
    rig.pump();
    // The program writes while the other computer has it.
    rig.bridge.write(&terminal, b"def");
    assert!(rig.client().take_back(&terminal));
    rig.pump();
    let attaches = rig.bridge.attaches(&terminal);
    let (offset, view) = attaches.last().expect("an attach");
    assert_eq!(*offset, 3, "from the byte owed");
    let view = view.expect("a view");
    assert!(view.take, "taking");
    assert_eq!(
        view.name.map(|name| name.to_string()),
        Some("MacBook".into())
    );
    assert!(!rig.client().is_taken(&terminal));
    let (bytes, end) = rig.output(hold);
    assert_eq!(bytes, b"abcdef");
    assert_eq!(end, Some(6));
    // A terminal that was not taken is nothing to take back.
    assert!(!rig.client().take_back(&terminal));
}

/// A pane that comes back to a terminal it showed (an application that was restarted) while
/// another computer has the terminal by now is not attached beside it: it keeps its place, bound
/// and dimmed, and ⏎ takes the terminal back. A holder that never saw the terminal opens it as
/// before.
#[test]
fn a_pane_that_comes_back_to_a_terminal_another_computer_has_is_dimmed_and_not_attached() {
    let mut bridge = Bridge::new(Age::Current);
    let terminal = bridge.add("b-1", SIZE);
    bridge.set_others(&terminal, &[Some("iMac")]);
    let mut rig = Rig::with(bridge);
    let hold = rig.hold();
    rig.greet(hold);
    rig.say(hold, &ToApp::Open(resume_request("b-1", Some(0))));
    rig.requests();
    rig.hub.resume(hold, ACCOUNT, false).expect("resumed");
    rig.pump();
    assert!(rig.client().is_taken(&terminal));
    assert_eq!(rig.client().taken_by(&terminal), Some("iMac"));
    assert!(
        rig.bridge.attaches(&terminal).is_empty(),
        "nothing attached beside it"
    );
    assert_eq!(rig.hub.terminal_of(hold), Some((ACCOUNT, &terminal)));
    assert!(
        !rig.heard_by(hold)
            .iter()
            .any(|message| matches!(message, ToHold::End { .. })),
        "the pane is not ended"
    );
    // ⏎: taken back from where the pane was.
    assert!(rig.client().take_back(&terminal));
    rig.pump();
    assert!(rig.client().is_attached(&terminal));
    assert!(rig.bridge.others_of(&terminal).is_empty());
}

/// A terminal moved here from another computer: the pane's first attach takes it, and the clients
/// the server had attached are let go; opened the usual way it takes nothing.
#[test]
fn a_resumed_terminal_is_taken_only_when_the_owner_says_so() {
    let mut bridge = Bridge::new(Age::Current);
    let terminal = bridge.add("b-1", SIZE);
    bridge.set_others(&terminal, &[Some("iMac")]);
    let mut rig = Rig::with(bridge);
    let hold = rig.hold();
    rig.greet(hold);
    rig.say(hold, &ToApp::Open(resume_request("b-1", None)));
    rig.requests();
    rig.hub.resume(hold, ACCOUNT, false).expect("resumed");
    rig.pump();
    let attaches = rig.bridge.attaches(&terminal);
    assert!(
        !attaches
            .last()
            .and_then(|(_, view)| *view)
            .expect("a view")
            .take
    );
    assert_eq!(
        rig.bridge.others_of(&terminal),
        vec![Some("iMac".to_owned())]
    );

    let second = rig.hold();
    rig.greet(second);
    rig.say(second, &ToApp::Open(resume_request("b-1", None)));
    rig.requests();
    rig.hub.resume(second, ACCOUNT, true).expect("resumed");
    rig.pump();
    let attaches = rig.bridge.attaches(&terminal);
    assert!(
        attaches
            .last()
            .and_then(|(_, view)| *view)
            .expect("a view")
            .take
    );
    assert!(
        rig.bridge.others_of(&terminal).is_empty(),
        "the other client was let go"
    );
}

#[test]
fn what_a_client_says_that_is_not_the_holders_goes_to_the_owner() {
    let mut rig = Rig::new();
    let (_, terminal) = rig.started("b-1");
    rig.requests();
    rig.bridge.write(&terminal, b"x");
    rig.pump();
    rig.drop_connection();
    rig.requests();
    assert!(rig.events.iter().any(|e| matches!(
        e,
        HubEvent::Client { event, account }
            if account == ACCOUNT && matches!(**event, Event::Lost { .. })
    )));
    assert!(
        !rig.events.iter().any(|e| matches!(
            e,
            HubEvent::Client { event, .. } if matches!(**event, Event::Output { .. })
        )),
        "output went to the holder, not the owner"
    );
}

#[test]
fn the_owner_can_have_a_program_run_on_the_pane_before_the_terminal_exists() {
    let mut rig = Rig::new();
    let hold = rig.hold();
    rig.greet(hold);
    rig.say(hold, &ToApp::Open(new_request("b-1")));
    assert_eq!(rig.requests(), vec![(hold, new_request("b-1"))]);

    rig.hub.say(hold, "bateri: set up");
    let argv = vec![b"ssh".to_vec(), b"-M".to_vec(), b"rasp".to_vec()];
    rig.hub.run(hold, &argv).expect("asked");
    rig.pump();
    assert!(rig.heard_by(hold).contains(&ToHold::Note {
        text: "bateri: set up".into()
    }));
    assert!(
        rig.heard_by(hold)
            .contains(&ToHold::Run { argv: argv.clone() })
    );

    // The request waits: no answer while the program runs, and no second one.
    assert_eq!(
        rig.hub.start(hold, ACCOUNT, NewSession::default(), None),
        Err(Declined::Running)
    );
    assert_eq!(rig.hub.run(hold, &argv), Err(Declined::Running));

    // Its end is told, with the status, and the owner answers as before.
    rig.say(hold, &ToApp::Ran { status: 0 });
    assert!(rig.requests().is_empty());
    assert!(rig.events.contains(&HubEvent::Ran { hold, status: 0 }));
    rig.hub
        .start(hold, ACCOUNT, NewSession::default(), None)
        .expect("started");
    rig.pump();
    assert_eq!(rig.hub.terminal_of(hold).map(|(_, t)| t), Some(&id("b-1")));
}

#[test]
fn a_holder_of_version_one_is_asked_nothing_and_told_nothing() {
    let mut rig = Rig::new();
    let hold = rig.hold();
    rig.say(
        hold,
        &ToApp::Hello {
            newest: 1,
            oldest: 1,
        },
    );
    assert_eq!(rig.hub.version_of(hold), 1);
    rig.say(hold, &ToApp::Open(new_request("b-1")));
    assert_eq!(rig.requests().len(), 1);
    assert_eq!(
        rig.hub.run(hold, &[b"ssh".to_vec()]),
        Err(Declined::Unsupported)
    );
    assert!(!rig.hub.say(hold, "a line"));
    rig.pump();
    assert!(
        !rig.heard_by(hold)
            .iter()
            .any(|m| matches!(m, ToHold::Run { .. } | ToHold::Note { .. }))
    );
}

#[test]
fn an_answer_nobody_asked_for_is_a_holder_out_of_turn_and_a_resume_is_not_run() {
    let mut rig = Rig::new();
    let hold = rig.hold();
    rig.greet(hold);
    rig.say(hold, &ToApp::Ran { status: 0 });
    assert!(rig.hub.is_finished(hold), "a Ran nobody asked for");

    let hold = rig.hold();
    rig.greet(hold);
    rig.say(hold, &ToApp::Open(resume_request("b-2", Some(0))));
    assert_eq!(rig.requests().len(), 1);
    assert_eq!(
        rig.hub.run(hold, &[b"ssh".to_vec()]),
        Err(Declined::WrongKind)
    );
}

// ─── a raw holder: the pane itself ───────────────────────────────────────

#[test]
fn a_raw_holder_is_sent_the_words_of_the_wire_as_the_bytes_that_show_them() {
    let mut hub = Hub::new();
    let hold = hub.open_raw();
    // It is bound to nothing yet: what it types has nowhere to go, and it hears a bell.
    hub.push(hold, b"ls\r");
    assert_eq!(hub.take_outgoing(hold), [0x07]);

    // A line the hub has for the pane is shown on the pane; an end is the status of the
    // program, with the note first.
    assert!(hub.say(hold, "bateri: a line"));
    assert_eq!(hub.take_outgoing(hold), b"bateri: a line\r\n");
    hub.end(hold, 3, Some("bateri: over"));
    assert_eq!(hub.take_outgoing(hold), b"\r\nbateri: over\r\n");
    assert_eq!(hub.exit_of(hold), Some(Exit::Code(3)));
    assert!(hub.is_finished(hold));
    // A raw holder is never greeted, so it never asks out of turn.
    assert_eq!(hub.version_of(hold), wire::VERSION);
}
