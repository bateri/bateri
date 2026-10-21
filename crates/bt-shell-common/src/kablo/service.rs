//! The running end of `ssh-hold`: the socket panes' programs connect to, the
//! [`Hub`] that ties each of them to a terminal, and the bridges to the
//! servers those terminals live on.
//!
//! [`Service::start`] binds `<instance directory>/hold`, accepts the holders
//! that connect and runs one thread that owns everything else: it reads what the
//! holders and the bridges say from a channel, hands it to the hub and the
//! clients (`kablo::hub`, `kablo::Client`, both without a transport of their
//! own), and writes what they have to say back. The transports are threads that
//! only carry bytes (one reader and one writer for a holder, for a bridge), and
//! the slow work — a master connection, the install, starting `ssh … kablo
//! attach` — runs on a thread of its own per request, so one server's slowness
//! is that server's. What the service cannot know is [`Backend`]'s: which pane
//! a process belongs to, whether a call is a kablo call, the steps on the
//! server. The macOS shell supplies the pane; [`crate::kablo::ssh`] the rest.
//!
//! **Who may ask.** The socket is private to the user (the directory is) and
//! the peer's uid is the user's; on top of it, the process that connects must
//! be **in a pane's own terminal** ([`Backend::pane`], read from the kernel's
//! record of the peer's controlling terminal, never from anything the peer
//! says): a program elsewhere cannot show a pane another server's screen. A
//! request that fails the check, or that is not a kablo call (the setting is
//! off, a `production` host, a command to run, a server kablo cannot be kept
//! on), is answered with the plain `ssh` — the holder replaces itself with it.
//!
//! **One bridge per account**, the account being the server's key
//! (`user@host:port`, `ssh_wrap::host_key`): the first terminal of an account
//! makes the master connection (on the pane's own terminal, where the user
//! answers the password and the server's key — [`Backend::master`]), puts kablo
//! on the server ([`Backend::install`]) and starts the bridge ([`Backend::carry`]);
//! the next terminals of the account — another pane, a ⌘T — ride the bridge
//! that is there and ask for nothing. A bridge that goes (the connection broke)
//! is made again by the account's client with growing pauses (`kablo::retry_delay`);
//! the holders stay and their terminals go on from the byte they were at. The
//! pane does not yet look different while it waits, and a login the connection
//! needs again is not asked for: it keeps trying with what `ssh` can do
//! without a person.
//!
//! **What a pane shows, and ending it.** The application asks the service which
//! terminal a pane shows ([`Service::shown`]: the account, the server, the
//! program in front as the server last said) for the questions it has about a
//! pane that is not the local shell's — the one before closing it, the folder a
//! new pane opens in — and tells it that the user closed the pane
//! ([`Service::end`]), which ends the terminal on the server: `SIGHUP` at once,
//! and when the terminal is still there after a grace, the kill. A pane that goes
//! with the application (a quit, a crash, an update) never says it, because the
//! pane's closing alone lets go of the terminal and it lives on.
//!
//! **What does not survive bateri.** A holder asks for a terminal it was
//! shown ([`wire::Target::Resume`]) by the account's name; an account bateri
//! has no client for (it was restarted) cannot be served, because the server's
//! address was never written down: the holder ends with a line. Writing it down
//! is the update's and the crash's work.

use std::collections::{BTreeMap, HashMap};
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use bt_core::PaneUuid;
use kablo_proto::v1::NewSession;

use super::hub::{HoldId, Hub, HubEvent};
use super::install::Installed;
use super::{Cause, Client, Event, Link, SessionId};
use crate::hold::HOLD_SOCKET;
use crate::hold::wire::{CHUNK, Open, Target};
use crate::ssh_wrap::{self, KabloHost};

/// The process at the other end of a holder's connection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Peer {
    /// Its user.
    pub uid: u32,
    /// Its process id.
    pub pid: u32,
}

/// The pane a holder runs in, as far as the service needs to know.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PaneInfo {
    /// The pane's identity, which the terminal's shell exports as
    /// `LC_BATERI_TAB_URL` on the server.
    pub tab: Option<PaneUuid>,
    /// The local block of the command that is running (the `ssh` itself): the
    /// parent of the remote blocks.
    pub block: Option<u32>,
    /// The folder the terminal's shell starts in on the server: the one a pane
    /// opened from a remote pane inherits. Used once.
    pub directory: Option<String>,
}

