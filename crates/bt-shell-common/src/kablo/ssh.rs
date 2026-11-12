//! The service's steps over ssh ([`SshBackend`]): the master connection the
//! user makes on the pane, kablo put on the server, and the bridge. The account of this Mac
//! ([`super::local`]) rides the same backend: its steps are the ones below with the transport
//! changed (a shell in the home directory, a bridge started here), and the steps that only a
//! server has — the master, the shells' files, the counter, the notes — are not taken for it.
//!
//! **Everything rides one connection.** The first `ssh` to a server makes a
//! master connection **on the pane's own terminal** — the program is `ssh -M -N
//! -f …` and runs where the user is, so a password, a second factor and the
//! question whether to trust a server's key are asked and answered where they
//! always were; the connection goes to the background once it is made. It lives
//! at the socket the rest of bateri already looks for
//! ([`crate::ssh_route::socket_path`]): the file jobs and the rest
//! find it there and ride it, and the steps here ride it too — the `uname`, the
//! install ([`crate::kablo::install`]) and the bridge (`kablo attach`) are
//! channels of that connection, started with `ControlMaster=no` and
//! `BatchMode=yes`, so none of them can ask a thing or become a master. A
//! server the user already has a master to (their own configuration shares
//! connections) needs no step on the pane at all.
//!
//! **Why not askpass.** The background master of the file jobs is refused an
//! unknown server's key (`StrictHostKeyChecking=yes`): it could not show the
//! question. The pane can; so the first connection is the pane's, and the
//! background is for what comes after.
//!
//! **A step that cannot be done is the plain `ssh`** (the [`Refusal`]), except
//! when ssh itself failed after it asked: a refused password or an unreachable
//! server ends the command the way `ssh` would have, because a second `ssh`
//! would only ask again.

use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use bt_core::{CommandLevel, PaneUuid, RemoteKind, RemoteTarget, Settings};

use kablo_proto::SessionId;
use kablo_proto::v1::stream::Command;

use super::Cause;
use super::install::{self, Bundle, Channel, Failure, Installed, SshChannel};
use super::local::{self, LocalChannel};
use super::remote::{RemoteCommand, RemoteStream};
use super::served;
use super::service::{
    Backend, Carried, Letter, PaneInfo, PaneLink, Peer, Reached, Refusal, Streams,
};
use crate::ssh_route::{self, Ask, Asker, Denied, Masters, Plan, Route, SshRunner};
use crate::ssh_wrap::{self, Fact, KabloHost, Wants};
use crate::upload::ssh_argv_for;

/// How a process is found to be in a pane (see [`SshBackend::panes`]).
pub type PaneLookup = Box<dyn Fn(&Peer) -> Option<PaneInfo> + Send + Sync>;

/// What the application does with the news that a pane's connection went or
/// came back ([`Backend::link_changed`]).
pub type LinkNews = Box<dyn Fn(&PaneUuid, PaneLink) + Send + Sync>;

/// What the application does with the news that a pane shows a terminal a
/// server keeps from now on, or no longer ([`Backend::terminal_changed`]).
pub type ChangeNews = Box<dyn Fn(&PaneUuid) + Send + Sync>;

/// Told the account whose terminals in the background are not what they were.
pub type BackgroundNews = Box<dyn Fn(&str) + Send + Sync>;

/// What the application does with what a program on a server said to it
/// ([`Backend::mail`]): it hops to its own thread and answers the letter there.
pub type MailNews = Box<dyn Fn(Letter) + Send + Sync>;

/// What a server left for the application while it was away, read when a connection to it came up
/// ([`Backend::reachable`]): the server and the notes ([`crate::agent::notes`]). The application
/// hops to its own thread and places what it can.
pub type NotesNews =
    Box<dyn Fn(crate::agent::serve::Server, Vec<crate::agent::notes::Note>) + Send + Sync>;

