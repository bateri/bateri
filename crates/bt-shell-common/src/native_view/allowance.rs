//! How much a module may say to its program, over time.
//!
//! What a module says is written into the program's input — the tty's input queue, which is
//! small and which a program that is not reading does not drain — so a module that talks without
//! end must not be able to fill it, or the memory of the application behind it. A call of the
//! module is limited in what it may send (`bt_view_wasm::Limits`); this is the limit over time:
//! a burst, and a rate, in bytes. A module that goes past it is closed like one that goes past
//! any other limit.

use std::time::Instant;

/// A bucket of bytes that fills at a rate.
#[derive(Debug)]
pub(super) struct Allowance {
    capacity: f64,
    per_second: f64,
    level: f64,
    at: Option<Instant>,
}

impl Allowance {
    /// A full bucket of `capacity` bytes that fills `per_second` bytes every second.
    pub(super) fn new(capacity: usize, per_second: usize) -> Self {
        Self {
            capacity: capacity as f64,
            per_second: per_second as f64,
            level: capacity as f64,
            at: None,
        }
    }

    /// Takes `bytes` at `now`, if the bucket has them: whether the module may say it.
    pub(super) fn take(&mut self, now: Instant, bytes: usize) -> bool {
        if let Some(before) = self.at {
            let seconds = now.saturating_duration_since(before).as_secs_f64();
            self.level = (self.level + seconds * self.per_second).min(self.capacity);
        }
        self.at = Some(now);
        let bytes = bytes as f64;
        if bytes > self.level {
            return false;
        }
        self.level -= bytes;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn a_burst_is_allowed_and_then_the_rate() {
        let start = Instant::now();
        let mut allowance = Allowance::new(1000, 100);
        assert!(allowance.take(start, 600));
        assert!(allowance.take(start, 400));
        assert!(!allowance.take(start, 1), "the bucket is empty");
        // A second later: a hundred bytes.
        let later = start + Duration::from_secs(1);
        assert!(!allowance.take(later, 101));
        assert!(allowance.take(later, 100));
        assert!(!allowance.take(later, 1));
    }

    #[test]
    fn the_bucket_never_holds_more_than_its_capacity() {
        let start = Instant::now();
        let mut allowance = Allowance::new(1000, 100);
        assert!(allowance.take(start, 1000));
        let much_later = start + Duration::from_secs(3600);
        assert!(!allowance.take(much_later, 1001));
        assert!(allowance.take(much_later, 1000));
    }
}
