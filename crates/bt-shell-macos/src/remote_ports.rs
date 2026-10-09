//! The server's listening ports in a remote pane — the dock's `↗` beside the
//! local ones, and the ports menu's "On {host}" half.
//!
//! - **Which**: the ports the processes under **our remote shell** listen on
//!   (`Session::remote_shell`, its pid on the server — a wrapped ssh's
//!   integration; a plain ssh gives none and the server shows no ports). The
//!   pane's helper session answers a `bt_ports` request
//!   (`remote_files::parse_ports`): a `/proc` walk on the server.
//! - **When**: on the local port probe's edges (output, a command's start or
//!   end — `pane::PortProbe`) and on the remote edge (the login, the session's
//!   end: `TerminalPane::sync_stats_generation`), at most one request in
//!   flight and one per [`INTERVAL`] — the walk reads every `/proc/*/stat` on
//!   the server. An idle remote pane sends nothing.
//! - **Reach**: a port bound to the server's loopback needs a forward; one
//!   bound wider is tried once from this Mac (`port_forward::reachable`, a TCP
//!   connect to `ssh -G`'s `hostname` — never behind a jump host, whose inner
//!   address may be another machine here). Reachable ones draw green and open
//!   directly; the rest draw dim.
//! - **Forward**: opening a dim port forwards it first
//!   (`port_forward::forward`: the master's `-O forward`, or a tunnel of its
//!   own) to the same number here when it is free, then opens
//!   `http://localhost:{port}`; the port turns green. Every forward ends with
//!   the remote session and with the pane ([`RemotePorts::reset`]): a tunnel
//!   of ours is killed, a master's forward cancelled (`ssh -O cancel`) — a
//!   master outlives its session for a while (a short persist, the helper
//!   riding it), and the server's port must not stay open here after `exit`.

use std::collections::HashMap;
use std::process::Child;
use std::time::{Duration, Instant};

use bt_core::FooterPort;
use dispatch2::{DispatchQueue, DispatchTime};
use objc2::MainThreadMarker;
use objc2::runtime::AnyObject;
use objc2_app_kit::NSPasteboard;

use crate::clipboard;
use crate::pane::TerminalPane;
use crate::port_forward::{self, Forwarded, REACH_TIMEOUT};
use crate::ports::{self, Bound};
use crate::remote_files::RemoteListener;
use crate::remote_helper::{Answer, Query, Request};
use crate::ssh_route::{self, SystemSsh};

/// The shortest time between two scans of the server's ports — a **design
/// constant**: output streams in a remote session (every echoed key), and a
/// scan walks the server's `/proc`; a server's port is still there within a
/// couple of seconds of the line that announces it.
const INTERVAL: Duration = Duration::from_secs(2);

/// How long a forward's failure stays in the pane's label.
const NOTE_FOR: Duration = Duration::from_secs(5);

/// Whether this Mac reaches a server's port.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Reach {
    /// A connect is under way.
    Checking,
    /// It answered: the port opens directly.
    Direct,
    /// Bound to the server's loopback, blocked, or behind a jump host.
    Closed,
}

/// The server's name as this Mac reaches it ([`ssh_route::direct_hostname`]).
#[derive(Debug, Default)]
enum Host {
    #[default]
    Unasked,
    Asking,
    /// `None`: unreadable, or behind a jump host — nothing opens directly.
    Known(Option<String>),
}

/// A forward of a server's port.
#[derive(Debug)]
enum Forward {
    Starting,
    Running { local: u16, how: Kept },
}

/// What keeps a running forward — and so how it ends.
#[derive(Debug)]
enum Kept {
    /// A tunnel of ours: killed.
    Tunnel(Child),
    /// A master: its forward is cancelled over the same argv, to the same
    /// server address.
    Master {
        ssh: Vec<String>,
        address: std::net::IpAddr,
    },
}

/// Ends a forward of the server's `port`: a tunnel at once, a master's
/// cancel on a thread of its own (it is an ssh call).
fn end_forward(port: u16, local: u16, how: Kept) {
    match how {
        Kept::Tunnel(mut child) => {
            let _ = child.kill();
            let _ = child.wait();
        }
        Kept::Master { ssh, address } => {
            let _ = std::thread::Builder::new()
                .name("remote port cancel".into())
                .spawn(move || port_forward::cancel(&ssh, local, address, port));
        }
    }
}

