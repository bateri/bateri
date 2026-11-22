//! A profile that is kept at the home of its program: what a Mac does so that the cookies, the
//! Local Storage and the IndexedDB of a page that runs on a server are the same on every
//! computer that shows the page.
//!
//! - [`records`]: what the helper calls a record and what the home keeps (the two formats).
//! - [`plan`]: which of a profile's records are taken from the home, sent to it or dropped.
//! - [`store`]: what is kept between runs.
//! - [`gate`]: what a browser's first page and a profile's requests wait for.
//! - [`engine`]: the state machine that does it, with no threads.
//! - [`ProfileSync`] (here): the engine on a thread of its own, with the connection to the home
//!   made, kept and made again.
//!
//! A profile with no depot (its program runs on this Mac) has none of this: the sync is made
//! only for a machine the shell gave a depot ([`super::depot`]).

pub mod engine;
pub mod gate;
pub mod plan;
pub mod records;
pub mod site;
pub mod store;

#[cfg(test)]
pub(crate) mod tests;

use std::collections::HashMap;
use std::io::{self, Write};
use std::sync::atomic::AtomicU32;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use browser_ipc::message::Event;
use kablo_proto::profile::{self as wire, Client, Server};

use self::engine::{Config, Engine, Helper, Home, Input};
use self::gate::Gate;
use super::depot::{Depot, OPEN_WAIT, SYNC_WAIT, Turn};
use super::profile::ProfileKey;
use super::proxy::Holds;

/// How long the home may take to answer the hello: a server that has no `kablo profile` to speak
/// exits at once; one that does not answer is let go of.
const HELLO_WAIT: Duration = Duration::from_secs(10);

/// How soon a home that was lost is tried again, and the longest it is waited for.
const RETRY_FIRST: Duration = if cfg!(test) {
    Duration::from_millis(20)
} else {
    Duration::from_secs(1)
};
const RETRY_LONGEST: Duration = Duration::from_secs(60);

/// How often what is to be kept is written, while there is something to write.
const SAVE_EVERY: Duration = Duration::from_secs(1);

/// How long the thread sleeps when nothing is to be written and nothing is waited for: it is
/// woken by what comes, not by the clock.
const IDLE_WAIT: Duration = Duration::from_secs(3600);

enum Msg {
    Input(Input),
    /// The hello of connection `.0` is over: the writer to speak by, or why not.
    Ready(u64, Option<Box<dyn Write + Send>>),
    Server(u64, Server),
    Closed(u64),
    Stop,
}

/// The sync of one profile, on a thread of its own.
pub struct ProfileSync {
    tx: Sender<Msg>,
    gate: Arc<Gate>,
    /// Said once the thread has ended and what was to be kept is written.
    ended: Mutex<Receiver<()>>,
}

/// The way the engine speaks to the home: the wire's messages on the connection's writer.
struct WireHome {
    writer: Box<dyn Write + Send>,
}

impl Home for WireHome {
    fn send(&mut self, message: &Client) -> io::Result<()> {
        wire::write_client(&mut self.writer, message)
            .map_err(|e| io::Error::other(e.to_string()))?;
        self.writer.flush()
    }
}

fn backoff(failures: u32) -> Duration {
    RETRY_FIRST
        .saturating_mul(1u32 << failures.min(6))
        .min(RETRY_LONGEST)
}

impl ProfileSync {
    /// Starts the sync of the profile `cfg` names, with the home `depot` opens connections to,
    /// speaking to the helper through `helper`. `turn` is a turn of the person's switch for the
    /// machine that is yet to be told to it ([`Turn`]).
    pub fn start(
        cfg: Config,
        helper: Box<dyn Helper>,
        depot: Arc<dyn Depot>,
        reqs: Arc<AtomicU32>,
        turn: Option<Turn>,
    ) -> io::Result<Arc<ProfileSync>> {
        let gate = Arc::new(Gate::new());
        let engine = Engine::new(cfg, helper, gate.clone(), reqs);
        let (tx, rx) = channel();
        let (end, ended) = channel();
        let sender = tx.clone();
        std::thread::Builder::new()
            .name("profile-sync".into())
            .spawn(move || {
                drive(engine, depot.as_ref(), turn, &rx, &sender);
                let _ = end.send(());
            })?;
        Ok(Arc::new(ProfileSync {
            tx,
            gate,
            ended: Mutex::new(ended),
        }))
    }