/// What the service gets from the application, as closures and values.
pub struct SshBackend {
    /// The ssh program (`ssh`).
    pub ssh: String,
    /// This instance's socket directories ([`ssh_route::Masters::bases`]).
    pub bases: Vec<PathBuf>,
    /// The hosts state file.
    pub state_path: PathBuf,
    /// kablo's binaries as the package carries them; none, and no `ssh` is a kablo call.
    pub bundle: Option<Bundle>,
    /// This Mac's home directory, which kablo's place for the account of this Mac is below
    /// ([`local`]); with a [`bundle`](SshBackend::bundle) the Mac has an account. `None`: it has
    /// none, and a pane of this Mac is an ordinary terminal.
    pub home: Option<PathBuf>,
    /// The settings as they are now.
    pub settings: Box<dyn Fn() -> Settings + Send + Sync>,
    /// The pane a process runs in.
    pub panes: PaneLookup,
    /// The real ssh runner, or a test's.
    pub runner: Box<dyn SshRunner + Send + Sync>,
    /// What this computer calls itself.
    pub name: Option<String>,
    /// The scrollback a terminal on a server keeps.
    pub history: Box<dyn Fn() -> Option<u32> + Send + Sync>,
    /// Whether a terminal of this Mac shown again is read from its past
    /// ([`Backend::shows_past`]).
    pub past: Box<dyn Fn() -> bool + Send + Sync>,
    /// The application's masters: what makes the connection again, with the
    /// password the Keychain keeps, when the one the user made is gone. `None`:
    /// only what `ssh` can do without a person (a key, an agent).
    pub masters: Option<Arc<Masters>>,
    /// A pane's terminal lost the connection or got it back
    /// ([`Backend::link_changed`]).
    pub linked: LinkNews,
    /// A pane began to show a terminal a server keeps, or stopped
    /// ([`Backend::terminal_changed`]).
    pub changed: ChangeNews,
    /// The program in front of a pane's terminal is another one as far as the
    /// pane's icon tells ([`Backend::program_changed`]).
    pub program: ChangeNews,
    /// The terminals an account keeps in the background are not what they were
    /// ([`Backend::background_changed`]).
    pub listed: BackgroundNews,
    /// The folder a server's account offers its programs is made ([`Backend::stage_ready`]).
    pub staged: BackgroundNews,
    /// The application takes what the programs on a server say to it: `Some` makes the
    /// accounts' clients ask for tickets ([`Backend::takes_mail`]). `None`: it takes none.
    pub mail: Option<MailNews>,
    /// The application places what was left for it on a server ([`Backend::reachable`]): `Some`
    /// reads the server's notes when a connection to it comes up. `None`: it reads none.
    pub notes: Option<NotesNews>,
    /// The servers a program's reach has set up in this run ([`SshBackend::install_for_reach`]).
    pub filed: Mutex<std::collections::HashSet<String>>,
    /// What asks the service for programs and connections on a server ([`Backend::serving`]).
    pub streams: std::sync::OnceLock<Streams>,
    /// The `kablo bateri package watch` of each machine a connection is up to
    /// ([`SshBackend::watch_packages`]): `Some` starts one when the connection comes up, on a
    /// thread of its own, so nothing waits for it. `None`: none is started (the timed run keeps the
    /// person's folder out of it).
    pub watches: Option<Arc<crate::apps::Watches>>,
}

/// A [`Channel`] over the connection kept to an account's server ([`Streams::command`]): the
/// script is run by the server's `sh`, started by the bridge, and nothing logs in for it. The
/// limit is the whole step's, the wait for the connection's answer included.
struct KeptChannel {
    streams: Streams,
    account: String,
    limit: Duration,
}

impl Channel for KeptChannel {
    fn run(&self, script: &str, input: &[u8]) -> io::Result<install::Reply> {
        let asked = Instant::now();
        let command = self
            .streams
            .command(&self.account, served::sh(script), self.limit)
            .map_err(io::Error::other)?;
        let left = self.limit.saturating_sub(asked.elapsed());
        // Whole: a step's answer is read by whoever asked, as an `ssh`'s is.
        install::reply_of(command, input, u64::MAX, Some(left))
    }
}

impl SshBackend {
    /// A server bateri has put its files on, or this Mac: `Err` is the line that says to open a
    /// terminal on it first. Nothing is asked of a server bateri has never touched.
    fn touched(&self, server: &KabloHost, name: &str) -> Result<(), String> {
        if ssh_wrap::load(&self.state_path).knows(Fact::Touched, &server.key) {
            Ok(())
        } else {
            Err(format!(
                "bateri has not set up {name} yet: open a terminal on it in bateri first (ssh \
                 {name}), then try again"
            ))
        }
    }