impl Forward {
    fn end(self, port: u16) {
        if let Forward::Running { local, how } = self {
            end_forward(port, local, how);
        }
    }
}

/// A forwarded result's [`Kept`]: the master's argv and address kept for
/// its cancel.
fn kept(forwarded: Forwarded, ssh: Vec<String>, address: std::net::IpAddr) -> Kept {
    match forwarded {
        Forwarded::Master => Kept::Master { ssh, address },
        Forwarded::Tunnel(child) => Kept::Tunnel(child),
    }
}

/// The pane's remote ports' state.
#[derive(Debug, Default)]
pub(crate) struct RemotePorts {
    /// The remote session's generation these belong to; `None` locally.
    generation: Option<u64>,
    /// The last scan.
    listeners: Vec<RemoteListener>,
    /// The helper's argv the last scan came over — where a forward goes.
    ssh: Vec<String>,
    host: Host,
    reach: HashMap<u16, Reach>,
    forwards: HashMap<u16, Forward>,
    in_flight: bool,
    last: Option<Instant>,
    /// An edge arrived while a scan was in flight or too soon after one.
    again: bool,
    /// A delayed scan is scheduled.
    timer: bool,
}

impl RemotePorts {
    /// A new generation (`None`: the remote session ended, the pane closes,
    /// the setting went off): everything goes, our tunnels end.
    fn reset(&mut self, generation: Option<u64>) {
        for (port, forward) in self.forwards.drain() {
            forward.end(port);
        }
        *self = Self {
            generation,
            ..Self::default()
        };
    }

    /// How a port opens from here; `None` while it has to be forwarded.
    fn opens(&self, port: u16) -> Option<String> {
        if let Some(Forward::Running { local, .. }) = self.forwards.get(&port) {
            return Some(ports::url(*local));
        }
        match (&self.host, self.reach.get(&port)) {
            (Host::Known(Some(host)), Some(Reach::Direct)) => Some(port_forward::url(host, port)),
            _ => None,
        }
    }

    /// The local ports our forwards listen on: a master that is the tab's
    /// own ssh lists them as the tab's — they are this list's, not twice.
    pub(crate) fn forwarded_locals(&self) -> Vec<u16> {
        self.forwards
            .values()
            .filter_map(|forward| match forward {
                Forward::Running { local, .. } => Some(*local),
                Forward::Starting => None,
            })
            .collect()
    }

    /// The dock's view of the server's ports.
    pub(crate) fn footer_ports(&self) -> Vec<FooterPort> {
        self.listeners
            .iter()
            .map(|listener| FooterPort {
                port: listener.port,
                open: self.opens(listener.port).is_some(),
            })
            .collect()
    }

    /// The ports menu's view of the server's ports.
    pub(crate) fn menu_items(&self) -> Vec<RemoteItem> {
        let mut items: Vec<RemoteItem> = Vec::new();
        for listener in &self.listeners {
            if items.iter().any(|item| item.port == listener.port) {
                continue;
            }
            let state = match self.forwards.get(&listener.port) {
                Some(Forward::Running { local, .. }) => RemoteState::Forwarded(*local),
                Some(Forward::Starting) => RemoteState::Forwarding,
                None => match self.reach.get(&listener.port) {
                    Some(Reach::Direct) => RemoteState::Direct,
                    Some(Reach::Checking) | None if listener.bound != Bound::Loopback => {
                        RemoteState::Checking
                    }
                    _ if listener.bound == Bound::Loopback => RemoteState::ServerOnly,
                    _ => RemoteState::Blocked,
                },
            };
            items.push(RemoteItem {
                port: listener.port,
                name: listener.name.clone(),
                url: self.opens(listener.port),
                state,
            });
        }
        items
    }
}

/// A server's port as the ports menu lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RemoteItem {
    pub(crate) port: u16,
    pub(crate) name: String,
    /// What opens it now; `None` while it has to be forwarded.
    pub(crate) url: Option<String>,
    pub(crate) state: RemoteState,
}

