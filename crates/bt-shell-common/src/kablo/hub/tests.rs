//! The hub against the bridge of [`super::super::fake`], over bytes: a holder
//! speaks the wire of `ssh-hold` on one side, the fake server on the other, and
//! what each gets is looked at.

use std::time::Instant;

use kablo_proto::v1::Exit;

use super::*;
use crate::hold::wire::{HoldReader, RESET, Target};
use crate::kablo::fake::{Age, Bridge};
use crate::kablo::{Cause, Link, Size};

const SIZE: Size = Size {
    cols: 80,
    rows: 24,
    xpixel: 0,
    ypixel: 0,
};

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
            .start(hold, ACCOUNT, NewSession::default())
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
    assert_eq!(rig.heard_by(hold)[0], ToHold::Welcome { version: 1 });
    assert!(rig.heard_by(hold).contains(&ToHold::Bound {
        account: ACCOUNT.into()
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
    expected.extend_from_slice(RESET);
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
    second.hub.resume(hold, ACCOUNT).expect("resumed");
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
    rig.hub.resume(hold, ACCOUNT).expect("resumed");
    rig.pump();
    rig.bridge.write(&terminal, b"!");
    rig.pump();
    let (bytes, end) = rig.output(hold);
    let mut expected = RESET.to_vec();
    expected.extend_from_slice(&Bridge::picture_of(13));
    expected.push(b'!');
    assert_eq!(bytes, expected);
    assert_eq!(end, Some(14));
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
    assert!(
        rig.events
            .iter()
            .any(|e| matches!(e, HubEvent::Gone { hold: h } if *h == hold))
    );
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
    rig.hub.resume(new, ACCOUNT).expect("resumed");
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
    for (newest, oldest, expected) in [(1, 1, Some(1)), (7, 1, Some(1)), (9, 2, None), (0, 0, None)]
    {
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
        rig.hub.start(hold, ACCOUNT, NewSession::default()),
        Err(Declined::NoRequest)
    );
    rig.say(hold, &ToApp::Open(resume_request("b-1", Some(0))));
    rig.requests();
    assert_eq!(
        rig.hub.start(hold, ACCOUNT, NewSession::default()),
        Err(Declined::WrongKind)
    );
    assert_eq!(rig.hub.resume(hold, "nowhere"), Err(Declined::NoClient));
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
    assert_eq!(rig.hub.resume(hold, ACCOUNT), Err(Declined::NoRequest));
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
        .start(hold, ACCOUNT, NewSession::default())
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

#[test]
fn a_terminal_taken_by_another_computer_ends_the_holder_with_a_line() {
    let mut rig = Rig::new();
    let (hold, terminal) = rig.started("b-1");
    rig.bridge.take_by(&terminal, "iMac");
    rig.pump();
    assert_eq!(
        rig.heard_by(hold).last(),
        Some(&ToHold::End {
            code: 1,
            note: Some("bateri: this terminal was taken by iMac".into())
        })
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
