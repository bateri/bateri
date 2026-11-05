//! Streams, the client's side: what the server runs or connects for the client over the
//! connection it already has, and the conversations of mail.
//!
//! A stream is opened by the client and has the server's end on the other side: a program
//! ([`Client::open_command`]: its standard input is what the client says, its standard output and
//! standard error what the server sends, and how it ended comes as [`Event::TalkExited`]), a TCP
//! connection ([`Client::open_connection`]), or the program on the server that asked the app for
//! something ([`Client::accept`], in [`mail`](super::mail)). Whoever owns a stream holds its
//! [`Talk`] and drives it with [`Client::say`], [`Client::finish`], [`Client::took`] and
//! [`Client::hang_up`], and hears of it as events: [`Event::TalkOpened`] when the server took the
//! open, then [`Event::TalkData`], [`Event::TalkStderr`], [`Event::TalkEof`] and, for a program,
//! [`Event::TalkExited`], and at the end [`Event::TalkOver`].
//!
//! **Flow.** What the server sends is credited back as the owner says it has *taken* it
//! ([`Client::took`]), not as it arrives, so the bytes that wait are the stream's window and a
//! slow reader makes the server wait instead of the app's memory grow; the window follows what the
//! link carries ([`kablo_proto::tune`]). The conversations of mail are the exception: their owner
//! bounds what it keeps itself and the credit goes back as the bytes arrive. What the owner says
//! goes as far as the window the server gave allows and the rest waits for its credit
//! ([`Client::unsent`] tells how much waits); a server that sends more than it was given, or
//! anything after its own `eof`, has the stream closed.
//!
//! **The end.** [`Client::finish`] says the owner has nothing more (`eof`) once what it said is
//! sent. A conversation or a connection is over ([`Over::Finished`]) when both ends have said so; a
//! program's stream ends when the server closes it, after `exited`. [`Client::hang_up`] is a
//! `close`, which drops what is on its way, and ends a program the server runs. A connection that
//! goes ends every stream on it ([`Over::Lost`]), and a [`Talk`] of a connection that is gone is no
//! talk of the next: it carries the connection's number.

use std::collections::{BTreeMap, VecDeque};
use std::time::Instant;

use kablo_proto::tune::{self, Rtt, Tuner};
use kablo_proto::v1::stream::{self, Command, MAX_DATA, MAX_STREAMS, MAX_WINDOW, Side};

pub use kablo_proto::v1::stream::FailCode;

use super::{Client, Event, clean};

/// What the streams of a connection hold in windows, all together: each stream's share of it is
/// the part its window may take.
const BUDGET: u32 = 16 * 1024 * 1024;

/// The largest window one stream is given, whatever the link would carry.
const LARGEST: u32 = 4 * 1024 * 1024;

/// How much of the owner's `say` waits at most before the owner is asked to wait: the owner keeps
/// what it has not handed to [`Client::say`] itself, and stops handing when this much is unsent.
pub const UNSENT_LIMIT: usize = 4 * MAX_DATA;

/// One end of a stream on the connection: the stream's number and the connection it is on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Talk {
    /// Which connection to the bridge the stream is on (counting up from 1): the
    /// streams of a connection that is gone are not the streams of the next.
    epoch: u32,
    stream: u32,
}

/// How a stream ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Over {
    /// Both ends said all they had.
    Finished,
    /// The server gave up, or ended the stream (an end sent more than it was given).
    Closed,
    /// The server would not open the stream: a ticket that was not outstanding (the program
    /// gave it up first), a program that does not start, a connection that is refused, or a
    /// limit.
    Failed {
        /// Why.
        code: FailCode,
        /// The server's words, controls removed.
        text: String,
    },
    /// The connection to the bridge ended.
    Lost,
}

/// What a stream is the far end of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// The app's end of a conversation a program on the server started.
    Mail,
    /// A program the server runs.
    Command,
    /// A TCP connection the server makes.
    Connection,
}

/// What the client keeps of streams on one connection (and the one thing it keeps across them,
/// the count of connections).
pub(super) struct Streams {
    /// The connection's number: counts up with every handshake.
    epoch: u32,
    /// The last stream number used on this connection.
    last: u32,
    /// The streams, by number.
    table: BTreeMap<u32, Stream>,
    /// The round trip of the connection, as its credit has timed it.
    rtt: Rtt,
    /// The window the streams of this connection last found they need.
    learned: Option<u32>,
}

impl Default for Streams {
    fn default() -> Streams {
        Streams {
            epoch: 0,
            last: 0,
            table: BTreeMap::new(),
            rtt: Rtt::new(),
            learned: None,
        }
    }
}

