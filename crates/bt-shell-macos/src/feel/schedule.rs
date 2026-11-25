//! When each burst of a pattern is due, from the moment the pattern started.
//!
//! A burst's time is an offset from the start, so one that is late does not push the ones after
//! it (a sum of waits would). The clock is a parameter: nothing here reads the time.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use super::steps::Step;

/// How late a piece of a held event may be and still play: past it the piece is dropped, as a
/// buzz that comes after the moment it was for is not the shape that was asked. A first guess
/// like the numbers of the steps, to be tried on the device.
pub(super) const MOST_LATE: Duration = Duration::from_millis(30);

#[derive(Debug)]
pub(super) struct Schedule {
    steps: VecDeque<Step>,
    start: Instant,
}

impl Schedule {
    pub(super) fn new(steps: Vec<Step>, start: Instant) -> Self {
        Self {
            steps: steps.into(),
            start,
        }
    }

    /// When the next burst is due, or `None` where all have played.
    pub(super) fn next_due(&self) -> Option<Instant> {
        self.steps.front().map(|step| self.start + step.at)
    }

    /// The next burst if its time has come by `now`. A piece of a held event that is later than
    /// [`MOST_LATE`] is dropped; a tap is never (a click late is still the click).
    pub(super) fn take_due(&mut self, now: Instant) -> Option<Step> {
        loop {
            let due = self.next_due()?;
            if due > now {
                return None;
            }
            let step = self.steps.pop_front()?;
            if step.tap || now.duration_since(due) <= MOST_LATE {
                return Some(step);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(at_ms: u64, tap: bool) -> Step {
        Step {
            at: Duration::from_millis(at_ms),
            hz: 100.0,
            level: 0.5,
            ms: 10.0,
            tap,
        }
    }

    #[test]
    fn a_burst_is_due_at_its_offset_from_the_start_and_not_before() {
        let start = Instant::now();
        let mut schedule = Schedule::new(vec![step(0, true), step(120, true)], start);
        assert_eq!(schedule.next_due(), Some(start));
        assert_eq!(schedule.take_due(start).map(|s| s.at), Some(Duration::ZERO));
        assert_eq!(
            schedule.next_due(),
            Some(start + Duration::from_millis(120))
        );
        assert!(
            schedule
                .take_due(start + Duration::from_millis(119))
                .is_none()
        );
        assert!(
            schedule
                .take_due(start + Duration::from_millis(120))
                .is_some()
        );
        assert_eq!(schedule.next_due(), None);
        assert!(schedule.take_due(start + Duration::from_secs(9)).is_none());
    }

    #[test]
    fn a_late_burst_does_not_move_the_ones_after_it() {
        let start = Instant::now();
        let mut schedule = Schedule::new(vec![step(0, true), step(100, true)], start);
        // The first is taken 50 ms late: the second is still due at 100.
        assert!(
            schedule
                .take_due(start + Duration::from_millis(50))
                .is_some()
        );
        assert_eq!(
            schedule.next_due(),
            Some(start + Duration::from_millis(100))
        );
    }

    #[test]
    fn a_piece_that_is_too_late_is_dropped_and_a_tap_never() {
        let start = Instant::now();
        let mut schedule = Schedule::new(
            vec![
                step(0, false),
                step(10, false),
                step(20, false),
                step(25, true),
            ],
            start,
        );
        // At 55 ms: the pieces at 0 and 10 and 20 are 55, 45 and 35 late, the tap 30 late.
        let now = start + Duration::from_millis(55);
        let first = schedule.take_due(now).expect("what is left");
        assert_eq!(first.at, Duration::from_millis(25));
        assert!(first.tap);
        assert!(schedule.take_due(now).is_none());
    }

    #[test]
    fn a_piece_within_the_allowance_plays() {
        let start = Instant::now();
        let mut schedule = Schedule::new(vec![step(0, false)], start);
        assert!(schedule.take_due(start + MOST_LATE).is_some());
    }
}
