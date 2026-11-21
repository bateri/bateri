//! The line in front of a kablo connection's streams (plan 034, Karar 2): kablo keeps 128 streams
//! open on a connection and lets 16 be connecting at once, and refuses the rest. A cold page of a
//! dev server asks for twenty at the same moment, so what asks waits here for its turn instead —
//! at most [`MAX_CONNECTING`] connecting and [`MAX_OPEN`] open, a little under what kablo takes,
//! counted **together** for everything that makes a stream through the connection: the browsers'
//! requests and the port forwards ([`crate::kablo::served::Served::connection`]).
//!
//! One line per server ([`line_of`]): bateri keeps one connection to a server.

use std::collections::HashMap;
use std::sync::{Arc, Condvar, Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant};

/// How many streams may be being connected at once through one connection (kablo's own limit is
/// 16: the rest is left for what does not come through here).
pub const MAX_CONNECTING: usize = 12;

/// How many streams may be open at once through one connection (kablo's own limit is 128).
pub const MAX_OPEN: usize = 120;

/// The line of one connection.
pub struct Queue {
    max_connecting: usize,
    max_open: usize,
    counts: Mutex<Counts>,
    changed: Condvar,
}

#[derive(Default)]
struct Counts {
    connecting: usize,
    open: usize,
}

impl Queue {
    pub fn new(max_connecting: usize, max_open: usize) -> Arc<Self> {
        Arc::new(Self {
            max_connecting,
            max_open,
            counts: Mutex::new(Counts::default()),
            changed: Condvar::new(),
        })
    }

    /// Waits for a place (up to `wait`) and takes it: the stream is being connected until
    /// [`Entry::connected`], and open until the entry is dropped. `None` if no place came in time.
    pub fn enter(self: &Arc<Self>, wait: Duration) -> Option<Entry> {
        let deadline = Instant::now() + wait;
        let mut counts = self.counts.lock().unwrap_or_else(PoisonError::into_inner);
        while counts.connecting >= self.max_connecting || counts.open >= self.max_open {
            let left = deadline.checked_duration_since(Instant::now())?;
            if left.is_zero() {
                return None;
            }
            counts = self
                .changed
                .wait_timeout(counts, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
        counts.connecting += 1;
        counts.open += 1;
        Some(Entry {
            queue: Arc::clone(self),
            connecting: true,
        })
    }

    /// How many are connecting and how many are open.
    pub fn counts(&self) -> (usize, usize) {
        let counts = self.counts.lock().unwrap_or_else(PoisonError::into_inner);
        (counts.connecting, counts.open)
    }

    fn release(&self, connecting: bool) {
        let mut counts = self.counts.lock().unwrap_or_else(PoisonError::into_inner);
        if connecting {
            counts.connecting -= 1;
        }
        counts.open -= 1;
        drop(counts);
        self.changed.notify_all();
    }

    fn connected(&self) {
        let mut counts = self.counts.lock().unwrap_or_else(PoisonError::into_inner);
        counts.connecting -= 1;
        drop(counts);
        self.changed.notify_all();
    }
}

/// A place in the line: held while the stream is being connected and open. Dropping it gives the
/// place back.
pub struct Entry {
    queue: Arc<Queue>,
    connecting: bool,
}

impl Entry {
    /// The stream is connected: the next one may begin to connect. Open it stays.
    pub fn connected(&mut self) {
        if self.connecting {
            self.connecting = false;
            self.queue.connected();
        }
    }
}

impl Drop for Entry {
    fn drop(&mut self) {
        self.queue.release(self.connecting);
    }
}

impl std::fmt::Debug for Entry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Entry")
    }
}

