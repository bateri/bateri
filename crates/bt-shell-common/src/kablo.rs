//! The client of a kablo server's bridge: terminals that run on the server
//! and outlive the connection to it.
//!
//! kablo keeps a terminal's program running on the server. A client talks to
//! the server's bridge (`kablo attach`) over any ordered byte stream — bateri
//! runs it as one channel of the host's ssh connection — and the bridge serves
//! every terminal of the server on that one stream, each message naming its
//! terminal. This module is that client, written once and **without a
//! transport**: it is a state machine the owner feeds with the bytes that
//! arrive ([`Client::push`]), asks for the bytes to send ([`Client::take_outgoing`])
//! and for what happened ([`Client::next_event`]). It opens no socket, starts
//! no process and reads no clock: the time of every step comes in as an
//! `Instant`, so a test steps minutes in no time and the Linux shell can bring
//! its own transport. The codecs are kablo's own (`kablo-proto`); nothing of
//! the wire is rewritten here.
//!
//! **A terminal's bytes are exact.** The client keeps, per terminal, the offset
//! of the next byte it is owed. After a lost connection it attaches again from
//! that offset and the server sends exactly what was missed, so the output
//! the owner gets is the program's output with no byte twice and none
//! missing. When the server no longer holds those bytes it sends a *picture*
//! of the screen instead ([`Event::Picture`]) and the stream goes on from
//! there. Every attach also says the owner's terminal **answers the questions
//! it is shown** (`picture_if_answered`): output that holds a question the
//! server has already answered is replaced by a picture, so a program is never
//! answered twice, and keeps an ended terminal readable for ten minutes
//! (`keep_after_end`), so a program that comes after the owner can still read
//! the last screen. Both are set on every attach, by the client, not asked of
//! the owner.
//!
//! **The connection's life** is [`Link`]: retrying, opening, live, needing the
//! user to log in, over. The owner dials when [`Client::dial_due`] says so and
//! reports what happened ([`Client::connected`], [`Client::lost`]); the client
//! backs off between attempts ([`retry_delay`]), checks that a quiet
//! connection is alive with a `ping` (a laptop that slept leaves a connection
//! that looks open and is not), and attaches its terminals again the moment a
//! new connection is up. [`Client::retry_now`] and [`Client::wake`] are the
//! hooks for "try now" and "the computer woke".
//!
//! `hub` is the application's end of the pipe a pane's program makes to its terminal
//! (`crate::hold`): it ties each such program to one terminal of a client and
//! carries bytes both ways, also without a transport.
//!
//! `service` is that end running: the socket, the one loop that drives the hub and the
//! clients, and the threads that carry bytes; `ssh` is the service's steps over
//! ssh (the master connection on the pane's terminal, the install, the bridge) and
//! `install` the rules for putting kablo on a server from the package's files.
//!
//! `local` is this Mac as an account of its own: the same steps with the transport changed (a
//! shell in the home directory for an ssh channel, a `kablo attach` started here for the
//! one at the far end of an ssh), so a pane of this Mac can be a terminal kablo keeps.
//!
//! `mail` is the other direction: what a program on the server (`kablo app`) hands the
//! app, as tickets the bridge gives a client that said it takes them and the
//! conversations the client opens to answer them (`take_tickets`, `accept`, `say`,
//! `finish`); the service turns a conversation into one letter for the application.
//!
//! `fake` (under `cfg(test)` and the `test-support` feature) is the server side of
//! the same pipe for tests: the bridge with a few terminals, a journal of a chosen
//! size, a mailbox and an old version of itself.

use std::collections::{BTreeMap, VecDeque};
use std::time::{Duration, Instant};

pub use kablo_proto::v1::{ErrorCode, Exit, NewSession, Size, Status, TerminalState};
pub use kablo_proto::{Error, SessionId};
pub use mail::{FailCode, Over, Talk};

use kablo_proto::v1::stream::{Incoming, IncomingDecoder};
use kablo_proto::v1::{Capture, ClientName, MAX_HISTORY, Message, OFFSET_END, Packet, View};
use kablo_proto::{NOISE_LIMIT, WIRE};

#[cfg(any(test, feature = "test-support"))]
pub mod fake;
pub mod hub;
pub mod install;
pub mod ledger;
pub mod local;
pub mod mail;
pub mod quit;
pub mod roster;
pub mod service;
pub mod ssh;
#[cfg(test)]
mod tests;

/// The pauses between attempts to reach a server, by how many have failed in a
/// row: the first retry comes soon (most breaks are a blink), then the pause
/// doubles up to the last one, which repeats.
pub const BACKOFF: [Duration; 6] = [
    Duration::from_secs(1),
    Duration::from_secs(2),
    Duration::from_secs(4),
    Duration::from_secs(8),
    Duration::from_secs(16),
    Duration::from_secs(30),
];

/// How long a connection must have lived before its loss counts as a fresh
/// one. A connection that is made and dropped at once, over and over, must not
/// start the pauses over every time: it would be retried every second for as
/// long as the server flaps.
pub const STABLE: Duration = Duration::from_secs(10);

/// How long a live connection may be silent before the client asks whether the
/// other end is there (a `ping`).
pub const PULSE_IDLE: Duration = Duration::from_secs(20);

/// How long the answer to a `ping` may take before the connection is given up.
pub const PULSE_REPLY: Duration = Duration::from_secs(10);

/// How long the handshake of a new connection may take: a far end that never
/// starts the bridge must not be waited for forever.
pub const OPEN_WAIT: Duration = Duration::from_secs(20);

/// The most bytes of one `input` message; a longer paste goes as several, in
/// order (the protocol's own limit is a megabyte).
const INPUT_CHUNK: usize = 256 * 1024;

/// How long to wait for the signature of the server's hello before giving up
/// on what has piled up: the noise the protocol skips, and the hello itself.
const HANDSHAKE_LIMIT: usize = NOISE_LIMIT + 16;

/// The pause before the `attempt`th retry (0 is the first after a loss).
pub fn retry_delay(attempt: u32) -> Duration {
    let last = BACKOFF.last().copied().unwrap_or(Duration::from_secs(30));
    usize::try_from(attempt)
        .ok()
        .and_then(|at| BACKOFF.get(at))
        .copied()
        .unwrap_or(last)
}

