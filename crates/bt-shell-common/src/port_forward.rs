//! A server's listening port, opened from this Mac: **directly** when the
//! Mac reaches it, otherwise **forwarded** through the ssh connection.
//!
//! - **Reach** ([`reachable`]): the server's `/proc` says where a port is
//!   bound, not whether a firewall or a cloud security group lets this Mac
//!   in — only a connection from here says that. One TCP connect with a short
//!   timeout ([`REACH_TIMEOUT`]) when the port first shows; nothing is sent.
//!   A port bound to the server's loopback is never reachable and is not
//!   tried; behind a jump host nothing is tried
//!   (`ssh_route::direct_hostname`).
//! - **Forward** ([`forward`]): through the connection the helper session
//!   rides — `ssh -O forward` on its master (bateri's own, the user's
//!   terminal session's, or the user's own `ControlMaster`), which keeps the
//!   forward until it is cancelled ([`cancel`]) — the caller cancels it with
//!   the remote session, since a master outlives its session for a while (a
//!   short persist, our helper riding it); without a master a tunnel of its
//!   own (`ssh -N -L`, `ExitOnForwardFailure`), which the caller ends with the
//!   remote session. The local port is the server's number when it is free
//!   here ([`free_local_port`]).

use std::io::Read as _;
use std::net::{IpAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// How long a reach test waits for the server — a **design constant**: long
/// enough for a far server's handshake, short enough that a blocked port
/// does not keep the test thread long.
pub const REACH_TIMEOUT: Duration = Duration::from_millis(1500);

/// How long a forward may take to start listening here — a **design
/// constant**: a tunnel of its own is a full ssh handshake.
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

/// ssh's `-L` value: this Mac's `local` to the server's `address:remote`; an
/// unspecified address (`0.0.0.0`, `::`) as the server's own loopback, an
/// IPv6 one in brackets.
pub fn forward_spec(local: u16, address: IpAddr, remote: u16) -> String {
    let target = match address {
        IpAddr::V4(v4) if v4.is_unspecified() => "127.0.0.1".to_owned(),
        IpAddr::V6(v6) if v6.is_unspecified() => "127.0.0.1".to_owned(),
        IpAddr::V4(v4) => v4.to_string(),
        IpAddr::V6(v6) => format!("[{v6}]"),
    };
    format!("{local}:{target}:{remote}")
}

/// `ssh -O forward -L {spec} …` on the helper's argv (`ssh` = program,
/// options, destination): the master the argv names adds the forward.
pub fn control_argv(ssh: &[String], spec: &str) -> Option<Vec<String>> {
    let (program, rest) = ssh.split_first()?;
    let mut argv = vec![
        program.clone(),
        "-O".to_owned(),
        "forward".to_owned(),
        "-L".to_owned(),
        spec.to_owned(),
    ];
    argv.extend(rest.iter().cloned());
    Some(argv)
}

/// `ssh -O cancel -L {spec} …`: the master drops the forward.
pub fn cancel_argv(ssh: &[String], spec: &str) -> Option<Vec<String>> {
    let mut argv = control_argv(ssh, spec)?;
    argv[2] = "cancel".to_owned();
    Some(argv)
}

/// Drops a master's forward ([`Forwarded::Master`]) — a master already gone
/// has nothing to drop. Blocking (one `ssh -O cancel`); for a thread of its
/// own.
pub fn cancel(ssh: &[String], local: u16, address: IpAddr, remote: u16) {
    let Some(argv) = cancel_argv(ssh, &forward_spec(local, address, remote)) else {
        return;
    };
    let _ = Command::new(&argv[0])
        .args(&argv[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

/// A tunnel of its own on the helper's argv: `ssh -N -o
/// ExitOnForwardFailure=yes -L {spec} …` — it ends if the forward cannot be
/// set up rather than sit there without it.
pub fn tunnel_argv(ssh: &[String], spec: &str) -> Option<Vec<String>> {
    let (program, rest) = ssh.split_first()?;
    let mut argv = vec![
        program.clone(),
        "-N".to_owned(),
        "-o".to_owned(),
        "ExitOnForwardFailure=yes".to_owned(),
        "-L".to_owned(),
        spec.to_owned(),
    ];
    argv.extend(rest.iter().cloned());
    Some(argv)
}

/// A forward that runs.
pub enum Forwarded {
    /// The master keeps it until it is cancelled ([`cancel`]) or the master ends.
    Master,
    /// A tunnel of its own — the caller ends it with the remote session.
    Tunnel(Child),
}

/// Forwards this Mac's `local` to the server's `address:remote` over the
/// helper's argv `ssh` — the master's `-O forward` first, else a tunnel of
/// its own, waited for until it listens ([`FORWARD_WAIT`]). Blocking; for a
/// thread of its own. `Err` is ssh's last line.
pub fn forward(
    ssh: &[String],
    local: u16,
    address: IpAddr,
    remote: u16,
) -> Result<Forwarded, String> {
    let spec = forward_spec(local, address, remote);
    let control = control_argv(ssh, &spec).ok_or_else(|| "No ssh command".to_owned())?;
    let master = Command::new(&control[0])
        .args(&control[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    if master.is_ok_and(|status| status.success()) {
        return Ok(Forwarded::Master);
    }
    let tunnel = tunnel_argv(ssh, &spec).ok_or_else(|| "No ssh command".to_owned())?;
    let mut child = Command::new(&tunnel[0])
        .args(&tunnel[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("ssh could not be started: {error}"))?;
    let deadline = Instant::now() + FORWARD_WAIT;
    loop {
        if let Ok(Some(_)) = child.try_wait() {
            let mut stderr = String::new();
            if let Some(mut pipe) = child.stderr.take() {
                let _ = pipe.read_to_string(&mut stderr);
            }
            let line = stderr.lines().rev().find(|line| !line.trim().is_empty());
            return Err(line.unwrap_or("ssh ended").trim().to_owned());
        }
        if TcpStream::connect_timeout(
            &std::net::SocketAddr::from(([127, 0, 0, 1], local)),
            Duration::from_millis(200),
        )
        .is_ok()
        {
            return Ok(Forwarded::Tunnel(child));
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err("The forward did not start in time".to_owned());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_spec_points_an_unspecified_binding_at_the_servers_loopback() {
        let v4 = |a, b, c, d| IpAddr::from([a, b, c, d]);
        assert_eq!(
            forward_spec(5173, v4(0, 0, 0, 0), 5173),
            "5173:127.0.0.1:5173"
        );
        assert_eq!(
            forward_spec(5174, v4(127, 0, 0, 1), 5173),
            "5174:127.0.0.1:5173"
        );
        assert_eq!(forward_spec(80, v4(10, 0, 0, 5), 8080), "80:10.0.0.5:8080");
        assert_eq!(
            forward_spec(8080, IpAddr::from(std::net::Ipv6Addr::LOCALHOST), 8080),
            "8080:[::1]:8080"
        );
        assert_eq!(
            forward_spec(8080, IpAddr::from(std::net::Ipv6Addr::UNSPECIFIED), 8080),
            "8080:127.0.0.1:8080"
        );
    }

    #[test]
    fn the_forward_argvs_keep_the_helpers_options_and_destination() {
        let ssh: Vec<String> = ["ssh", "-T", "-o", "ControlPath=/s", "prod"]
            .map(str::to_owned)
            .to_vec();
        assert_eq!(
            control_argv(&ssh, "1:127.0.0.1:1").expect("argv"),
            [
                "ssh",
                "-O",
                "forward",
                "-L",
                "1:127.0.0.1:1",
                "-T",
                "-o",
                "ControlPath=/s",
                "prod"
            ]
        );
        assert_eq!(
            tunnel_argv(&ssh, "1:127.0.0.1:1").expect("argv"),
            [
                "ssh",
                "-N",
                "-o",
                "ExitOnForwardFailure=yes",
                "-L",
                "1:127.0.0.1:1",
                "-T",
                "-o",
                "ControlPath=/s",
                "prod"
            ]
        );
        assert_eq!(
            cancel_argv(&ssh, "1:127.0.0.1:1").expect("argv")[..3],
            ["ssh", "-O", "cancel"]
        );
        assert_eq!(control_argv(&[], "x"), None);
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