    /// The connection to `server` is one a stream can wait for: the server is one bateri keeps
    /// terminals on, and the application's master to it is up (made now, asking for a password
    /// through `ask` when it must be), as a pane of it would have it. This Mac needs none.
    fn connected(&self, server: &KabloHost, ask: Ask) -> Result<(), String> {
        if local::is_account(&server.key) {
            return Ok(());
        }
        self.touched(server, &server.host)?;
        // A connection that is up needs no sign-in: the `ssh` that carries it is the login, and
        // looking at the route would start `ssh -G` and `ssh -O check` for every command.
        if self
            .streams()
            .is_ok_and(|streams| streams.live(&server.key))
        {
            return Ok(());
        }
        if let Some(masters) = &self.masters {
            masters
                .ensure(&self.target(server), ask)
                .map_err(|denied| denied.text())?;
        }
        Ok(())
    }

    fn streams(&self) -> Result<&Streams, String> {
        self.streams
            .get()
            .ok_or_else(|| "bateri's connections are not running".to_owned())
    }

    /// The streams for a job: a person's when it has a sheet to ask with ([`Ask::Sheet`]), that
    /// is, when a person started it.
    fn streams_for(&self, person: bool) -> Result<Streams, String> {
        let streams = self.streams()?;
        Ok(if person {
            streams.of_a_person()
        } else {
            streams.clone()
        })
    }

    /// What a program's `bateri` runs on a server is kablo there, and the one-line file that
    /// starts it: a program that reaches a server before any terminal of this run did would run
    /// what an earlier build left (or, with no package's kablo there, nothing). So a program's
    /// reach does the whole of the install ([`install::ensure_for`]: kablo, the shells' files and
    /// the command) once a run, and renews what runs from an earlier kablo (`live`, the ledger's
    /// terminals of the server) when it wrote the binary. Once a run, since `send` and `capture`
    /// reach too; one that failed is tried by the next, and says nothing: the program's own line
    /// says what is wrong on that server.
    fn install_for_reach(
        &self,
        server: &KabloHost,
        argv: &[String],
        live: &dyn Fn(&str) -> Vec<SessionId>,
    ) {
        let Some(bundle) = &self.bundle else {
            return;
        };
        // Whether the command and the hooks are wanted is part of what was done: a server whose
        // level went to `off` (the file taken away) and back, or whose agent's setting turned, is
        // set up again by the next reach.
        let wants = self.wants(server);
        let done = format!("{}#{wants:?}", server.key);
        if self.filed.lock().is_ok_and(|filed| filed.contains(&done)) {
            return;
        }
        let channel = SshChannel {
            argv: argv.to_vec(),
            limit: install::STEP_LIMIT,
        };
        let Ok(installed) = install::ensure_for(&channel, bundle, wants) else {
            return;
        };
        if installed.fresh {
            renew(argv, &installed, &live(&server.key));
        }
        if let Ok(mut filed) = self.filed.lock() {
            filed.insert(done);
        }
    }

    /// Whether the `bateri` command goes to this server: not when its level is `off`
    /// ([`Settings::commands_for`]), where nothing of it is to be found. This Mac's level is
    /// `[commands] level`.
    fn agent_wanted(&self, server: &KabloHost) -> bool {
        self.wants(server).agent
    }

    /// What goes to this server of the files that depend on the settings: [`Self::agent_wanted`]'s
    /// command and the hooks of the agents whose setting is on ([`Wants::of`]), read from the
    /// file once.
    fn wants(&self, server: &KabloHost) -> Wants {
        let settings = (self.settings)();
        let agent = if local::is_account(&server.key) {
            settings.commands_level != CommandLevel::Off
        } else {
            settings.commands_for(&server.host) != CommandLevel::Off
        };
        Wants::of(&settings, agent)
    }

    /// The server as the file jobs know a target: the user's arguments, with
    /// the ssh program ours.
    fn target(&self, server: &KabloHost) -> RemoteTarget {
        let mut argv = server.argv.clone();
        if let Some(program) = argv.first_mut() {
            program.clone_from(&self.ssh);
        }
        RemoteTarget {
            host: server.host.clone(),
            port: server.port,
            kind: RemoteKind::Ssh,
            line: crate::quote::command_line(&argv),
            argv,
        }
    }

