//! The thread that plays a pattern's bursts at their times, and the cut.
//!
//! One thread sleeps until the next burst is due or a request comes, whichever is first, and
//! otherwise waits for a request without waking: nothing here polls. A new pattern takes the
//! place of the one that plays; the end of one cuts it. What a burst does to a device, and
//! whether the finger it is for is still on the glass, are the [`Driver`]'s.

use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::thread::JoinHandle;
use std::time::Instant;

use objc2::rc::autoreleasepool;

use super::schedule::Schedule;
use super::steps::Step;

/// What plays the bursts.
pub(super) trait Driver: Send + 'static {
    /// Where a pattern asked for now plays: the device a finger is on, or `None` where there is
    /// none (the ask is dropped and not kept for when a finger comes).
    fn begin(&mut self) -> Option<usize>;

    /// Plays `step` on `target`. `false` where it did not, because the finger left: the rest of
    /// the pattern is cut.
    fn fire(&mut self, target: usize, step: &Step) -> bool;
}

/// What is said to whoever asked, once the thread has looked for a finger: whether it found one. A
/// tell that is dropped unsaid (the thread is gone, or the request never reached it) says no.
pub(super) struct Tell(Option<Box<dyn FnOnce(bool) + Send>>);

impl Tell {
    fn say(mut self, found: bool) {
        if let Some(then) = self.0.take() {
            then(found);
        }
    }
}

impl Drop for Tell {
    fn drop(&mut self) {
        if let Some(then) = self.0.take() {
            then(false);
        }
    }
}

enum Command {
    /// A pattern, and what to tell if the one who asked wants to know whether it was played.
    Play(Vec<Step>, Option<Tell>),
    Stop,
}

/// The thread, and the way to ask it.
pub(super) struct Player {
    commands: Option<Sender<Command>>,
    thread: Option<JoinHandle<()>>,
}

impl Player {
    /// Starts the thread with `driver`, or `None` where the system gives no thread.
    pub(super) fn spawn(driver: impl Driver) -> Option<Self> {
        let (commands, requests) = mpsc::channel();
        let thread = std::thread::Builder::new()
            .name("bateri-feel".to_owned())
            .spawn(move || run(&requests, driver))
            .ok()?;
        Some(Self {
            commands: Some(commands),
            thread: Some(thread),
        })
    }

    /// Plays `steps` in place of what plays.
    pub(super) fn play(&self, steps: Vec<Step>) {
        self.send(Command::Play(steps, None));
    }

    /// Plays `steps` in place of what plays and tells `then`, on this thread's own, whether it found
    /// somewhere to play them (a finger on the glass): the caller waits for nothing.
    pub(super) fn play_then(&self, steps: Vec<Step>, then: impl FnOnce(bool) + Send + 'static) {
        self.send(Command::Play(steps, Some(Tell(Some(Box::new(then))))));
    }

    /// Ends what plays.
    pub(super) fn stop(&self) {
        self.send(Command::Stop);
    }