/// Why a connection is not there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cause {
    /// The transport closed, could not be made or went silent: try again.
    Dropped,
    /// The far end broke the protocol (a frame that cannot be trusted, bytes
    /// where none were owed): the connection is dropped and made again.
    Protocol,
    /// The server wants the user to log in (a password, a second factor)
    /// before the transport can be made. Nothing is retried until the owner
    /// says [`Client::retry_now`]: guessing a password at a backoff is a lockout.
    LoginNeeded,
    /// The owner says not to try again (a host key that changed, a server
    /// kablo cannot be kept on).
    Refused,
    /// The far end speaks no protocol version this build speaks, or no kablo
    /// at all: retrying cannot change it.
    Incompatible,
}

/// Where the connection to the server stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Link {
    /// No connection. The owner dials at `at` or later ([`Client::dial_due`]);
    /// `attempt` is how many attempts in a row have failed.
    Retrying {
        /// The earliest moment to dial.
        at: Instant,
        /// How many attempts in a row have failed.
        attempt: u32,
    },
    /// The transport is up and the handshake is in flight.
    Opening {
        /// When the transport came up.
        since: Instant,
    },
    /// The handshake is done; terminals can be attached and typed to.
    Live,
    /// Waiting for the user to log in; see [`Cause::LoginNeeded`].
    NeedsLogin,
    /// Over: no attempt will be made unless the owner builds a new client.
    Over(Cause),
}

impl Link {
    /// Whether messages can be sent now.
    pub fn is_live(&self) -> bool {
        matches!(self, Link::Live)
    }
}

/// How the client wants to look at a terminal when it attaches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Attach {
    /// The owner's terminal. The server's terminal takes this size before the
    /// first byte is sent.
    pub size: Size,
    /// The first byte wanted ([`OFFSET_END`]: from now on).
    pub offset: u64,
    /// Start with a picture of the screen whatever the offset: for a terminal
    /// the owner has never seen.
    pub picture: bool,
    /// Take the terminal: every other client attached is let go, told by whom.
    /// Asked of the first attach only; a connection that breaks and attaches
    /// again does not take a second time (it would let go whoever came after).
    pub take: bool,
    /// Ask who has the terminal before attaching, and do not attach beside another
    /// computer: if one with a name is attached, the terminal is [`Event::Taken`] by it
    /// instead. For a terminal this client had and lost sight of (an application that was
    /// restarted); a connection that breaks does it by itself.
    pub check: bool,
    /// The owner reads the bytes it is given from a terminal of its own that holds back the
    /// answers to the questions in the part the server has answered already (it is told how far
    /// that is, [`Event::Attached`]'s `answered`): the first attach does not ask for a picture
    /// instead of such bytes. A terminal the owner has been showing, whose connection broke, is
    /// attached again the ordinary way. For a terminal shown again after the bateri that showed it
    /// was gone, whose bytes are all there is to build the screen from.
    pub replaying: bool,
}

impl Attach {
    /// Attach from the first byte at this size, taking nothing.
    pub fn new(size: Size) -> Attach {
        Attach {
            size,
            offset: 0,
            picture: false,
            take: false,
            check: false,
            replaying: false,
        }
    }

    /// From this byte on.
    pub fn from(self, offset: u64) -> Attach {
        Attach { offset, ..self }
    }

    /// From now on: nothing of what was written before.
    pub fn from_now(self) -> Attach {
        self.from(OFFSET_END)
    }

    /// With a picture of the screen first.
    pub fn with_picture(self) -> Attach {
        Attach {
            picture: true,
            ..self
        }
    }

    /// Taking the terminal from whoever has it.
    pub fn taking(self) -> Attach {
        Attach { take: true, ..self }
    }

    /// Asking who has the terminal first, and not attaching beside another computer.
    pub fn checking(self) -> Attach {
        Attach {
            check: true,
            ..self
        }
    }

    /// Holding back the answers itself ([`Attach::replaying`]).
    pub fn replaying(self) -> Attach {
        Attach {
            replaying: true,
            ..self
        }
    }
}

/// What happened, in order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// A connection is up and its handshake is done. Terminals that were
    /// attached are attached again by the client itself.
    Linked,
    /// The connection is gone, or could not be made. [`Client::link`] says
    /// what comes next.
    Lost {
        /// Why.
        cause: Cause,
    },
    /// A terminal was started on the server (the answer to
    /// [`Client::new_terminal`]). The client attaches it as asked.
    Created(Status),
    /// A terminal is attached: its output starts at `offset`. When `truncated`
    /// a picture follows in place of the bytes asked for. `answered` is how
    /// far the server has answered the program's questions; the client
    /// does not act on it (the picture flag already keeps a question from being
    /// answered twice, unless the attach was [`Attach::replaying`]) and passes it on for
    /// whoever shows the bytes some other way.
    Attached {
        /// The terminal.
        terminal: SessionId,
        /// Where the output that follows starts.
        offset: u64,
        /// A picture follows in place of the bytes asked for.
        truncated: bool,
        /// How far the program's questions are answered.
        answered: u64,
    },
    /// A piece of the screen as the bytes that build it. A picture is one or
    /// more pieces in order; the owner **resets its terminal before a piece with
    /// `first`** and writes the bytes as they are. A `first` after pieces that
    /// never finished means the first picture was thrown away: reset again.
    Picture {
        /// The terminal.
        terminal: SessionId,
        /// The offset of the program's output the picture is of; the stream
        /// goes on from it.
        offset: u64,
        /// The columns of the terminal the picture was made for.
        cols: u16,
        /// The rows of the terminal the picture was made for.
        rows: u16,
        /// The first piece: reset the terminal first.
        first: bool,
        /// The last piece: the stream goes on from `offset`.
        last: bool,
        /// The bytes.
        data: Vec<u8>,
    },
    /// The answer to [`Client::read_picture`]: the screen of a terminal this client does not
    /// attach, as the bytes that build it when they are written to a freshly reset terminal of
    /// `cols` × `rows`. `offset` is how much of the program's output it is of — the same offset,
    /// the same screen.
    Screen {
        /// The terminal.
        terminal: SessionId,
        /// The offset of the program's output the screen is of.
        offset: u64,
        /// The columns of the terminal the picture was made for.
        cols: u16,
        /// The rows of the terminal the picture was made for.
        rows: u16,
        /// The bytes, the pieces together.
        data: Vec<u8>,
    },
    /// Program output, exactly as it was written, in order.
    Output {
        /// The terminal.
        terminal: SessionId,
        /// The bytes.
        data: Vec<u8>,
    },
    /// What the terminal is doing that its bytes do not say: the program in
    /// front, its directory, whether it asks for a password. The whole of it,
    /// each time ([`Client::state_of`] keeps the last).
    State {
        /// The terminal.
        terminal: SessionId,
        /// What it is doing.
        state: TerminalState,
    },
    /// The program ended. The terminal is not attached again.
    Ended {
        /// The terminal.
        terminal: SessionId,
        /// How it ended.
        exit: Exit,
    },
    /// Another client took the terminal. It is not attached again unless the
    /// owner attaches it.
    Taken {
        /// The terminal.
        terminal: SessionId,
        /// The name the other client gave itself, controls removed.
        by: Option<String>,
    },
    /// A request failed. `terminal` is the session it was about, if any.
    Failed {
        /// The session.
        terminal: Option<SessionId>,
        /// Why.
        code: ErrorCode,
        /// The server's words, controls removed.
        text: String,
    },
    /// The answer to [`Client::status`].
    Status(Status),
    /// The answer to [`Client::list`].
    Listed(Vec<Status>),
    /// A program on the server has bytes for the app (`kablo app`): answered with
    /// [`Client::accept`] or [`Client::decline`], at once — the program waits
    /// ten seconds for it. `from` is the terminal the program runs in, found by
    /// the bridge from the program itself (`None`: it runs in none).
    Ticket {
        /// The ticket's number.
        ticket: u32,
        /// The terminal the program runs in.
        from: Option<SessionId>,
    },
    /// The program said these bytes in a conversation the app took. They are
    /// credited back already; the owner bounds what it keeps (see [`mail`]).
    TalkData {
        /// The conversation.
        talk: Talk,
        /// The bytes.
        data: Vec<u8>,
    },
    /// The program has nothing more to say in the conversation; the app may
    /// go on answering.
    TalkEof {
        /// The conversation.
        talk: Talk,
    },
    /// The conversation is over, and how. Not told of an end the owner asked for
    /// ([`Client::hang_up`]).
    TalkOver {
        /// The conversation.
        talk: Talk,
        /// How.
        over: Over,
    },
}