/// How a server's port opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RemoteState {
    /// This Mac reaches it.
    Direct,
    /// Forwarded to this local port.
    Forwarded(u16),
    /// A forward is starting.
    Forwarding,
    /// The reach test is under way.
    Checking,
    /// Bound to the server's loopback.
    ServerOnly,
    /// Bound wider, but this Mac does not reach it (or a jump host stands between).
    Blocked,
}

/// `RemoteItem`'s menu title: `db1:8080 — node · opens directly`.
pub(crate) fn remote_title(host: &str, item: &RemoteItem) -> String {
    let name = &item.name;
    let port = item.port;
    match item.state {
        RemoteState::Direct => format!("{host}:{port} \u{2014} {name} \u{b7} opens directly"),
        RemoteState::Forwarded(local) => {
            format!("localhost:{local} \u{2192} {host}:{port} \u{2014} {name}")
        }
        RemoteState::Forwarding => {
            format!("{host}:{port} \u{2014} {name} \u{b7} forwarding\u{2026}")
        }
        RemoteState::Checking => {
            format!("Forward {host}:{port} \u{2014} {name} \u{b7} checking\u{2026}")
        }
        RemoteState::ServerOnly => {
            format!("Forward {host}:{port} \u{2014} {name} \u{b7} server only")
        }
        RemoteState::Blocked => {
            format!("Forward {host}:{port} \u{2014} {name} \u{b7} not reachable from this Mac")
        }
    }
}

impl TerminalPane {
    /// Asks the helper for the server's ports when the pane is remote, its
    /// user logged in and our remote shell known — throttled
    /// ([`INTERVAL`], one in flight). Anything else ends the remote ports.
    pub(crate) fn request_remote_ports(&self) {
        let Some(session) = self.session() else {
            return;
        };
        let current = session
            .remote_target()
            .filter(|(command, ..)| crate::jobs::remote_login(session) == Some(*command))
            .and_then(|(command, target, _)| {
                let (generation, shell) = session.remote_shell()?;
                (generation == command).then_some((command, target, shell))
            })
            .filter(|_| self.port_probe().is_on());
        let Some((generation, target, shell)) = current else {
            self.end_remote_ports();
            return;
        };
        {
            let mut state = self.remote_ports().borrow_mut();
            if state.generation != Some(generation) {
                state.reset(Some(generation));
            }
            if state.in_flight {
                state.again = true;
                return;
            }
            if let Some(since) = state.last.map(|last| last.elapsed())
                && since < INTERVAL
            {
                if !state.timer {
                    state.timer = true;
                    drop(state);
                    self.request_remote_ports_after(INTERVAL - since);
                }
                return;
            }
            state.in_flight = true;
            state.last = Some(Instant::now());
        }
        let (id, lookup) = (self.id(), self.lookup());
        let reply = Box::new(move |answer: Result<Answer, String>, ssh: &[String]| {
            let ssh = ssh.to_vec();
            DispatchQueue::main().exec_async(move || {
                // audit: a block running on the main queue is on the main thread by definition.
                let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
                if let Some(pane) = lookup(mtm, id) {
                    pane.remote_ports_answered(generation, answer, ssh);
                }
            });
        });
        self.remote_helper().borrow_mut().ask(Request {
            command: generation,
            host: target.host.clone(),
            // A background job: rides a live master or today's argv, never asks.
            dial: self.dial(target, None),
            query: Query::Ports { shell },
            reply,
        });
    }

    /// The throttle's delayed request.
    fn request_remote_ports_after(&self, after: Duration) {
        let (id, lookup) = (self.id(), self.lookup());
        let run = move || {
            // audit: a block running on the main queue is on the main thread by definition.
            let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
            if let Some(pane) = lookup(mtm, id) {
                pane.remote_ports().borrow_mut().timer = false;
                pane.request_remote_ports();
            }
        };
        match DispatchTime::try_from(after) {
            Ok(when) => {
                let _ = DispatchQueue::main().after(when, run);
            }
            Err(_) => DispatchQueue::main().exec_async(run),
        }
    }

