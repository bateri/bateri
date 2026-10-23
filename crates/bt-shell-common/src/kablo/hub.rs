//! The application's side of `ssh-hold`: the holders that are connected to the
//! running bateri, each tied to the one terminal its pane shows.
//!
//! A pane that shows a terminal of a kablo server runs `bateri ssh-hold` as its
//! program (`crate::hold`): a pipe between the pane's own pseudo-terminal and
//! bateri. This is the other end of that pipe, written like [`Client`] and
//! beside it **without a transport**: the owner feeds the bytes a holder's
//! connection delivers ([`Hub::push`]), writes the bytes the hub has for it
//! ([`Hub::take_outgoing`]), and reads what happened ([`Hub::next_event`]). A
//! holder's connection is whatever ordered byte stream the owner has accepted
//! (a unix socket in the application); which holder may speak for which pane
//! is the owner's to decide before it answers a request.
//!
//! **Who decides what.** The hub holds the mechanics, the owner the policy. A
//! holder's request arrives as [`HubEvent::Wants`]; the owner finds or makes the
//! [`Client`] of the server it is for ([`Hub::add_client`]) and answers with
//! [`Hub::start`] (a terminal that does not exist yet), [`Hub::resume`] (one
//! that does) or [`Hub::refuse`]. From then on the hub carries the terminal's
//! output to the holder and what the holder types and the pane's size back to
//! the client, and ends the holder when the terminal ends. The hub owns the
//! clients, one per account ([`Hub::client_mut`] reaches them for the transport),
//! and [`Hub::pump`] moves what a client has to say.
//!
//! **What a holder is told.** The terminal's bytes exactly, in order, each frame saying
//! where the stream stands after it (`crate::hold::wire`); a picture of the
//! screen as a reset followed by the picture's bytes, and the stream's position
//! moves only when the picture is whole, so a connection that breaks half a
//! picture later asks for the position before it and gets a picture again. The
//! reset is not `RIS`: it leaves the pane's history, the screen that was showing
//! included, where it is ([`bt_core::reset_keeping_history`]) — the pane's
//! history is not only the terminal's, it holds what ran in the pane before. A
//! connection to the server that is down is **not** told to the holder: the
//! client retries and attaches again by itself, the holder sees the output go on
//! when it does, and what it types meanwhile is answered with a bell
//! ([`ToHold::Bell`]) and dropped — a line typed minutes ago and sent when the
//! connection came back would run where nobody looks.
//!
//! A holder that is slow to write what it is given grows [`Hub::backlog`]; the
//! owner stops reading the server's stream while it is large, which is the
//! server's own flow control reaching the program.
//!
//! **A raw holder** ([`Hub::open_raw`]) is the pane itself, not a program in it: a pane that
//! shows a terminal of this Mac reads the terminal's bytes as they are (no frames, nothing to
//! greet or ask) and types into it as they are. It is bound to the terminal the moment the
//! owner starts it ([`Hub::start_raw`]), told its size with [`Hub::resize`], and how the program
//! ended is [`Hub::exit_of`]; the words a framed holder is sent as frames (a bell, a line, a
//! refusal) it is sent as the bytes that show them. Everything else — the routing, the
//! position, the backlog, the ending — is the framed holder's.
//!
//! **A raw holder that is given a terminal that was running** ([`Hub::resume_raw`]) is a pane
//! shown again after the bateri that showed it was gone. It is attached from the first byte the
//! server still has and is not given a picture in place of bytes that hold questions the server
//! has answered: the pane's terminal parses the whole of it, muted for the part the questions of
//! which were answered ([`bt_core::Silence`]) — the hub says how far that part reaches as the
//! bytes are written, and when no more of it will come. When the server no longer has the bytes
//! the pane is given a picture, as every pane is, and is muted for the picture and for the
//! bytes of the answered part that follow it.

use std::collections::{BTreeMap, VecDeque};

use std::sync::Arc;

use bt_core::Silence;

use super::{Attach, Client, ErrorCode, Event, Exit, NewSession, SessionId, Size};
use crate::hold::wire::{self, AppReader, CHUNK, Open, Target, ToApp, ToHold, exit_status};

/// One holder's connection, as the hub knows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct HoldId(u64);

