//! The service's steps over ssh ([`SshBackend`]): the master connection the
//! user makes on the pane, kablo put on the server, and the bridge.
//!
//! **Everything rides one connection.** The first `ssh` to a server makes a
//! master connection **on the pane's own terminal** — the program is `ssh -M -N
//! -f …` and runs where the user is, so a password, a second factor and the
//! question whether to trust a server's key are asked and answered where they
//! always were; the connection goes to the background once it is made. It lives
//! at the socket the rest of bateri already looks for
//! ([`crate::ssh_route::socket_path`]): the file jobs, the preview and the rest
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
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;

use bt_core::{PaneUuid, RemoteKind, RemoteTarget, Settings};

use super::Cause;
use super::install::{self, Bundle, Failure, Installed, SshChannel};
use super::service::{Backend, Carried, PaneInfo, PaneLink, Peer, Refusal};
use crate::ssh_route::{self, Ask, Asker, Denied, Masters, Plan, Route, SshRunner};
use crate::ssh_wrap::{self, Fact, KabloHost};
use crate::upload::ssh_argv_for;

/// How a process is found to be in a pane (see [`SshBackend::panes`]).
pub type PaneLookup = Box<dyn Fn(&Peer) -> Option<PaneInfo> + Send + Sync>;

/// What the application does with the news that a pane's connection went or
/// came back ([`Backend::link_changed`]).
pub type LinkNews = Box<dyn Fn(&PaneUuid, PaneLink) + Send + Sync>;

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
    /// The application's masters: what makes the connection again, with the
    /// password the Keychain keeps, when the one the user made is gone. `None`:
    /// only what `ssh` can do without a person (a key, an agent).
    pub masters: Option<Arc<Masters>>,
    /// A pane's terminal lost the connection or got it back
    /// ([`Backend::link_changed`]).
    pub linked: LinkNews,
}

impl SshBackend {
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

    fn install(&self, server: &KabloHost) -> Result<Installed, Refusal> {
        let bundle = self.bundle.as_ref().ok_or_else(Refusal::plain)?;
        let target = self.target(server);
        let channel = SshChannel {
            argv: ssh_argv_for(&target, &self.route_of(&target)),
            limit: install::STEP_LIMIT,
        };
        match install::ensure(&channel, bundle) {
            Ok(installed) => {
                self.remember(Fact::Touched, server);
                if installed.fresh {
                    // The binary was not there: terminals this user has on the
                    // server run an earlier one, and are renewed before a bridge
                    // exists to be left behind.
                    renew(&channel.argv, &installed);
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

    fn carry(&self, server: &KabloHost, installed: &Installed) -> io::Result<Carried> {
        let target = self.target(server);
        let route = self.route_for_bridge(&target)?;
        let mut argv = ssh_argv_for(&target, &route);
        argv.push(format!("{} attach", installed.path));
        let (program, args) = argv
            .split_first()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "empty argv"))?;
        let mut child = Command::new(program)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let (Some(stdin), Some(stdout), Some(stderr)) =
            (child.stdin.take(), child.stdout.take(), child.stderr.take())
        else {
            let _ = child.kill();
            let _ = child.wait();
            return Err(io::Error::other("the bridge has no pipes"));
        };
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
            close: Box::new(move || {
                let _ = child.kill();
                let _ = child.wait();
            }),
            ended: Box::new(move || {
                if let Some(reading) = reading {
                    let _ = reading.join();
                }
                let said = said.lock().unwrap_or_else(|e| e.into_inner());
                why_it_ended(&said)
            }),
        })
    }

    fn version(&self) -> Option<String> {
        self.bundle
            .as_ref()
            .map(|bundle| bundle.version().to_owned())
    }

    fn refresh(&self, server: &KabloHost) -> Option<Installed> {
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
        let installed = install::ensure(&channel, bundle).ok()?;
        renew(&argv, &installed);
        Some(installed)
    }

    fn name(&self) -> Option<String> {
        self.name.clone()
    }

    fn history(&self) -> Option<u32> {
        (self.history)()
    }

    fn link_changed(&self, pane: &PaneUuid, link: PaneLink) {
        (self.linked)(pane, link);
    }
}

/// The server's running terminals are renewed with the binary just checked
/// ([`install::upgrade_sessions`]), on a channel of its own that is let less
/// time than the install: what is waiting for it is a pane that shows a
/// connection being made.
fn renew(argv: &[String], installed: &Installed) {
    let channel = SshChannel {
        argv: argv.to_vec(),
        limit: install::UPGRADE_LIMIT,
    };
    install::upgrade_sessions(&channel, installed);
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

/// Why a bridge whose ssh ended by itself ended, from what ssh said. A key that
/// is not the trusted one is a connection that stops and asks nothing; a
/// refused login that a password could have opened is the user's to give; the
/// rest is the network and is tried again.
pub fn why_it_ended(said: &str) -> Cause {
    if said.contains("Host key verification failed")
        || said.contains("REMOTE HOST IDENTIFICATION HAS CHANGED")
    {
        Cause::Refused
    } else if ssh_route::password_refused(said) {
        Cause::LoginNeeded
    } else {
        Cause::Dropped
    }
}

#[cfg(test)]
mod tests;
