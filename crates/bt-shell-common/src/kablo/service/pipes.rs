//! The programs and connections the service runs for callers over an account's connection:
//! [`Streams`] is what a caller (or a [`Backend`](super::Backend)) asks with, and the rest is
//! the service's side of it — the opens that wait for the connection, and the pipes between a
//! stream of the client and the handles a caller holds ([`super::super::remote`]).
//!
//! The caller blocks, the service thread never does: what the server sends goes into a channel
//! the caller's reads take from (and say they have taken, which credits the server), and what the
//! caller writes waits in a bounded channel the service takes from as far as the server's window
//! allows, whenever it settles.

use std::sync::mpsc::Sender;
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
}

impl Streams {
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
                dialed: false,
            }))
            .map_err(|_| "bateri is shutting down".to_owned())?;
        handles.opened(wait)
    }
}

impl Core {
    /// A caller wants a stream: it waits for the account's connection.
    pub(super) fn stream_wanted(&mut self, open: StreamOpen) {
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
                open.far
                    .refused("bateri keeps no connection to this server now");
                continue;
            };
            match client.link() {
                Link::Live => {}
                Link::Opening { .. } => {
                    waiting.push(open);
                    continue;
                }
                Link::Retrying { .. } => {
                    // The user asked: not after the pause of a connection that failed.
                    if !open.dialed {
                        open.dialed = true;
                        client.retry_now(now);
                    }
                    waiting.push(open);
                    continue;
                }
                Link::NeedsLogin => {
                    if open.dialed {
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
            piped.far.over(&Over::Closed);
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
