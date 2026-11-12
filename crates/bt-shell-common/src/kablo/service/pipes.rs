//! The programs and connections the service runs for callers over an account's connection:
//! [`Streams`] is what a caller (or a [`Backend`](super::Backend)) asks with, and the rest is
//! the service's side of it — the opens that wait for the connection, and the pipes between a
//! stream of the client and the handles a caller holds ([`super::super::remote`]).
//!
//! The caller blocks, the service thread never does: what the server sends goes into a channel
//! the caller's reads take from (and say they have taken, which credits the server), and what the
//! caller writes waits in a bounded channel the service takes from as far as the server's window
//! allows, whenever it settles.

use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use kablo_proto::v1::stream::Command;

use super::{Core, Msg};
use crate::kablo::remote::{self, Far, Port, RemoteCommand, RemoteStream};
use crate::kablo::{Link, Over, Talk, UNSENT_LIMIT};

/// What a stream is the far end of.
pub(super) enum What {
    Command(Command),
    Connection { host: String, port: u16 },
}

/// A caller's request for a stream, waiting for the account's connection.
pub(super) struct StreamOpen {
    account: String,
    what: What,
    port: Arc<ServicePort>,
    far: Far,
    /// A person asked for it ([`Streams::of_a_person`]).
    person: bool,
    /// The connection was told to dial now for it (once: a login that is still wanted is not
    /// tried again and again).
    dialed: bool,
}

/// A stream that is open: the service's ends of its pipes.
pub(super) struct Piped {
    far: Far,
    /// `finish` was said to the client: the caller has nothing more to write.
    finished: bool,
}

/// What the handles of one stream tell the service.
struct ServicePort {
    tx: Sender<Msg>,
    account: String,
    /// The stream's number on the connection, known once the client opened it.
    talk: OnceLock<Talk>,
}

impl Port for ServicePort {
    fn took(&self, bytes: usize) {
        if let Some(talk) = self.talk.get() {
            let _ = self.tx.send(Msg::StreamTook {
                account: self.account.clone(),
                talk: *talk,
                bytes,
            });
        }
    }

    fn wrote(&self) {
        // Any message makes the service settle, which is what moves the caller's bytes on.
        let _ = self.tx.send(Msg::StreamWrote);
    }

    fn hang_up(&self) {
        if let Some(talk) = self.talk.get() {
            let _ = self.tx.send(Msg::StreamHangUp {
                account: self.account.clone(),
                talk: *talk,
            });
        }
    }
}

/// What asks the service for a program or a connection on a server: cheap to clone, usable from
/// any thread. A request waits for the connection to the account's server (and dials it when it is
/// not up), so a caller that may have to ask for a password has the backend do that first
/// ([`Backend::command`](super::Backend::command)).
#[derive(Clone)]
pub struct Streams {
    pub(super) tx: Sender<Msg>,
    /// What is asked for is for a person ([`Streams::of_a_person`]).
    pub(super) person: bool,
}

impl Streams {
    /// The same, for what a person asked for (a command they ran, a file they dropped): a
    /// connection that is waiting out the pause of a failure dials now for it, and one that wants
    /// a login is tried once more. What nobody asked for (a listing, the load
    /// indicator) waits for the connection as it is and does not dial it: every attempt that
    /// fails is a login the server counts.
    pub fn of_a_person(&self) -> Streams {
        Streams {
            tx: self.tx.clone(),
            person: true,
        }
    }

    /// Runs `command` on the server of `account`, as [`RemoteCommand`] says. `wait` is how long
    /// the connection and the server's answer may take. `Err` is a line for the person who asked.
    pub fn command(
        &self,
        account: &str,
        command: Command,
        wait: Duration,
    ) -> Result<RemoteCommand, String> {
        self.open(account, What::Command(command), wait)
            .map(RemoteCommand::of)
    }

    /// Has the server of `account` connect to `host:port`, as [`RemoteStream`] says.
    pub fn connection(
        &self,
        account: &str,
        host: &str,
        port: u16,
        wait: Duration,
    ) -> Result<RemoteStream, String> {
        let what = What::Connection {
            host: host.to_owned(),
            port,
        };
        self.open(account, what, wait).map(RemoteStream::of)
    }

    /// A person signed in for `account`'s server (a sheet they answered): a connection that
    /// waited for the login dials now, as when a pane's login is done.
    pub fn dial(&self, account: &str) {
        let _ = self.tx.send(Msg::Retry(account.to_owned()));
    }

    /// Whether the connection to the server of `account` is up now: then a caller needs no
    /// sign-in first, and none is looked at.
    pub fn live(&self, account: &str) -> bool {
        let (reply, answer) = mpsc::channel();
        let asked = Msg::StreamLive {
            account: account.to_owned(),
            reply,
        };
        self.tx.send(asked).is_ok() && answer.recv_timeout(Duration::from_secs(2)).unwrap_or(false)
    }