/// The terminal a pane shows, as far as the application's questions go.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shown {
    /// The server's account ([`ssh_wrap::host_key`]).
    pub account: String,
    /// The server as the user typed it (`rasp`): the word that names it in the
    /// lines the user reads.
    pub host: String,
    /// The terminal's name on the server.
    pub terminal: SessionId,
    /// The program in front of the terminal as the server last said, controls
    /// removed; `None` while it has not said.
    pub program: Option<String>,
}

impl Shown {
    /// What a question about closing the pane calls the program that would
    /// end: the program in front with the server it runs on (`npm on rasp`), or
    /// `None` when the shell is in front (waiting for a command) or the server
    /// has not said what is.
    pub fn running(&self) -> Option<String> {
        let program = self.program.as_deref().filter(|name| !name.is_empty())?;
        (!crate::program::is_shell_name(program)).then(|| format!("{program} on {}", self.host))
    }
}

/// Why no terminal is shown, and what the pane does instead.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refusal {
    /// The holder replaces itself with the plain `ssh` it stands in for
    /// (nothing has been shown yet), or ends and says `text`.
    pub fall_back: bool,
    /// What to tell the user when it does not fall back.
    pub text: String,
}

impl Refusal {
    /// The plain `ssh`.
    pub fn plain() -> Refusal {
        Refusal {
            fall_back: true,
            text: String::new(),
        }
    }

    /// An end with a line to read.
    pub fn ended(text: impl Into<String>) -> Refusal {
        Refusal {
            fall_back: false,
            text: text.into(),
        }
    }
}

/// A started bridge: the two ends of the stream, and how to end it.
pub struct Carried {
    /// What the bridge says.
    pub reader: Box<dyn Read + Send>,
    /// What is said to it.
    pub writer: Box<dyn Write + Send>,
    /// Ends the process that carries them (called once, when the bridge is let
    /// go of).
    pub close: Box<dyn FnOnce() + Send>,
}

/// What the service asks of the world around it. Every method but
/// [`Backend::pane`] and [`Backend::route`] may take its time: the service
/// calls them on a thread of their own.
pub trait Backend: Send + Sync + 'static {
    /// The pane the process runs in, or `None` when it is in no pane's own
    /// terminal.
    fn pane(&self, peer: &Peer) -> Option<PaneInfo>;

    /// Whether the `ssh` arguments are a kablo call, and to whom. `None`: the
    /// plain `ssh`.
    fn route(&self, args: &[String]) -> Option<KabloHost>;

    /// The program that makes sure a master connection to the server exists, to
    /// be run on the pane's terminal (where it may ask for a password and for
    /// the server's key), or `None` when one is there.
    fn master(&self, server: &KabloHost) -> Result<Option<Vec<Vec<u8>>>, Refusal>;

    /// Called with how the program of [`Backend::master`] ended.
    fn master_made(&self, server: &KabloHost, status: u8) -> Result<(), Refusal>;

    /// Puts kablo on the server (or finds it there).
    fn install(&self, server: &KabloHost) -> Result<Installed, Refusal>;

    /// Starts the bridge to the server.
    fn carry(&self, server: &KabloHost, installed: &Installed) -> io::Result<Carried>;

    /// How this computer calls itself in the list of who is attached to a
    /// terminal.
    fn name(&self) -> Option<String> {
        None
    }

    /// How many rows of scrollback the server keeps for a new terminal.
    fn history(&self) -> Option<u32> {
        None
    }
}

/// What the loop is told, from the threads that carry bytes and do the slow
/// work.
enum Msg {
    Accepted(UnixStream, Peer),
    Hold(HoldId, Vec<u8>),
    HoldEof(HoldId),
    /// A request's preparation wants `argv` run on the holder's terminal; the
    /// status goes back on `reply`.
    Run {
        hold: HoldId,
        argv: Vec<Vec<u8>>,
        reply: Sender<u8>,
    },
    /// The questions that are quick but not instant (which pane, is it a
    /// kablo call) were answered for a request.
    Admitted {
        hold: HoldId,
        open: Open,
        pane: Option<PaneInfo>,
        server: Option<KabloHost>,
    },
    Prepared {
        hold: HoldId,
        outcome: Result<Ready, Refusal>,
    },
    Dialed {
        account: String,
        outcome: io::Result<Carried>,
    },
    Bridge {
        account: String,
        generation: u64,
        bytes: Vec<u8>,
    },
    /// The user closed the pane: the terminal it shows ends.
    End(PaneUuid),
    BridgeEof {
        account: String,
        generation: u64,
    },
    Stop,
}

/// What a request's preparation produced.
struct Ready {
    server: KabloHost,
    installed: Installed,
    spec: NewSession,
    /// A line for the user, shown before the terminal.
    note: Option<String>,
}