impl Streams {
    /// A connection is up: the next number.
    pub(super) fn begin(&mut self) {
        self.epoch = self.epoch.wrapping_add(1);
    }

    /// A new connection: nothing of the last one stands.
    pub(super) fn reset(&mut self) {
        self.last = 0;
        self.table.clear();
        self.rtt = Rtt::new();
        self.learned = None;
    }

    /// The stream `talk` names, if it is of this connection and still on.
    fn of(&mut self, talk: Talk) -> Option<&mut Stream> {
        if talk.epoch != self.epoch {
            return None;
        }
        self.table.get_mut(&talk.stream)
    }

    fn talk(&self, stream: u32) -> Talk {
        Talk {
            epoch: self.epoch,
            stream,
        }
    }

    /// The window one stream may take while `sharing` streams share the budget.
    fn part(&self, sharing: usize) -> u32 {
        let sharing = u32::try_from(sharing.max(1)).unwrap_or(u32::MAX);
        (BUDGET / sharing).clamp(tune::LEAST, LARGEST)
    }

    /// The window a stream about to be added starts with: what the link was last found to need
    /// (or the start), within the part it has with the streams there are.
    fn first_window(&self) -> u32 {
        let part = self.part(self.table.len().saturating_add(1));
        self.learned.unwrap_or(tune::START).min(part)
    }
}

struct Stream {
    kind: Kind,
    /// The server has taken the open.
    opened: bool,
    /// The bytes the client may still send before the server credits it.
    room: u32,
    /// Said by the owner and not yet sent: before the `opened`, or past the room.
    unsent: VecDeque<u8>,
    /// The owner has nothing more to say: `eof` follows what is unsent.
    finishing: bool,
    eof_sent: bool,
    eof_heard: bool,
    /// The server said how the program ended.
    exited: bool,
    /// The bytes the server may still send before the client credits it.
    window: u32,
    /// The bytes the server sent that the owner has not yet taken.
    held: u64,
    /// The bytes the server has sent in all.
    received: u64,
    /// This stream's window, as the bytes that arrive measure it.
    tuner: Tuner,
}

impl Stream {
    fn asked(kind: Kind, window: u32) -> Stream {
        Stream {
            kind,
            opened: false,
            room: 0,
            unsent: VecDeque::new(),
            finishing: false,
            eof_sent: false,
            eof_heard: false,
            exited: false,
            window,
            held: 0,
            received: 0,
            tuner: Tuner::new(window),
        }
    }
}

impl Client {
    /// Asks the server to run `command` and call its standard streams a stream of its own. The
    /// answer is [`Event::TalkOpened`] or, if the program does not start, [`Event::TalkOver`]
    /// with the server's words. `Err` is why nothing was asked: the connection is not up, the
    /// command is not one a server could start, or the server's streams are all in use.
    pub fn open_command(&mut self, command: Command) -> Result<Talk, String> {
        command
            .check()
            .map_err(|error| format!("the command cannot be sent: {error}"))?;
        self.open_stream(Kind::Command, |window| stream::Message::OpenCommand {
            window,
            command,
        })
    }

    /// Asks the server to connect to `host:port` and call the connection a stream of its own. The
    /// answer is [`Event::TalkOpened`] or [`Event::TalkOver`] with the server's words. `Err` is why
    /// nothing was asked.
    pub fn open_connection(&mut self, host: &str, port: u16) -> Result<Talk, String> {
        if port == 0 || host.is_empty() || host.len() > stream::MAX_HOST || host.contains('\0') {
            return Err(format!(
                "{host}:{port} is not an address the server can connect to"
            ));
        }
        let host = host.to_owned();
        self.open_stream(Kind::Connection, |window| stream::Message::Open {
            window,
            port,
            host,
        })
    }

    /// The conversation a ticket stands for, answered: the mail's own open (see
    /// [`Client::accept`](super::mail)).
    pub(super) fn open_conversation(&mut self, ticket: u32) -> Option<Talk> {
        self.open_stream(Kind::Mail, |window| stream::Message::OpenTicket {
            window,
            ticket,
        })
        .ok()
    }

    fn open_stream(
        &mut self,
        kind: Kind,
        open: impl FnOnce(u32) -> stream::Message,
    ) -> Result<Talk, String> {
        if !self.link.is_live() {
            return Err("the connection to the server is not up".to_owned());
        }
        let live = u32::try_from(self.streams.table.len()).unwrap_or(u32::MAX);
        if live >= MAX_STREAMS {
            return Err(format!(
                "the server's {MAX_STREAMS} streams are all in use: end one first"
            ));
        }
        let Some(number) = self.streams.last.checked_add(1) else {
            return Err("this connection has used all its stream numbers".to_owned());
        };
        let window = match kind {
            Kind::Mail => super::mail::WINDOW,
            _ => self.streams.first_window(),
        };
        self.streams.last = number;
        self.streams
            .table
            .insert(number, Stream::asked(kind, window));
        self.queue_stream(number, open(window));
        Ok(self.streams.talk(number))
    }