    /// What a browser's first page and the profile's requests wait for.
    pub fn gate(&self) -> &Arc<Gate> {
        &self.gate
    }

    pub fn attach(&self, browser: u32) {
        let _ = self.tx.send(Msg::Input(Input::Attach(browser)));
    }

    pub fn detach(&self, browser: u32) {
        let _ = self.tx.send(Msg::Input(Input::Detach(browser)));
    }

    /// The person cleared the data of site `host` (a name) in `browser`: the sync says the
    /// deletion of what it knows of the site to the home, and asks the helper to delete it too,
    /// as request `req` ([`Event::SiteCleared`] answers it).
    pub fn clear_site(&self, browser: u32, req: u32, host: String) -> bool {
        self.tx
            .send(Msg::Input(Input::ClearSite { browser, req, host }))
            .is_ok()
    }

    /// The helper's news of the profile; given back when the sync is over and nobody reads it.
    pub fn event(&self, event: Event) -> Option<Event> {
        match self.tx.send(Msg::Input(Input::Helper(event))) {
            Ok(()) => None,
            Err(e) => match e.0 {
                Msg::Input(Input::Helper(event)) => Some(event),
                _ => None,
            },
        }
    }

    /// Ends the sync: what is kept is written, the home is hung up on, nothing waits any more.
    pub fn stop(&self) {
        let _ = self.tx.send(Msg::Stop);
        self.gate.release_all();
        self.gate.hold(false);
    }
}

impl ProfileSync {
    /// Whether the thread has ended within `wait` (after [`ProfileSync::stop`]).
    pub fn ended(&self, wait: Duration) -> bool {
        let ended = self.ended.lock().unwrap_or_else(PoisonError::into_inner);
        matches!(
            ended.recv_timeout(wait),
            Ok(()) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected)
        )
    }
}

impl Drop for ProfileSync {
    fn drop(&mut self) {
        let _ = self.tx.send(Msg::Stop);
    }
}

/// A connection being made or made: the way to close it.
struct Connection {
    id: u64,
    end: Arc<dyn Fn() + Send + Sync>,
    since: Instant,
    /// The hello is not over yet.
    greeting: bool,
}

fn connect(depot: &dyn Depot, id: u64, tx: &Sender<Msg>) -> Result<Connection, String> {
    let link = depot.open(OPEN_WAIT)?;
    let parts = link.split();
    let (mut reader, mut writer, end) = (parts.reader, parts.writer, parts.end);
    let sender = tx.clone();
    let hold = parts.hold;
    std::thread::Builder::new()
        .name("profile-sync-read".into())
        .spawn(move || {
            let _hold = hold;
            if wire::WIRE.client(&mut reader, &mut writer).is_err() {
                let _ = sender.send(Msg::Ready(id, None));
                let _ = sender.send(Msg::Closed(id));
                return;
            }
            let _ = sender.send(Msg::Ready(id, Some(writer)));
            while let Ok(Some(message)) = wire::read_server(&mut reader) {
                if sender.send(Msg::Server(id, message)).is_err() {
                    return;
                }
            }
            let _ = sender.send(Msg::Closed(id));
        })
        .map_err(|e| e.to_string())?;
    Ok(Connection {
        id,
        end,
        since: Instant::now(),
        greeting: true,
    })
}

