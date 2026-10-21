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

use std::io;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use bt_core::{RemoteKind, RemoteTarget, Settings};

use super::install::{self, Bundle, Failure, Installed, SshChannel};
use super::service::{Backend, Carried, PaneInfo, Peer, Refusal};
use crate::ssh_route::{self, Asker, Plan, Route, SshRunner};
use crate::ssh_wrap::{self, Fact, KabloHost};
use crate::upload::ssh_argv_for;

/// How a process is found to be in a pane (see [`SshBackend::panes`]).
pub type PaneLookup = Box<dyn Fn(&Peer) -> Option<PaneInfo> + Send + Sync>;

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
        let mut argv = ssh_argv_for(&target, &self.route_of(&target));
        argv.push(format!("{} attach", installed.path));
        let (program, args) = argv
            .split_first()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "empty argv"))?;
        let mut child = Command::new(program)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
            let _ = child.kill();
            let _ = child.wait();
            return Err(io::Error::other("the bridge has no pipes"));
        };
        Ok(Carried {
            reader: Box::new(stdout),
            writer: Box::new(stdin),
            close: Box::new(move || {
                let _ = child.kill();
                let _ = child.wait();
            }),
        })
    }

    fn name(&self) -> Option<String> {
        self.name.clone()
    }

    fn history(&self) -> Option<u32> {
        (self.history)()
    }
}

#[cfg(test)]
mod tests;
