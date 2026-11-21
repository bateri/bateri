//! The way from this Mac to the program's machine (plan 034 Phase 6, Karar 2): what the proxy
//! asks to have a connection made, and what shows that a way ends where it is thought to.
//!
//! The proxy knows [`Dialer`] and nothing of kablo or of this Mac's sockets: a way is whatever
//! makes a connection "as seen from the program's machine" — this Mac's own TCP ([`Direct`]) for a
//! program that runs here, the connection kept to a server ([`Through`]) for one that runs there.
//!
//! **A way is used only after it has shown where it ends** ([`prove`]): the program listens on its
//! machine's loopback and gives a token to one connection; the host connects by the way it is
//! trying and must read the token back. A pane where the program runs behind a nested `ssh`, or on
//! a machine that shares a home folder with another, has a way that fails this and so has none:
//! its pages would otherwise be fetched from the wrong machine's `localhost`.

use std::any::Any;
use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpStream, ToSocketAddrs};
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use crate::kablo::remote::RemoteStream;
use crate::kablo::served::Served;
use crate::ssh_route::Ask;

/// How long a connection may take to be made, for a request of a page (a design constant: the
/// first one through a server may have to bring the connection to it up).
pub const DIAL_WAIT: Duration = Duration::from_secs(15);

/// How long the proof of a way may take, from the program's answer to the token.
pub const PROOF_WAIT: Duration = Duration::from_secs(8);

/// Something that makes connections from the program's machine.
pub trait Dialer: Send + Sync {
    /// A connection to `host:port` as the program's machine sees them, made within `wait`. The
    /// `Err` is a line for the person: why the way does not make it.
    fn dial(&self, host: &str, port: u16, wait: Duration) -> Result<Link, String>;
}

/// A connection a [`Dialer`] made: its two ends, and the way to close it.
pub struct Link {
    /// What the far end says.
    pub reader: Box<dyn Read + Send>,
    /// What is said to the far end. Dropping it says there is no more (the far end hears `eof`).
    pub writer: Box<dyn Write + Send>,
    end: Box<dyn Fn() + Send + Sync>,
    /// What the way keeps the connection by (its place in a line): dropped with the link.
    _hold: Option<Box<dyn Any + Send>>,
}

impl Link {
    pub fn new(
        reader: impl Read + Send + 'static,
        writer: impl Write + Send + 'static,
        end: impl Fn() + Send + Sync + 'static,
        hold: Option<Box<dyn Any + Send>>,
    ) -> Self {
        Self {
            reader: Box::new(reader),
            writer: Box::new(writer),
            end: Box::new(end),
            _hold: hold,
        }
    }

    /// Closes the connection, both ways, at once.
    pub fn end(&self) {
        (self.end)();
    }

    /// Takes the link apart: its ends, the way to close it (from any thread) and what it is held by.
    pub fn split(self) -> Parts {
        Parts {
            reader: self.reader,
            writer: self.writer,
            end: Arc::from(self.end),
            hold: self._hold,
        }
    }
}

/// What a [`Link`] is made of, each part on its own so that the two directions of a connection can
/// be carried by two threads.
pub struct Parts {
    pub reader: Box<dyn Read + Send>,
    pub writer: Box<dyn Write + Send>,
    /// Closes the connection, both ways, at once.
    pub end: Arc<dyn Fn() + Send + Sync>,
    /// What keeps the connection's place in a line: dropped when the connection is over.
    pub hold: Option<Box<dyn Any + Send>>,
}

/// This Mac's own TCP: the way for a program that runs here.
pub struct Direct;

impl Dialer for Direct {
    fn dial(&self, host: &str, port: u16, wait: Duration) -> Result<Link, String> {
        let addresses = (host, port)
            .to_socket_addrs()
            .map_err(|error| format!("{host}: {error}"))?;
        let mut last = format!("{host}:{port}: no address");
        for address in addresses {
            match TcpStream::connect_timeout(&address, wait) {
                Ok(stream) => return tcp_link(stream).map_err(|error| error.to_string()),
                Err(error) => last = format!("{host}:{port}: {error}"),
            }
        }
        Err(last)
    }
}

fn tcp_link(stream: TcpStream) -> io::Result<Link> {
    stream.set_nodelay(true)?;
    let reader = stream.try_clone()?;
    let closing = stream.try_clone()?;
    Ok(Link::new(
        reader,
        stream,
        move || {
            let _ = closing.shutdown(Shutdown::Both);
        },
        None,
    ))
}

/// The connection kept to a server: the way for a program that runs there. Its connections wait
/// in the server's line ([`crate::kablo::queue`]) with the port forwards'.
pub struct Through(pub Served);

impl Dialer for Through {
    fn dial(&self, host: &str, port: u16, wait: Duration) -> Result<Link, String> {
        let RemoteStream {
            reader,
            writer,
            end,
            permit,
        } = self.0.connection(host, port, Ask::Never, wait)?;
        let writer = writer.ok_or_else(|| "the server made no connection".to_owned())?;
        Ok(Link::new(
            reader,
            writer,
            move || end.kill(),
            permit.map(|permit| Box::new(permit) as Box<dyn Any + Send>),
        ))
    }
}

/// The ways that have been proved, by machine: what the proxy looks a profile's connections up
/// in. A machine with no way here has no connections.
#[derive(Default)]
pub struct Routes {
    ways: Mutex<HashMap<String, Arc<dyn Dialer>>>,
}

impl Routes {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// `machine` is reached by `way` from now on (another proof of it replaces the first).
    pub fn set(&self, machine: &str, way: Arc<dyn Dialer>) {
        self.ways
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(machine.to_owned(), way);
    }

    pub fn get(&self, machine: &str) -> Option<Arc<dyn Dialer>> {
        self.ways
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(machine)
            .cloned()
    }

    pub fn remove(&self, machine: &str) {
        self.ways
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(machine);
    }
}

/// What the program's listener gives back: the 32 hex digits of the token.
const TOKEN_LEN: usize = 32;

/// Shows that `way` ends at the loopback of the program's machine: connects to `port` of it and
/// reads back the token. `Ok` is the proof. The `Err` is a line for the person, and is the
/// proxy's and the room's word for it: there is no way.
pub fn prove(way: &dyn Dialer, port: u16, token: &str, wait: Duration) -> Result<(), String> {
    if token.len() != TOKEN_LEN {
        return Err(no_way("the program gave a token that is not one"));
    }
    let mut link = way.dial("127.0.0.1", port, wait).map_err(|why| {
        no_way(&format!(
            "the connection to the program's machine is refused ({why})"
        ))
    })?;
    let (heard, listen) = channel();
    let mut reader = std::mem::replace(&mut link.reader, Box::new(io::empty()));
    std::thread::Builder::new()
        .name("browser-route-proof".into())
        .spawn(move || {
            let mut got = [0u8; TOKEN_LEN];
            let result = reader.read_exact(&mut got).map(|()| got);
            let _ = heard.send(result);
        })
        .map_err(|error| error.to_string())?;
    let result = listen.recv_timeout(wait);
    link.end();
    match result {
        Ok(Ok(got)) if got == token.as_bytes() => Ok(()),
        Ok(Ok(_)) => Err(no_way("what was there is not what the program gave")),
        Ok(Err(_)) => Err(no_way("what was there said too little")),
        Err(_) => Err(no_way("nothing was said in time")),
    }
}

/// The line for a way that was not shown to end at the program: the way and why.
pub fn no_way(why: &str) -> String {
    format!("There is no route from this pane to the program's machine: {why}.")
}

#[cfg(test)]
mod tests;
