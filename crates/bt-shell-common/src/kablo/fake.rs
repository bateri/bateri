//! The far end of the client's pipe, for tests: a kablo bridge with a few
//! terminals, written against the same codecs the client uses (`kablo-proto`),
//! so a test drives the client over bytes, as a real server would, and can
//! look at what the client put on the wire.
//!
//! It is the *server's* behaviour in the few places the client relies on, not a
//! copy of the server: a journal that keeps the last N bytes of what a program
//! wrote and a screen it can describe in a picture, how far the program's
//! questions are answered, a terminal kept for a while once an app that asked
//! was told it ended, a terminal another client can take, and an old version of
//! itself (a relay from before the view's flags and names, that ignores them).
//! It keeps a mailbox too, as the bridge of today does: a test plays the program
//! that runs `kablo app` ([`Bridge::mail`] and the `sender_*` methods) and looks at
//! what the app answered ([`Bridge::said_to_sender`]).
//! Nothing here is timed: the test says when the program writes and when the
//! connection breaks.
//!
//! ```text
//! client.take_outgoing() ──► Bridge::feed        (the client's bytes)
//! client.push(..)        ◄── Bridge::take_output (the bridge's bytes)
//! ```

use std::collections::BTreeMap;

use kablo_proto::hello::{Protocol, negotiate};
use kablo_proto::v1::stream::{self, FailCode, Incoming, IncomingDecoder, MAX_DATA, Side};
use kablo_proto::v1::{
    ClientName, ErrorCode, Exit, Features, Message, NewSession, OFFSET_END, Packet, Size, State,
    Status, TerminalState, View,
};
use kablo_proto::{Error, SIGNATURE, SessionId};

/// The process id the bridge says every shell has.
pub const FAKE_PID: u32 = 4242;

/// Which version of kablo the bridge plays.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Age {
    /// The relay of today: honours every flag of the view, tells the terminal's
    /// state, says how far it answered the program's questions, keeps an ended
    /// terminal for an app that asked.
    Current,
    /// A relay from before the view's flags, the names and the state: it reads
    /// the offset and ignores the rest of an `attach`, never sends a picture,
    /// says nothing of the program's questions, and removes a terminal as soon as
    /// a client was told it ended.
    Old,
}

/// What a relay of today says of a terminal's times (`status`'s `started`, `front_started`,
/// `front_is_own` and `output_at`, in seconds of the server's clock).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Activity {
    /// When the terminal's own program started.
    pub started: Option<u64>,
    /// When the program in front started.
    pub front_started: Option<u64>,
    /// The program in front is the terminal's own (a shell waiting for a command).
    pub front_is_own: Option<bool>,
    /// When the relay last read output.
    pub output_at: Option<u64>,
}

/// A terminal on the bridge.
pub struct Session {
    /// The bytes the program wrote that the journal still holds; the first is at
    /// `retained_from`.
    journal: Vec<u8>,
    retained_from: u64,
    /// Every question the program asked before this offset is answered.
    answered: u64,
    exit: Option<Exit>,
    /// A client was told the program ended.
    told: bool,
    /// An app that asked for the terminal to be kept has attached.
    keep: bool,
    /// This connection's client is attached; `sent` is the next offset it is owed.
    attached: bool,
    sent: u64,
    /// The clients attached on other connections (another computer's bateri, an agent's
    /// `kablo wait`), by the name each gave itself. They are only counted and named: nothing is
    /// sent to them, and an `attach` that takes the terminal lets them all go.
    others: Vec<Option<String>>,
    /// What this connection's client called itself in its last `attach` (a relay of today).
    name: Option<ClientName>,
    size: Size,
    state: TerminalState,
    activity: Activity,
    /// The label the terminal was given (`kablo new --label`).
    label: Option<String>,
    /// What the client typed.
    typed: Vec<u8>,
    /// The view of every `attach` the client sent, in order.
    views: Vec<(u64, Option<View>)>,
    /// The `attach`es to refuse with `UNAVAILABLE` before serving one.
    unavailable: u32,
    /// `terminate` was asked (`Some(force)`).
    terminated: Option<bool>,
    /// How many times a client asked the terminal to forget its past.
    forgotten: u32,
    /// The command the terminal was made with.
    argv: Vec<Vec<u8>>,
    /// The environment it was made with, over the defaults.
    env: Vec<(Vec<u8>, Vec<u8>)>,
}

