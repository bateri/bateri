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
//! **What a terminal is called.** A holder asks for a new terminal with a name of its own (`b-` and
//! 128 random bits, so that asking again cannot make a second); the service starts it under the
//! next number of the server's counter instead (`p7`, [`Backend::number`]: the very file the agent's
//! command on the server numbers its terminals from), writes that name in the ledger before the
//! request goes out and tells the holder when the server has made the terminal
//! ([`crate::hold::wire::ToHold::Bound`]), which asks for the terminal by it from then on. A
//! holder that cannot be told (wire version 2 and before), a server that cannot count and a
//! backend with no counter leave the name the holder made.
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
//!
//! **What the programs on a server say to the app.** A backend that takes mail
//! ([`Backend::takes_mail`]) has its accounts' clients ask for tickets
//! ([`crate::kablo::mail`]): a program that runs `kablo app` on the server (or on this Mac, whose
//! account is asked the same) gets a
//! conversation with the application, and the service takes it at once (the program
//! waits ten seconds for that), keeps what the program says until it has said all it
//! had, and hands the whole of it to [`Backend::mail`] as a [`Letter`] with the way to
//! answer it. The answer is the application's own time: a [`Letter`] that is answered
//! sends the bytes and ends the conversation; one that is dropped hangs up, so a program
//! is never left waiting on an application that forgot it. What a letter says and what the
//! answer means is between the program and the application; the service reads none of it.
//!
//! **The panes of this Mac.** A pane that shows a terminal of this Mac's own account
//! ([`super::local`]) has no `ssh-hold` between it and the terminal: the pane's session reads the
//! terminal's bytes from a socket the service writes ([`Service::submit_local`]), types into the
//! same socket, and the hub holds it as a raw holder ([`Hub::open_raw`]). Everything else is a
//! server pane's — the account's client, the ledger, the ending of the terminal — and it is
//! **not** a server pane: it is not in [`Service::shown`] and not in the background lists, which
//! are a server's.
//!
//! **What survives bateri, for the panes of this Mac.** The terminal outlives the pane's
//! session like a server's: the stream closing detaches it, and the ending of it is a request —
//! the user's close ([`Service::end`]), a quit that ends the programs ([`Service::end_all`]), and
//! the launch after a crash under `keep_running = "update"` (the ledger marks it to end). The
//! ledger has it with its pane, and the bateri that comes next, asked for a terminal by that pane,
//! finds it again ([`Service::submit_local`]): attached from the first byte the server has, the
//! stream starts with the terminal's past ([`bt_core::Silence`]) — the bytes the server answered
//! the questions of, which the pane's session reads without doing again what they asked — and
//! the process id comes from the server's list. A terminal the server does not have is made again
//! for the pane. What no pane showed again by the time this Mac's account is there is not ended:
//! the application shows it again in a pane of its own ([`Service::when_local`],
//! [`crate::kablo::ledger::Ledger::unshown_local`]).
//!
//! **What they left while the application was away.** A program that asked when no application
//! was connected left its request on the server. When a connection to the server comes up and the
//! server has said what it keeps ([`Service::background`], [`Service::exited`]), the backend is
//! told once ([`Backend::reachable`], on a thread of its own) and reads what was left.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use bt_core::{PaneUuid, RemoteTarget, StreamExit};
use kablo_proto::v1::stream::Command;
use kablo_proto::v1::{NewSession, Status};

use super::hub::{Declined, HoldId, Hub, HubEvent};
use super::install::{Installed, Root, Watch};
use super::ledger::Ledger;
use super::local;
use super::remote::{RemoteCommand, RemoteStream};
use super::roster::{Background, Roster, background_note, sort_recent_first};
use super::{Attach, Cause, Client, ErrorCode, Event, Exit, Link, SessionId, Size, Talk};
use crate::agent::Terminal;
use crate::agent::notes::Record;
use crate::hold::HOLD_SOCKET;
use crate::hold::wire::{self, CHUNK, Open, Target};
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
    /// The words of the program the pane was opened for, which the terminal runs instead of a
    /// shell ([`crate::open_with::argv_server`]); [`PaneInfo::directory`] is then the folder it
    /// starts in. The pane says it, never the holder: it is made by the app when the pane is, and
    /// it is no part of what the holder asks for.
    pub program: Option<Vec<String>>,
    /// The pane has a dock to put the first connection's row in. A pane opened for a program
    /// reserves none: a person is not asked there, and nothing of it is shown.
    pub rows: bool,
}

impl PaneInfo {
    /// Whether the first connection's row can be shown to this pane's person: the pane says which
    /// it is, and has the dock for it.
    pub fn can_show_row(&self) -> bool {
        self.tab.is_some() && self.rows
    }
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
    /// [`Shown::program`] was said since the terminal was last attached. A word from before a
    /// connection that was lost is good enough to show and not to write on: the program may have
    /// ended while the connection was gone.
    pub program_fresh: bool,
    /// How the connection to the server stands for this terminal.
    pub link: PaneLink,
    /// While the link is [`PaneLink::Taken`], the name the computer that took the terminal gave
    /// itself; `None` when it gave none.
    pub taken_by: Option<String>,
}

/// How the connection to the server stands for the terminal a pane shows: what
/// the pane looks like while it waits for it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaneLink {
    /// The terminal is being brought up: nothing of it was shown yet, and what
    /// the pane shows is the user's own `ssh` still making the connection.
    /// **Not passive** — the first connection is not one that was lost.
    Connecting,
    /// The connection is there and the terminal is on it: what is written to it arrives.
    Live,
    /// The terminal was shown and is not on a connection: the connection is gone, and the bridge
    /// is made again on its own clock, which the user can skip; or it is back and the server has
    /// not served the terminal yet ([`crate::kablo::Client::is_coming_back`]).
    Reconnecting,
    /// The terminal was shown and the server wants a login nobody has saved
    /// (a password, a second factor). Nothing is tried until the user logs in.
    NeedsLogin,
    /// The terminal was shown and another computer took it ([`Shown::taken_by`]): the pane keeps
    /// its last screen, dimmed, and ⏎ takes the terminal back. Whatever the connection does, this
    /// stays — the terminal is not this Mac's to wait for.
    Taken,
}

impl PaneLink {
    /// Whether the pane shows a screen that is no longer live: dimmed, with no
    /// caret and no keys to the terminal.
    pub fn is_passive(self) -> bool {
        matches!(self, Self::Reconnecting | Self::NeedsLogin | Self::Taken)
    }

    /// Whether a pane that stood at `self` and is told `now` has its terminal's screen **back**:
    /// it was a screen that was no longer live and is live again (the connection came back, or
    /// the user took the terminal back). The one edge at which a program's view over the pane is
    /// started again: what came back was the screen, from a picture or from a replay, and not
    /// the messages the view lives on (`Session::restart_view`). A first connection is not one
    /// ([`PaneLink::Connecting`] was never passive), and neither is any change between two
    /// passive states.
    pub fn comes_back(self, now: PaneLink) -> bool {
        self.is_passive() && now == Self::Live
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
    /// Whether the terminal is one of this Mac's own ([`super::local`]) and not a server's. **The
    /// one place that says which kind a pane's terminal is** (the account's name decides, by
    /// [`local::is_account`], the one rule of which kind an account is): everything bateri does
    /// for a server — its "on {host}" in the lines the user reads, the connection that is lost
    /// and comes back, the server's card and list, the load, the files and the ports — asks this
    /// and does not guess from the host or the arguments, which a terminal of this Mac does not
    /// have.
    pub fn is_local(&self) -> bool {
        local::is_account(&self.account)
    }

    /// Whether the terminal this is serves `target`, the remote session the pane's
    /// `ssh` is in: the same server as the user typed it. **The one rule of
    /// what bateri does on a server for a pane**: file drops and downloads,
    /// the load indicator, the server's ports, the ⌘ check of a path
    /// and the connection of bateri's own those ride on are offered in
    /// a pane that shows a terminal a server keeps and in no other — an `ssh`
    /// that is a plain one (the setting is off for the host, a `production`
    /// host, a server kablo cannot be kept on, a call that runs a command) has
    /// none of them and costs the host no connection of bateri's. Every add-on
    /// asks it through `TerminalPane::server_target`, and nobody else decides.
    pub fn serves(&self, target: &RemoteTarget) -> bool {
        self.host == target.host
    }

    /// The server this terminal is on, as what asks for a program or a connection on it takes
    /// it ([`Backend::command`]). `port` is the one the pane's `ssh` names. The `ssh -G` answer is
    /// not kept here, and nothing reads it afterwards.
    pub fn server(&self, port: Option<u16>) -> KabloHost {
        KabloHost {
            host: self.host.clone(),
            port,
            argv: self.argv.clone(),
            config: String::new(),
            key: self.account.clone(),
        }
    }

    /// What the pane's icon draws for the program in front of the terminal, by the
    /// name the server gave it and the table of a local program's
    /// ([`crate::program::identity::from_server`]): the same kind the same program
    /// has in a pane on this Mac. `None` while the server has not said, and for a
    /// name nothing here knows — the pane keeps the server's icon.
    pub fn kind(&self) -> Option<crate::program::identity::Kind> {
        self.program
            .as_deref()
            .and_then(crate::program::identity::from_server)
    }

    /// Whether the server has said that the shell itself is in front of the terminal
    /// (`-zsh`, `bash`, `fish`), since it was last attached. `false` while it has not said, and
    /// for a program: a byte meant for a shell's line editor is a key to anything else. The pane
    /// asks before it speaks to the shell on its own (`Session::refresh_dock`), because what
    /// the pane remembers of the terminal is a picture from before the connection went.
    pub fn shell_in_front(&self) -> bool {
        self.program_fresh
            && self
                .program
                .as_deref()
                .is_some_and(crate::program::is_shell_name)
    }

    /// What a question about closing the pane calls the program that would
    /// end: the program in front with the server it runs on (`npm on rasp`), or
    /// `None` when the shell is in front (waiting for a command) or the server
    /// has not said what is.
    pub fn running(&self) -> Option<String> {
        // A terminal another computer took is not running here: closing the pane ends nothing.
        if self.link == PaneLink::Taken {
            return None;
        }
        let program = self.program.as_deref().filter(|name| !name.is_empty())?;
        (!crate::program::is_shell_name(program)).then(|| format!("{program} on {}", self.host))
    }
}

/// The line the pane of a program ends with when it is given no terminal on `server`, named as
/// the person wrote it: all the person is told, since the pane has no plain `ssh` that would
/// say what is wrong. `given_up`: the server is not reached ([`Core::gives_up`]); else it is
/// there, and the terminal could not be started on it.
fn not_started_line(server: &str, given_up: bool) -> String {
    if given_up {
        format!("bateri: could not connect to {server}")
    } else {
        format!("bateri: could not start the program on {server}")
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
    /// A dim line for the pane's terminal before the plain `ssh`, told once per server (its
    /// account) while bateri runs: `(account, line)`.
    pub note: Option<(String, String)>,
}

impl Refusal {
    /// The plain `ssh`.
    pub fn plain() -> Refusal {
        Refusal {
            fall_back: true,
            text: String::new(),
            note: None,
        }
    }

    /// The plain `ssh` of a pane that has no dock to ask in, with the line that says what the
    /// server could have ([`Service::prepared`] tells it once per server).
    pub fn unasked(server: &KabloHost) -> Refusal {
        let host = bt_core::bare_host(&server.host);
        Refusal {
            note: Some((
                server.key.clone(),
                format!(
                    "bateri can keep terminals on {host} running: Shell \u{25b8} Set Up bateri on \u{201c}{host}\u{201d}."
                ),
            )),
            ..Refusal::plain()
        }
    }

    /// An end with a line to read.
    pub fn ended(text: impl Into<String>) -> Refusal {
        Refusal {
            fall_back: false,
            text: text.into(),
            note: None,
        }
    }
}

/// Whether bateri may put its files on a server, and how that is found out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Consent {
    /// It may: the setting says to go ahead (`auto`, or the host's own entry), or bateri has been
    /// there before, or the machine is this Mac.
    Granted,
    /// The person is asked, in the pane's dock ([`Setup::Asking`]), once the login is made — the
    /// login is ssh's own, in the pane, and the question follows it. Nothing has been written to
    /// the server, and nothing is until the answer is in ([`Answer`]).
    Ask,
    /// It may not: the setting says `off`, or nothing can ask. The plain `ssh`, and no connection
    /// is made for it.
    Refused,
}

/// Where a server's first connection is, as the dock row of the pane says it: the question, the
/// wait while bateri goes up, the end. Told by [`Backend::setup_changed`], once per change, on the
/// service's thread.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Setup {
    /// The person is asked whether to set the server up (*Plain ssh* / *Set Up*), and the
    /// service waits for [`Service::answer_setup`].
    Asking,
    /// bateri goes up: `sent` of `total` bytes of its binary are with the connection (`total` is
    /// `0` while it is not yet known), and *Cancel* is [`Service::answer_setup`] with
    /// [`Answer::Cancel`].
    Putting { sent: u64, total: u64 },
    /// bateri is on the server: the row says so for a moment, then the server's own dock takes
    /// its place.
    Done,
    /// The row is not wanted any more: the answer was *Plain ssh*, the install was stopped or
    /// failed, or the holder went. The pane is what the plain `ssh` makes it.
    Gone,
}

/// What the person said to the first connection's row ([`Service::answer_setup`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Answer {
    /// *Set Up*: put bateri on the server.
    SetUp,
    /// *Plain ssh*, asked: not on this server, which is kept for the host (the setting is `off`
    /// for it from now on), and the connection goes on as the plain `ssh` it stands in for.
    Plain,
    /// *Cancel*, while bateri goes up: stop, keep nothing; the connection goes on as the plain
    /// `ssh`. While the question is up it is the same as [`Answer::Plain`] without the record.
    Cancel,
    /// The pane has no row to ask in (no dock, no window): nothing was asked, nothing is kept,
    /// and the plain `ssh` it is.
    Unshown,
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
    /// when the server's key is not the one that was trusted, [`Cause::Missing`]
    /// when the far shell could not run kablo. It runs on the reader's thread,
    /// which may wait for what the process left to say.
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

    /// Whether bateri may put its files on the server, asked before anything is: the first step
    /// of a terminal's preparation, so that a `Refused` opens no connection at all. The question
    /// of a [`Consent::Ask`] is asked after the login, in the pane's dock. The default is yes, for
    /// a backend with nothing to ask.
    fn consent(&self, _server: &KabloHost) -> Consent {
        Consent::Granted
    }

    /// Told what the person answered to the question of [`Consent::Ask`], to keep it: a *Plain
    /// ssh* is a "no" for this host. A *Set Up* is kept by the install that follows, which
    /// records the server before it writes anything.
    fn answered(&self, _server: &KabloHost, _answer: Answer) {}

    /// The first connection's row of the pane `pane`, about the server `host` (as typed), is as
    /// `setup` says ([`Setup`]): the question is to be put up and waited for, the bytes are going,
    /// bateri is there, or the row is to go. Called on the service's thread, in order; the
    /// application hops to its own. A pane that cannot show the row says so at once:
    /// [`Service::answer_setup`] with [`Answer::Unshown`].
    fn setup_changed(&self, _pane: &PaneUuid, _host: &str, _setup: Setup) {}

    /// [`Backend::install`] for a person who watches bateri go up ([`Watch`]): told how many bytes
    /// are with the connection, and able to stop it — the install then fails as a transient one
    /// does, and nothing that is not whole is left on the server. The default watches nothing.
    fn install_watched(
        &self,
        server: &KabloHost,
        live: &[SessionId],
        _watch: &dyn Watch,
    ) -> Result<Installed, Refusal> {
        self.install(server, live)
    }

    /// The program that makes sure a master connection to the server exists, to
    /// be run on the pane's terminal (where it may ask for a password and for
    /// the server's key), or `None` when one is there.
    fn master(&self, server: &KabloHost) -> Result<Option<Vec<Vec<u8>>>, Refusal>;

    /// Called with how the program of [`Backend::master`] ended.
    fn master_made(&self, server: &KabloHost, status: u8) -> Result<(), Refusal>;

    /// Puts kablo on the server (or finds it there), and tidies it as every connection does:
    /// the terminals that run an earlier release are renewed, the versions nothing runs from go
    /// ([`super::install::tidy`]). `live` are the account's terminals the ledger has, the ones
    /// renewed whoever has them open. The answer says what the server's terminals are when the
    /// look at it was a whole one ([`Installed::census`]), and the service forgets the
    /// ledger's rows that are not among them ([`forget_gone`]).
    fn install(&self, server: &KabloHost, live: &[SessionId]) -> Result<Installed, Refusal>;

    /// The number of the next terminal on the server (`7` for the `p7` an agent there is told
    /// and says), from the server's counter, which the agent's command there draws from too.
    /// `None`: the server could not count now; the terminal keeps the name its holder made.
    /// Called on a thread of its own, like the other steps that ask the server.
    fn number(&self, _server: &KabloHost) -> Option<u64> {
        None
    }

    /// Where a line about a pane's wait for its terminal of this Mac goes: a wait that was slow
    /// ([`Backend::slow_open`]), and every pane that was not given a terminal, with the step it was
    /// stuck in or the reason, and a pane that was let go of ([`Core::stalled`]) and is back. The
    /// default is the log (`bateri.log` is the standard error of the application), each line
    /// beginning with the time of day it was written at: these lines say what happened while no
    /// one looked, and the person remembers when.
    fn log_line(&self, line: &str) {
        eprintln!("{} bateri: {line}", crate::clock::stamp());
    }

    /// How long a pane's wait for its terminal is before it is told ([`Backend::log_line`]).
    fn slow_open(&self) -> Duration {
        waiting::SLOW_OPEN
    }

    /// Writes the files the shells of a new terminal are set up from to the server, for an account
    /// the ledger brought back: `install` does it with the look for kablo, and a recalled account
    /// is not looked at (kablo is where the ledger says), so without this its new terminals would
    /// start from the files an earlier application left. Best effort, on a thread of its own, once
    /// per account per run.
    fn files(&self, _server: &KabloHost, _installed: &Installed) {}

    /// Makes the folder a program on the server may be offered for the bytes of a file
    /// ([`crate::stage`]) and returns it: through the account's main connection, once per account
    /// per run, on a thread of its own and after the step it follows has answered, so that no one
    /// waits for it. `None`: the server would not, or there is no folder to offer (the timed run);
    /// the next terminal of the account tries again.
    fn stage_folder(&self, _server: &KabloHost) -> Option<crate::stage::Folder> {
        None
    }

    /// The folder of `account` ([`Backend::stage_folder`]) is made: the panes that show its
    /// terminals offer it to their programs from now on. Told from the thread that made it.
    fn stage_ready(&self, _account: &str) {}

    /// Starts the bridge to the server.
    fn carry(&self, server: &KabloHost, installed: &Installed) -> io::Result<Carried>;

    /// The version of the kablo this computer brings (the package's), or `None`
    /// when it brings none. An account whose server was last known at another
    /// version is [`Backend::refresh`]ed before its bridge is made.
    fn version(&self) -> Option<String> {
        None
    }

    /// Puts the version this computer brings on a server that has had another
    /// (a ledger's account after bateri was updated) and tidies it as [`Backend::install`]
    /// does, without ending any terminal; the answer is what is on the server now. `live` are
    /// the account's terminals the ledger has ([`Backend::install`]). Called for an account the
    /// ledger brought back at its first dial of every run, whether or not its version is
    /// the one this computer brings.
    /// Called on the thread that makes the bridge, **before** it is
    /// made: a bridge that runs is not replaced by the renewal. `None`: it could
    /// not be done now (no connection, a login nobody saved, a server that will
    /// not take the file) — the bridge is made from the binary the ledger knows,
    /// which is still there and serves the relays that run, and the next dial of
    /// the account tries again.
    fn refresh(&self, _server: &KabloHost, _live: &[SessionId]) -> Option<Installed> {
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

    /// Whether a terminal of this Mac shown again is read from its past ([`Hub::resume_raw`]):
    /// `false` in a launch after one that did not settle, which may have been crashed by what
    /// it read there. Asked as each pane is given its terminal.
    fn shows_past(&self) -> bool {
        true
    }

    /// Whether the shells of a new terminal draw the dock (the settings' `[shell] integration`
    /// says so: `"auto"`); `false` is `"blocks"`, the marks without the dock. The same answer the
    /// Mac's own shells get, so a server is no different.
    fn dock(&self) -> bool {
        true
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

    /// The program in front of the terminal `pane` shows is another one as far as
    /// the pane's icon tells ([`Shown::kind`]): `npm` for `claude`, a program
    /// for the shell. Told once per change of that kind — not of the name: one
    /// program nothing here knows for another is not news — with the terminal
    /// appearing (a program already in front) or going (the pane has the server's
    /// icon again), and when the server first says what is in front, whatever it is
    /// (a pane handed over with a program's view over the terminal waits for that
    /// word). Told also where the program in front of a pane's terminal on this Mac is another
    /// one by name ([`Service::local_program`]). Called on the service's thread; the application
    /// hops to its own.
    fn program_changed(&self, _pane: &PaneUuid) {}

    /// The terminals the account's server keeps in the background
    /// ([`Service::background`]) are not the ones they were: one came, went, or says something
    /// else of itself. Told once per change of what [`Service::background`] returns for the
    /// account, which is not a change of the server's list — a terminal a pane starts to show leaves
    /// the background. Called on the service's thread; the application hops to its own.
    fn background_changed(&self, _account: &str) {}

    /// Whether the application takes what the programs on a server say to it
    /// (`kablo app`): asked when an account's client is made, so that the bridge is
    /// told to give it tickets, and again for each ticket (a `false` then declines
    /// it). The default is no: the bridge gives such a client nothing and `kablo app`
    /// finds no app.
    fn takes_mail(&self) -> bool {
        false
    }

    /// A program on the server said all it had to the application. Called on the
    /// service's thread, which must not wait: the application hops to its own and
    /// answers the [`Letter`] when it has the answer. The default does not answer,
    /// which hangs up.
    fn mail(&self, _letter: Letter) {}

    /// The connection to the account's server came up and the server has answered with its
    /// terminals ([`Service::background`] and [`Service::exited`] are as it said): what a
    /// program on the server left for the application while no connection was up can be
    /// read now. Called on a thread of its own, once per connection, so it may take as long as
    /// the server does.
    fn reachable(&self, _account: &str, _server: &KabloHost) {}

    /// Who opened which of the server's terminals, as the command there wrote it down
    /// ([`crate::agent::notes::RECORDS`]); empty when it cannot be read. Called on a thread of
    /// its own, for a [`Service::survey`], so it may wait on the server.
    fn records(&self, _server: &KabloHost) -> Vec<(Terminal, Record)> {
        Vec::new()
    }

    /// How a program reaches the server the `ssh` arguments `args` name (`bateri split -S prod`):
    /// the server and bateri's level for it, over the connection bateri keeps to it. The same
    /// gate as the server's own pane ([`Backend::route`]); `ask` is how a password is asked for
    /// when the connection must be made. It **waits** for that password, so it is called on a
    /// thread of its own and never on the main one. The `Err` is a line for the program that asked. It also sets the server up
    /// as a terminal does, once a run — kablo, the files, the command — and renews the terminals
    /// kablo there ran before it (`live` gives the ledger's terminals of an account).
    fn reach(
        &self,
        _args: &[String],
        _ask: crate::ssh_route::Ask,
        _live: &dyn Fn(&str) -> Vec<SessionId>,
    ) -> Result<Reached, String> {
        Err("this build cannot reach a server for a program".to_owned())
    }

    /// The service's way to ask for programs and connections on the servers it keeps accounts
    /// for: given once, when the service starts, to a backend that runs things for callers
    /// ([`Backend::command`], [`Backend::connection`]).
    fn serving(&self, _streams: Streams) {}

    /// Runs `command` on `server` over the one connection kept to it, and gives it as a
    /// [`RemoteCommand`] is held. `ask` is how a password is asked for when the connection has to
    /// be made first; it **waits** for it, so this is called on a thread of its own and never on
    /// the main one. `wait` is how long the connection and the server's answer may take. The `Err`
    /// is a line for the person who asked.
    fn command(
        &self,
        _server: &KabloHost,
        _command: Command,
        _ask: crate::ssh_route::Ask,
        _wait: Duration,
    ) -> Result<RemoteCommand, String> {
        Err("this build cannot run a program on a server".to_owned())
    }

    /// Has `server` connect to `host:port` over the one connection kept to it, and gives the
    /// connection as a [`RemoteStream`] is held. Waits as [`Backend::command`] does.
    fn connection(
        &self,
        _server: &KabloHost,
        _host: &str,
        _port: u16,
        _ask: crate::ssh_route::Ask,
        _wait: Duration,
    ) -> Result<RemoteStream, String> {
        Err("this build cannot open a connection through a server".to_owned())
    }
}

/// A server this Mac can reach for a program ([`Backend::reach`]).
#[derive(Debug)]
pub struct Reached {
    /// The server, as a pane of it is started.
    pub server: KabloHost,
    /// What the `bateri` command may do on it.
    pub level: bt_core::CommandLevel,
}

/// What a [`Service::survey`] came back with.
#[derive(Debug, Default)]
pub struct Survey {
    /// Every terminal the server has, as it said it just now; `None` when it could not be asked
    /// (no connection to it, or no answer in time).
    pub terminals: Option<Vec<Status>>,
    /// Who opened which of them, as far as it could be read.
    pub records: Vec<(Terminal, Record)>,
}

/// The screen of a terminal no pane shows, as the server built it ([`Service::picture`]): the
/// bytes that make it when they are written to a freshly reset terminal of `cols` × `rows`.
/// `offset` is how much of the program's output it is of, so that two with the same one are the
/// same screen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Screen {
    pub offset: u64,
    pub cols: u16,
    pub rows: u16,
    pub bytes: Vec<u8>,
}