fn drive(
    mut engine: Engine,
    depot: &dyn Depot,
    mut turn: Option<Turn>,
    rx: &std::sync::mpsc::Receiver<Msg>,
    tx: &Sender<Msg>,
) {
    let mut next_id = 0u64;
    let mut conn: Option<Connection> = None;
    let mut failures = 0u32;
    let mut retry_at = Instant::now();
    let mut saved_at = Instant::now();
    let mut told_at = Instant::now();
    let mut told_failures = 0u32;
    // When the profile's requests stop waiting for a join that is not over (one bound for each
    // connection: a home that says hello and then goes slow does not hold a page for ever).
    let mut hold_until: Option<Instant> = None;
    loop {
        if conn.is_none()
            && engine.is_down()
            && engine.refused().is_none()
            && Instant::now() >= retry_at
        {
            // A switch turned on again: the mark that none is kept is taken away before the
            // home is asked, or the home would turn the Mac away for good.
            let mut lifted = true;
            if let Some(Turn::Lift(mark)) = turn.clone() {
                if depot.switch(true, OPEN_WAIT).is_ok() {
                    mark.remember(true);
                    turn = None;
                } else {
                    lifted = false;
                }
            }
            next_id += 1;
            let made = if lifted {
                connect(depot, next_id, tx)
            } else {
                Err("the mark is still there".to_owned())
            };
            match made {
                Ok(made) => conn = Some(made),
                Err(_) => {
                    failures += 1;
                    retry_at = Instant::now() + backoff(failures);
                    // A home that cannot be reached is not waited for.
                    engine.unreachable();
                }
            }
        }
        let now = Instant::now();
        let retrying = conn.is_none() && engine.is_down() && engine.refused().is_none();
        let busy = engine.is_dirty() || turn.is_some() || conn.as_ref().is_some_and(|c| c.greeting);
        let mut wait = if retrying {
            retry_at.saturating_duration_since(now)
        } else if busy {
            SAVE_EVERY
        } else {
            IDLE_WAIT
        };
        if let Some(at) = hold_until {
            wait = wait.min(at.saturating_duration_since(now));
        }
        let message = match rx.recv_timeout(wait.max(Duration::from_millis(1))) {
            Ok(message) => Some(message),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => None,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => Some(Msg::Stop),
        };
        match message {
            Some(Msg::Input(input)) => engine.handle(input),
            Some(Msg::Ready(id, writer)) => {
                if let Some(c) = conn.as_mut().filter(|c| c.id == id) {
                    c.greeting = false;
                    match writer {
                        Some(writer) => {
                            engine.connected(Box::new(WireHome { writer }));
                            hold_until = Some(Instant::now() + SYNC_WAIT);
                        }
                        None => {
                            // No hello: the server has no kablo that keeps profiles, or it
                            // answers nothing. A browser does not wait for it.
                            engine.unreachable();
                            (c.end)();
                        }
                    }
                }
            }
            Some(Msg::Server(id, server)) => {
                if conn.as_ref().is_some_and(|c| c.id == id) {
                    engine.handle(Input::Home(server));
                }
            }
            Some(Msg::Closed(id)) => {
                if conn.as_ref().is_some_and(|c| c.id == id) {
                    // A connection that got as far as being live was a good one; one that was
                    // let down before that (a hello that did not come, a join that did not end)
                    // is one more failure, and the next try waits longer.
                    let was_live = engine.is_live();
                    conn = None;
                    engine.lost();
                    hold_until = None;
                    failures = if was_live { 0 } else { failures + 1 };
                    retry_at = Instant::now() + backoff(failures);
                }
            }
            Some(Msg::Stop) => {
                if let Some(c) = conn.take() {
                    (c.end)();
                }
                engine.stop();
                return;
            }
            None => {}
        }
        if engine.is_live() {
            failures = 0;
            hold_until = None;
        } else if hold_until.is_some_and(|at| Instant::now() >= at) {
            engine.let_requests_go();
            hold_until = None;
        }
        // A hello that is not answered ends the connection.
        if let Some(c) = &conn
            && c.greeting
            && c.since.elapsed() >= HELLO_WAIT
        {
            (c.end)();
        }
        // The engine let go of the home by itself (a write failed): the connection is closed.
        if engine.is_down()
            && let Some(c) = conn.as_ref().filter(|c| !c.greeting)
        {
            (c.end)();
        }
        // A switch turned off: what the home had is taken first (the join is over), then the
        // machine is told to keep none and the sync ends. A home that turned the Mac away for
        // that reason has been told already.
        if let Some(Turn::Close(mark)) = turn.clone() {
            match engine.refused() {
                Some(wire::code::OFF) => {
                    mark.remember(false);
                    turn = None;
                }
                Some(_) => turn = None,
                None if engine.is_live() && Instant::now() >= told_at => {
                    if depot.switch(false, OPEN_WAIT).is_ok() {
                        mark.remember(false);
                        if let Some(c) = conn.take() {
                            (c.end)();
                        }
                        engine.stop();
                        return;
                    }
                    told_failures += 1;
                    told_at = Instant::now() + backoff(told_failures);
                }
                None => {}
            }
        }
        if saved_at.elapsed() >= SAVE_EVERY {
            engine.tick();
            saved_at = Instant::now();
        }
    }
}

