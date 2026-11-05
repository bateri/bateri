//! A server's listening port, opened from this Mac: **directly** when the
//! Mac reaches it, otherwise **forwarded** through the connection kept to the server.
//!
//! - **Reach** ([`reachable`]): the server's `/proc` says where a port is
//!   bound, not whether a firewall or a cloud security group lets this Mac
//!   in — only a connection from here says that. One TCP connect with a short
//!   timeout ([`REACH_TIMEOUT`]) when the port first shows; nothing is sent.
//!   A port bound to the server's loopback is never reachable and is not
//!   tried; behind a jump host nothing is tried
//!   (`ssh_route::direct_hostname`).
//! - **Forward** ([`forward`]): a listener on this Mac's loopback, and for every connection
//!   made to it a connection the server makes to the port ([`Served::connection`]) with the
//!   bytes copied both ways. The first connection is made when the forward begins, so that a
//!   server that does not let connections be made through it (an administrator's
//!   `/etc/kablo/no-forwarding`) says so then and not at the first page load. Dropping the
//!   [`Forwarding`] closes the listener and every connection it carries. The local port is the
//!   server's number when it is free here ([`free_local_port`]).

use std::collections::HashMap;
use std::io;
use std::net::{IpAddr, Shutdown, TcpListener, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::Duration;

use crate::kablo::remote::{End, RemoteStream};
use crate::kablo::served::Served;
use crate::ssh_route::Ask;

/// How long a reach test waits for the server — a **design constant**: long
/// enough for a far server's handshake, short enough that a blocked port
/// does not keep the test thread long.
pub const REACH_TIMEOUT: Duration = Duration::from_millis(1500);

/// How long the server may take to make a connection for a forward — a **design
/// constant**: the first one may have to bring the connection to the server up.
pub const FORWARD_WAIT: Duration = Duration::from_secs(8);

/// Whether this Mac reaches `host:port` — a TCP connect to any of the name's
/// addresses within `timeout` each; closed at once, nothing is sent.
pub fn reachable(host: &str, port: u16, timeout: Duration) -> bool {
    let Ok(addresses) = (host, port).to_socket_addrs() else {
        return false;
    };
    addresses
        .into_iter()
        .any(|address| TcpStream::connect_timeout(&address, timeout).is_ok())
}

/// `http://{host}:{port}`, an IPv6 literal in brackets.
pub fn url(host: &str, port: u16) -> String {
    if host.contains(':') && !host.starts_with('[') {
        format!("http://[{host}]:{port}")
    } else {
        format!("http://{host}:{port}")
    }
}

/// A local port to forward to: `preferred` (the server's number) when it is
/// free on this Mac's loopback, otherwise one the system picks. `None` if
/// the loopback cannot be bound at all.
pub fn free_local_port(preferred: u16) -> Option<u16> {
    if preferred != 0 && TcpListener::bind(("127.0.0.1", preferred)).is_ok() {
        return Some(preferred);
    }
    let listener = TcpListener::bind(("127.0.0.1", 0)).ok()?;
    Some(listener.local_addr().ok()?.port())
}

/// The address the server connects to for the one it lists a port on: an unspecified binding
/// (`0.0.0.0`, `::`) is reached by the server's own loopback.
pub fn connect_address(address: IpAddr) -> String {
    match address {
        IpAddr::V4(v4) if v4.is_unspecified() => "127.0.0.1".to_owned(),
        IpAddr::V6(v6) if v6.is_unspecified() => "127.0.0.1".to_owned(),
        address => address.to_string(),
    }
}

/// A forward that runs. Dropping it ends it: the listener closes and so does every connection
/// that went through it.
pub struct Forwarding {
    local: u16,
    shared: Arc<Shared>,
}

impl std::fmt::Debug for Forwarding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Forwarding")
            .field("local", &self.local)
            .finish_non_exhaustive()
    }
}

/// What the forward's threads and its owner share.
struct Shared {
    stopped: AtomicBool,
    next: AtomicU64,
    /// The connections that are carried now, by number: what ends them when the forward ends.
    open: Mutex<HashMap<u64, Carried>>,
}

struct Carried {
    socket: TcpStream,
    end: Arc<End>,
}

