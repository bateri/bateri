//! The client against the bridge of [`super::fake`], over bytes: what the
//! client puts on the wire, and what it gives the owner for what comes back.

use std::time::{Duration, Instant};

use kablo_proto::v1::{MAX_HISTORY, Message, Packet, PacketDecoder, View};

use super::fake::{Age, Bridge};
use super::*;

const SIZE: Size = Size {
    cols: 80,
    rows: 24,
    xpixel: 0,
    ypixel: 0,
};

const WIDE: Size = Size {
    cols: 100,
    rows: 30,
    xpixel: 0,
    ypixel: 0,
};

fn id(name: &str) -> SessionId {
    SessionId::new(name).expect("a session id")
}

fn secs(n: u64) -> Duration {
    Duration::from_secs(n)
}

/// A client and a bridge with the pipe between them, stepped by hand.
struct Rig {
    client: Client,
    bridge: Bridge,
    now: Instant,
    events: Vec<Event>,
}

impl Rig {
    fn new(age: Age) -> Rig {
        Rig::with(Bridge::new(age))
    }

    fn with(bridge: Bridge) -> Rig {
        let now = Instant::now();
        Rig {
            client: Client::new(Some("MacBook"), now),
            bridge,
            now,
            events: Vec::new(),
        }
    }

    /// Dials, and carries the handshake.
    fn connect(&mut self) {
        assert!(self.client.dial_due(self.now), "{:?}", self.client.link());
        self.client.connected(self.now);
        self.pump();
    }

    /// Moves bytes both ways until nothing moves, keeping the events.
    fn pump(&mut self) {
        loop {
            let mut moved = false;
            let out = self.client.take_outgoing();
            if !out.is_empty() {
                self.bridge.feed(&out);
                moved = true;
            }
            let back = self.bridge.take_output();
            if !back.is_empty() {
                self.client.push(&back, self.now);
                moved = true;
            }
            self.drain();
            if !moved {
                break;
            }
        }
    }

    fn drain(&mut self) {
        while let Some(event) = self.client.next_event() {
            self.events.push(event);
        }
    }

    /// Time passes, and the client is ticked.
    fn after(&mut self, by: Duration) {
        self.now += by;
        self.client.tick(self.now);
        self.pump();
    }

    /// The connection breaks: the bridge loses whatever it had not delivered, the
    /// client is told its transport closed.
    fn drop_connection(&mut self) {
        self.bridge.disconnect();
        self.client.lost(self.now, Cause::Dropped);
        self.drain();
    }

    /// Waits out the pause and dials again.
    fn redial(&mut self) {
        let Link::Retrying { at, .. } = self.client.link() else {
            panic!("not retrying: {:?}", self.client.link());
        };
        self.now = self.now.max(at);
        self.connect();
    }

    /// Everything the owner has been given as output of a terminal, in order.
    fn output(&self, terminal: &SessionId) -> Vec<u8> {
        let mut all = Vec::new();
        for event in &self.events {
            if let Event::Output { terminal: t, data } = event
                && t == terminal
            {
                all.extend_from_slice(data);
            }
        }
        all
    }

    /// The events so far, taken.
    fn take(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.events)
    }

    /// A rig with the connection up and a terminal made and attached.
    fn with_terminal(age: Age, name: &str) -> (Rig, SessionId) {
        let mut rig = Rig::new(age);
        let terminal = id(name);
        rig.connect();
        rig.client
            .new_terminal(terminal.clone(), NewSession::default(), Attach::new(SIZE))
            .expect("a request");
        rig.pump();
        assert!(rig.client.is_attached(&terminal), "{:?}", rig.events);
        rig.take();
        (rig, terminal)
    }
}

fn attaches(bridge: &Bridge, terminal: &SessionId) -> Vec<(u64, View)> {
    bridge
        .attaches(terminal)
        .into_iter()
        .map(|(offset, view)| (offset, view.expect("every attach carries a view")))
        .collect()
}

fn pictures(events: &[Event]) -> Vec<(bool, bool, Vec<u8>)> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::Picture {
                first, last, data, ..
            } => Some((*first, *last, data.clone())),
            _ => None,
        })
        .collect()
}

fn picture_bytes(events: &[Event]) -> Vec<u8> {
    pictures(events)
        .into_iter()
        .flat_map(|(_, _, data)| data)
        .collect()
}

fn sent_by(out: &[u8]) -> Vec<Packet> {
    let mut decoder = PacketDecoder::new();
    decoder.push(out);
    let mut packets = Vec::new();
    while let Some(packet) = decoder.next_packet().expect("whole packets") {
        packets.push(packet);
    }
    packets
}

// Terminals and their bytes

#[test]
fn a_new_terminal_is_made_attached_and_its_bytes_come_in_order() {
    let mut rig = Rig::new(Age::Current);
    let terminal = id("t1");
    rig.connect();
    let mut spec = NewSession::default();
    spec.argv = vec![b"ls".to_vec()];
    spec.history = Some(MAX_HISTORY + 5);
    rig.client
        .new_terminal(terminal.clone(), spec, Attach::new(SIZE))
        .expect("a request");
    rig.pump();
    let events = rig.take();
    assert_eq!(events.first(), Some(&Event::Linked));
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Event::Created(status) if status.id == terminal))
    );
    assert!(events.contains(&Event::Attached {
        terminal: terminal.clone(),
        offset: 0,
        truncated: false,
        answered: 0,
    }));

    rig.bridge.write(&terminal, b"hello ");
    rig.bridge.write(&terminal, b"world");
    rig.pump();
    assert_eq!(rig.output(&terminal), b"hello world");
    assert_eq!(rig.client.offset_of(&terminal), Some(11));

    // On the wire: the spec as given, the size of the attach and the history cut to
    // what the protocol allows.
    let made = rig
        .bridge
        .received
        .iter()
        .find_map(|p| match &p.message {
            Message::New(spec) => Some(spec.clone()),
            _ => None,
        })
        .expect("a new");
    assert_eq!(made.argv, vec![b"ls".to_vec()]);
    assert_eq!(made.size, Some(SIZE));
    assert_eq!(made.history, Some(MAX_HISTORY));
}

