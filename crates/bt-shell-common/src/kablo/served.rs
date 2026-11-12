//! The server a job of the application is served by: what a job holds to run a program there
//! over the one connection kept to it ([`Backend::command`]) and does not know the service or the
//! account by.
//!
//! Every job that runs on a server (the probe and the stream of a drop, the download, the
//! helper's questions, a program's file transfer) runs `sh -c <script>`: the words are the same
//! and the server's `sh` reads them, whatever shell the person logs in with. The first
//! connection is made like a pane's: a job the person started has a sheet ask for a password
//! (`ask`); one that did not asks nobody, and says why when it cannot go on.

use std::sync::Arc;
use std::time::Duration;

use kablo_proto::v1::stream::Command;

use super::remote::{RemoteCommand, RemoteStream};
use super::service::Backend;
use crate::ssh_route::Ask;
use crate::ssh_wrap::KabloHost;

/// What runs a program on a server for a job.
pub trait Runs: Send + Sync {
    /// Runs `command` as [`Backend::command`] says.
    fn run(&self, command: Command, ask: Ask, wait: Duration) -> Result<RemoteCommand, String>;

    /// Has the server connect to `host:port`, as [`Backend::connection`] says.
    fn connect(
        &self,
        host: &str,
        port: u16,
        ask: Ask,
        wait: Duration,
    ) -> Result<RemoteStream, String>;
}

/// The backend and the server a job is served by.
struct Through {
    backend: Arc<dyn Backend>,
    server: KabloHost,
}

impl Runs for Through {
    fn run(&self, command: Command, ask: Ask, wait: Duration) -> Result<RemoteCommand, String> {
        self.backend.command(&self.server, command, ask, wait)
    }

    fn connect(
        &self,
        host: &str,
        port: u16,
        ask: Ask,
        wait: Duration,
    ) -> Result<RemoteStream, String> {
        self.backend.connection(&self.server, host, port, ask, wait)
    }
}

/// The POSIX `script` as a program for the server's `sh`: the words every job on a server runs
/// ([`Served::sh`]), which no login shell reads on the way.
pub(crate) fn sh(script: &str) -> Command {
    let words = ["sh", "-c", script];
    Command::new(words.iter().map(|word| word.as_bytes().to_vec()).collect())
}

/// A server, as a job runs programs on it. Cheap to clone and to send to a thread.
#[derive(Clone)]
pub struct Served {
    /// `None` in a process that keeps no connections.
    runs: Option<Arc<dyn Runs>>,
}

impl Served {
    /// The server `server`, through the connections `backend` keeps.
    pub fn new(backend: Arc<dyn Backend>, server: KabloHost) -> Self {
        Self {
            runs: Some(Arc::new(Through { backend, server })),
        }
    }

    /// No server: every program asked for is refused, and says that no connections are kept.
    pub fn none() -> Self {
        Self { runs: None }
    }

    /// Runs the POSIX `script` with the server's `sh`. `ask` is how a password is asked for when
    /// the connection has to be made first; the call **waits** for it, and so is made on a thread
    /// of its own and never on the main one. `wait` is how long the connection and the server's
    /// answer may take. The `Err` is a line for the person who asked.
    pub fn sh(&self, script: &str, ask: Ask, wait: Duration) -> Result<RemoteCommand, String> {
        self.run(sh(script), ask, wait)
    }

    /// Runs `command` as it is: its words are the program's `argv`, and nothing reads them on the
    /// way. Waits as [`Served::sh`] does.
    pub fn run(&self, command: Command, ask: Ask, wait: Duration) -> Result<RemoteCommand, String> {
        self.runs()?.run(command, ask, wait)
    }

    /// A TCP connection the server makes to `host:port` (as seen from the server). Waits as
    /// [`Served::sh`] does.
    pub fn connection(
        &self,
        host: &str,
        port: u16,
        ask: Ask,
        wait: Duration,
    ) -> Result<RemoteStream, String> {
        self.runs()?.connect(host, port, ask, wait)
    }

