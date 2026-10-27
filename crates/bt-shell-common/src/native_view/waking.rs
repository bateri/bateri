//! The wakes a module's page asks for, the host's half of them (the kit's specification, §4).
//!
//! The sandbox keeps the earliest time a page asked to be woken at, in its replies and its frames
//! (`pencere_sandbox::Update::wake`, on the clock its frames are given), and tells it at every
//! taking while it stands. The pane wakes the page at the first frame at or after that time, before
//! it asks for the frame's page, and never twice in one frame however often the page asks: a page
//! that asks to be woken again at once costs a frame's work, as one that is moving does. A pane that
//! is not seen draws no frames, so it wakes the page from a clock of its own instead, a second
//! apart at the least, and loses no wake: one it holds back is given at its next, or at the first
//! frame once the pane is seen again. A page that asks nothing costs nothing: no frame, no clock.

/// The fewest seconds between two wakes of a page whose pane is not seen.
pub(super) const UNSEEN_GAP: f64 = 1.0;

/// What the pane keeps of a page's wakes, on the frames' clock.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct Wakes {
    /// The time the page asked to be woken at, as the module last told it.
    asked: Option<f64>,
    /// The time of the last wake the page was given.
    gave: Option<f64>,
}

impl Wakes {
    /// What the module tells now (`Update::wake`); whether the time the page waits to be woken at
    /// changed with it.
    pub(super) fn told(&mut self, asked: Option<f64>) -> bool {
        let before = self.waiting();
        self.asked = asked;
        self.waiting() != before
    }

    /// The time the page waits to be woken at. One before the last wake given is not: the module
    /// took what it held with that wake, and says so at its next taking. The time of that wake
    /// itself is: it is what a page that asks to be woken again at once (a `wake` of 0 in the
    /// wake's reply) is told it asked for, and it is woken at the next frame.
    pub(super) fn waiting(&self) -> Option<f64> {
        self.asked
            .filter(|asked| self.gave.is_none_or(|gave| *asked >= gave))
    }

    /// Whether the frame at `now` wakes the page: its time has come, and no wake was given at this
    /// time or a later one (a frame built twice wakes it once).
    pub(super) fn frame(&mut self, now: f64) -> bool {
        self.give(now, self.waiting())
    }

    /// When a pane that is not seen is to wake the page: at its time, and a second after the last
    /// wake at the soonest. `None`: the page waits for no wake.
    pub(super) fn unseen_at(&self) -> Option<f64> {
        let asked = self.waiting()?;
        Some(self.gave.map_or(asked, |gave| asked.max(gave + UNSEEN_GAP)))
    }

    /// Whether a pane that is not seen wakes the page at `now`.
    pub(super) fn unseen(&mut self, now: f64) -> bool {
        self.give(now, self.unseen_at())
    }

    /// The page is woken at `now` where `at` has come and no wake was given at `now` or after.
    fn give(&mut self, now: f64, at: Option<f64>) -> bool {
        let due = now.is_finite()
            && at.is_some_and(|at| now >= at)
            && self.gave.is_none_or(|gave| now > gave);
        if due {
            self.gave = Some(now);
        }
        due
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_page_that_asks_nothing_waits_for_nothing_and_is_never_woken() {
        let mut wakes = Wakes::default();
        assert!(!wakes.told(None), "nothing asked is no change");
        assert_eq!(wakes.waiting(), None);
        assert_eq!(wakes.unseen_at(), None);
        for now in [0.0, 1.0, 1e6] {
            assert!(!wakes.frame(now));
            assert!(!wakes.unseen(now));
        }
    }

    #[test]
    fn a_page_is_woken_at_the_first_frame_at_its_time_and_once_in_that_frame() {
        let mut wakes = Wakes::default();
        assert!(wakes.told(Some(2.0)));
        assert!(
            !wakes.told(Some(2.0)),
            "told again at every taking: no change"
        );
        assert_eq!(wakes.waiting(), Some(2.0));
        assert!(!wakes.frame(1.0), "not before its time");
        assert!(wakes.frame(2.5));
        assert!(!wakes.frame(2.5), "the same frame, built again");
        // The module has not taken the wake yet and tells the time it held: the page is not
        // waiting for it any more, and no frame is asked for it.
        assert!(!wakes.told(Some(2.0)));
        assert_eq!(wakes.waiting(), None);
        assert!(!wakes.frame(3.0));
        // Then it is gone; a later ask is waited for again.
        assert!(!wakes.told(None));
        assert!(wakes.told(Some(9.0)));
        assert!(!wakes.frame(8.0));
        assert!(wakes.frame(9.0));
    }

    #[test]
    fn a_page_that_asks_again_at_once_is_woken_at_the_next_frame_and_once_a_frame() {
        let mut wakes = Wakes::default();
        wakes.told(Some(0.0));
        assert!(wakes.frame(5.0));
        // The wake's reply asks to be woken in 0 seconds: dated at the time of that wake.
        assert!(wakes.told(Some(5.0)), "a page that asks again waits again");
        assert_eq!(wakes.waiting(), Some(5.0));
        assert!(!wakes.frame(5.0), "not twice at one frame's time");
        assert!(wakes.frame(5.016));
        wakes.told(Some(5.016));
        assert!(wakes.frame(5.033));
    }

    #[test]
    fn a_pane_that_is_not_seen_wakes_its_page_a_second_apart_and_loses_no_wake() {
        let mut wakes = Wakes::default();
        wakes.told(Some(0.5));
        assert_eq!(wakes.unseen_at(), Some(0.5), "the first at its time");
        assert!(!wakes.unseen(0.25));
        assert!(wakes.unseen(3.0));
        // Asked again at once: held back a second from the last.
        wakes.told(Some(3.0));
        assert_eq!(wakes.unseen_at(), Some(4.0));
        assert!(!wakes.unseen(3.5));
        assert!(wakes.unseen(4.0), "held back, then given");
        // A time further off than the second is kept as it is.
        wakes.told(Some(10.0));
        assert_eq!(wakes.unseen_at(), Some(10.0));
        assert!(!wakes.unseen(9.0));
        // The pane is seen again: the frames wake it, with no second to wait.
        assert!(wakes.frame(10.016));
        wakes.told(Some(10.016));
        assert!(wakes.frame(10.033));
    }

    #[test]
    fn a_time_that_is_not_one_wakes_nothing() {
        let mut wakes = Wakes::default();
        wakes.told(Some(1.0));
        assert!(!wakes.frame(f64::NAN));
        assert!(!wakes.unseen(f64::INFINITY));
        assert!(wakes.frame(1.0));
    }
}