impl Session {
    fn new(size: Size) -> Session {
        Session {
            journal: Vec::new(),
            retained_from: 0,
            answered: 0,
            exit: None,
            told: false,
            keep: false,
            attached: false,
            sent: 0,
            others: Vec::new(),
            name: None,
            size,
            state: TerminalState::default(),
            activity: Activity::default(),
            label: None,
            typed: Vec::new(),
            views: Vec::new(),
            unavailable: 0,
            terminated: None,
            forgotten: 0,
            argv: Vec::new(),
            env: Vec::new(),
        }
    }

    /// One past the last byte the program wrote.
    fn end(&self) -> u64 {
        self.retained_from + self.journal.len() as u64
    }

    fn from(&self, offset: u64) -> &[u8] {
        let skip = usize::try_from(offset.saturating_sub(self.retained_from)).unwrap_or(usize::MAX);
        self.journal.get(skip..).unwrap_or_default()
    }
}

/// One end of a conversation, as the bridge counts what it is owed.
struct Leg {
    /// The bytes this end may still send before the other credits it.
    left: u32,
    eof: bool,
}

/// A conversation the app took: the program's `kablo app` on one end, the client on the other.
struct Talking {
    /// What the program sends (the client gave the window).
    from_sender: Leg,
    /// What the app sends (the program gave the window).
    from_app: Leg,
    /// What the app said, in order.
    said: Vec<u8>,
    /// The bytes the app has credited the program with (it has taken that much of what the
    /// program sent).
    credit_from_app: u32,
    /// The app closed it.
    closed: bool,
}

/// The bridge. One value is the server: its terminals outlive a connection;
/// [`Bridge::disconnect`] ends the connection and the next bytes fed are a new one.
pub struct Bridge {
    age: Age,
    range: (u8, u8),
    banner: Vec<u8>,
    sessions: BTreeMap<SessionId, Session>,
    hello: Vec<u8>,
    greeted: bool,
    shut: bool,
    decoder: IncomingDecoder,
    out: Vec<u8>,
    made: u32,
    /// The bridge keeps a mailbox (a relay of today does, and says so in `features_reply`).
    mailbox: bool,
    /// The client said it takes tickets, on this connection.
    tickets: bool,
    /// The last ticket given.
    last_ticket: u32,
    /// The conversations a program opened that the app has not answered: the ticket and
    /// the window the program gave.
    waiting: BTreeMap<u32, u32>,
    /// The conversations the app took, by its stream.
    talks: BTreeMap<u32, Talking>,
    /// The program behind `kablo app` credits what the app says as it takes it.
    sender_reads: bool,
    /// Every ticket the app declined, with its words.
    declined: Vec<(u32, String)>,
    /// How many times an end sent more than it was given, or after its `eof`.
    breaches: u32,
    /// A terminal's program takes a hang-up for no reason to end (only the
    /// kill does).
    stubborn: bool,
    /// How many connections have been greeted.
    pub connections: u32,
    /// Every packet the client sent, in order, over every connection.
    pub received: Vec<Packet>,
    /// Every stream message the client sent, in order, over every connection.
    pub received_streams: Vec<stream::Packet>,
}

impl Bridge {
    /// A bridge of this age that speaks wire version 1, with no terminals.
    pub fn new(age: Age) -> Bridge {
        Bridge {
            age,
            range: (1, 1),
            banner: Vec::new(),
            sessions: BTreeMap::new(),
            hello: Vec::new(),
            greeted: false,
            shut: false,
            decoder: IncomingDecoder::new(),
            out: Vec::new(),
            made: 0,
            mailbox: true,
            tickets: false,
            last_ticket: 0,
            waiting: BTreeMap::new(),
            talks: BTreeMap::new(),
            sender_reads: true,
            declined: Vec::new(),
            breaches: 0,
            stubborn: false,
            connections: 0,
            received: Vec::new(),
            received_streams: Vec::new(),
        }
    }

    /// A relay that answers `features` and keeps no mailbox: no ticket is ever given.
    /// ([`Age::Old`] answers nothing at all.)
    pub fn without_mailbox(mut self) -> Bridge {
        self.mailbox = false;
        self
    }