    /// The route the bridge takes when it is made again. The user's own master
    /// may be gone with the connection: the one the application keeps for the
    /// account is opened again with the password the Keychain holds, **once and
    /// silently** (a stale password must not lock the account), and a login
    /// nobody saved is the user's to give — `PermissionDenied`, which the
    /// service reads as "needs a login".
    fn route_for_bridge(&self, target: &RemoteTarget) -> io::Result<Route> {
        let Some(masters) = &self.masters else {
            return Ok(self.route_of(target));
        };
        match masters.ensure(target, Ask::Never) {
            Ok(route) => Ok(route),
            Err(Denied::SignIn) => Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "the server wants a login",
            )),
            Err(denied) => Err(io::Error::other(denied.text())),
        }
    }

    /// The route a command to the server takes now, and the ssh argv for it.
    fn route_of(&self, target: &RemoteTarget) -> Route {
        match ssh_route::plan(&*self.runner, target, &self.bases) {
            Plan::Ready(route) => route,
            // No master is alive: a command still tries, with what ssh can do
            // without a person (a key, an agent).
            Plan::Open(_) => Route::Direct,
        }
    }

    /// The way a step reaches `server` now. Over the connection kept to it when that is up
    /// ([`KeptChannel`]): the script is a program the bridge starts, a round trip on a
    /// connection that has logged in, which is what every job on a server takes. Else an `ssh`
    /// of its own, as the steps before there is a connection are: it rides a master that is
    /// there and logs in again when none is, which on a server reached through a proxy is
    /// seconds — and a pane that waits for the step waits them out.
    fn step_channel(&self, server: &KabloHost, limit: Duration) -> Box<dyn Channel> {
        if let Ok(streams) = self.streams()
            && streams.live(&server.key)
        {
            return Box::new(KeptChannel {
                streams: streams.clone(),
                account: server.key.clone(),
                limit,
            });
        }
        let target = self.target(server);
        Box::new(SshChannel {
            argv: ssh_argv_for(&target, &self.route_of(&target)),
            limit,
        })
    }

    /// The package and the home directory the account of this Mac needs; none of either, no
    /// account.
    fn here(&self) -> Option<(&Bundle, &Path)> {
        Some((self.bundle.as_ref()?, self.home.as_deref()?))
    }

    /// kablo for this Mac ([`install::ensure_here`]); a failure is no kablo here, for now: the
    /// Mac is not a server that is remembered as one kablo cannot be kept on.
    fn install_here(&self, live: &[SessionId]) -> Result<Installed, Refusal> {
        let (bundle, home) = self.here().ok_or_else(Refusal::plain)?;
        let channel = LocalChannel::new(home.to_owned());
        let installed = install::ensure_here(&channel, bundle, self.wants(&local::host()))
            .map_err(|_| Refusal::plain())?;
        if installed.fresh {
            // The binary was not there: terminals this user has here run an earlier one.
            settle(&here_channel(home), &installed, live);
        }
        Ok(installed)
    }

    /// What a program on the server left for the app while no connection was up
    /// ([`Backend::reachable`]).
    fn read_notes(&self, account: &str, server: &KabloHost) {
        let Some(news) = &self.notes else {
            return;
        };
        // A server's script leaves notes; this Mac has no such script.
        if local::is_account(&server.key) {
            return;
        }
        // Only a server kablo is kept on has been written to by the command.
        if self.bundle.is_none() {
            return;
        }
        let channel = self.step_channel(server, install::NOTES_LIMIT);
        let Ok(reply) = channel.run(crate::agent::notes::READ, &[]) else {
            return;
        };
        if reply.status != Some(0) {
            return;
        }
        let notes = crate::agent::notes::parse(reply.out.as_bytes());
        if notes.is_empty() {
            return;
        }
        news(
            crate::agent::serve::Server {
                account: account.to_owned(),
                host: server.host.clone(),
                argv: server.argv.clone(),
                level: (self.settings)().commands_for(&server.host),
            },
            notes,
        );
    }

    /// Whether a machine has the programs watched: kablo is kept on it, the `bateri` command is
    /// wanted there (`commands = "off"` puts nothing of ours), and, for a server, its shells get
    /// bateri's integration ([`Settings::integration_for`]), which is what puts the programs'
    /// folder on `PATH`.
    fn watch_wanted(&self, server: &KabloHost) -> bool {
        self.bundle.is_some()
            && self.agent_wanted(server)
            && (local::is_account(&server.key) || (self.settings)().integration_for(&server.host))
    }

    /// Starts `kablo bateri package watch` on the machine the connection is up to and waits for it
    /// as long as it lives, so that the programs of the machine are looked at: when the person is
    /// connected, and only then ([`SshBackend::watch_wanted`]).
    fn watch_packages(&self, server: &KabloHost) {
        let Some(watches) = &self.watches else {
            return;
        };
        if !self.watch_wanted(server) {
            return;
        }
        let here = local::is_account(&server.key);
        let words = ["package".to_owned(), "watch".to_owned()];
        watches.keep(&server.key, &|| {
            self.command(
                server,
                crate::agent::relay::start(here, &server.host, &words),
                Ask::Never,
                crate::apps::upload::WAIT,
            )
        });
    }

    fn remember(&self, fact: Fact, server: &KabloHost) {
        let _ = ssh_wrap::record(
            &self.state_path,
            fact,
            &server.key,
            ssh_wrap::unix_now(),
            ssh_wrap::LOCK_PATIENCE,
        );
    }
}