/// What the hub tells its owner.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HubEvent {
    /// A holder asked for a terminal. The owner answers with [`Hub::start`],
    /// [`Hub::resume`] or [`Hub::refuse`].
    Wants {
        /// The holder.
        hold: HoldId,
        /// What it asked for.
        open: Open,
    },
    /// The program the owner asked a holder to run ([`Hub::run`]) has ended;
    /// the request is still waiting for the owner's answer.
    Ran {
        /// The holder.
        hold: HoldId,
        /// How it ended: its status, 128 plus the signal that ended it, 255 if
        /// it could not be started.
        status: u8,
    },
    /// The holder's connection is over (it closed, or the hub is done with it);
    /// its terminal is let go of and lives on, on the server.
    Gone {
        /// The holder.
        hold: HoldId,
        /// The account and the terminal it showed, if it showed one: the owner no longer knows
        /// who has the terminal (this Mac was among those attached a moment ago).
        showed: Option<(String, SessionId)>,
    },
    /// The terminal the application numbered for a holder (`t7`) is there already — one a person
    /// made by hand with that name — and the holder was not shown anything: its request is
    /// waiting again, and the owner starts it under the next number ([`Hub::start`] with `spec`).
    Collided {
        /// The holder.
        hold: HoldId,
        /// The account whose server has the name.
        account: String,
        /// The name that was taken.
        terminal: SessionId,
        /// What was asked for, to ask for again.
        spec: Box<NewSession>,
    },
    /// Something a client reported that the hub does not act on itself: the
    /// connection's life, a terminal's state, the answer to a question. Output
    /// and pictures are not passed on; they went to the holder.
    Client {
        /// The account of the client.
        account: String,
        /// What it reported.
        event: Box<Event>,
    },
}

/// Why the owner's answer to a request could not be carried out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Declined {
    /// The holder is not waiting for an answer (gone, or already answered).
    NoRequest,
    /// The answer does not suit the request (a new terminal for a request to
    /// resume one, or the other way).
    WrongKind,
    /// No client for the account: the owner adds one first.
    NoClient,
    /// The client would not take the request (a terminal name already in use
    /// here, an argument the protocol refuses).
    Refused,
    /// The holder speaks a wire version that has no such thing ([`Hub::run`],
    /// [`Hub::say`] need version 2).
    Unsupported,
    /// A program the holder was asked to run is still running: the answer
    /// waits for [`HubEvent::Ran`].
    Running,
}

/// A holder tied to a terminal.
struct Binding {
    account: String,
    terminal: SessionId,
    /// The position of the terminal's output after what has been given to the
    /// holder.
    end: u64,
    /// The terminal has been attached or has written: the holder has been
    /// shown, or is about to be shown, something.
    shown: bool,
    /// The first attach asked for a picture of the screen, so the position
    /// the attach names is the picture's and not that of bytes.
    picture_asked: bool,
    /// Where the output starts, as the server said when it attached a terminal that
    /// was asked for a picture: the position of the bytes that follow **if no picture
    /// does**. A server that cannot make one (an older kablo) goes straight on with
    /// bytes, and a holder that was left with the position of nothing would ask for
    /// the wrong place after the next restart. A picture sets the position itself when
    /// it is whole.
    attached_at: Option<u64>,
    /// How far the server has answered the program's questions, once it has said so, for a
    /// terminal whose start is the past of a pane that was not there to read it
    /// ([`Hold::silence`]).
    answered: Option<u64>,
}

struct Hold {
    reader: AppReader,
    out: Vec<u8>,
    /// The holder is the pane itself ([`Hub::open_raw`]): `out` is the terminal's bytes and
    /// what it sends is typed.
    raw: bool,
    /// How the terminal's program ended, once the hub has said its last word to the holder.
    exit: Option<Exit>,
    /// The wire version agreed, 0 before the greeting.
    version: u8,
    /// The request waiting for the owner.
    pending: Option<Open>,
    /// The holder was asked to run a program and has not said it ended.
    running: bool,
    bound: Option<Binding>,
    /// What a terminal the application numbered was asked with, until the server has made it:
    /// the name may be taken by one a person made, and then it is asked for again
    /// ([`HubEvent::Collided`]).
    renumber: Option<(NewSession, Open)>,
    /// Nothing is read from this holder any more and, once `out` is written,
    /// its connection is to be closed.
    finished: bool,
    /// The past a raw holder's stream starts with ([`Hub::resume_raw`]).
    silence: Option<Arc<Silence>>,
    /// How many bytes were put in `out` since the holder was made, a raw holder's: the position
    /// of the next in its stream.
    fed: u64,
}

impl Hold {
    fn new() -> Hold {
        Hold {
            reader: AppReader::new(),
            out: Vec::new(),
            raw: false,
            exit: None,
            version: 0,
            pending: None,
            running: false,
            bound: None,
            renumber: None,
            finished: false,
            silence: None,
            fed: 0,
        }
    }

    /// Bytes for a raw holder's stream.
    fn put(&mut self, bytes: &[u8]) {
        self.out.extend_from_slice(bytes);
        self.fed += bytes.len() as u64;
    }

    /// No more of the past will be written.
    fn close_past(&self) {
        if let Some(silence) = &self.silence {
            silence.close();
        }
    }

    fn send(&mut self, message: &ToHold) {
        if !self.raw {
            message.encode(&mut self.out);
            return;
        }
        // What a pane reads is bytes: the words of the wire are the bytes that show them.
        match message {
            ToHold::Output { data, .. } => self.put(data),
            ToHold::Bell => self.put(&[0x07]),
            ToHold::Note { text } => {
                self.put(text.as_bytes());
                self.put(b"\r\n");
            }
            ToHold::End { code, note } => {
                if let Some(note) = note {
                    self.line(note);
                }
                self.exit.get_or_insert(Exit::Code(i32::from(*code)));
            }
            ToHold::Refused { text, .. } => {
                if !text.is_empty() {
                    self.line(text);
                }
                self.exit.get_or_insert(Exit::Code(1));
            }
            ToHold::Welcome { .. } | ToHold::Bound { .. } | ToHold::Run { .. } => {}
        }
    }

