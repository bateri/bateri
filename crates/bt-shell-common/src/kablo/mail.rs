//! Mail, the client's side: what a program on the server hands the app through
//! `kablo app`, and what the app says back.
//!
//! A client that wants mail says so in its `features` on every connection
//! ([`Client::take_tickets`]); the bridge then gives it a *ticket* for each
//! conversation a program starts on its mailbox ([`Event::Ticket`]), saying
//! which terminal the program runs in. The terminal comes from the kernel's
//! record of the program at the other end, found by the bridge, never from
//! anything the program says: a program in a terminal is that terminal's, and
//! one in none (a cron job) is from none. The client answers the ticket with
//! [`Client::accept`], which opens a stream of its own and is the app's end of
//! the conversation, or with [`Client::decline`], whose text the program is shown.
//!
//! **Flow.** What the program sends comes as [`Event::TalkData`] and is credited
//! back as it is handed over, so the owner's queue is the only place those bytes
//! wait: an owner that keeps them must bound what it keeps and [`hang up`]
//! when a program says more (anything on the same computer may connect to the
//! mailbox). What the app says ([`Client::say`]) goes as far as the window the
//! program gave allows and waits for its credit; the bridge ends a conversation
//! whose end sends more than it was given, or anything after its own `eof`.
//!
//! **The end.** [`Client::finish`] says the app has nothing more (`eof`) once
//! what it said is sent; when both ends have said so the conversation is over
//! ([`Over::Finished`]) and `kablo app` ends with 0. [`Client::hang_up`] is a
//! `close`, which drops what is on its way. A connection that goes ends every
//! conversation on it ([`Over::Lost`]), and a [`Talk`] of a connection that is
//! gone is no talk of the next: it carries the connection's number.
//!
//! [`hang up`]: Client::hang_up

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use kablo_proto::v1::stream::{self, MAX_DATA, MAX_WINDOW, Side};
use kablo_proto::v1::{Message, Offers, Packet};

pub use kablo_proto::v1::stream::FailCode;

use super::{Client, Event, SessionId, clean};

/// How many bytes the program may send before the client has handed them on:
/// the window every conversation is opened with. Far above a request a person
/// or an agent writes, far below anything worth a limit of its own.
pub const WINDOW: u32 = 64 * 1024;

/// The longest text of a `decline` the client sends, in bytes.
const DECLINE_TEXT: usize = 256;

/// The app's end of a conversation: the stream the client opened to answer a
/// ticket, and the connection it is on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Talk {
    /// Which connection to the bridge the stream is on (counting up from 1): the
    /// streams of a connection that is gone are not the streams of the next.
    epoch: u32,
    stream: u32,
}

/// How a conversation ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Over {
    /// Both ends said all they had.
    Finished,
    /// The program gave up, or the bridge ended the conversation (an end sent
    /// more than it was given).
    Closed,
    /// The bridge would not open the app's end: the ticket was not outstanding
    /// (the program gave it up first), or a limit.
    Failed {
        /// Why.
        code: FailCode,
        /// The bridge's words, controls removed.
        text: String,
    },
    /// The connection to the bridge ended.
    Lost,
}

/// What the client keeps of mail on one connection (and the two things it keeps
/// across them: whether it asks for tickets, and the count of connections).
#[derive(Default)]
pub(super) struct Mail {
    /// Tickets are asked for on every connection.
    offered: bool,
    /// What the bridge said of its mailbox in `features_reply` (`None`: nothing
    /// yet, or a bridge from before `features`).
    mailbox: Option<bool>,
    /// The connection's number: counts up with every handshake.
    epoch: u32,
    /// The tickets the bridge gave that are not answered.
    tickets: BTreeSet<u32>,
    /// The last stream number used on this connection.
    last_stream: u32,
    /// The conversations, by the app's stream.
    talks: BTreeMap<u32, Conversation>,
}

impl Mail {
    /// A connection is up: the next number.
    pub(super) fn begin(&mut self) {
        self.epoch = self.epoch.wrapping_add(1);
    }

    /// Whether tickets are asked for on every connection.
    pub(super) fn offered(&self) -> bool {
        self.offered
    }

    /// The bridge's `features_reply` said whether it keeps a mailbox.
    pub(super) fn said_mailbox(&mut self, mailbox: bool) {
        self.mailbox = Some(mailbox);
    }

    /// A new connection: nothing of the last one stands.
    pub(super) fn reset(&mut self) {
        self.mailbox = None;
        self.tickets.clear();
        self.last_stream = 0;
        self.talks.clear();
    }

