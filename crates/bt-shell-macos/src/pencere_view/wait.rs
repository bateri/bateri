//! The deadlines a pane puts on the conversation with a program's view.
//!
//! The protocol's crate has no clock: when its state is one that waits for
//! something that may never come, it is the terminal that decides how long and
//! says when the time is up (`Session::view_wait_over`). Three states wait:
//!
//! * **`Opening`**: an `open` whose files, MCP start-up or answer from the person
//!   have not all come. Its end is the `closed` `timeout` the program is told. The
//!   time is long, because a bundle is up to 64 MiB of base64 that may cross an
//!   `ssh` link through a pty;
//! * **`Closing`**: a view that has been told to go (`ui/resource-teardown`) and
//!   has not answered. Its end is the view being torn down without the answer, so
//!   it is short — a view that does not answer is stuck, and it is over the
//!   pane;
//! * **`Offered`**: a token is out. After a restart (an update's handover, a
//!   connection that came back) it is a token the program was handed unasked, and a
//!   program that does not open its view again must not leave it valid for as long as
//!   the pane lives; the end of the wait is the token being forgotten. A token for a
//!   `hello` is the same state and its end changes nothing, so a wait that ran out is
//!   not set again until the conversation has been in another state ([`Waits::observe`]).
//!
//! And one that is not a state: **the requests of the view** that the program has not
//! answered ([`REQUESTS`]). They are set when the view's message made the pane write to the
//! program ([`Waits::requests_out`]), and their end is every request that is out failing the
//! way MCP defines (`Session::view_requests_overdue`).
//!
//! All are **policy values, not measurements**: nobody measured how long a view
//! needs to answer a teardown, a program to send its files or to open a view again. They are
//! one-shot delays on the main queue and ask for no frame.
//!
//! A delayed block cannot be cancelled, so every wait that is set has a number,
//! and a block that wakes for a number that is not the current one is nothing.

use std::time::Duration;

use bt_core::pencere::State;

/// How long an `open` may wait for its files, the program's MCP start-up or the
/// person's answer.
pub(super) const OPENING: Duration = Duration::from_secs(30);

/// How long a view may take to answer its teardown before it is torn down
/// without the answer.
pub(super) const TEARDOWN: Duration = Duration::from_secs(2);

/// How long a program that was offered a restart has to open its view again before the
/// token it was handed is forgotten. A program that can open again does it at once; the
/// time is for one that is busy, or far away behind an `ssh` link.
pub(super) const OFFERED: Duration = Duration::from_secs(30);

/// How long the program has to answer the requests the view passed on to it, counted from the
/// first of a burst: the time an MCP client's request waits for an answer unless it hears
/// of progress, which a pane cannot (a tool that takes longer than this fails for the view).
/// Requests that went out later in the same burst fail with it, and the next one after it
/// sets a new wait.
pub(super) const REQUESTS: Duration = Duration::from_secs(60);

/// The time a conversation in `state` may wait: `None` for one that waits for
/// nothing the terminal ends.
fn limit(state: State) -> Option<Duration> {
    match state {
        State::Opening => Some(OPENING),
        State::Closing => Some(TEARDOWN),
        State::Offered => Some(OFFERED),
        State::Idle | State::Open | State::Ended => None,
    }
}

/// The waits a pane has set: for a state, and for the requests of the view.
#[derive(Debug, Default)]
pub(crate) struct Waits {
    /// The number of the last wait for a state set or cancelled.
    number: u64,
    /// The state the wait that is set is for, if one is.
    armed: Option<State>,
    /// `Offered` when its wait ran out and the conversation has not been in another state
    /// since: not waited for again.
    spent: Option<State>,
    /// The number of the last wait for the requests set or given up.
    request_number: u64,
    /// Whether the wait for the requests is set.
    requests_armed: bool,
}

impl Waits {
    /// The conversation is in `state` (`None`: it is not one that hosts views):
    /// the wait to set, as its number and delay, if one has to be set now.
    ///
    /// A state that is the one already waited for sets nothing, so a burst of
    /// news does not push the time out; a state that waits for something else sets its
    /// own; one that waits for nothing cancels what was set. **`Offered` whose wait ran out
    /// ([`Waits::expired`]) is not waited for again** while it goes on: the end of that wait
    /// changes nothing when the token is a `hello`'s, and a wait set again at every turn would
    /// run for as long as the pane sits there. Being in another state ends that, and so does a
    /// new restart offer ([`Waits::restart_offered`]). The other two leave their state when
    /// their time is up, and one that did not is waited for again.
    pub(crate) fn observe(&mut self, state: Option<State>) -> Option<(u64, Duration)> {
        if self.spent.is_some() && state != self.spent {
            self.spent = None;
        }
        let wanted = state
            .filter(|state| limit(*state).is_some())
            .filter(|state| Some(*state) != self.spent);
        if wanted == self.armed {
            return None;
        }
        self.number += 1;
        self.armed = wanted;
        wanted.and_then(limit).map(|delay| (self.number, delay))
    }

    /// The wait numbered `number` ran out: whether it is the one that is set, and
    /// so whether the conversation is told the time is up. The wait is spent
    /// either way — the next [`Waits::observe`] sets a new one if the state still waits
    /// (but for an offer that went unanswered, [`Waits::observe`]).
    pub(crate) fn expired(&mut self, number: u64) -> bool {
        if number != self.number || self.armed.is_none() {
            return false;
        }
        if self.armed.take() == Some(State::Offered) {
            self.spent = Some(State::Offered);
        }
        true
    }

    /// The program is offered a restart again ([`bt_core::Session::restart_view`]): an
    /// offer that went unanswered before is not this one.
    pub(crate) fn restart_offered(&mut self) {
        if self.spent == Some(State::Offered) {
            self.spent = None;
        }
    }

    /// The view's message made the pane write to the program: a request is out. The wait to
    /// set, as its number and delay, if none is set already — a burst is one wait, counted
    /// from its first.
    pub(crate) fn requests_out(&mut self) -> Option<(u64, Duration)> {
        if self.requests_armed {
            return None;
        }
        self.request_number += 1;
        self.requests_armed = true;
        Some((self.request_number, REQUESTS))
    }

    /// The wait for the requests numbered `number` ran out: whether it is the one that is
    /// set, and so whether the requests that are out are given up. Spent either way.
    pub(crate) fn requests_expired(&mut self, number: u64) -> bool {
        if number != self.request_number || !self.requests_armed {
            return false;
        }
        self.requests_armed = false;
        true
    }

    /// The view is gone, and with it the requests that were out: a wait that is set wakes for
    /// nothing, and the next view sets its own.
    pub(crate) fn requests_over(&mut self) {
        self.request_number += 1;
        self.requests_armed = false;
    }
}