    /// A line of the hub's own, after what the terminal wrote.
    fn line(&mut self, text: &str) {
        self.put(b"\r\n");
        self.put(text.as_bytes());
        self.put(b"\r\n");
    }

    fn finish(&mut self, message: &ToHold) {
        self.send(message);
        self.finished = true;
        self.close_past();
    }
}

/// The holders of one application and the clients they ride on. See the
/// module documentation.
pub struct Hub {
    holds: BTreeMap<HoldId, Hold>,
    clients: BTreeMap<String, Client>,
    /// Which holder shows which terminal.
    bound: BTreeMap<(String, SessionId), HoldId>,
    events: VecDeque<HubEvent>,
    next: u64,
}

impl Default for Hub {
    fn default() -> Hub {
        Hub::new()
    }
}

impl Hub {
    /// A hub with no holders and no clients.
    pub fn new() -> Hub {
        Hub {
            holds: BTreeMap::new(),
            clients: BTreeMap::new(),
            bound: BTreeMap::new(),
            events: VecDeque::new(),
            next: 1,
        }
    }

    // The clients

    /// The client of the server `account` names; replaces one already there.
    pub fn add_client(&mut self, account: &str, client: Client) {
        self.clients.insert(account.to_owned(), client);
    }

    /// The client of an account, for the transport to drive.
    pub fn client_mut(&mut self, account: &str) -> Option<&mut Client> {
        self.clients.get_mut(account)
    }

    /// The client of an account.
    pub fn client(&self, account: &str) -> Option<&Client> {
        self.clients.get(account)
    }

    /// The accounts that have a client.
    pub fn accounts(&self) -> impl Iterator<Item = &str> {
        self.clients.keys().map(String::as_str)
    }

    /// Takes what an account's client has to say: the terminals' output and
    /// the end of their programs go to the holders showing them, the rest to
    /// the owner as [`HubEvent::Client`]. Call it after bytes were pushed to the
    /// client or time was.
    pub fn pump(&mut self, account: &str) {
        let Some(client) = self.clients.get_mut(account) else {
            return;
        };
        let events: Vec<Event> = std::iter::from_fn(|| client.next_event()).collect();
        for event in events {
            self.route(account, event);
        }
    }

    fn route(&mut self, account: &str, event: Event) {
        let hold_of = |hub: &Hub, terminal: &SessionId| {
            hub.bound
                .get(&(account.to_owned(), terminal.clone()))
                .copied()
        };
        match &event {
            Event::Output { terminal, data } => {
                if let Some(hold) = hold_of(self, terminal) {
                    self.give(hold, data);
                }
                return;
            }
            Event::Picture {
                terminal,
                offset,
                cols,
                first,
                last,
                data,
                ..
            } => {
                if let Some(hold) = hold_of(self, terminal) {
                    self.give_picture(hold, *offset, *cols, *first, *last, data);
                }
                return;
            }
            Event::Attached {
                terminal,
                offset,
                truncated,
                answered,
            } => {
                if let Some(hold) = hold_of(self, terminal)
                    && let Some(entry) = self.holds.get_mut(&hold)
                    && let Some(binding) = entry.bound.as_mut()
                {
                    binding.shown = true;
                    // Output from here on starts at `offset`, unless a picture
                    // comes first: it sets the position when it is whole.
                    let asked = std::mem::take(&mut binding.picture_asked);
                    if !truncated && !asked {
                        binding.end = *offset;
                    }
                    if asked {
                        binding.attached_at = Some(*offset);
                    }
                    // A terminal shown again: the bytes below `answered` are the past. With
                    // none of them coming — the stream starts at or beyond it — the past is
                    // complete before it starts; with a picture coming, the picture is the
                    // past and what follows it is judged by the picture's end.
                    if let Some(silence) = entry.silence.clone() {
                        binding.answered = Some(*answered);
                        if !truncated && *answered <= *offset {
                            silence.close();
                        }
                    }
                }
                return;
            }
            Event::Created(status) => {
                if let Some(hold) = hold_of(self, &status.id)
                    && let Some(hold) = self.holds.get_mut(&hold)
                {
                    // The server has the terminal under the name it was asked with.
                    hold.renumber = None;
                    // A holder of version 3 or later is told the name the terminal has, which
                    // is the one it asked with unless the application numbered it; an
                    // older one is not (it would take the frame for a broken peer) and
                    // keeps its own, which is then the terminal's.
                    let terminal = (hold.version >= wire::NAMED).then(|| status.id.clone());
                    hold.send(&ToHold::Bound {
                        account: account.to_owned(),
                        terminal,
                    });
                }
            }
            Event::Ended { terminal, exit } => {
                if let Some(hold) = hold_of(self, terminal) {
                    self.release(hold, false);
                    if let Some(hold) = self.holds.get_mut(&hold) {
                        hold.exit = Some(*exit);
                        hold.finish(&ToHold::End {
                            code: exit_status(*exit),
                            note: None,
                        });
                    }
                }
            }
            // Another computer took the terminal. The holder is not ended: its pane keeps the last
            // screen, dimmed, and says where the terminal went; the client remembers who took it
            // ([`Client::taken_by`]) and the owner is told below. ⏎ in the pane takes it back
            // ([`Client::take_back`]), and the holder is bound the whole time, so the terminal is
            // not offered to be shown again meanwhile.
            Event::Taken { .. } => {}
            Event::Failed {
                terminal: Some(terminal),
                code,
                text,
            } => {
                if let Some(hold) = hold_of(self, terminal) {
                    // Before anything was shown the holder may still fall back
                    // to a plain `ssh`; afterwards there is only an end.
                    let shown = self
                        .holds
                        .get(&hold)
                        .and_then(|h| h.bound.as_ref())
                        .is_some_and(|b| b.shown);
                    // A number somebody took by hand is not the holder's failure: its request
                    // waits for the owner to ask again under the next number.
                    if *code == ErrorCode::SESSION_EXISTS
                        && !shown
                        && let Some((spec, open)) =
                            self.holds.get_mut(&hold).and_then(|h| h.renumber.take())
                    {
                        self.release(hold, false);
                        if let Some(entry) = self.holds.get_mut(&hold) {
                            entry.pending = Some(open);
                        }
                        self.events.push_back(HubEvent::Collided {
                            hold,
                            account: account.to_owned(),
                            terminal: terminal.clone(),
                            spec: Box::new(spec),
                        });
                        return;
                    }
                    self.release(hold, false);
                    if let Some(hold) = self.holds.get_mut(&hold) {
                        if shown {
                            // A terminal asked for again that the server no longer has
                            // (it ended while bateri was closed) is a line the user reads
                            // under the history of the pane that showed it.
                            let note = if *code == ErrorCode::UNKNOWN_SESSION {
                                "bateri: the terminal that ran here is no longer on the server"
                                    .to_owned()
                            } else {
                                format!("bateri: {text}")
                            };
                            hold.finish(&ToHold::End {
                                code: 1,
                                note: Some(note),
                            });
                        } else {
                            hold.finish(&ToHold::Refused {
                                fall_back: true,
                                text: text.clone(),
                            });
                        }
                    }
                }
            }
            _ => {}
        }
        self.events.push_back(HubEvent::Client {
            account: account.to_owned(),
            event: Box::new(event),
        });
    }