impl Backend for SshBackend {
    fn pane(&self, peer: &Peer) -> Option<PaneInfo> {
        (self.panes)(peer)
    }

    fn route(&self, args: &[String]) -> Option<KabloHost> {
        self.bundle.as_ref()?;
        let state = ssh_wrap::load(&self.state_path);
        ssh_wrap::kablo_path(args, &(self.settings)(), &*self.runner, &state)
    }

    fn master(&self, server: &KabloHost) -> Result<Option<Vec<Vec<u8>>>, Refusal> {
        // There is no connection to make to this Mac.
        if local::is_account(&server.key) {
            return Ok(None);
        }
        let target = self.target(server);
        match ssh_route::plan(&*self.runner, &target, &self.bases) {
            Plan::Ready(_) => Ok(None),
            Plan::Open(socket) => Ok(Some(
                ssh_route::master_argv(&target, &socket, Asker::Terminal)
                    .into_iter()
                    .map(String::into_bytes)
                    .collect(),
            )),
        }
    }

    fn master_made(&self, server: &KabloHost, status: u8) -> Result<(), Refusal> {
        if local::is_account(&server.key) {
            return Ok(());
        }
        match status {
            0 => {}
            // ssh's own failure, a signal (Ctrl-C at the prompt): what the user
            // saw is the answer; a second ssh would ask the same questions.
            128..=255 => return Err(Refusal::ended("")),
            _ => return Err(Refusal::plain()),
        }
        match ssh_route::plan(&*self.runner, &self.target(server), &self.bases) {
            Plan::Ready(_) => Ok(()),
            // The connection did not become a master we can find: nothing to
            // ride on.
            Plan::Open(_) => Err(Refusal::plain()),
        }
    }

    fn install(&self, server: &KabloHost, live: &[SessionId]) -> Result<Installed, Refusal> {
        if local::is_account(&server.key) {
            return self.install_here(live);
        }
        let bundle = self.bundle.as_ref().ok_or_else(Refusal::plain)?;
        let target = self.target(server);
        let channel = SshChannel {
            argv: ssh_argv_for(&target, &self.route_of(&target)),
            limit: install::STEP_LIMIT,
        };
        match install::ensure_for(&channel, bundle, self.wants(server)) {
            Ok(installed) => {
                self.remember(Fact::Touched, server);
                if installed.fresh {
                    // The binary was not there: terminals this user has on the
                    // server run an earlier one, and are renewed before a bridge
                    // exists to be left behind.
                    renew(&channel.argv, &installed, live);
                }
                Ok(installed)
            }
            Err(Failure::Impossible(_)) => {
                // This server is a plain one from now on.
                self.remember(Fact::NoKablo, server);
                Err(Refusal::plain())
            }
            Err(Failure::Transient(_)) => Err(Refusal::plain()),
        }
    }

    fn files(&self, server: &KabloHost, installed: &Installed) {
        let wants = self.wants(server);
        // What a reach has done for this server ([`Self::install_for_reach`]) under other wants
        // is no longer what its files are: a reach with those again must write them, not find
        // them done.
        if let Ok(mut filed) = self.filed.lock() {
            let kept = format!("{}#{wants:?}", server.key);
            let prefix = format!("{}#", server.key);
            filed.retain(|done| *done == kept || !done.starts_with(&prefix));
        }
        // The shells of this Mac start from the application's own files: what it has of these
        // is the agent's command and its counter.
        if local::is_account(&server.key) {
            if let Some((_, home)) = self.here() {
                let channel = LocalChannel::new(home.to_owned());
                install::put_files_here(&channel, installed, wants);
            }
            return;
        }
        let channel = self.step_channel(server, install::STEP_LIMIT);
        install::put_files(&*channel, installed, wants);
    }

    fn stage_folder(&self, server: &KabloHost) -> Option<crate::stage::Folder> {
        // This Mac's programs have the folder the application made; a run that keeps no
        // masters (the timed one) names no instance to keep a folder for.
        if local::is_account(&server.key) {
            return None;
        }
        let instance = self.masters.as_ref()?.instance();
        let channel = self.step_channel(server, install::STEP_LIMIT);
        let name = crate::stage::folder_name(instance, &server.key, &crate::stage::this_machine());
        install::prepare_stage(&*channel, &name)
    }