/// Where a terminal is in the client's care.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    /// `new` is asked and not answered; it is asked again after a lost
    /// connection, because it is not known whether it happened.
    Creating,
    /// To be attached the next time the connection is live.
    Wanted,
    /// `attach` is sent, `attached` has not come.
    Asked,
    /// `status` is sent to find out who has the terminal before attaching to it
    /// ([`Attach::check`]); the attach follows the answer, unless another computer has it.
    Checking,
    /// Attached: output flows.
    Attached,
    /// The program ended.
    Ended(Exit),
    /// Another client took it.
    Taken,
    /// The owner detached it.
    Detached,
    /// The server has no such terminal (or refused the request that made it).
    Gone,
}

struct Terminal {
    phase: Phase,
    /// The offset of the next byte of output owed.
    next: u64,
    size: Size,
    /// A picture was asked for and `attached` has not come yet.
    picture_wanted: bool,
    /// A take was asked for and `attached` has not come yet (so it did not
    /// happen, and is asked again).
    take_wanted: bool,
    /// Who has the terminal is asked before the next attach ([`Attach::check`]).
    check_wanted: bool,
    /// Who took the terminal, while it is `Taken` (`None`: nobody said, or it is not taken).
    taken_by: Option<String>,
    /// A picture has begun and its last piece has not come.
    picture_open: bool,
    state: Option<TerminalState>,
    /// The request that makes the terminal, while it is `Creating`.
    spec: Option<NewSession>,
    /// `new` has been put on a connection.
    sent: bool,
    /// `new` was sent again after a lost connection.
    resent: bool,
    /// `status` is asked to find out whether a resent `new` had happened.
    adopting: bool,
    /// The server could not serve an attach just now: try again then.
    retry: Option<Instant>,
    /// How many attaches in a row the server could not serve.
    unavailable: u32,
    /// The first attach holds the answers back itself ([`Attach::replaying`]) and has not been
    /// served yet.
    replay_wanted: bool,
}

impl Terminal {
    fn new(phase: Phase, attach: Attach) -> Terminal {
        Terminal {
            phase,
            next: attach.offset,
            size: attach.size,
            picture_wanted: attach.picture,
            take_wanted: attach.take,
            check_wanted: attach.check && !attach.take,
            taken_by: None,
            picture_open: false,
            state: None,
            spec: None,
            sent: false,
            resent: false,
            adopting: false,
            retry: None,
            unavailable: 0,
            replay_wanted: attach.replaying,
        }
    }
}

/// The client of one bridge connection and of the terminals on it. See the
/// module documentation.
pub struct Client {
    name: Option<ClientName>,
    link: Link,
    /// Attempts in a row that failed; the next retry waits `retry_delay` of it.
    failures: u32,
    /// When the handshake of the live connection finished.
    linked_at: Option<Instant>,
    /// Bytes of the handshake that have not made it whole.
    handshake: Vec<u8>,
    decoder: IncomingDecoder,
    outbox: Vec<u8>,
    events: VecDeque<Event>,
    terminals: BTreeMap<SessionId, Terminal>,
    /// The pictures asked for with [`Client::read_picture`] and not whole yet: each terminal's
    /// pieces so far. Terminals this client has no attach for, so nothing of `terminals`.
    reads: BTreeMap<SessionId, Vec<u8>>,
    /// When anything last came from the far end.
    heard: Instant,
    /// When a `ping` was sent that nothing has come after.
    ping: Option<Instant>,
    /// What the app takes from programs on the server ([`mail`]).
    mail: mail::Mail,
}

impl Client {
    /// A client with no connection, to be dialed at once. `name` is what this
    /// computer calls itself in the list of who is attached to a terminal
    /// ("MacBook"); it is cut to the length the protocol allows.
    pub fn new(name: Option<&str>, now: Instant) -> Client {
        Client {
            name: name.and_then(ClientName::clipped),
            link: Link::Retrying {
                at: now,
                attempt: 0,
            },
            failures: 0,
            linked_at: None,
            handshake: Vec::new(),
            decoder: IncomingDecoder::new(),
            outbox: Vec::new(),
            events: VecDeque::new(),
            terminals: BTreeMap::new(),
            reads: BTreeMap::new(),
            heard: now,
            ping: None,
            mail: mail::Mail::default(),
        }
    }