    /// Program output for a holder.
    fn give(&mut self, hold: HoldId, data: &[u8]) {
        let Some(entry) = self.holds.get_mut(&hold) else {
            return;
        };
        let Some(binding) = entry.bound.as_mut() else {
            return;
        };
        binding.shown = true;
        // No picture came first: the bytes start where the server said.
        if let Some(at) = binding.attached_at.take() {
            binding.end = at;
        }
        // A terminal shown again: how much of these bytes is the past is said before they are
        // written, and the past is complete once the stream has passed where it ends.
        if entry.raw
            && let Some(silence) = &entry.silence
            && let Some(answered) = binding.answered
            && !silence.is_closed()
        {
            let past = answered.saturating_sub(binding.end).min(data.len() as u64);
            if past > 0 {
                silence.extend(entry.fed + past);
            }
            if binding.end + data.len() as u64 >= answered {
                silence.close();
            }
        }
        // The position moves with each frame, so a long write is cut here and
        // not by the codec, which would cut it the same way: one rule for
        // where the stream stands after a frame.
        for chunk in data.chunks(CHUNK) {
            binding.end += chunk.len() as u64;
            if entry.raw {
                entry.out.extend_from_slice(chunk);
                entry.fed += chunk.len() as u64;
                continue;
            }
            ToHold::Output {
                end: binding.end,
                data: chunk.to_vec(),
            }
            .encode(&mut entry.out);
        }
    }

    /// One piece of a picture for a holder: a reset before the first, and the
    /// stream's position moves to the picture's only with the last byte of the
    /// last piece. `cols` is the width the picture was made for, which the
    /// reset needs for the tab stops a new terminal of that width has.
    fn give_picture(
        &mut self,
        hold: HoldId,
        offset: u64,
        cols: u16,
        first: bool,
        last: bool,
        data: &[u8],
    ) {
        let Some(entry) = self.holds.get_mut(&hold) else {
            return;
        };
        let Some(binding) = entry.bound.as_mut() else {
            return;
        };
        binding.shown = true;
        binding.attached_at = None;
        let mut bytes = if first {
            bt_core::reset_keeping_history(cols)
        } else {
            Vec::new()
        };
        bytes.reserve(data.len());
        bytes.extend_from_slice(data);
        if entry.raw {
            entry.out.extend_from_slice(&bytes);
            entry.fed += bytes.len() as u64;
            if last {
                binding.end = offset;
            }
            // A terminal shown again: a picture is the screen the past made. It is all past,
            // and what follows it is the past too as far as the questions were answered.
            if let Some(silence) = entry.silence.clone()
                && !silence.is_closed()
            {
                silence.extend(entry.fed);
                if last && binding.answered.is_none_or(|answered| answered <= offset) {
                    silence.close();
                }
            }
            return;
        }
        let pieces: Vec<&[u8]> = bytes.chunks(CHUNK).collect();
        for (at, piece) in pieces.iter().enumerate() {
            let whole = last && at + 1 == pieces.len();
            ToHold::Output {
                end: if whole { offset } else { binding.end },
                data: piece.to_vec(),
            }
            .encode(&mut entry.out);
        }
        if last {
            binding.end = offset;
        }
    }