    /// The owner says these bytes. They go as far as the window the server gave allows and the
    /// rest waits for its credit. `false` when the stream is not on (over, of another connection)
    /// or `finish` was already said.
    pub fn say(&mut self, talk: Talk, bytes: &[u8]) -> bool {
        let Some(stream) = self.streams.of(talk) else {
            return false;
        };
        if stream.finishing {
            return false;
        }
        stream.unsent.extend(bytes);
        self.flush_talk(talk.stream);
        true
    }

    /// How many bytes the owner has said that have not gone yet (past the window or before the
    /// `opened`); `None` when the stream is not on. An owner that has more to say waits until this
    /// is under [`UNSENT_LIMIT`].
    pub fn unsent(&mut self, talk: Talk) -> Option<usize> {
        self.streams.of(talk).map(|stream| stream.unsent.len())
    }

    /// The owner has nothing more to say: `eof` follows what was said. The stream is over
    /// ([`Over::Finished`]) when the server has said all it had too, if it is not a program's.
    /// `false` when it is not on.
    pub fn finish(&mut self, talk: Talk) -> bool {
        let Some(stream) = self.streams.of(talk) else {
            return false;
        };
        stream.finishing = true;
        self.flush_talk(talk.stream);
        true
    }

    /// Ends the stream now, dropping what has not been sent and what is on its way; the owner is
    /// not told of an end it asked for. A program the server runs for it is ended. `false` when it
    /// is not on.
    pub fn hang_up(&mut self, talk: Talk) -> bool {
        if self.streams.of(talk).is_none() {
            return false;
        }
        self.streams.table.remove(&talk.stream);
        self.queue_stream(talk.stream, stream::Message::Close);
        true
    }

    /// The owner has taken `bytes` of what [`Event::TalkData`] and [`Event::TalkStderr`] gave: the
    /// server may send as much again, once enough is due to be worth a message. The credit is
    /// what times the link.
    pub fn took(&mut self, talk: Talk, bytes: usize) {
        let now = Instant::now();
        self.streams.rtt.look(now);
        let part = self.streams.part(self.streams.table.len());
        let rtt = self.streams.rtt.clone();
        let Some(stream) = self.streams.of(talk) else {
            return;
        };
        stream.held = stream
            .held
            .saturating_sub(u64::try_from(bytes).unwrap_or(u64::MAX));
        if stream.eof_heard {
            return;
        }
        // What the server may still send and what is held unread come to the window the stream
        // should have: credit for what was taken makes up the difference, more of it when the
        // window grew and less when it shrank, and none until a quarter of the window is due.
        let window = stream.tuner.window(&rtt, part);
        let held = u32::try_from(stream.held).unwrap_or(u32::MAX);
        let due = window.saturating_sub(held).saturating_sub(stream.window);
        if due >= (window / 4).max(1) {
            let limit = stream.received.saturating_add(u64::from(stream.window));
            stream.tuner.crediting(limit, now, &rtt);
            stream.window = stream.window.saturating_add(due);
            self.queue_stream(talk.stream, stream::Message::Credit(due));
        }
    }

    /// How many streams are on this connection.
    pub fn streams_open(&self) -> usize {
        self.streams.table.len()
    }

    /// Sends what the window allows, then the `eof` if the owner is done, and ends the stream when
    /// both ends have said all they had.
    fn flush_talk(&mut self, stream: u32) {
        let talk = self.streams.talk(stream);
        let mut sending = Vec::new();
        let finished;
        {
            let Some(held) = self.streams.table.get_mut(&stream) else {
                return;
            };
            if !held.opened {
                return;
            }
            while held.room > 0 && !held.unsent.is_empty() {
                let room = usize::try_from(held.room).unwrap_or(usize::MAX);
                let n = held.unsent.len().min(MAX_DATA).min(room);
                let chunk: Vec<u8> = held.unsent.drain(..n).collect();
                held.room -= u32::try_from(n).unwrap_or(held.room);
                sending.push(stream::Message::Data(chunk));
            }
            if held.finishing && held.unsent.is_empty() && !held.eof_sent {
                held.eof_sent = true;
                sending.push(stream::Message::Eof);
            }
            // A program's stream ends with the server's `close`, which follows how it ended.
            finished = held.eof_sent && held.eof_heard && held.kind != Kind::Command;
        }
        for message in sending {
            self.queue_stream(stream, message);
        }
        if finished {
            self.streams.table.remove(&stream);
            self.events.push_back(Event::TalkOver {
                talk,
                over: Over::Finished,
            });
        }
    }