    fn send(&self, command: Command) {
        if let Some(commands) = &self.commands {
            // The thread is gone only after a panic of the driver: the vibration is lost, nothing else.
            let _ = commands.send(command);
        }
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        // The thread ends when its requests do.
        self.commands = None;
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn run(requests: &mpsc::Receiver<Command>, mut driver: impl Driver) {
    let mut playing: Option<(Schedule, usize)> = None;
    loop {
        let waited = match playing
            .as_ref()
            .and_then(|(schedule, _)| schedule.next_due())
        {
            Some(due) => requests.recv_timeout(due.saturating_duration_since(Instant::now())),
            None => requests.recv().map_err(|_| RecvTimeoutError::Disconnected),
        };
        match waited {
            Ok(Command::Play(steps, tell)) => {
                playing = driver
                    .begin()
                    .map(|target| (Schedule::new(steps, Instant::now()), target));
                if let Some(tell) = tell {
                    tell.say(playing.is_some());
                }
            }
            Ok(Command::Stop) => playing = None,
            Err(RecvTimeoutError::Disconnected) => return,
            Err(RecvTimeoutError::Timeout) => {
                let Some((schedule, target)) = playing.as_mut() else {
                    continue;
                };
                let mut cut = false;
                while let Some(step) = schedule.take_due(Instant::now()) {
                    // This thread has no run loop to drain what the calls into AppKit and the
                    // framework autorelease: each burst is its own pool, or they wait for the
                    // thread to end.
                    if !autoreleasepool(|_| driver.fire(*target, &step)) {
                        cut = true;
                        break;
                    }
                }
                if cut || schedule.next_due().is_none() {
                    playing = None;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc::{Receiver, channel};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use super::*;

    /// What a fake device heard, in the order it heard it.
    struct Fake {
        heard: Sender<(usize, u64)>,
        /// The target a pattern is given, or none: no finger.
        target: Arc<Mutex<Option<usize>>>,
        /// The step number (from 0) at which the finger leaves, if it does.
        lifts_at: Option<usize>,
        fired: usize,
    }

    impl Driver for Fake {
        fn begin(&mut self) -> Option<usize> {
            self.fired = 0;
            *self.target.lock().expect("a lock")
        }

        fn fire(&mut self, target: usize, step: &Step) -> bool {
            if self.lifts_at == Some(self.fired) {
                return false;
            }
            self.fired += 1;
            let _ = self.heard.send((target, step.at.as_millis() as u64));
            true
        }
    }

    fn tap(at_ms: u64) -> Step {
        Step {
            at: Duration::from_millis(at_ms),
            hz: 100.0,
            level: 0.5,
            ms: 8.0,
            tap: true,
        }
    }

    /// The player, what its device hears, and where a pattern is told to play.
    type Rig = (Player, Receiver<(usize, u64)>, Arc<Mutex<Option<usize>>>);

    fn player(target: Option<usize>, lifts_at: Option<usize>) -> Rig {
        let (heard, rx) = channel();
        let target = Arc::new(Mutex::new(target));
        let fake = Fake {
            heard,
            target: Arc::clone(&target),
            lifts_at,
            fired: 0,
        };
        (Player::spawn(fake).expect("a thread"), rx, target)
    }

    const WAIT: Duration = Duration::from_secs(5);

    #[test]
    fn the_bursts_play_in_order_on_the_device_the_finger_is_on() {
        let (player, heard, _) = player(Some(1), None);
        player.play(vec![tap(0), tap(15), tap(30)]);
        for expected in [0, 15, 30] {
            assert_eq!(heard.recv_timeout(WAIT).expect("a burst"), (1, expected));
        }
        assert!(
            heard.recv_timeout(Duration::from_millis(80)).is_err(),
            "nothing after the last"
        );
    }

    #[test]
    fn a_pattern_asked_with_no_finger_on_the_glass_is_dropped_and_not_kept_for_later() {
        let (player, heard, target) = player(None, None);
        player.play(vec![tap(0), tap(10)]);
        assert!(heard.recv_timeout(Duration::from_millis(100)).is_err());
        // A finger that comes afterwards finds nothing waiting.
        *target.lock().expect("a lock") = Some(0);
        assert!(heard.recv_timeout(Duration::from_millis(100)).is_err());
    }

    #[test]
    fn the_end_of_a_pattern_cuts_what_is_left_of_it() {
        let (player, heard, _) = player(Some(0), None);
        player.play(vec![tap(0), tap(400), tap(500)]);
        assert_eq!(heard.recv_timeout(WAIT).expect("the first"), (0, 0));
        player.stop();
        assert!(
            heard.recv_timeout(Duration::from_millis(700)).is_err(),
            "cut"
        );
    }

    #[test]
    fn a_new_pattern_takes_the_place_of_the_one_that_plays() {
        let (player, heard, _) = player(Some(0), None);
        player.play(vec![tap(0), tap(400)]);
        assert_eq!(heard.recv_timeout(WAIT).expect("the first"), (0, 0));
        player.play(vec![tap(5)]);
        assert_eq!(heard.recv_timeout(WAIT).expect("the new one"), (0, 5));
        assert!(
            heard.recv_timeout(Duration::from_millis(600)).is_err(),
            "the old one is gone"
        );
    }

    #[test]
    fn a_finger_that_leaves_cuts_the_pattern_there() {
        let (player, heard, _) = player(Some(0), Some(1));
        player.play(vec![tap(0), tap(10), tap(20)]);
        assert_eq!(heard.recv_timeout(WAIT).expect("the first"), (0, 0));
        assert!(
            heard.recv_timeout(Duration::from_millis(150)).is_err(),
            "cut by the lifted finger"
        );
    }

    #[test]
    fn what_the_thread_decided_is_told_to_who_asked_for_it_and_nobody_waits_for_it() {
        let (player, _heard, target) = player(Some(0), None);
        let (said, told) = channel();
        let again = said.clone();
        player.play_then(vec![tap(0)], move |found| {
            let _ = said.send(found);
        });
        assert_eq!(told.recv_timeout(WAIT), Ok(true));
        *target.lock().expect("a lock") = None;
        player.play_then(vec![tap(0)], move |found| {
            let _ = again.send(found);
        });
        assert_eq!(told.recv_timeout(WAIT), Ok(false));
    }

    #[test]
    fn a_tell_that_never_reached_the_thread_says_no() {
        let (player, _heard, _) = player(Some(0), None);
        let (said, told) = channel();
        // The thread is gone: what was asked of it is dropped, and the one who asked is told.
        drop(player);
        let dead = {
            let (commands, requests) = mpsc::channel::<Command>();
            drop(requests);
            commands
        };
        let tell = Tell(Some(Box::new(move |found| {
            let _ = said.send(found);
        })));
        let _ = dead.send(Command::Play(vec![tap(0)], Some(tell)));
        assert_eq!(told.recv_timeout(WAIT), Ok(false));
    }

    #[test]
    fn a_player_that_is_dropped_ends_its_thread() {
        let (player, heard, _) = player(Some(0), None);
        player.play(vec![tap(0)]);
        assert_eq!(heard.recv_timeout(WAIT).expect("a burst"), (0, 0));
        drop(player);
        assert!(matches!(
            heard.recv_timeout(WAIT),
            Err(mpsc::RecvTimeoutError::Disconnected)
        ));
    }
}