    // The holders

    /// A holder's connection was accepted.
    pub fn open(&mut self) -> HoldId {
        let id = HoldId(self.next);
        self.next += 1;
        self.holds.insert(id, Hold::new());
        id
    }

    /// A raw holder: the pane itself, which reads the terminal's bytes and types into it as
    /// they are ([`Hub::start_raw`] gives it its terminal). It needs no greeting and asks for
    /// nothing, so it is bound to a terminal by the owner alone.
    pub fn open_raw(&mut self) -> HoldId {
        let id = HoldId(self.next);
        self.next += 1;
        let mut hold = Hold::new();
        hold.raw = true;
        hold.version = wire::VERSION;
        self.holds.insert(id, hold);
        id
    }

    /// A holder's connection ended. Its terminal is let go of: the stream stops
    /// and the terminal lives on, on the server.
    pub fn closed(&mut self, hold: HoldId) {
        if self.holds.contains_key(&hold) {
            let showed = self
                .holds
                .get(&hold)
                .and_then(|entry| entry.bound.as_ref())
                .map(|binding| (binding.account.clone(), binding.terminal.clone()));
            self.release(hold, true);
            self.holds.remove(&hold);
            self.events.push_back(HubEvent::Gone { hold, showed });
        }
    }

    /// Lets go of the holder's terminal: it is no longer routed, and when
    /// `detach` the client stops the stream.
    fn release(&mut self, hold: HoldId, detach: bool) {
        if let Some(entry) = self.holds.get(&hold) {
            entry.close_past();
        }
        let Some(binding) = self.holds.get_mut(&hold).and_then(|h| h.bound.take()) else {
            return;
        };
        let key = (binding.account, binding.terminal);
        if self.bound.get(&key) == Some(&hold) {
            self.bound.remove(&key);
            if detach && let Some(client) = self.clients.get_mut(&key.0) {
                client.detach(&key.1);
            }
        }
    }

    /// Bytes the holder sent.
    pub fn push(&mut self, hold: HoldId, bytes: &[u8]) {
        let Some(entry) = self.holds.get_mut(&hold) else {
            return;
        };
        if entry.finished {
            return;
        }
        // A raw holder says nothing but what is typed.
        if entry.raw {
            self.typed(hold, bytes);
            return;
        }
        entry.reader.push(bytes);
        loop {
            let message = match self.holds.get_mut(&hold) {
                Some(entry) if !entry.finished => entry.reader.read(),
                _ => return,
            };
            match message {
                Ok(Some(message)) => self.on_message(hold, message),
                Ok(None) => return,
                Err(_) => {
                    self.fail(hold, "bateri: the terminal's connection broke the protocol");
                    return;
                }
            }
        }
    }

    /// A holder that speaks out of turn or garbles the wire is told so and
    /// ended: before anything was shown it may fall back to a plain `ssh`.
    fn fail(&mut self, hold: HoldId, text: &str) {
        let shown = self
            .holds
            .get(&hold)
            .and_then(|h| h.bound.as_ref())
            .is_some_and(|b| b.shown);
        self.release(hold, true);
        if let Some(entry) = self.holds.get_mut(&hold) {
            entry.pending = None;
            if shown {
                entry.finish(&ToHold::End {
                    code: 1,
                    note: Some(text.to_owned()),
                });
            } else {
                entry.finish(&ToHold::Refused {
                    fall_back: true,
                    text: text.to_owned(),
                });
            }
        }
    }

    fn on_message(&mut self, hold: HoldId, message: ToApp) {
        let Some(entry) = self.holds.get_mut(&hold) else {
            return;
        };
        match message {
            ToApp::Hello { newest, oldest } => {
                if entry.version != 0 {
                    return self.fail(hold, "bateri: the terminal's connection greeted twice");
                }
                match wire::agree(newest, oldest) {
                    Some(version) => {
                        entry.version = version;
                        entry.send(&ToHold::Welcome { version });
                    }
                    None => entry.finish(&ToHold::Refused {
                        fall_back: true,
                        text: "bateri: this terminal speaks a protocol this bateri does not"
                            .to_owned(),
                    }),
                }
            }
            ToApp::Open(open) => {
                if entry.version == 0 || entry.pending.is_some() || entry.bound.is_some() {
                    return self.fail(hold, "bateri: the terminal's connection asked out of turn");
                }
                entry.pending = Some(open.clone());
                self.events.push_back(HubEvent::Wants { hold, open });
            }
            ToApp::Ran { status } => {
                if !entry.running {
                    return self.fail(
                        hold,
                        "bateri: the terminal's connection answered out of turn",
                    );
                }
                entry.running = false;
                self.events.push_back(HubEvent::Ran { hold, status });
            }
            ToApp::Input(bytes) => self.typed(hold, &bytes),
            ToApp::Resize(size) => self.resize(hold, size),
        }
    }