    fn open(&self, account: &str, what: What, wait: Duration) -> Result<remote::Opened, String> {
        let port = Arc::new(ServicePort {
            tx: self.tx.clone(),
            account: account.to_owned(),
            talk: OnceLock::new(),
        });
        let (handles, far) = remote::pipes(Arc::clone(&port) as Arc<dyn Port>);
        self.tx
            .send(Msg::StreamOpen(StreamOpen {
                account: account.to_owned(),
                what,
                port,
                far,
                person: self.person,
                dialed: false,
            }))
            .map_err(|_| "bateri is shutting down".to_owned())?;
        handles.opened(wait)
    }
}

impl Core {
    /// Whether the account's connection is up ([`Streams::live`]).
    pub(super) fn stream_live(&mut self, account: &str) -> bool {
        self.hub
            .client_mut(account)
            .is_some_and(|client| matches!(client.link(), Link::Live))
    }

    /// A caller wants a stream: it waits for the account's connection. A server this process
    /// keeps no connection to yet, but whose files the ledger knows, is connected as a pane
    /// that returns to it would be (the program that asks is not a pane's).
    pub(super) fn stream_wanted(&mut self, open: StreamOpen) {
        if !self.accounts.contains_key(&open.account) {
            self.recall(&open.account);
        }
        self.opening.push(open);
    }

    /// Asks the clients for the streams that wait, as far as their connections are up. `true`
    /// when something was asked.
    pub(super) fn start_streams(&mut self, now: Instant) -> bool {
        let mut moved = false;
        let mut waiting = Vec::new();
        for mut open in std::mem::take(&mut self.opening) {
            if open.far.abandoned() {
                continue;
            }
            let host = self
                .accounts
                .get(&open.account)
                .map(|entry| entry.server.host.clone());
            let (Some(host), Some(client)) = (host, self.hub.client_mut(&open.account)) else {
                open.far.refused(&format!(
                    "bateri keeps no connection to {} now",
                    open.account
                ));
                continue;
            };
            match client.link() {
                Link::Live => {}
                Link::Opening { .. } => {
                    waiting.push(open);
                    continue;
                }
                Link::Retrying { .. } => {
                    // A person asked: not after the pause of a connection that failed.
                    if open.person && !open.dialed {
                        open.dialed = true;
                        client.retry_now(now);
                    }
                    waiting.push(open);
                    continue;
                }
                Link::NeedsLogin => {
                    if open.dialed || !open.person {
                        open.far.refused(&format!(
                            "{host} wants a login that bateri cannot give: open a terminal on it and log in"
                        ));
                    } else {
                        open.dialed = true;
                        client.retry_now(now);
                        waiting.push(open);
                    }
                    continue;
                }
                Link::Over(_) => {
                    open.far
                        .refused(&format!("the connection to {host} could not be kept"));
                    continue;
                }
            }
            let asked = match &open.what {
                What::Command(command) => client.open_command(command.clone()),
                What::Connection { host, port } => client.open_connection(host, *port),
            };
            match asked {
                Ok(talk) => {
                    let _ = open.port.talk.set(talk);
                    self.pipes.insert(
                        (open.account, talk),
                        Piped {
                            far: open.far,
                            finished: false,
                        },
                    );
                    moved = true;
                }
                Err(text) => open.far.refused(&text),
            }
        }
        self.opening = waiting;
        moved
    }

    /// The server took the open of a stream.
    pub(super) fn stream_opened(&mut self, account: &str, talk: Talk) {
        if let Some(piped) = self.pipes.get_mut(&(account.to_owned(), talk)) {
            piped.far.opened();
        }
    }

    /// Bytes of a stream arrived. When nobody reads them any more they count as taken, so that the
    /// server is not held up by a reader that is gone.
    pub(super) fn stream_bytes(&mut self, account: &str, talk: Talk, data: Vec<u8>, error: bool) {
        let len = data.len();
        let Some(piped) = self.pipes.get_mut(&(account.to_owned(), talk)) else {
            return;
        };
        let read = if error {
            piped.far.err(data)
        } else {
            piped.far.out(data)
        };
        if !read && let Some(client) = self.hub.client_mut(account) {
            client.took(talk, len);
        }
    }

    /// The server has nothing more to send on a stream.
    pub(super) fn stream_eof(&mut self, account: &str, talk: Talk) {
        if let Some(piped) = self.pipes.get_mut(&(account.to_owned(), talk)) {
            piped.far.eof();
        }
    }

    /// The program the server runs for a stream ended.
    pub(super) fn stream_exited(&mut self, account: &str, talk: Talk, exit: kablo_proto::v1::Exit) {
        if let Some(piped) = self.pipes.get_mut(&(account.to_owned(), talk)) {
            piped.far.exited(exit);
        }
    }

    /// A stream is over.
    pub(super) fn stream_over(&mut self, account: &str, talk: Talk, over: &Over) {
        if let Some(mut piped) = self.pipes.remove(&(account.to_owned(), talk)) {
            piped.far.over(over);
        }
    }

