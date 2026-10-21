//! The requests that wait for their anchor to have a pane.
//!
//! A terminal an agent started beside a terminal that no tab of this Mac shows (the Mac does not
//! show the agent's own terminal: it is in the list of what runs in the background, or on another
//! Mac) has no place yet. The terminal runs on, in that list; the request is kept here, in memory,
//! and when a pane starts to show the anchor — opened from the list, restored, moved here from
//! another Mac, any way a pane comes to show a terminal — the pane goes beside it as the agent
//! asked ([`super::serve::settle_waiting`]).
//!
//! In memory, because the terminal is in the list either way and a request kept across a restart
//! would be one more thing that could place a pane nobody expects: when bateri quits the pairing
//! is lost, the terminal is not. The queue is bounded; the oldest request is dropped for a new
//! one, and its terminal is still in the list.

use std::collections::VecDeque;

use super::message::Place;
use super::serve::Server;
use super::{Anchor, Terminal};

/// A request waiting for its anchor.
#[derive(Clone, Debug, PartialEq)]
pub struct Waiting {
    /// The server it is about.
    pub server: Server,
    /// The terminal whose pane it goes beside.
    pub anchor: Anchor,
    /// What was asked.
    pub place: Place,
    /// The terminal the request came from, as kablo said it then.
    pub opener: Option<String>,
}

/// The requests that wait, oldest first.
#[derive(Debug, Default)]
pub struct Queue {
    waiting: VecDeque<Waiting>,
}

impl Queue {
    /// The most requests kept.
    pub const LIMIT: usize = 64;

    /// Keeps a request. One for the same terminal replaces the older.
    pub fn add(&mut self, waiting: Waiting) {
        self.forget(&waiting.server.account, waiting.place.terminal);
        self.waiting.push_back(waiting);
        while self.waiting.len() > Self::LIMIT {
            self.waiting.pop_front();
        }
    }

    /// The requests of `account`'s server that go beside `anchor`, oldest first, taken out.
    pub fn take_beside(&mut self, account: &str, anchor: &str) -> Vec<Waiting> {
        let mut due = Vec::new();
        let mut kept = VecDeque::new();
        for waiting in self.waiting.drain(..) {
            if waiting.server.account == account && waiting.anchor.name() == anchor {
                due.push(waiting);
            } else {
                kept.push_back(waiting);
            }
        }
        self.waiting = kept;
        due
    }

    /// A pane shows `terminal` of `account`'s server itself now: what waited for it to have one
    /// is done.
    pub fn forget(&mut self, account: &str, terminal: Terminal) {
        self.waiting.retain(|waiting| {
            !(waiting.server.account == account && waiting.place.terminal == terminal)
        });
    }

    /// Whether a request for a pane of `terminal` of `account`'s server waits: `ls` says so
    /// (`waiting`), and `close` takes the request away with the terminal.
    pub fn waits(&self, account: &str, terminal: Terminal) -> bool {
        self.waiting
            .iter()
            .any(|waiting| waiting.server.account == account && waiting.place.terminal == terminal)
    }

    pub fn is_empty(&self) -> bool {
        self.waiting.is_empty()
    }

    pub fn len(&self) -> usize {
        self.waiting.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn waiting(account: &str, terminal: u64, anchor: &str) -> Waiting {
        Waiting {
            server: Server {
                account: account.into(),
                host: "rasp".into(),
                argv: vec!["ssh".into(), "rasp".into()],
            },
            anchor: Anchor::parse(anchor).expect("a name"),
            place: Place {
                terminal: Terminal(terminal),
                anchor: Anchor::parse(anchor),
                side: None,
                ratio: None,
                keep_open: false,
                focus: false,
            },
            opener: None,
        }
    }

    #[test]
    fn a_request_is_known_to_wait_by_its_terminal_on_its_server() {
        let mut queue = Queue::default();
        queue.add(waiting("a", 7, "t4"));
        assert!(queue.waits("a", Terminal(7)));
        assert!(!queue.waits("a", Terminal(8)));
        assert!(!queue.waits("b", Terminal(7)));
        queue.forget("a", Terminal(7));
        assert!(!queue.waits("a", Terminal(7)));
    }

    #[test]
    fn the_requests_of_one_server_beside_one_anchor_are_taken_oldest_first() {
        let mut queue = Queue::default();
        queue.add(waiting("a", 7, "t4"));
        queue.add(waiting("a", 8, "t5"));
        queue.add(waiting("b", 9, "t4"));
        queue.add(waiting("a", 10, "t4"));
        let due = queue.take_beside("a", "t4");
        assert_eq!(
            due.iter().map(|w| w.place.terminal.0).collect::<Vec<_>>(),
            [7, 10]
        );
        assert_eq!(queue.len(), 2, "another anchor and another server stay");
        assert!(queue.take_beside("a", "t4").is_empty());
    }

    #[test]
    fn a_request_for_a_terminal_that_has_a_pane_now_is_forgotten_and_a_newer_one_replaces() {
        let mut queue = Queue::default();
        queue.add(waiting("a", 7, "t4"));
        queue.add(waiting("a", 7, "t5"));
        assert_eq!(queue.len(), 1);
        assert_eq!(
            queue.take_beside("a", "t4").len(),
            0,
            "the newer one stands"
        );
        queue.forget("b", Terminal(7));
        assert_eq!(
            queue.len(),
            1,
            "another server's terminal 7 is another terminal"
        );
        queue.forget("a", Terminal(7));
        assert!(queue.is_empty());
    }

    #[test]
    fn the_queue_is_bounded_and_drops_the_oldest() {
        let mut queue = Queue::default();
        for terminal in 0..Queue::LIMIT as u64 + 5 {
            queue.add(waiting("a", terminal, "t1"));
        }
        assert_eq!(queue.len(), Queue::LIMIT);
        let due = queue.take_beside("a", "t1");
        assert_eq!(due[0].place.terminal, Terminal(5));
    }
}
