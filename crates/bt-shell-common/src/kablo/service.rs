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
//! the holders stay and their terminals go on from the byte they were at. The pane
//! looks passive while it waits ([`PaneLink`]); a login the connection needs again
//! is tried with the Keychain's password and, failing that, is the user's to give.
//!
//! **What a pane shows, and ending it.** The application asks the service which
//! terminal a pane shows ([`Service::shown`]: the account, the server, the
//! program in front as the server last said) for the questions it has about a
//! pane that is not the local shell's — the one before closing it, the folder a
//! new pane opens in — and tells it that the user closed the pane
//! ([`Service::end`]), which ends the terminal on the server: `SIGHUP` at once,
//! and when the terminal is still there after a grace, the kill. A pane that goes
//! with the application says it only when the quit is one that ends the terminals
//! ([`Service::end_all`], `keep_running`'s table); a crash and an update never do,
//! because the pane's closing alone lets go of the terminal and it lives on.
//!
//! **What runs in the background.** The terminals the account keeps that no pane of this Mac shows
//! ([`Service::background`]) are the server's own list less the ones a holder is bound to
//! ([`crate::kablo::roster`]): a cache the events and a `list` renew, asked again when a connection
//! comes up, when a holder goes and when the application is about to show it
//! ([`Service::refresh_background`]), never on a clock.
//!
//! **What survives bateri.** A holder asks for a terminal it was shown
//! ([`wire::Target::Resume`]) by the account's name, and the bateri it asks may be
//! another process than the one that made the account (an update carried the panes
//! over, a crash left them with a holder, the Mac restarted): the account is made
//! again from the [`Ledger`], which wrote down the server, the way to reach it and the
//! terminals the panes show when they were made, and the connection is dialed as it
//! is after a loss. A terminal the ledger does not know (the file is gone) ends its
//! holder with a line. The ledger also remembers the terminals that were **asked to
//! end** until they are seen to have: a connection that is down cannot carry the
//! request, and a quit does not wait for a server that does not answer
//! ([`Service::end_all`]); the next connection to the account ends them first.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use bt_core::{PaneUuid, RemoteTarget};
use kablo_proto::v1::NewSession;

use super::hub::{Declined, HoldId, Hub, HubEvent};
use super::install::Installed;
use super::ledger::Ledger;
use super::roster::{Background, Roster, background_note, sort_recent_first};
use super::{Attach, Cause, Client, ErrorCode, Event, Link, SessionId, Size};
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
    /// The `ssh` arguments that reach the server, the program's name first: what a
    /// restored pane's pipe carries on its command line so that the pane's remote
    /// session can be read from it.
    pub argv: Vec<String>,
    /// The terminal's name on the server.
    pub terminal: SessionId,
    /// The program in front of the terminal as the server last said, controls
    /// removed; `None` while it has not said.
    pub program: Option<String>,
    /// How the connection to the server stands for this terminal.
    pub link: PaneLink,
}

/// How the connection to the server stands for the terminal a pane shows: what
/// the pane looks like while it waits for it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaneLink {
    /// The terminal is being brought up: nothing of it was shown yet, and what
    /// the pane shows is the user's own `ssh` still making the connection.
    /// **Not passive** — the first connection is not one that was lost.
    Connecting,
    /// The connection is there.
    Live,
    /// The terminal was shown and the connection is gone; the bridge is made
    /// again on its own clock, which the user can skip.
    Reconnecting,
    /// The terminal was shown and the server wants a login nobody has saved
    /// (a password, a second factor). Nothing is tried until the user logs in.
    NeedsLogin,
}

impl PaneLink {
    /// Whether the pane shows a screen that is no longer live: dimmed, with no
    /// caret and no keys to the terminal.
    pub fn is_passive(self) -> bool {
        matches!(self, Self::Reconnecting | Self::NeedsLogin)
    }

    /// What the pane is told: [`PaneLink::Connecting`] is the connection being there as far as
    /// the pane's looks go.
    fn told(self) -> PaneLink {
        match self {
            Self::Connecting => Self::Live,
            other => other,
        }
    }
}

