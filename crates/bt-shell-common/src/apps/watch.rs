//! The `kablo bateri package watch` that runs on each machine the person is connected to.
//!
//! It is started once, when the connection to a machine comes up, with the same path a relayed
//! command takes ([`crate::agent::relay::command`]): the bridge on the machine starts it, so it
//! ends with the bridge, which ends with the connection. It asks the app about the machine's
//! programs ([`super::package`]) and sleeps a day; nothing here wakes it but a person's Check Now,
//! which ends it and has it started again, because a watch that is started asks at once.

use std::collections::{HashMap, HashSet};
use std::io::{self, Read};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;

use crate::kablo::remote::{End, RemoteCommand};

/// The watches that run, by account.
#[derive(Default)]
pub struct Watches {
    /// An account that has a watch, or is starting one: the end of it once it is started.
    running: Mutex<HashMap<String, Option<Arc<End>>>>,
    /// The accounts whose watch was ended to be started again.
    again: Mutex<HashSet<String>>,
}

fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Watches {
    pub fn new() -> Self {
        Self::default()
    }

    /// Keeps a watch on `account` until its connection ends: `start` runs it. Waits for as long as
    /// the watch lives, so it is called on a thread that may; it does nothing when the account has
    /// a watch already. A watch that Check Now ended is started again.
    pub fn keep(&self, account: &str, start: &dyn Fn() -> Result<RemoteCommand, String>) {
        {
            let mut running = locked(&self.running);
            if running.contains_key(account) {
                // One is there already, and what it holds to be ended with stays.
                return;
            }
            running.insert(account.to_owned(), None);
        }
        loop {
            let started = match start() {
                Ok(started) => started,
                Err(why) => {
                    eprintln!("bateri: could not start package watch on {account}: {why}");
                    break;
                }
            };
            let RemoteCommand {
                stdin,
                stdout,
                stderr,
                end,
            } = started;
            // Nothing crosses on standard input, and what it says is of no one's: it is read so
            // that the stream is not held back.
            drop(stdin);
            drain(stdout);
            drain(stderr);
            locked(&self.running).insert(account.to_owned(), Some(Arc::clone(&end)));
            let _ = end.wait();
            locked(&self.running).insert(account.to_owned(), None);
            if !locked(&self.again).remove(account) {
                break;
            }
        }
        locked(&self.running).remove(account);
    }

    /// Check Now: every watch is ended and started again, so that each asks at once. Returns how
    /// many were ended.
    pub fn ask_now(&self) -> usize {
        let ends: Vec<(String, Arc<End>)> = locked(&self.running)
            .iter()
            .filter_map(|(account, end)| Some((account.clone(), Arc::clone(end.as_ref()?))))
            .collect();
        for (account, end) in &ends {
            locked(&self.again).insert(account.clone());
            end.kill();
        }
        ends.len()
    }

    /// The accounts that have a watch (or are starting one).
    pub fn accounts(&self) -> Vec<String> {
        let mut accounts: Vec<String> = locked(&self.running).keys().cloned().collect();
        accounts.sort();
        accounts
    }
}

/// Reads `from` to its end on a thread of its own, and drops it.
fn drain<R: Read + Send + 'static>(mut from: R) {
    thread::spawn(move || {
        let _ = io::copy(&mut from, &mut io::sink());
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::{Command, Stdio};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    fn program(script: &str) -> RemoteCommand {
        use std::os::unix::process::CommandExt as _;
        let child = Command::new("/bin/sh")
            .args(["-c", script])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0)
            .spawn()
            .expect("a program");
        RemoteCommand::local(child)
    }

    fn wait_for(what: &str, mut done: impl FnMut() -> bool) {
        let until = Instant::now() + Duration::from_secs(20);
        while !done() {
            assert!(Instant::now() < until, "never: {what}");
            thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn an_account_has_one_watch_at_a_time_and_the_second_call_leaves_the_first_endable() {
        let watches = Arc::new(Watches::new());
        let starts = Arc::new(AtomicUsize::new(0));
        let start = {
            let starts = Arc::clone(&starts);
            move || {
                // The first runs until it is ended; the one after it ends by itself.
                let n = starts.fetch_add(1, Ordering::SeqCst);
                Ok(program(if n == 0 { "sleep 60" } else { "exit 0" }))
            }
        };
        let keeper = {
            let (watches, start) = (Arc::clone(&watches), start.clone());
            thread::spawn(move || watches.keep("u@h:22", &start))
        };
        wait_for("the watch to run", || {
            starts.load(Ordering::SeqCst) == 1 && watches.accounts() == ["u@h:22"]
        });
        // A second connection event for the account starts nothing, and takes nothing away.
        watches.keep("u@h:22", &start);
        assert_eq!(starts.load(Ordering::SeqCst), 1);
        wait_for("the first watch to be endable", || watches.ask_now() == 1);
        keeper
            .join()
            .expect("the keeper ends with the second watch");
        assert_eq!(starts.load(Ordering::SeqCst), 2);
        assert!(watches.accounts().is_empty());
    }

    #[test]
    fn check_now_ends_a_watch_and_has_it_started_again_to_ask_at_once() {
        let watches = Arc::new(Watches::new());
        let starts = Arc::new(AtomicUsize::new(0));
        let keeper = {
            let (watches, starts) = (Arc::clone(&watches), Arc::clone(&starts));
            thread::spawn(move || {
                watches.keep("u@h:22", &|| {
                    // The second one ends by itself, so that the keeper does.
                    let n = starts.fetch_add(1, Ordering::SeqCst);
                    Ok(program(if n == 0 { "sleep 60" } else { "exit 0" }))
                });
            })
        };
        wait_for("the watch to run", || {
            starts.load(Ordering::SeqCst) == 1 && watches.ask_now() == 1
        });
        keeper.join().expect("the keeper ends");
        assert_eq!(starts.load(Ordering::SeqCst), 2);
        assert!(watches.accounts().is_empty());
        assert_eq!(watches.ask_now(), 0);
    }

    #[test]
    fn a_watch_that_cannot_be_started_leaves_the_account_free_for_the_next_connection() {
        let watches = Watches::new();
        watches.keep("u@h:22", &|| Err("no connection".to_owned()));
        assert!(watches.accounts().is_empty());
        let started = AtomicUsize::new(0);
        watches.keep("u@h:22", &|| {
            started.fetch_add(1, Ordering::SeqCst);
            Ok(program("exit 0"))
        });
        assert_eq!(started.load(Ordering::SeqCst), 1);
    }
}