/// More queued for a holder than this and the bridge's stream is not read: the
/// server's own flow control reaches the program.
const HIGH_WATER: usize = 1 << 20;

/// What a holder's writer thread is given.
enum Out {
    Bytes(Vec<u8>, Arc<AtomicUsize>),
    Close,
}

/// One holder's connection as the loop keeps it.
struct Conn {
    peer: Peer,
    out: Sender<Out>,
}

/// A bridge that is up.
struct Bridge {
    writer: Sender<Vec<u8>>,
    close: Option<Box<dyn FnOnce() + Send>>,
}

impl Drop for Bridge {
    fn drop(&mut self) {
        if let Some(close) = self.close.take() {
            close();
        }
    }
}

/// A server the service has a client for.
struct Account {
    server: KabloHost,
    installed: Installed,
    /// Which bridge messages are current.
    generation: u64,
    bridge: Option<Bridge>,
    /// A thread is starting a bridge.
    dialing: bool,
    /// Bytes queued for the holders of this account's terminals.
    queued: Arc<AtomicUsize>,
}

/// The running service. Dropping it stops it.
pub struct Service {
    socket: PathBuf,
    tx: Sender<Msg>,
    stop: Arc<AtomicBool>,
    threads: Mutex<Vec<thread::JoinHandle<()>>>,
    /// What each pane shows, as the loop last wrote it.
    shown: Arc<Mutex<HashMap<PaneUuid, Shown>>>,
}

/// How long a terminal that was asked to hang up has before it is killed.
pub const END_GRACE: Duration = Duration::from_secs(3);

impl Service {
    /// Binds `<dir>/hold` (a stale file is removed first) and starts serving.
    /// `dir` is the instance's directory, which must be a private one.
    pub fn start(dir: &Path, backend: Arc<dyn Backend>) -> io::Result<Service> {
        Service::start_with(dir, backend, END_GRACE)
    }