impl Shown {
    /// Whether the terminal this is serves `target`, the remote session the pane's
    /// `ssh` is in: the same server as the user typed it. **The one rule of
    /// what bateri does on a server for a pane**: file drops and downloads,
    /// previews, the load indicator, the server's ports, the ⌘ check of a path
    /// and the connection of bateri's own those ride on are offered in
    /// a pane that shows a terminal a server keeps and in no other — an `ssh`
    /// that is a plain one (the setting is off for the host, a `production`
    /// host, a server kablo cannot be kept on, a call that runs a command) has
    /// none of them and costs the host no connection of bateri's. Every add-on
    /// asks it through `TerminalPane::server_target`, and nobody else decides.
    pub fn serves(&self, target: &RemoteTarget) -> bool {
        self.host == target.host
    }

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
    /// Why the stream ended, asked once when it has ended by itself:
    /// [`Cause::Dropped`] when nothing says more, [`Cause::LoginNeeded`] when
    /// the process was refused a login nobody could answer, [`Cause::Refused`]
    /// when the server's key is not the one that was trusted. It runs on the
    /// reader's thread, which may wait for what the process left to say.
    pub ended: Box<dyn FnOnce() -> Cause + Send>,
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

    /// The version of the kablo this computer brings (the package's), or `None`
    /// when it brings none. An account whose server was last known at another
    /// version is [`Backend::refresh`]ed before its bridge is made.
    fn version(&self) -> Option<String> {
        None
    }

    /// Puts the version this computer brings on a server that has had another
    /// (a ledger's account after bateri was updated) and has the terminals it
    /// runs renewed with it, without ending any; the answer is what is on the
    /// server now. Called on the thread that makes the bridge, **before** it is
    /// made: a bridge that runs is not replaced by the renewal. `None`: it could
    /// not be done now (no connection, a login nobody saved, a server that will
    /// not take the file) — the bridge is made from the binary the ledger knows,
    /// which is still there and serves the relays that run, and the next dial of
    /// the account tries again.
    fn refresh(&self, _server: &KabloHost) -> Option<Installed> {
        None
    }

    /// How this computer calls itself in the list of who is attached to a
    /// terminal.
    fn name(&self) -> Option<String> {
        None
    }

    /// How many rows of scrollback the server keeps for a new terminal.
    fn history(&self) -> Option<u32> {
        None
    }

    /// The connection to the server of the terminal `pane` shows went or came
    /// back: told once per **change** of what the pane looks like
    /// ([`PaneLink::is_passive`]), never per attempt — the clock that tries
    /// again, the wait and the wake from sleep are not news. Called on the
    /// service's thread; the application hops to its own.
    fn link_changed(&self, _pane: &PaneUuid, _link: PaneLink) {}

    /// The terminal `pane` shows appeared or went: the pane is a server pane
    /// from now on, or is no longer ([`Shown::serves`]), and what works on a
    /// server (file drops, the load indicator, the ports…) is to be offered in
    /// it or taken away. Told once per change, **before** the terminal is shown
    /// and whatever the link is — the probe that finds the pane's `ssh` in the
    /// process table and the holder that connects to the service race, and
    /// whichever is second must make the pane ask again; and a holder that is
    /// refused after it was admitted (the pane then runs the plain `ssh`, which
    /// the process table shows exactly as before) must not leave the server's
    /// work open in a pane that is a plain one. Called on the service's
    /// thread; the application hops to its own.
    fn terminal_changed(&self, _pane: &PaneUuid) {}

    /// The terminals the account's server keeps in the background
    /// ([`Service::background`]) are not the ones they were: one came, went, or says something
    /// else of itself. Told once per change of what [`Service::background`] returns for the
    /// account, which is not a change of the server's list — a terminal a pane starts to show leaves
    /// the background. Called on the service's thread; the application hops to its own.
    fn background_changed(&self, _account: &str) {}
}