/// The line of the connection to the server whose key is `key`: one for the whole process.
pub fn line_of(key: &str) -> Arc<Queue> {
    static LINES: OnceLock<Mutex<HashMap<String, Arc<Queue>>>> = OnceLock::new();
    let mut lines = LINES
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    Arc::clone(
        lines
            .entry(key.to_owned())
            .or_insert_with(|| Queue::new(MAX_CONNECTING, MAX_OPEN)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn places_are_taken_up_to_the_limits_and_given_back() {
        let queue = Queue::new(2, 3);
        let mut a = queue.enter(Duration::ZERO).unwrap();
        let mut b = queue.enter(Duration::ZERO).unwrap();
        assert_eq!(queue.counts(), (2, 2));
        assert!(
            queue.enter(Duration::from_millis(20)).is_none(),
            "a third waits for a connecting place"
        );
        a.connected();
        a.connected();
        assert_eq!(queue.counts(), (1, 2));
        let c = queue.enter(Duration::ZERO).unwrap();
        b.connected();
        assert!(
            queue.enter(Duration::from_millis(20)).is_none(),
            "no open place is left"
        );
        drop((a, b, c));
        assert_eq!(queue.counts(), (0, 0));
    }

    #[test]
    fn a_waiter_goes_when_a_place_is_given_back() {
        let queue = Queue::new(1, 1);
        let first = queue.enter(Duration::ZERO).unwrap();
        let waiting = {
            let queue = Arc::clone(&queue);
            std::thread::spawn(move || queue.enter(Duration::from_secs(5)).is_some())
        };
        std::thread::sleep(Duration::from_millis(50));
        drop(first);
        assert!(waiting.join().unwrap());
    }

    /// What kablo does: it refuses what is beyond its limits. Two hundred wanting through the
    /// line are all let through in turn, and kablo is never asked for more than it takes.
    #[test]
    fn two_hundred_at_once_pass_in_turn_and_kablo_refuses_none() {
        let queue = Queue::new(MAX_CONNECTING, MAX_OPEN);
        let connecting = Arc::new(AtomicUsize::new(0));
        let open = Arc::new(AtomicUsize::new(0));
        let refused = Arc::new(AtomicUsize::new(0));
        let peaks = Arc::new((AtomicUsize::new(0), AtomicUsize::new(0)));
        let threads: Vec<_> = (0..200)
            .map(|n| {
                let queue = Arc::clone(&queue);
                let (connecting, open, refused) =
                    (connecting.clone(), open.clone(), refused.clone());
                let peaks = peaks.clone();
                std::thread::spawn(move || {
                    let mut entry = queue.enter(Duration::from_secs(30)).expect("a turn");
                    // Kablo's side: 16 connecting, 128 open, and the rest refused.
                    let now = connecting.fetch_add(1, Ordering::SeqCst) + 1;
                    let held = open.fetch_add(1, Ordering::SeqCst) + 1;
                    peaks.0.fetch_max(now, Ordering::SeqCst);
                    peaks.1.fetch_max(held, Ordering::SeqCst);
                    if now > 16 || held > 128 {
                        refused.fetch_add(1, Ordering::SeqCst);
                    }
                    std::thread::sleep(Duration::from_millis(2));
                    connecting.fetch_sub(1, Ordering::SeqCst);
                    entry.connected();
                    // Some stay open for a while, as a page's connections do.
                    std::thread::sleep(Duration::from_millis(if n % 3 == 0 { 30 } else { 1 }));
                    open.fetch_sub(1, Ordering::SeqCst);
                    drop(entry);
                })
            })
            .collect();
        for thread in threads {
            thread.join().unwrap();
        }
        assert_eq!(refused.load(Ordering::SeqCst), 0);
        assert!(peaks.0.load(Ordering::SeqCst) <= MAX_CONNECTING);
        assert!(peaks.1.load(Ordering::SeqCst) <= MAX_OPEN);
        assert_eq!(queue.counts(), (0, 0));
    }

    #[test]
    fn a_server_has_one_line() {
        assert!(Arc::ptr_eq(&line_of("one"), &line_of("one")));
        assert!(!Arc::ptr_eq(&line_of("one"), &line_of("two")));
    }
}