    /// What the holder typed goes to its terminal; a holder whose terminal does not take it
    /// (the connection is down, the terminal is not there yet) is told with a bell and the
    /// keys are dropped.
    fn typed(&mut self, hold: HoldId, bytes: &[u8]) {
        let sent = match self.holds.get(&hold).and_then(|entry| entry.bound.as_ref()) {
            Some(binding) => {
                let (account, terminal) = (binding.account.clone(), binding.terminal.clone());
                self.clients
                    .get_mut(&account)
                    .is_some_and(|client| client.input(&terminal, bytes))
            }
            None => false,
        };
        if !sent && let Some(entry) = self.holds.get_mut(&hold) {
            entry.send(&ToHold::Bell);
        }
    }

    /// The holder's pane changed size: its terminal's is the same from now on (a request that
    /// is still waiting has it for when it is started).
    pub fn resize(&mut self, hold: HoldId, size: Size) {
        let Some(entry) = self.holds.get_mut(&hold) else {
            return;
        };
        if let Some(binding) = entry.bound.as_ref() {
            let (account, terminal) = (binding.account.clone(), binding.terminal.clone());
            if let Some(client) = self.clients.get_mut(&account) {
                client.resize(&terminal, size);
            }
        } else if let Some(open) = entry.pending.as_mut() {
            open.size = size;
        }
    }

    // The owner's answers

    /// Starts the terminal a holder asked for ([`Target::New`]) on the server
    /// of `account`, running `spec`, and ties the holder to it. The terminal's
    /// name is the holder's, or `name` when the owner numbered it: a holder of
    /// version 3 or later is told the name when the server has made the terminal
    /// ([`ToHold::Bound`]), and an older one keeps its own (`name` is not used for it).
    pub fn start(
        &mut self,
        hold: HoldId,
        account: &str,
        spec: NewSession,
        name: Option<SessionId>,
    ) -> Result<(), Declined> {
        let mut open = self.take_pending(hold, |open| matches!(open.target, Target::New { .. }))?;
        let Some(client) = self.clients.get_mut(account) else {
            return Err(self.put_back(hold, open, Declined::NoClient));
        };
        let asked = open.terminal.clone();
        // A name a holder cannot be told is not used: it would ask for the terminal by the
        // name it made, which the server does not know.
        let version = self.holds.get(&hold).map_or(0, |entry| entry.version);
        let mut renumber = None;
        if let Some(name) = name.filter(|_| version >= wire::NAMED) {
            renumber = Some((spec.clone(), open.clone()));
            open.terminal = name;
        }
        if client
            .new_terminal(open.terminal.clone(), spec, Attach::new(open.size))
            .is_err()
        {
            open.terminal = asked;
            return Err(self.put_back(hold, open, Declined::Refused));
        }
        self.bind(hold, account, open.terminal, 0, false, false);
        if let Some(entry) = self.holds.get_mut(&hold) {
            entry.renumber = renumber;
        }
        Ok(())
    }

    /// Starts a terminal of `account` named `terminal` for a raw holder, which is bound to it at
    /// once and is sent its bytes from the first. The owner names it: a request repeated cannot
    /// make two.
    pub fn start_raw(
        &mut self,
        hold: HoldId,
        account: &str,
        terminal: SessionId,
        spec: NewSession,
        size: Size,
    ) -> Result<(), Declined> {
        if !self
            .holds
            .get(&hold)
            .is_some_and(|entry| entry.raw && entry.bound.is_none())
        {
            return Err(Declined::NoRequest);
        }
        let Some(client) = self.clients.get_mut(account) else {
            return Err(Declined::NoClient);
        };
        if client
            .new_terminal(terminal.clone(), spec, Attach::new(size))
            .is_err()
        {
            return Err(Declined::Refused);
        }
        self.bind(hold, account, terminal, 0, false, false);
        Ok(())
    }

    /// Shows a raw holder a terminal of `account` that is running already: one a bateri before
    /// this one made for the pane, found again. The holder is bound to it and attached from the
    /// first byte the server has, and it is given the [`Silence`] that says how much of what it
    /// is sent is the past (see the module documentation) — the owner hands it to the pane's
    /// session. The stream is nobody's yet when the server does not have the terminal: the holder
    /// is refused, as a terminal that was asked for and is not there is anywhere.
    pub fn resume_raw(
        &mut self,
        hold: HoldId,
        account: &str,
        terminal: SessionId,
        size: Size,
    ) -> Result<Arc<Silence>, Declined> {
        if !self
            .holds
            .get(&hold)
            .is_some_and(|entry| entry.raw && entry.bound.is_none())
        {
            return Err(Declined::NoRequest);
        }
        let Some(client) = self.clients.get_mut(account) else {
            return Err(Declined::NoClient);
        };
        client.attach(terminal.clone(), Attach::new(size).from(0).replaying());
        let silence = Silence::new();
        if let Some(entry) = self.holds.get_mut(&hold) {
            entry.silence = Some(Arc::clone(&silence));
        }
        // Not shown until the server attaches it: a terminal it does not have is a refusal the
        // owner answers (the pane makes a new terminal), not a line under what was shown.
        self.bind(hold, account, terminal, 0, false, false);
        Ok(silence)
    }