    /// The programs of its terminals do not end on a hang-up.
    pub fn stubborn(mut self) -> Bridge {
        self.stubborn = true;
        self
    }

    /// Speaks these wire versions (the client picks the highest both speak).
    pub fn speaking(mut self, min: u8, max: u8) -> Bridge {
        self.range = (min, max);
        self
    }

    /// Prints this before the hello each time, as a login shell does on the way
    /// to the program the far end of an ssh command starts.
    pub fn with_banner(mut self, banner: &[u8]) -> Bridge {
        self.banner = banner.to_vec();
        self
    }

    /// The relay that serves the terminals is replaced by one of another release
    /// (`kablo upgrade --all`): the terminals, their journals and their programs
    /// stay, and the next connection is served the way the new relay serves.
    pub fn replace_relays(&mut self, age: Age) {
        self.age = age;
    }

    /// The release of the relay that serves the terminals.
    pub fn age(&self) -> Age {
        self.age
    }

    /// A terminal that exists already, as one started earlier.
    pub fn add(&mut self, id: &str, size: Size) -> SessionId {
        let id = SessionId::new(id).expect("a session id");
        self.sessions.insert(id.clone(), Session::new(size));
        id
    }

    // What the program does

    /// The program writes: kept in the journal and, if a client is attached,
    /// sent to it.
    pub fn write(&mut self, id: &SessionId, bytes: &[u8]) {
        let Some(session) = self.sessions.get_mut(id) else {
            return;
        };
        let at = session.end();
        session.journal.extend_from_slice(bytes);
        if session.attached && session.sent == at {
            session.sent = session.end();
            let packet = Packet::about(
                id.clone(),
                Message::Output {
                    offset: at,
                    data: bytes.to_vec(),
                },
            );
            self.send(&packet);
        }
    }

    /// The journal forgets all but the last `keep` bytes.
    pub fn retain_last(&mut self, id: &SessionId, keep: usize) {
        if let Some(session) = self.sessions.get_mut(id) {
            let drop = session.journal.len().saturating_sub(keep);
            session.journal.drain(..drop);
            session.retained_from += drop as u64;
        }
    }

    /// Every question the program asked before `offset` is answered.
    pub fn answer_until(&mut self, id: &SessionId, offset: u64) {
        if let Some(session) = self.sessions.get_mut(id) {
            session.answered = offset;
        }
    }

    /// The terminal's state changes; the attached client is told (a relay from
    /// before the state says nothing).
    pub fn set_state(&mut self, id: &SessionId, state: TerminalState) {
        let Some(session) = self.sessions.get_mut(id) else {
            return;
        };
        session.state = state.clone();
        if session.attached && self.age == Age::Current {
            self.send(&Packet::about(id.clone(), Message::TerminalState(state)));
        }
    }

    /// What the relay says of the terminal's times from now on (a relay from before
    /// them, [`Age::Old`], says none).
    pub fn set_activity(&mut self, id: &SessionId, activity: Activity) {
        if let Some(session) = self.sessions.get_mut(id) {
            session.activity = activity;
        }
    }

    /// The terminal is given a label from now on.
    pub fn set_label(&mut self, id: &SessionId, label: &str) {
        if let Some(session) = self.sessions.get_mut(id) {
            session.label = Some(label.to_owned());
        }
    }

    /// The program exits. An attached client is told; otherwise the terminal
    /// waits for one to attach.
    pub fn exit(&mut self, id: &SessionId, exit: Exit) {
        let Some(session) = self.sessions.get_mut(id) else {
            return;
        };
        session.exit = Some(exit);
        if session.attached {
            self.tell_ended(id);
        }
    }

    /// Another client takes the terminal: ours is let go and told by whom.
    pub fn take_by(&mut self, id: &SessionId, who: &str) {
        let Some(session) = self.sessions.get_mut(id) else {
            return;
        };
        if session.attached {
            session.attached = false;
            self.send(&Packet::about(
                id.clone(),
                Message::Error {
                    code: ErrorCode::TAKEN,
                    text: who.to_owned(),
                },
            ));
        }
    }