impl Shared {
    fn open(&self) -> std::sync::MutexGuard<'_, HashMap<u64, Carried>> {
        self.open.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Forwarding {
    /// The port on this Mac that the forward listens on.
    pub fn local(&self) -> u16 {
        self.local
    }
}

impl Drop for Forwarding {
    fn drop(&mut self) {
        self.shared.stopped.store(true, Ordering::SeqCst);
        // The accept that waits sees a connection and then the flag.
        let _ = TcpStream::connect_timeout(
            &std::net::SocketAddr::from(([127, 0, 0, 1], self.local)),
            Duration::from_millis(500),
        );
        let carried: Vec<Carried> = self.shared.open().drain().map(|(_, one)| one).collect();
        for one in carried {
            one.end.kill();
            let _ = one.socket.shutdown(Shutdown::Both);
        }
    }
}

/// Forwards this Mac's `local` to the server's `address:remote` over the connection `served`
/// keeps. Blocking while the first connection is made; for a thread of its own. `Err` is the
/// line for the person: why the server makes no connection, or why the port here is not free.
pub fn forward(
    served: &Served,
    local: u16,
    address: IpAddr,
    remote: u16,
) -> Result<Forwarding, String> {
    let host = connect_address(address);
    // The server's answer comes first: a connection it will not make is told while the person
    // is looking at the port that was clicked.
    drop(served.connection(&host, remote, Ask::Never, FORWARD_WAIT)?);
    let listener = TcpListener::bind(("127.0.0.1", local)).map_err(|error| {
        if error.kind() == io::ErrorKind::AddrInUse {
            format!("bind [127.0.0.1]:{local}: Address already in use")
        } else {
            error.to_string()
        }
    })?;
    let shared = Arc::new(Shared {
        stopped: AtomicBool::new(false),
        next: AtomicU64::new(0),
        open: Mutex::new(HashMap::new()),
    });
    let accepting = Arc::clone(&shared);
    let served = served.clone();
    thread::Builder::new()
        .name("port forward".into())
        .spawn(move || {
            for socket in listener.incoming() {
                if accepting.stopped.load(Ordering::SeqCst) {
                    break;
                }
                let Ok(socket) = socket else {
                    continue;
                };
                let (served, shared, host) = (served.clone(), Arc::clone(&accepting), host.clone());
                let _ = thread::Builder::new()
                    .name("port forward connection".into())
                    .spawn(move || carry(socket, &served, &host, remote, &shared));
            }
        })
        .map_err(|error| error.to_string())?;
    Ok(Forwarding { local, shared })
}

/// One connection made to the forward's port: the server's connection to the port, and the bytes
/// of both copied until each says it has no more.
fn carry(socket: TcpStream, served: &Served, host: &str, port: u16, shared: &Shared) {
    // A connection the server does not make closes this one, which is all a client of a port
    // is told of it.
    let Ok(RemoteStream {
        mut reader,
        writer: Some(mut writer),
        end,
    }) = served.connection(host, port, Ask::Never, FORWARD_WAIT)
    else {
        return;
    };
    let Ok(mut from_client) = socket.try_clone() else {
        return;
    };
    let Ok(mut to_client) = socket.try_clone() else {
        return;
    };
    let id = shared.next.fetch_add(1, Ordering::SeqCst);
    shared.open().insert(
        id,
        Carried {
            socket,
            end: Arc::clone(&end),
        },
    );
    // Ended while this was being set up: the owner's sweep may have gone by already.
    if shared.stopped.load(Ordering::SeqCst) {
        if let Some(one) = shared.open().remove(&id) {
            one.end.kill();
            let _ = one.socket.shutdown(Shutdown::Both);
        }
        return;
    }
    let up = thread::Builder::new()
        .name("port forward up".into())
        .spawn(move || {
            let _ = io::copy(&mut from_client, &mut writer);
            // The client has no more to say: the server's end hears it end (`writer` drops).
        });
    // The server's end ended: cleanly, the client may still have something to say and is waited
    // for; with an error the answer is cut, and the client is let go of at once.
    let said_all = io::copy(&mut reader, &mut to_client).is_ok();
    let _ = to_client.shutdown(if said_all {
        Shutdown::Write
    } else {
        Shutdown::Both
    });
    if let Ok(up) = up {
        let _ = up.join();
    }
    end.kill();
    shared.open().remove(&id);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unspecified_binding_is_reached_by_the_servers_loopback() {
        let v4 = |a, b, c, d| IpAddr::from([a, b, c, d]);
        assert_eq!(connect_address(v4(0, 0, 0, 0)), "127.0.0.1");
        assert_eq!(connect_address(v4(127, 0, 0, 1)), "127.0.0.1");
        assert_eq!(connect_address(v4(10, 0, 0, 5)), "10.0.0.5");
        assert_eq!(
            connect_address(IpAddr::from(std::net::Ipv6Addr::LOCALHOST)),
            "::1"
        );
        assert_eq!(
            connect_address(IpAddr::from(std::net::Ipv6Addr::UNSPECIFIED)),
            "127.0.0.1"
        );
    }

    #[test]
    fn a_server_that_makes_no_connection_says_why_and_leaves_no_listener() {
        let loopback = IpAddr::from([127, 0, 0, 1]);
        for (served, why) in [
            (Served::none(), "bateri's connections are not running"),
            (
                Served::refusing("turned off by the server's administrator"),
                "turned off by the server's administrator",
            ),
        ] {
            let local = free_local_port(0).expect("a free port");
            let error = forward(&served, local, loopback, 8080).expect_err("no connection");
            assert_eq!(error, why);
            assert_eq!(free_local_port(local), Some(local), "nothing listens");
        }
    }

    #[test]
    fn an_ipv6_host_is_bracketed_in_its_address() {
        assert_eq!(url("db1.example.com", 8080), "http://db1.example.com:8080");
        assert_eq!(url("2001:db8::5", 8080), "http://[2001:db8::5]:8080");
    }

    #[test]
    fn a_listening_port_is_reachable_and_a_free_one_is_not() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind");
        let port = listener.local_addr().expect("addr").port();
        assert!(reachable("127.0.0.1", port, REACH_TIMEOUT));
        let free = free_local_port(0).expect("a free port");
        assert!(!reachable("127.0.0.1", free, REACH_TIMEOUT));
        // A taken port is not offered; a free preferred one is.
        assert_ne!(free_local_port(port), Some(port));
        drop(listener);
        assert_eq!(free_local_port(port), Some(port));
    }
}