    /// The conversation `talk` names, if it is of this connection and still on.
    fn of(&mut self, talk: Talk) -> Option<&mut Conversation> {
        if talk.epoch != self.epoch {
            return None;
        }
        self.talks.get_mut(&talk.stream)
    }
}

struct Conversation {
    /// The program has taken the app's end (`opened`).
    opened: bool,
    /// The bytes the app may still send before the program credits it.
    room: u32,
    /// Said by the owner and not yet sent: before the `opened`, or past the room.
    unsent: VecDeque<u8>,
    /// The owner has nothing more to say: `eof` follows what is unsent.
    finishing: bool,
    eof_sent: bool,
    eof_heard: bool,
}

impl Conversation {
    fn asked() -> Conversation {
        Conversation {
            opened: false,
            room: 0,
            unsent: VecDeque::new(),
            finishing: false,
            eof_sent: false,
            eof_heard: false,
        }
    }
}

impl Client {
    /// This client is an app that takes tickets: the bridge is told so on every
    /// connection, the one that is up now included. A client that never calls it
    /// says nothing of mail, and a program that asks the bridge for the app is
    /// told there is none.
    pub fn take_tickets(&mut self) {
        self.mail.offered = true;
        if self.link.is_live() {
            self.offer_tickets();
        }
    }

    /// Whether the bridge keeps a mailbox, as it said when asked: `None` before
    /// it has (or from a bridge that is from before `features`, which has none).
    pub fn mailbox(&self) -> Option<bool> {
        self.mail.mailbox
    }

    pub(super) fn offer_tickets(&mut self) {
        self.queue(Packet::connection(Message::FeaturesWith(
            Offers::with_tickets(),
        )));
    }

    /// The bridge gave a ticket. One that was not asked for is not acted on: the
    /// owner would have to answer something it never offered to take.
    pub(super) fn ticketed(&mut self, from: Option<SessionId>, ticket: u32) {
        if self.mail.offered && self.mail.tickets.insert(ticket) {
            self.events.push_back(Event::Ticket { ticket, from });
        }
    }

    /// Answers a ticket by opening the app's end of the conversation. The
    /// program is waiting for this (it gives up after ten seconds), so it is
    /// answered at once or declined. `None` when the ticket is not outstanding
    /// on this connection, or the connection is not live.
    pub fn accept(&mut self, ticket: u32) -> Option<Talk> {
        if !self.link.is_live() || !self.mail.tickets.contains(&ticket) {
            return None;
        }
        let stream = self.mail.last_stream.checked_add(1)?;
        self.mail.tickets.remove(&ticket);
        self.mail.last_stream = stream;
        self.mail.talks.insert(stream, Conversation::asked());
        self.queue_stream(
            stream,
            stream::Message::OpenTicket {
                window: WINDOW,
                ticket,
            },
        );
        Some(Talk {
            epoch: self.mail.epoch,
            stream,
        })
    }

    /// Refuses a ticket, in the app's own words (cut to a short text): the
    /// program is shown them. `false` when the ticket is not outstanding.
    pub fn decline(&mut self, ticket: u32, text: &str) -> bool {
        if !self.link.is_live() || !self.mail.tickets.remove(&ticket) {
            return false;
        }
        let mut end = text.len().min(DECLINE_TEXT);
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        self.queue(Packet::connection(Message::Decline {
            ticket,
            text: text.get(..end).unwrap_or_default().to_owned(),
        }));
        true
    }

    /// The app says these bytes. They go as far as the window the program gave
    /// allows and the rest waits for its credit. `false` when the conversation
    /// is not on (over, of another connection) or `finish` was already said.
    pub fn say(&mut self, talk: Talk, bytes: &[u8]) -> bool {
        let Some(conversation) = self.mail.of(talk) else {
            return false;
        };
        if conversation.finishing {
            return false;
        }
        conversation.unsent.extend(bytes);
        self.flush_talk(talk.stream);
        true
    }

    /// The app has nothing more to say: `eof` follows what was said. The
    /// conversation is over ([`Over::Finished`]) when the program has said all
    /// it had too. `false` when it is not on.
    pub fn finish(&mut self, talk: Talk) -> bool {
        let Some(conversation) = self.mail.of(talk) else {
            return false;
        };
        conversation.finishing = true;
        self.flush_talk(talk.stream);
        true
    }

    /// Ends the conversation now, dropping what has not been sent and what is on
    /// its way; the owner is not told of an end it asked for. `false` when it is
    /// not on.
    pub fn hang_up(&mut self, talk: Talk) -> bool {
        if self.mail.of(talk).is_none() {
            return false;
        }
        self.mail.talks.remove(&talk.stream);
        self.queue_stream(talk.stream, stream::Message::Close);
        true
    }