#[test]
fn an_attach_says_what_the_client_needs_of_the_server() {
    let (rig, terminal) = Rig::with_terminal(Age::Current, "t1");
    let attached = attaches(&rig.bridge, &terminal);
    assert_eq!(attached.len(), 1);
    let (offset, view) = &attached[0];
    assert_eq!(*offset, 0);
    assert_eq!(view.size, SIZE);
    // The terminal answers what it is shown, and the terminal is kept for a late reader.
    assert!(view.picture_if_answered);
    assert!(view.keep_after_end);
    assert!(!view.picture);
    assert!(!view.take);
    assert_eq!(
        view.name.map(|name| name.to_string()).as_deref(),
        Some("MacBook")
    );
}

#[test]
fn the_attach_waits_until_the_server_has_made_the_terminal() {
    let mut rig = Rig::new(Age::Current);
    let terminal = id("t1");
    rig.connect();
    rig.take();
    rig.client
        .new_terminal(terminal.clone(), NewSession::default(), Attach::new(SIZE))
        .expect("a request");
    let asked = rig.client.take_outgoing();
    let first = sent_by(&asked);
    assert!(
        matches!(first.as_slice(), [p] if matches!(p.message, Message::New(_))),
        "{first:?}"
    );
    assert!(!rig.client.has_outgoing());

    // Only with the server's answer in hand does the attach follow.
    rig.bridge.feed(&asked);
    rig.client.push(&rig.bridge.take_output(), rig.now);
    let second = sent_by(&rig.client.take_outgoing());
    assert!(
        matches!(second.as_slice(), [p] if matches!(p.message, Message::Attach { .. })),
        "{second:?}"
    );
}

#[test]
fn a_lost_connection_resumes_from_the_last_byte_with_none_twice_or_missing() {
    let (mut rig, terminal) = Rig::with_terminal(Age::Current, "t1");
    rig.bridge.write(&terminal, b"one ");
    rig.pump();
    assert_eq!(rig.output(&terminal), b"one ");

    // Written, never delivered: the connection broke first.
    rig.bridge.write(&terminal, b"lost ");
    rig.drop_connection();
    let lost_at = rig.now;
    assert_eq!(
        rig.client.link(),
        Link::Retrying {
            at: lost_at + secs(1),
            attempt: 0
        }
    );
    // Written while nobody was attached.
    rig.bridge.write(&terminal, b"two ");
    rig.bridge.write(&terminal, b"three");
    assert!(
        !rig.client.input(&terminal, b"x"),
        "nothing is typed into a gap"
    );
    assert!(!rig.client.is_attached(&terminal));

    rig.redial();
    assert_eq!(rig.output(&terminal), b"one lost two three");
    assert!(rig.client.is_attached(&terminal));
    // The second attach asked for exactly the byte after the last one given.
    let attached = attaches(&rig.bridge, &terminal);
    assert_eq!(attached.len(), 2);
    assert_eq!(attached[1].0, 4);
    assert!(attached[1].1.picture_if_answered && attached[1].1.keep_after_end);
    assert_eq!(rig.client.offset_of(&terminal), Some(18));
    assert_eq!(rig.bridge.connections, 2);
}

#[test]
fn bytes_the_journal_dropped_come_as_a_picture_and_the_stream_goes_on_from_it() {
    let (mut rig, terminal) = Rig::with_terminal(Age::Current, "t1");
    rig.bridge.write(&terminal, b"aaaa");
    rig.pump();
    rig.drop_connection();
    rig.bridge.write(&terminal, b"bbbbbbbb");
    rig.bridge.retain_last(&terminal, 4);
    rig.take();
    rig.redial();
    let events = rig.take();

    assert!(events.contains(&Event::Attached {
        terminal: terminal.clone(),
        offset: 12,
        truncated: true,
        answered: 0,
    }));
    // In two pieces: reset before the first, and the last says where the stream resumes.
    let pieces = pictures(&events);
    assert_eq!(pieces.len(), 2);
    assert!(pieces[0].0 && !pieces[0].1);
    assert!(!pieces[1].0 && pieces[1].1);
    assert_eq!(picture_bytes(&events), Bridge::picture_of(12));
    assert!(
        events.iter().all(|e| !matches!(e, Event::Output { .. })),
        "no bytes: the picture stands for them"
    );
    assert_eq!(rig.client.offset_of(&terminal), Some(12));

    rig.bridge.write(&terminal, b"cc");
    rig.pump();
    assert_eq!(rig.output(&terminal), b"cc");
    assert_eq!(rig.client.offset_of(&terminal), Some(14));
}