    fn stage_ready(&self, account: &str) {
        (self.staged)(account);
    }

    fn number(&self, server: &KabloHost) -> Option<u64> {
        // Only a machine kablo is kept on has numbers to give. This Mac's terminals are numbered
        // by the same counter, run here, unless the command is not wanted (`off`): they then keep
        // the names they are made with.
        let kablo = self.bundle.as_ref()?.remote_path();
        if local::is_account(&server.key) {
            if !self.agent_wanted(server) {
                return None;
            }
            let (_, home) = self.here()?;
            let channel = LocalChannel {
                home: home.to_owned(),
                limit: install::LOCAL_NUMBER_LIMIT,
            };
            return install::next_number(&channel, &kablo);
        }
        // The pane that asked waits for this step: over the connection the server's terminals
        // are on it is a round trip, and only the first terminal, which is started before there
        // is one, is numbered by an `ssh` (on the master its own login just made).
        let channel = self.step_channel(server, install::NUMBER_LIMIT);
        install::next_number(&*channel, &kablo)
    }

    fn carry(&self, server: &KabloHost, installed: &Installed) -> io::Result<Carried> {
        if local::is_account(&server.key) {
            let home = self
                .home
                .as_deref()
                .ok_or_else(|| io::Error::other("this Mac has no home to keep kablo in"))?;
            return local::carry(home, installed);
        }
        let target = self.target(server);
        let route = self.route_for_bridge(&target)?;
        let mut argv = ssh_argv_for(&target, &route);
        argv.push(format!("{} attach", installed.path));
        let (program, args) = argv
            .split_first()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "empty argv"))?;
        let mut child = ssh_route::command(program)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let (Some(stdin), Some(stdout), Some(stderr)) =
            (child.stdin.take(), child.stdout.take(), child.stderr.take())
        else {
            crate::program::end_group(&mut child);
            return Err(io::Error::other("the bridge has no pipes"));
        };
        // The ssh is held by `close` and by `ended`, whichever comes first: the bridge's
        // end is read from the ssh's status, and the group it leads is ended only while the
        // ssh is not reaped yet ([`crate::program::end_group`]).
        let child = Arc::new(Mutex::new(Some(child)));
        // What ssh says when it is refused is the only way to tell a login that
        // nobody answered from a network that is down: both end the stream.
        let said = Arc::new(Mutex::new(String::new()));
        let reading = thread::Builder::new()
            .name("kablo bridge's ssh says".into())
            .spawn({
                let said = Arc::clone(&said);
                move || read_said(stderr, &said)
            })
            .ok();
        Ok(Carried {
            reader: Box::new(stdout),
            writer: Box::new(stdin),
            // The ssh with its proxy: a bridge that never opened is ended at every attempt.
            close: Box::new({
                let child = Arc::clone(&child);
                move || {
                    if let Some(mut ssh) = child.lock().unwrap_or_else(|e| e.into_inner()).take() {
                        crate::program::end_group(&mut ssh);
                    }
                }
            }),
            ended: Box::new(move || {
                // The ssh first: what it led goes with it, and with that whatever held its
                // stderr, which the words are read from.
                let status = exit_status_of(&child);
                if let Some(reading) = reading {
                    let _ = reading.join();
                }
                let said = said.lock().unwrap_or_else(|e| e.into_inner());
                why_it_ended(&said, status)
            }),
        })
    }

    fn version(&self) -> Option<String> {
        self.bundle
            .as_ref()
            .map(|bundle| bundle.version().to_owned())
    }

    fn refresh(&self, server: &KabloHost, live: &[SessionId]) -> Option<Installed> {
        if local::is_account(&server.key) {
            let (bundle, home) = self.here()?;
            let installed = install::ensure_here(
                &LocalChannel::new(home.to_owned()),
                bundle,
                self.wants(server),
            )
            .ok()?;
            settle(&here_channel(home), &installed, live);
            return Some(installed);
        }
        let bundle = self.bundle.as_ref()?;
        let target = self.target(server);
        // The connection the bridge is about to take, made here when it is not
        // there: both steps ride one and a login nobody saved is the bridge's to
        // report. No connection, no renewal; the bridge's turn comes anyway.
        let route = self.route_for_bridge(&target).ok()?;
        let argv = bounded(ssh_argv_for(&target, &route));
        let channel = SshChannel {
            argv: argv.clone(),
            limit: install::STEP_LIMIT,
        };
        // A server that cannot keep kablo *now* is not recorded as one that never
        // can: terminals are running there, and what `install` would write down
        // changes what the next `ssh` is.
        let installed = install::ensure_for(&channel, bundle, self.wants(server)).ok()?;
        renew(&argv, &installed, live);
        Some(installed)
    }

    fn name(&self) -> Option<String> {
        self.name.clone()
    }

    fn history(&self) -> Option<u32> {
        (self.history)()
    }

    fn shows_past(&self) -> bool {
        (self.past)()
    }

    fn dock(&self) -> bool {
        (self.settings)().shell_integration.wants_dock()
    }

    fn terminal_changed(&self, pane: &PaneUuid) {
        (self.changed)(pane);
    }

    fn program_changed(&self, pane: &PaneUuid) {
        (self.program)(pane);
    }

    fn link_changed(&self, pane: &PaneUuid, link: PaneLink) {
        (self.linked)(pane, link);
    }

    fn background_changed(&self, account: &str) {
        (self.listed)(account);
    }

    fn takes_mail(&self) -> bool {
        self.mail.is_some()
    }

    fn mail(&self, letter: Letter) {
        if let Some(mail) = &self.mail {
            mail(letter);
        }
    }

    fn reach(
        &self,
        args: &[String],
        ask: Ask,
        live: &dyn Fn(&str) -> Vec<SessionId>,
    ) -> Result<Reached, String> {
        let name = args.first().map_or("the server", String::as_str);
        // The gate of the server's own pane: settings, `ssh -G`, what is known of the server.
        let server = self.route(args).ok_or_else(|| {
            format!(
                "{name} is not a server bateri keeps terminals on: its ssh is a plain one (the \
                 settings, a command, or a server kablo cannot be kept on)"
            )
        })?;
        if local::is_account(&server.key) {
            return Err(format!("{name} is this computer"));
        }
        // A server bateri has never put its files on has no `bateri` to start a terminal with.
        self.touched(&server, name)?;
        let level = (self.settings)().commands_for(&server.host);
        // Nothing is done for a server whose commands are off, a connection included: the
        // caller refuses, naming the setting.
        if level == CommandLevel::Off {
            return Ok(Reached { server, level });
        }
        let target = self.target(&server);
        let person = matches!(ask, Ask::Sheet(_));
        let route = match &self.masters {
            Some(masters) => masters
                .ensure(&target, ask)
                .map_err(|denied| denied.text())?,
            None => self.route_of(&target),
        };
        // The person who was asked for the login has given it: the connection that waited for it
        // dials now, for the program that came after (it asks nothing itself).
        if person && let Ok(streams) = self.streams() {
            streams.dial(&server.key);
        }
        let argv = ssh_argv_for(&target, &route);
        self.install_for_reach(&server, &argv, live);
        Ok(Reached { server, level })
    }

    fn serving(&self, streams: Streams) {
        let _ = self.streams.set(streams);
    }

    fn command(
        &self,
        server: &KabloHost,
        command: Command,
        ask: Ask,
        wait: Duration,
    ) -> Result<RemoteCommand, String> {
        let person = matches!(ask, Ask::Sheet(_));
        self.connected(server, ask)?;
        self.streams_for(person)?
            .command(&server.key, command, wait)
    }

    fn connection(
        &self,
        server: &KabloHost,
        host: &str,
        port: u16,
        ask: Ask,
        wait: Duration,
    ) -> Result<RemoteStream, String> {
        let person = matches!(ask, Ask::Sheet(_));
        self.connected(server, ask)?;
        self.streams_for(person)?
            .connection(&server.key, host, port, wait)
    }

    fn reachable(&self, account: &str, server: &KabloHost) {
        self.read_notes(account, server);
        // Last, for it lasts as long as the connection does.
        self.watch_packages(server);
    }

    fn records(
        &self,
        server: &KabloHost,
    ) -> Vec<(crate::agent::Terminal, crate::agent::notes::Record)> {
        // Only a server kablo is kept on has been written to by the command.
        if self.bundle.is_none() || local::is_account(&server.key) {
            return Vec::new();
        }
        let channel = self.step_channel(server, install::RECORDS_LIMIT);
        match channel.run(crate::agent::notes::RECORDS, &[]) {
            Ok(reply) if reply.status == Some(0) => {
                crate::agent::notes::parse_records(reply.out.as_bytes())
            }
            _ => Vec::new(),
        }
    }
}