/// Why a bridge could not be started, as far as the error says: a login the
/// bridge cannot answer is [`io::ErrorKind::PermissionDenied`].
fn cause_of(error: &io::Error) -> Cause {
    match error.kind() {
        io::ErrorKind::PermissionDenied => Cause::LoginNeeded,
        _ => Cause::Dropped,
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
        /// What the server has after the kablo there was renewed before the
        /// bridge was made ([`Backend::refresh`]); `None`: nothing was.
        refreshed: Option<Installed>,
    },
    Bridge {
        account: String,
        generation: u64,
        bytes: Vec<u8>,
    },
    /// The user closed the pane: the terminal it shows ends.
    End(PaneUuid),
    /// The user ended a terminal that no pane shows (the server's card).
    EndTerminal {
        account: String,
        terminal: SessionId,
    },
    /// A quit that ends the terminals: those the panes show are asked to end, and
    /// `done` is told when they have, or when none could be asked.
    EndAll {
        panes: Vec<PaneUuid>,
        done: Sender<()>,
    },
    BridgeEof {
        account: String,
        generation: u64,
        cause: Cause,
    },
    /// The user asked to try now (⏎), or logged in: the account dials as soon
    /// as it can.
    Retry(String),
    /// The computer woke up.
    Wake,
    /// Ask every account for its terminals ([`Service::refresh_background`]).
    Relist,
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
    /// The kablo the server was last known to have is not the version this
    /// computer brings: the next dial renews it first ([`Backend::refresh`]).
    stale: bool,
    /// Which bridge messages are current.
    generation: u64,
    bridge: Option<Bridge>,
    /// A thread is starting a bridge.
    dialing: bool,
    /// Bytes queued for the holders of this account's terminals.
    queued: Arc<AtomicUsize>,
    /// The terminals the ledger has marked to end were asked to, on the connection
    /// that is up (false again when it goes).
    purged: bool,
}

/// The running service. Dropping it stops it.
pub struct Service {
    socket: PathBuf,
    tx: Sender<Msg>,
    stop: Arc<AtomicBool>,
    threads: Mutex<Vec<thread::JoinHandle<()>>>,
    /// What each pane shows, as the loop last wrote it.
    shown: Arc<Mutex<HashMap<PaneUuid, Shown>>>,
    /// The terminals each account keeps in the background, as the loop last wrote them.
    background: Arc<Mutex<BTreeMap<String, Vec<Background>>>>,
}

/// How long a terminal that was asked to hang up has before it is killed.
pub const END_GRACE: Duration = Duration::from_secs(3);

/// How long a quit that ends the terminals waits for the servers to say they have
/// ([`Service::end_all`]): the hang-up and the answer cross the connection once
/// each, and a server that takes longer is ended at the next connection instead
/// (the ledger remembers). A design constant, in the order of the pane teardown's
/// own bound (`bt_core::SHUTDOWN_GRACE`, which runs after it).
pub const QUIT_END_WAIT: Duration = Duration::from_millis(1500);

/// The size a terminal that is attached only to be ended is attached at.
const END_SIZE: Size = Size {
    cols: 80,
    rows: 24,
    xpixel: 0,
    ypixel: 0,
};

impl Service {
    /// Binds `<dir>/hold` (a stale file is removed first) and starts serving.
    /// `dir` is the instance's directory, which must be a private one. `ledger` is
    /// where the terminals that have to outlive this process are written down
    /// ([`Ledger`]).
    pub fn start(
        dir: &Path,
        backend: Arc<dyn Backend>,
        ledger: Arc<Ledger>,
    ) -> io::Result<Service> {
        Service::start_with(dir, backend, ledger, END_GRACE)
    }