    /// A caller's reads took `bytes`: the server may send that much more.
    pub(super) fn stream_took(&mut self, account: &str, talk: Talk, bytes: usize) {
        if let Some(client) = self.hub.client_mut(account) {
            client.took(talk, bytes);
        }
    }

    /// A caller is done with a stream, or ended it: it is closed.
    pub(super) fn stream_hung_up(&mut self, account: &str, talk: Talk) {
        if let Some(client) = self.hub.client_mut(account) {
            client.hang_up(talk);
        }
        if let Some(mut piped) = self.pipes.remove(&(account.to_owned(), talk)) {
            piped.far.over(&Over::Finished);
        }
    }

    /// Moves what callers wrote to the clients, as far as the servers' windows allow, and
    /// says what a caller that has nothing more to write has said. A caller that gave up
    /// waiting for an open that came late has the stream closed; a stream whose account is
    /// gone is over. `true` when something was handed to a client, which then has it to send.
    pub(super) fn pump_streams(&mut self) -> bool {
        let mut moved = false;
        let keys: Vec<(String, Talk)> = self.pipes.keys().cloned().collect();
        for key in keys {
            let (account, talk) = &key;
            if self.hub.client_mut(account).is_none() {
                if let Some(mut gone) = self.pipes.remove(&key) {
                    gone.far.over(&Over::Lost);
                }
                continue;
            }
            let (Some(piped), Some(client)) =
                (self.pipes.get_mut(&key), self.hub.client_mut(account))
            else {
                continue;
            };
            if piped.far.abandoned() {
                client.hang_up(*talk);
                self.pipes.remove(&key);
                continue;
            }
            while client
                .unsent(*talk)
                .is_some_and(|waiting| waiting < UNSENT_LIMIT)
            {
                match piped.far.next_input() {
                    Ok(Some(chunk)) => {
                        client.say(*talk, &chunk);
                        moved = true;
                    }
                    Ok(None) => break,
                    Err(()) => {
                        if !piped.finished {
                            piped.finished = true;
                            client.finish(*talk);
                            moved = true;
                        }
                        break;
                    }
                }
            }
        }
        moved
    }
}

#[cfg(test)]
pub(crate) mod fake {
    //! A connection that is up to a server that is a directory here: for the tests of what asks
    //! [`Streams`] for a program, with a real process at the far end and no service behind it.

    use std::io::Write as _;
    use std::os::unix::process::CommandExt as _;
    use std::path::Path;
    use std::process::{Command as Process, Stdio};
    use std::sync::Mutex;
    use std::thread;

    use kablo_proto::v1::Exit;

    use super::*;

    /// The words of every program asked for, in the order they were.
    pub(crate) type Ran = Arc<Mutex<Vec<Vec<String>>>>;

    impl Streams {
        /// What asks for programs on a server that is the directory `home`, over a connection
        /// that is up for every account: each program is run here with the words it was asked
        /// with, `home` for its home, and its words are written down in `ran`. A connection is
        /// refused: a directory makes none.
        pub(crate) fn at_home(home: &Path, ran: Ran) -> Streams {
            let (tx, asked) = mpsc::channel();
            let home = home.to_owned();
            thread::spawn(move || {
                while let Ok(msg) = asked.recv() {
                    match msg {
                        Msg::StreamLive { reply, .. } => {
                            let _ = reply.send(true);
                        }
                        Msg::StreamOpen(open) => run(open, &home, &ran),
                        _ => {}
                    }
                }
            });
            Streams { tx, person: false }
        }
    }

    fn run(open: StreamOpen, home: &Path, ran: &Ran) {
        let StreamOpen { what, mut far, .. } = open;
        let What::Command(command) = what else {
            far.refused("a directory makes no connections");
            return;
        };
        let words: Vec<String> = command
            .argv
            .iter()
            .map(|word| String::from_utf8_lossy(word).into_owned())
            .collect();
        ran.lock().expect("lock").push(words.clone());
        let Some((program, args)) = words.split_first() else {
            far.refused("no program");
            return;
        };
        let child = Process::new(program)
            .args(args)
            .current_dir(home)
            .env("HOME", home)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0)
            .spawn();
        let mut child = match child {
            Ok(child) => child,
            Err(error) => {
                far.refused(&error.to_string());
                return;
            }
        };
        thread::spawn(move || {
            far.opened();
            // What the caller writes, until it has nothing more to write.
            let mut stdin = child.stdin.take();
            loop {
                match far.next_input() {
                    Ok(Some(chunk)) => {
                        if let Some(stdin) = stdin.as_mut() {
                            let _ = stdin.write_all(&chunk);
                        }
                    }
                    Ok(None) => thread::sleep(Duration::from_millis(1)),
                    Err(()) => break,
                }
            }
            drop(stdin);
            match child.wait_with_output() {
                Ok(output) => {
                    far.out(output.stdout);
                    far.err(output.stderr);
                    far.eof();
                    far.exited(Exit::Code(output.status.code().unwrap_or(1)));
                }
                Err(_) => far.over(&Over::Lost),
            }
        });
    }
}