    /// [`Service::start`] with the time a terminal that was asked to end has
    /// before it is killed.
    pub fn start_with(
        dir: &Path,
        backend: Arc<dyn Backend>,
        grace: Duration,
    ) -> io::Result<Service> {
        if !crate::ssh_route::private_dir(dir) {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        let socket = dir.join(HOLD_SOCKET);
        if socket.as_os_str().len() >= crate::ssh_route::SUN_PATH {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        match std::fs::remove_file(&socket) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        let listener = UnixListener::bind(&socket)?;
        listener.set_nonblocking(true)?;
        let (tx, rx) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let accepting = {
            let (tx, stop) = (tx.clone(), Arc::clone(&stop));
            thread::Builder::new()
                .name("ssh-hold accept".into())
                .spawn(move || accept_loop(&listener, &tx, &stop))?
        };
        let shown = Arc::new(Mutex::new(HashMap::new()));
        let core = Core {
            shared: Arc::new(Shared {
                backend,
                tx: tx.clone(),
            }),
            hub: Hub::new(),
            conns: BTreeMap::new(),
            accounts: BTreeMap::new(),
            waiting: BTreeMap::new(),
            panes: BTreeMap::new(),
            shown: Arc::clone(&shown),
            refresh: false,
            ending: Vec::new(),
            grace,
            rx,
            generation: 0,
        };
        let serving = thread::Builder::new()
            .name("ssh-hold service".into())
            .spawn(move || core.run())?;
        Ok(Service {
            socket,
            tx,
            stop,
            threads: Mutex::new(vec![accepting, serving]),
            shown,
        })
    }

    /// The terminal the pane shows, if it shows one: written by the loop after
    /// anything that could change it, so it is what was true a moment ago.
    pub fn shown(&self, pane: &PaneUuid) -> Option<Shown> {
        self.shown
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(pane)
            .cloned()
    }

    /// The user closed the pane: the terminal it shows ends on the server
    /// (hung up now, killed after [`END_GRACE`] if it is still there). Say it
    /// **before** the pane's program is signalled — the holder's going is the
    /// terminal being let go of, and from then on the pane is not found. A
    /// pane that shows nothing, and a connection that is down, do nothing.
    pub fn end(&self, pane: &PaneUuid) {
        let _ = self.tx.send(Msg::End(pane.clone()));
    }

    /// The socket's path.
    pub fn socket(&self) -> &Path {
        &self.socket
    }

    /// Stops the service: the holders' connections close, the bridges end.
    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = self.tx.send(Msg::Stop);
        let threads = std::mem::take(&mut *self.threads.lock().unwrap_or_else(|e| e.into_inner()));
        for thread in threads {
            let _ = thread.join();
        }
        let _ = std::fs::remove_file(&self.socket);
    }
}

impl Drop for Service {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Accepts connections until stopped. The socket is not blocking so that the
/// stop flag is looked at; `poll` wakes at once for a connection.
fn accept_loop(listener: &UnixListener, tx: &Sender<Msg>, stop: &AtomicBool) {
    // SAFETY: `getuid` has no preconditions and cannot fail.
    let uid = unsafe { libc::getuid() };
    while !stop.load(Ordering::SeqCst) {
        let mut fds = [libc::pollfd {
            fd: listener.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        }];
        // SAFETY: one valid descriptor, a bounded wait.
        unsafe { libc::poll(fds.as_mut_ptr(), 1, 200) };
        loop {
            match listener.accept() {
                Ok((stream, _)) => {
                    if stream.set_nonblocking(false).is_err() {
                        continue;
                    }
                    // Another user's process is turned away before it says a word.
                    let Ok((peer_uid, pid)) = crate::handover::peer(&stream) else {
                        continue;
                    };
                    if peer_uid != uid {
                        continue;
                    }
                    if tx
                        .send(Msg::Accepted(stream, Peer { uid: peer_uid, pid }))
                        .is_err()
                    {
                        return;
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                // Out of descriptors or the like: a pause, not a spin.
                Err(_) => {
                    thread::sleep(Duration::from_millis(100));
                    break;
                }
            }
        }
    }
}

struct Shared {
    backend: Arc<dyn Backend>,
    tx: Sender<Msg>,
}

/// Everything the loop owns.
struct Core {
    shared: Arc<Shared>,
    hub: Hub,
    conns: BTreeMap<HoldId, Conn>,
    accounts: BTreeMap<String, Account>,
    /// A preparation waits for the holder to run a program.
    waiting: BTreeMap<HoldId, Sender<u8>>,
    /// The pane each holder runs in, once it was admitted.
    panes: BTreeMap<HoldId, PaneUuid>,
    /// What [`Service::shown`] reads.
    shown: Arc<Mutex<HashMap<PaneUuid, Shown>>>,
    /// Something happened that [`Core::shown`] may say differently.
    refresh: bool,
    /// Terminals asked to hang up that are killed if they are still there.
    ending: Vec<Ending>,
    grace: Duration,
    rx: Receiver<Msg>,
    generation: u64,
}

/// A terminal asked to end: killed at `at` unless it has by then.
struct Ending {
    account: String,
    terminal: SessionId,
    at: Instant,
}

impl Core {
    fn run(mut self) {
        loop {
            let now = Instant::now();
            let wait = self
                .accounts
                .keys()
                .filter_map(|account| self.hub.client(account))
                .filter_map(Client::next_deadline)
                .chain(self.ending.iter().map(|ending| ending.at))
                .min()
                .map_or(Duration::from_secs(3600), |at| {
                    at.saturating_duration_since(now)
                });
            let message = match self.rx.recv_timeout(wait) {
                Ok(message) => Some(message),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => return,
            };
            if let Some(message) = message {
                if matches!(message, Msg::Stop) {
                    return;
                }
                self.handle(message);
                // What else is waiting, before the (costlier) settling.
                while let Ok(more) = self.rx.try_recv() {
                    if matches!(more, Msg::Stop) {
                        return;
                    }
                    self.handle(more);
                }
            }
            self.settle();
        }
    }

    fn handle(&mut self, message: Msg) {
        let now = Instant::now();
        match message {
            Msg::Accepted(stream, peer) => self.accepted(stream, peer),
            Msg::Hold(hold, bytes) => self.hub.push(hold, &bytes),
            Msg::HoldEof(hold) => {
                self.hub.closed(hold);
            }
            Msg::Run { hold, argv, reply } => match self.hub.run(hold, &argv) {
                Ok(()) => {
                    self.waiting.insert(hold, reply);
                }
                // A holder that cannot run it (version 1, gone): no answer
                // comes and the preparation ends.
                Err(_) => drop(reply),
            },
            Msg::Admitted {
                hold,
                open,
                pane,
                server,
            } => {
                self.refresh = true;
                self.admitted(hold, &open, pane, server);
            }
            Msg::Prepared { hold, outcome } => {
                self.refresh = true;
                self.prepared(hold, outcome, now);
            }
            Msg::End(pane) => self.end(&pane, now),
            Msg::Dialed { account, outcome } => self.dialed(&account, outcome, now),
            Msg::Bridge {
                account,
                generation,
                bytes,
            } => {
                if self
                    .accounts
                    .get(&account)
                    .is_some_and(|a| a.generation == generation && a.bridge.is_some())
                    && let Some(client) = self.hub.client_mut(&account)
                {
                    client.push(&bytes, now);
                }
            }
            Msg::BridgeEof {
                account,
                generation,
            } => {
                if let Some(entry) = self.accounts.get_mut(&account)
                    && entry.generation == generation
                    && entry.bridge.take().is_some()
                    && let Some(client) = self.hub.client_mut(&account)
                    && matches!(client.link(), Link::Opening { .. } | Link::Live)
                {
                    client.lost(now, Cause::Dropped);
                }
            }
            Msg::Stop => {}
        }
    }

    fn accepted(&mut self, stream: UnixStream, peer: Peer) {
        let Ok(reader) = stream.try_clone() else {
            return;
        };
        let hold = self.hub.open();
        let (out, written) = mpsc::channel();
        self.conns.insert(hold, Conn { peer, out });
        let tx = self.shared.tx.clone();
        let _ = thread::Builder::new()
            .name("ssh-hold reads".into())
            .spawn(move || hold_reader(hold, reader, &tx));
        let _ = thread::Builder::new()
            .name("ssh-hold writes".into())
            .spawn(move || hold_writer(stream, &written));
    }

    /// Everything that follows from what was just heard: the clients move,
    /// the hub's events are answered, bytes go out, bridges are started or
    /// let go of — until nothing changes.
    fn settle(&mut self) {
        self.kill_due(Instant::now());
        for _ in 0..64 {
            let now = Instant::now();
            let accounts: Vec<String> = self.accounts.keys().cloned().collect();
            for account in &accounts {
                if let Some(client) = self.hub.client_mut(account) {
                    client.tick(now);
                }
                self.hub.pump(account);
            }
            let mut moved = false;
            while let Some(event) = self.hub.next_event() {
                moved = true;
                match event {
                    HubEvent::Wants { hold, open } => self.wants(hold, open),
                    HubEvent::Ran { hold, status } => {
                        if let Some(reply) = self.waiting.remove(&hold) {
                            let _ = reply.send(status);
                        }
                    }
                    HubEvent::Gone { hold } => {
                        self.waiting.remove(&hold);
                        self.conns.remove(&hold);
                        self.panes.remove(&hold);
                    }
                    HubEvent::Client { account, event } => {
                        if let Event::Ended { terminal, .. } = *event {
                            // It went by itself: nothing is left to kill.
                            self.ending
                                .retain(|e| !(e.account == account && e.terminal == terminal));
                        }
                    }
                }
                self.refresh = true;
            }
            self.over(&accounts);
            // A holder the hub has said its last word to is let go of, which the
            // hub tells as an event: another pass.
            moved |= self.flush();
            self.dial(now);
            if !moved {
                break;
            }
        }
        self.write_shown();
    }

    /// What each pane shows, written where [`Service::shown`] reads it. Only
    /// when something happened that could change it.
    fn write_shown(&mut self) {
        if !std::mem::take(&mut self.refresh) {
            return;
        }
        let mut shown = HashMap::new();
        for (hold, pane) in &self.panes {
            let Some((account, terminal)) = self.hub.terminal_of(*hold) else {
                continue;
            };
            let program = self
                .hub
                .client(account)
                .and_then(|client| client.state_of(terminal))
                .and_then(|state| state.program.as_deref())
                .map(|name| {
                    String::from_utf8_lossy(name)
                        .chars()
                        .filter(|c| !c.is_control())
                        .collect()
                });
            let host = self
                .accounts
                .get(account)
                .map(|entry| entry.server.host.clone())
                .unwrap_or_default();
            shown.insert(
                pane.clone(),
                Shown {
                    account: account.to_owned(),
                    host,
                    terminal: terminal.clone(),
                    program,
                },
            );
        }
        *self.shown.lock().unwrap_or_else(|e| e.into_inner()) = shown;
    }

    /// The user closed `pane`: the terminal it shows is asked to hang up, and
    /// killed after the grace if it is still there. A connection that is down
    /// cannot ask, and the terminal stays where it is.
    fn end(&mut self, pane: &PaneUuid, now: Instant) {
        let holds: Vec<HoldId> = self
            .panes
            .iter()
            .filter(|(_, owner)| *owner == pane)
            .map(|(hold, _)| *hold)
            .collect();
        for hold in holds {
            let Some((account, terminal)) = self
                .hub
                .terminal_of(hold)
                .map(|(account, terminal)| (account.to_owned(), terminal.clone()))
            else {
                continue;
            };
            if self
                .ending
                .iter()
                .any(|e| e.account == account && e.terminal == terminal)
            {
                continue;
            }
            let asked = self
                .hub
                .client_mut(&account)
                .is_some_and(|client| client.terminate(&terminal, false));
            if asked {
                self.ending.push(Ending {
                    account,
                    terminal,
                    at: now + self.grace,
                });
            }
        }
    }

    /// The terminals that were asked to hang up and are still there are killed.
    fn kill_due(&mut self, now: Instant) {
        if self.ending.is_empty() {
            return;
        }
        let (due, waiting): (Vec<Ending>, Vec<Ending>) = std::mem::take(&mut self.ending)
            .into_iter()
            .partition(|ending| ending.at <= now);
        self.ending = waiting;
        for ending in due {
            if let Some(client) = self.hub.client_mut(&ending.account) {
                client.terminate(&ending.terminal, true);
            }
        }
    }

    /// An account whose client is over for good is let go of: its holders
    /// fall back to the plain `ssh` or end with a line.
    fn over(&mut self, accounts: &[String]) {
        for account in accounts {
            let Some(Link::Over(cause)) = self.hub.client(account).map(Client::link) else {
                // A bridge a client no longer wants is ended: the process that
                // carries it would otherwise outlive the connection it carried.
                if let Some(link) = self.hub.client(account).map(Client::link)
                    && !matches!(link, Link::Opening { .. } | Link::Live)
                    && let Some(entry) = self.accounts.get_mut(account)
                {
                    entry.bridge = None;
                }
                continue;
            };
            let text = match cause {
                Cause::Incompatible => {
                    "bateri: the kablo on this server speaks a protocol this bateri does not"
                }
                _ => "bateri: the connection to this server could not be kept",
            };
            self.hub.drop_account(account, text);
            self.accounts.remove(account);
        }
    }

    /// Writes what the hub and the clients have to say. `true` when a holder was
    /// let go of (its connection is closed after what is queued).
    fn flush(&mut self) -> bool {
        let mut closed = false;
        let accounts: Vec<String> = self.accounts.keys().cloned().collect();
        for account in &accounts {
            let Some(client) = self.hub.client_mut(account) else {
                continue;
            };
            let bytes = client.take_outgoing();
            if bytes.is_empty() {
                continue;
            }
            if let Some(bridge) = self
                .accounts
                .get(account)
                .and_then(|entry| entry.bridge.as_ref())
            {
                let _ = bridge.writer.send(bytes);
            }
        }
        let holds: Vec<HoldId> = self.conns.keys().copied().collect();
        for hold in holds {
            let bytes = self.hub.take_outgoing(hold);
            let queued = self
                .hub
                .terminal_of(hold)
                .and_then(|(account, _)| self.accounts.get(account))
                .map(|entry| Arc::clone(&entry.queued))
                .unwrap_or_default();
            if let Some(conn) = self.conns.get(&hold) {
                if !bytes.is_empty() {
                    queued.fetch_add(bytes.len(), Ordering::SeqCst);
                    let _ = conn.out.send(Out::Bytes(bytes, queued));
                }
                if self.hub.is_finished(hold) {
                    let _ = conn.out.send(Out::Close);
                }
            }
            if self.hub.is_finished(hold) {
                self.hub.closed(hold);
                closed = true;
            }
        }
        closed
    }

    /// Starts a bridge for every account that is due one.
    fn dial(&mut self, now: Instant) {
        let due: Vec<String> = self
            .accounts
            .iter()
            .filter(|(account, entry)| {
                !entry.dialing
                    && entry.bridge.is_none()
                    && self
                        .hub
                        .client(account)
                        .is_some_and(|client| client.dial_due(now))
            })
            .map(|(account, _)| account.clone())
            .collect();
        for account in due {
            let Some(entry) = self.accounts.get_mut(&account) else {
                continue;
            };
            entry.dialing = true;
            let (server, installed) = (entry.server.clone(), entry.installed.clone());
            let shared = Arc::clone(&self.shared);
            let started = thread::Builder::new()
                .name("ssh-hold dials".into())
                .spawn({
                    let account = account.clone();
                    move || {
                        let outcome = shared.backend.carry(&server, &installed);
                        let _ = shared.tx.send(Msg::Dialed { account, outcome });
                    }
                })
                .is_ok();
            if !started {
                entry.dialing = false;
            }
        }
    }

    fn dialed(&mut self, account: &str, outcome: io::Result<Carried>, now: Instant) {
        let Some(entry) = self.accounts.get_mut(account) else {
            // The account is gone; what was started is ended with it.
            if let Ok(carried) = outcome {
                (carried.close)();
            }
            return;
        };
        entry.dialing = false;
        match outcome {
            Ok(carried) => {
                self.generation += 1;
                entry.generation = self.generation;
                let generation = self.generation;
                let Carried {
                    reader,
                    writer,
                    close,
                } = carried;
                let (to_bridge, written) = mpsc::channel();
                let tx = self.shared.tx.clone();
                let queued = Arc::clone(&entry.queued);
                let name = account.to_owned();
                let _ = thread::Builder::new()
                    .name("kablo bridge reads".into())
                    .spawn(move || bridge_reader(name, generation, reader, &tx, &queued));
                let _ = thread::Builder::new()
                    .name("kablo bridge writes".into())
                    .spawn(move || bridge_writer(writer, &written));
                entry.bridge = Some(Bridge {
                    writer: to_bridge,
                    close: Some(close),
                });
                if let Some(client) = self.hub.client_mut(account) {
                    client.connected(now);
                }
            }
            Err(_) => {
                if let Some(client) = self.hub.client_mut(account) {
                    client.lost(now, Cause::Dropped);
                }
            }
        }
    }

    /// A holder asked for a terminal. Which pane it is in and whether the call
    /// is a kablo call take a process or a hop to the main thread, so they are
    /// asked on a thread of their own; [`Core::admitted`] goes on from there.
    fn wants(&mut self, hold: HoldId, open: Open) {
        let Some(peer) = self.conns.get(&hold).map(|conn| conn.peer) else {
            return;
        };
        let shared = Arc::clone(&self.shared);
        let spawned = thread::Builder::new()
            .name("ssh-hold admits".into())
            .spawn({
                let shared = Arc::clone(&shared);
                move || {
                    // In a pane's own terminal, or not at all.
                    let pane = shared.backend.pane(&peer);
                    let server = match (&pane, &open.target) {
                        (Some(_), Target::New { args }) => args
                            .iter()
                            .map(|arg| String::from_utf8(arg.clone()).ok())
                            .collect::<Option<Vec<String>>>()
                            .and_then(|args| shared.backend.route(&args))
                            .filter(|server| server.key.len() <= 255),
                        _ => None,
                    };
                    let _ = shared.tx.send(Msg::Admitted {
                        hold,
                        open,
                        pane,
                        server,
                    });
                }
            });
        if spawned.is_err() {
            self.hub.refuse(hold, true, "");
        }
    }

    fn admitted(
        &mut self,
        hold: HoldId,
        open: &Open,
        pane: Option<PaneInfo>,
        server: Option<KabloHost>,
    ) {
        let Some(pane) = pane else {
            self.hub.refuse(hold, true, "");
            return;
        };
        if let Some(tab) = &pane.tab {
            self.panes.insert(hold, tab.clone());
        }
        match &open.target {
            Target::Resume { account, .. } => {
                if self.accounts.contains_key(account) && self.hub.resume(hold, account).is_ok() {
                    return;
                }
                self.hub.refuse(
                    hold,
                    false,
                    "bateri: this terminal's connection was lost when bateri restarted",
                );
            }
            Target::New { .. } => match server {
                Some(server) => self.prepare(hold, server, pane),
                None => self.hub.refuse(hold, true, ""),
            },
        }
    }

    /// The first terminal of an account is prepared on a thread of its own;
    /// one that rides a bridge that is there starts at once.
    fn prepare(&mut self, hold: HoldId, server: KabloHost, pane: PaneInfo) {
        if let Some(entry) = self.accounts.get(&server.key) {
            let spec = new_session(&self.shared, &pane);
            let account = entry.server.key.clone();
            if self.hub.start(hold, &account, spec).is_err() {
                self.hub.refuse(hold, true, "");
            }
            return;
        }
        let shared = Arc::clone(&self.shared);
        let spawned = thread::Builder::new()
            .name("ssh-hold prepares".into())
            .spawn({
                let shared = Arc::clone(&shared);
                move || {
                    let outcome = prepare(&shared, hold, server, &pane);
                    let _ = shared.tx.send(Msg::Prepared { hold, outcome });
                }
            });
        if spawned.is_err() {
            self.hub.refuse(hold, true, "");
        }
    }

    fn prepared(&mut self, hold: HoldId, outcome: Result<Ready, Refusal>, now: Instant) {
        self.waiting.remove(&hold);
        let ready = match outcome {
            Ok(ready) => ready,
            Err(refusal) => {
                self.hub.refuse(hold, refusal.fall_back, &refusal.text);
                return;
            }
        };
        let account = ready.server.key.clone();
        if !self.accounts.contains_key(&account) {
            let name = self.shared.backend.name();
            self.hub
                .add_client(&account, Client::new(name.as_deref(), now));
            self.accounts.insert(
                account.clone(),
                Account {
                    server: ready.server.clone(),
                    installed: ready.installed.clone(),
                    generation: 0,
                    bridge: None,
                    dialing: false,
                    queued: Arc::new(AtomicUsize::new(0)),
                },
            );
        }
        if let Some(note) = &ready.note {
            self.hub.say(hold, note);
        }
        if self.hub.start(hold, &account, ready.spec).is_err() {
            self.hub.refuse(hold, true, "");
        }
    }
}

/// The new terminal's request: the bootstrap as the command, bateri's
/// scrollback as the history.
fn new_session(shared: &Shared, pane: &PaneInfo) -> NewSession {
    let nonce = ssh_wrap::new_nonce().unwrap_or_else(|| "0".repeat(ssh_wrap::NONCE_LEN));
    // `NewSession` is not exhaustive: it is started from its default.
    let mut spec = NewSession::default();
    spec.argv = ssh_wrap::kablo_argv(
        pane.directory.as_deref(),
        pane.block,
        &nonce,
        pane.tab.as_ref(),
    );
    spec.history = shared.backend.history();
    spec
}

/// The slow steps of a first terminal, on a thread of its own: the master
/// connection on the holder's terminal, then kablo on the server.
fn prepare(
    shared: &Shared,
    hold: HoldId,
    server: KabloHost,
    pane: &PaneInfo,
) -> Result<Ready, Refusal> {
    let backend = &shared.backend;
    if let Some(argv) = backend.master(&server)? {
        let (reply, answer) = mpsc::channel();
        shared
            .tx
            .send(Msg::Run { hold, argv, reply })
            .map_err(|_| Refusal::plain())?;
        // The holder's terminal is the user's for as long as they take to
        // answer. No answer is a holder that went away.
        let status = answer.recv().map_err(|_| Refusal::plain())?;
        backend.master_made(&server, status)?;
    }
    let installed = backend.install(&server)?;
    let note = installed.fresh.then(|| {
        format!(
            "bateri: kablo is set up on {}; if the connection drops, this terminal keeps running",
            server.host
        )
    });
    let spec = new_session(shared, pane);
    Ok(Ready {
        server,
        installed,
        spec,
        note,
    })
}

// ─── the threads that carry bytes ────────────────────────────────────────

fn hold_reader(hold: HoldId, mut stream: UnixStream, tx: &Sender<Msg>) {
    let mut buffer = vec![0u8; CHUNK];
    loop {
        match stream.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                if tx.send(Msg::Hold(hold, buffer[..n].to_vec())).is_err() {
                    return;
                }
            }
        }
    }
    let _ = tx.send(Msg::HoldEof(hold));
}

fn hold_writer(mut stream: UnixStream, written: &Receiver<Out>) {
    let mut broken = false;
    while let Ok(out) = written.recv() {
        match out {
            Out::Bytes(bytes, queued) => {
                if !broken && stream.write_all(&bytes).is_err() {
                    broken = true;
                }
                queued.fetch_sub(bytes.len(), Ordering::SeqCst);
            }
            Out::Close => break,
        }
    }
    let _ = stream.shutdown(std::net::Shutdown::Both);
}

fn bridge_reader(
    account: String,
    generation: u64,
    mut reader: Box<dyn Read + Send>,
    tx: &Sender<Msg>,
    queued: &AtomicUsize,
) {
    let mut buffer = vec![0u8; CHUNK];
    loop {
        // The holders are slow to take what they were given: the stream waits
        // in the pipe, and the server waits for the pipe.
        while queued.load(Ordering::SeqCst) > HIGH_WATER {
            thread::sleep(Duration::from_millis(5));
        }
        match reader.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                let sent = tx.send(Msg::Bridge {
                    account: account.clone(),
                    generation,
                    bytes: buffer[..n].to_vec(),
                });
                if sent.is_err() {
                    return;
                }
            }
        }
    }
    let _ = tx.send(Msg::BridgeEof {
        account,
        generation,
    });
}

fn bridge_writer(mut writer: Box<dyn Write + Send>, written: &Receiver<Vec<u8>>) {
    while let Ok(bytes) = written.recv() {
        if writer
            .write_all(&bytes)
            .and_then(|()| writer.flush())
            .is_err()
        {
            return;
        }
    }
}

#[cfg(test)]
mod tests;