    /// A scan's answer for `generation`, on the main queue.
    fn remote_ports_answered(
        &self,
        generation: u64,
        answer: Result<Answer, String>,
        ssh: Vec<String>,
    ) {
        let again = {
            let mut state = self.remote_ports().borrow_mut();
            if state.generation != Some(generation) {
                return;
            }
            state.in_flight = false;
            match answer {
                Ok(Answer::Ports(Some(listeners))) => {
                    // A port that went takes its forward with it.
                    let gone: Vec<u16> = state
                        .forwards
                        .keys()
                        .copied()
                        .filter(|port| !listeners.iter().any(|l| l.port == *port))
                        .collect();
                    for port in gone {
                        if let Some(forward) = state.forwards.remove(&port) {
                            forward.end(port);
                        }
                    }
                    state.listeners = listeners;
                    if !ssh.is_empty() {
                        state.ssh = ssh;
                    }
                }
                _ => state.listeners.clear(),
            }
            std::mem::take(&mut state.again)
        };
        self.check_reach(generation);
        self.publish_ports();
        if again {
            self.request_remote_ports();
        }
    }

    /// Starts what the reach still needs: the server's direct name once per
    /// generation, then one connect per port not yet tried; a loopback-bound
    /// port, or any port without a direct name, is closed at once.
    fn check_reach(&self, generation: u64) {
        let (ask_host, tests) = {
            let mut state = self.remote_ports().borrow_mut();
            let state = &mut *state;
            match &state.host {
                Host::Unasked => {
                    state.host = Host::Asking;
                    (true, Vec::new())
                }
                Host::Asking => (false, Vec::new()),
                Host::Known(host) => {
                    let mut tests = Vec::new();
                    for listener in &state.listeners {
                        if state.reach.contains_key(&listener.port) {
                            continue;
                        }
                        match host {
                            Some(host) if listener.bound != Bound::Loopback => {
                                state.reach.insert(listener.port, Reach::Checking);
                                tests.push((host.clone(), listener.port));
                            }
                            _ => {
                                state.reach.insert(listener.port, Reach::Closed);
                            }
                        }
                    }
                    (false, tests)
                }
            }
        };
        let (id, lookup) = (self.id(), self.lookup());
        if ask_host {
            let target = self
                .session()
                .and_then(|session| session.remote_target())
                .map(|(_, target, _)| target);
            let spawned = target.and_then(|target| {
                std::thread::Builder::new()
                    .name("remote port host".into())
                    .spawn(move || {
                        let host = ssh_route::direct_hostname(&SystemSsh, &target);
                        DispatchQueue::main().exec_async(move || {
                            // audit: a block running on the main queue is on the main thread by definition.
                            let mtm =
                                MainThreadMarker::new().expect("the main queue is the main thread");
                            if let Some(pane) = lookup(mtm, id) {
                                let current = {
                                    let mut state = pane.remote_ports().borrow_mut();
                                    let current = state.generation == Some(generation);
                                    if current {
                                        state.host = Host::Known(host);
                                    }
                                    current
                                };
                                if current {
                                    pane.check_reach(generation);
                                    pane.publish_ports();
                                }
                            }
                        });
                    })
                    .ok()
            });
            if spawned.is_none() {
                self.remote_ports().borrow_mut().host = Host::Known(None);
                self.check_reach(generation);
            }
        }
        for (host, port) in tests {
            let spawned = std::thread::Builder::new()
                .name("remote port reach".into())
                .spawn(move || {
                    let reach = if port_forward::reachable(&host, port, REACH_TIMEOUT) {
                        Reach::Direct
                    } else {
                        Reach::Closed
                    };
                    DispatchQueue::main().exec_async(move || {
                        // audit: a block running on the main queue is on the main thread by definition.
                        let mtm =
                            MainThreadMarker::new().expect("the main queue is the main thread");
                        if let Some(pane) = lookup(mtm, id) {
                            let current = {
                                let mut state = pane.remote_ports().borrow_mut();
                                let current = state.generation == Some(generation);
                                if current {
                                    state.reach.insert(port, reach);
                                }
                                current
                            };
                            if current {
                                pane.publish_ports();
                            }
                        }
                    });
                });
            if spawned.is_err() {
                self.remote_ports()
                    .borrow_mut()
                    .reach
                    .insert(port, Reach::Closed);
            }
        }
    }