    fn runs(&self) -> Result<&Arc<dyn Runs>, String> {
        self.runs
            .as_ref()
            .ok_or_else(|| "bateri's connections are not running".to_owned())
    }
}

impl std::fmt::Debug for Served {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Served")
    }
}

#[cfg(test)]
pub(crate) mod fake {
    use std::os::unix::process::CommandExt as _;
    use std::process::{Command as Process, Stdio};

    use super::*;

    /// What ssh does with the command it carries: the login shell on the other side reads it as
    /// `sh -c '<script>'` ([`remote_command`]).
    pub(crate) fn remote_command(script: &str) -> String {
        format!("sh -c {}", crate::upload::sq(script))
    }

    /// A server that refuses to make connections, and says `why`.
    struct Refuses(String);

    impl Runs for Refuses {
        fn run(&self, _: Command, _: Ask, _: Duration) -> Result<RemoteCommand, String> {
            Err(self.0.clone())
        }

        fn connect(&self, _: &str, _: u16, _: Ask, _: Duration) -> Result<RemoteStream, String> {
            Err(self.0.clone())
        }
    }

    /// A program that stands in for ssh: `argv` and then the command, as ssh would be given them.
    struct Ssh(Vec<String>);

    impl Runs for Ssh {
        fn run(&self, command: Command, _: Ask, _: Duration) -> Result<RemoteCommand, String> {
            let script = command
                .argv
                .last()
                .map(|word| String::from_utf8_lossy(word).into_owned())
                .ok_or("no command")?;
            let (program, args) = self.0.split_first().ok_or("no program")?;
            let child = Process::new(program)
                .args(args)
                .arg(remote_command(&script))
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .process_group(0)
                .spawn()
                .map_err(|error| error.to_string())?;
            Ok(RemoteCommand::local(child))
        }

        fn connect(&self, _: &str, _: u16, _: Ask, _: Duration) -> Result<RemoteStream, String> {
            Err("a stand-in for ssh makes no connections".to_owned())
        }
    }

    /// A server that is a directory here: the program is run with the `argv` it was given, with
    /// `home` for its home and `shell` for the shell the person logs in with.
    struct Home {
        home: std::path::PathBuf,
        shell: std::path::PathBuf,
    }

    impl Runs for Home {
        fn run(&self, command: Command, _: Ask, _: Duration) -> Result<RemoteCommand, String> {
            let words: Vec<String> = command
                .argv
                .iter()
                .map(|word| String::from_utf8_lossy(word).into_owned())
                .collect();
            let (program, args) = words.split_first().ok_or("no program")?;
            let child = Process::new(program)
                .args(args)
                .current_dir(&self.home)
                .env("HOME", &self.home)
                .env("SHELL", &self.shell)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .process_group(0)
                .spawn()
                .map_err(|error| error.to_string())?;
            Ok(RemoteCommand::local(child))
        }

        fn connect(&self, _: &str, _: u16, _: Ask, _: Duration) -> Result<RemoteStream, String> {
            Err("a directory makes no connections".to_owned())
        }
    }

    impl Served {
        /// A server that is the directory `home`, where the person logs in with `shell`.
        pub(crate) fn at_home(home: &std::path::Path, shell: &std::path::Path) -> Self {
            Self {
                runs: Some(Arc::new(Home {
                    home: home.to_owned(),
                    shell: shell.to_owned(),
                })),
            }
        }

        /// A server that refuses everything it is asked, with `why`.
        pub(crate) fn refusing(why: &str) -> Self {
            Self {
                runs: Some(Arc::new(Refuses(why.to_owned()))),
            }
        }

        /// A server that is `argv`, run here in place of ssh.
        pub(crate) fn fake(argv: Vec<String>) -> Self {
            Self {
                runs: Some(Arc::new(Ssh(argv))),
            }
        }
    }
}