    /// Where the connection stands.
    pub fn link(&self) -> Link {
        self.link
    }

    /// Whether it is time to dial.
    pub fn dial_due(&self, now: Instant) -> bool {
        matches!(self.link, Link::Retrying { at, .. } if now >= at)
    }

    /// The next moment [`Client::tick`] has something to do (a dial, a pulse, a
    /// handshake that has taken too long, an attach to try again), or `None`
    /// when only the outside can change anything.
    pub fn next_deadline(&self) -> Option<Instant> {
        let link = match self.link {
            Link::Retrying { at, .. } => Some(at),
            Link::Opening { since } => Some(since + OPEN_WAIT),
            Link::Live => Some(match self.ping {
                Some(sent) => sent + PULSE_REPLY,
                None => self.heard + PULSE_IDLE,
            }),
            Link::NeedsLogin | Link::Over(_) => None,
        };
        let retries = self
            .terminals
            .values()
            .filter(|_| self.link.is_live())
            .filter_map(|terminal| terminal.retry);
        link.into_iter().chain(retries).min()
    }

    /// The transport was made: start the handshake. Meant to be called after
    /// [`Client::dial_due`]; a client that is not waiting to dial ignores it.
    pub fn connected(&mut self, now: Instant) {
        if !matches!(self.link, Link::Retrying { .. } | Link::NeedsLogin) {
            return;
        }
        self.reset_connection();
        self.link = Link::Opening { since: now };
        self.heard = now;
        // The hello is a few bytes with no failure to speak of.
        let mut hello = Vec::new();
        if WIRE.write_hello(&mut hello).is_ok() {
            self.outbox.extend_from_slice(&hello);
        }
    }

    /// The transport could not be made, or closed or went bad: the connection
    /// is lost and the next step follows from `cause`. Terminals that were
    /// attached wait to be attached again.
    pub fn lost(&mut self, now: Instant, cause: Cause) {
        if let (Link::Live, Some(since)) = (self.link, self.linked_at)
            && now.saturating_duration_since(since) >= STABLE
        {
            self.failures = 0;
        }
        self.end_talks();
        self.reset_connection();
        for terminal in self.terminals.values_mut() {
            match terminal.phase {
                // A terminal this client had is asked about before it is attached again: a
                // computer that took it while the connection was down could not tell this one.
                Phase::Asked | Phase::Attached => {
                    terminal.phase = Phase::Wanted;
                    terminal.check_wanted |= !terminal.take_wanted;
                }
                Phase::Checking => terminal.phase = Phase::Wanted,
                _ => {}
            }
        }
        self.link = match cause {
            Cause::LoginNeeded => Link::NeedsLogin,
            Cause::Refused | Cause::Incompatible => Link::Over(cause),
            Cause::Dropped | Cause::Protocol => {
                let attempt = self.failures;
                self.failures = self.failures.saturating_add(1);
                Link::Retrying {
                    at: now + retry_delay(attempt),
                    attempt,
                }
            }
        };
        // A picture asked for is not coming over a connection that is gone: it is asked again
        // when the owner wants it.
        self.reads.clear();
        self.events.push_back(Event::Lost { cause });
    }

    /// Dial as soon as the owner can: the user asked ("try now"), or logged in.
    /// Does nothing to a connection that is up or over.
    pub fn retry_now(&mut self, now: Instant) {
        match self.link {
            Link::Retrying { attempt, .. } => self.link = Link::Retrying { at: now, attempt },
            Link::NeedsLogin => {
                self.link = Link::Retrying {
                    at: now,
                    attempt: self.failures,
                }
            }
            Link::Opening { .. } | Link::Live | Link::Over(_) => {}
        }
    }

    /// The computer woke up. A connection that looked open may not be: ask at
    /// once, and give it the usual time to answer. One that is waiting to retry
    /// retries now.
    pub fn wake(&mut self, now: Instant) {
        match self.link {
            Link::Live => self.send_ping(now),
            _ => self.retry_now(now),
        }
    }

    /// Give up on the server: nothing is dialed or sent again.
    pub fn close(&mut self) {
        self.end_talks();
        self.reset_connection();
        self.link = Link::Over(Cause::Refused);
    }