    /// The remote ports end: the session ended, the pane closes or the
    /// setting went off. Our tunnels end with them.
    pub(crate) fn end_remote_ports(&self) {
        let had = {
            let mut state = self.remote_ports().borrow_mut();
            let had = state.generation.is_some();
            state.reset(None);
            had
        };
        if had {
            self.publish_ports();
        }
    }

    /// Opens a server's port: directly or through its forward when it opens
    /// from here, otherwise forwards it first (the same number here when
    /// free) and opens that. A forward under way is not started twice.
    pub(crate) fn open_remote_port(&self, port: u16) {
        enum Next {
            Open(String),
            Forward(Vec<String>, std::net::IpAddr, u64),
            Nothing,
        }
        let next = {
            let mut state = self.remote_ports().borrow_mut();
            match (state.opens(port), state.generation) {
                (Some(url), _) => Next::Open(url),
                (None, Some(generation)) => {
                    let address = state
                        .listeners
                        .iter()
                        .find(|listener| listener.port == port)
                        .map(|listener| listener.address);
                    match (address, state.forwards.contains_key(&port)) {
                        (Some(address), false) if !state.ssh.is_empty() => {
                            state.forwards.insert(port, Forward::Starting);
                            Next::Forward(state.ssh.clone(), address, generation)
                        }
                        (Some(_), false) => {
                            drop(state);
                            self.note_for_a_while(&format!(
                                "Can't forward :{port} — no connection to the server yet"
                            ));
                            Next::Nothing
                        }
                        _ => Next::Nothing,
                    }
                }
                (None, None) => Next::Nothing,
            }
        };
        let (ssh, address, generation) = match next {
            Next::Open(url) => {
                crate::footer::open_url(&url);
                return;
            }
            Next::Forward(ssh, address, generation) => (ssh, address, generation),
            Next::Nothing => return,
        };
        self.publish_ports();
        let (id, lookup) = (self.id(), self.lookup());
        let spawned = std::thread::Builder::new()
            .name("remote port forward".into())
            .spawn(move || {
                let result = port_forward::free_local_port(port)
                    .ok_or_else(|| "No free local port".to_owned())
                    .and_then(|local| {
                        port_forward::forward(&ssh, local, address, port).map(|f| (local, f))
                    });
                let ssh = ssh.clone();
                DispatchQueue::main().exec_async(move || {
                    // audit: a block running on the main queue is on the main thread by definition.
                    let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
                    let result =
                        result.map(|(local, forwarded)| (local, kept(forwarded, ssh, address)));
                    let Some(pane) = lookup(mtm, id) else {
                        if let Ok((local, how)) = result {
                            end_forward(port, local, how);
                        }
                        return;
                    };
                    pane.forward_finished(generation, port, result);
                });
            });
        if spawned.is_err() {
            self.remote_ports().borrow_mut().forwards.remove(&port);
            self.publish_ports();
        }
    }

    /// A forward's end, on the main queue: running → the port turns green
    /// and opens; failed → the label says why.
    fn forward_finished(&self, generation: u64, port: u16, result: Result<(u16, Kept), String>) {
        let opened = {
            let mut state = self.remote_ports().borrow_mut();
            if state.generation != Some(generation) {
                if let Ok((local, how)) = result {
                    end_forward(port, local, how);
                }
                return;
            }
            match result {
                Ok((local, how)) => {
                    state.forwards.insert(port, Forward::Running { local, how });
                    Ok(local)
                }
                Err(text) => {
                    state.forwards.remove(&port);
                    Err(text)
                }
            }
        };
        self.publish_ports();
        match opened {
            Ok(local) => crate::footer::open_url(&ports::url(local)),
            Err(text) => self.note_for_a_while(&format!("Can't forward :{port} — {text}")),
        }
    }