// ---- the syncs of a helper run ----

/// The syncs that run beside one helper, by the profile's folder: the browsers of a profile share
/// one. It also routes what the helper says of a profile to its sync, and holds the requests of a
/// profile that is being brought up to date ([`Holds`]).
#[derive(Default)]
pub struct Syncs {
    slots: Mutex<Slots>,
}

#[derive(Default)]
struct Slots {
    by_dir: HashMap<String, Slot>,
    by_browser: HashMap<u32, String>,
    by_id: HashMap<u32, String>,
    next: u32,
}

struct Slot {
    id: u32,
    sync: Arc<ProfileSync>,
    browsers: Vec<u32>,
}

/// The helper's requests of a sync are numbered from `id << REQ_BITS`, so that an answer that
/// names no browser (a folder read or written) is found by its number.
const REQ_BITS: u32 = engine::REQ_BITS;

/// How many syncs may run beside one helper: the numbers of their requests are `id << REQ_BITS`
/// in a `u32`.
const MAX_SYNCS: u32 = (1 << (32 - REQ_BITS)) - 1;

impl Syncs {
    pub fn new() -> Arc<Syncs> {
        Arc::new(Syncs::default())
    }

    fn slots(&self) -> std::sync::MutexGuard<'_, Slots> {
        self.slots.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The sync of the profile in folder `dir`, started by `make` (given the number its requests
    /// go from) when there is none, with `browser` as one of its browsers.
    pub fn join(
        &self,
        dir: &str,
        browser: u32,
        make: impl FnOnce(Arc<AtomicU32>) -> io::Result<Arc<ProfileSync>>,
    ) -> io::Result<Arc<ProfileSync>> {
        let mut slots = self.slots();
        if !slots.by_dir.contains_key(dir) {
            // The next number that no live sync has: a number given again would send the
            // answers of one profile's folders to another's.
            let mut id = 0;
            for _ in 0..MAX_SYNCS {
                slots.next = slots.next % MAX_SYNCS + 1;
                if !slots.by_id.contains_key(&slots.next) {
                    id = slots.next;
                    break;
                }
            }
            if id == 0 {
                return Err(io::Error::other("too many profiles are being synced"));
            }
            let sync = make(Arc::new(AtomicU32::new(id << REQ_BITS)))?;
            slots.by_id.insert(id, dir.to_owned());
            slots.by_dir.insert(
                dir.to_owned(),
                Slot {
                    id,
                    sync,
                    browsers: Vec::new(),
                },
            );
        }
        let Some(slot) = slots.by_dir.get_mut(dir) else {
            return Err(io::Error::other("a sync that is not there"));
        };
        slot.browsers.push(browser);
        let sync = slot.sync.clone();
        slots.by_browser.insert(browser, dir.to_owned());
        Ok(sync)
    }

    /// `browser` is closed: the last one of a profile ends its sync.
    pub fn leave(&self, browser: u32) {
        let ended = {
            let mut slots = self.slots();
            let Some(dir) = slots.by_browser.remove(&browser) else {
                return;
            };
            let Some(slot) = slots.by_dir.get_mut(&dir) else {
                return;
            };
            slot.browsers.retain(|b| *b != browser);
            slot.sync.detach(browser);
            if slot.browsers.is_empty() {
                let id = slot.id;
                slots.by_id.remove(&id);
                slots.by_dir.remove(&dir)
            } else {
                None
            }
        };
        if let Some(slot) = ended {
            slot.sync.stop();
        }
    }

    /// Clears the data of site `host` in `browser` through the profile's sync, so that the home
    /// is told too. `false` for a browser whose profile has no sync (one kept on this Mac alone,
    /// one with no name): its caller asks the helper itself, and the deletion is local.
    pub fn clear_site(&self, browser: u32, req: u32, host: &str) -> bool {
        let sync = {
            let slots = self.slots();
            slots
                .by_browser
                .get(&browser)
                .and_then(|dir| slots.by_dir.get(dir))
                .map(|slot| slot.sync.clone())
        };
        match sync {
            // A sync that has ended takes nothing: the helper is asked directly, as for a profile with none.
            Some(sync) => sync.clear_site(browser, req, host.to_owned()),
            None => false,
        }
    }

    /// What the helper says of a profile goes to its sync; anything else is given back. The
    /// answer to a site's clearing goes to its sync *and* is given back: the sync writes the
    /// deletions to the home, and whoever asked (a page's room, an agent's job) hears how it ended.
    pub fn route(&self, event: Event) -> Option<Event> {
        let target = {
            let slots = self.slots();
            let dir = match &event {
                Event::ProfileRecords { browser, .. }
                | Event::ProfileWritten { browser, .. }
                | Event::ProfileChanged { browser, .. }
                | Event::SiteCleared { browser, .. } => slots.by_browser.get(browser),
                Event::IndexedDbRecords { req, .. } | Event::IndexedDbWritten { req, .. } => {
                    slots.by_id.get(&(req >> REQ_BITS))
                }
                _ => return Some(event),
            };
            dir.and_then(|dir| slots.by_dir.get(dir))
                .map(|s| s.sync.clone())
        };
        let Some(sync) = target else {
            // A folder answer with nobody to read it is given back: its stage is the caller's to
            // drop; so is a site's clearing, which a browser of no sync asked for. The rest is news
            // of a profile that is gone.
            return matches!(
                event,
                Event::IndexedDbRecords { .. } | Event::SiteCleared { .. }
            )
            .then_some(event);
        };
        if matches!(event, Event::SiteCleared { .. }) {
            sync.event(event.clone());
            return Some(event);
        }
        sync.event(event)
            .filter(|event| matches!(event, Event::IndexedDbRecords { .. }))
    }

    /// How many profiles have a sync.
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.slots().by_dir.len()
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The helper is gone: nothing waits for its answers any more.
    pub fn stop_all(&self) {
        let slots = std::mem::take(&mut *self.slots());
        for (_, slot) in slots.by_dir {
            slot.sync.stop();
        }
    }
}

impl Holds for Syncs {
    fn hold(&self, profile: &ProfileKey) {
        let Some(dir) = profile.dir() else { return };
        let gate = self.slots().by_dir.get(&dir).map(|s| s.sync.gate().clone());
        if let Some(gate) = gate {
            gate.wait_released(Instant::now() + SYNC_WAIT);
        }
    }
}