    /// Takes the bytes to write to the transport.
    pub fn take_outgoing(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.outbox)
    }

    /// Whether there are bytes to write.
    pub fn has_outgoing(&self) -> bool {
        !self.outbox.is_empty()
    }

    /// The next thing that happened, if anything did.
    pub fn next_event(&mut self) -> Option<Event> {
        self.events.pop_front()
    }

    /// Bytes arrived from the transport, in whatever pieces. Bytes that arrive
    /// when no connection is open (a transport already declared lost) are dropped.
    pub fn push(&mut self, bytes: &[u8], now: Instant) {
        match self.link {
            Link::Opening { .. } => {
                self.heard = now;
                self.handshake.extend_from_slice(bytes);
                self.finish_handshake(now);
            }
            Link::Live => {
                // Anything from the far end says it is there, a `pong` or not.
                self.heard = now;
                self.ping = None;
                self.decoder.push(bytes);
                self.read_packets(now);
            }
            Link::Retrying { .. } | Link::NeedsLogin | Link::Over(_) => {}
        }
    }

    /// Time passed: sends the pulse, notices a connection that went silent and
    /// a handshake that never finished, and tries again an attach the server
    /// could not serve. Call it at [`Client::next_deadline`] and whenever
    /// convenient; calling it early does nothing.
    pub fn tick(&mut self, now: Instant) {
        match self.link {
            Link::Opening { since } => {
                if now.saturating_duration_since(since) >= OPEN_WAIT {
                    self.lost(now, Cause::Dropped);
                }
            }
            Link::Live => {
                match self.ping {
                    Some(sent) => {
                        if now.saturating_duration_since(sent) >= PULSE_REPLY {
                            self.lost(now, Cause::Dropped);
                            return;
                        }
                    }
                    None => {
                        if now.saturating_duration_since(self.heard) >= PULSE_IDLE {
                            self.send_ping(now);
                        }
                    }
                }
                let due: Vec<SessionId> = self
                    .terminals
                    .iter()
                    .filter(|(_, t)| {
                        t.phase == Phase::Wanted && t.retry.is_some_and(|at| now >= at)
                    })
                    .map(|(id, _)| id.clone())
                    .collect();
                for id in due {
                    self.send_attach(&id);
                }
            }
            Link::Retrying { .. } | Link::NeedsLogin | Link::Over(_) => {}
        }
    }

    // Requests

    /// Starts a terminal on the server and attaches it as `attach` says, the
    /// moment the server has made it. `spec` says what to run (empty: the
    /// user's login shell); the size is `attach`'s and the scrollback the
    /// server keeps for it is cut to what the protocol allows. The owner names
    /// the terminal, so that a request repeated after a lost connection cannot
    /// make two. Nothing is sent if the connection is not live yet: the request
    /// waits for it. Fails, without a trace, on a request the protocol would
    /// refuse (a name already in use here, an argument with a NUL).
    pub fn new_terminal(
        &mut self,
        id: SessionId,
        mut spec: NewSession,
        attach: Attach,
    ) -> Result<(), Error> {
        if self
            .terminals
            .get(&id)
            .is_some_and(|t| !matches!(t.phase, Phase::Gone | Phase::Ended(_)))
        {
            return Err(Error::Malformed("terminal in use"));
        }
        spec.size = Some(attach.size);
        spec.history = spec.history.map(|rows| rows.min(MAX_HISTORY));
        // Whether the server would take it is found out now, not after the
        // connection is up.
        Packet::about(id.clone(), Message::New(spec.clone())).encode(&mut Vec::new())?;
        let mut terminal = Terminal::new(Phase::Creating, attach);
        terminal.spec = Some(spec);
        self.terminals.insert(id.clone(), terminal);
        if self.link.is_live() {
            self.send_new(&id);
        }
        Ok(())
    }

    /// Attaches a terminal that exists on the server — one made earlier, by
    /// this client or another — as `attach` says. Attaches again, with these
    /// parameters, one that is already known (after it was taken, or detached).
    pub fn attach(&mut self, id: SessionId, attach: Attach) {
        self.terminals
            .insert(id.clone(), Terminal::new(Phase::Wanted, attach));
        if self.link.is_live() {
            self.send_attach(&id);
        }
    }

    /// Takes a terminal back that another client took: attached again from the byte owed, at the
    /// size it had, taking it from whoever has it now (the other client is told by whom). `false`
    /// when the terminal was not taken. Like a first attach, the take is asked once: a connection
    /// that breaks before the server answers asks again, one that breaks after does not.
    pub fn take_back(&mut self, id: &SessionId) -> bool {
        let Some(terminal) = self.terminals.get_mut(id) else {
            return false;
        };
        if terminal.phase != Phase::Taken {
            return false;
        }
        terminal.phase = Phase::Wanted;
        terminal.take_wanted = true;
        terminal.taken_by = None;
        if self.link.is_live() {
            self.send_attach(id);
        }
        true
    }

    /// Whether another client took the terminal and it has not been taken back.
    pub fn is_taken(&self, id: &SessionId) -> bool {
        self.terminals
            .get(id)
            .is_some_and(|t| t.phase == Phase::Taken)
    }

    /// The name the client that took the terminal gave itself, controls removed; `None` when it
    /// gave none or the terminal is not taken.
    pub fn taken_by(&self, id: &SessionId) -> Option<&str> {
        self.terminals
            .get(id)
            .filter(|t| t.phase == Phase::Taken)
            .and_then(|t| t.taken_by.as_deref())
    }

    /// Stops the stream of a terminal; it lives on, on the server.
    pub fn detach(&mut self, id: &SessionId) {
        let attached = self
            .terminals
            .get(id)
            .is_some_and(|t| matches!(t.phase, Phase::Asked | Phase::Attached));
        if let Some(terminal) = self.terminals.get_mut(id) {
            if !matches!(terminal.phase, Phase::Ended(_) | Phase::Gone | Phase::Taken) {
                terminal.phase = Phase::Detached;
            }
            terminal.retry = None;
        }
        if attached && self.link.is_live() {
            self.queue(Packet::about(id.clone(), Message::Detach));
        }
    }

    /// Types `bytes` into the program, exactly as they are. `false` when they
    /// were not sent — the connection is not live or the terminal is not
    /// attached — and the owner should say so (a bell), not keep them: a line
    /// typed minutes ago and sent when the connection comes back runs in a
    /// place the user no longer looks at.
    pub fn input(&mut self, id: &SessionId, bytes: &[u8]) -> bool {
        let streaming = self
            .terminals
            .get(id)
            .is_some_and(|t| matches!(t.phase, Phase::Asked | Phase::Attached));
        if !streaming || !self.link.is_live() {
            return false;
        }
        for chunk in bytes.chunks(INPUT_CHUNK) {
            self.queue(Packet::about(id.clone(), Message::Input(chunk.to_vec())));
        }
        true
    }

    /// The owner's terminal changed size. Remembered, so that an attach after a
    /// lost connection says the size it has now.
    pub fn resize(&mut self, id: &SessionId, size: Size) {
        let streaming = match self.terminals.get_mut(id) {
            Some(terminal) => {
                terminal.size = size;
                matches!(terminal.phase, Phase::Asked | Phase::Attached)
            }
            None => false,
        };
        if streaming && self.link.is_live() {
            self.queue(Packet::about(id.clone(), Message::Resize(size)));
        }
    }

    /// Ends the terminal's program (a hang-up; `force`: a kill). Any terminal of
    /// the server, known to this client or not — one a previous bateri left
    /// behind is ended by name. `false` when the connection is not live.
    pub fn terminate(&mut self, id: &SessionId, force: bool) -> bool {
        if !self.link.is_live() {
            return false;
        }
        self.queue(Packet::about(id.clone(), Message::Terminate { force }));
        true
    }

    /// Asks for one terminal's status; the answer is [`Event::Status`].
    pub fn status(&mut self, id: &SessionId) -> bool {
        if !self.link.is_live() {
            return false;
        }
        self.queue(Packet::about(id.clone(), Message::Status));
        true
    }

    /// Asks for the screen of a terminal this client does not attach, as the bytes that build
    /// it (a `read` with `picture`): the answer is [`Event::Screen`], or [`Event::Failed`] about
    /// the terminal when the server has none by that name. Nothing is streamed afterwards, and
    /// nothing of the terminal's other clients is touched: the server counts no attach and
    /// notes no size for it.
    ///
    /// `false` when the connection is not up or the terminal is one this client attaches or is
    /// about to (its own picture comes with the attach). A terminal this client let go of
    /// (detached, taken, ended) is read like one it never had.
    pub fn read_picture(&mut self, id: &SessionId) -> bool {
        let attaching = self.terminals.get(id).is_some_and(|t| {
            matches!(
                t.phase,
                Phase::Creating | Phase::Wanted | Phase::Asked | Phase::Checking | Phase::Attached
            )
        });
        if !self.link.is_live() || attaching {
            return false;
        }
        self.reads.insert(id.clone(), Vec::new());
        let mut capture = Capture::default();
        capture.picture = true;
        self.queue(Packet::about(id.clone(), Message::Read(capture)));
        true
    }

    /// Asks for every terminal of the server; the answer is [`Event::Listed`].
    pub fn list(&mut self) -> bool {
        if !self.link.is_live() {
            return false;
        }
        self.queue(Packet::connection(Message::List));
        true
    }

    /// Stops keeping a terminal in mind (after it ended or went). It is not
    /// ended on the server.
    pub fn forget(&mut self, id: &SessionId) {
        self.terminals.remove(id);
    }

    /// The server is asked to forget the past of an attached terminal: the scrollback its screen
    /// keeps and the record of its output so far. A terminal attached afterwards — after a
    /// restart of the owner — is not given the lines that were cleared (what Clear Scrollback and
    /// Clear to Start ask of the terminal the user looks at), and this client is not affected.
    /// `false` when nothing was asked: the connection is not live or the terminal is not
    /// attached. A server from before the message skips it.
    pub fn forget_past(&mut self, id: &SessionId) -> bool {
        if !self.link.is_live() || !self.is_attached(id) {
            return false;
        }
        self.queue(Packet::about(id.clone(), Message::Forget));
        true
    }

    /// The server is asked to drop the input the terminal's program has not read: what was
    /// typed to the terminal and still waits, in the terminal or in the relay that holds back
    /// what the terminal does not take yet. For the end of a program that was sent more than it
    /// read (a view's words, answers to its questions), so that the shell that reads the
    /// terminal next does not find them and type them at its prompt. Nothing else is touched:
    /// not the output, the screen, the offsets, and the program is not signalled. What is typed
    /// after the server has the message is kept. `false` when nothing was asked: the connection
    /// is not live or the terminal is not one this client attached or is attaching (like
    /// [`Client::input`], whose bytes it comes after: the server serves the two in the order
    /// they were sent). A server from before the message skips it, and nothing is dropped.
    pub fn drop_input(&mut self, id: &SessionId) -> bool {
        let streaming = self
            .terminals
            .get(id)
            .is_some_and(|t| matches!(t.phase, Phase::Asked | Phase::Attached));
        if !streaming || !self.link.is_live() {
            return false;
        }
        self.queue(Packet::about(id.clone(), Message::DropInput));
        true
    }

    // What is known

    /// The offset of the next byte of output owed for a terminal: what an
    /// attach asks for, and what the owner keeps to start from after a restart.
    pub fn offset_of(&self, id: &SessionId) -> Option<u64> {
        self.terminals.get(id).map(|t| t.next)
    }

    /// What the terminal was last said to be doing.
    pub fn state_of(&self, id: &SessionId) -> Option<&TerminalState> {
        self.terminals.get(id).and_then(|t| t.state.as_ref())
    }

    /// How the program of a terminal ended, once the client was told.
    pub fn exit_of(&self, id: &SessionId) -> Option<Exit> {
        match self.terminals.get(id)?.phase {
            Phase::Ended(exit) => Some(exit),
            _ => None,
        }
    }

    /// Whether output of the terminal is flowing.
    pub fn is_attached(&self, id: &SessionId) -> bool {
        self.terminals
            .get(id)
            .is_some_and(|t| t.phase == Phase::Attached)
    }

    // The connection

    fn reset_connection(&mut self) {
        self.handshake.clear();
        self.decoder = IncomingDecoder::new();
        self.mail.reset();
        self.outbox.clear();
        self.linked_at = None;
        self.ping = None;
        for terminal in self.terminals.values_mut() {
            terminal.picture_open = false;
        }
    }

    /// Reads the server's hello out of what has arrived. A hello may arrive in
    /// pieces, after any noise a login shell prints first; what follows the
    /// hello is the first frames and goes to the packet reader.
    fn finish_handshake(&mut self, now: Instant) {
        let mut rest: &[u8] = &self.handshake;
        let reply = WIRE.read_reply(&mut rest);
        let consumed = self.handshake.len() - rest.len();
        match reply {
            Ok(reply) => {
                let selected = reply.selected;
                if selected == 0 {
                    self.lost(now, Cause::Incompatible);
                    return;
                }
                if !(WIRE.min..=WIRE.max).contains(&selected) {
                    self.lost(now, Cause::Protocol);
                    return;
                }
                let after = self.handshake.split_off(consumed);
                self.handshake.clear();
                self.link = Link::Live;
                self.linked_at = Some(now);
                self.events.push_back(Event::Linked);
                self.mail.begin();
                if self.mail.offered() {
                    self.offer_tickets();
                }
                self.attach_again();
                if !after.is_empty() {
                    self.decoder.push(&after);
                    self.read_packets(now);
                }
            }
            Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                if self.handshake.len() > HANDSHAKE_LIMIT {
                    self.lost(now, Cause::Incompatible);
                }
            }
            Err(Error::NoSignature | Error::NoCommonVersion { .. }) => {
                self.lost(now, Cause::Incompatible)
            }
            Err(_) => self.lost(now, Cause::Protocol),
        }
    }

    /// Asks again for what was asked and not got, on a connection that is new.
    fn attach_again(&mut self) {
        let ids: Vec<(SessionId, Phase)> = self
            .terminals
            .iter()
            .map(|(id, t)| (id.clone(), t.phase))
            .collect();
        for (id, phase) in ids {
            match phase {
                Phase::Creating => self.send_new(&id),
                Phase::Wanted => {
                    if let Some(terminal) = self.terminals.get_mut(&id) {
                        terminal.retry = None;
                    }
                    self.send_attach(&id);
                }
                _ => {}
            }
        }
    }

    fn read_packets(&mut self, now: Instant) {
        while self.link.is_live() {
            match self.decoder.next_incoming() {
                Ok(Some(Incoming::Session(packet))) => self.handle(packet, now),
                Ok(Some(Incoming::Stream { from, packet })) => self.heard_stream(from, packet),
                // A channel this build does not know (the codec skips what it cannot read).
                Ok(Some(_)) => {}
                Ok(None) => break,
                Err(e) if e.is_recoverable() => {}
                Err(_) => {
                    self.lost(now, Cause::Protocol);
                    break;
                }
            }
        }
    }

    fn send_ping(&mut self, now: Instant) {
        if self.ping.is_some() {
            return;
        }
        self.ping = Some(now);
        self.queue(Packet::connection(Message::Ping(Vec::new())));
    }

    fn queue(&mut self, packet: Packet) {
        // A packet of the client's own making cannot be too large: the only
        // payloads of any size are chunked, and ids are validated types.
        if packet.encode(&mut self.outbox).is_err() {
            debug_assert!(false, "a packet the client built does not encode");
        }
    }

    fn send_new(&mut self, id: &SessionId) {
        let Some(terminal) = self.terminals.get_mut(id) else {
            return;
        };
        let Some(spec) = terminal.spec.clone() else {
            return;
        };
        // A `new` sent before may have reached the server and its answer been
        // lost with the connection: then this one is told the terminal exists.
        terminal.resent |= terminal.sent;
        terminal.sent = true;
        self.queue(Packet::about(id.clone(), Message::New(spec)));
    }

    fn send_attach(&mut self, id: &SessionId) {
        let Some(terminal) = self.terminals.get_mut(id) else {
            return;
        };
        if terminal.check_wanted && !terminal.take_wanted {
            terminal.phase = Phase::Checking;
            terminal.retry = None;
            self.queue(Packet::about(id.clone(), Message::Status));
            return;
        }
        let mut view = View::new(terminal.size);
        view.picture = terminal.picture_wanted;
        view.picture_if_answered = !terminal.replay_wanted;
        view.keep_after_end = true;
        view.take = terminal.take_wanted;
        view.name = self.name;
        let offset = terminal.next;
        terminal.phase = Phase::Asked;
        terminal.retry = None;
        self.queue(Packet::about(
            id.clone(),
            Message::Attach {
                offset,
                view: Some(view),
            },
        ));
    }

    // What the server says

    fn handle(&mut self, packet: Packet, now: Instant) {
        let Packet { session, message } = packet;
        match message {
            Message::Attached {
                offset,
                truncated,
                answered,
            } => {
                let Some(id) = session else { return };
                let Some(terminal) = self.terminals.get_mut(&id) else {
                    return;
                };
                if !matches!(terminal.phase, Phase::Asked | Phase::Attached) {
                    return;
                }
                terminal.phase = Phase::Attached;
                // An attach that comes before a picture is whole starts everything over.
                terminal.picture_open = false;
                terminal.next = offset;
                terminal.picture_wanted = false;
                terminal.take_wanted = false;
                terminal.unavailable = 0;
                // Served: a connection that breaks from here on is attached again the ordinary
                // way, with a picture in place of bytes that hold a question already answered.
                terminal.replay_wanted = false;
                self.events.push_back(Event::Attached {
                    terminal: id,
                    offset,
                    truncated,
                    answered,
                });
            }
            Message::Picture {
                offset,
                cols,
                rows,
                more,
                data,
            } => {
                let Some(id) = session else { return };
                let Some(terminal) = self.terminals.get_mut(&id) else {
                    self.read_piece(id, offset, cols, rows, more, data);
                    return;
                };
                if terminal.phase != Phase::Attached {
                    // A picture of a terminal that is not attached answers a read: it was asked
                    // before the terminal was let go of, or before it was attached again (the
                    // server answers in the order it was asked).
                    self.read_piece(id, offset, cols, rows, more, data);
                    return;
                }
                let first = !terminal.picture_open;
                terminal.picture_open = more;
                if !more {
                    terminal.next = offset;
                }
                self.events.push_back(Event::Picture {
                    terminal: id,
                    offset,
                    cols,
                    rows,
                    first,
                    last: !more,
                    data,
                });
            }
            Message::Output { offset, data } => {
                let Some(id) = session else { return };
                let Some(terminal) = self.terminals.get_mut(&id) else {
                    return;
                };
                if terminal.phase != Phase::Attached || terminal.picture_open {
                    return;
                }
                let len = u64::try_from(data.len()).unwrap_or(u64::MAX);
                if offset > terminal.next {
                    // Bytes are missing: nothing past a gap can be trusted.
                    self.lost(now, Cause::Protocol);
                    return;
                }
                // Bytes already given are not given twice.
                let skip = usize::try_from(terminal.next - offset).unwrap_or(usize::MAX);
                if skip >= data.len() {
                    return;
                }
                terminal.next = offset.saturating_add(len);
                let fresh = data.get(skip..).unwrap_or_default().to_vec();
                self.events.push_back(Event::Output {
                    terminal: id,
                    data: fresh,
                });
            }
            Message::Ended(exit) => {
                let Some(id) = session else { return };
                let Some(terminal) = self.terminals.get_mut(&id) else {
                    return;
                };
                if matches!(terminal.phase, Phase::Detached | Phase::Gone | Phase::Taken) {
                    return;
                }
                terminal.phase = Phase::Ended(exit);
                terminal.picture_open = false;
                self.events.push_back(Event::Ended { terminal: id, exit });
            }
            Message::TerminalState(state) => {
                let Some(id) = session else { return };
                let Some(terminal) = self.terminals.get_mut(&id) else {
                    return;
                };
                terminal.state = Some(state.clone());
                self.events.push_back(Event::State {
                    terminal: id,
                    state,
                });
            }
            Message::Created(status) => {
                if let Some(terminal) = self.terminals.get_mut(&status.id)
                    && terminal.phase == Phase::Creating
                {
                    terminal.spec = None;
                    terminal.phase = Phase::Wanted;
                    let id = status.id.clone();
                    self.events.push_back(Event::Created(status));
                    self.send_attach(&id);
                } else {
                    self.events.push_back(Event::Created(status));
                }
            }
            Message::StatusReply(status) => {
                if self
                    .terminals
                    .get(&status.id)
                    .is_some_and(|t| t.phase == Phase::Checking)
                {
                    self.checked(&status);
                } else if let Some(terminal) = self.terminals.get_mut(&status.id)
                    && terminal.phase == Phase::Creating
                    && terminal.adopting
                {
                    terminal.spec = None;
                    terminal.adopting = false;
                    terminal.phase = Phase::Wanted;
                    let id = status.id.clone();
                    self.events.push_back(Event::Created(status));
                    self.send_attach(&id);
                } else {
                    self.events.push_back(Event::Status(status));
                }
            }
            Message::ListReply(list) => self.events.push_back(Event::Listed(list)),
            Message::FeaturesReply(features) => self.mail.said_mailbox(features.mailbox),
            Message::Ticket(ticket) => self.ticketed(session, ticket),
            Message::Error { code, text } => self.handle_error(session, code, &text, now),
            _ => {}
        }
    }

    /// A piece of the picture a [`Client::read_picture`] asked for. One nobody asked for is
    /// let go.
    fn read_piece(
        &mut self,
        id: SessionId,
        offset: u64,
        cols: u16,
        rows: u16,
        more: bool,
        data: Vec<u8>,
    ) {
        let Some(pieces) = self.reads.get_mut(&id) else {
            return;
        };
        pieces.extend_from_slice(&data);
        if more {
            return;
        }
        let data = std::mem::take(pieces);
        self.reads.remove(&id);
        self.events.push_back(Event::Screen {
            terminal: id,
            offset,
            cols,
            rows,
            data,
        });
    }

    fn handle_error(
        &mut self,
        session: Option<SessionId>,
        code: ErrorCode,
        text: &str,
        now: Instant,
    ) {
        let text = clean(text);
        let Some(id) = session else {
            self.events.push_back(Event::Failed {
                terminal: None,
                code,
                text,
            });
            return;
        };
        let phase = self.terminals.get(&id).map(|t| t.phase);
        // The answer to a read is an error: the picture will not come.
        if phase != Some(Phase::Attached) {
            self.reads.remove(&id);
        }
        match (code, phase) {
            (ErrorCode::TAKEN, Some(Phase::Asked | Phase::Attached | Phase::Wanted)) => {
                let by = Some(text).filter(|by| !by.is_empty());
                if let Some(terminal) = self.terminals.get_mut(&id) {
                    terminal.phase = Phase::Taken;
                    terminal.taken_by = by.clone();
                    terminal.picture_open = false;
                }
                self.events.push_back(Event::Taken { terminal: id, by });
            }
            (ErrorCode::SESSION_EXISTS, Some(Phase::Creating)) => {
                let resent = self.terminals.get(&id).is_some_and(|t| t.resent);
                if resent {
                    // It happened before the connection broke: ask what it is.
                    if let Some(terminal) = self.terminals.get_mut(&id) {
                        terminal.adopting = true;
                    }
                    self.queue(Packet::about(id, Message::Status));
                } else {
                    self.give_up(id, code, text);
                }
            }
            (ErrorCode::UNAVAILABLE, Some(Phase::Asked)) => {
                // The server could not serve the attach just now (the session's
                // process did not answer): try again, not at once.
                if let Some(terminal) = self.terminals.get_mut(&id) {
                    terminal.phase = Phase::Wanted;
                    terminal.retry = Some(now + retry_delay(terminal.unavailable));
                    terminal.unavailable = terminal.unavailable.saturating_add(1);
                }
            }
            // A terminal the server will not make, will not attach (an offset past
            // its end, a size it cannot have) or does not have.
            (_, Some(Phase::Creating))
            | (ErrorCode::BAD_REQUEST, Some(Phase::Asked))
            | (
                ErrorCode::UNKNOWN_SESSION,
                Some(
                    Phase::Wanted
                    | Phase::Checking
                    | Phase::Asked
                    | Phase::Attached
                    | Phase::Detached,
                ),
            ) => self.give_up(id, code, text),
            _ => self.events.push_back(Event::Failed {
                terminal: Some(id),
                code,
                text,
            }),
        }
    }

    /// The answer to the question that comes before an attach ([`Attach::check`]): another
    /// computer that has the terminal makes it taken (the owner shows it dimmed and ⏎ takes it
    /// back); nobody does, and it is attached as it would have been.
    fn checked(&mut self, status: &Status) {
        let id = status.id.clone();
        let by = self.held_by(status);
        let Some(terminal) = self.terminals.get_mut(&id) else {
            return;
        };
        terminal.check_wanted = false;
        match by {
            Some(by) => {
                terminal.phase = Phase::Taken;
                terminal.taken_by = by.clone();
                self.events.push_back(Event::Taken { terminal: id, by });
            }
            None => {
                terminal.phase = Phase::Wanted;
                self.send_attach(&id);
            }
        }
    }

    /// The computer that has the terminal besides this client, by the name it gave itself
    /// (`Some(None)`: a name that is nothing once the controls are taken out; a client that gave
    /// none is not a computer and is never this). `None` when nobody does — or when the server cannot say (a relay that does not name its clients
    /// lists fewer than it counts): then nothing is known and the client attaches as before.
    /// This client's own name is left out once, because the server may not have noticed that
    /// the connection it had is gone.
    fn held_by(&self, status: &Status) -> Option<Option<String>> {
        let counted = usize::try_from(status.clients).unwrap_or(usize::MAX);
        if status.clients_named.len() < counted {
            return None;
        }
        let mut own = self.name;
        status
            .clients_named
            .iter()
            .filter_map(|name| name.as_ref())
            .find(|name| {
                if own.as_ref() == Some(*name) {
                    own = None;
                    return false;
                }
                true
            })
            .map(|name| Some(clean(name.as_str())).filter(|name| !name.is_empty()))
    }

    /// The server will not have this terminal: say so and stop asking.
    fn give_up(&mut self, id: SessionId, code: ErrorCode, text: String) {
        if let Some(terminal) = self.terminals.get_mut(&id) {
            terminal.phase = Phase::Gone;
            terminal.spec = None;
            terminal.picture_open = false;
        }
        self.events.push_back(Event::Failed {
            terminal: Some(id),
            code,
            text,
        });
    }
}

/// Text the server (or another client) wrote, safe to show a person: control
/// characters removed.
fn clean(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).collect()
}