#[test]
fn a_question_already_answered_is_shown_as_a_picture_not_replayed() {
    let (mut rig, terminal) = Rig::with_terminal(Age::Current, "t1");
    rig.bridge.write(&terminal, b"ok");
    rig.pump();
    rig.drop_connection();
    // The program asked something in these bytes and the server answered it.
    rig.bridge.write(&terminal, b"\x1b[6n");
    rig.bridge.answer_until(&terminal, 6);
    rig.take();
    rig.redial();
    let events = rig.take();

    assert!(events.contains(&Event::Attached {
        terminal: terminal.clone(),
        offset: 6,
        truncated: true,
        answered: 6,
    }));
    assert_eq!(picture_bytes(&events), Bridge::picture_of(6));
    assert!(
        events.iter().all(|e| !matches!(e, Event::Output { .. })),
        "the question must not come again as bytes"
    );
    // The client said it needs that: it answers what it is shown.
    let attached = attaches(&rig.bridge, &terminal);
    assert!(attached[1].1.picture_if_answered);
}

#[test]
fn output_with_no_answered_question_is_bytes_even_after_a_loss() {
    let (mut rig, terminal) = Rig::with_terminal(Age::Current, "t1");
    rig.bridge.write(&terminal, b"ok");
    rig.pump();
    rig.drop_connection();
    rig.bridge.write(&terminal, b"more");
    // Answered up to what the client had already seen: nothing in the gap.
    rig.bridge.answer_until(&terminal, 2);
    rig.redial();
    assert!(pictures(&rig.events).is_empty());
    assert_eq!(rig.output(&terminal), b"okmore");
}

#[test]
fn the_end_of_the_program_gives_its_exit_status_and_the_terminal_is_not_attached_again() {
    let (mut rig, terminal) = Rig::with_terminal(Age::Current, "t1");
    rig.bridge.write(&terminal, b"bye");
    rig.bridge.exit(&terminal, Exit::Code(3));
    rig.pump();
    assert!(rig.events.contains(&Event::Ended {
        terminal: terminal.clone(),
        exit: Exit::Code(3),
    }));
    assert_eq!(rig.client.exit_of(&terminal), Some(Exit::Code(3)));
    assert!(!rig.client.is_attached(&terminal));

    rig.drop_connection();
    rig.redial();
    assert_eq!(
        attaches(&rig.bridge, &terminal).len(),
        1,
        "told once, asked once"
    );

    let signalled = id("t2");
    rig.client
        .new_terminal(signalled.clone(), NewSession::default(), Attach::new(SIZE))
        .expect("a request");
    rig.pump();
    rig.bridge.exit(&signalled, Exit::Signal(9));
    rig.pump();
    assert_eq!(rig.client.exit_of(&signalled), Some(Exit::Signal(9)));
}

#[test]
fn an_ended_terminal_is_kept_for_a_late_reader_when_the_app_asked() {
    let (mut rig, terminal) = Rig::with_terminal(Age::Current, "t1");
    rig.bridge.write(&terminal, b"last screen");
    rig.bridge.exit(&terminal, Exit::Code(0));
    rig.pump();
    assert!(rig.bridge.has(&terminal), "kept: the app asked in its view");

    // Another client, later, finds the last screen and how it ended.
    rig.drop_connection();
    rig.client = Client::new(None, rig.now);
    rig.events.clear();
    rig.connect();
    rig.client.attach(
        terminal.clone(),
        Attach::new(SIZE).from_now().with_picture(),
    );
    rig.pump();
    assert_eq!(picture_bytes(&rig.events), Bridge::picture_of(11));
    assert!(rig.events.contains(&Event::Ended {
        terminal: terminal.clone(),
        exit: Exit::Code(0),
    }));
}

#[test]
fn a_previous_relay_removes_the_terminal_once_it_was_told() {
    let (mut rig, terminal) = Rig::with_terminal(Age::Old, "t1");
    rig.bridge.exit(&terminal, Exit::Code(0));
    rig.pump();
    assert!(!rig.bridge.has(&terminal), "the old relay ignores the bit");
    assert_eq!(rig.client.exit_of(&terminal), Some(Exit::Code(0)));

    rig.drop_connection();
    rig.client = Client::new(None, rig.now);
    rig.events.clear();
    rig.connect();
    rig.client.attach(terminal.clone(), Attach::new(SIZE));
    rig.pump();
    assert!(rig.events.iter().any(|e| matches!(
        e,
        Event::Failed { terminal: Some(t), code, .. }
            if *t == terminal && *code == ErrorCode::UNKNOWN_SESSION
    )));
}

#[test]
fn the_terminals_state_is_told_and_kept() {
    let (mut rig, terminal) = Rig::with_terminal(Age::Current, "t1");
    let mut state = TerminalState::default();
    state.program = Some(b"npm".to_vec());
    state.cwd = Some(b"/srv/app".to_vec());
    rig.bridge.set_state(&terminal, state.clone());
    rig.pump();
    assert!(rig.events.contains(&Event::State {
        terminal: terminal.clone(),
        state: state.clone(),
    }));
    assert_eq!(rig.client.state_of(&terminal), Some(&state));
}

// A relay from before views

#[test]
fn a_previous_relay_is_served_the_same_way() {
    let (mut rig, terminal) = Rig::with_terminal(Age::Old, "t1");
    // No state, no answered offset: nothing the old relay does not have is waited for.
    rig.bridge.write(&terminal, b"abcd");
    rig.pump();
    rig.drop_connection();
    rig.bridge.write(&terminal, b"efgh");
    rig.redial();
    assert_eq!(rig.output(&terminal), b"abcdefgh");
    assert!(rig.events.iter().all(|e| !matches!(e, Event::State { .. })));
    assert!(rig.client.is_attached(&terminal));

    // The journal moved on: the relay says truncated, with no picture, and the
    // bytes it still has. The client takes them from where the relay says.
    rig.drop_connection();
    rig.bridge.write(&terminal, b"ijklmnop");
    rig.bridge.retain_last(&terminal, 4);
    rig.take();
    rig.redial();
    let events = rig.take();
    assert!(events.contains(&Event::Attached {
        terminal: terminal.clone(),
        offset: 12,
        truncated: true,
        answered: 0,
    }));
    assert!(pictures(&events).is_empty());
    assert_eq!(rig.client.offset_of(&terminal), Some(16));
}