/// The server's terminals that bateri keeps (`live`, the ledger's) are renewed with the binary
/// just checked ([`install::upgrade_sessions`]) and the version they left is removed when
/// nothing runs from it ([`install::prune_versions`]), on a channel of its own that is let less
/// time than the install: what is waiting for it is a pane that shows a connection being made.
fn renew(argv: &[String], installed: &Installed, live: &[SessionId]) {
    let channel = SshChannel {
        argv: argv.to_vec(),
        limit: install::UPGRADE_LIMIT,
    };
    settle(&channel, installed, live);
}

/// [`renew`] on any channel: the same two steps for a server and for this Mac.
fn settle(channel: &dyn Channel, installed: &Installed, live: &[SessionId]) {
    install::upgrade_sessions(channel, installed, live);
    install::prune_versions(channel, installed);
}

/// This Mac's channel for [`settle`]: the bound of a renewal, not of an install.
fn here_channel(home: &Path) -> LocalChannel {
    LocalChannel {
        home: home.to_owned(),
        limit: install::UPGRADE_LIMIT,
    }
}

/// How long a connection that is made again for a renewal may take to be made
/// (`ConnectTimeout`): a network that is not there is not waited for by the
/// time of a TCP retry, the bridge that follows finds that out by itself.
const CONNECT_SECONDS: u32 = 15;