    /// How the terminal's program ended, for a holder the hub has said its last word to.
    pub fn exit_of(&self, hold: HoldId) -> Option<Exit> {
        self.holds.get(&hold).and_then(|entry| entry.exit)
    }

    /// Ties the holder to the terminal it asked to see again ([`Target::Resume`])
    /// on the server of `account`, from the position it said (or a picture of
    /// the screen when it said none). With `take` the terminal is taken from every other client
    /// attached to it (a terminal moved here from another computer), each told by whom.
    pub fn resume(&mut self, hold: HoldId, account: &str, take: bool) -> Result<(), Declined> {
        let open = self.take_pending(hold, |open| matches!(open.target, Target::Resume { .. }))?;
        let Target::Resume { offset, .. } = open.target.clone() else {
            return Err(Declined::WrongKind);
        };
        let Some(client) = self.clients.get_mut(account) else {
            return Err(self.put_back(hold, open, Declined::NoClient));
        };
        // A pane that comes back to a terminal it showed (`offset`: an application that was
        // restarted) finds out first whether another computer has it by now, and is dimmed
        // instead of attached beside that computer.
        let attach = match offset {
            Some(offset) => Attach::new(open.size).from(offset).checking(),
            None => Attach::new(open.size).from_now().with_picture(),
        };
        client.attach(
            open.terminal.clone(),
            if take { attach.taking() } else { attach },
        );
        // The holder has shown this terminal before (to the bateri that is gone, or
        // before the Mac restarted): its pane is a screen that is no longer live
        // when the connection is not, not a first connection being made.
        self.bind(
            hold,
            account,
            open.terminal,
            offset.unwrap_or(0),
            offset.is_none(),
            true,
        );
        Ok(())
    }

    /// Asks the holder to run `argv` on the pane's terminal before anything is
    /// shown (the first connection to a server asks for a password or the
    /// server's key there); [`HubEvent::Ran`] says how it ended, and the
    /// request still waits for the answer. Only for a request for a new
    /// terminal, and only to a holder of wire version 2.
    pub fn run(&mut self, hold: HoldId, argv: &[Vec<u8>]) -> Result<(), Declined> {
        let entry = self.holds.get_mut(&hold).ok_or(Declined::NoRequest)?;
        match &entry.pending {
            None => return Err(Declined::NoRequest),
            Some(open) if !matches!(open.target, Target::New { .. }) => {
                return Err(Declined::WrongKind);
            }
            Some(_) => {}
        }
        if entry.running {
            return Err(Declined::Running);
        }
        if entry.version < 2 {
            return Err(Declined::Unsupported);
        }
        entry.running = true;
        entry.send(&ToHold::Run {
            argv: argv.to_vec(),
        });
        Ok(())
    }

    /// Tells the user a line, in front of the terminal (the holder shows it
    /// where it stands). A holder of wire version 1 has nothing to show it
    /// with: `false`.
    pub fn say(&mut self, hold: HoldId, text: &str) -> bool {
        match self.holds.get_mut(&hold) {
            Some(entry) if entry.version >= 2 && !entry.finished => {
                entry.send(&ToHold::Note {
                    text: text.to_owned(),
                });
                true
            }
            _ => false,
        }
    }

    /// The account's connection is over for good (the server speaks no version
    /// this build does, or kablo cannot be kept on it): its client is dropped
    /// and the holders tied to its terminals are ended — a holder that was shown
    /// nothing may fall back to a plain `ssh` (`fall_back`), one that was shown a
    /// terminal is told `text` and ends.
    pub fn drop_account(&mut self, account: &str, text: &str) {
        self.clients.remove(account);
        let tied: Vec<HoldId> = self
            .bound
            .iter()
            .filter(|((owner, _), _)| owner == account)
            .map(|(_, hold)| *hold)
            .collect();
        for hold in tied {
            let shown = self
                .holds
                .get(&hold)
                .and_then(|h| h.bound.as_ref())
                .is_some_and(|b| b.shown);
            self.release(hold, false);
            if let Some(entry) = self.holds.get_mut(&hold) {
                if shown {
                    entry.finish(&ToHold::End {
                        code: 1,
                        note: Some(text.to_owned()),
                    });
                } else {
                    entry.finish(&ToHold::Refused {
                        fall_back: true,
                        text: text.to_owned(),
                    });
                }
            }
        }
    }

