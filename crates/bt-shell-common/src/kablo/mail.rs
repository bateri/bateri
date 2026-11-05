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
//! The conversation is a stream like any other ([`streams`](super::streams)): what the program
//! sends comes as [`Event::TalkData`] and is credited back as it is handed over, so the owner's
//! queue is the only place those bytes wait: an owner that keeps them must bound what it keeps
//! and hang up ([`Client::hang_up`]) when a program says more (anything on the same computer may
//! connect to the mailbox).

use std::collections::BTreeSet;

use kablo_proto::v1::{Message, Offers, Packet};

use super::streams::Talk;
use super::{Client, Event, SessionId};

/// How many bytes the program may send before the client has handed them on:
/// the window every conversation is opened with. Far above a request a person
/// or an agent writes, far below anything worth a limit of its own.
pub const WINDOW: u32 = 64 * 1024;

/// The longest text of a `decline` the client sends, in bytes.
const DECLINE_TEXT: usize = 256;

/// What the client keeps of mail (across connections: whether it asks for tickets; on one: what
/// the bridge said of its mailbox and the tickets it gave).
#[derive(Default)]
pub(super) struct Mail {
    /// Tickets are asked for on every connection.
    offered: bool,
    /// What the bridge said of its mailbox in `features_reply` (`None`: nothing
    /// yet, or a bridge from before `features`).
    mailbox: Option<bool>,
    /// The tickets the bridge gave that are not answered.
    tickets: BTreeSet<u32>,
}

impl Mail {
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
        let talk = self.open_conversation(ticket)?;
        self.mail.tickets.remove(&ticket);
        Some(talk)
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
}