// The handshake

#[test]
fn the_handshake_skips_a_banner_and_may_arrive_in_pieces() {
    let bridge = Bridge::new(Age::Current).with_banner(b"Last login: today\r\nKABLKABLO KABLO\x01");
    let mut rig = Rig::with(bridge);
    rig.client.connected(rig.now);
    rig.bridge.feed(&rig.client.take_outgoing());
    let reply = rig.bridge.take_output();
    for (at, byte) in reply.iter().enumerate() {
        assert!(!rig.client.link().is_live(), "live too early, at byte {at}");
        rig.client.push(&[*byte], rig.now);
    }
    assert!(rig.client.link().is_live());
    assert_eq!(rig.client.next_event(), Some(Event::Linked));
}

#[test]
fn a_hello_and_the_first_frames_in_one_read_are_both_taken() {
    let mut rig = Rig::new(Age::Current);
    let terminal = rig.bridge.add("old", SIZE);
    rig.bridge.write(&terminal, b"waiting");
    rig.client.attach(terminal.clone(), Attach::new(SIZE));
    rig.client.connected(rig.now);
    rig.bridge.feed(&rig.client.take_outgoing());
    // The bridge answers the hello; the attach goes out when the client is linked.
    let reply = rig.bridge.take_output();
    let mut with_frames = reply.clone();
    let mut packet = Vec::new();
    Packet::connection(Message::Pong(vec![1]))
        .encode(&mut packet)
        .expect("a packet");
    with_frames.extend_from_slice(&packet);
    rig.client.push(&with_frames, rig.now);
    assert!(rig.client.link().is_live());
    rig.pump();
    assert_eq!(rig.output(&terminal), b"waiting");
}

#[test]
fn a_server_speaking_a_range_that_holds_ours_is_taken_at_ours() {
    let mut rig = Rig::with(Bridge::new(Age::Current).speaking(1, 3));
    rig.connect();
    assert!(rig.client.link().is_live());
}

#[test]
fn a_server_that_shares_no_version_ends_the_attempts() {
    let mut rig = Rig::with(Bridge::new(Age::Current).speaking(2, 3));
    rig.connect();
    assert_eq!(rig.client.link(), Link::Over(Cause::Incompatible));
    assert_eq!(
        rig.events,
        vec![Event::Lost {
            cause: Cause::Incompatible
        }]
    );
    rig.now += secs(3600);
    assert!(!rig.client.dial_due(rig.now));
    assert_eq!(rig.client.next_deadline(), None);
}

#[test]
fn a_far_end_that_never_speaks_the_protocol_is_given_up_on() {
    let mut client = Client::new(None, Instant::now());
    let now = Instant::now();
    client.connected(now);
    let noise = vec![b'x'; 70 * 1024];
    for piece in noise.chunks(4096) {
        client.push(piece, now);
    }
    assert_eq!(client.link(), Link::Over(Cause::Incompatible));
}

#[test]
fn a_handshake_that_never_finishes_is_given_up_after_a_wait() {
    let now = Instant::now();
    let mut client = Client::new(None, now);
    client.connected(now);
    assert_eq!(client.next_deadline(), Some(now + OPEN_WAIT));
    client.tick(now + OPEN_WAIT - Duration::from_millis(1));
    assert!(matches!(client.link(), Link::Opening { .. }));
    client.tick(now + OPEN_WAIT);
    assert_eq!(
        client.link(),
        Link::Retrying {
            at: now + OPEN_WAIT + secs(1),
            attempt: 0
        }
    );
}

// The link and its pauses

#[test]
fn the_pauses_double_up_to_a_cap() {
    let pauses: Vec<u64> = (0..9).map(|n| retry_delay(n).as_secs()).collect();
    assert_eq!(pauses, [1, 2, 4, 8, 16, 30, 30, 30, 30]);
    assert_eq!(retry_delay(u32::MAX), secs(30));
}

#[test]
fn the_link_goes_from_retrying_through_opening_to_live_and_back() {
    let now = Instant::now();
    let mut client = Client::new(None, now);
    assert_eq!(
        client.link(),
        Link::Retrying {
            at: now,
            attempt: 0
        }
    );
    assert!(client.dial_due(now));
    assert_eq!(client.next_deadline(), Some(now));

    // A dial that fails is retried after a pause that grows.
    client.lost(now, Cause::Dropped);
    assert_eq!(
        client.link(),
        Link::Retrying {
            at: now + secs(1),
            attempt: 0
        }
    );
    assert!(!client.dial_due(now + Duration::from_millis(999)));
    assert!(client.dial_due(now + secs(1)));
    client.lost(now + secs(1), Cause::Dropped);
    assert_eq!(
        client.link(),
        Link::Retrying {
            at: now + secs(3),
            attempt: 1
        }
    );
    client.lost(now + secs(3), Cause::Dropped);
    assert_eq!(
        client.link(),
        Link::Retrying {
            at: now + secs(7),
            attempt: 2
        }
    );
}