/// How many times a terminal is started again under the next number when a person's terminal
/// has the one the counter gave.
const RENUMBER_TRIES: u8 = 5;

/// How long a [`Service::survey`] waits for the server's list and for the records: a program is
/// waiting for the answer, and half an answer is better than none.
const SURVEY_WAIT: Duration = Duration::from_secs(10);

/// How long a [`Service::picture`] waits for the server's screen: the picture is a courtesy, and
/// the next round asks again.
const PICTURE_WAIT: Duration = Duration::from_secs(10);

/// The most a program may say to the application in one conversation, in bytes.
/// What a conversation keeps while the program is still talking is bounded by this
/// (anything on the same computer may start one): a program that says more is hung up
/// on. Far above any request of a few lines.
pub const LETTER_LIMIT: usize = 256 * 1024;

/// What a program on a server said to the application, all of it, and the way to
/// answer.
pub struct Letter {
    /// The server's account ([`ssh_wrap::host_key`]).
    pub account: String,
    /// The terminal the program runs in, as the kernel's record of the program at
    /// the other end of the bridge's mailbox says (never what the program says of
    /// itself); `None` when it runs in none (a cron job).
    pub from: Option<SessionId>,
    /// The server as the user typed it (`rasp`): the word that names it in the lines a person
    /// reads.
    pub host: String,
    /// The `ssh` arguments that reach the server, the program's name first
    /// ([`Shown::argv`]): what a pane that shows one of its terminals is started with.
    pub argv: Vec<String>,
    /// What it said, up to its end.
    pub bytes: Vec<u8>,
    tx: Sender<Msg>,
    talk: Talk,
    answered: bool,
    gone: Gone,
}

/// Whether the program a [`Letter`] came from is still in the conversation: it is over when the
/// program hung up on it (a `^C` on `bateri wait`), when the answer was read, or when the
/// connection to the server went. What the application started for the program stops with it.
#[derive(Clone, Default)]
pub struct Gone(Arc<Mutex<GoneState>>);

#[derive(Default)]
struct GoneState {
    over: bool,
    then: Vec<Box<dyn FnOnce() + Send>>,
}

impl Gone {
    /// A conversation that is going on.
    pub fn new() -> Self {
        Self::default()
    }

    /// Runs `then` when the conversation is over, on whichever thread ends it; at once, on this
    /// one, when it is over already.
    pub fn when_over(&self, then: impl FnOnce() + Send + 'static) {
        let mut state = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if state.over {
            drop(state);
            then();
        } else {
            state.then.push(Box::new(then));
        }
    }

    /// The conversation is over: what waited for that runs.
    pub fn end(&self) {
        let then = {
            let mut state = self.0.lock().unwrap_or_else(PoisonError::into_inner);
            state.over = true;
            std::mem::take(&mut state.then)
        };
        for then in then {
            then();
        }
    }
}

impl Letter {
    /// The conversation the letter is of, which says when the program is gone from it.
    pub fn gone(&self) -> Gone {
        self.gone.clone()
    }

    /// Says `bytes` to the program and ends the conversation. Nothing happens
    /// when the connection to the server went meanwhile: the program saw that.
    pub fn answer(mut self, bytes: Vec<u8>) {
        self.answered = true;
        let _ = self.tx.send(Msg::Answer {
            account: self.account.clone(),
            talk: self.talk,
            bytes,
        });
    }

    /// Ends the conversation without an answer; so does dropping the letter.
    pub fn hang_up(self) {}
}

impl Drop for Letter {
    fn drop(&mut self) {
        if !self.answered {
            let _ = self.tx.send(Msg::Hang {
                account: self.account.clone(),
                talk: self.talk,
            });
        }
    }
}

/// Why a bridge could not be started, as far as the error says: a login the bridge cannot
/// answer is [`io::ErrorKind::PermissionDenied`], and a program that is not there is
/// [`io::ErrorKind::NotFound`], which is this Mac's kablo with its file removed. (A bridge over
/// ssh starts `ssh`; the far shell's own 127 is read from its status, [`ssh::why_it_ended`]. An
/// `ssh` that is not there ends here too: writing kablo again does not help, and costs one more
/// try before the panes that wait are let go.)
fn cause_of(error: &io::Error) -> Cause {
    match error.kind() {
        io::ErrorKind::PermissionDenied => Cause::LoginNeeded,
        io::ErrorKind::NotFound => Cause::Missing,
        _ => Cause::Dropped,
    }
}

/// A terminal of this Mac that has been asked for and has not come: the pane is born over its
/// stream already ([`LocalAsk`]), and what it waits for is the terminal.
struct Awaiting {
    parts: Parts,
    /// The terminal the ledger had for the pane, which is to be found again: it has to be on the
    /// server, the program's process id comes with the server's word of it, and a terminal that
    /// is not there is made instead ([`Core::resumed_gone`]).
    resumed: Option<SessionId>,
    /// The terminal asked of the server for the pane (a new one, or the one made instead).
    terminal: Option<SessionId>,
    /// The pane was closed, or its quit ends its programs, while the terminal was on its way: it
    /// is ended when it comes.
    end_asked: bool,
    /// The person chose an ordinary shell instead ([`Core::choose_ordinary`]).
    chosen: bool,
}

/// How long a refused account of this Mac stays refused: the next pane does not repeat a step
/// that has just failed (and is given the same reason, and an ordinary shell, as the first).
const LOCAL_REFUSAL: Duration = Duration::from_secs(60);

/// What the loop is told, from the threads that carry bytes and do the slow
/// work.
enum Msg {
    Accepted(UnixStream, Peer),
    Hold(HoldId, Vec<u8>),
    HoldEof(HoldId),
    /// The bridge's reader has waited for the account's holders to take what is queued for them
    /// for longer than [`HOLD_STALL`] and none has: the ones that took nothing are let go of.
    Stalled {
        account: String,
        generation: u64,
    },
    /// A holder that was let go of ([`Core::stalled`]) has taken all it was owed.
    Drained(HoldId),
    /// A request's preparation wants `argv` run on the holder's terminal; the
    /// status goes back on `reply`.
    Run {
        hold: HoldId,
        argv: Vec<Vec<u8>>,
        reply: Sender<u8>,
    },
    /// A preparation asks the person whether to set the server up and waits: the answer goes back
    /// on `reply`, and `stop` is set when the person stops the install afterwards or the holder
    /// goes. The reply is dropped (the wait ends) with the holder.
    Ask {
        hold: HoldId,
        reply: Sender<Answer>,
        stop: Arc<AtomicBool>,
    },
    /// The person's answer for the pane ([`Service::answer_setup`]).
    Answered(PaneUuid, Answer),
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
    /// The server's counter was asked for the number of a terminal that rides an account that
    /// is there already ([`Backend::number`]).
    Numbered {
        hold: HoldId,
        account: String,
        spec: NewSession,
        number: Option<u64>,
        /// The account's files were written on the way ([`Backend::files`]).
        filed: bool,
    },
    /// The account of this Mac is wanted ([`Service::open_here`]).
    Here(Pending),
    /// A pane wants a terminal of this Mac ([`Service::submit_local`]).
    Local(LocalAsk),
    /// The person chose an ordinary shell in place of the terminal a pane waits for: the
    /// request's ticket says which ([`bt_core::PtyOps::ordinary_instead`]).
    Ordinary(Arc<ask::Ticket>),
    /// A view opened in the pane a terminal of this Mac carries, by a program of that name if it
    /// gave one (`Some`), or none is open (`None`) ([`Service::view_changed`]).
    View {
        terminal: SessionId,
        program: Option<Option<String>>,
    },
    /// The counter was asked for the number of the terminal a pane of this Mac wants
    /// ([`Core::local_numbered`]).
    LocalNumbered {
        hold: HoldId,
        number: Option<u64>,
    },
    /// The account of this Mac that the panes waiting for it are for was made, or could not be.
    LocalAccount(Result<(), Refusal>),
    /// The pane of a raw holder changed size.
    Resize {
        hold: HoldId,
        size: Size,
    },
    /// The program of the pane of a holder ended with input unread ([`PtyOps::drop_input`]);
    /// `remote`: it ran on the server the pane shows.
    DropInput {
        hold: HoldId,
        remote: bool,
    },
    /// The program of a pane that holds a terminal of its own ended on the server it shows
    /// ([`Service::drop_shown`]).
    DropShown(PaneUuid),
    /// kablo was put on this Mac, or could not be.
    HereMade {
        outcome: Result<Installed, Refusal>,
        pending: Pending,
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
        /// What the reader counted the bytes in ([`Queue::arrive`]).
        queue: Arc<Queue>,
    },
    /// The user closed the pane: the terminal it shows ends.
    End(PaneUuid),
    /// The user closed the pane and chose to leave the terminal it shows running.
    Leave(PaneUuid),
    /// bateri is being taken off the server of this account ([`Service::removing`]).
    Removing(String),
    /// The removal could not be finished ([`Service::removal_failed`]).
    RemovalFailed(String),
    /// bateri was taken off the server of this account ([`Service::removed`]).
    Removed(String),
    /// The user cleared the pane's scrollback: its terminals forget theirs.
    Forget(PaneUuid),
    /// Someone waits for this Mac's account to be there ([`Service::when_local`]).
    WhenLocal(Box<dyn FnOnce(bool) + Send>),
    /// The next pane to ask for the terminal takes it ([`Service::take_over`]).
    TakeOver {
        account: String,
        terminal: SessionId,
    },
    /// ⏎ in a pane whose terminal was taken ([`Service::bring_back`]).
    BringBack(PaneUuid),
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
    /// The files of an account's server are to be written again ([`Service::rewrite_files`]).
    Rewrite(String),
    /// The files of every account's server are ([`Service::rewrite_all_files`]).
    RewriteAll,
    /// A caller wants a program or a connection on an account's server ([`Streams`]).
    StreamOpen(pipes::StreamOpen),
    /// Whether the account's connection is up now ([`Streams::live`]).
    StreamLive {
        account: String,
        reply: Sender<bool>,
    },
    /// A caller's reads took `bytes` of what a stream's far end sent.
    StreamTook {
        account: String,
        talk: Talk,
        bytes: usize,
    },
    /// A caller wrote to a stream or has nothing more to write.
    StreamWrote,
    /// A caller is done with a stream, or ended it.
    StreamHangUp {
        account: String,
        talk: Talk,
    },
    /// The computer woke up.
    Wake,
    /// Ask every account for its terminals ([`Service::refresh_background`]).
    Relist,
    /// The application answers a [`Letter`]: the bytes are said and the conversation ends.
    Answer {
        account: String,
        talk: Talk,
        bytes: Vec<u8>,
    },
    /// The application let go of a [`Letter`] unanswered.
    Hang {
        account: String,
        talk: Talk,
    },
    /// The application wants every terminal of an account as the server has it just now
    /// ([`Service::survey`]).
    Survey {
        account: String,
        done: Box<dyn FnOnce(Survey) + Send>,
    },
    /// The application wants the screen of a terminal of an account ([`Service::picture`]).
    Picture {
        account: String,
        terminal: SessionId,
        done: Box<dyn FnOnce(Option<Screen>) + Send>,
    },
    /// The records of survey `id` were read ([`Backend::records`]).
    Recorded {
        id: u64,
        records: Vec<(Terminal, Record)>,
    },
    /// What the loop holds for the holders and terminals it knows, for a test that looks that
    /// nothing is kept for one that is gone ([`Service::kept_counts`]).
    #[cfg(test)]
    Counts(Sender<Counts>),
    Stop,
}

/// How many entries the loop keeps of a few kinds ([`Service::kept_counts`]).
#[cfg(test)]
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Counts {
    /// The panes of holders ([`Core::panes`]).
    pub(crate) panes: usize,
    /// The programs' views of terminals of this Mac ([`Core::views`]).
    pub(crate) views: usize,
}

/// What is told when the account of this Mac is there, or could not be made: its name.
type Told = Box<dyn FnOnce(Result<String, Refusal>) + Send>;

/// A [`Told`] that the thread making the account and the loop both reach: whichever finds it
/// first says it, so that a thread that could not be made does not leave the caller waiting.
type Pending = Arc<Mutex<Option<Told>>>;

/// What a request's preparation produced.
struct Ready {
    server: KabloHost,
    installed: Installed,
    spec: NewSession,
    /// The number the server's counter gave the terminal ([`Backend::number`]); none when the
    /// holder is not one that can be told ([`wire::NAMED`]) or the server could not count.
    number: Option<u64>,
    /// A line for the user, shown before the terminal.
    note: Option<String>,
}

/// More waits for the panes of an account than this (what is queued for their holders, and what
/// the bridge's reader has sent the loop and the loop has not given them yet) and the account's
/// bridge is not read: the server's own flow control reaches the program, the program of every
/// terminal of the account that shares the bridge. A pane that takes nothing for [`HOLD_STALL`]
/// is let go of ([`Core::stalled`]), so that it does not hold the others.
const HIGH_WATER: usize = 1 << 20;

/// How long the account's bridge may be held back before the panes that take nothing of what is
/// queued for them are let go of ([`Core::stalled`]), and how long a pane may take nothing before
/// it is one of them. Well under the second the relay of a terminal waits for a client that does
/// not move (kablo's `STALL_LIMIT`, in `relay.rs`): while the bridge is held back every terminal of the
/// account stands still, and the relay of a slow pane that is read behind the one that takes
/// nothing gives up on it — the ring gets past it, a picture in place of bytes — a second after
/// the bridge stopped; the pane that takes nothing must be let go of before that, and the look
/// that finds it may come a period late (measured: at 1.1 s a slow pane beside a stuck one was
/// given a picture three times in three). A pane that reads less than a step in this time
/// ([`WRITE_STEP`]) is stuck, not slow.
const HOLD_STALL: Duration = Duration::from_millis(300);

/// How much of a holder's backlog its writer puts in the pane's socket at a time: the count of
/// what is queued falls with each, so that a pane that reads little but goes on reading is seen
/// to move.
const WRITE_STEP: usize = 16 << 10;

/// What a holder's writer thread is given. The load is the holder's ([`Load`]), or one of its
/// own for bytes that are not counted against the account.
enum Out {
    Bytes(Vec<u8>, Arc<Load>),
    Close,
}

/// What waits to reach the panes of one account's terminals, and what the bridge's reader waits
/// for before it reads more of the account's stream.
#[derive(Default)]
struct Queue {
    /// The panes that hold the account back can be let go of ([`Core::stalled`]): the account is
    /// this Mac's, whose panes are the terminals' holders themselves. A server's account has
    /// none that can — the pane's holder is the pipe to a terminal, which the server's own flow
    /// control already waits for — so no clock runs for it ([`Queue::wait`]), and nothing
    /// looks again and again at what it cannot change.
    stallable: bool,
    /// The service has stopped: nobody is left to be told what the reader reads, and a reader
    /// that waits for the count to go down stops waiting ([`Waited::Closed`]).
    closed: AtomicBool,
    /// Bytes the holders' writers have been given and have not put in the panes' sockets.
    bytes: AtomicUsize,
    /// Bytes the bridge's reader has sent the loop that the loop has not settled yet: they are
    /// on their way to the holders, and a reader that did not count them would read on at the
    /// speed of the pipe while the loop worked them off, and stand far past [`HIGH_WATER`].
    coming: AtomicUsize,
    /// What the bridge's reader waits on: it is woken when something goes down.
    wait_here: Mutex<()>,
    wake: Condvar,
}

/// How the bridge's reader found the stream's way: open, or held back so long that the panes
/// that hold it are to be looked at.
enum Waited {
    Open,
    /// The service is gone: the reader is to end.
    Closed,
    /// The count has been past the mark for [`HOLD_STALL`] without a break.
    Stalled,
}

impl Queue {
    /// The queue of an account whose stalled panes can be let go of when `stallable`.
    fn new(stallable: bool) -> Queue {
        Queue {
            stallable,
            ..Queue::default()
        }
    }

    fn bytes(&self) -> usize {
        self.bytes.load(Ordering::SeqCst)
    }

    fn pending(&self) -> usize {
        self.bytes() + self.coming.load(Ordering::SeqCst)
    }

    fn raise(&self, n: usize) {
        self.bytes.fetch_add(n, Ordering::SeqCst);
    }