    /// The clients attached to the terminal on other connections, each by the name it gave
    /// itself (`None`: it gave none): who `status` lists beside this connection's client. The
    /// fake is one connection's far end, so another client is only what the server knows of it —
    /// its name and that it counts as attached — and what happens to it is observed with
    /// [`Bridge::others_of`].
    pub fn set_others(&mut self, id: &SessionId, others: &[Option<&str>]) {
        if let Some(session) = self.sessions.get_mut(id) {
            session.others = others.iter().map(|name| name.map(str::to_owned)).collect();
        }
    }

    /// The clients attached on other connections that are still attached: an `attach` that takes
    /// the terminal lets them go.
    pub fn others_of(&self, id: &SessionId) -> Vec<Option<String>> {
        self.sessions
            .get(id)
            .map(|s| s.others.clone())
            .unwrap_or_default()
    }

    /// Refuses the next `n` attaches to the terminal with `UNAVAILABLE`, as a
    /// bridge does for a session whose process does not answer.
    pub fn refuse_attaches(&mut self, id: &SessionId, n: u32) {
        if let Some(session) = self.sessions.get_mut(id) {
            session.unavailable = n;
        }
    }

    /// Sends the client a packet as it stands, for the cases a server should
    /// never produce.
    pub fn inject(&mut self, packet: &Packet) {
        self.send(packet);
    }

    // The connection

    /// The connection ends. The terminals stay; whatever the client had not yet
    /// read is lost with it.
    pub fn disconnect(&mut self) {
        self.hello.clear();
        self.greeted = false;
        self.shut = false;
        self.decoder = IncomingDecoder::new();
        self.out.clear();
        for session in self.sessions.values_mut() {
            session.attached = false;
        }
        // The mailbox goes with the connection to the app: its tickets and what it said of
        // itself are of the connection.
        self.tickets = false;
        self.waiting.clear();
        self.talks.clear();
    }