#[test]
fn a_connection_that_lived_starts_the_pauses_over_and_one_that_flaps_does_not() {
    let mut rig = Rig::new(Age::Current);
    // Up and down in a second, again and again: the pauses keep growing.
    let mut pauses = Vec::new();
    for _ in 0..3 {
        rig.connect();
        rig.now += secs(1);
        rig.drop_connection();
        let Link::Retrying { at, attempt } = rig.client.link() else {
            panic!("{:?}", rig.client.link());
        };
        pauses.push((attempt, at - rig.now));
        rig.now = at;
    }
    assert_eq!(pauses, [(0, secs(1)), (1, secs(2)), (2, secs(4))]);

    // One that stayed up for a while is a fresh start.
    rig.connect();
    rig.now += STABLE;
    rig.drop_connection();
    assert_eq!(
        rig.client.link(),
        Link::Retrying {
            at: rig.now + secs(1),
            attempt: 0
        }
    );
}

#[test]
fn a_login_needed_stops_the_retries_until_asked() {
    let now = Instant::now();
    let mut client = Client::new(None, now);
    client.lost(now, Cause::LoginNeeded);
    assert_eq!(client.link(), Link::NeedsLogin);
    assert_eq!(client.next_deadline(), None);
    assert!(!client.dial_due(now + secs(86_400)));
    assert_eq!(
        client.next_event(),
        Some(Event::Lost {
            cause: Cause::LoginNeeded
        })
    );

    // The user logged in: dial now.
    client.retry_now(now + secs(5));
    assert!(client.dial_due(now + secs(5)));
}

#[test]
fn try_now_dials_without_waiting_the_pause_out() {
    let now = Instant::now();
    let mut client = Client::new(None, now);
    for n in 0..4 {
        client.lost(now + secs(n), Cause::Dropped);
    }
    assert!(!client.dial_due(now + secs(4)));
    client.retry_now(now + secs(4));
    assert!(client.dial_due(now + secs(4)));
    // A wake after a sleep is the same.
    client.lost(now + secs(5), Cause::Dropped);
    assert!(!client.dial_due(now + secs(5)));
    client.wake(now + secs(5));
    assert!(client.dial_due(now + secs(5)));
}

#[test]
fn a_refusal_is_final() {
    let now = Instant::now();
    let mut client = Client::new(None, now);
    client.lost(now, Cause::Refused);
    assert_eq!(client.link(), Link::Over(Cause::Refused));
    client.retry_now(now);
    client.wake(now);
    assert!(!client.dial_due(now + secs(3600)));
    client.connected(now);
    assert_eq!(client.link(), Link::Over(Cause::Refused));
}

// The pulse

#[test]
fn a_quiet_connection_is_asked_whether_it_is_alive_and_the_answer_is_enough() {
    let mut rig = Rig::new(Age::Current);
    rig.connect();
    let live_at = rig.now;
    assert_eq!(rig.client.next_deadline(), Some(live_at + PULSE_IDLE));
    rig.after(PULSE_IDLE - Duration::from_millis(1));
    assert!(
        rig.bridge
            .received
            .iter()
            .all(|p| !matches!(p.message, Message::Ping(_)))
    );
    rig.after(Duration::from_millis(1));
    assert_eq!(
        rig.bridge
            .received
            .iter()
            .filter(|p| matches!(p.message, Message::Ping(_)))
            .count(),
        1
    );
    // The pong came back in the same pump: the connection is alive and the next
    // question waits another quiet stretch.
    assert!(rig.client.link().is_live());
    assert_eq!(rig.client.next_deadline(), Some(rig.now + PULSE_IDLE));
}

#[test]
fn a_connection_that_does_not_answer_is_given_up() {
    let mut rig = Rig::new(Age::Current);
    rig.connect();
    rig.now += PULSE_IDLE;
    rig.client.tick(rig.now);
    // The ping is on its way and nothing ever comes back.
    assert!(rig.client.has_outgoing());
    let _ = rig.client.take_outgoing();
    assert_eq!(rig.client.next_deadline(), Some(rig.now + PULSE_REPLY));
    rig.now += PULSE_REPLY - Duration::from_millis(1);
    rig.client.tick(rig.now);
    assert!(rig.client.link().is_live());
    rig.now += Duration::from_millis(1);
    rig.client.tick(rig.now);
    assert!(matches!(rig.client.link(), Link::Retrying { .. }));
    rig.drain();
    assert!(rig.events.contains(&Event::Lost {
        cause: Cause::Dropped
    }));
}

#[test]
fn waking_asks_at_once_and_a_silent_connection_is_given_up_after_the_usual_wait() {
    let mut rig = Rig::new(Age::Current);
    rig.connect();
    rig.now += secs(3);
    rig.client.wake(rig.now);
    let sent = sent_by(&rig.client.take_outgoing());
    assert!(matches!(sent.as_slice(), [p] if matches!(p.message, Message::Ping(_))));
    rig.now += PULSE_REPLY;
    rig.client.tick(rig.now);
    assert!(matches!(rig.client.link(), Link::Retrying { .. }));
}

#[test]
fn any_sound_from_the_far_end_counts_as_the_answer() {
    let (mut rig, terminal) = Rig::with_terminal(Age::Current, "t1");
    rig.now += PULSE_IDLE;
    rig.client.tick(rig.now);
    let _ = rig.client.take_outgoing();
    // Output, not a pong, arrives: the connection is alive.
    rig.now += Duration::from_secs(5);
    rig.bridge.write(&terminal, b"x");
    rig.pump();
    rig.now += PULSE_REPLY;
    rig.client.tick(rig.now);
    assert!(rig.client.link().is_live());
    assert_eq!(rig.output(&terminal), b"x");
}