    fn lower(&self, n: usize) {
        let _ = self
            .bytes
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |bytes| {
                Some(bytes.saturating_sub(n))
            });
        self.bump();
    }

    /// The bridge's reader sent the loop `n` bytes.
    fn arrive(&self, n: usize) {
        self.coming.fetch_add(n, Ordering::SeqCst);
    }

    /// The loop has settled `n` bytes of what the bridge's reader sent.
    fn settled(&self, n: usize) {
        let _ = self
            .coming
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |coming| {
                Some(coming.saturating_sub(n))
            });
        self.bump();
    }

    /// The service is stopped: whoever waits for the count to go down is woken, and ends.
    fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
        self.bump();
    }

    /// Wakes whoever waits for the count to go down.
    fn bump(&self) {
        drop(lock(&self.wait_here));
        self.wake.notify_all();
    }

    /// Returns when what is queued is no more than [`HIGH_WATER`]: the server's own flow control
    /// reaches the program while it waits. A wait has a clock of its own only while the account
    /// is held back, and it counts from when it was first held back, whatever goes down since:
    /// that a pane takes something does not say that the one beside it does, so after
    /// [`HOLD_STALL`] it says so ([`Waited::Stalled`]) and the panes are looked at one by one
    /// ([`Core::stalled`]). An account that is not held back has no timer, and nor has one whose
    /// panes cannot be let go of ([`Queue::stallable`]): it waits until the count goes down.
    fn wait(&self) -> Waited {
        let mut here = lock(&self.wait_here);
        let mut since = None;
        loop {
            if self.closed.load(Ordering::SeqCst) {
                return Waited::Closed;
            }
            if self.pending() <= HIGH_WATER {
                return Waited::Open;
            }
            if !self.stallable {
                here = self.wake.wait(here).unwrap_or_else(PoisonError::into_inner);
                continue;
            }
            let began = *since.get_or_insert_with(Instant::now);
            let left = HOLD_STALL.saturating_sub(began.elapsed());
            if left.is_zero() {
                return Waited::Stalled;
            }
            here = self
                .wake
                .wait_timeout(here, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }
}

/// What is queued for one holder, as the loop that queues it and the writer that takes it both
/// see it. What the holder has not taken counts against its account while the holder takes
/// something now and then; a holder that takes nothing for [`HOLD_STALL`] while the account is
/// held back by it is let go of ([`Core::stalled`]), and what it is owed stops counting then, so
/// the others' terminals can go on, until its pane has taken it all.
struct Load {
    state: Mutex<LoadState>,
}

struct LoadState {
    /// The account's queue the bytes count in, once the holder has bytes that count.
    queue: Option<Arc<Queue>>,
    /// Bytes given to the writer and not written.
    owed: usize,
    /// Whether `owed` is in the account's count.
    counted: bool,
    /// When the writer last wrote something, or when the holder was last given bytes with
    /// nothing owed before: where a holder that takes nothing is measured from.
    moved: Instant,
}

impl Load {
    fn new() -> Arc<Load> {
        Arc::new(Load {
            state: Mutex::new(LoadState {
                queue: None,
                owed: 0,
                counted: true,
                moved: Instant::now(),
            }),
        })
    }

    /// A load of its own, owed `owed` bytes that count against nobody's queue: what the holder
    /// of a terminal that has not been given its stream yet is sent ([`Core::flush`]).
    fn alone(owed: usize) -> Arc<Load> {
        let load = Load::new();
        let queue = Arc::new(Queue::default());
        queue.raise(owed);
        let mut state = lock(&load.state);
        state.queue = Some(queue);
        state.owed = owed;
        drop(state);
        load
    }

    /// `n` bytes were given to the holder's writer.
    fn add(&self, n: usize, queue: &Arc<Queue>) {
        let mut state = lock(&self.state);
        let queue = Arc::clone(state.queue.get_or_insert_with(|| Arc::clone(queue)));
        if state.owed == 0 {
            state.moved = Instant::now();
        }
        state.owed += n;
        if state.counted {
            queue.raise(n);
        }
    }

    /// The writer wrote `n` bytes. `true` when that was all of what the holder was owed, and
    /// the holder was let go of: it is to be attached again.
    fn wrote(&self, n: usize) -> bool {
        let mut state = lock(&self.state);
        state.owed = state.owed.saturating_sub(n);
        state.moved = Instant::now();
        if state.counted
            && let Some(queue) = &state.queue
        {
            queue.lower(n);
        }
        state.owed == 0 && !state.counted
    }

    /// Whether the holder is owed bytes that count and has taken none for `limit`.
    fn stalled(&self, now: Instant, limit: Duration) -> bool {
        let state = lock(&self.state);
        state.counted && state.owed > 0 && now.saturating_duration_since(state.moved) >= limit
    }

    /// What the holder is owed no longer counts against the account. `true` when it is owed
    /// nothing: the writer took the last of it while the holder was being let go of and did not
    /// say so, as it says it only of a holder that was let go of already.
    fn uncount(&self) -> bool {
        self.uncount_owing() == 0
    }

    /// [`Load::uncount`], saying how many bytes the holder is owed, read under the same lock.
    fn uncount_owing(&self) -> usize {
        let mut state = lock(&self.state);
        if state.counted {
            state.counted = false;
            if let Some(queue) = &state.queue {
                queue.lower(state.owed);
            }
        }
        state.owed
    }

    /// What the holder is owed counts again.
    fn recount(&self) {
        let mut state = lock(&self.state);
        if !state.counted {
            state.counted = true;
            state.moved = Instant::now();
            if let Some(queue) = &state.queue {
                queue.raise(state.owed);
            }
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// More typed into a pane than the client has put on the wire ([`Client::input_waiting`]) and
/// the pane's connection is not read: the pane's writer waits for the socket, and the pane's
/// own flow control reaches whatever it is writing. About four windows
/// ([`crate::kablo::INPUT_WINDOW`]), so that a burst the window cannot take at once is held here and
/// not spread over the memory of the process (2 MiB for a window of 512 KiB, a pane's reader
/// shut at 2 MiB and open again at 512 KiB).
const INPUT_HOLD: usize = 4 * crate::kablo::INPUT_WINDOW;

/// Where a pane's connection that is not being read is read again: one window waits.
const INPUT_RESUME: usize = crate::kablo::INPUT_WINDOW;

/// Whether a holder's reader thread may read: the loop shuts it when too much of what the
/// holder typed waits for the server, and opens it when the server has taken enough.
#[derive(Default)]
struct Gate {
    shut: Mutex<bool>,
    opened: Condvar,
}

impl Gate {
    fn shut(&self) {
        *self.shut.lock().unwrap_or_else(PoisonError::into_inner) = true;
    }

    fn open(&self) {
        *self.shut.lock().unwrap_or_else(PoisonError::into_inner) = false;
        self.opened.notify_all();
    }

    fn is_shut(&self) -> bool {
        *self.shut.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Returns when the gate is open.
    fn pass(&self) {
        let mut shut = self.shut.lock().unwrap_or_else(PoisonError::into_inner);
        while *shut {
            shut = self
                .opened
                .wait(shut)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }
}

/// A holder's gate, opened when the holder's connection is let go of, so that a reader that
/// waits at it goes on to find the end.
struct Throttle(Arc<Gate>);

impl Drop for Throttle {
    fn drop(&mut self) {
        self.0.open();
    }
}

/// One holder's connection as the loop keeps it.
struct Conn {
    peer: Peer,
    out: Sender<Out>,
    /// Whether the holder's reader may read ([`INPUT_HOLD`]).
    throttle: Throttle,
    /// The holder is a pane of this Mac: how its terminal's program ended is written here when
    /// the hub has said its last word, before the connection closes.
    status: Option<Arc<StreamExit>>,
    /// What the holder's writer is owed ([`Load`]).
    load: Arc<Load>,
    /// The holder took nothing for so long that its terminal's stream was stopped
    /// ([`Core::stalled`]), and when; it is started again when the pane has taken what it was
    /// owed.
    suspended: Option<Instant>,
    /// The pane's terminal does not take input yet ([`Core::open_gates`]): what the pane types
    /// waits in its stream, in order, and is read when it does.
    waits_for_input: bool,
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

/// How many attempts in a row to reach a server may fail before the panes that wait for a new
/// terminal there are let go of: a holder of a server falls back to a plain `ssh`, a pane of this
/// Mac is given an ordinary shell and told why.
const GIVE_UP_AFTER: u32 = 3;

/// A server the service has a client for.
struct Account {
    server: KabloHost,
    installed: Installed,
    /// The kablo the server was last known to have is not the version this
    /// computer brings: the next dial renews it first ([`Backend::refresh`]).
    stale: bool,
    /// The account was brought back by the ledger and has not been looked at in this run: its
    /// first dial tidies the server's kablo, as every connection does ([`Backend::refresh`]),
    /// whatever version it has. Unlike `stale`, it does not hold the shells' files back.
    tidy_due: bool,
    /// Which bridge messages are current.
    generation: u64,
    bridge: Option<Bridge>,
    /// A thread is starting a bridge.
    dialing: bool,
    /// How many bridges in a row ended because the kablo could not be run ([`Cause::Missing`]),
    /// until a connection comes up; what a login file prints on the way is not one. The first is
    /// mended by the next dial (`stale`); a second is a server that cannot keep kablo for now,
    /// and the panes that wait for a new terminal there do not wait for it ([`Core::gives_up`]).
    missed: u32,
    /// Bytes queued for the holders of this account's terminals.
    queue: Arc<Queue>,
    /// The terminals the ledger has marked to end were asked to, on the connection
    /// that is up (false again when it goes).
    purged: bool,
    /// The files a new terminal's shell is set up from are on the server as this application
    /// has them: written with the look for kablo ([`Backend::install`]) or, for an account the
    /// ledger brought back, by the first new terminal's step ([`Backend::files`]).
    files: bool,
    /// Held while the files are written ([`Core::rewrite`]), so that writes made one after the
    /// other end in the order they were asked for: the last one reads the setting last.
    writing: Arc<Mutex<()>>,
}

impl Account {
    /// The bridge ended because the kablo could not be run: the next dial writes it again
    /// ([`Backend::refresh`]) before it starts a bridge.
    fn missing(&mut self) {
        self.missed = self.missed.saturating_add(1);
        self.stale = true;
    }
}

/// What one account's server keeps that no pane of this Mac shows, in the roster's two halves.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Kept {
    /// Nobody has it open ([`Service::background`]).
    background: Vec<Background>,
    /// Another computer has it open ([`Service::elsewhere`]).
    elsewhere: Vec<Background>,
}

/// The running service. Dropping it stops it.
pub struct Service {
    socket: PathBuf,
    tx: Sender<Msg>,
    stop: Arc<AtomicBool>,
    threads: Mutex<Vec<thread::JoinHandle<()>>>,
    /// What each pane shows, as the loop last wrote it.
    shown: Arc<Mutex<HashMap<PaneUuid, Shown>>>,
    /// The program in front of each terminal of this Mac that a pane has, as the loop last wrote
    /// it ([`Service::local_program`]).
    local_programs: Arc<Mutex<HashMap<SessionId, String>>>,
    /// The panes that chose an ordinary shell over a terminal found again, as the loop keeps them
    /// ([`Service::left_running`]).
    left_running: Arc<Mutex<HashSet<PaneUuid>>>,
    /// The terminals each account keeps in the background, as the loop last wrote them.
    background: Arc<Mutex<BTreeMap<String, Kept>>>,
    /// The terminals each account's server lists that are no longer running and that no pane
    /// shows, as the loop last wrote them ([`Service::exited`]).
    exited: Arc<Mutex<BTreeMap<String, Vec<SessionId>>>>,
    /// Where the terminals that have to outlive this process are written down.
    ledger: Arc<Ledger>,
    /// The names panes of this Mac are to have for their terminals ([`Service::expect_name`]).
    names: Mutex<HashMap<PaneUuid, SessionId>>,
    /// What [`Service::stage_of`] reads.
    stages: Arc<Mutex<HashMap<String, Staged>>>,
    /// What each account has queued for its holders, as the loop made the account
    /// ([`Service::queued`]).
    queues: Arc<Mutex<HashMap<String, Arc<Queue>>>>,
    /// What the service asks of the world around it, for what a program asks that is not a
    /// terminal's ([`Service::backend`]).
    backend: Arc<dyn Backend>,
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

/// How long a terminal said to be taken ([`Service::take_over`]) waits for the pane that was
/// opened to show it: a pane that never asks leaves nothing behind that would take the terminal
/// from somebody the next time it is opened the usual way.
const TAKE_WAIT: Duration = Duration::from_secs(60);

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
        let kept_backend = Arc::clone(&backend);
        let shown = Arc::new(Mutex::new(HashMap::new()));
        let local_programs = Arc::new(Mutex::new(HashMap::new()));
        let left_running = Arc::new(Mutex::new(HashSet::new()));
        let background = Arc::new(Mutex::new(BTreeMap::new()));
        let exited = Arc::new(Mutex::new(BTreeMap::new()));
        let stages = Arc::new(Mutex::new(HashMap::new()));
        let queues = Arc::new(Mutex::new(HashMap::new()));
        let kept_ledger = Arc::clone(&ledger);
        let core = Core {
            shared: Arc::new(Shared {
                backend,
                tx: tx.clone(),
                ledger,
                stages: Arc::clone(&stages),
                queues: Arc::clone(&queues),
            }),
            hub: Hub::new(),
            conns: BTreeMap::new(),
            accounts: BTreeMap::new(),
            waiting: BTreeMap::new(),
            asking: BTreeMap::new(),
            waiters: Vec::new(),
            panes: BTreeMap::new(),
            local_waiting: Vec::new(),
            local_when: Vec::new(),
            local_refused: None,
            awaiting: BTreeMap::new(),
            end_on_arrival: BTreeSet::new(),
            ordinaries: BTreeMap::new(),
            left_running: Arc::clone(&left_running),
            views: HashMap::new(),
            arrived: Vec::new(),
            shown: Arc::clone(&shown),
            local_programs: Arc::clone(&local_programs),
            local_told: HashMap::new(),
            roster: BTreeMap::new(),
            relist: BTreeSet::new(),
            fresh: BTreeSet::new(),
            reached: Vec::new(),
            told: BTreeMap::new(),
            unasked: BTreeSet::new(),
            background: Arc::clone(&background),
            exited: Arc::clone(&exited),
            refresh: false,
            takes: BTreeMap::new(),
            ending: Vec::new(),
            removing: BTreeSet::new(),
            drafts: BTreeMap::new(),
            letters: BTreeMap::new(),
            opening: Vec::new(),
            pipes: BTreeMap::new(),
            collisions: BTreeMap::new(),
            surveys: Vec::new(),
            pictures: Vec::new(),
            surveyed: 0,
            grace,
            rx,
            generation: 0,
        };
        let serving = thread::Builder::new()
            .name("ssh-hold service".into())
            .spawn(move || core.run())?;
        kept_backend.serving(Streams {
            tx: tx.clone(),
            person: false,
        });
        Ok(Service {
            socket,
            tx,
            stop,
            threads: Mutex::new(vec![accepting, serving]),
            shown,
            local_programs,
            left_running,
            background,
            exited,
            ledger: kept_ledger,
            names: Mutex::new(HashMap::new()),
            stages,
            queues,
            backend: kept_backend,
        })
    }

    /// What asks the service for a program or a connection on an account's server
    /// ([`Streams`]); the application's own is the one [`Backend::serving`] was given.
    #[cfg(test)]
    pub fn streams(&self) -> Streams {
        Streams {
            tx: self.tx.clone(),
            person: false,
        }
    }

    /// What the service asks of the world around it: the application asks it for what a
    /// program wants of a server ([`Backend::reach`], [`Backend::route`]) on a thread of its own.
    pub fn backend(&self) -> Arc<dyn Backend> {
        Arc::clone(&self.backend)
    }

    /// The terminal a server keeps that the pane shows, if it shows one: written by the loop
    /// after anything that could change it, so it is what was true a moment ago. **Never a
    /// terminal of this Mac** ([`Shown::is_local`]): the terminal that carries a pane's bytes
    /// is not what the pane shows, and every surface that is for servers (the card, the list,
    /// the load, the files, "on {host}") asks here.
    pub fn shown(&self, pane: &PaneUuid) -> Option<Shown> {
        self.shown
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(pane)
            .cloned()
    }

    /// The program in front of `terminal`, a terminal of this Mac that a pane has, as kablo last
    /// said, controls removed: what names the program that asks for a pane ("Claude opened…").
    /// Apart from [`Service::shown`], which is a server's alone.
    pub fn local_program(&self, terminal: &SessionId) -> Option<String> {
        self.local_programs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(terminal)
            .cloned()
    }

    /// The folder for a program's files on the server of `account`, once it is made
    /// ([`Backend::stage_folder`]); `None` before, and for an account of this Mac.
    pub fn stage_of(&self, account: &str) -> Option<crate::stage::Folder> {
        match self
            .stages
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(account)?
        {
            Staged::Ready(folder) => Some(folder.clone()),
            Staged::Making => None,
        }
    }

    /// The terminals the server of `account` keeps for the user that no pane of this Mac shows and
    /// no other computer has open, the most recently written first: running, not asked to end, and
    /// as the server last said — a cache that events and [`Service::refresh_background`] keep, read
    /// here without waiting for the server. An account the service has no bridge for has none.
    pub fn background(&self, account: &str) -> Vec<Background> {
        self.kept_of(account, |kept| &kept.background)
    }

    /// [`Service::background`] of every account, the most recently written first.
    pub fn background_all(&self) -> Vec<Background> {
        self.kept_all(|kept| &kept.background)
    }

    /// The terminals of `account` another computer has open ([`Background::elsewhere`] names
    /// them) and no pane of this Mac shows, the most recently written first. Like
    /// [`Service::background`], read from the cache without waiting.
    pub fn elsewhere(&self, account: &str) -> Vec<Background> {
        self.kept_of(account, |kept| &kept.elsewhere)
    }

    /// The terminals of `account` the server still lists that are not running any more (the
    /// program ended; nobody has heard of it) and that no pane of this Mac shows. Like
    /// [`Service::background`], read from the cache without waiting.
    pub fn exited(&self, account: &str) -> Vec<SessionId> {
        self.exited
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(account)
            .cloned()
            .unwrap_or_default()
    }

    /// [`Service::elsewhere`] of every account, the most recently written first.
    pub fn elsewhere_all(&self) -> Vec<Background> {
        self.kept_all(|kept| &kept.elsewhere)
    }

    /// One half of the cache for `account`. The two halves are read by the one pair of functions,
    /// so that they cannot drift apart.
    fn kept_of(&self, account: &str, half: fn(&Kept) -> &Vec<Background>) -> Vec<Background> {
        self.background
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(account)
            .map(|kept| half(kept).clone())
            .unwrap_or_default()
    }

    /// One half of the cache for every account, the most recently written first.
    fn kept_all(&self, half: fn(&Kept) -> &Vec<Background>) -> Vec<Background> {
        let mut all: Vec<Background> = self
            .background
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .values()
            .flat_map(|kept| half(kept).iter())
            .cloned()
            .collect();
        sort_recent_first(&mut all);
        all
    }

    /// The pane about to show `terminal` of `account` is to **take** it from whichever computer
    /// has it open (a terminal moved here, [`Service::elsewhere`]): the next time a pane asks to
    /// see it, within [`TAKE_WAIT`], it attaches taking, and the other computer's pane is told by
    /// whom. Said before the pane is opened, as [`Service::background`]'s terminals are claimed;
    /// asking for nothing else changes: a terminal opened the usual way is never taken from anyone.
    pub fn take_over(&self, account: &str, terminal: &SessionId) {
        let _ = self.tx.send(Msg::TakeOver {
            account: account.to_owned(),
            terminal: terminal.clone(),
        });
    }

    /// ⏎ in a pane whose terminal another computer took: it is taken back, from where the pane
    /// left off, and the other computer's pane is told by whom. A pane that shows nothing, or a
    /// terminal nobody took, does nothing.
    pub fn bring_back(&self, pane: &PaneUuid) {
        let _ = self.tx.send(Msg::BringBack(pane.clone()));
    }

    /// Asks every account whose bridge is up for its terminals again; the answer renews
    /// [`Service::background`] ([`Backend::background_changed`] says when it differs). For the
    /// moments somebody is about to look at the list (the palette, the card opening): the cache they
    /// read is what the last answers said, and nothing asks on a clock.
    pub fn refresh_background(&self) {
        let _ = self.tx.send(Msg::Relist);
    }

    /// The user closed the pane: the terminals it has end — the one it shows ends on the server
    /// (hung up now, killed after [`END_GRACE`] if it is still there), and the one of this Mac
    /// that carries its bytes with it. Say it
    /// **before** the pane's program is signalled — the holder's going is the
    /// terminal being let go of, and from then on the pane is not found. A
    /// pane that shows nothing, and a connection that is down, do nothing.
    pub fn end(&self, pane: &PaneUuid) {
        let _ = self.tx.send(Msg::End(pane.clone()));
    }

    /// bateri is about to be taken off the server of `account` (Shell ▸ Remove bateri from
    /// “{host}”, after the question was answered): until [`Service::removed`] or
    /// [`Service::removal_failed`] the service writes nothing to that server. The terminals the
    /// removal ends cut the account's bridge, and a bridge whose kablo is gone is a missing
    /// kablo, which the next dial would write again — over a server the person is taking
    /// bateri off.
    pub fn removing(&self, account: &str) {
        let _ = self.tx.send(Msg::Removing(account.to_owned()));
    }

    /// The removal that [`Service::removing`] announced did not finish: the account is served
    /// as before, and what is missing of the server is written again as it is for any
    /// kablo that cannot be run.
    pub fn removal_failed(&self, account: &str) {
        let _ = self.tx.send(Msg::RemovalFailed(account.to_owned()));
    }

    /// bateri was taken off the server of `account` (Shell ▸ Remove bateri from “{host}”: its
    /// terminals ended and its folders deleted): the service lets go of the account — the
    /// panes that showed its terminals end with a line, the connection is closed, and the ledger,
    /// the quits that waited for its terminals and the card of what it kept forget it. Nothing is
    /// asked of the server.
    pub fn removed(&self, account: &str) {
        let _ = self.tx.send(Msg::Removed(account.to_owned()));
    }

    /// The user closed the pane and chose **Leave Running**: the terminal it shows is not
    /// asked to end, and the ledger lets go of it — a row left would be a terminal the next
    /// quit that ends ([`crate::kablo::quit::fate`]) or the launch after a crash ends, which
    /// is the opposite of what was chosen. Once the pane's holder goes the terminal is a
    /// server's to keep, listed in [`Service::background`] with the ones nobody ever showed;
    /// shown by a pane again, it is written back. Say it **before** the pane's program is
    /// signalled, as [`Service::end`] must be said: the holder's going is what loses the pane.
    pub fn leave(&self, pane: &PaneUuid) {
        let _ = self.tx.send(Msg::Leave(pane.clone()));
    }

    /// The person answered the first connection's row of `pane` ([`Setup`]): *Set Up* or *Plain
    /// ssh* to the question, *Cancel* while bateri goes up. An answer to nothing is nobody's.
    pub fn answer_setup(&self, pane: &PaneUuid, answer: Answer) {
        let _ = self.tx.send(Msg::Answered(pane.clone(), answer));
    }

    /// The program of a view in the pane `pane` ended on the server the pane shows, with what
    /// it was sent unread, and the pane holds a terminal of its own that runs the pipe to the
    /// server's terminal (it has no stream to ask by, [`PtyOps::drop_input`]): the server's
    /// terminal is asked to drop what its program has not read, **behind what the pipe had
    /// passed on** ([`crate::kablo::hub::Hub::drop_input`]). A pane that shows no terminal a
    /// server keeps asks nothing of anything.
    pub fn drop_shown(&self, pane: &PaneUuid) {
        let _ = self.tx.send(Msg::DropShown(pane.clone()));
    }

    /// `done` is told, on the service's thread, once this Mac's account is there (`true`) or
    /// could not be made (`false`); at once when it is already one or the other, and the account
    /// is made for it when nothing asked for it yet. What a launch waits for before it shows the
    /// terminals the bateri before left that no pane of it shows: before the account is there a
    /// pane cannot be given one, and after it is, every pane can.
    pub fn when_local(&self, done: impl FnOnce(bool) + Send + 'static) {
        let _ = self.tx.send(Msg::WhenLocal(Box::new(done)));
    }

    /// The user cleared what the pane remembers of its past (Clear Scrollback, Clear to Start):
    /// the terminals the pane has — the one of this Mac that carries its bytes, and the one a
    /// server keeps that it shows — are asked to forget theirs too
    /// ([`Client::forget_past`]), so that a bateri that finds them again does not give back what
    /// was cleared. A terminal that is not attached or a connection that is down is not asked:
    /// the next bateri gets the whole.
    pub fn forget_past(&self, pane: &PaneUuid) {
        let _ = self.tx.send(Msg::Forget(pane.clone()));
    }

    /// Every terminal the server of `account` has, as it says it now (a list asked for now, not the
    /// cache the other readers use), with who opened each, handed to `done` — on the service's
    /// thread, which must not wait: the application hops to its own. An account that is not
    /// connected, or a server that does not answer in [`SURVEY_WAIT`], gives what there is
    /// ([`Survey::terminals`] `None` when there is not even the list). Asked for `bateri ls`.
    pub fn survey(&self, account: &str, done: impl FnOnce(Survey) + Send + 'static) {
        let _ = self.tx.send(Msg::Survey {
            account: account.to_owned(),
            done: Box::new(done),
        });
    }

    /// The screen of a terminal of the server `account` that no pane shows, as the bytes that
    /// build it, handed to `done` — on the service's thread, which must not wait: the
    /// application hops to its own. The server is asked with a `read`, not an `attach`: the
    /// terminal keeps its clients, its size and its journal, and nothing is streamed after the
    /// answer. `None` when the connection is down, the server has no such terminal or says
    /// nothing in [`PICTURE_WAIT`]. Many askers of one terminal share one question.
    pub fn picture(
        &self,
        account: &str,
        terminal: &SessionId,
        done: impl FnOnce(Option<Screen>) + Send + 'static,
    ) {
        let _ = self.tx.send(Msg::Picture {
            account: account.to_owned(),
            terminal: terminal.clone(),
            done: Box::new(done),
        });
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

    /// A quit that ends the terminals: every terminal the panes have — the one of this Mac
    /// that carries a pane's bytes and the one a server keeps that it shows — is asked to end
    /// (a hang-up; the ledger remembers the request until it is seen to have been carried out,
    /// so a server that cannot be reached ends it at the next connection), and this waits for
    /// them to have — at most `wait`, so a server that does not answer does not hold the quit. A
    /// pane that has none, and a connection that is down, are not waited for. Say it **before**
    /// the panes' programs are signalled, as [`Service::end`] must be said.
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

    /// Makes the account of this Mac ([`super::local`]) if the service has none, and tells `done`
    /// its name — or why there is none ([`Refusal::plain`]: the package brings no kablo, or it
    /// cannot be kept here). Putting kablo in the home directory takes its time, so `done` is
    /// called on a thread of the service's, which must not wait: the application hops to its own.
    /// The account dials its bridge as every account does; no terminal is made by this.
    pub fn open_here(&self, done: impl FnOnce(Result<String, Refusal>) + Send + 'static) {
        let pending: Pending = Arc::new(Mutex::new(Some(Box::new(done))));
        if let Err(error) = self.tx.send(Msg::Here(Arc::clone(&pending))) {
            // The service is gone; nobody else will say it.
            if let Msg::Here(pending) = error.0 {
                tell(&pending, Err(Refusal::plain()));
            }
        }
    }

    /// The request of a pane for a terminal of this Mac ([`LocalAsk::new`], which gave the pane its
    /// stream already) is the service's to answer: the account of this Mac is made first if the
    /// service has none (kablo is put below the home directory on the way), and the terminal comes
    /// to the stream when it exists, with its process id. Nothing waits here: what the request ends
    /// in is the terminal, or a reason and an ordinary shell on the stream ([`LocalAsk::refuse`]) —
    /// never a timer, because a pane that was given an ordinary shell for being slow would have no
    /// terminal that outlives bateri, and one that comes after would have nobody to show it to.
    ///
    /// The terminal outlives the stream: letting go of it ends nothing. It is ended by the
    /// user's close of the pane ([`Service::end`]) and by a quit that ends the programs
    /// ([`Service::end_all`]), and it is the ledger's otherwise — when the ledger has a terminal
    /// of this Mac for the pane that is meant to live (a pane that comes back after an update, a
    /// quit that kept the programs, a crash), that one is found again instead of a new one made:
    /// the stream starts with its past ([`Stream::silence`], which the request's pane asked for
    /// with [`Service::found_again`]) and the process id is the program's there. A terminal the
    /// server does not have any more is made again, behind the same stream.
    pub fn submit_local(&self, mut ask: LocalAsk) {
        ask.handed(
            Arc::clone(&self.stop),
            Arc::clone(&self.backend),
            self.tx.clone(),
        );
        let name = ask.pane().and_then(|pane| {
            self.names
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(pane)
        });
        ask.named(name);
        if let Err(unsent) = self.tx.send(Msg::Local(ask))
            && let Msg::Local(ask) = unsent.0
        {
            ask.refuse("the service that makes the terminals of this Mac has stopped");
        }
    }

    /// Whether the ledger has a terminal of this Mac for `pane` that is to be found again: the
    /// pane's stream starts with a past then ([`LocalAsk::new`]).
    pub fn found_again(&self, pane: &PaneUuid) -> bool {
        self.ledger.local_terminal_of(pane).is_some()
    }

    /// Whether `pane` chose an ordinary shell over the terminal it found again, which runs on
    /// unseen ([`Core::choose_ordinary`]): it keeps its row in the ledger and is shown again by the
    /// next launch; the pane's close and a quit that ends the programs end it.
    pub fn left_running(&self, pane: &PaneUuid) -> bool {
        lock(&self.left_running).contains(pane)
    }

    /// The terminal of this Mac that carries `pane`'s bytes, as far as anything that asks the
    /// application about the pane goes (a program's `send` and `capture`, the name of the pane in
    /// `ls`, the view a program opens in it). A pane that left the terminal it found again
    /// running for an ordinary shell has none: the terminal is still the pane's in the ledger,
    /// for the next launch, but what the pane runs now is the shell.
    pub fn local_terminal(&self, pane: &PaneUuid) -> Option<SessionId> {
        if self.left_running(pane) {
            return None;
        }
        self.ledger.local_terminal_of(pane)
    }

    /// The terminals of this Mac that are meant to live and that none of `shown` carries
    /// ([`Ledger::unshown_local`]): what a launch shows again in tabs of their own. A terminal a
    /// pane left running for an ordinary shell is the pane's still, and waits for the next launch
    /// (it is not shown beside the pane that did not want to wait for it).
    pub fn unshown_local(&self, shown: &[PaneUuid]) -> Vec<SessionId> {
        let mut panes = shown.to_vec();
        panes.extend(lock(&self.left_running).iter().cloned());
        self.ledger.unshown_local(&panes)
    }

    /// A view opened (`open`, by the program that calls itself `program`) or closed in the pane
    /// whose terminal of this Mac is `terminal`: what the log says the pane shows when it is let
    /// go of ([`Core::stalled`]), kept by the loop so that it reads no file for it. The ledger,
    /// which the next bateri reads, is the caller's to write.
    pub fn view_changed(&self, terminal: &SessionId, open: bool, program: Option<&str>) {
        let _ = self.tx.send(Msg::View {
            terminal: terminal.clone(),
            program: open.then(|| program.map(str::to_owned)),
        });
    }

    /// How many entries the loop keeps of a few kinds, once it has taken in everything sent to it
    /// before.
    #[cfg(test)]
    pub(crate) fn kept_counts(&self) -> Counts {
        let (tx, rx) = std::sync::mpsc::channel();
        let _ = self.tx.send(Msg::Counts(tx));
        rx.recv_timeout(Duration::from_secs(10))
            .expect("the loop answers")
    }

    /// A terminal for `pane`, waited for: [`Service::submit_local`] and then until the terminal's
    /// process id is known on the stream, or the pane was given an ordinary shell instead, or
    /// `wait` is over. What a test that wants the terminal asks.
    #[cfg(test)]
    pub(crate) fn open_local_wait(
        &self,
        pane: &PaneUuid,
        spec: NewSession,
        size: Size,
        wait: Duration,
    ) -> Result<bt_core::Stream, Refusal> {
        let launch = bt_core::ChildLaunch {
            argv: vec!["/usr/bin/true".to_owned()],
            env: Vec::new(),
            cwd: None,
            size: bt_core::PtySize {
                cols: size.cols,
                rows: size.rows,
                cell_width: 8,
                cell_height: 16,
            },
        };
        let (stream, ask) = LocalAsk::new(pane, spec, size, launch, self.found_again(pane))
            .map_err(|_| Refusal::plain())?;
        self.submit_local(ask);
        let until = Instant::now() + wait;
        loop {
            if stream.status.is_ordinary() {
                return Err(Refusal::plain());
            }
            if stream.pid.get().is_some() {
                return Ok(stream);
            }
            if Instant::now() >= until {
                return Err(Refusal::plain());
            }
            thread::sleep(Duration::from_millis(5));
        }
    }

    /// How many bytes are queued for the holders of `account`'s terminals, which the bridge's
    /// stream is not read past ([`HIGH_WATER`], at the most a chunk more, with what the loop has
    /// not settled yet): what the panes of the account have not taken, less what a pane that was
    /// let go of is owed. `None` before the account is made. For a test and a measurement.
    pub fn queued(&self, account: &str) -> Option<usize> {
        let queues = self.queues.lock().unwrap_or_else(PoisonError::into_inner);
        queues.get(account).map(|queue| queue.bytes())
    }

    /// What [`Service::queued`] is held to before the bridge's stream is no longer read.
    pub const fn high_water() -> usize {
        HIGH_WATER
    }

    /// The next terminal of this Mac `pane` asks for ([`Service::submit_local`]) is named `name`,
    /// started under it as it is not found again: the pane a program asked for without a
    /// program of its own (`bateri split`), whose number the program was told before there was a
    /// terminal. Without it the terminal is numbered by the counter, as a pane the user opens
    /// is.
    pub fn expect_name(&self, pane: &PaneUuid, name: SessionId) {
        self.names
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(pane.clone(), name);
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

    /// The files the account's machine has of bateri's are written again, as its level now says
    /// (`[commands] level` for this Mac: the `bateri` command comes or goes without a restart).
    /// An account that is not made yet is written when it is.
    pub fn rewrite_files(&self, account: &str) {
        let _ = self.tx.send(Msg::Rewrite(account.to_owned()));
    }

    /// [`Service::rewrite_files`] for every account made so far that is connected (this Mac's
    /// always): what the files hold is a setting of every machine's (an agent's hook), not of one.
    /// A server that is not connected is written by its first new terminal after.
    pub fn rewrite_all_files(&self) {
        let _ = self.tx.send(Msg::RewriteAll);
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
        // The readers of the bridges that wait for what is queued to go down have nobody to tell.
        for queue in self
            .queues
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .values()
        {
            queue.close();
        }
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
                    let Ok((peer_uid, pid)) = crate::jobs::peer(&stream) else {
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
    /// The folder of each server account for a program's files, as it is made
    /// ([`Service::stage_of`]).
    stages: Arc<Mutex<HashMap<String, Staged>>>,
    /// [`Service::queued`]'s counters, one per account.
    queues: Arc<Mutex<HashMap<String, Arc<Queue>>>>,
}

/// An account's folder for a program's files.
enum Staged {
    /// A thread is making it.
    Making,
    /// It is made, for as long as the service lives.
    Ready(crate::stage::Folder),
}

impl Shared {
    /// Makes the folder of `server`'s account if it is not made and no one is making it
    /// ([`Backend::stage_folder`]), and tells the backend when it is. **After** the answer of
    /// whatever step it follows: the folder is for the next view a program opens, not for the
    /// terminal that is being started. A server's only: the Mac's programs have the folder the
    /// application made for them.
    fn stage_account(&self, server: &KabloHost) {
        if local::is_account(&server.key) {
            return;
        }
        {
            let mut stages = self.stages.lock().unwrap_or_else(PoisonError::into_inner);
            if stages.contains_key(&server.key) {
                return;
            }
            stages.insert(server.key.clone(), Staged::Making);
        }
        let folder = self.backend.stage_folder(server);
        let mut stages = self.stages.lock().unwrap_or_else(PoisonError::into_inner);
        match folder {
            Some(folder) => {
                stages.insert(server.key.clone(), Staged::Ready(folder));
                drop(stages);
                self.backend.stage_ready(&server.key);
            }
            None => {
                stages.remove(&server.key);
            }
        }
    }
}

/// A preparation that asked the person whether to set a server up.
struct Asked {
    /// Where the answer goes; taken when it is given, so that a second one is nobody's.
    reply: Option<Sender<Answer>>,
    /// Set to stop the install that follows.
    stop: Arc<AtomicBool>,
}

/// Everything the loop owns.
struct Core {
    shared: Arc<Shared>,
    hub: Hub,
    conns: BTreeMap<HoldId, Conn>,
    accounts: BTreeMap<String, Account>,
    /// A preparation waits for the holder to run a program.
    waiting: BTreeMap<HoldId, Sender<u8>>,
    /// The preparations that asked the person ([`Msg::Ask`]), until their holder goes.
    asking: BTreeMap<HoldId, Asked>,
    /// Quits waiting for terminals to have ended ([`Service::end_all`]).
    waiters: Vec<Waiter>,
    /// The pane each holder runs in, once it was admitted.
    panes: BTreeMap<HoldId, PaneUuid>,
    /// The panes of this Mac that wait for the account of this Mac to be made.
    local_waiting: Vec<HoldId>,
    /// Who else waits for it ([`Service::when_local`]).
    local_when: Vec<Box<dyn FnOnce(bool) + Send>>,
    /// When the account of this Mac was last refused, and why.
    local_refused: Option<(Instant, Refusal)>,
    /// The terminals of this Mac that were asked for and have not been made.
    awaiting: BTreeMap<HoldId, Awaiting>,
    /// The terminals asked for a pane that was closed while they were on their way: ended when
    /// they come ([`Core::created`]).
    end_on_arrival: BTreeSet<(String, SessionId)>,
    /// The ordinary shells that carry a pane's stream in place of a terminal the person did not
    /// wait for ([`Core::choose_ordinary`]), by the holder of the stream: what the pane types
    /// goes to them.
    ordinaries: BTreeMap<HoldId, ask::Carried>,
    /// The panes that chose an ordinary shell over a terminal found again, which was left
    /// running: the close of such a pane ends the terminal ([`Core::end`]), and so does a quit
    /// that ends the programs ([`Core::end_all`]); any other quit leaves it for the next launch.
    /// Shared with [`Service::left_running`]: the application asks it of a pane, and a pane in it
    /// has no terminal of this Mac to anything that asks.
    left_running: Arc<Mutex<HashSet<PaneUuid>>>,
    /// The program whose view is open in the pane each terminal of this Mac carries, as the
    /// application said it ([`Service::view_changed`], `None`: a view of a program that gave no
    /// name), or as the ledger had it when the terminal was found again: what a line of the log
    /// says a pane showed, read without the ledger's file. Forgotten when the terminal ends
    /// ([`Core::finished`]) or is not there to be found again ([`Core::resumed_gone`]).
    views: HashMap<SessionId, Option<String>>,
    /// What the bridge's readers have sent the loop that it has taken in and not settled
    /// ([`Queue::settled`]), by the queue the reader counted it in.
    arrived: Vec<(Arc<Queue>, usize)>,
    /// What [`Service::shown`] reads.
    shown: Arc<Mutex<HashMap<PaneUuid, Shown>>>,
    /// What [`Service::local_program`] reads.
    local_programs: Arc<Mutex<HashMap<SessionId, String>>>,
    /// The program in front of the terminal of this Mac each pane has, as it was last told
    /// ([`Backend::program_changed`]).
    local_told: HashMap<PaneUuid, String>,
    /// What each account's server last said of its terminals.
    roster: BTreeMap<String, Roster>,
    /// The accounts to ask for their terminals again before anything else is done.
    relist: BTreeSet<String>,
    /// The accounts whose connection came up and have not been listed since
    /// ([`Backend::reachable`]).
    fresh: BTreeSet<String>,
    /// The accounts that were just listed after their connection came up: told
    /// ([`Backend::reachable`]) once the lists are written, so that what the backend reads
    /// the cache for is what the server said.
    reached: Vec<String>,
    /// The terminals in the background the last line to a new terminal named, by account
    /// ([`background_note`]): for as long as the service lives, which is the application's.
    told: BTreeMap<String, Vec<SessionId>>,
    /// The servers a pane that has no dock was told bateri can be set up on ([`Refusal::unasked`]),
    /// by account: once for as long as the service lives, which is the application's.
    unasked: BTreeSet<String>,
    /// What [`Service::background`] and [`Service::elsewhere`] read.
    background: Arc<Mutex<BTreeMap<String, Kept>>>,
    /// What [`Service::exited`] reads.
    exited: Arc<Mutex<BTreeMap<String, Vec<SessionId>>>>,
    /// Something happened that [`Core::shown`] may say differently.
    refresh: bool,
    /// Terminals the next pane to ask for them takes, and when it was said
    /// ([`Service::take_over`]).
    takes: BTreeMap<(String, SessionId), Instant>,
    /// Terminals asked to hang up that are killed if they are still there.
    ending: Vec<Ending>,
    /// The accounts bateri is being taken off the servers of ([`Service::removing`]): nothing is
    /// dialed, renewed or written for them.
    removing: BTreeSet<String>,
    /// What programs on the servers are saying to the application, until each has said
    /// all it had ([`Letter`]).
    drafts: BTreeMap<(String, Talk), Draft>,
    /// The conversations whose letter is out, until each is over ([`Gone`]).
    letters: BTreeMap<(String, Talk), Gone>,
    /// The programs and connections callers asked for that wait for their account's connection
    /// ([`Streams`]).
    opening: Vec<pipes::StreamOpen>,
    /// The ones that are open, by account and stream.
    pipes: BTreeMap<(String, Talk), pipes::Piped>,
    /// How many times each holder's terminal was started again under another number
    /// ([`Core::collided`]).
    collisions: BTreeMap<HoldId, u8>,
    /// The surveys the application is waiting for ([`Service::survey`]).
    surveys: Vec<Surveying>,
    /// The screens the application is waiting for ([`Service::picture`]).
    pictures: Vec<Picturing>,
    /// How many surveys were asked for: the next one's identity.
    surveyed: u64,
    grace: Duration,
    rx: Receiver<Msg>,
    generation: u64,
}

/// A conversation a program is still talking in.
struct Draft {
    from: Option<SessionId>,
    bytes: Vec<u8>,
}

/// A survey that waits for the server's list and for the records.
struct Surveying {
    id: u64,
    account: String,
    due: Instant,
    terminals: Option<Vec<Status>>,
    records: Option<Vec<(Terminal, Record)>>,
    done: Box<dyn FnOnce(Survey) + Send>,
}

/// A screen the application waits for: asked of the server once however many ask.
struct Picturing {
    account: String,
    terminal: SessionId,
    due: Instant,
    done: Box<dyn FnOnce(Option<Screen>) + Send>,
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
                .chain(self.surveys.iter().map(|survey| survey.due))
                .chain(self.pictures.iter().map(|picture| picture.due))
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
                    self.wind_down();
                    return;
                }
                self.handle(message);
                // What else is waiting, before the (costlier) settling.
                while let Ok(more) = self.rx.try_recv() {
                    if matches!(more, Msg::Stop) {
                        self.wind_down();
                        return;
                    }
                    self.handle(more);
                }
            }
            self.settle();
            // What came from the bridge is with the holders now (or gone): the reader may count
            // what is queued for them.
            for (queue, sum) in self.arrived.drain(..) {
                queue.settled(sum);
            }
        }
    }

    /// The panes of `account` have taken nothing of what is queued for them for [`HOLD_STALL`],
    /// and the bridge's stream is not read for it: every terminal of the account stands still,
    /// the answer to a new terminal with them. The panes of this Mac that took nothing in that
    /// time are let go of: their terminals' streams are stopped (the relay does not wait for a
    /// client that is detached, and keeps what the terminal writes in its ring), and what they are
    /// owed no longer counts against the account, so that the others go on. The bytes they are
    /// owed are theirs and stay queued in order; once the pane has taken them all
    /// ([`Core::drained`]) its terminal's stream goes on from where it stopped. A pane that takes
    /// anything, however little, in that time is not touched: it is slow, and holds the stream
    /// back byte by byte.
    fn stalled(&mut self, account: &str, generation: u64, now: Instant) {
        let Some(entry) = self.accounts.get(account) else {
            return;
        };
        if entry.generation != generation {
            return;
        }
        let stuck: Vec<HoldId> = self
            .conns
            .iter()
            .filter(|(hold, conn)| {
                conn.suspended.is_none()
                    && conn.load.stalled(now, HOLD_STALL)
                    && self
                        .hub
                        .terminal_of(**hold)
                        .is_some_and(|(of, _)| of == account)
            })
            .map(|(hold, _)| *hold)
            .collect();
        for hold in stuck {
            if !self.hub.suspend(hold) {
                continue;
            }
            let mut owed = 0;
            if let Some(conn) = self.conns.get_mut(&hold) {
                conn.suspended = Some(now);
                owed = conn.load.uncount_owing();
            }
            // The writer took the last of what the pane was owed while it was being let go of:
            // it is taken back at once, and a pair of lines about a pane that owed nothing would
            // say nothing.
            if owed == 0 {
                self.awaken_saying(hold, false);
                continue;
            }
            let line =
                let_go::let_go_line(&self.pane_of(hold), &self.shows(hold), owed, HOLD_STALL);
            self.shared.backend.log_line(&line);
        }
    }

    /// The pane `hold` runs in, as a line of the log names it.
    fn pane_of(&self, hold: HoldId) -> String {
        self.panes
            .get(&hold)
            .map_or_else(|| "?".to_owned(), |pane| pane.as_str().to_owned())
    }

    /// What the pane of `hold` shows, as a line of the log says it: a plain terminal, or the view
    /// of a program, with its name, as the application last said it ([`Core::views`]). Not the
    /// pane, which is what is not heard from when it is let go of, and not the ledger's file,
    /// which the loop would read and parse under the ledger's lock for every line.
    fn shows(&self, hold: HoldId) -> String {
        let view = self
            .hub
            .terminal_of(hold)
            .and_then(|(_, terminal)| self.views.get(terminal));
        let_go::shows(view.map(Option::as_deref))
    }

    /// A pane that was let go of ([`Core::stalled`]) has taken all it was owed: its terminal's
    /// stream goes on from the byte after the last it was given.
    fn drained(&mut self, hold: HoldId) {
        let Some(conn) = self.conns.get_mut(&hold) else {
            return;
        };
        if conn.suspended.is_some() {
            self.awaken(hold);
        } else {
            // Streaming and not counted ([`Core::awaken_to_end`]): it counts again.
            conn.load.recount();
        }
    }

    /// A pane that was let go of ([`Core::stalled`]) is streamed to again: the terminal is
    /// attached from the byte after the last the pane was given, and what the pane is owed counts
    /// against the account again (it has taken all it was owed: [`Core::drained`]).
    fn awaken(&mut self, hold: HoldId) {
        self.awaken_saying(hold, true);
    }

    /// [`Core::awaken`], with the line of the return when `say` (the pane's going was said).
    fn awaken_saying(&mut self, hold: HoldId, say: bool) {
        let Some(conn) = self.conns.get_mut(&hold) else {
            return;
        };
        let Some(since) = conn.suspended.take() else {
            return;
        };
        conn.load.recount();
        self.hub.awake(hold);
        if !say {
            return;
        }
        let line = let_go::taken_back_line(
            &self.pane_of(hold),
            &self.shows(hold),
            Instant::now().saturating_duration_since(since),
            let_go::Back::Read,
        );
        self.shared.backend.log_line(&line);
    }

    /// Says `say` to the terminal of `hold`. A pane that was let go of ([`Core::stalled`]) has
    /// something to say all the same — a key, a size, a request about its past or the input its
    /// program left unread —, and the server serves what a client says about a terminal only
    /// while it is attached: its stream is asked for, `say` is said behind the attach, and the
    /// stream is stopped again at once, in that order on the one connection, so the server
    /// serves them in it. Nothing counts against the account and nothing streams to a pane that
    /// reads nothing: what the server sends between the attach and the stop is not taken
    /// ([`Client::suspend`]) and is sent again from the byte owed when the pane reads. What the
    /// pane typed and the server has not taken is kept, and goes the same way when the answers
    /// that open its window come ([`Core::pump_suspended`]).
    fn say(&mut self, hold: HoldId, say: impl FnOnce(&mut Hub)) {
        let let_go = self
            .conns
            .get(&hold)
            .is_some_and(|conn| conn.suspended.is_some());
        if !let_go || !self.hub.awake_to_say(hold) {
            // Said as it is: the connection is down, and what cannot be said is told as always.
            say(&mut self.hub);
            return;
        }
        say(&mut self.hub);
        self.hub.suspend(hold);
    }

    /// [`Core::say`] for the holder that shows `terminal` of `account`, if one does.
    fn say_to_terminal(&mut self, account: &str, terminal: &SessionId, say: impl FnOnce(&mut Hub)) {
        match self.hub.hold_of(account, terminal) {
            Some(hold) => self.say(hold, say),
            None => say(&mut self.hub),
        }
    }

    /// The terminal of `account` is to end ([`Core::finish_off`]) and the pane that shows it is
    /// to be told when it has, whether the pane reads or not: the terminal streams to it, and
    /// what the pane is owed does not count against the account — a pane that took nothing could
    /// otherwise hold back the very stream that carries the end of its terminal, and be let go of
    /// again before the end came. It counts again when the pane has taken all it was owed.
    fn awaken_to_end(&mut self, account: &str, terminal: &SessionId) {
        let Some(hold) = self.hub.hold_of(account, terminal) else {
            return;
        };
        let Some(conn) = self.conns.get_mut(&hold) else {
            return;
        };
        let nothing_owed = conn.load.uncount();
        if let Some(since) = conn.suspended.take() {
            self.hub.awake(hold);
            let line = let_go::taken_back_line(
                &self.pane_of(hold),
                &self.shows(hold),
                Instant::now().saturating_duration_since(since),
                let_go::Back::ToEnd,
            );
            self.shared.backend.log_line(&line);
        }
        if nothing_owed {
            self.drained(hold);
        }
    }

    /// The panes of `account` that are let go of and have typed bytes waiting for the window,
    /// which the server's answers have opened since: the next of it is sent ([`Core::say`]).
    fn pump_suspended(&mut self, account: &str) {
        let ready: Vec<HoldId> = self
            .conns
            .iter()
            .filter(|(hold, conn)| conn.suspended.is_some() && self.hub.input_ready(**hold))
            .map(|(hold, _)| *hold)
            .filter(|hold| {
                self.hub
                    .terminal_of(*hold)
                    .is_some_and(|(of, _)| of == account)
            })
            .collect();
        for hold in ready {
            self.say(hold, |_| {});
        }
    }

    /// Shuts a holder's connection to reading while too much of what it typed waits for the
    /// server, and opens it again when little does.
    fn meter(&self, hold: HoldId) {
        let Some(conn) = self.conns.get(&hold) else {
            return;
        };
        let waiting = self.hub.input_waiting(hold);
        if waiting >= INPUT_HOLD {
            conn.throttle.0.shut();
        } else if waiting <= INPUT_RESUME && !conn.waits_for_input {
            conn.throttle.0.open();
        }
    }

    /// Writes the account's server's files again, on a thread of their own, one write of an
    /// account after the other. Not while its kablo is an earlier release's (`stale`): the command
    /// file would name a binary that has no `bateri` in it and undo what the renewal of the first
    /// dial writes. `connected` leaves a server that is not connected alone (this Mac is always
    /// there): no connection is made for a file, and what the setting asks is written by the first
    /// new terminal's step ([`Backend::files`]), since `files` is false again either way.
    fn rewrite(&mut self, account: &str, connected: bool) {
        if self.removing.contains(account) {
            return;
        }
        let Some(entry) = self.accounts.get_mut(account) else {
            return;
        };
        entry.files = false;
        let here = local::is_account(&entry.server.key);
        if entry.stale || (connected && !here && entry.bridge.is_none()) {
            return;
        }
        let (shared, server, installed, writing) = (
            Arc::clone(&self.shared),
            entry.server.clone(),
            entry.installed.clone(),
            Arc::clone(&entry.writing),
        );
        let _ = thread::Builder::new()
            .name("ssh-hold writes the files".into())
            .spawn(move || {
                let _one_at_a_time = writing.lock().unwrap_or_else(PoisonError::into_inner);
                shared.backend.files(&server, &installed);
            });
    }

    fn handle(&mut self, message: Msg) {
        let now = Instant::now();
        match message {
            Msg::Accepted(stream, peer) => self.accepted(stream, peer),
            Msg::Hold(hold, bytes) => {
                // The pane's stream is carried by an ordinary shell: what is typed is its input.
                if let Some(carried) = self.ordinaries.get(&hold) {
                    carried.shell.write(bytes);
                } else {
                    self.say(hold, |hub| hub.push(hold, &bytes));
                    self.meter(hold);
                }
            }
            Msg::HoldEof(hold) => {
                self.ordinaries.remove(&hold);
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
            Msg::Ask { hold, reply, stop } => {
                // A holder that is gone is asking no one: the wait ends, and so does the install.
                if self.conns.contains_key(&hold) {
                    self.asking.insert(
                        hold,
                        Asked {
                            reply: Some(reply),
                            stop,
                        },
                    );
                } else {
                    stop.store(true, Ordering::SeqCst);
                }
            }
            Msg::Answered(pane, answer) => self.answered(&pane, answer),
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
            Msg::Numbered {
                hold,
                account,
                spec,
                number,
                filed,
            } => {
                self.refresh = true;
                if filed && let Some(entry) = self.accounts.get_mut(&account) {
                    entry.files = true;
                }
                if self
                    .started(hold, &account, spec, terminal_named(number))
                    .is_err()
                {
                    self.not_started(hold, &account);
                }
            }
            Msg::End(pane) => self.end(&pane, now),
            Msg::Leave(pane) => self.leave(&pane, now),
            Msg::Removing(account) => {
                self.removing.insert(account);
            }
            Msg::RemovalFailed(account) => {
                self.removing.remove(&account);
            }
            Msg::Removed(account) => self.removed(&account),
            Msg::WhenLocal(done) => self.when_local(done, now),
            Msg::Forget(pane) => {
                for (account, terminal) in self.terminals_of(&pane) {
                    self.say_to_terminal(&account, &terminal, |hub| {
                        if let Some(client) = hub.client_mut(&account) {
                            client.forget_past(&terminal);
                        }
                    });
                }
                self.refresh = true;
            }
            Msg::TakeOver { account, terminal } => {
                let now = Instant::now();
                self.takes
                    .retain(|_, at| now.saturating_duration_since(*at) < TAKE_WAIT);
                self.takes.insert((account, terminal), now);
            }
            Msg::BringBack(pane) => self.bring_back(&pane),
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
                queue,
            } => {
                // Counted as arrived (and not yet as queued for a holder) until the loop has
                // settled, whether it keeps the bytes or not.
                match self
                    .arrived
                    .iter_mut()
                    .find(|(of, _)| Arc::ptr_eq(of, &queue))
                {
                    Some((_, sum)) => *sum += bytes.len(),
                    None => self.arrived.push((Arc::clone(&queue), bytes.len())),
                }
                if self
                    .accounts
                    .get(&account)
                    .is_some_and(|a| a.generation == generation && a.bridge.is_some())
                    && let Some(client) = self.hub.client_mut(&account)
                {
                    client.push(&bytes, now);
                    // The answers that open the window of what a pane let go of typed.
                    self.pump_suspended(&account);
                }
            }
            Msg::Stalled {
                account,
                generation,
            } => self.stalled(&account, generation, now),
            Msg::Drained(hold) => self.drained(hold),
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
            Msg::StreamOpen(open) => self.stream_wanted(open),
            Msg::StreamLive { account, reply } => {
                let _ = reply.send(self.stream_live(&account));
            }
            Msg::StreamTook {
                account,
                talk,
                bytes,
            } => self.stream_took(&account, talk, bytes),
            // What it did is moved on when the service settles, which every message ends with.
            Msg::StreamWrote => {}
            Msg::StreamHangUp { account, talk } => self.stream_hung_up(&account, talk),
            Msg::Retry(account) => {
                if let Some(client) = self.hub.client_mut(&account) {
                    client.retry_now(now);
                }
                // A login that was waited for is a connection being made.
                self.refresh = true;
            }
            Msg::Rewrite(account) => self.rewrite(&account, false),
            Msg::RewriteAll => {
                let accounts: Vec<String> = self.accounts.keys().cloned().collect();
                for account in accounts {
                    self.rewrite(&account, true);
                }
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
            #[cfg(test)]
            Msg::Counts(tx) => {
                let _ = tx.send(Counts {
                    panes: self.panes.len(),
                    views: self.views.len(),
                });
            }
            Msg::Answer {
                account,
                talk,
                bytes,
            } => {
                if let Some(client) = self.hub.client_mut(&account) {
                    client.say(talk, &bytes);
                    client.finish(talk);
                }
                self.conversation_over(&account, talk);
            }
            Msg::Hang { account, talk } => {
                if let Some(client) = self.hub.client_mut(&account) {
                    client.hang_up(talk);
                }
                self.conversation_over(&account, talk);
            }
            Msg::Here(pending) => self.here(pending),
            Msg::Local(ask) => self.local(ask, now),
            Msg::Ordinary(ticket) => {
                let hold = self
                    .awaiting
                    .iter()
                    .find(|(_, awaiting)| Arc::ptr_eq(&awaiting.parts.ticket, &ticket))
                    .map(|(hold, _)| *hold);
                if let Some(hold) = hold {
                    self.choose_ordinary(hold);
                }
            }
            Msg::View { terminal, program } => match program {
                Some(program) => {
                    self.views.insert(terminal, program);
                }
                None => {
                    self.views.remove(&terminal);
                }
            },
            Msg::LocalNumbered { hold, number } => {
                if let Some(awaiting) = self.awaiting.get_mut(&hold) {
                    awaiting.parts.name = terminal_named(number);
                    self.local_open(hold, None);
                }
            }
            Msg::LocalAccount(outcome) => self.local_account(&outcome, now),
            Msg::Resize { hold, size } => self.say(hold, |hub| hub.resize(hold, size)),
            Msg::DropInput { hold, remote } => self.drop_input(hold, remote),
            Msg::DropShown(pane) => self.drop_shown(&pane),
            Msg::HereMade { outcome, pending } => self.here_made(outcome, &pending, now),
            Msg::Survey { account, done } => self.survey(account, done, now),
            Msg::Picture {
                account,
                terminal,
                done,
            } => self.picture(account, terminal, done, now),
            Msg::Recorded { id, records } => {
                if let Some(survey) = self.surveys.iter_mut().find(|survey| survey.id == id) {
                    survey.records = Some(records);
                }
                self.finish_surveys(now);
            }
            Msg::Stop => {}
        }
    }

    fn accepted(&mut self, stream: UnixStream, peer: Peer) {
        let Ok(reader) = stream.try_clone() else {
            return;
        };
        let hold = self.hub.open();
        self.carry_hold(hold, stream, reader, peer, None, false);
    }

    /// The threads that carry a holder's bytes: one reads what it says, one writes what the hub
    /// has for it. A pane of this Mac has the `status` its program's end is written in, and
    /// `after`, the peer of the pane's exit signal, which the writer closes once the last byte
    /// is written.
    fn carry_hold(
        &mut self,
        hold: HoldId,
        stream: UnixStream,
        reader: UnixStream,
        peer: Peer,
        local: Option<(Arc<StreamExit>, UnixStream)>,
        waits_for_input: bool,
    ) {
        let (out, written) = mpsc::channel();
        let (status, after) = match local {
            Some((status, after)) => (Some(status), Some(after)),
            None => (None, None),
        };
        let gate = Arc::new(Gate::default());
        if waits_for_input {
            gate.shut();
        }
        let throttle = Throttle(Arc::clone(&gate));
        self.conns.insert(
            hold,
            Conn {
                peer,
                out,
                throttle,
                status,
                load: Load::new(),
                suspended: None,
                waits_for_input,
            },
        );
        let tx = self.shared.tx.clone();
        let writes = tx.clone();
        let _ = thread::Builder::new()
            .name("ssh-hold reads".into())
            .spawn(move || hold_reader(hold, reader, &tx, &gate));
        let _ = thread::Builder::new()
            .name("ssh-hold writes".into())
            .spawn(move || hold_writer(hold, stream, &written, after, &writes));
    }

    /// Everything that follows from what was just heard: the clients move,
    /// the hub's events are answered, bytes go out, bridges are started or
    /// let go of — until nothing changes.
    fn settle(&mut self) {
        // The server may have taken what held a pane's connection shut.
        let shut: Vec<HoldId> = self
            .conns
            .iter()
            .filter(|(_, conn)| conn.throttle.0.is_shut())
            .map(|(hold, _)| *hold)
            .collect();
        for hold in shut {
            self.meter(hold);
        }
        self.kill_due(Instant::now());
        self.open_gates();
        self.finish_surveys(Instant::now());
        self.finish_pictures(Instant::now());
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
                    HubEvent::Collided {
                        hold,
                        account,
                        terminal,
                        spec,
                    } => self.collided(hold, &account, &terminal, *spec),
                    HubEvent::Ran { hold, status } => {
                        if let Some(reply) = self.waiting.remove(&hold) {
                            let _ = reply.send(status);
                        }
                    }
                    HubEvent::Gone { hold, showed } => {
                        // A pane that waited for its terminal when its holder went: the pane is
                        // given an ordinary shell if it has no holder yet, and a terminal that
                        // was asked for it is ended when it comes.
                        if let Some(awaiting) = self.awaiting.remove(&hold) {
                            self.local_gone(awaiting);
                        }
                        self.waiting.remove(&hold);
                        // A person who is asked, or whose install is on its way, is asked no
                        // more: the question ends, and the install is stopped.
                        if let Some(asked) = self.asking.remove(&hold) {
                            asked.stop.store(true, Ordering::SeqCst);
                        }
                        self.collisions.remove(&hold);
                        self.ordinaries.remove(&hold);
                        self.conns.remove(&hold);
                        self.panes.remove(&hold);
                        // This Mac was attached to the terminal until now, and what the server
                        // counted of who is attached has it in: not believed until the server
                        // answers again (the question is asked just below).
                        if let Some((account, terminal)) = showed {
                            self.roster.entry(account).or_default().attached(&terminal);
                        }
                        // A holder that goes lets go of its terminal, which lives on unless
                        // it was asked to end: the server is asked what it has now.
                        self.relist.extend(self.accounts.keys().cloned());
                    }
                    HubEvent::Client { account, event } => {
                        self.roster
                            .entry(account.clone())
                            .or_default()
                            .apply(&event);
                        match &*event {
                            Event::Listed(list) => {
                                self.found(&account, list);
                                self.listed_for_surveys(&account, list);
                            }
                            Event::Lost { cause } => {
                                self.lost_for_surveys(&account);
                                self.answer_pictures(&account, None, None);
                                self.lost(&account, *cause);
                            }
                            Event::Screen {
                                terminal,
                                offset,
                                cols,
                                rows,
                                data,
                            } => self.answer_pictures(
                                &account,
                                Some(terminal),
                                Some(Screen {
                                    offset: *offset,
                                    cols: *cols,
                                    rows: *rows,
                                    bytes: data.clone(),
                                }),
                            ),
                            // The server will not give the screen: it has no such terminal, or
                            // could not ask the program.
                            Event::Failed {
                                terminal: Some(terminal),
                                ..
                            } => self.answer_pictures(&account, Some(terminal), None),
                            _ => {}
                        }
                        match *event {
                            // A connection that came up is asked for its terminals.
                            Event::Linked => {
                                if let Some(entry) = self.accounts.get_mut(&account) {
                                    entry.missed = 0;
                                }
                                self.fresh.insert(account.clone());
                                self.relist.insert(account);
                            }
                            // The first answer after a connection came up: the roster is as
                            // the server has it, and what was left for the application can be
                            // read against it.
                            Event::Listed(_) if self.fresh.remove(&account) => {
                                self.reached.push(account);
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
                            Event::Created(status) => self.created(&account, &status, now),
                            Event::Ticket { ticket, from } => self.ticketed(&account, ticket, from),
                            Event::TalkOpened { talk } => self.stream_opened(&account, talk),
                            Event::TalkData { talk, data } => {
                                self.talked(&account, talk, &data);
                                self.stream_bytes(&account, talk, data, false);
                            }
                            Event::TalkStderr { talk, data } => {
                                self.stream_bytes(&account, talk, data, true);
                            }
                            Event::TalkEof { talk } => {
                                self.said_all(&account, talk);
                                self.stream_eof(&account, talk);
                            }
                            Event::TalkExited { talk, exit } => {
                                self.stream_exited(&account, talk, exit);
                            }
                            // Over before it said all it had: nothing was handed to anybody.
                            // Over after: what the application started for the letter stops.
                            Event::TalkOver { talk, over } => {
                                self.drafts.remove(&(account.clone(), talk));
                                self.conversation_over(&account, talk);
                                self.stream_over(&account, talk, &over);
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
            moved |= self.start_streams(now);
            moved |= self.pump_streams();
            if !moved {
                break;
            }
        }
        self.write_shown();
        for account in std::mem::take(&mut self.reached) {
            self.reach(&account);
        }
    }

    /// [`Backend::reachable`] for `account`, on a thread of its own: it may wait on the server.
    fn reach(&self, account: &str) {
        let Some(entry) = self.accounts.get(account) else {
            return;
        };
        let (backend, server, account) = (
            Arc::clone(&self.shared.backend),
            entry.server.clone(),
            account.to_owned(),
        );
        let _ = thread::Builder::new()
            .name("ssh-hold reads the server's notes".into())
            .spawn(move || backend.reachable(&account, &server));
    }

    /// [`Service::survey`]: the server is asked for its list now and the records are read on a
    /// thread of their own; an account that is not connected is answered at once with nothing.
    fn survey(&mut self, account: String, done: Box<dyn FnOnce(Survey) + Send>, now: Instant) {
        let live = self
            .hub
            .client(&account)
            .is_some_and(|client| matches!(client.link(), Link::Live));
        let Some(entry) = self.accounts.get(&account).filter(|_| live) else {
            done(Survey::default());
            return;
        };
        self.surveyed += 1;
        let id = self.surveyed;
        let (backend, server, tx) = (
            Arc::clone(&self.shared.backend),
            entry.server.clone(),
            self.shared.tx.clone(),
        );
        let _ = thread::Builder::new()
            .name("ssh-hold reads who opened what".into())
            .spawn(move || {
                let records = backend.records(&server);
                let _ = tx.send(Msg::Recorded { id, records });
            });
        self.relist.insert(account.clone());
        self.surveys.push(Surveying {
            id,
            account,
            due: now + SURVEY_WAIT,
            terminals: None,
            records: None,
            done,
        });
    }

    /// [`Service::picture`]: the server is asked once for the screen however many ask; a
    /// connection that is not up answers at once with nothing.
    fn picture(
        &mut self,
        account: String,
        terminal: SessionId,
        done: Box<dyn FnOnce(Option<Screen>) + Send>,
        now: Instant,
    ) {
        let asked = self
            .pictures
            .iter()
            .any(|picture| picture.account == account && picture.terminal == terminal);
        if !asked
            && !self
                .hub
                .client_mut(&account)
                .is_some_and(|client| client.read_picture(&terminal))
        {
            done(None);
            return;
        }
        self.pictures.push(Picturing {
            account,
            terminal,
            due: now + PICTURE_WAIT,
            done,
        });
        self.refresh = true;
    }

    /// The server answered (`screen`) or refused (`None`) for `terminal` of `account` — or, with
    /// no terminal, the connection to the account went: those who wait for it are told.
    fn answer_pictures(
        &mut self,
        account: &str,
        terminal: Option<&SessionId>,
        screen: Option<Screen>,
    ) {
        let mut kept = Vec::new();
        for picture in std::mem::take(&mut self.pictures) {
            if picture.account == account && terminal.is_none_or(|t| *t == picture.terminal) {
                (picture.done)(screen.clone());
            } else {
                kept.push(picture);
            }
        }
        self.pictures = kept;
    }

    /// The screens whose time is out are answered with nothing.
    fn finish_pictures(&mut self, now: Instant) {
        let mut kept = Vec::new();
        for picture in std::mem::take(&mut self.pictures) {
            if picture.due <= now {
                (picture.done)(None);
            } else {
                kept.push(picture);
            }
        }
        self.pictures = kept;
    }

    /// The server's list came: the surveys of the account that wait for it have it.
    fn listed_for_surveys(&mut self, account: &str, list: &[Status]) {
        for survey in &mut self.surveys {
            if survey.account == account && survey.terminals.is_none() {
                survey.terminals = Some(list.to_vec());
            }
        }
        self.finish_surveys(Instant::now());
    }

    /// The connection to the account went: the surveys that wait for its list are answered with
    /// what they have.
    fn lost_for_surveys(&mut self, account: &str) {
        let mut over = Vec::new();
        let mut kept = Vec::new();
        for survey in std::mem::take(&mut self.surveys) {
            if survey.account == account && survey.terminals.is_none() {
                over.push(survey);
            } else {
                kept.push(survey);
            }
        }
        self.surveys = kept;
        for survey in over {
            (survey.done)(Survey {
                terminals: None,
                records: survey.records.unwrap_or_default(),
            });
        }
    }

    /// The surveys that have the list and the records, or whose time is out, are answered.
    fn finish_surveys(&mut self, now: Instant) {
        let mut kept = Vec::new();
        for survey in std::mem::take(&mut self.surveys) {
            let whole = survey.terminals.is_some() && survey.records.is_some();
            if whole || survey.due <= now {
                (survey.done)(Survey {
                    terminals: survey.terminals,
                    records: survey.records.unwrap_or_default(),
                });
            } else {
                kept.push(survey);
            }
        }
        self.surveys = kept;
    }

    /// A program on the server of `account` wants the application: taken at once
    /// when the backend takes mail, declined with a line the program shows otherwise.
    fn ticketed(&mut self, account: &str, ticket: u32, from: Option<SessionId>) {
        let takes = self.shared.backend.takes_mail();
        let Some(client) = self.hub.client_mut(account) else {
            return;
        };
        if !takes {
            client.decline(ticket, "bateri is not taking requests from programs now");
            return;
        }
        // All the connection's streams in use: the program is told, not left to wait out its ten
        // seconds for nothing.
        let Some(talk) = client.accept(ticket) else {
            client.decline(ticket, "bateri has too many requests going on now");
            return;
        };
        self.drafts.insert(
            (account.to_owned(), talk),
            Draft {
                from,
                bytes: Vec::new(),
            },
        );
    }

    /// The program said more. A program that says more than a request is ever
    /// hung up on.
    fn talked(&mut self, account: &str, talk: Talk, data: &[u8]) {
        let key = (account.to_owned(), talk);
        let Some(draft) = self.drafts.get_mut(&key) else {
            return;
        };
        draft.bytes.extend_from_slice(data);
        if draft.bytes.len() > LETTER_LIMIT {
            self.drafts.remove(&key);
            if let Some(client) = self.hub.client_mut(account) {
                client.hang_up(talk);
            }
        }
    }

    /// The conversation of a letter is over, however: what waits for that runs.
    fn conversation_over(&mut self, account: &str, talk: Talk) {
        if let Some(gone) = self.letters.remove(&(account.to_owned(), talk)) {
            gone.end();
        }
    }

    /// The program has said all it had: the application gets it, whole.
    fn said_all(&mut self, account: &str, talk: Talk) {
        let Some(draft) = self.drafts.remove(&(account.to_owned(), talk)) else {
            return;
        };
        let (host, argv) = self
            .accounts
            .get(account)
            .map(|entry| (entry.server.host.clone(), entry.server.argv.clone()))
            .unwrap_or_default();
        let gone = Gone::new();
        self.letters
            .insert((account.to_owned(), talk), gone.clone());
        self.shared.backend.mail(Letter {
            account: account.to_owned(),
            from: draft.from,
            host,
            argv,
            bytes: draft.bytes,
            tx: self.shared.tx.clone(),
            talk,
            answered: false,
            gone,
        });
    }

    /// What each pane shows, written where [`Service::shown`] reads it. Only
    /// when something happened that could change it.
    fn write_shown(&mut self) {
        if !std::mem::take(&mut self.refresh) {
            return;
        }
        let mut shown = HashMap::new();
        let mut local_programs = HashMap::new();
        let mut local_told = HashMap::new();
        for (hold, pane) in &self.panes {
            let Some((account, terminal)) = self.hub.terminal_of(*hold) else {
                continue;
            };
            let program: Option<String> = self
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
            // The Mac's own terminals are no server's: what asks for a server's terminal of a
            // pane does not find one. A pane can have both — its program is the pipe to a
            // server's terminal and runs in a terminal of this Mac — and what it *shows* is the
            // server's: the two are two handles, this one carries the bytes, and the writer
            // leaves it out, so which of the two holds is looked at last decides nothing. What a
            // program asks of the app (`bateri`) needs to know the program in front of it, and
            // that is kept apart.
            if local::is_account(account) {
                if let Some(program) = program {
                    local_told.insert(pane.clone(), program.clone());
                    local_programs.insert(terminal.clone(), program);
                }
                continue;
            }
            let (host, argv) = self
                .accounts
                .get(account)
                .map(|entry| (entry.server.host.clone(), entry.server.argv.clone()))
                .unwrap_or_default();
            // Passive is "was shown and is not live any more": until the
            // terminal was shown the connection being made is the first one.
            let client = self.hub.client(account);
            let taken = client.is_some_and(|client| client.is_taken(terminal));
            let link = match client.map(Client::link) {
                // Whatever the connection does, and even before the terminal was shown (a
                // pane that came back to a terminal another computer has by now), the terminal
                // is another computer's.
                _ if taken => PaneLink::Taken,
                _ if !self.hub.is_shown(*hold) => PaneLink::Connecting,
                Some(Link::NeedsLogin) => PaneLink::NeedsLogin,
                Some(Link::Retrying { .. } | Link::Opening { .. }) => PaneLink::Reconnecting,
                // A screen is not back until its terminal is: whatever waits for that edge (a
                // program's view, the shell's mirror of its line) writes to it at once, and what
                // reaches a terminal that is not served yet is let go of.
                Some(Link::Live)
                    if client.is_some_and(|client| client.is_coming_back(terminal)) =>
                {
                    PaneLink::Reconnecting
                }
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
                    program_fresh: client.is_some_and(|client| client.state_is_fresh(terminal)),
                    link,
                    taken_by: client
                        .and_then(|client| client.taken_by(terminal))
                        .map(str::to_owned),
                },
            );
        }
        *self
            .local_programs
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = local_programs;
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
        // The icon follows the kind of the program, not its name, and a pane that came or went
        // is a pane whose terminal had a kind or has one.
        let kind_of = |shown: &HashMap<PaneUuid, Shown>, pane: &PaneUuid| {
            shown.get(pane).and_then(Shown::kind)
        };
        // The server's first word on what is in front since the terminal was attached is news
        // whatever the kind, the same program's too: a pane that owes a program's view waits for
        // it to know whether the program is still there.
        let said = |shown: &HashMap<PaneUuid, Shown>, pane: &PaneUuid| {
            shown
                .get(pane)
                .is_some_and(|shown| shown.program_fresh && shown.program.is_some())
        };
        for pane in shown
            .keys()
            .chain(before.keys().filter(|p| !shown.contains_key(*p)))
        {
            if kind_of(&before, pane) != kind_of(&shown, pane)
                || (said(&shown, pane) && !said(&before, pane))
            {
                self.shared.backend.program_changed(pane);
            }
        }
        // A terminal of this Mac has no kind to tell, but what is in front of it is news too: a
        // program's view that waits for the person is over when the program is.
        let told = std::mem::replace(&mut self.local_told, local_told);
        for pane in self.local_told.keys() {
            if told.get(pane) != self.local_told.get(pane) {
                self.shared.backend.program_changed(pane);
            }
        }
        for pane in told
            .keys()
            .filter(|pane| !self.local_told.contains_key(*pane))
        {
            self.shared.backend.program_changed(pane);
        }
        self.write_background();
    }

    /// What the accounts keep in the background, written where [`Service::background`] reads
    /// it: what each server last said of its terminals, less those a holder is bound to (a pane
    /// that shows it, or is about to — a terminal still being brought up is not in the
    /// background for a moment) and those asked to end.
    fn write_background(&mut self) {
        let mut background: BTreeMap<String, Kept> = BTreeMap::new();
        let mut exited: BTreeMap<String, Vec<SessionId>> = BTreeMap::new();
        for (account, roster) in &self.roster {
            // What this Mac's kablo keeps is the user's own business (`kablo list`), not a
            // server's background to list.
            if local::is_account(account) {
                continue;
            }
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
            let (host, argv) = (&entry.server.host, &entry.server.argv);
            let (waiting, away) = roster.split(account, host, argv, hidden);
            let gone = roster.exited(hidden);
            if !gone.is_empty() {
                exited.insert(account.clone(), gone);
            }
            let kept = Kept {
                background: waiting,
                elsewhere: away,
            };
            if kept != Kept::default() {
                background.insert(account.clone(), kept);
            }
        }
        *self.exited.lock().unwrap_or_else(|e| e.into_inner()) = exited;
        let before = std::mem::replace(
            &mut *self.background.lock().unwrap_or_else(|e| e.into_inner()),
            background.clone(),
        );
        // Told after the lock is let go of, once per account whose lists are not what they were.
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

    /// The program of the pane `hold` belongs to ended with input unread: the terminal it ran
    /// in is asked to drop it. A program of this Mac ran in the terminal that carries the
    /// pane's bytes. A program of a server ran in the terminal the pane shows there, and only
    /// that one is asked ([`Core::drop_shown`]): the pipe in the terminal of this Mac
    /// ([`crate::hold`]) reads what is typed as soon as it comes and passes it on, so nothing
    /// waits there for the program, and a drop there could only take keys the person is typing
    /// for the shell on the server. A pane in a remote session that shows no server's terminal
    /// (a plain `ssh`) has nothing to ask.
    ///
    /// The request for the server's terminal is ordered behind what the pipe had passed on; the
    /// one for the terminal of this Mac is made at once. What the pane wrote a moment before
    /// reaches the service from the stream's reader thread, and the request from this one, so a
    /// byte still on its way between the two follows the request and is not dropped; the window
    /// is the hand-off of a socket to the service's thread, and the end of the program that
    /// triggers the request comes back from the terminal after a round trip.
    fn drop_input(&mut self, hold: HoldId, remote: bool) {
        if remote {
            if let Some(pane) = self.panes.get(&hold).cloned() {
                self.drop_shown(&pane);
            }
            return;
        }
        let local = self
            .hub
            .terminal_of(hold)
            .map(|(account, terminal)| (account.to_owned(), terminal.clone()));
        if let Some((account, terminal)) = local {
            self.say(hold, |hub| {
                if let Some(client) = hub.client_mut(&account) {
                    client.drop_input(&terminal);
                }
            });
        }
    }

    /// The server terminals `pane` shows are asked to drop the input their programs have not
    /// read, each behind what its holder had read of what was typed ([`Hub::drop_input`]).
    fn drop_shown(&mut self, pane: &PaneUuid) {
        for (account, terminal) in self.shown_terminals_of(pane) {
            self.hub.drop_input(&account, &terminal);
        }
    }

    /// The terminals `pane` shows that a server keeps: the account of this Mac is not one
    /// ([`local::is_account`]) — its terminal carries the pane's bytes, it is not what the pane
    /// shows, and a request about the terminal a pane *shows* does not reach it.
    fn shown_terminals_of(&self, pane: &PaneUuid) -> Vec<(String, SessionId)> {
        let mut terminals = self.terminals_of(pane);
        terminals.retain(|(account, _)| !local::is_account(account));
        terminals
    }

    /// The user closed `pane`: the terminal it shows is asked to hang up, and
    /// killed after the grace if it is still there. A connection that is down
    /// cannot ask; the ledger keeps the request and the next connection to the
    /// account carries it out.
    fn end(&mut self, pane: &PaneUuid, now: Instant) {
        let (coming, unbound) = self.end_coming(pane);
        // A pane that chose an ordinary shell over a terminal found again ends that terminal too:
        // closing a pane ends its terminal, and this one is the pane's still.
        let left = self.left_by(pane);
        for (account, terminal) in self
            .terminals_of(pane)
            .into_iter()
            .chain(unbound)
            .chain(left)
        {
            // A terminal another computer took is that computer's: closing the dimmed pane that
            // showed it hangs up nothing.
            if self.is_taken(&account, &terminal) {
                self.shared.ledger.gone(&account, &terminal);
                continue;
            }
            self.shared.ledger.end(&account, &terminal);
            if !coming.contains(&terminal) {
                self.finish_off(&account, &terminal, now);
            }
        }
    }

    /// An account made in this process before the ledger had it (the file was lost, or its last
    /// row went) is written down again, so that the next restart can show its terminals.
    fn remember_account(&self, account: &str) {
        if self.shared.ledger.known(account).is_none()
            && let Some(entry) = self.accounts.get(account)
        {
            self.shared.ledger.remember(&entry.server, &entry.installed);
        }
    }

    /// Whether another computer took the terminal from the pane that showed it.
    fn is_taken(&self, account: &str, terminal: &SessionId) -> bool {
        self.hub
            .client(account)
            .is_some_and(|client| client.is_taken(terminal))
    }

    /// ⏎ in a pane whose terminal was taken ([`Service::bring_back`]): taken back from where the
    /// pane left off. The ledger writes the terminal down again — it is this Mac's to end once
    /// more — and the pane is no longer dimmed once the next pass writes what it shows.
    fn bring_back(&mut self, pane: &PaneUuid) {
        for (account, terminal) in self.shown_terminals_of(pane) {
            let Some(client) = self.hub.client_mut(&account) else {
                continue;
            };
            if client.take_back(&terminal) {
                // The ledger forgot the account with its last row when the terminal was taken.
                self.remember_account(&account);
                self.shared.ledger.shown(&account, &terminal, Some(pane));
                self.roster.entry(account).or_default().attached(&terminal);
                self.refresh = true;
            }
        }
    }

    /// The user closed `pane` and left its terminal running ([`Service::leave`]): nothing is
    /// asked of the server, and the ledger forgets the terminal.
    fn leave(&mut self, pane: &PaneUuid, now: Instant) {
        for (account, terminal) in self.shown_terminals_of(pane) {
            self.shared.ledger.gone(&account, &terminal);
        }
        // The pane's own terminal of this Mac is not left running: it carried the pipe to the
        // terminal the user chose to leave, and the pane that would show it is gone.
        let carriers: Vec<_> = self
            .terminals_of(pane)
            .into_iter()
            .filter(|(account, _)| local::is_account(account))
            .collect();
        for (account, terminal) in carriers {
            self.shared.ledger.end(&account, &terminal);
            self.finish_off(&account, &terminal, now);
        }
    }

    /// [`Service::removed`]: the account goes, and with it everything that waited on it.
    fn removed(&mut self, account: &str) {
        self.removing.remove(account);
        self.shared.ledger.forget_account(account);
        self.ending.retain(|ending| ending.account != account);
        for waiter in &mut self.waiters {
            waiter.pending.retain(|(owner, _)| owner != account);
            if waiter.pending.is_empty() {
                let _ = waiter.done.send(());
            }
        }
        self.waiters.retain(|waiter| !waiter.pending.is_empty());
        self.hub
            .drop_account(account, "bateri: bateri was removed from this server");
        self.accounts.remove(account);
        self.roster.remove(account);
        self.write_background();
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
            let (coming, unbound) = self.end_coming(pane);
            // A terminal found again that the pane chose an ordinary shell over was left running:
            // a quit that ends the programs ends it.
            let left = self.left_by(pane);
            for (account, terminal) in self
                .terminals_of(pane)
                .into_iter()
                .chain(unbound)
                .chain(left)
            {
                if self.is_taken(&account, &terminal) {
                    self.shared.ledger.gone(&account, &terminal);
                    continue;
                }
                self.shared.ledger.end(&account, &terminal);
                if coming.contains(&terminal) {
                    continue;
                }
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

    /// The terminal found again that `pane` left running when it chose an ordinary shell
    /// ([`Core::choose_ordinary`]), taken out of [`Core::left_running`]: what the pane's close
    /// and a quit that ends the programs end with the pane's other terminals.
    fn left_by(&self, pane: &PaneUuid) -> Option<(String, SessionId)> {
        lock(&self.left_running)
            .remove(pane)
            .then(|| self.shared.ledger.local_terminal_of(pane))
            .flatten()
            .map(|terminal| (local::ACCOUNT.to_owned(), terminal))
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
        // A pane that is let go of is told when its terminal has ended like any other.
        self.awaken_to_end(account, terminal);
        let Some(client) = self.hub.client_mut(account) else {
            return false;
        };
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
        // Its program's view goes with it; the views are kept by name for this Mac's terminals
        // only, and a server's terminal of the same name is not one of them.
        if local::is_account(account) {
            self.views.remove(terminal);
        }
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
                    "bateri: the bateri on this server speaks a protocol this one does not"
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
        // The server would not take a terminal: one found again is made again behind the stream,
        // and a new one is a reason for an ordinary shell on it — or, where a holder carries the
        // stream already, the hub's own words end the pane as they would anywhere.
        let ended: Vec<HoldId> = self
            .awaiting
            .keys()
            .filter(|hold| self.hub.said_last_word(**hold))
            .copied()
            .collect();
        for hold in ended {
            // The person chose a shell while the terminal was on its way: the wait ended then,
            // with its line, and the pane has its shell; what the server says of the terminal now
            // is nobody's.
            if self
                .awaiting
                .get(&hold)
                .is_some_and(|awaiting| awaiting.chosen)
            {
                self.awaiting.remove(&hold);
                self.hub.closed(hold);
                continue;
            }
            if self
                .awaiting
                .get(&hold)
                .is_some_and(|awaiting| awaiting.resumed.is_some())
            {
                self.resumed_gone(hold);
            } else if self
                .awaiting
                .get(&hold)
                .is_some_and(|awaiting| awaiting.parts.held.is_some())
            {
                let said = String::from_utf8_lossy(&self.hub.take_outgoing(hold))
                    .trim()
                    .to_owned();
                let why = if said.is_empty() {
                    "the server did not take the terminal".to_owned()
                } else {
                    format!("the server did not take the terminal: {said}")
                };
                self.refuse_local(hold, &why);
            } else if let Some(awaiting) = self.awaiting.remove(&hold) {
                let line = awaiting.parts.trace.refused_line(
                    awaiting.parts.pane.as_str(),
                    "the server did not keep the terminal",
                    Instant::now(),
                );
                self.shared.backend.log_line(&line);
                awaiting.parts.pid.settle();
            }
        }
        let holds: Vec<HoldId> = self.conns.keys().copied().collect();
        for hold in holds {
            let bytes = self.hub.take_outgoing(hold);
            // A pane that waits for its terminal has its stream, and reads it: what its terminal
            // writes in the meantime (the past of one found again, which can be many times what
            // the bridge lets wait) is counted as any pane's, and one that reads nothing is let
            // go of as any is ([`Core::stalled`]).
            let queue = self
                .hub
                .terminal_of(hold)
                .and_then(|(account, _)| self.accounts.get(account))
                .map(|entry| Arc::clone(&entry.queue));
            if let Some(conn) = self.conns.get(&hold) {
                if !bytes.is_empty() {
                    // Counted against the account's queue and the holder's own load, or against
                    // neither: a load of its own.
                    let load = match &queue {
                        Some(queue) => {
                            conn.load.add(bytes.len(), queue);
                            Arc::clone(&conn.load)
                        }
                        None => Load::alone(bytes.len()),
                    };
                    let _ = conn.out.send(Out::Bytes(bytes, load));
                }
                if self.hub.is_finished(hold) {
                    // The pane is told how the program ended before its connection closes.
                    if let Some(status) = &conn.status {
                        match self.hub.exit_of(hold) {
                            Some(Exit::Code(code)) => status.code(code),
                            Some(Exit::Signal(signal)) => status.signal(signal),
                            None => status.code(1),
                        }
                    }
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
                    && !self.removing.contains(account.as_str())
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
            // A kablo that could not be run is written again at every dial, for as long as it
            // takes: the dial after the server has room again is the one that brings it back.
            let tidy = entry.stale || entry.tidy_due;
            let shared = Arc::clone(&self.shared);
            let started = thread::Builder::new()
                .name("ssh-hold dials".into())
                .spawn({
                    let account = account.clone();
                    move || {
                        // The terminals are renewed before any bridge exists:
                        // the renewal does not replace one that runs.
                        let refreshed = if tidy {
                            let live = shared.ledger.terminals_of(&account);
                            shared.backend.refresh(&server, &live).map(|installed| {
                                forget_gone(&shared.ledger, &account, installed, &live)
                            })
                        } else {
                            None
                        };
                        let outcome = shared
                            .backend
                            .carry(&server, refreshed.as_ref().unwrap_or(&installed));
                        let reached = outcome.is_ok();
                        let _ = shared.tx.send(Msg::Dialed {
                            account,
                            outcome,
                            refreshed,
                        });
                        // The terminals this account keeps are shown again: their programs'
                        // folder is made once the bridge is on its way. Not for a server that
                        // could not be reached, whose every redial would run it again.
                        if reached {
                            shared.stage_account(&server);
                        }
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
            entry.tidy_due = false;
            // The look that renewed it wrote the shells' files too.
            entry.files = true;
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
                let queue = Arc::clone(&entry.queue);
                let name = account.to_owned();
                let _ = thread::Builder::new()
                    .name("kablo bridge reads".into())
                    .spawn(move || bridge_reader(name, generation, reader, &tx, &queue, ended));
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

    /// A connection was lost, for whatever reason it was (the client says so once, whichever of
    /// the ways it came to know): a kablo that could not be run is to be written again before the
    /// next dial, and the panes that wait for a new terminal are let go if the server is not
    /// coming.
    fn lost(&mut self, account: &str, cause: Cause) {
        if let Some(entry) = self.accounts.get_mut(account) {
            if cause == Cause::Missing {
                entry.missing();
            } else {
                entry.missed = 0;
            }
        }
        self.give_up_if_hopeless(account);
    }

    /// The panes that wait for a new terminal of an account that cannot be reached, or whose
    /// kablo cannot be written again, are let go of: a pane that waits and is told nothing is the
    /// worse of the two. A holder of a server falls back to a plain `ssh`, which says what is the
    /// matter itself; a pane of this Mac is given an ordinary shell of this process and is told
    /// why in a line of its own ([`Core::refuse_local`]), and the terminal it asked for, which
    /// the server may have made, is ended by the connection that comes up (the ledger has it
    /// marked to end) — no shell runs that nobody sees.
    fn give_up_if_hopeless(&mut self, account: &str) {
        if !self.gives_up(account) && !self.local_stuck(account) {
            return;
        }
        let why = self.why_given_up(account);
        let awaiting = &self.awaiting;
        // A pane of this Mac that has not taken a stream of a terminal yet has an ordinary shell
        // to be given. One that has — a terminal found again, whose program may live, or the
        // new one made instead of a found one that was not there — is not let go of: it waits
        // for kablo, as the terminal does.
        let given_up = self.hub.give_up_new(account, |hold| {
            awaiting
                .get(&hold)
                .is_some_and(|waiting| waiting.resumed.is_none() && waiting.parts.held.is_some())
        });
        let mut plain = false;
        for given in &given_up {
            if given.raw {
                self.refuse_local(given.hold, &why);
                continue;
            }
            // The row `started` wrote for this pane. If the request had reached the server the
            // terminal may be there, on no row: the pane is a plain `ssh` by now.
            self.shared.ledger.gone(account, &given.terminal);
            plain = true;
        }
        // A row that was the account's only one took the account with it.
        if plain && let Some(entry) = self.accounts.get(account) {
            self.shared.ledger.remember(&entry.server, &entry.installed);
        }
    }

    /// What a pane of this Mac that is let go of is told, in the words of the server panes'
    /// `could not connect`.
    fn why_given_up(&self, account: &str) -> String {
        if self.local_stuck(account) {
            return "kablo on this Mac could not be run: it is not allowed to".to_owned();
        }
        let missed = self.accounts.get(account).map_or(0, |entry| entry.missed);
        if missed >= 2 {
            return "kablo on this Mac could not be run".to_owned();
        }
        let tries = self.hub.client(account).map_or(0, Client::unreached);
        format!(
            "kablo on this Mac could not be reached: {tries} attempts in a row did not get a connection"
        )
    }

    /// Whether the kablo of this Mac cannot be run for want of permission, which the connection
    /// takes for a login it waits for: nobody is there to log in to this Mac's own bridge, and
    /// waiting does not mend it. (An account that is over is let go of by [`Core::over`].)
    fn local_stuck(&self, account: &str) -> bool {
        local::is_account(account)
            && matches!(
                self.hub.client(account).map(Client::link),
                Some(Link::NeedsLogin)
            )
    }

    /// Whether a new terminal on `account` is not worth waiting for: the account waits for its
    /// next attempt, and the last [`GIVE_UP_AFTER`] attempts to reach it in a row failed
    /// ([`Client::unreached`]; a live connection that was lost is not one), or the kablo could not
    /// be run twice, which is the dial that wrote it again and still nothing to run
    /// ([`Account::missed`]). A first [`Cause::Missing`] is not hopeless: the next dial is the
    /// one that mends it. An account that is waiting for a login is waiting for the person, who
    /// is not hurried, and one that is connecting is about to be there.
    fn gives_up(&self, account: &str) -> bool {
        let (Some(entry), Some(client)) = (self.accounts.get(account), self.hub.client(account))
        else {
            return false;
        };
        if !matches!(client.link(), Link::Retrying { .. }) {
            return false;
        }
        entry.missed >= 2 || (entry.missed == 0 && client.unreached() >= GIVE_UP_AFTER)
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
        if pane.program.is_some() {
            // Whatever refuses it from here on, the pane ends: it was made for the program.
            self.hub.for_a_program(hold);
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
                let now = Instant::now();
                let take = self
                    .takes
                    .remove(&(account.clone(), open.terminal.clone()))
                    .is_some_and(|at| now.saturating_duration_since(at) < TAKE_WAIT);
                if self.accounts.contains_key(account)
                    && self.hub.resume(hold, account, take).is_ok()
                {
                    // Who is attached is not known until the server says again.
                    self.roster
                        .entry(account.clone())
                        .or_default()
                        .attached(&open.terminal);
                    self.remember_account(account);
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

    /// The account of this Mac is wanted: told at once if the service has it, else made on a
    /// thread of its own ([`Backend::install`] puts kablo in the home directory).
    fn here(&mut self, pending: Pending) {
        if self.accounts.contains_key(local::ACCOUNT) {
            tell(&pending, Ok(local::ACCOUNT.to_owned()));
            return;
        }
        let shared = Arc::clone(&self.shared);
        let spawned = thread::Builder::new()
            .name("kablo makes this Mac's account".into())
            .spawn({
                let pending = Arc::clone(&pending);
                move || {
                    let live = shared.ledger.terminals_of(local::ACCOUNT);
                    let outcome = shared
                        .backend
                        .install(&local::host(), &live)
                        .map(|installed| {
                            forget_gone(&shared.ledger, local::ACCOUNT, installed, &live)
                        });
                    let _ = shared.tx.send(Msg::HereMade { outcome, pending });
                }
            });
        if spawned.is_err() {
            tell(&pending, Err(Refusal::plain()));
        }
    }

    fn here_made(&mut self, outcome: Result<Installed, Refusal>, pending: &Pending, now: Instant) {
        match outcome {
            Ok(installed) => {
                // A second request that was made while the first was still putting kablo there.
                if !self.accounts.contains_key(local::ACCOUNT) {
                    let host = local::host();
                    self.shared.ledger.remember(&host, &installed);
                    self.make_account(host, installed, true, now);
                }
                tell(pending, Ok(local::ACCOUNT.to_owned()));
            }
            Err(refusal) => tell(pending, Err(refusal)),
        }
    }

    /// A pane wants a terminal of this Mac: started at once if the account is there, else when
    /// the account has been made (once, for every pane that waits for it). The pane has its
    /// stream already ([`LocalAsk::new`]), and gets the terminal on it when it comes.
    fn local(&mut self, mut ask: LocalAsk, now: Instant) {
        let Some(parts) = ask.take() else {
            return;
        };
        let hold = self.hub.open_raw();
        self.awaiting.insert(
            hold,
            Awaiting {
                parts,
                resumed: None,
                terminal: None,
                end_asked: false,
                chosen: false,
            },
        );
        // Chosen before the request reached the loop (the service was not up).
        if self
            .awaiting
            .get(&hold)
            .is_some_and(|awaiting| awaiting.parts.ticket.chose())
        {
            return self.choose_ordinary(hold);
        }
        if self.accounts.contains_key(local::ACCOUNT) {
            return self.local_start(hold);
        }
        // A step that has just failed is not repeated for the next pane.
        if let Some((at, _)) = &self.local_refused
            && now.saturating_duration_since(*at) < LOCAL_REFUSAL
        {
            let why = format!(
                "the account of this Mac was refused {} ago, and a refusal is kept for {}",
                waiting::seconds(now.saturating_duration_since(*at)),
                waiting::seconds(LOCAL_REFUSAL)
            );
            return self.refuse_local(hold, &why);
        }
        if let Some(awaiting) = self.awaiting.get(&hold) {
            awaiting.parts.trace.enter(waiting::Step::Account);
        }
        let first = self.local_waiting.is_empty() && self.local_when.is_empty();
        self.local_waiting.push(hold);
        if first {
            self.make_local();
        }
    }

    /// The pane that waits for `hold`'s terminal is not given one, for `why`: it is told, and an
    /// ordinary shell of this process takes its stream ([`ask::refuse`]).
    fn refuse_local(&mut self, hold: HoldId, why: &str) {
        let Some(awaiting) = self.awaiting.remove(&hold) else {
            return;
        };
        self.hub.closed(hold);
        if let Some(Awaiting {
            terminal: Some(terminal),
            ..
        }) = Some(&awaiting)
        {
            self.shared.ledger.end(local::ACCOUNT, terminal);
        }
        let Awaiting { parts, .. } = awaiting;
        if parts.held.is_none() {
            // The stream is carried by a holder already ([`Core::carry_local`]): there is no
            // shell to put behind it, and the pane's program ends with the reason.
            parts.status.code(1);
            parts.pid.settle();
            self.conns.remove(&hold);
            self.panes.remove(&hold);
            parts.trace.refuse(why);
            let line = parts
                .trace
                .refused_line(parts.pane.as_str(), why, Instant::now());
            self.shared.backend.log_line(&line);
            return;
        }
        let backend = Arc::clone(&self.shared.backend);
        ask::refuse(parts, why, Some(backend.as_ref()));
    }

    /// The person chose an ordinary shell in place of the terminal the pane waits for
    /// ([`bt_core::PtyOps::ordinary_instead`], once, from a click). What becomes of the terminal
    /// depends on what it is:
    ///
    /// - **nothing was asked of the server yet** (the account is being made, the counter asked):
    ///   the pane is refused as for any reason ([`Core::refuse_local`]);
    /// - **a new terminal is on its way**: the pane's stream is given the shell, and the terminal
    ///   is ended when it comes, as for a pane that was closed ([`Awaiting::end_asked`]) — no
    ///   shell runs that nobody sees;
    /// - **a terminal found again**, whose program may live: it is left as it is, in the ledger
    ///   with its pane, and what the pane's holder carries is the shell from now on. The pane's
    ///   close ends it, as a close ends a pane's terminal ([`Core::end`]), and so does a quit that
    ///   ends the programs ([`Core::end_all`]); any other quit leaves it for the next launch.
    /// - **the new terminal made instead of a found one that was not there**: as a new one.
    ///
    /// A stream a holder carries already (the last two) cannot be given to a shell by handing
    /// over its ends, so the shell writes to the holder's writer and what the holder reads of the
    /// pane goes to the shell ([`Msg::Hold`]). A choice that comes after the terminal has come
    /// as far as the pane — attached, or taking what the pane types — is not taken: the pane
    /// may have been shown some of it, or typed into it. The click ends the pane's wait all the
    /// same, at once ([`bt_core::ChildPid::shown`]): the terminal is what the pane shows, and a
    /// link that goes while the line stays would be a click that did nothing to see. The pid
    /// follows with the server's list ([`Core::found`]).
    fn choose_ordinary(&mut self, hold: HoldId) {
        let Some(awaiting) = self.awaiting.get_mut(&hold) else {
            return;
        };
        if std::mem::replace(&mut awaiting.chosen, true) {
            return;
        }
        let why = ask::CHOSEN;
        let pane = awaiting.parts.pane.clone();
        let found_again = self.shared.ledger.local_terminal_of(&pane).is_some()
            && awaiting.terminal.is_none()
            || awaiting.resumed.is_some();
        if awaiting.parts.held.is_some() {
            // The stream is not carried yet: a shell takes the ends the service has.
            if awaiting.terminal.is_none() {
                if found_again {
                    lock(&self.left_running).insert(pane);
                }
                return self.refuse_local(hold, why);
            }
            // A new terminal asked for has no row from now on: a bateri that is gone before it
            // comes leaves nothing a pane would find again, and the next connection ends it. One
            // found again is left running in its row, as where a holder carries the stream (a
            // found one's stream is carried as it is asked for, so this is not expected).
            let found = awaiting.resumed.is_some();
            if found {
                lock(&self.left_running).insert(pane.clone());
            } else if let Some(terminal) = &awaiting.terminal {
                self.shared.ledger.end(local::ACCOUNT, terminal);
            }
            let behind = awaiting.parts.lend_behind();
            awaiting.parts.trace.refuse(why);
            let line = awaiting
                .parts
                .trace
                .refused_line(pane.as_str(), why, Instant::now());
            self.shared.backend.log_line(&line);
            ask::ordinary_behind(behind, why);
            if found {
                // Nothing waits for it from now on: its holder is let go of, and the terminal
                // lives on unseen.
                self.awaiting.remove(&hold);
                self.hub.closed(hold);
                return;
            }
            // As for a pane that was closed while its terminal was on its way: a new one is ended
            // as it comes ([`Core::created`]), and nothing is made of it for nobody.
            awaiting.end_asked = true;
            return;
        }
        // A holder carries the stream. The terminal has come if the server attached it (its past
        // may be on the pane's screen) or it takes what the pane types (what was typed before
        // the click went to it): the choice came too late, and the terminal is the pane's.
        let arrived = self.hub.is_attached(hold)
            || self
                .conns
                .get(&hold)
                .is_some_and(|conn| !conn.waits_for_input);
        if arrived {
            // The wait goes on to its end as if the choice had not been made, and the log says
            // so: a click on the link that did nothing is a question someone asks. On the pane's
            // screen the wait is over: its line goes now, not when the list comes.
            if let Some(awaiting) = self.awaiting.get_mut(&hold) {
                awaiting.chosen = false;
                awaiting.parts.ticket.forget_choice();
                self.shared.backend.log_line(&format!(
                    "the pane {} chose an ordinary shell after its terminal had come: it is shown its terminal",
                    awaiting.parts.pane.as_str()
                ));
                awaiting.parts.pid.shown();
            }
            return;
        }
        let Some(awaiting) = self.awaiting.remove(&hold) else {
            return;
        };
        let Awaiting {
            parts,
            resumed,
            terminal,
            ..
        } = awaiting;
        self.hub.let_go_raw(hold);
        if resumed.is_some() {
            lock(&self.left_running).insert(pane);
        } else if let Some(terminal) = terminal {
            self.shared.ledger.end(local::ACCOUNT, &terminal);
            self.end_on_arrival
                .insert((local::ACCOUNT.to_owned(), terminal));
        }
        parts.trace.refuse(why);
        let line = parts
            .trace
            .refused_line(parts.pane.as_str(), why, Instant::now());
        self.shared.backend.log_line(&line);
        let Some(conn) = self.conns.get_mut(&hold) else {
            parts.status.code(1);
            parts.pid.settle();
            return;
        };
        let shell = ask::ordinary_carried(parts.lend_carried(), conn.out.clone(), why);
        // What the pane typed while it waited goes to the shell now, in order.
        conn.waits_for_input = false;
        conn.throttle.0.open();
        if let Some(shell) = shell {
            self.ordinaries.insert(hold, shell);
        }
        self.refresh = true;
    }

    /// The account of this Mac is made for the ones that wait for it ([`Core::local_account`]).
    fn make_local(&mut self) {
        let tx = self.shared.tx.clone();
        let told: Told = Box::new(move |outcome| {
            let _ = tx.send(Msg::LocalAccount(outcome.map(|_| ())));
        });
        self.here(Arc::new(Mutex::new(Some(told))));
    }

    /// [`Service::when_local`]: told now when the account is there or was just refused, else
    /// when it has been made — started here if nothing else is making it.
    fn when_local(&mut self, done: Box<dyn FnOnce(bool) + Send>, now: Instant) {
        if self.accounts.contains_key(local::ACCOUNT) {
            return done(true);
        }
        if self
            .local_refused
            .as_ref()
            .is_some_and(|(at, _)| now.saturating_duration_since(*at) < LOCAL_REFUSAL)
        {
            return done(false);
        }
        let first = self.local_waiting.is_empty() && self.local_when.is_empty();
        self.local_when.push(done);
        if first {
            self.make_local();
        }
    }

    fn local_account(&mut self, outcome: &Result<(), Refusal>, now: Instant) {
        self.local_refused = outcome.as_ref().err().map(|refusal| (now, refusal.clone()));
        for hold in std::mem::take(&mut self.local_waiting) {
            match outcome {
                Ok(()) => self.local_start(hold),
                Err(_) => self.refuse_local(hold, "the account of this Mac was refused"),
            }
        }
        for done in std::mem::take(&mut self.local_when) {
            done(outcome.is_ok());
        }
    }

    /// Starts the terminal a pane asked for — or finds again the one the ledger says the pane
    /// showed before the bateri that showed it was gone, if that is still on the machine: the
    /// pane that comes back after an update, a quit that kept the programs, or a crash is shown
    /// its terminal where it was left.
    fn local_start(&mut self, hold: HoldId) {
        let Some(awaiting) = self.awaiting.get(&hold) else {
            return;
        };
        // The pane let go of its stream while it waited, or was closed: there is nobody to show a
        // terminal to.
        if awaiting.parts.ticket.is_gone() || awaiting.end_asked {
            self.awaiting.remove(&hold);
            self.hub.closed(hold);
            return;
        }
        // Named by the program that told its number: that terminal is made, not found.
        if awaiting.parts.name.is_some() {
            if let Some(silence) = &awaiting.parts.silence {
                silence.close();
            }
            return self.local_open(hold, None);
        }
        // Not one a holder shows already: a pane that asks twice is given a second terminal. A
        // pane born without a past ([`LocalAsk::new`]) is a new terminal's.
        let resume = awaiting.parts.silence.as_ref().and_then(|_| {
            self.shared
                .ledger
                .local_terminal_of(&awaiting.parts.pane)
                .filter(|terminal| self.hub.hold_of(local::ACCOUNT, terminal).is_none())
        });
        match resume {
            Some(_) => self.local_open(hold, resume),
            None => {
                // There is no past: what the stream carries is live from its first byte.
                if let Some(silence) = &awaiting.parts.silence {
                    silence.close();
                }
                self.local_numbered(hold)
            }
        }
    }

    /// A terminal of this Mac that is to be made is numbered by the counter the `bateri` command
    /// uses ([`Backend::number`]), on a thread of its own, so that `send`, `capture` and `wait`
    /// find a pane the user opened by its number as they find one a program opened. No number
    /// (the command is not wanted, the counter could not count): the terminal keeps the name it
    /// is made with.
    fn local_numbered(&mut self, hold: HoldId) {
        if let Some(awaiting) = self.awaiting.get(&hold) {
            awaiting.parts.trace.enter(waiting::Step::Number);
        }
        let shared = Arc::clone(&self.shared);
        let tx = shared.tx.clone();
        let spawned = thread::Builder::new()
            .name("ssh-hold numbers this Mac".into())
            .spawn(move || {
                let number = shared.backend.number(&local::host());
                let _ = tx.send(Msg::LocalNumbered { hold, number });
            });
        if spawned.is_err() {
            self.local_open(hold, None);
        }
    }

    /// [`Core::local_start`] with the terminal chosen: the terminal is written in the ledger with
    /// the pane before anything is asked of the server (a crash in between must not leave a
    /// terminal nobody can name), and the pane's stream is carried when the terminal is there
    /// ([`Core::created`]) — at once for one found again, whose past is waiting ([`Core::found`]
    /// has the server's word of it).
    fn local_open(&mut self, hold: HoldId, resume: Option<SessionId>) {
        let Some(mut awaiting) = self.awaiting.remove(&hold) else {
            return;
        };
        awaiting.parts.trace.enter(waiting::Step::Start);
        if resume.is_some() {
            awaiting.parts.trace.found_again();
        }
        let (said, gone) = awaiting.parts.ticket.bind(hold, self.shared.tx.clone());
        if gone || awaiting.end_asked {
            // The pane let go of its stream while it waited: nothing is started for nobody, and
            // a terminal found again stays for the next bateri that has a pane for it.
            self.hub.closed(hold);
            return;
        }
        let size = said.unwrap_or(awaiting.parts.size);
        let account = local::ACCOUNT;
        let terminal = match &resume {
            Some(terminal) => {
                let silence = awaiting
                    .parts
                    .silence
                    .clone()
                    .unwrap_or_else(bt_core::Silence::new);
                let taken = self.hub.resume_raw(
                    hold,
                    account,
                    terminal.clone(),
                    size,
                    self.shared.backend.shows_past(),
                    silence,
                );
                if taken.is_err() {
                    self.awaiting.insert(hold, awaiting);
                    return self.refuse_local(
                        hold,
                        "the terminal that was shown before could not be taken up",
                    );
                }
                terminal.clone()
            }
            None => {
                let Some(terminal) = awaiting
                    .parts
                    .name
                    .clone()
                    .or_else(|| crate::hold::new_terminal_name().ok())
                else {
                    self.awaiting.insert(hold, awaiting);
                    return self.refuse_local(hold, "no name for the terminal");
                };
                if self
                    .hub
                    .start_raw(
                        hold,
                        account,
                        terminal.clone(),
                        awaiting.parts.spec.clone(),
                        size,
                    )
                    .is_err()
                {
                    self.awaiting.insert(hold, awaiting);
                    return self.refuse_local(hold, "the bridge would not start the terminal");
                }
                terminal
            }
        };
        self.remember_account(account);
        let view = self
            .shared
            .ledger
            .shown(account, &terminal, Some(&awaiting.parts.pane));
        // A terminal found again may have a program's view open from before: what a line of the
        // log says the pane shows until the application says otherwise ([`Core::views`]).
        if let Some(view) = view {
            self.views.insert(terminal.clone(), view.program);
        }
        self.panes.insert(hold, awaiting.parts.pane.clone());
        awaiting.parts.trace.enter(waiting::Step::Server);
        awaiting.terminal = Some(terminal);
        let found = resume.is_some();
        awaiting.resumed = resume;
        if found {
            self.carry_local(hold, &mut awaiting.parts);
            // A terminal found again is answered by the server's list of its terminals, which has
            // the process id of its program: the list is asked for now, and again when the
            // connection comes up.
            self.relist.insert(account.to_owned());
        }
        self.awaiting.insert(hold, awaiting);
        // A pane that asks of a kablo that was given up on is a reason to look at it again: if it
        // is back, the pane is given its terminal now and not after the long pause the last
        // attempts left, and if it is not, the attempt that fails lets the pane go at once. One
        // that asks of a kablo that is only slow does not make the attempts come faster than
        // their own pauses: three panes opened in a second would use up the ones a short outage
        // is given ([`Core::gives_up`]).
        if (self.gives_up(account) || self.local_stuck(account))
            && let Some(client) = self.hub.client_mut(account)
        {
            client.retry_now(Instant::now());
        }
        self.refresh = true;
    }

    /// The pane's stream is carried by a holder from now on: what the terminal writes goes to it
    /// and what the pane types goes to the terminal — but not before the terminal takes input
    /// ([`Core::open_gates`]): what is typed until then waits in the stream, in order.
    fn carry_local(&mut self, hold: HoldId, parts: &mut Parts) {
        let (Some(held), Some(exit)) = (parts.held.take(), parts.exit.take()) else {
            return;
        };
        let Ok(reader) = held.try_clone() else {
            parts.status.code(1);
            return;
        };
        let peer = Peer { uid: 0, pid: 0 };
        self.carry_hold(
            hold,
            held,
            reader,
            peer,
            Some((Arc::clone(&parts.status), exit)),
            true,
        );
    }

    /// Lets the readers of the panes whose terminals take input now read what the panes typed.
    /// A terminal found again takes none until the server has said it is there — its list has it
    /// ([`Core::found`]) or it attached it: until then it may be one the server does not have,
    /// and what was typed into it would be lost with it ([`Core::resumed_gone`] gives the pane a
    /// new one, which is typed into).
    fn open_gates(&mut self) {
        let waiting: Vec<(HoldId, bool)> = self
            .conns
            .iter()
            .filter(|(_, conn)| conn.waits_for_input)
            .map(|(hold, _)| {
                let found_again = self
                    .awaiting
                    .get(hold)
                    .is_some_and(|awaiting| awaiting.resumed.is_some());
                (*hold, found_again)
            })
            .collect();
        for (hold, found_again) in waiting {
            let takes = if found_again {
                self.hub.is_attached(hold)
            } else {
                self.hub.accepts_input(hold)
            };
            if !takes {
                continue;
            }
            if let Some(conn) = self.conns.get_mut(&hold) {
                conn.waits_for_input = false;
                conn.throttle.0.open();
            }
        }
    }

    /// The server made a terminal: the pane that waits for it has it on its stream, with the
    /// process id the terminal's program has.
    fn created(&mut self, account: &str, status: &Status, now: Instant) {
        // The pane was closed while the terminal was on its way: it is ended as it comes.
        if self
            .end_on_arrival
            .remove(&(account.to_owned(), status.id.clone()))
        {
            self.finish_off(account, &status.id, now);
            return;
        }
        let Some(hold) = self.hub.hold_of(account, &status.id) else {
            return;
        };
        if self
            .awaiting
            .get(&hold)
            .is_none_or(|awaiting| awaiting.resumed.is_some())
        {
            return;
        }
        let Some(mut awaiting) = self.awaiting.remove(&hold) else {
            return;
        };
        if awaiting.parts.ticket.is_gone() || awaiting.end_asked {
            // Nobody will show this terminal: the pane let go of its stream, or its close asked
            // for the terminal's end while it was on its way.
            self.shared.ledger.end(account, &status.id);
            self.finish_off(account, &status.id, now);
            self.hub.closed(hold);
            return;
        }
        let line = awaiting.parts.trace.slow_line(
            awaiting.parts.pane.as_str(),
            self.shared.backend.slow_open(),
            now,
        );
        if let Some(line) = line {
            self.shared.backend.log_line(&line);
        }
        // After the line: whoever sees the process id can read what the log says of the wait.
        awaiting.parts.pid.set(status.pid);
        self.carry_local(hold, &mut awaiting.parts);
    }

    /// The server listed its terminals: the panes that wait for one found again have the process
    /// id of its program, and one the server does not have is let go of — the pane gets a new
    /// terminal on its stream ([`Core::resumed_gone`]).
    fn found(&mut self, account: &str, list: &[Status]) {
        if !local::is_account(account) {
            return;
        }
        let waiting: Vec<(HoldId, SessionId)> = self
            .awaiting
            .iter()
            .filter_map(|(hold, awaiting)| {
                awaiting
                    .resumed
                    .as_ref()
                    .map(|terminal| (*hold, terminal.clone()))
            })
            .collect();
        for (hold, terminal) in waiting {
            // One whose program ended while the bateri was away is no terminal to show: the pane
            // is given a new one, as it is when the server's word of the end comes before the
            // list's (the ending is the same either way, and the old terminal is marked to end,
            // which tells the server it was seen).
            let Some(status) = list
                .iter()
                .find(|status| status.id == terminal && status.exit.is_none())
            else {
                self.resumed_gone(hold);
                continue;
            };
            let Some(awaiting) = self.awaiting.remove(&hold) else {
                continue;
            };
            let line = awaiting.parts.trace.slow_line(
                awaiting.parts.pane.as_str(),
                self.shared.backend.slow_open(),
                Instant::now(),
            );
            if let Some(line) = line {
                self.shared.backend.log_line(&line);
            }
            awaiting.parts.pid.set(status.pid);
        }
        self.open_gates();
    }

    /// The terminal a pane was to be shown again is not there (the server does not list it, or
    /// refused it): it is marked to end, and the pane is given a new one on the stream it has,
    /// as if it had asked for none — with what the earlier run left above it, if the pane has it
    /// ([`bt_core::Stream::silence`]).
    fn resumed_gone(&mut self, hold: HoldId) {
        let Some(awaiting) = self.awaiting.get_mut(&hold) else {
            return;
        };
        let Some(terminal) = awaiting.resumed.take() else {
            return;
        };
        awaiting.terminal = None;
        if let Some(silence) = &awaiting.parts.silence {
            silence.close();
        }
        self.shared.ledger.end(local::ACCOUNT, &terminal);
        // What the ledger had of a view in it is not the new terminal's.
        self.views.remove(&terminal);
        self.hub.let_go_raw(hold);
        if let Some(conn) = self.conns.get_mut(&hold) {
            conn.waits_for_input = true;
            conn.throttle.0.shut();
        }
        // A pane whose wait ended at a click that was not taken, as it showed this terminal
        // ([`Core::choose_ordinary`]), waits again, and says so as it did when it was born.
        if let Some(awaiting) = self.awaiting.get(&hold) {
            awaiting.parts.pid.waits_again();
        }
        self.local_numbered(hold);
    }

    /// The holder of a pane that waited for its terminal is gone (the pane let go of its stream, or
    /// the hub closed it): a pane that has no holder yet is given an ordinary shell, and a terminal
    /// that was asked for it and has not come is ended when it does.
    fn local_gone(&mut self, awaiting: Awaiting) {
        let Awaiting {
            parts,
            resumed,
            terminal,
            ..
        } = awaiting;
        if let Some(terminal) = &terminal
            && resumed.is_none()
        {
            self.shared.ledger.end(local::ACCOUNT, terminal);
            // Nobody shows it when it comes: it is ended as it does.
            self.end_on_arrival
                .insert((local::ACCOUNT.to_owned(), terminal.clone()));
        }
        if parts.held.is_some() {
            let backend = Arc::clone(&self.shared.backend);
            ask::refuse(
                parts,
                "the holder of the terminal was closed before it came",
                Some(backend.as_ref()),
            );
        }
    }

    /// What `pane`'s close (or a quit that ends the programs) finds that is not shown yet: the
    /// terminals asked for that have not come — ended when they do ([`Core::created`]), and no
    /// quit waits for an end the server cannot say yet — and the terminal the ledger has for a
    /// pane that waits to find it again before anything was asked: marked to end like any other
    /// of the pane's, and ended by the connection that comes.
    fn end_coming(&mut self, pane: &PaneUuid) -> (BTreeSet<SessionId>, Vec<(String, SessionId)>) {
        let mut coming = BTreeSet::new();
        let mut unbound = Vec::new();
        for awaiting in self.awaiting.values_mut() {
            if awaiting.parts.pane != *pane {
                continue;
            }
            awaiting.end_asked = true;
            if awaiting.resumed.is_some() {
                continue;
            }
            if let Some(terminal) = &awaiting.terminal {
                coming.insert(terminal.clone());
            } else if awaiting.parts.silence.is_some()
                && awaiting.parts.name.is_none()
                && let Some(terminal) = self.shared.ledger.local_terminal_of(pane)
                && self.hub.hold_of(local::ACCOUNT, &terminal).is_none()
            {
                unbound.push((local::ACCOUNT.to_owned(), terminal));
            }
        }
        (coming, unbound)
    }

    /// The terminals this service asked for that no pane has shown end with it: the ledger does
    /// not keep a row of a terminal whose shell nobody saw.
    fn wind_down(&mut self) {
        for awaiting in self.awaiting.values() {
            if let Some(terminal) = &awaiting.terminal
                && awaiting.resumed.is_none()
            {
                self.shared.ledger.end(local::ACCOUNT, terminal);
            }
        }
    }

    /// The client of an account the ledger remembers, as it was before this
    /// process: started to dial at once. Nothing happens for an account it does not
    /// know.
    fn recall(&mut self, account: &str) {
        let Some(known) = self.shared.ledger.known(account) else {
            return;
        };
        self.make_account(known.server(), known.installed(), false, Instant::now());
    }

    /// A client for the server and the account it stands for.
    fn make_account(&mut self, server: KabloHost, installed: Installed, files: bool, now: Instant) {
        // An account the ledger brought back has not been looked at in this run; one a pane made
        // by a look (`files`) has.
        let tidy_due = !files;
        let key = server.key.clone();
        let name = self.shared.backend.name();
        // An account whose kablo is in the former root is refreshed too, whatever its version: the
        // current root gets its kablo. The terminals that already run from the former root are
        // not moved (a terminal on the package's own version is nobody's to renew), and stay
        // there until they end; the former root is read until it is empty.
        let stale = self.shared.backend.version().is_some_and(|version| {
            version != installed.version || installed.root() == Root::Former
        });
        let mut client = Client::new(name.as_deref(), now);
        // A program on this Mac that runs `kablo app` finds the application, as a program on a
        // server does: the `bateri` command is the same one, and what it may do is the Mac's
        // level ([`Letter`]'s account tells which).
        if self.shared.backend.takes_mail() {
            client.take_tickets();
        }
        self.hub.add_client(&key, client);
        let queue = Arc::new(Queue::new(local::is_account(&key)));
        self.shared
            .queues
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(key.clone(), Arc::clone(&queue));
        self.accounts.insert(
            key,
            Account {
                server,
                installed,
                stale,
                tidy_due,
                generation: 0,
                bridge: None,
                dialing: false,
                missed: 0,
                queue,
                purged: false,
                files,
                writing: Arc::new(Mutex::new(())),
            },
        );
    }

    /// Starts the terminal a holder asked for and writes it in the ledger, with
    /// the pane that shows it: from the moment of the request, since a crash between
    /// it and the server's answer must not leave a terminal nobody can name. Its name
    /// is final by now: the number the server's counter gave it (`name`), or the one
    /// its holder made when there is none — it is written here before the request goes
    /// to the server, and the holder is told it when the server has made the terminal.
    fn started(
        &mut self,
        hold: HoldId,
        account: &str,
        spec: NewSession,
        name: Option<SessionId>,
    ) -> Result<(), Declined> {
        // The server may have been given up on while its counter was being asked.
        if self.gives_up(account) {
            return Err(Declined::Refused);
        }
        self.hub.start(hold, account, spec, name)?;
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
            .map(|kept| kept.background.clone())
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
    /// one that rides a bridge that is there waits only for the server's counter, which is
    /// asked on a thread of its own too (a holder that cannot be told the terminal's number
    /// is not waited for at all: it keeps the name it made).
    fn prepare(&mut self, hold: HoldId, server: KabloHost, pane: PaneInfo) {
        let numbered = self.hub.version_of(hold) >= wire::NAMED;
        // Bateri is being taken off this server: nothing is put on it until that is over, and the
        // pane is the plain `ssh` it stands in for.
        if self.removing.contains(&server.key) {
            self.not_started(hold, &server.key);
            return;
        }
        if self.gives_up(&server.key) {
            self.not_started(hold, &server.key);
            // Being asked is a reason to look again: a server that is back serves the next one.
            if let Some(client) = self.hub.client_mut(&server.key) {
                client.retry_now(Instant::now());
            }
            return;
        }
        if let Some(entry) = self.accounts.get(&server.key) {
            let spec = new_session(&self.shared, &pane);
            let account = entry.server.key.clone();
            if !numbered {
                if self.started(hold, &account, spec, None).is_err() {
                    self.not_started(hold, &account);
                }
                return;
            }
            // An account the ledger brought back has had no look at its server: its files are
            // written on the way to the number, once. Not while its kablo is an earlier release's
            // (`stale`): the command file would name a binary that has no `bateri` in it and undo
            // what the renewal of the first dial writes.
            let files = (!entry.files && !entry.stale).then(|| entry.installed.clone());
            if !self.ask_number(hold, server, account.clone(), spec, files) {
                self.not_started(hold, &account);
            }
            return;
        }
        let shared = Arc::clone(&self.shared);
        let spawned = thread::Builder::new()
            .name("ssh-hold prepares".into())
            .spawn({
                let shared = Arc::clone(&shared);
                move || {
                    let outcome = prepare(&shared, hold, server.clone(), &pane, numbered);
                    let made = outcome.is_ok();
                    let _ = shared.tx.send(Msg::Prepared { hold, outcome });
                    // After the answer: the account is there and a program's folder is made
                    // while the terminal starts.
                    if made {
                        shared.stage_account(&server);
                    }
                }
            });
        if spawned.is_err() {
            self.hub.refuse(hold, true, "");
        }
    }

    /// The server's counter is asked for the number of the terminal `hold` wants, on a thread of
    /// its own; the answer comes back as [`Msg::Numbered`]. `false` when the thread could not be
    /// made.
    fn ask_number(
        &self,
        hold: HoldId,
        server: KabloHost,
        account: String,
        spec: NewSession,
        files: Option<Installed>,
    ) -> bool {
        let shared = Arc::clone(&self.shared);
        thread::Builder::new()
            .name("ssh-hold numbers".into())
            .spawn(move || {
                // Before the number, so that the terminal starts from files that are there.
                let filed = files.is_some_and(|installed| {
                    shared.backend.files(&server, &installed);
                    true
                });
                let number = shared.backend.number(&server);
                let counted = number.is_some();
                let _ = shared.tx.send(Msg::Numbered {
                    hold,
                    account,
                    spec,
                    number,
                    filed,
                });
                // A server that counted is a server that answers.
                if counted {
                    shared.stage_account(&server);
                }
            })
            .is_ok()
    }

    /// The number the counter gave was taken by a terminal a person made: the row it wrote for
    /// it goes (that terminal is not this pane's, and must not be ended with it), and the next
    /// number is asked for — a few times, then the holder is let fall back to a plain `ssh`.
    fn collided(&mut self, hold: HoldId, account: &str, terminal: &SessionId, spec: NewSession) {
        self.shared.ledger.gone(account, terminal);
        // A row that was the account's only one took the account with it.
        if let Some(entry) = self.accounts.get(account) {
            self.shared.ledger.remember(&entry.server, &entry.installed);
        }
        let tries = self.collisions.entry(hold).or_insert(0);
        *tries += 1;
        let again = *tries <= RENUMBER_TRIES;
        let server = self.accounts.get(account).map(|entry| entry.server.clone());
        match server {
            Some(server) if again => {
                if !self.ask_number(hold, server, account.to_owned(), spec, None) {
                    self.not_started(hold, account);
                }
            }
            _ => {
                self.collisions.remove(&hold);
                self.not_started(hold, account);
            }
        }
    }

    /// Gives a holder none of the new terminal it asked for on `account`. Nothing was shown, so
    /// a pane of the user's runs the plain `ssh` it stands in for, which says what is wrong in
    /// its own words. The pane of a program has none to become and is told in a line
    /// ([`not_started_line`]); where the account is not known, the hub's own line says it.
    fn not_started(&mut self, hold: HoldId, account: &str) {
        let text = match self.accounts.get(account) {
            Some(entry) if self.hub.is_for_a_program(hold) => {
                not_started_line(&entry.server.host, self.gives_up(account))
            }
            _ => String::new(),
        };
        self.hub.refuse(hold, true, &text);
    }

    /// The person's answer to the first connection's row of `pane` ([`Service::answer_setup`]):
    /// to the preparation that asked, or, once the install is on its way, to stop it.
    fn answered(&mut self, pane: &PaneUuid, answer: Answer) {
        let holds: Vec<HoldId> = self
            .panes
            .iter()
            .filter(|(_, shown)| *shown == pane)
            .map(|(hold, _)| *hold)
            .collect();
        for hold in holds {
            let Some(asked) = self.asking.get_mut(&hold) else {
                continue;
            };
            if answer == Answer::Cancel {
                asked.stop.store(true, Ordering::SeqCst);
            }
            if let Some(reply) = asked.reply.take() {
                let _ = reply.send(answer);
            }
        }
    }

    fn prepared(&mut self, hold: HoldId, outcome: Result<Ready, Refusal>, now: Instant) {
        self.waiting.remove(&hold);
        self.asking.remove(&hold);
        let ready = match outcome {
            Ok(ready) => ready,
            Err(refusal) => {
                if let Some((account, line)) = &refusal.note
                    && !self.unasked.contains(account)
                    && self.hub.say(hold, line)
                {
                    self.unasked.insert(account.clone());
                }
                self.hub.refuse(hold, refusal.fall_back, &refusal.text);
                return;
            }
        };
        let account = ready.server.key.clone();
        if !self.accounts.contains_key(&account) {
            self.shared.ledger.remember(&ready.server, &ready.installed);
            self.make_account(ready.server.clone(), ready.installed.clone(), true, now);
        }
        if let Some(note) = &ready.note {
            self.hub.say(hold, note);
        }
        if self
            .started(hold, &account, ready.spec, terminal_named(ready.number))
            .is_err()
        {
            self.not_started(hold, &account);
        }
    }
}

/// A bigger buffer on a stream socket, as far as the system lets it be. Best effort.
fn widen(stream: &UnixStream) {
    let buffer: libc::c_int = 256 * 1024;
    for option in [libc::SO_SNDBUF, libc::SO_RCVBUF] {
        // SAFETY: the descriptor is open for the call and the value is a `c_int` of the size
        // that is passed; a refusal changes nothing.
        unsafe {
            libc::setsockopt(
                stream.as_raw_fd(),
                libc::SOL_SOCKET,
                option,
                (&raw const buffer).cast(),
                std::mem::size_of::<libc::c_int>() as libc::socklen_t,
            );
        }
    }
}

/// Says what became of the account of this Mac to whoever asked, once.
fn tell(pending: &Pending, outcome: Result<String, Refusal>) {
    let told = pending.lock().unwrap_or_else(|e| e.into_inner()).take();
    if let Some(told) = told {
        told(outcome);
    }
}

/// The name of the terminal the server's counter numbered: `p7` for 7, as an agent names it.
fn terminal_named(number: Option<u64>) -> Option<SessionId> {
    number.and_then(|number| SessionId::new(&crate::agent::Terminal(number).to_string()).ok())
}

/// The new terminal's request: the bootstrap as the command (the shells' files are on the server
/// already: `install::ensure` wrote them), the dock's tier as its environment, bateri's
/// scrollback as the history.
///
/// A pane that was opened for a program asks for that program and the folder it starts in, and
/// nothing of the bootstrap: there is no shell in it to take the identity or the dock's tier, and
/// the terminal ends with the program.
fn new_session(shared: &Shared, pane: &PaneInfo) -> NewSession {
    // `NewSession` is not exhaustive: it is started from its default.
    let mut spec = NewSession::default();
    if let Some(program) = &pane.program {
        spec = program_session(program, pane.directory.as_deref());
        spec.history = shared.backend.history();
        return spec;
    }
    let nonce = ssh_wrap::new_nonce().unwrap_or_else(|| "0".repeat(ssh_wrap::NONCE_LEN));
    spec.argv = ssh_wrap::kablo_argv(
        pane.directory.as_deref(),
        pane.block,
        &nonce,
        pane.tab.as_ref(),
    );
    // The dock's tier goes with the terminal, in its environment: the shells there read it, and
    // `bateri` (the agent's command) hands it to the terminals it opens.
    if !shared.backend.dock() {
        spec.env.push((b"BATERI_DOCK".to_vec(), b"off".to_vec()));
    }
    spec.history = shared.backend.history();
    // The shell asks the moment it starts, a round trip before the pane can have attached: its
    // questions are the pane's to answer ([`crate::open_with::CLIENT_WAIT_MS`]).
    spec.wait_for_client = Some(crate::open_with::CLIENT_WAIT_MS);
    spec
}

/// The request for a terminal that runs `program` in `directory` (the server's home when there is
/// none): the words as the command, and the terminal's questions kept for the pane that asked
/// for it ([`crate::open_with::CLIENT_WAIT_MS`]): the program asks the moment it starts, before
/// the pane can have attached. It is a program somebody asked for ([`crate::open_with::ask_for_program`]).
pub(crate) fn program_session(program: &[String], directory: Option<&str>) -> NewSession {
    // `NewSession` is not exhaustive: it is started from its default.
    let mut spec = NewSession::default();
    spec.argv = program
        .iter()
        .map(|word| word.as_bytes().to_vec())
        .collect();
    spec.cwd = directory.map(|directory| directory.as_bytes().to_vec());
    crate::open_with::ask_for_program(&mut spec);
    spec
}

/// The rows of the ledger for terminals the server does not have are gone from it: a terminal
/// that ended while no bateri was looking (`live` is what the ledger had when the look began, so
/// a row written since is not judged) is the same as one whose end was seen ([`Core::finished`]).
/// Only a look that was a whole one says so ([`Installed::census`]); the look is spent here and
/// the account keeps none of it.
fn forget_gone(
    ledger: &Ledger,
    account: &str,
    installed: Installed,
    live: &[SessionId],
) -> Installed {
    if let Some(census) = &installed.census {
        for terminal in live.iter().filter(|terminal| !census.has(terminal)) {
            ledger.gone(account, terminal);
        }
    }
    Installed {
        census: None,
        ..installed
    }
}

/// The slow steps of a first terminal, on a thread of its own: whether the server may be set up
/// (a "no" opens no connection), the login on the holder's terminal, the person's say in the
/// pane's dock where the server is new to bateri, then kablo on the server, then its number
/// (when `numbered`: the holder can be told it).
///
/// **One connection serves the whole of it.** The login is the master, opened on the pane's terminal
/// where ssh asks for what it asks; the steps after it ride that master ([`Backend::install`]),
/// so a server behind a proxy costs the seconds of one connection, not of one per step.
fn prepare(
    shared: &Shared,
    hold: HoldId,
    server: KabloHost,
    pane: &PaneInfo,
    numbered: bool,
) -> Result<Ready, Refusal> {
    let backend = &shared.backend;
    let consent = backend.consent(&server);
    if consent == Consent::Refused {
        return Err(Refusal::plain());
    }
    // A pane that cannot say which it is, or has no dock to put the row in, cannot be asked in,
    // nor answered to: no login is made for it either. A pane the person typed `ssh` in whose dock
    // is off is told, once, where bateri can be set up from (they closed the dock themselves:
    // nothing is asked and no band is put up for it). A pane opened for a program is told nothing.
    if consent == Consent::Ask && !pane.can_show_row() {
        return Err(if pane.tab.is_some() && pane.program.is_none() {
            Refusal::unasked(&server)
        } else {
            Refusal::plain()
        });
    }
    // The login first: ssh's own questions (the server's key, a password) are asked in the pane
    // as they always were, and what is asked of the person after it needs no second password.
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
    let row = Row::new(&**backend, &server.host, pane);
    let outcome = set_up(shared, hold, server, pane, numbered, consent, &row);
    row.ended(&outcome);
    outcome
}

/// The steps of [`prepare`] after the login.
fn set_up(
    shared: &Shared,
    hold: HoldId,
    server: KabloHost,
    pane: &PaneInfo,
    numbered: bool,
    consent: Consent,
    row: &Row<'_>,
) -> Result<Ready, Refusal> {
    let backend = &shared.backend;
    if consent == Consent::Ask {
        match row.ask(shared, hold)? {
            Answer::SetUp => row.put(0, 0),
            Answer::Plain => {
                backend.answered(&server, Answer::Plain);
                return Err(Refusal::plain());
            }
            // Nothing was asked, or the person left: nothing is kept.
            Answer::Cancel | Answer::Unshown => return Err(Refusal::plain()),
        }
    } else {
        // Nothing is asked (`auto`, the host's own yes): the row still shows the bytes going, and
        // its *Cancel* must find the install.
        row.watch(shared, hold);
    }
    let live = shared.ledger.terminals_of(&server.key);
    let installed = forget_gone(
        &shared.ledger,
        &server.key,
        backend.install_watched(&server, &live, row)?,
        &live,
    );
    // The row says it when it was there; a pane that had none gets the line in its terminal.
    let note = (installed.fresh && !row.shown()).then(|| {
        format!(
            "bateri is set up on {}; if the connection drops, this terminal keeps running",
            server.host
        )
    });
    let spec = new_session(shared, pane);
    // Last, so that a number is not drawn for a terminal that was never going to start.
    let number = if numbered {
        backend.number(&server)
    } else {
        None
    };
    Ok(Ready {
        server,
        installed,
        spec,
        number,
        note,
    })
}

/// The first connection's row of a pane, as far as the preparation drives it: what it says is
/// told to the application ([`Backend::setup_changed`]), in order, and the install's bytes and
/// the person's *Cancel* pass through it ([`Watch`]).
struct Row<'a> {
    backend: &'a dyn Backend,
    /// The server, as typed: what the row names.
    host: String,
    /// The pane the row is in; `None` for a pane that cannot say which it is or has no dock
    /// (nothing is asked there, and nothing is shown).
    pane: Option<PaneUuid>,
    /// Set when the person stops the install.
    stop: Arc<AtomicBool>,
    /// Whether the row has been put up: only then is it taken down, and only then does it say the
    /// server is set up.
    up: AtomicBool,
    /// The last whole percent of the bytes told, so that the application hears of each step and
    /// not of each chunk.
    percent: AtomicU64,
}

impl<'a> Row<'a> {
    fn new(backend: &'a dyn Backend, host: &str, pane: &PaneInfo) -> Row<'a> {
        Row {
            backend,
            host: host.to_owned(),
            pane: pane.tab.clone().filter(|_| pane.rows),
            stop: Arc::new(AtomicBool::new(false)),
            up: AtomicBool::new(false),
            percent: AtomicU64::new(u64::MAX),
        }
    }

    fn shown(&self) -> bool {
        self.up.load(Ordering::SeqCst)
    }

    fn tell(&self, setup: Setup) {
        if let Some(pane) = &self.pane {
            self.up.store(true, Ordering::SeqCst);
            self.backend.setup_changed(pane, &self.host, setup);
        }
    }

    /// Puts the question up and waits for the answer. A pane that cannot be asked in is not waited
    /// for: [`Refusal::plain`], and nothing is kept. No answer is a holder that went away.
    fn ask(&self, shared: &Shared, hold: HoldId) -> Result<Answer, Refusal> {
        if self.pane.is_none() {
            return Err(Refusal::plain());
        }
        let (reply, answer) = mpsc::channel();
        // Before the row is put up, so that an answer that comes at once finds the wait.
        shared
            .tx
            .send(Msg::Ask {
                hold,
                reply,
                stop: Arc::clone(&self.stop),
            })
            .map_err(|_| Refusal::plain())?;
        self.tell(Setup::Asking);
        answer.recv().map_err(|_| Refusal::plain())
    }

    /// Lets the person stop the install that nothing was asked for: the stop is registered as
    /// [`Self::ask`] does, with no answer to wait for. A pane that shows no row has none to press.
    fn watch(&self, shared: &Shared, hold: HoldId) {
        if self.pane.is_none() {
            return;
        }
        let (reply, _) = mpsc::channel();
        let _ = shared.tx.send(Msg::Ask {
            hold,
            reply,
            stop: Arc::clone(&self.stop),
        });
    }

    /// bateri goes up: `sent` of `total` bytes.
    fn put(&self, sent: u64, total: u64) {
        self.tell(Setup::Putting { sent, total });
    }

    /// The preparation is over: the row says bateri is there, or goes.
    fn ended(&self, outcome: &Result<Ready, Refusal>) {
        if !self.shown() {
            return;
        }
        self.tell(if outcome.is_ok() {
            Setup::Done
        } else {
            Setup::Gone
        });
    }
}

impl Watch for Row<'_> {
    fn sent(&self, done: u64, total: u64) {
        let percent = (done * 100).checked_div(total).unwrap_or(0);
        if self.percent.swap(percent, Ordering::SeqCst) != percent {
            self.put(done, total);
        }
    }

    fn stopped(&self) -> bool {
        self.stop.load(Ordering::SeqCst)
    }
}

// ─── the threads that carry bytes ────────────────────────────────────────

fn hold_reader(hold: HoldId, mut stream: UnixStream, tx: &Sender<Msg>, gate: &Gate) {
    let mut buffer = vec![0u8; CHUNK];
    loop {
        // A pane that types faster than the server takes is not read: its socket fills and
        // its writer waits, instead of the process keeping what it said.
        gate.pass();
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

/// Writes what the hub has for a holder until it is told to close. `after` is the peer of a
/// pane's exit signal ([`Stream::exit`]): closed last, so the pane has read every byte there is
/// when it hears its program is gone.
fn hold_writer(
    hold: HoldId,
    mut stream: UnixStream,
    written: &Receiver<Out>,
    after: Option<UnixStream>,
    tx: &Sender<Msg>,
) {
    let mut broken = false;
    while let Ok(out) = written.recv() {
        match out {
            Out::Bytes(bytes, load) => {
                // A step at a time, so that what the pane takes is seen as it takes it.
                for step in bytes.chunks(WRITE_STEP) {
                    if !broken && stream.write_all(step).is_err() {
                        broken = true;
                    }
                    if load.wrote(step.len()) {
                        // Let go of for taking nothing, and it has taken all it was owed.
                        let _ = tx.send(Msg::Drained(hold));
                    }
                }
            }
            Out::Close => break,
        }
    }
    let _ = stream.shutdown(std::net::Shutdown::Both);
    drop(after);
}

fn bridge_reader(
    account: String,
    generation: u64,
    mut reader: Box<dyn Read + Send>,
    tx: &Sender<Msg>,
    queue: &Arc<Queue>,
    ended: Box<dyn FnOnce() -> Cause + Send>,
) {
    let mut buffer = vec![0u8; CHUNK];
    loop {
        // The holders are slow to take what they were given: the stream waits
        // in the pipe, and the server waits for the pipe. It waits at a condition and not on
        // a clock, and when the holders take nothing for [`HOLD_STALL`] the loop is told, to
        // let go of the ones that stand in the way.
        match queue.wait() {
            Waited::Open => {}
            Waited::Closed => return,
            Waited::Stalled => {
                let told = tx.send(Msg::Stalled {
                    account: account.clone(),
                    generation,
                });
                if told.is_err() {
                    return;
                }
                continue;
            }
        }
        match reader.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                queue.arrive(n);
                let sent = tx.send(Msg::Bridge {
                    account: account.clone(),
                    generation,
                    bytes: buffer[..n].to_vec(),
                    queue: Arc::clone(queue),
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

mod ask;
mod let_go;
mod pipes;
pub use pipes::Streams;
pub(super) mod waiting;

pub use ask::LocalAsk;
use ask::Parts;
#[cfg(test)]
pub(crate) use pipes::fake as fake_streams;

#[cfg(test)]
pub(crate) mod tests;