    /// What the bridge has to say, taken.
    pub fn take_output(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.out)
    }

    /// Bytes arrived from the client.
    pub fn feed(&mut self, bytes: &[u8]) {
        if self.shut {
            return;
        }
        if self.greeted {
            self.decoder.push(bytes);
            self.serve();
            return;
        }
        self.hello.extend_from_slice(bytes);
        let mut slice: &[u8] = &self.hello;
        let protocol = Protocol::new(SIGNATURE, self.range.0, self.range.1);
        match protocol.read_hello(&mut slice) {
            Ok((min, max)) => {
                let used = self.hello.len() - slice.len();
                let after = self.hello.split_off(used);
                self.hello.clear();
                let selected = negotiate(self.range, (min, max));
                self.out.extend_from_slice(&self.banner);
                protocol
                    .write_reply(&mut self.out, selected.unwrap_or(0))
                    .expect("a hello fits a vec");
                if selected.is_none() {
                    self.shut = true;
                    return;
                }
                self.greeted = true;
                self.connections += 1;
                self.decoder.push(&after);
                self.serve();
            }
            // The hello is not whole yet.
            Err(Error::Io(_)) => {}
            Err(_) => self.shut = true,
        }
    }

    // The program behind `kablo app`

    /// A program on the server opens a conversation on the mailbox, giving `window` (the bytes
    /// the app may send it before it credits the app). The ticket, when the bridge gives one:
    /// not to a client that did not say it takes tickets, nor from a bridge without a mailbox.
    /// `from` is the terminal the program runs in, as the bridge found it.
    pub fn mail(&mut self, from: Option<&SessionId>, window: u32) -> Option<u32> {
        if !self.mailbox || !self.tickets || self.age == Age::Old {
            return None;
        }
        self.last_ticket += 1;
        let ticket = self.last_ticket;
        self.waiting.insert(ticket, window);
        self.send(&Packet {
            session: from.cloned(),
            message: Message::Ticket(ticket),
        });
        Some(ticket)
    }

    /// The program gives its conversation up before the app answered: the ticket is forgotten
    /// (the app is told nothing; its `open` is refused when it comes).
    pub fn mail_given_up(&mut self, ticket: u32) {
        self.waiting.remove(&ticket);
    }

    /// The program sends bytes into the conversation the app's stream `stream` is the end of.
    pub fn sender_data(&mut self, stream: u32, bytes: &[u8]) {
        let Some(talking) = self.talks.get_mut(&stream) else {
            return;
        };
        let size = u32::try_from(bytes.len()).unwrap_or(u32::MAX);
        if talking.from_sender.eof || size > talking.from_sender.left {
            self.breaches += 1;
            return;
        }
        talking.from_sender.left -= size;
        self.send_stream(stream, stream::Message::Data(bytes.to_vec()));
    }

    /// The program has nothing more to send.
    pub fn sender_eof(&mut self, stream: u32) {
        if let Some(talking) = self.talks.get_mut(&stream) {
            talking.from_sender.eof = true;
            self.send_stream(stream, stream::Message::Eof);
        }
    }

    /// The program ends the conversation (its `kablo app` was interrupted).
    pub fn sender_close(&mut self, stream: u32) {
        if self.talks.remove(&stream).is_some() {
            self.send_stream(stream, stream::Message::Close);
        }
    }

    /// The program's side has taken `bytes` more of what the app said and the app may send
    /// that many more.
    pub fn sender_credit(&mut self, stream: u32, bytes: u32) {
        if let Some(talking) = self.talks.get_mut(&stream) {
            talking.from_app.left = talking.from_app.left.saturating_add(bytes);
            self.send_stream(stream, stream::Message::Credit(bytes));
        }
    }

    /// Whether the program takes what the app says as it comes (it does, until a test says
    /// otherwise): when it does not, the app's `data` is held to the window until
    /// [`Bridge::sender_credit`].
    pub fn sender_reads(&mut self, on: bool) {
        self.sender_reads = on;
    }

    /// What the app said in the conversation, in order.
    pub fn said_to_sender(&self, stream: u32) -> Vec<u8> {
        self.talks
            .get(&stream)
            .map(|t| t.said.clone())
            .unwrap_or_default()
    }

    /// How many bytes the app has credited the program with: what it took of what was sent.
    pub fn credited_by_app(&self, stream: u32) -> u32 {
        self.talks.get(&stream).map_or(0, |t| t.credit_from_app)
    }

    /// Whether the app said it has nothing more.
    pub fn app_finished(&self, stream: u32) -> bool {
        self.talks.get(&stream).is_some_and(|t| t.from_app.eof)
    }

    /// Whether the app ended the conversation with a `close`.
    pub fn app_closed(&self, stream: u32) -> bool {
        self.talks.get(&stream).is_some_and(|t| t.closed)
    }

    /// The tickets the app declined, with its words.
    pub fn declined(&self) -> &[(u32, String)] {
        &self.declined
    }

    /// How many times an end sent more than it was given, or after its own `eof`: the real
    /// bridge ends the conversation at the first.
    pub fn breaches(&self) -> u32 {
        self.breaches
    }

    // What a test looks at

    /// The view and offset of every `attach` the client sent for a terminal.
    pub fn attaches(&self, id: &SessionId) -> Vec<(u64, Option<View>)> {
        self.sessions
            .get(id)
            .map(|s| s.views.clone())
            .unwrap_or_default()
    }

    /// What the client typed into the terminal.
    pub fn typed(&self, id: &SessionId) -> Vec<u8> {
        self.sessions
            .get(id)
            .map(|s| s.typed.clone())
            .unwrap_or_default()
    }

    /// The size the terminal has.
    pub fn size_of(&self, id: &SessionId) -> Option<Size> {
        self.sessions.get(id).map(|s| s.size)
    }

    /// Whether a client is attached to the terminal.
    pub fn is_attached(&self, id: &SessionId) -> bool {
        self.sessions.get(id).is_some_and(|s| s.attached)
    }

    /// Whether the bridge still has the terminal.
    pub fn has(&self, id: &SessionId) -> bool {
        self.sessions.contains_key(id)
    }

    /// The command the terminal was made with.
    pub fn argv_of(&self, id: &SessionId) -> Option<&[Vec<u8>]> {
        self.sessions.get(id).map(|s| s.argv.as_slice())
    }

    /// The environment the terminal was made with, over the defaults.
    pub fn env_of(&self, id: &SessionId) -> Option<&[(Vec<u8>, Vec<u8>)]> {
        self.sessions.get(id).map(|s| s.env.as_slice())
    }

    /// How `terminate` was asked for the terminal (`Some(force)`).
    pub fn terminated(&self, id: &SessionId) -> Option<bool> {
        self.sessions.get(id).and_then(|s| s.terminated)
    }

    /// How many times the terminal was asked to forget its past.
    pub fn forgotten(&self, id: &SessionId) -> u32 {
        self.sessions.get(id).map_or(0, |s| s.forgotten)
    }

    /// The bytes of the picture of a terminal whose program has written up to
    /// `end`: what a client writes to a freshly reset terminal to see the screen.
    pub fn picture_of(end: u64) -> Vec<u8> {
        format!("<screen at {end}>").into_bytes()
    }

    // Serving

    fn send(&mut self, packet: &Packet) {
        packet
            .encode(&mut self.out)
            .expect("a packet of the bridge's own encodes");
    }

    fn error(&mut self, session: Option<&SessionId>, code: ErrorCode, text: &str) {
        let packet = Packet {
            session: session.cloned(),
            message: Message::Error {
                code,
                text: text.to_owned(),
            },
        };
        self.send(&packet);
    }

    fn version(&self) -> &'static str {
        match self.age {
            Age::Current => "0.2.0",
            Age::Old => "0.1.0",
        }
    }

    fn status(&self, id: &SessionId) -> Option<Status> {
        let session = self.sessions.get(id)?;
        let mut status = Status::new(id.clone());
        status.pid = FAKE_PID;
        status.size = session.size;
        status.clients =
            u32::from(session.attached) + u32::try_from(session.others.len()).unwrap_or(u32::MAX);
        if self.age == Age::Current {
            // One entry per attached client, in the order they attached: the others were
            // attached before this connection's client came to take its turn.
            status.clients_named = session
                .others
                .iter()
                .map(|name| name.as_deref().and_then(ClientName::new))
                .chain(session.attached.then_some(session.name))
                .collect();
        }
        status.offset = session.end();
        status.retained_from = session.retained_from;
        status.state = if session.exit.is_some() {
            State::ENDED
        } else {
            State::RUNNING
        };
        status.exit = session.exit;
        status.version = self.version().to_owned();
        status.program = session.state.program.clone();
        status.cwd = session.state.cwd.clone();
        if self.age == Age::Current {
            status.label = session.label.clone();
            status.started = session.activity.started;
            status.front_started = session.activity.front_started;
            status.front_is_own = session.activity.front_is_own;
            status.output_at = session.activity.output_at;
        }
        Some(status)
    }

    fn serve(&mut self) {
        loop {
            match self.decoder.next_incoming() {
                Ok(Some(Incoming::Session(packet))) => {
                    self.received.push(packet.clone());
                    self.answer(packet);
                }
                Ok(Some(Incoming::Stream {
                    from: Side::Client,
                    packet,
                })) => {
                    self.received_streams.push(packet.clone());
                    self.streamed(packet);
                }
                Ok(Some(_)) => {}
                Ok(None) => break,
                Err(e) if e.is_recoverable() => {}
                Err(_) => {
                    self.shut = true;
                    break;
                }
            }
        }
    }

    fn answer(&mut self, packet: Packet) {
        let Packet { session, message } = packet;
        match message {
            Message::New(spec) => self.make(session, spec),
            Message::List => {
                let all: Vec<Status> = self
                    .sessions
                    .keys()
                    .filter_map(|id| self.status(id))
                    .collect();
                self.send(&Packet::connection(Message::ListReply(all)));
            }
            Message::Ping(bytes) => self.send(&Packet::connection(Message::Pong(bytes))),
            Message::Features | Message::FeaturesWith(_) if self.age == Age::Old => {}
            Message::Features => self.features(false),
            Message::FeaturesWith(offers) => self.features(offers.tickets),
            Message::Decline { ticket, text } => {
                if self.waiting.remove(&ticket).is_some() {
                    self.declined.push((ticket, text));
                }
            }
            other => {
                let Some(id) = session else {
                    self.error(None, ErrorCode::BAD_REQUEST, "no session");
                    return;
                };
                if !self.sessions.contains_key(&id) {
                    self.error(Some(&id), ErrorCode::UNKNOWN_SESSION, "no such terminal");
                    return;
                }
                self.about(id, other);
            }
        }
    }

    fn features(&mut self, tickets: bool) {
        self.tickets = tickets;
        let features = Features::with_streams();
        let features = if self.mailbox {
            features.with_mailbox()
        } else {
            features
        };
        self.send(&Packet::connection(Message::FeaturesReply(features)));
    }

    fn send_stream(&mut self, stream: u32, message: stream::Message) {
        stream::Packet::new(stream, message)
            .encode(Side::Server, &mut self.out)
            .expect("a stream message of the bridge's own encodes");
    }

    /// A message of the client on channel 1: the app answering a ticket, or talking.
    fn streamed(&mut self, packet: stream::Packet) {
        let stream::Packet { stream, message } = packet;
        match message {
            stream::Message::OpenTicket { window, ticket } => {
                let Some(given) = self.waiting.remove(&ticket) else {
                    self.send_stream(
                        stream,
                        stream::Message::Failed {
                            code: FailCode::BAD_REQUEST,
                            text: "no such ticket".to_owned(),
                        },
                    );
                    return;
                };
                self.talks.insert(
                    stream,
                    Talking {
                        from_sender: Leg {
                            left: window,
                            eof: false,
                        },
                        from_app: Leg {
                            left: given,
                            eof: false,
                        },
                        said: Vec::new(),
                        credit_from_app: 0,
                        closed: false,
                    },
                );
                self.send_stream(stream, stream::Message::Opened { window: given });
            }
            stream::Message::Data(bytes) => {
                let reads = self.sender_reads;
                let Some(talking) = self.talks.get_mut(&stream) else {
                    return;
                };
                let size = u32::try_from(bytes.len()).unwrap_or(u32::MAX);
                if talking.from_app.eof || bytes.len() > MAX_DATA || size > talking.from_app.left {
                    self.breaches += 1;
                    return;
                }
                talking.said.extend_from_slice(&bytes);
                if reads {
                    self.send_stream(stream, stream::Message::Credit(size));
                } else {
                    talking.from_app.left -= size;
                }
            }
            stream::Message::Credit(bytes) => {
                if let Some(talking) = self.talks.get_mut(&stream) {
                    talking.from_sender.left = talking.from_sender.left.saturating_add(bytes);
                    talking.credit_from_app += bytes;
                }
            }
            stream::Message::Eof => {
                if let Some(talking) = self.talks.get_mut(&stream) {
                    if talking.from_app.eof {
                        self.breaches += 1;
                    }
                    talking.from_app.eof = true;
                }
            }
            stream::Message::Close => {
                if let Some(talking) = self.talks.get_mut(&stream) {
                    talking.closed = true;
                }
            }
            _ => {}
        }
    }

    fn make(&mut self, session: Option<SessionId>, spec: NewSession) {
        let id = match session {
            Some(id) => id,
            None => {
                self.made += 1;
                SessionId::new(&format!("s{}", self.made)).expect("a session id")
            }
        };
        if self.sessions.contains_key(&id) {
            self.error(Some(&id), ErrorCode::SESSION_EXISTS, "it exists");
            return;
        }
        let mut session = Session::new(spec.size.unwrap_or_default());
        session.argv = spec.argv.clone();
        session.env = spec.env.clone();
        self.sessions.insert(id.clone(), session);
        if let Some(status) = self.status(&id) {
            self.send(&Packet::about(id, Message::Created(status)));
        }
    }

    fn about(&mut self, id: SessionId, message: Message) {
        match message {
            Message::Attach { offset, view } => self.attach(id, offset, view),
            Message::Detach => {
                if let Some(session) = self.sessions.get_mut(&id) {
                    session.attached = false;
                }
            }
            Message::Resize(size) => {
                let attached = self.sessions.get(&id).is_some_and(|s| s.attached);
                if attached {
                    if let Some(session) = self.sessions.get_mut(&id) {
                        session.size = size;
                    }
                } else {
                    self.error(Some(&id), ErrorCode::NOT_ATTACHED, "not attached");
                }
            }
            Message::Input(bytes) => {
                let attached = self.sessions.get(&id).is_some_and(|s| s.attached);
                if attached {
                    if let Some(session) = self.sessions.get_mut(&id) {
                        session.typed.extend_from_slice(&bytes);
                    }
                } else {
                    self.error(Some(&id), ErrorCode::NOT_ATTACHED, "not attached");
                }
            }
            Message::Terminate { force } => {
                if let Some(session) = self.sessions.get_mut(&id) {
                    session.terminated = Some(force);
                }
                if force || !self.stubborn {
                    self.exit(&id, Exit::Signal(if force { 9 } else { 1 }));
                }
            }
            Message::Status => {
                if let Some(status) = self.status(&id) {
                    self.send(&Packet::about(id, Message::StatusReply(status)));
                }
            }
            // The past goes: the journal and, with it, the offsets before the present.
            Message::Forget => {
                let attached = self.sessions.get(&id).is_some_and(|s| s.attached);
                if attached {
                    if let Some(session) = self.sessions.get_mut(&id) {
                        session.retained_from = session.end();
                        session.journal.clear();
                        session.forgotten += 1;
                    }
                } else {
                    self.error(Some(&id), ErrorCode::NOT_ATTACHED, "not attached");
                }
            }
            _ => {}
        }
    }

    fn attach(&mut self, id: SessionId, offset: u64, view: Option<View>) {
        let current = self.age == Age::Current;
        let Some(session) = self.sessions.get_mut(&id) else {
            return;
        };
        session.views.push((offset, view));
        if session.unavailable > 0 {
            session.unavailable -= 1;
            self.error(Some(&id), ErrorCode::UNAVAILABLE, "it does not answer");
            return;
        }
        let end = session.end();
        let from = if offset == OFFSET_END { end } else { offset };
        if from > end {
            self.error(Some(&id), ErrorCode::BAD_REQUEST, "past the end");
            return;
        }
        // A relay from before views reads the offset and nothing else.
        let view = view.filter(|_| current);
        if let Some(view) = &view {
            if view.size.cols > 0 && view.size.rows > 0 {
                session.size = view.size;
            }
            session.keep |= view.keep_after_end;
            session.name = view.name;
            // Taking: whoever else is attached is let go (a relay from before the bit ignores it
            // and attaches this client beside them).
            if view.take {
                session.others.clear();
            }
        }
        let gone = from < session.retained_from;
        let question = view.is_some_and(|v| v.picture_if_answered) && session.answered > from;
        let picture = current && (gone || question || view.is_some_and(|v| v.picture));
        let truncated = gone || question;
        let start = if picture {
            end
        } else {
            from.max(session.retained_from)
        };
        let answered = if current { session.answered } else { 0 };
        let data = session.from(start).to_vec();
        let state = session.state.clone();
        let size = session.size;
        session.attached = true;
        session.sent = end;
        self.send(&Packet::about(
            id.clone(),
            Message::Attached {
                offset: start,
                truncated,
                answered,
            },
        ));
        if current {
            self.send(&Packet::about(id.clone(), Message::TerminalState(state)));
        }
        if picture {
            let bytes = Bridge::picture_of(end);
            let (head, tail) = bytes.split_at(bytes.len() / 2);
            for (piece, more) in [(head, true), (tail, false)] {
                self.send(&Packet::about(
                    id.clone(),
                    Message::Picture {
                        offset: end,
                        cols: size.cols,
                        rows: size.rows,
                        more,
                        data: piece.to_vec(),
                    },
                ));
            }
        }
        if !data.is_empty() {
            self.send(&Packet::about(
                id.clone(),
                Message::Output {
                    offset: start,
                    data,
                },
            ));
        }
        if self.sessions.get(&id).is_some_and(|s| s.exit.is_some()) {
            self.tell_ended(&id);
        }
    }

    /// The attached client is told the program ended. The terminal is then
    /// removed, unless an app that asked for it to be kept has attached to it
    /// (and the relay is one that keeps).
    fn tell_ended(&mut self, id: &SessionId) {
        let current = self.age == Age::Current;
        let Some(session) = self.sessions.get_mut(id) else {
            return;
        };
        let Some(exit) = session.exit else { return };
        session.told = true;
        let keep = session.keep && current;
        self.send(&Packet::about(id.clone(), Message::Ended(exit)));
        if !keep {
            self.sessions.remove(id);
        }
    }
}