// Taking a terminal

#[test]
fn a_terminal_taken_by_another_client_is_let_go_and_not_attached_again_unless_asked() {
    let mut rig = Rig::new(Age::Current);
    let terminal = rig.bridge.add("shared", SIZE);
    rig.connect();
    rig.client
        .attach(terminal.clone(), Attach::new(SIZE).taking());
    rig.pump();
    assert!(rig.client.is_attached(&terminal));
    rig.take();

    rig.bridge.take_by(&terminal, "iMac\x07");
    rig.pump();
    assert_eq!(
        rig.take(),
        vec![Event::Taken {
            terminal: terminal.clone(),
            by: Some("iMac".to_owned())
        }]
    );
    assert!(!rig.client.is_attached(&terminal));
    assert!(!rig.client.input(&terminal, b"x"));

    // A new connection does not take it back.
    rig.drop_connection();
    rig.redial();
    assert_eq!(attaches(&rig.bridge, &terminal).len(), 1);

    // Asking is the owner's.
    rig.client.attach(terminal.clone(), Attach::new(SIZE));
    rig.pump();
    assert!(rig.client.is_attached(&terminal));
}

#[test]
fn a_take_is_asked_once_and_asked_again_only_if_it_did_not_happen() {
    let mut rig = Rig::new(Age::Current);
    let terminal = rig.bridge.add("shared", SIZE);
    rig.connect();

    // The attach goes out and the connection breaks before the answer: it did not happen.
    rig.client
        .attach(terminal.clone(), Attach::new(SIZE).taking());
    let _ = rig.client.take_outgoing();
    rig.drop_connection();
    rig.redial();
    let asked = attaches(&rig.bridge, &terminal);
    assert_eq!(asked.len(), 1);
    assert!(asked[0].1.take, "asked again, because it did not happen");

    // It happened: a connection that breaks now attaches again as the client it is.
    assert!(rig.client.is_attached(&terminal));
    rig.drop_connection();
    rig.redial();
    let asked = attaches(&rig.bridge, &terminal);
    assert_eq!(asked.len(), 2);
    assert!(!asked[1].1.take, "not a second take");
}

// Typing and sizes

#[test]
fn typing_goes_to_the_program_in_order_and_is_refused_where_it_cannot_arrive() {
    let mut rig = Rig::new(Age::Current);
    let terminal = id("t1");
    assert!(!rig.client.input(&terminal, b"x"), "no terminal yet");
    rig.connect();
    rig.client
        .new_terminal(terminal.clone(), NewSession::default(), Attach::new(SIZE))
        .expect("a request");
    rig.pump();
    assert!(rig.client.input(&terminal, b"ls\r"));
    // A paste larger than one message goes as several, in order.
    let paste: Vec<u8> = (0..600 * 1024).map(|n| (n % 251) as u8).collect();
    assert!(rig.client.input(&terminal, &paste));
    rig.pump();
    let mut typed = b"ls\r".to_vec();
    typed.extend_from_slice(&paste);
    assert_eq!(rig.bridge.typed(&terminal), typed);
    let chunks = rig
        .bridge
        .received
        .iter()
        .filter(|p| matches!(&p.message, Message::Input(data) if data.len() <= INPUT_CHUNK))
        .count();
    assert_eq!(chunks, 1 + 3);

    rig.drop_connection();
    assert!(
        !rig.client.input(&terminal, b"x"),
        "the line would run unseen later"
    );
}

#[test]
fn a_resize_is_sent_and_remembered_for_the_next_attach() {
    let (mut rig, terminal) = Rig::with_terminal(Age::Current, "t1");
    rig.client.resize(&terminal, WIDE);
    rig.pump();
    assert_eq!(rig.bridge.size_of(&terminal), Some(WIDE));
    rig.drop_connection();
    // Resized while away: said in the attach that follows.
    rig.client.resize(&terminal, SIZE);
    rig.redial();
    let attached = attaches(&rig.bridge, &terminal);
    assert_eq!(attached[1].1.size, SIZE);
    assert_eq!(rig.bridge.size_of(&terminal), Some(SIZE));
}

#[test]
fn a_terminal_the_client_did_not_start_can_be_ended_by_name() {
    let mut rig = Rig::new(Age::Current);
    let left = rig.bridge.add("left-behind", SIZE);
    assert!(!rig.client.terminate(&left, false), "not connected");
    rig.connect();
    assert!(rig.client.terminate(&left, true));
    rig.pump();
    assert_eq!(rig.bridge.terminated(&left), Some(true));
}

#[test]
fn the_status_and_the_list_come_back_as_events() {
    let mut rig = Rig::new(Age::Current);
    let one = rig.bridge.add("one", SIZE);
    rig.bridge.add("two", SIZE);
    rig.connect();
    assert!(rig.client.status(&one));
    assert!(rig.client.list());
    rig.pump();
    let listed = rig
        .events
        .iter()
        .find_map(|e| match e {
            Event::Listed(list) => Some(list.iter().map(|s| s.id.to_string()).collect::<Vec<_>>()),
            _ => None,
        })
        .expect("a list");
    assert_eq!(listed, ["one", "two"]);
    assert!(rig.events.iter().any(
        |e| matches!(e, Event::Status(status) if status.id == one && status.version == "0.2.0")
    ));
}