    /// The wire version a holder speaks (0 before its greeting).
    pub fn version_of(&self, hold: HoldId) -> u8 {
        self.holds.get(&hold).map_or(0, |entry| entry.version)
    }

    /// Answers the request with no terminal. `fall_back`: the holder runs the
    /// plain `ssh` it replaced (it has not shown anything yet).
    pub fn refuse(&mut self, hold: HoldId, fall_back: bool, text: &str) {
        if let Some(entry) = self.holds.get_mut(&hold) {
            entry.pending = None;
            entry.finish(&ToHold::Refused {
                fall_back,
                text: text.to_owned(),
            });
        }
    }

    /// Ends a holder: it says `note` if there is one and exits with `code`.
    /// Its terminal is let go of and lives on, on the server.
    pub fn end(&mut self, hold: HoldId, code: u8, note: Option<&str>) {
        self.release(hold, true);
        if let Some(entry) = self.holds.get_mut(&hold) {
            entry.pending = None;
            entry.finish(&ToHold::End {
                code,
                note: note.map(str::to_owned),
            });
        }
    }

    fn take_pending(
        &mut self,
        hold: HoldId,
        fits: impl FnOnce(&Open) -> bool,
    ) -> Result<Open, Declined> {
        let entry = self.holds.get_mut(&hold).ok_or(Declined::NoRequest)?;
        if entry.running {
            return Err(Declined::Running);
        }
        match entry.pending.take() {
            None => Err(Declined::NoRequest),
            Some(open) if !fits(&open) => {
                entry.pending = Some(open);
                Err(Declined::WrongKind)
            }
            Some(open) => Ok(open),
        }
    }

    fn put_back(&mut self, hold: HoldId, open: Open, why: Declined) -> Declined {
        if let Some(entry) = self.holds.get_mut(&hold) {
            entry.pending = Some(open);
        }
        why
    }

    fn bind(
        &mut self,
        hold: HoldId,
        account: &str,
        terminal: SessionId,
        end: u64,
        picture_asked: bool,
        shown: bool,
    ) {
        let key = (account.to_owned(), terminal.clone());
        // A terminal is shown by one holder: the newer one wins and the older,
        // a pane that outlived the application it was asked of, is ended.
        if let Some(older) = self.bound.insert(key, hold)
            && older != hold
        {
            if let Some(entry) = self.holds.get_mut(&older) {
                entry.bound = None;
                entry.finish(&ToHold::End {
                    code: 1,
                    note: Some("bateri: this terminal is shown somewhere else".to_owned()),
                });
            }
        }
        if let Some(entry) = self.holds.get_mut(&hold) {
            entry.bound = Some(Binding {
                account: account.to_owned(),
                terminal,
                end,
                shown,
                picture_asked,
                attached_at: None,
                answered: None,
            });
        }
    }

    // What the owner writes and asks

    /// Takes the bytes to write to a holder's connection.
    pub fn take_outgoing(&mut self, hold: HoldId) -> Vec<u8> {
        self.holds
            .get_mut(&hold)
            .map(|entry| std::mem::take(&mut entry.out))
            .unwrap_or_default()
    }

    /// Whether there are bytes to write to a holder.
    pub fn has_outgoing(&self, hold: HoldId) -> bool {
        self.holds
            .get(&hold)
            .is_some_and(|entry| !entry.out.is_empty())
    }

    /// Whether the connection is to be closed once its bytes are written: the
    /// hub has said its last word to this holder.
    pub fn is_finished(&self, hold: HoldId) -> bool {
        self.holds
            .get(&hold)
            .is_some_and(|entry| entry.finished && entry.out.is_empty())
    }

    /// How many bytes are waiting to be written to a holder. A large number
    /// means the pane is not taking its output as fast as the server sends it.
    pub fn backlog(&self, hold: HoldId) -> usize {
        self.holds.get(&hold).map_or(0, |entry| entry.out.len())
    }

    /// Whether the holder's terminal has been **shown** — attached, so its
    /// output or its picture has started to reach the holder — at least once; a
    /// holder that asked for a terminal it had shown before ([`Hub::resume`]) counts
    /// from the start. The line between a first connection that is still being made
    /// and one that was lost.
    pub fn is_shown(&self, hold: HoldId) -> bool {
        self.holds
            .get(&hold)
            .and_then(|entry| entry.bound.as_ref())
            .is_some_and(|binding| binding.shown)
    }

    /// The terminal a holder shows: the account and its name.
    pub fn terminal_of(&self, hold: HoldId) -> Option<(&str, &SessionId)> {
        self.holds
            .get(&hold)
            .and_then(|entry| entry.bound.as_ref())
            .map(|binding| (binding.account.as_str(), &binding.terminal))
    }

    /// The holder that shows a terminal.
    pub fn hold_of(&self, account: &str, terminal: &SessionId) -> Option<HoldId> {
        self.bound
            .get(&(account.to_owned(), terminal.clone()))
            .copied()
    }

    /// The next thing that happened, if anything did.
    pub fn next_event(&mut self) -> Option<HubEvent> {
        self.events.pop_front()
    }
}

#[cfg(test)]
mod tests;
