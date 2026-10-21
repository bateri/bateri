//! The deadlines a pane puts on the conversation with a program's view.
//!
//! The protocol's crate has no clock: when its state is one that waits for
//! something that may never come, it is the terminal that decides how long and
//! says when the time is up (`Session::view_wait_over`). Two states wait:
//!
//! * **`Opening`**: an `open` whose files, MCP start-up or answer from the person
//!   have not all come. Its end is the `closed` `timeout` the program is told. The
//!   time is long, because a bundle is up to 64 MiB of base64 that may cross an
//!   `ssh` link through a pty;
//! * **`Closing`**: a view that has been told to go (`ui/resource-teardown`) and
//!   has not answered. Its end is the view being torn down without the answer, so
//!   it is short — a view that does not answer is stuck, and it is over the
//!   pane.
//!
//! Both are **policy values, not measurements**: nobody measured how long a view
//! needs to answer a teardown, or a program to send its files. They are one-shot
//! delays on the main queue and ask for no frame.
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

/// The time a conversation in `state` may wait: `None` for one that waits for
/// nothing the terminal ends.
fn limit(state: State) -> Option<Duration> {
    match state {
        State::Opening => Some(OPENING),
        State::Closing => Some(TEARDOWN),
        State::Idle | State::Offered | State::Open | State::Ended => None,
    }
}

/// The wait a pane has set: which state it is for and which number it carries.
#[derive(Debug, Default)]
pub(crate) struct Waits {
    /// The number of the last wait set or cancelled.
    number: u64,
    /// The state the wait that is set is for, if one is.
    armed: Option<State>,
}

impl Waits {
    /// The conversation is in `state` (`None`: it is not one that hosts views):
    /// the wait to set, as its number and delay, if one has to be set now.
    ///
    /// A state that is the one already waited for sets nothing, so a burst of news
    /// does not push the time out; a state that waits for something else sets its
    /// own; one that waits for nothing cancels what was set.
    pub(crate) fn observe(&mut self, state: Option<State>) -> Option<(u64, Duration)> {
        let wanted = state.filter(|state| limit(*state).is_some());
        if wanted == self.armed {
            return None;
        }
        self.number += 1;
        self.armed = wanted;
        wanted.and_then(limit).map(|delay| (self.number, delay))
    }

    /// The wait numbered `number` ran out: whether it is the one that is set, and
    /// so whether the conversation is told the time is up. The wait is spent
    /// either way — the next [`Waits::observe`] sets a new one if the state still
    /// waits.
    pub(crate) fn expired(&mut self, number: u64) -> bool {
        if number != self.number || self.armed.is_none() {
            return false;
        }
        self.armed = None;
        true
    }
}