// What goes wrong

#[test]
fn output_that_overlaps_what_was_given_is_trimmed() {
    let (mut rig, terminal) = Rig::with_terminal(Age::Current, "t1");
    rig.bridge.write(&terminal, b"abcdef");
    rig.pump();
    rig.take();
    // Resent from two bytes back, and one wholly given before.
    rig.bridge.inject(&Packet::about(
        terminal.clone(),
        Message::Output {
            offset: 4,
            data: b"efgh".to_vec(),
        },
    ));
    rig.bridge.inject(&Packet::about(
        terminal.clone(),
        Message::Output {
            offset: 0,
            data: b"abc".to_vec(),
        },
    ));
    rig.pump();
    assert_eq!(rig.output(&terminal), b"gh");
    assert_eq!(rig.client.offset_of(&terminal), Some(8));
}

#[test]
fn a_gap_in_the_output_drops_the_connection_and_the_new_one_fills_it() {
    let (mut rig, terminal) = Rig::with_terminal(Age::Current, "t1");
    rig.bridge.write(&terminal, b"abc");
    rig.pump();
    rig.bridge.inject(&Packet::about(
        terminal.clone(),
        Message::Output {
            offset: 10,
            data: b"zzz".to_vec(),
        },
    ));
    rig.pump();
    assert!(rig.events.contains(&Event::Lost {
        cause: Cause::Protocol
    }));
    assert_eq!(
        rig.output(&terminal),
        b"abc",
        "nothing past the gap is given"
    );
    rig.bridge.disconnect();
    rig.bridge.write(&terminal, b"def");
    rig.redial();
    assert_eq!(rig.output(&terminal), b"abcdef");
}

#[test]
fn a_picture_that_never_finished_is_started_over() {
    let (mut rig, terminal) = Rig::with_terminal(Age::Current, "t1");
    let attached = |offset| {
        Packet::about(
            terminal.clone(),
            Message::Attached {
                offset,
                truncated: true,
                answered: 0,
            },
        )
    };
    let piece = |offset, more, data: &str| {
        Packet::about(
            terminal.clone(),
            Message::Picture {
                offset,
                cols: 80,
                rows: 24,
                more,
                data: data.as_bytes().to_vec(),
            },
        )
    };
    // A first picture is cut off by a second attach (the connection it came over
    // was replaced), and the second one is whole.
    for packet in [
        attached(5),
        piece(5, true, "A1"),
        attached(7),
        piece(7, true, "B1"),
        piece(7, false, "B2"),
    ] {
        rig.bridge.inject(&packet);
    }
    rig.pump();
    let seen: Vec<(bool, bool)> = pictures(&rig.events)
        .into_iter()
        .map(|(first, last, _)| (first, last))
        .collect();
    assert_eq!(seen, [(true, false), (true, false), (false, true)]);
    assert_eq!(rig.client.offset_of(&terminal), Some(7));
}

#[test]
fn text_from_the_server_is_shown_without_its_controls() {
    let (mut rig, terminal) = Rig::with_terminal(Age::Current, "t1");
    rig.bridge.inject(&Packet::about(
        terminal.clone(),
        Message::Error {
            code: ErrorCode::INPUT_FULL,
            text: "full\x1b[2J up".to_owned(),
        },
    ));
    rig.pump();
    assert!(rig.events.iter().any(|e| matches!(
        e,
        Event::Failed { text, .. } if text == "full[2J up"
    )));
    // A refused input is not the end of the terminal.
    assert!(rig.client.is_attached(&terminal));
}

#[test]
fn an_attach_the_server_cannot_serve_just_now_is_tried_again_after_a_pause() {
    let mut rig = Rig::new(Age::Current);
    let terminal = rig.bridge.add("slow", SIZE);
    rig.bridge.refuse_attaches(&terminal, 2);
    rig.connect();
    rig.client.attach(terminal.clone(), Attach::new(SIZE));
    rig.pump();
    assert!(!rig.client.is_attached(&terminal));
    assert!(
        rig.events
            .iter()
            .all(|e| !matches!(e, Event::Failed { .. }))
    );
    let first = rig.now;
    assert_eq!(rig.client.next_deadline(), Some(first + secs(1)));

    rig.after(secs(1));
    assert!(!rig.client.is_attached(&terminal));
    assert_eq!(rig.client.next_deadline(), Some(first + secs(1) + secs(2)));
    rig.after(secs(2));
    assert!(rig.client.is_attached(&terminal));
    assert_eq!(attaches(&rig.bridge, &terminal).len(), 3);
}

#[test]
fn a_terminal_the_server_does_not_have_is_given_up_not_asked_for_again() {
    let mut rig = Rig::new(Age::Current);
    rig.connect();
    let nothing = id("nothing");
    rig.client.attach(nothing.clone(), Attach::new(SIZE));
    rig.pump();
    assert!(rig.events.iter().any(|e| matches!(
        e,
        Event::Failed { terminal: Some(t), code, .. }
            if *t == nothing && *code == ErrorCode::UNKNOWN_SESSION
    )));
    let asks = |bridge: &Bridge| {
        bridge
            .received
            .iter()
            .filter(|p| {
                p.session.as_ref() == Some(&nothing) && matches!(p.message, Message::Attach { .. })
            })
            .count()
    };
    assert_eq!(asks(&rig.bridge), 1);
    rig.drop_connection();
    rig.redial();
    assert_eq!(asks(&rig.bridge), 1);
}