    /// The server sent `message` on a stream.
    pub(super) fn heard_stream(&mut self, from: Side, packet: stream::Packet) {
        if from != Side::Server {
            return;
        }
        let stream::Packet { stream, message } = packet;
        let talk = self.streams.talk(stream);
        match message {
            stream::Message::Opened { window } => {
                if let Some(held) = self.streams.table.get_mut(&stream)
                    && !held.opened
                {
                    held.opened = true;
                    held.room = window.min(MAX_WINDOW);
                    if held.kind != Kind::Mail {
                        self.events.push_back(Event::TalkOpened { talk });
                    }
                    self.flush_talk(stream);
                }
            }
            stream::Message::Failed { code, text } => {
                if self.streams.table.remove(&stream).is_some() {
                    self.events.push_back(Event::TalkOver {
                        talk,
                        over: Over::Failed {
                            code,
                            text: clean(&text),
                        },
                    });
                }
            }
            stream::Message::Data(data) => self.heard_bytes(talk, data, false),
            stream::Message::Stderr(data) => self.heard_bytes(talk, data, true),
            stream::Message::Credit(bytes) => {
                if let Some(held) = self.streams.table.get_mut(&stream) {
                    held.room = held.room.saturating_add(bytes).min(MAX_WINDOW);
                    self.flush_talk(stream);
                }
            }
            stream::Message::Eof => {
                if let Some(held) = self.streams.table.get_mut(&stream)
                    && !held.eof_heard
                {
                    held.eof_heard = true;
                    self.events.push_back(Event::TalkEof { talk });
                    self.flush_talk(stream);
                }
            }
            stream::Message::Exited(exit) => {
                if let Some(held) = self.streams.table.get_mut(&stream)
                    && held.kind == Kind::Command
                    && !held.exited
                {
                    held.exited = true;
                    self.events.push_back(Event::TalkExited { talk, exit });
                }
            }
            stream::Message::Close => {
                if self.streams.table.remove(&stream).is_some() {
                    self.events.push_back(Event::TalkOver {
                        talk,
                        over: Over::Closed,
                    });
                }
            }
            // Messages only the client sends, and any a newer protocol adds.
            _ => {}
        }
    }

    /// `data` or (`stderr`) arrived on a stream.
    fn heard_bytes(&mut self, talk: Talk, data: Vec<u8>, error: bool) {
        let now = Instant::now();
        let part = self.streams.part(self.streams.table.len());
        let Some(held) = self.streams.table.get_mut(&talk.stream) else {
            return;
        };
        // Only a program has an error output, and nothing is heard before the open is taken or
        // after the server's own `eof`.
        if !held.opened || held.eof_heard || (error && held.kind != Kind::Command) {
            return;
        }
        let size = u32::try_from(data.len()).unwrap_or(u32::MAX);
        if size > held.window {
            // The server sent more than the window it was given: the stream is over.
            self.streams.table.remove(&talk.stream);
            self.queue_stream(talk.stream, stream::Message::Close);
            self.events.push_back(Event::TalkOver {
                talk,
                over: Over::Closed,
            });
            return;
        }
        if data.is_empty() {
            return;
        }
        if held.kind == Kind::Mail {
            // The owner of a conversation bounds what it keeps and is credited as it is handed
            // the bytes.
            self.queue_stream(talk.stream, stream::Message::Credit(size));
            self.events.push_back(Event::TalkData { talk, data });
            return;
        }
        held.window -= size;
        held.held = held.held.saturating_add(u64::from(size));
        held.received = held.received.saturating_add(u64::from(size));
        held.tuner
            .received(held.received, now, &mut self.streams.rtt, part);
        self.streams.learned = Some(held.tuner.measured());
        self.events.push_back(if error {
            Event::TalkStderr { talk, data }
        } else {
            Event::TalkData { talk, data }
        });
    }

    /// The connection ended: every stream on it is over.
    pub(super) fn end_talks(&mut self) {
        let epoch = self.streams.epoch;
        for stream in std::mem::take(&mut self.streams.table).into_keys() {
            self.events.push_back(Event::TalkOver {
                talk: Talk { epoch, stream },
                over: Over::Lost,
            });
        }
    }

    pub(super) fn queue_stream(&mut self, stream: u32, message: stream::Message) {
        // A message of the client's own making is within what the codec takes: a `data` is cut at
        // `MAX_DATA`, and what an `open` asks for was checked before it was built.
        if stream::Packet::new(stream, message)
            .encode(Side::Client, &mut self.outbox)
            .is_err()
        {
            debug_assert!(false, "a stream message the client built does not encode");
        }
    }
}