    /// [`Service::start`] with the time a terminal that was asked to end has
    /// before it is killed.
    pub fn start_with(
        dir: &Path,
        backend: Arc<dyn Backend>,
        ledger: Arc<Ledger>,
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
        let background = Arc::new(Mutex::new(BTreeMap::new()));
        let core = Core {
            shared: Arc::new(Shared {
                backend,
                tx: tx.clone(),
                ledger,
            }),
            hub: Hub::new(),
            conns: BTreeMap::new(),
            accounts: BTreeMap::new(),
            waiting: BTreeMap::new(),
            waiters: Vec::new(),
            panes: BTreeMap::new(),
            shown: Arc::clone(&shown),
            roster: BTreeMap::new(),
            relist: BTreeSet::new(),
            told: BTreeMap::new(),
            background: Arc::clone(&background),
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
            background,
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

    /// The terminals the server of `account` keeps for the user that no pane of this Mac shows,
    /// the most recently written first: running, not asked to end, and as the server last said —
    /// a cache that events and [`Service::refresh_background`] keep, read here without waiting
    /// for the server. An account the service has no bridge for has none.
    pub fn background(&self, account: &str) -> Vec<Background> {
        self.background
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(account)
            .cloned()
            .unwrap_or_default()
    }

    /// [`Service::background`] of every account, the most recently written first.
    pub fn background_all(&self) -> Vec<Background> {
        let mut all: Vec<Background> = self
            .background
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .values()
            .flatten()
            .cloned()
            .collect();
        sort_recent_first(&mut all);
        all
    }

    /// Asks every account whose bridge is up for its terminals again; the answer renews
    /// [`Service::background`] ([`Backend::background_changed`] says when it differs). For the
    /// moments somebody is about to look at the list (the palette, the card opening): the cache they
    /// read is what the last answers said, and nothing asks on a clock.
    pub fn refresh_background(&self) {
        let _ = self.tx.send(Msg::Relist);
    }

    /// The user closed the pane: the terminal it shows ends on the server
    /// (hung up now, killed after [`END_GRACE`] if it is still there). Say it
    /// **before** the pane's program is signalled — the holder's going is the
    /// terminal being let go of, and from then on the pane is not found. A
    /// pane that shows nothing, and a connection that is down, do nothing.
    pub fn end(&self, pane: &PaneUuid) {
        let _ = self.tx.send(Msg::End(pane.clone()));
    }

    /// The user ended a terminal no pane shows — one of the server's card: it is
    /// asked to hang up now and killed after [`END_GRACE`] if it is still there,
    /// as the one a closed pane showed is, and the list takes it out at once.
    /// A connection that is down cannot ask; the ledger keeps the request and the
    /// next connection to the account carries it out.
    pub fn end_terminal(&self, account: &str, terminal: &SessionId) {
        let _ = self.tx.send(Msg::EndTerminal {
            account: account.to_owned(),
            terminal: terminal.clone(),
        });
    }

    /// A quit that ends the terminals: every terminal the panes show is asked to
    /// end (a hang-up; the ledger remembers the request until it is seen to have
    /// been carried out, so a server that cannot be reached ends it at the next
    /// connection), and this waits for them to have — at most `wait`, so a server
    /// that does not answer does not hold the quit. A pane that shows nothing, and a
    /// connection that is down, are not waited for. Say it **before** the panes'
    /// programs are signalled, as [`Service::end`] must be said.
    pub fn end_all(&self, panes: &[PaneUuid], wait: Duration) {
        let (done, finished) = mpsc::channel();
        let message = Msg::EndAll {
            panes: panes.to_vec(),
            done,
        };
        if self.tx.send(message).is_ok() {
            let _ = finished.recv_timeout(wait);
        }
    }

    /// Try now: the account of the terminal `pane` shows dials at once instead
    /// of waiting out its pause. What ⏎ does on a pane that waits for the
    /// connection. A pane that shows nothing does nothing.
    pub fn retry(&self, pane: &PaneUuid) {
        if let Some(shown) = self.shown(pane) {
            self.retry_account(&shown.account);
        }
    }

    /// [`Service::retry`] for an account: the user logged in (the master
    /// connection is there now) and every terminal on the server comes back.
    pub fn retry_account(&self, account: &str) {
        let _ = self.tx.send(Msg::Retry(account.to_owned()));
    }

    /// The computer woke up: a connection that looked open is asked whether it
    /// is, and the ones waiting for their pause dial now.
    pub fn wake(&self) {
        let _ = self.tx.send(Msg::Wake);
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
    ledger: Arc<Ledger>,
}

/// Everything the loop owns.
struct Core {
    shared: Arc<Shared>,
    hub: Hub,
    conns: BTreeMap<HoldId, Conn>,
    accounts: BTreeMap<String, Account>,
    /// A preparation waits for the holder to run a program.
    waiting: BTreeMap<HoldId, Sender<u8>>,
    /// Quits waiting for terminals to have ended ([`Service::end_all`]).
    waiters: Vec<Waiter>,
    /// The pane each holder runs in, once it was admitted.
    panes: BTreeMap<HoldId, PaneUuid>,
    /// What [`Service::shown`] reads.
    shown: Arc<Mutex<HashMap<PaneUuid, Shown>>>,
    /// What each account's server last said of its terminals.
    roster: BTreeMap<String, Roster>,
    /// The accounts to ask for their terminals again before anything else is done.
    relist: BTreeSet<String>,
    /// The terminals in the background the last line to a new terminal named, by account
    /// ([`background_note`]): for as long as the service lives, which is the application's.
    told: BTreeMap<String, Vec<SessionId>>,
    /// What [`Service::background`] reads.
    background: Arc<Mutex<BTreeMap<String, Vec<Background>>>>,
    /// Something happened that [`Core::shown`] may say differently.
    refresh: bool,
    /// Terminals asked to hang up that are killed if they are still there.
    ending: Vec<Ending>,
    grace: Duration,
    rx: Receiver<Msg>,
    generation: u64,
}

/// A quit that waits for the terminals it asked to end.
struct Waiter {
    pending: BTreeSet<(String, SessionId)>,
    done: Sender<()>,
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
            Msg::EndTerminal { account, terminal } => self.end_terminal(&account, &terminal, now),
            Msg::EndAll { panes, done } => self.end_all(&panes, done, now),
            Msg::Dialed {
                account,
                outcome,
                refreshed,
            } => self.dialed(&account, outcome, refreshed, now),
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
                cause,
            } => {
                if let Some(entry) = self.accounts.get_mut(&account)
                    && entry.generation == generation
                    && entry.bridge.take().is_some()
                {
                    // The next connection asks for the terminals to be ended again.
                    entry.purged = false;
                    if let Some(client) = self.hub.client_mut(&account)
                        && matches!(client.link(), Link::Opening { .. } | Link::Live)
                    {
                        client.lost(now, cause);
                    }
                }
            }
            Msg::Retry(account) => {
                if let Some(client) = self.hub.client_mut(&account) {
                    client.retry_now(now);
                }
                // A login that was waited for is a connection being made.
                self.refresh = true;
            }
            Msg::Wake => {
                let accounts: Vec<String> = self.accounts.keys().cloned().collect();
                for account in accounts {
                    if let Some(client) = self.hub.client_mut(&account) {
                        client.wake(now);
                    }
                }
                self.refresh = true;
            }
            Msg::Relist => self.relist.extend(self.accounts.keys().cloned()),
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
            for account in std::mem::take(&mut self.relist) {
                if let Some(client) = self.hub.client_mut(&account) {
                    client.list();
                }
            }
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
                        // A holder that goes lets go of its terminal, which lives on unless
                        // it was asked to end: the server is asked what it has now.
                        self.relist.extend(self.accounts.keys().cloned());
                    }
                    HubEvent::Client { account, event } => {
                        self.roster
                            .entry(account.clone())
                            .or_default()
                            .apply(&event);
                        match *event {
                            // A connection that came up is asked for its terminals.
                            Event::Linked => {
                                self.relist.insert(account);
                            }
                            Event::Ended { terminal, .. } | Event::Taken { terminal, .. } => {
                                // It went by itself: nothing is left to kill.
                                self.ending
                                    .retain(|e| !(e.account == account && e.terminal == terminal));
                                self.finished(&account, &terminal);
                            }
                            // The server does not have it: nothing is left to end either.
                            Event::Failed {
                                terminal: Some(terminal),
                                code: ErrorCode::UNKNOWN_SESSION,
                                ..
                            } => {
                                self.ending
                                    .retain(|e| !(e.account == account && e.terminal == terminal));
                                self.finished(&account, &terminal);
                            }
                            _ => {}
                        }
                    }
                }
                self.refresh = true;
            }
            self.over(&accounts);
            self.purge(now);
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
            let (host, argv) = self
                .accounts
                .get(account)
                .map(|entry| (entry.server.host.clone(), entry.server.argv.clone()))
                .unwrap_or_default();
            // Passive is "was shown and is not live any more": until the
            // terminal was shown the connection being made is the first one.
            let link = match self.hub.client(account).map(Client::link) {
                _ if !self.hub.is_shown(*hold) => PaneLink::Connecting,
                Some(Link::NeedsLogin) => PaneLink::NeedsLogin,
                Some(Link::Retrying { .. } | Link::Opening { .. }) => PaneLink::Reconnecting,
                // Live, or over: the holder is about to end and the pane with
                // the terminal it shows has nothing to wait for.
                Some(Link::Live | Link::Over(_)) | None => PaneLink::Live,
            };
            shown.insert(
                pane.clone(),
                Shown {
                    account: account.to_owned(),
                    host,
                    argv,
                    terminal: terminal.clone(),
                    program,
                    link,
                },
            );
        }
        let before = std::mem::replace(
            &mut *self.shown.lock().unwrap_or_else(|e| e.into_inner()),
            shown.clone(),
        );
        // Told once per change of what the pane looks like, after the lock is
        // let go of. A pane that is gone is a pane that is no longer waiting.
        let was = |pane: &PaneUuid| before.get(pane).map_or(PaneLink::Live, |s| s.link.told());
        for (pane, now) in &shown {
            if was(pane) != now.link.told() {
                self.shared.backend.link_changed(pane, now.link.told());
            }
        }
        for pane in before.keys().filter(|pane| !shown.contains_key(*pane)) {
            if was(pane) != PaneLink::Live {
                self.shared.backend.link_changed(pane, PaneLink::Live);
            }
        }
        let came = shown.keys().filter(|pane| !before.contains_key(*pane));
        let went = before.keys().filter(|pane| !shown.contains_key(*pane));
        for pane in came.chain(went) {
            self.shared.backend.terminal_changed(pane);
        }
        self.write_background();
    }

    /// What the accounts keep in the background, written where [`Service::background`] reads
    /// it: what each server last said of its terminals, less those a holder is bound to (a pane
    /// that shows it, or is about to — a terminal still being brought up is not in the
    /// background for a moment) and those asked to end.
    fn write_background(&mut self) {
        let mut background: BTreeMap<String, Vec<Background>> = BTreeMap::new();
        for (account, roster) in &self.roster {
            let Some(entry) = self.accounts.get(account) else {
                continue;
            };
            let ending = self.shared.ledger.ending(account);
            let hidden = |terminal: &SessionId| {
                self.hub.hold_of(account, terminal).is_some()
                    || ending.contains(terminal)
                    || self
                        .ending
                        .iter()
                        .any(|e| e.account == *account && e.terminal == *terminal)
            };
            let found = roster.background(account, &entry.server.host, &entry.server.argv, hidden);
            if !found.is_empty() {
                background.insert(account.clone(), found);
            }
        }
        let before = std::mem::replace(
            &mut *self.background.lock().unwrap_or_else(|e| e.into_inner()),
            background.clone(),
        );
        // Told after the lock is let go of, once per account whose list is not what it was.
        let accounts: BTreeSet<&String> = before.keys().chain(background.keys()).collect();
        for account in accounts {
            if before.get(account) != background.get(account) {
                self.shared.backend.background_changed(account);
            }
        }
    }

    /// The terminals the panes `pane` owns show: the account and the name.
    fn terminals_of(&self, pane: &PaneUuid) -> Vec<(String, SessionId)> {
        self.panes
            .iter()
            .filter(|(_, owner)| *owner == pane)
            .filter_map(|(hold, _)| self.hub.terminal_of(*hold))
            .map(|(account, terminal)| (account.to_owned(), terminal.clone()))
            .collect()
    }

    /// The user closed `pane`: the terminal it shows is asked to hang up, and
    /// killed after the grace if it is still there. A connection that is down
    /// cannot ask; the ledger keeps the request and the next connection to the
    /// account carries it out.
    fn end(&mut self, pane: &PaneUuid, now: Instant) {
        for (account, terminal) in self.terminals_of(pane) {
            self.shared.ledger.end(&account, &terminal);
            self.finish_off(&account, &terminal, now);
        }
    }

    /// A terminal no pane shows is ended ([`Service::end_terminal`]): the ledger
    /// keeps the request until it is seen to be carried out, and the list stops
    /// offering it at once — an ending terminal is not in the background.
    fn end_terminal(&mut self, account: &str, terminal: &SessionId, now: Instant) {
        self.shared.ledger.end(account, terminal);
        self.finish_off(account, terminal, now);
        self.write_background();
    }

    /// A quit that ends the terminals ([`Service::end_all`]).
    fn end_all(&mut self, panes: &[PaneUuid], done: Sender<()>, now: Instant) {
        let mut pending = BTreeSet::new();
        for pane in panes {
            for (account, terminal) in self.terminals_of(pane) {
                self.shared.ledger.end(&account, &terminal);
                if self.finish_off(&account, &terminal, now) {
                    pending.insert((account, terminal));
                }
            }
        }
        if pending.is_empty() {
            let _ = done.send(());
        } else {
            self.waiters.push(Waiter { pending, done });
        }
    }

    /// Asks the terminal to hang up, and to be killed after the grace if it is
    /// still there. A terminal this client is not attached to (one a bateri before
    /// this one left on the server) is attached first, so that the client is told
    /// when it ended. `false` when the connection is not live: nothing was asked.
    fn finish_off(&mut self, account: &str, terminal: &SessionId, now: Instant) -> bool {
        let shown = self.hub.hold_of(account, terminal).is_some();
        let Some(client) = self.hub.client_mut(account) else {
            return false;
        };
        if !client.link().is_live() {
            return false;
        }
        if !shown && !client.is_attached(terminal) {
            client.attach(terminal.clone(), Attach::new(END_SIZE).from_now());
        }
        let asked = client.terminate(terminal, false);
        if asked
            && !self
                .ending
                .iter()
                .any(|e| e.account == account && e.terminal == *terminal)
        {
            self.ending.push(Ending {
                account: account.to_owned(),
                terminal: terminal.clone(),
                at: now + self.grace,
            });
        }
        asked
    }

    /// Every connection that came up asks for the terminals the ledger has marked
    /// to end to be ended, once, before anything else is asked of it.
    fn purge(&mut self, now: Instant) {
        let accounts: Vec<String> = self.accounts.keys().cloned().collect();
        for account in accounts {
            let live = self
                .hub
                .client(&account)
                .is_some_and(|client| client.link().is_live());
            let Some(entry) = self.accounts.get_mut(&account) else {
                continue;
            };
            if !live {
                entry.purged = false;
                continue;
            }
            if std::mem::replace(&mut entry.purged, true) {
                continue;
            }
            for terminal in self.shared.ledger.ending(&account) {
                self.finish_off(&account, &terminal, now);
            }
        }
    }

    /// The terminal is not there any more, whoever ended it: the ledger forgets
    /// it and the quits that waited for it are one terminal closer.
    fn finished(&mut self, account: &str, terminal: &SessionId) {
        self.shared.ledger.gone(account, terminal);
        let key = (account.to_owned(), terminal.clone());
        for waiter in &mut self.waiters {
            waiter.pending.remove(&key);
            if waiter.pending.is_empty() {
                let _ = waiter.done.send(());
            }
        }
        self.waiters.retain(|waiter| !waiter.pending.is_empty());
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
            self.roster.remove(account);
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
            let stale = entry.stale;
            let shared = Arc::clone(&self.shared);
            let started = thread::Builder::new()
                .name("ssh-hold dials".into())
                .spawn({
                    let account = account.clone();
                    move || {
                        // The terminals are renewed before any bridge exists:
                        // the renewal does not replace one that runs.
                        let refreshed = if stale {
                            shared.backend.refresh(&server)
                        } else {
                            None
                        };
                        let outcome = shared
                            .backend
                            .carry(&server, refreshed.as_ref().unwrap_or(&installed));
                        let _ = shared.tx.send(Msg::Dialed {
                            account,
                            outcome,
                            refreshed,
                        });
                    }
                })
                .is_ok();
            if !started {
                entry.dialing = false;
            }
        }
    }

    fn dialed(
        &mut self,
        account: &str,
        outcome: io::Result<Carried>,
        refreshed: Option<Installed>,
        now: Instant,
    ) {
        let Some(entry) = self.accounts.get_mut(account) else {
            // The account is gone; what was started is ended with it.
            if let Ok(carried) = outcome {
                (carried.close)();
            }
            return;
        };
        entry.dialing = false;
        // The kablo on the server was renewed, whatever became of the bridge after:
        // the account is known at the new version from now on, in the ledger too.
        if let Some(installed) = refreshed {
            entry.installed = Installed {
                fresh: false,
                ..installed
            };
            entry.stale = false;
            self.shared.ledger.remember(&entry.server, &entry.installed);
        }
        match outcome {
            Ok(carried) => {
                self.generation += 1;
                entry.generation = self.generation;
                entry.purged = false;
                let generation = self.generation;
                let Carried {
                    reader,
                    writer,
                    close,
                    ended,
                } = carried;
                let (to_bridge, written) = mpsc::channel();
                let tx = self.shared.tx.clone();
                let queued = Arc::clone(&entry.queued);
                let name = account.to_owned();
                let _ = thread::Builder::new()
                    .name("kablo bridge reads".into())
                    .spawn(move || bridge_reader(name, generation, reader, &tx, &queued, ended));
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
            Err(error) => {
                if let Some(client) = self.hub.client_mut(account) {
                    client.lost(now, cause_of(&error));
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
                if self.shared.ledger.ending(account).contains(&open.terminal) {
                    self.hub.refuse(
                        hold,
                        false,
                        "bateri: this terminal was ended with the bateri that showed it",
                    );
                    return;
                }
                // A terminal is taken up again by the pane it was shown in and by no
                // other: the names that ask for it (`--terminal`, `--account`) are in
                // every process list, and a process of another pane that said them would
                // be shown another pane's server screen.
                if let (Some(owner), Some(tab)) = (
                    self.shared.ledger.pane_of(account, &open.terminal),
                    pane.tab.as_ref(),
                ) && &owner != tab
                {
                    self.hub.refuse(
                        hold,
                        false,
                        "bateri: this terminal is shown by another pane",
                    );
                    return;
                }
                // An account this process has no client for is one a bateri before it
                // made (an update, a crash, a restart of the Mac): the ledger knows
                // the server, and the connection is made again as it is after a
                // loss — with the Keychain's password, or the user's login.
                if !self.accounts.contains_key(account) {
                    self.recall(account);
                }
                if self.accounts.contains_key(account) && self.hub.resume(hold, account).is_ok() {
                    // An account made in this process before the ledger had it (the file
                    // was lost) is written down again, so that the next restart can show it.
                    if self.shared.ledger.known(account).is_none()
                        && let Some(entry) = self.accounts.get(account)
                    {
                        self.shared.ledger.remember(&entry.server, &entry.installed);
                    }
                    self.shared
                        .ledger
                        .shown(account, &open.terminal, pane.tab.as_ref());
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

    /// The client of an account the ledger remembers, as it was before this
    /// process: started to dial at once. Nothing happens for an account it does not
    /// know.
    fn recall(&mut self, account: &str) {
        let Some(known) = self.shared.ledger.known(account) else {
            return;
        };
        self.make_account(known.server(), known.installed(), Instant::now());
    }

    /// A client for the server and the account it stands for.
    fn make_account(&mut self, server: KabloHost, installed: Installed, now: Instant) {
        let key = server.key.clone();
        let name = self.shared.backend.name();
        let stale = self
            .shared
            .backend
            .version()
            .is_some_and(|version| version != installed.version);
        self.hub.add_client(&key, Client::new(name.as_deref(), now));
        self.accounts.insert(
            key,
            Account {
                server,
                installed,
                stale,
                generation: 0,
                bridge: None,
                dialing: false,
                queued: Arc::new(AtomicUsize::new(0)),
                purged: false,
            },
        );
    }

    /// Starts the terminal a holder asked for and writes it in the ledger, with
    /// the pane that shows it: from the moment of the request, since a crash between
    /// it and the server's answer must not leave a terminal nobody can name.
    fn started(&mut self, hold: HoldId, account: &str, spec: NewSession) -> Result<(), Declined> {
        self.hub.start(hold, account, spec)?;
        if let Some((_, terminal)) = self.hub.terminal_of(hold) {
            let terminal = terminal.clone();
            self.shared
                .ledger
                .shown(account, &terminal, self.panes.get(&hold));
        }
        self.tell_background(hold, account);
        Ok(())
    }

    /// A new terminal on a server that keeps others nobody looks at says so, once for these
    /// terminals ([`background_note`]): in front of what the terminal writes, where the pane
    /// reads it. The list is what is known now — an account that was made a moment ago has not
    /// heard of its terminals yet and says nothing, which is the quiet side.
    fn tell_background(&mut self, hold: HoldId, account: &str) {
        let Some(host) = self.accounts.get(account).map(|a| a.server.host.clone()) else {
            return;
        };
        let waiting = self
            .background
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(account)
            .cloned()
            .unwrap_or_default();
        let Some((text, named)) =
            background_note(&host, &waiting, self.told.get(account).map(Vec::as_slice))
        else {
            return;
        };
        if self.hub.say(hold, &text) {
            self.told.insert(account.to_owned(), named);
        }
    }

    /// The first terminal of an account is prepared on a thread of its own;
    /// one that rides a bridge that is there starts at once.
    fn prepare(&mut self, hold: HoldId, server: KabloHost, pane: PaneInfo) {
        if let Some(entry) = self.accounts.get(&server.key) {
            let spec = new_session(&self.shared, &pane);
            let account = entry.server.key.clone();
            if self.started(hold, &account, spec).is_err() {
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
            self.shared.ledger.remember(&ready.server, &ready.installed);
            self.make_account(ready.server.clone(), ready.installed.clone(), now);
        }
        if let Some(note) = &ready.note {
            self.hub.say(hold, note);
        }
        if self.started(hold, &account, ready.spec).is_err() {
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
    ended: Box<dyn FnOnce() -> Cause + Send>,
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
    let cause = ended();
    let _ = tx.send(Msg::BridgeEof {
        account,
        generation,
        cause,
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