/// The ssh argv with a connect timeout of ours before the user's options (the
/// first value ssh reads for an option wins).
fn bounded(mut argv: Vec<String>) -> Vec<String> {
    if !argv.is_empty() {
        argv.splice(
            1..1,
            ["-o".to_owned(), format!("ConnectTimeout={CONNECT_SECONDS}")],
        );
    }
    argv
}

/// The most of ssh's standard error that is kept: a refusal is a line or two.
const SAID_LIMIT: usize = 16 * 1024;

/// Reads what the bridge's ssh says on its standard error into `said` until the
/// process lets go of it.
fn read_said(mut stderr: impl Read, said: &Mutex<String>) {
    let mut buffer = [0u8; 1024];
    while let Ok(n) = stderr.read(&mut buffer) {
        if n == 0 {
            break;
        }
        let mut said = said.lock().unwrap_or_else(|e| e.into_inner());
        if said.len() < SAID_LIMIT {
            said.push_str(&String::from_utf8_lossy(&buffer[..n]));
        }
    }
}

/// Why a bridge whose ssh ended by itself ended, from what ssh said and the status it ended
/// with (`None`: it was let go of, or ended by a signal, or did not end in time to be asked).
/// A key that is not the trusted one is a connection that stops and asks nothing; a refused
/// login that a password could have opened is the user's to give; a status of the far shell that
/// says it could not run the command (127 not found, 126 not executable) is a kablo that is not
/// there; the rest is the network and is tried again. The status, not the words: shells write
/// "not found" in their own ways, and ssh's own failures are 255.
pub fn why_it_ended(said: &str, status: Option<i32>) -> Cause {
    if said.contains("Host key verification failed")
        || said.contains("REMOTE HOST IDENTIFICATION HAS CHANGED")
    {
        Cause::Refused
    } else if ssh_route::password_refused(said) {
        Cause::LoginNeeded
    } else if matches!(status, Some(126 | 127)) {
        Cause::Missing
    } else {
        Cause::Dropped
    }
}

/// How long a bridge's ssh, whose stream has ended, is given to end too before its status is
/// given up on.
const EXIT_WAIT: Duration = Duration::from_secs(2);

/// The exit status of the bridge's ssh, which is asked about here until it has ended or
/// [`EXIT_WAIT`] is over. An ssh that has ended is not reaped before what it led is ended
/// ([`crate::program::end_group`] signals the group by the pid, which only a child not reaped
/// still owns), and it is reaped by that. `None` when `close` ended it first, when it did not end
/// in time (left to `close`), or when a signal ended it.
fn exit_status_of(child: &Mutex<Option<std::process::Child>>) -> Option<i32> {
    let until = std::time::Instant::now() + EXIT_WAIT;
    loop {
        {
            let mut slot = child.lock().unwrap_or_else(|e| e.into_inner());
            let ssh = slot.as_mut()?;
            if let Some(code) = crate::program::ended_with(ssh) {
                crate::program::end_group(ssh);
                *slot = None;
                return code;
            }
        }
        if std::time::Instant::now() >= until {
            return None;
        }
        thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(test)]
mod tests;