    /// Shows `text` in the pane's bottom-left label for [`NOTE_FOR`]; a later
    /// note or link replaces it.
    fn note_for_a_while(&self, text: &str) {
        self.set_link_target(Some(text));
        let (id, lookup, shown) = (self.id(), self.lookup(), text.to_owned());
        if let Ok(when) = DispatchTime::try_from(NOTE_FOR) {
            let _ = DispatchQueue::main().after(when, move || {
                // audit: a block running on the main queue is on the main thread by definition.
                let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
                if let Some(pane) = lookup(mtm, id)
                    && pane.link_target_text().as_deref() == Some(shown.as_str())
                {
                    pane.set_link_target(None);
                }
            });
        }
    }

    /// An item's `openRemotePort:` (the ports menu): its `tag` is the port.
    pub(crate) fn open_remote_port_sent(&self, sender: Option<&AnyObject>) {
        if let Some(port) = crate::footer::sender_port(sender) {
            self.open_remote_port(port);
        }
    }

    /// The item's ⌥ alternate, `copyRemotePortURL:`: the address that opens it.
    pub(crate) fn copy_remote_port_url_sent(&self, sender: Option<&AnyObject>) {
        let Some(port) = crate::footer::sender_port(sender) else {
            return;
        };
        let url = self.remote_ports().borrow().opens(port);
        if url.is_some() {
            clipboard::copy(&NSPasteboard::generalPasteboard(), url);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listener(port: u16, bound: Bound) -> RemoteListener {
        RemoteListener {
            port,
            address: match bound {
                Bound::Loopback => std::net::IpAddr::from([127, 0, 0, 1]),
                _ => std::net::IpAddr::from([0, 0, 0, 0]),
            },
            bound,
            pid: 7,
            name: "node".into(),
        }
    }

    #[test]
    fn a_port_opens_directly_through_its_forward_or_not_yet() {
        let mut state = RemotePorts {
            generation: Some(1),
            listeners: vec![
                listener(5173, Bound::Loopback),
                listener(8080, Bound::Any),
                listener(9000, Bound::Any),
            ],
            host: Host::Known(Some("db1.example.com".into())),
            ..RemotePorts::default()
        };
        state.reach.insert(5173, Reach::Closed);
        state.reach.insert(8080, Reach::Direct);
        state.reach.insert(9000, Reach::Closed);
        assert_eq!(
            state.footer_ports(),
            [
                FooterPort {
                    port: 5173,
                    open: false
                },
                FooterPort {
                    port: 8080,
                    open: true
                },
                FooterPort {
                    port: 9000,
                    open: false
                },
            ]
        );
        assert_eq!(
            state.opens(8080).as_deref(),
            Some("http://db1.example.com:8080")
        );
        state.forwards.insert(
            5173,
            Forward::Running {
                local: 5174,
                how: Kept::Master {
                    ssh: Vec::new(),
                    address: std::net::IpAddr::from([127, 0, 0, 1]),
                },
            },
        );
        assert_eq!(state.opens(5173).as_deref(), Some("http://localhost:5174"));
        assert_eq!(state.forwarded_locals(), [5174]);
        let items = state.menu_items();
        assert_eq!(
            items.iter().map(|item| item.state).collect::<Vec<_>>(),
            [
                RemoteState::Forwarded(5174),
                RemoteState::Direct,
                RemoteState::Blocked
            ]
        );
        assert_eq!(
            remote_title("db1", &items[0]),
            "localhost:5174 \u{2192} db1:5173 \u{2014} node"
        );
        assert_eq!(
            remote_title("db1", &items[2]),
            "Forward db1:9000 \u{2014} node \u{b7} not reachable from this Mac"
        );
        // Behind a jump host nothing opens directly, whatever a connect said.
        state.host = Host::Known(None);
        assert_eq!(state.opens(8080), None);
        state.reset(None);
        assert!(state.listeners.is_empty() && state.forwards.is_empty());
    }

    #[test]
    fn an_untried_port_is_checking_unless_it_is_the_servers_own() {
        let state = RemotePorts {
            generation: Some(1),
            listeners: vec![listener(5173, Bound::Loopback), listener(8080, Bound::Any)],
            ..RemotePorts::default()
        };
        assert_eq!(
            state
                .menu_items()
                .iter()
                .map(|item| item.state)
                .collect::<Vec<_>>(),
            [RemoteState::ServerOnly, RemoteState::Checking]
        );
    }
}