    /// Sends what the window allows, then the `eof` if the owner is done, and
    /// ends the conversation when both ends have said all they had.
    fn flush_talk(&mut self, stream: u32) {
        let talk = Talk {
            epoch: self.mail.epoch,
            stream,
        };
        let mut sending = Vec::new();
        let finished;
        {
            let Some(conversation) = self.mail.talks.get_mut(&stream) else {
                return;
            };
            if !conversation.opened {
                return;
            }
            while conversation.room > 0 && !conversation.unsent.is_empty() {
                let room = usize::try_from(conversation.room).unwrap_or(usize::MAX);
                let n = conversation.unsent.len().min(MAX_DATA).min(room);
                let chunk: Vec<u8> = conversation.unsent.drain(..n).collect();
                conversation.room -= u32::try_from(n).unwrap_or(conversation.room);
                sending.push(stream::Message::Data(chunk));
            }
            if conversation.finishing && conversation.unsent.is_empty() && !conversation.eof_sent {
                conversation.eof_sent = true;
                sending.push(stream::Message::Eof);
            }
            finished = conversation.eof_sent && conversation.eof_heard;
        }
        for message in sending {
            self.queue_stream(stream, message);
        }
        if finished {
            self.mail.talks.remove(&stream);
            self.events.push_back(Event::TalkOver {
                talk,
                over: Over::Finished,
            });
        }
    }

    /// A message of channel 1 arrived. Only the bridge's about the app's own
    /// streams mean anything here: this client opens no other.
    pub(super) fn heard_stream(&mut self, from: Side, packet: stream::Packet) {
        if from != Side::Server {
            return;
        }
        let stream::Packet { stream, message } = packet;
        let talk = Talk {
            epoch: self.mail.epoch,
            stream,
        };
        match message {
            stream::Message::Opened { window } => {
                if let Some(conversation) = self.mail.talks.get_mut(&stream)
                    && !conversation.opened
                {
                    conversation.opened = true;
                    conversation.room = window.min(MAX_WINDOW);
                    self.flush_talk(stream);
                }
            }
            stream::Message::Failed { code, text } => {
                if self.mail.talks.remove(&stream).is_some() {
                    self.events.push_back(Event::TalkOver {
                        talk,
                        over: Over::Failed {
                            code,
                            text: clean(&text),
                        },
                    });
                }
            }
            stream::Message::Data(data) => {
                let hears = self
                    .mail
                    .talks
                    .get(&stream)
                    .is_some_and(|c| c.opened && !c.eof_heard);
                if hears && !data.is_empty() {
                    let taken = u32::try_from(data.len()).unwrap_or(u32::MAX);
                    self.queue_stream(stream, stream::Message::Credit(taken));
                    self.events.push_back(Event::TalkData { talk, data });
                }
            }
            stream::Message::Credit(bytes) => {
                if let Some(conversation) = self.mail.talks.get_mut(&stream) {
                    conversation.room = conversation.room.saturating_add(bytes).min(MAX_WINDOW);
                    self.flush_talk(stream);
                }
            }
            stream::Message::Eof => {
                if let Some(conversation) = self.mail.talks.get_mut(&stream)
                    && !conversation.eof_heard
                {
                    conversation.eof_heard = true;
                    self.events.push_back(Event::TalkEof { talk });
                    self.flush_talk(stream);
                }
            }
            stream::Message::Close => {
                if self.mail.talks.remove(&stream).is_some() {
                    self.events.push_back(Event::TalkOver {
                        talk,
                        over: Over::Closed,
                    });
                }
            }
            // A command's standard error and ending: no conversation has either.
            _ => {}
        }
    }

    /// The connection ended: every conversation on it is over.
    pub(super) fn end_talks(&mut self) {
        let epoch = self.mail.epoch;
        for stream in std::mem::take(&mut self.mail.talks).into_keys() {
            self.events.push_back(Event::TalkOver {
                talk: Talk { epoch, stream },
                over: Over::Lost,
            });
        }
    }

    fn queue_stream(&mut self, stream: u32, message: stream::Message) {
        // A message of the client's own making is within what the codec takes:
        // a `data` is cut at `MAX_DATA`.
        if stream::Packet::new(stream, message)
            .encode(Side::Client, &mut self.outbox)
            .is_err()
        {
            debug_assert!(false, "a stream message the client built does not encode");
        }
    }
}