#[test]
fn a_new_terminal_whose_answer_was_lost_is_found_again_not_made_twice() {
    let mut rig = Rig::new(Age::Current);
    let terminal = id("t1");
    rig.connect();
    rig.client
        .new_terminal(terminal.clone(), NewSession::default(), Attach::new(SIZE))
        .expect("a request");
    // The server makes it; the answer is lost with the connection.
    rig.bridge.feed(&rig.client.take_outgoing());
    rig.drop_connection();
    rig.take();
    assert!(rig.bridge.has(&terminal));

    rig.redial();
    let events = rig.take();
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e, Event::Created(_)))
            .count(),
        1,
        "{events:?}"
    );
    assert!(rig.client.is_attached(&terminal));
    assert!(events.iter().all(|e| !matches!(e, Event::Failed { .. })));
}

#[test]
fn a_name_that_is_taken_by_another_is_refused_and_a_request_the_protocol_refuses_is_refused_at_once()
 {
    let mut rig = Rig::new(Age::Current);
    let theirs = rig.bridge.add("theirs", SIZE);
    rig.connect();
    rig.client
        .new_terminal(theirs.clone(), NewSession::default(), Attach::new(SIZE))
        .expect("a request");
    rig.pump();
    assert!(rig.events.iter().any(|e| matches!(
        e,
        Event::Failed { code, .. } if *code == ErrorCode::SESSION_EXISTS
    )));
    assert!(!rig.client.is_attached(&theirs));

    // Twice the same name here.
    let mine = id("mine");
    rig.client
        .new_terminal(mine.clone(), NewSession::default(), Attach::new(SIZE))
        .expect("a request");
    assert!(
        rig.client
            .new_terminal(mine, NewSession::default(), Attach::new(SIZE))
            .is_err()
    );
    // A NUL in an argument the server could not start.
    let mut bad = NewSession::default();
    bad.argv = vec![b"a\0b".to_vec()];
    assert!(
        rig.client
            .new_terminal(id("bad"), bad, Attach::new(SIZE))
            .is_err()
    );
}

#[test]
fn bytes_from_a_transport_already_given_up_are_dropped() {
    let (mut rig, terminal) = Rig::with_terminal(Age::Current, "t1");
    rig.take();
    rig.bridge.write(&terminal, b"late");
    let late = rig.bridge.take_output();
    rig.client.lost(rig.now, Cause::Dropped);
    rig.client.push(&late, rig.now);
    rig.drain();
    assert_eq!(rig.output(&terminal), b"");
}

#[test]
fn a_detached_terminal_is_not_asked_for_again_and_a_closed_client_sends_nothing() {
    let (mut rig, terminal) = Rig::with_terminal(Age::Current, "t1");
    rig.client.detach(&terminal);
    rig.pump();
    assert!(!rig.bridge.is_attached(&terminal));
    rig.drop_connection();
    rig.redial();
    assert_eq!(attaches(&rig.bridge, &terminal).len(), 1);
    rig.client.close();
    assert!(!rig.client.has_outgoing());
    assert!(!rig.client.list());
}

// A real pipe

#[cfg(unix)]
#[test]
fn the_client_and_the_bridge_speak_across_a_real_pipe() {
    use std::io::{Read, Write};
    use std::os::unix::net::UnixStream;
    use std::sync::{Arc, Mutex};
    use std::thread;

    let (mut near, mut far) = UnixStream::pair().expect("a pipe");
    let terminal = id("pipe");
    let bridge = Arc::new(Mutex::new(Bridge::new(Age::Current)));

    // The far end: reads what the client says, answers, and plays a program that
    // echoes what is typed to it.
    let served = Arc::clone(&bridge);
    let echoing = terminal.clone();
    let server = thread::spawn(move || {
        let mut buf = [0u8; 4096];
        let mut echoed = 0;
        loop {
            let n = match far.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
            let mut bridge = served.lock().expect("a lock");
            bridge.feed(buf.get(..n).unwrap_or_default());
            let typed = bridge.typed(&echoing);
            if typed.len() > echoed {
                let mut line = b"echo: ".to_vec();
                line.extend_from_slice(typed.get(echoed..).unwrap_or_default());
                echoed = typed.len();
                bridge.write(&echoing, &line);
            }
            let out = bridge.take_output();
            if !out.is_empty() && far.write_all(&out).is_err() {
                break;
            }
        }
    });

    near.set_read_timeout(Some(Duration::from_millis(20)))
        .expect("a timeout");
    let mut client = Client::new(Some("pipe test"), Instant::now());
    client.connected(Instant::now());
    client
        .new_terminal(terminal.clone(), NewSession::default(), Attach::new(SIZE))
        .expect("a request");
    let mut heard = Vec::new();
    let mut typed = false;
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut buf = [0u8; 4096];
    while Instant::now() < deadline && !heard.ends_with(b"echo: ls\r") {
        let out = client.take_outgoing();
        if !out.is_empty() {
            near.write_all(&out).expect("a write");
        }
        match near.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => client.push(buf.get(..n).unwrap_or_default(), Instant::now()),
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(e) => panic!("{e}"),
        }
        while let Some(event) = client.next_event() {
            if let Event::Output { data, .. } = event {
                heard.extend_from_slice(&data);
            }
        }
        if !typed && client.is_attached(&terminal) {
            typed = client.input(&terminal, b"ls\r");
        }
    }
    drop(near);
    server
        .join()
        .expect("the far end ends when the pipe closes");
    assert_eq!(heard, b"echo: ls\r");
    assert_eq!(client.offset_of(&terminal), Some(9));
}
