//! The deadlines a pane puts on the conversation with a program's view.
//!
//! The protocol's crate has no clock: when its state is one that waits for
//! something that may never come, it is the terminal that decides how long and
//! says when the time is up (`Session::view_wait_over`). Two states wait:
//!
//! * **`Opening`**: an `open` whose files or answer from the person have not all
//!   come. Its end is the `closed` `timeout` the program is told. The time is long,
//!   because a bundle is up to 64 MiB of base64 that may cross an `ssh` link through
//!   a pty;
//! * **`Offered`**: a token is out. After a restart (a new bateri showing the terminal, a
//!   connection that came back) it is a token the program was handed unasked, and a
//!   program that does not open its view again must not leave it valid for as long as
//!   the pane lives; the end of the wait is the token being forgotten. A token for a
//!   `hello` is the same state and its end changes nothing, so a wait that ran out is
//!   not set again until the conversation has been in another state ([`Waits::observe`]).
//!
//! A view is closed at once, whatever its kind, and a call the program has not answered
//! is the page's to give up: neither is waited for here.
//!
//! Both are **policy values, not measurements**: nobody measured how long a program
//! needs to send its files or to open a view again. They are one-shot delays on the main
//! queue and ask for no frame.
//!
//! A delayed block cannot be cancelled, so every wait that is set has a number,
//! and a block that wakes for a number that is not the current one is nothing.

use std::time::Duration;

use bt_core::pencere::State;

/// How long an `open` may wait for its files or the person's answer.
pub(super) const OPENING: Duration = Duration::from_secs(30);

/// How long a program that was offered a restart has to open its view again before the
/// token it was handed is forgotten. A program that can open again does it at once; the
/// time is for one that is busy, or far away behind an `ssh` link.
pub(super) const OFFERED: Duration = Duration::from_secs(30);

/// The time a conversation in `state` may wait: `None` for one that waits for
/// nothing the terminal ends.
fn limit(state: State) -> Option<Duration> {
    match state {
        State::Opening => Some(OPENING),
        State::Offered => Some(OFFERED),
        State::Idle | State::Open | State::Ended => None,
    }
}

/// The waits a pane has set: for a state.
#[derive(Debug, Default)]
pub(crate) struct Waits {
    /// The number of the last wait for a state set or cancelled.
    number: u64,
    /// The state the wait that is set is for, if one is.
    armed: Option<State>,
    /// `Offered` when its wait ran out and the conversation has not been in another state
    /// since: not waited for again.
    spent: Option<State>,
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
    /// new restart offer ([`Waits::restart_offered`]). `Opening` leaves its state when its time
    /// is up, and one that did not is waited for again.
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
    /// offer that went unanswered before is not this one, and neither is the time of an
    /// offer still waited for — the new token has a time of its own, or an offer made late in
    /// the first one's (the first went nowhere: a terminal a server keeps, before its bridge
    /// was up) would be forgotten almost at once.
    pub(crate) fn restart_offered(&mut self) {
        if self.spent == Some(State::Offered) {
            self.spent = None;
        }
        if self.armed == Some(State::Offered) {
            self.armed = None;
        }
    }
}
