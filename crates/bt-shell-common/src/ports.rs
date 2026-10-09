//! The TCP ports a pane's programs listen on: the dock's `↗ :3000  :6006`
//! and Shell ▸ Open Port ▸.
//!
//! **Whose ports**: every process in the shell's tree — the shell, the
//! foreground job, a background `&` job, a nested shell's programs, a dev
//! server's worker that does the listening (Next.js's `next dev` listens in its
//! `next-server` child, measured). On macOS also every process whose
//! controlling terminal is the pane's: a program whose parent exited keeps the
//! terminal but leaves the tree (`npm`'s server after `npm` is gone). Outside:
//! a daemon that double-forks out of the tree (`pm2`, `brew services`), a
//! container's published port (the port is held by Docker's own process), and
//! another user's process — root's included: its socket list is closed to us
//! (measured: `PROC_PIDLISTFDS` on pid 1 is `EPERM`), so a server started with
//! `sudo` does not show.
//!
//! **What**: TCP sockets in LISTEN. IPv4 and IPv6 on the same port of the same
//! process are one listener; UDP has no listening state and a dev server is TCP.
//!
//! Two halves: the pure walk over a [`ProcessTable`] ([`pane_pids`]) and the
//! system's body ([`listening`]: `libproc` on macOS, `/proc` on Linux). Never
//! on the frame path, nor on the main thread: the pane runs a scan on a queue
//! of its own after an edge.

use std::collections::BTreeSet;

use crate::jobs::{ProcessTable, ShellParent, shell_pid};

/// The most processes a walk visits — a guard, not a policy: a pane with more
/// is a fork bomb, not a dev server.
const WALK_LIMIT: usize = 4096;

/// Where a listener is bound.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Bound {
    /// Every interface (`0.0.0.0`, `::`): reachable from other machines too.
    Any,
    /// The loopback (`127.0.0.1`, `::1`): this Mac only.
    Loopback,
    /// One address of this machine.
    Address,
}

/// A TCP port a pane's process listens on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Listener {
    pub port: u16,
    pub bound: Bound,
    pub pid: u32,
    /// The process as `ps` shows it: its `argv[0]`'s last component (a
    /// rewritten title such as `next-server (v16.0.10)` kept whole), else the
    /// kernel's name.
    pub name: String,
}

impl Listener {
    /// The address that opens it from this Mac.
    pub fn url(&self) -> String {
        url(self.port)
    }
}

/// `http://localhost:{port}` — the address that opens a local port.
pub fn url(port: u16) -> String {
    format!("http://localhost:{port}")
}

/// The pids of the pane whose PTY child is `child`: the shell and its
/// descendants, then `tty_members` (the terminal's other processes) — each
/// once, in that order. Empty while `login` has forked no shell yet.
pub fn pane_pids(
    parent: ShellParent,
    child: u32,
    table: &impl ProcessTable,
    tty_members: &[u32],
) -> Vec<u32> {
    let Some(shell) = shell_pid(parent, child, table) else {
        return Vec::new();
    };
    let mut seen = BTreeSet::new();
    let mut order = Vec::new();
    let mut stack = vec![shell];
    while let Some(pid) = stack.pop() {
        if order.len() >= WALK_LIMIT {
            break;
        }
        if !seen.insert(pid) {
            continue;
        }
        order.push(pid);
        stack.extend(table.children(pid).into_iter().rev());
    }
    for &pid in tty_members {
        // `login` holds the terminal too; it is root's and lists nothing,
        // but it is not the pane's program either.
        if pid != child && order.len() < WALK_LIMIT && seen.insert(pid) {
            order.push(pid);
        }
    }
    order
}

/// The listeners of the pane whose PTY child is `child`, ascending by port,
/// then by pid; one per port and process.
pub fn pane_listeners(parent: ShellParent, child: u32, table: &impl ProcessTable) -> Vec<Listener> {
    let tty = shell_pid(parent, child, table)
        .map(terminal_members)
        .unwrap_or_default();
    let mut found: Vec<Listener> = Vec::new();
    for pid in pane_pids(parent, child, table, &tty) {
        let sockets = listening(pid);
        if sockets.is_empty() {
            continue;
        }
        let name = display_name(pid, table);
        for (port, bound) in sockets {
            found.push(Listener {
                port,
                bound,
                pid,
                name: name.clone(),
            });
        }
    }
    merge(found)
}

/// One listener per (port, pid), ascending; the widest binding wins
/// (`Any` over `Loopback`): IPv4's `0.0.0.0` and IPv6's `::1` on the same
/// port are reachable from outside.
fn merge(mut found: Vec<Listener>) -> Vec<Listener> {
    found.sort_by_key(|listener| (listener.port, listener.pid, listener.bound));
    found.dedup_by(|later, kept| later.port == kept.port && later.pid == kept.pid);
    found
}

/// The process's name as `ps` shows it ([`Listener::name`]).
fn display_name(pid: u32, table: &impl ProcessTable) -> String {
    let argv0 = table
        .procargs(pid, &[])
        .and_then(|record| record.args.into_iter().next())
        .filter(|arg| !arg.is_empty())
        .map(|arg| {
            // A path's last component; a rewritten title with spaces whole.
            if arg.starts_with('/') && !arg.contains(' ') {
                arg.rsplit('/').next().unwrap_or(&arg).to_owned()
            } else {
                arg
            }
        });
    argv0
        .or_else(|| table.name(pid))
        .unwrap_or_else(|| pid.to_string())
}

/// The other processes whose controlling terminal is the shell's: macOS's
/// `PROC_TTY_ONLY` listing by the shell's terminal device. Empty on Linux,
/// where the tree alone is walked: a listing there reads every `/proc/<pid>/stat`.
#[cfg(target_os = "macos")]
fn terminal_members(shell: u32) -> Vec<u32> {
    /// `PROC_TTY_ONLY` (`<sys/proc_info.h>`, not in `libc`): 3, read from the
    /// header.
    const PROC_TTY_ONLY: u32 = 3;
    // SAFETY: `proc_bsdinfo` is a plain C struct holding only integers and
    // `c_char` arrays; all zero bytes is a valid value of it.
    let Some(info) =
        (unsafe { crate::jobs::pid_info::<libc::proc_bsdinfo>(shell, libc::PROC_PIDTBSDINFO) })
    else {
        return Vec::new();
    };
    if info.e_tdev == 0 || info.e_tdev == u32::MAX {
        return Vec::new();
    }
    // SAFETY: a null buffer of size zero: the call returns the bytes it needs.
    let needed =
        unsafe { libc::proc_listpids(PROC_TTY_ONLY, info.e_tdev, std::ptr::null_mut(), 0) };
    let Ok(needed) = usize::try_from(needed) else {
        return Vec::new();
    };
    // Room for processes born between the two calls.
    let capacity = needed / size_of::<libc::pid_t>() + 16;
    let mut pids: Vec<libc::pid_t> = vec![0; capacity];
    let Ok(bytes) = std::ffi::c_int::try_from(capacity * size_of::<libc::pid_t>()) else {
        return Vec::new();
    };
    // SAFETY: the buffer is `capacity` pids long and belongs to this frame;
    // the size passed is exactly its byte length.
    let filled =
        unsafe { libc::proc_listpids(PROC_TTY_ONLY, info.e_tdev, pids.as_mut_ptr().cast(), bytes) };
    let count = usize::try_from(filled).unwrap_or(0) / size_of::<libc::pid_t>();
    pids.truncate(count.min(capacity));
    pids.into_iter()
        .filter_map(|pid| u32::try_from(pid).ok())
        .filter(|&pid| pid != 0)
        .collect()
}

#[cfg(target_os = "linux")]
fn terminal_members(_shell: u32) -> Vec<u32> {
    Vec::new()
}

/// The TCP ports `pid` listens on, with their binding; empty for a process we
/// cannot read (another user's) or that is gone.
#[cfg(target_os = "macos")]
pub fn listening(pid: u32) -> Vec<(u16, Bound)> {
    use std::ffi::c_int;

    // `struct socket_fdinfo` (`<sys/proc_info.h>`) is not in `libc`; the
    // offsets below were read from the header with `offsetof` (arm64 and
    // x86_64 share the layout: fixed-size integers, no pointers). A reply
    // whose size is not the struct's is refused, so a changed layout reads as
    // "no socket", never as a wrong port.
    const PROC_PIDFDSOCKETINFO: c_int = 3;
    const SOCKET_FDINFO_SIZE: usize = 792;
    const SOI_KIND: usize = 256;
    const SOCKINFO_TCP: i32 = 2;
    const INSI_LPORT: usize = 268;
    const INSI_VFLAG: usize = 288;
    const INSI_LADDR: usize = 312;
    const TCPSI_STATE: usize = 344;
    const TSI_S_LISTEN: i32 = 1;
    const INI_IPV4: u8 = 1;

    let Ok(c_pid) = c_int::try_from(pid) else {
        return Vec::new();
    };
    // SAFETY: a null buffer of size zero: the call returns the bytes it needs.
    let needed =
        unsafe { libc::proc_pidinfo(c_pid, libc::PROC_PIDLISTFDS, 0, std::ptr::null_mut(), 0) };
    let Ok(needed) = usize::try_from(needed) else {
        return Vec::new();
    };
    if needed == 0 {
        return Vec::new();
    }
    // Room for descriptors opened between the two calls.
    let capacity = needed / size_of::<libc::proc_fdinfo>() + 16;
    let mut fds: Vec<libc::proc_fdinfo> = vec![
        libc::proc_fdinfo {
            proc_fd: 0,
            proc_fdtype: 0,
        };
        capacity
    ];
    let Ok(bytes) = c_int::try_from(capacity * size_of::<libc::proc_fdinfo>()) else {
        return Vec::new();
    };
    // SAFETY: the buffer is `capacity` records long and belongs to this frame;
    // the size passed is exactly its byte length.
    let filled = unsafe {
        libc::proc_pidinfo(
            c_pid,
            libc::PROC_PIDLISTFDS,
            0,
            fds.as_mut_ptr().cast(),
            bytes,
        )
    };
    let count = usize::try_from(filled).unwrap_or(0) / size_of::<libc::proc_fdinfo>();
    fds.truncate(count.min(capacity));

    let mut found = Vec::new();
    let mut info = [0u8; SOCKET_FDINFO_SIZE];
    for fd in fds {
        if fd.proc_fdtype != libc::PROX_FDTYPE_SOCKET as u32 {
            continue;
        }
        // SAFETY: the buffer belongs to this frame and its size goes to the
        // call as is; the kernel writes at most that many bytes.
        let written = unsafe {
            libc::proc_pidfdinfo(
                c_pid,
                fd.proc_fd,
                PROC_PIDFDSOCKETINFO,
                info.as_mut_ptr().cast(),
                SOCKET_FDINFO_SIZE as c_int,
            )
        };
        if usize::try_from(written).ok() != Some(SOCKET_FDINFO_SIZE) {
            continue;
        }
        let int =
            |at: usize| i32::from_ne_bytes([info[at], info[at + 1], info[at + 2], info[at + 3]]);
        if int(SOI_KIND) != SOCKINFO_TCP || int(TCPSI_STATE) != TSI_S_LISTEN {
            continue;
        }
        // `insi_lport` holds the port in network order in its first two bytes
        // (lsof's `ntohs((u_short)insi_lport)`).
        let port = u16::from_be_bytes([info[INSI_LPORT], info[INSI_LPORT + 1]]);
        if port == 0 {
            continue;
        }
        let address = &info[INSI_LADDR..INSI_LADDR + 16];
        let bound = if info[INSI_VFLAG] & INI_IPV4 != 0 && address[..12].iter().all(|&b| b == 0) {
            // `ina_46.i46a_addr4`: the last four bytes.
            v4_bound([address[12], address[13], address[14], address[15]])
        } else {
            let mut v6 = [0u8; 16];
            v6.copy_from_slice(address);
            v6_bound(v6)
        };
        found.push((port, bound));
    }
    found
}

/// The TCP ports `pid` listens on: its descriptors' `socket:[inode]` links
/// matched against `/proc/net/tcp` and `tcp6`'s LISTEN rows.
#[cfg(target_os = "linux")]
pub fn listening(pid: u32) -> Vec<(u16, Bound)> {
    let Ok(entries) = std::fs::read_dir(format!("/proc/{pid}/fd")) else {
        return Vec::new();
    };
    let inodes: BTreeSet<u64> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| std::fs::read_link(entry.path()).ok())
        .filter_map(|target| socket_inode(&target.to_string_lossy()))
        .collect();
    if inodes.is_empty() {
        return Vec::new();
    }
    let mut found = Vec::new();
    for table in ["/proc/net/tcp", "/proc/net/tcp6"] {
        let Ok(text) = std::fs::read_to_string(table) else {
            continue;
        };
        found.extend(
            text.lines()
                .skip(1)
                .filter_map(parse_tcp_row)
                .filter(|row| row.listen && inodes.contains(&row.inode))
                .map(|row| (row.port, row.bound)),
        );
    }
    found
}

/// `socket:[12345]` → `12345`.
#[cfg(any(target_os = "linux", test))]
fn socket_inode(link: &str) -> Option<u64> {
    link.strip_prefix("socket:[")?
        .strip_suffix(']')?
        .parse()
        .ok()
}

/// A row of `/proc/net/tcp{,6}`.
#[cfg(any(target_os = "linux", test))]
#[derive(Debug, PartialEq, Eq)]
struct TcpRow {
    port: u16,
    bound: Bound,
    listen: bool,
    inode: u64,
}

/// `  0: 0100007F:0BB8 00000000:0000 0A … 0 12345 …` → the local port and
/// address, whether the state is LISTEN (`0A`) and the inode (field 10).
#[cfg(any(target_os = "linux", test))]
fn parse_tcp_row(line: &str) -> Option<TcpRow> {
    let fields: Vec<&str> = line.split_whitespace().collect();
    let (address, port) = parse_proc_address(fields.get(1)?)?;
    let state = fields.get(3)?;
    let inode = fields.get(9)?.parse().ok()?;
    Some(TcpRow {
        port,
        bound: bound_of(address),
        listen: *state == "0A",
        inode,
    })
}

/// `/proc/net/tcp{,6}`'s local address field, `0100007F:0BB8` → the address
/// and the port. The address is the kernel's words printed as numbers in
/// hex: IPv4's one word, IPv6's four; on the little-endian machines Linux
/// runs on, a word's bytes in memory are the address's order. Read on every
/// platform: a Linux server's answer comes to the Mac too.
pub fn parse_proc_address(field: &str) -> Option<(std::net::IpAddr, u16)> {
    let (address, port) = field.split_once(':')?;
    let port = u16::from_str_radix(port, 16).ok()?;
    if address.is_empty() || address.len() % 8 != 0 || !address.is_ascii() {
        return None;
    }
    let words: Vec<u32> = (0..address.len() / 8)
        .map(|i| u32::from_str_radix(&address[i * 8..i * 8 + 8], 16))
        .collect::<Result<_, _>>()
        .ok()?;
    let address = match words.as_slice() {
        [word] => std::net::IpAddr::from(word.to_le_bytes()),
        [a, b, c, d] => {
            let mut v6 = [0u8; 16];
            for (chunk, word) in v6.chunks_exact_mut(4).zip([a, b, c, d]) {
                chunk.copy_from_slice(&word.to_le_bytes());
            }
            std::net::IpAddr::from(v6)
        }
        _ => return None,
    };
    Some((address, port))
}

/// An address's binding.
pub fn bound_of(address: std::net::IpAddr) -> Bound {
    match address {
        std::net::IpAddr::V4(v4) => v4_bound(v4.octets()),
        std::net::IpAddr::V6(v6) => v6_bound(v6.octets()),
    }
}

/// An IPv4 address's binding.
fn v4_bound(octets: [u8; 4]) -> Bound {
    match octets {
        [0, 0, 0, 0] => Bound::Any,
        [127, ..] => Bound::Loopback,
        _ => Bound::Address,
    }
}

/// An IPv6 address's binding; an IPv4-mapped one by its IPv4 half.
fn v6_bound(octets: [u8; 16]) -> Bound {
    let address = std::net::Ipv6Addr::from(octets);
    if let Some(v4) = address.to_ipv4_mapped() {
        return v4_bound(v4.octets());
    }
    if address.is_unspecified() {
        Bound::Any
    } else if address.is_loopback() {
        Bound::Loopback
    } else {
        Bound::Address
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jobs::{Groups, ProcArgs};
    use std::collections::HashMap;

    /// A tree of pids: `children[pid]`.
    #[derive(Default)]
    struct Tree {
        children: HashMap<u32, Vec<u32>>,
    }

    impl ProcessTable for Tree {
        fn children(&self, pid: u32) -> Vec<u32> {
            self.children.get(&pid).cloned().unwrap_or_default()
        }
        fn groups(&self, _shell: u32) -> Option<Groups> {
            None
        }
        fn members(&self, _group: u32) -> Vec<u32> {
            Vec::new()
        }
        fn parent(&self, _pid: u32) -> Option<u32> {
            None
        }
        fn name(&self, _pid: u32) -> Option<String> {
            None
        }
        fn procargs(&self, _pid: u32, _keys: &[&str]) -> Option<ProcArgs> {
            None
        }
        fn uid(&self, _pid: u32) -> Option<u32> {
            None
        }
    }

    fn tree(edges: &[(u32, &[u32])]) -> Tree {
        Tree {
            children: edges
                .iter()
                .map(|&(pid, children)| (pid, children.to_vec()))
                .collect(),
        }
    }

    #[test]
    fn the_walk_takes_the_whole_tree_under_login_once() {
        // login 10 → zsh 11 → npm 12 → next 13 → next-server 14; a
        // background job 15 beside npm.
        let table = tree(&[(10, &[11]), (11, &[12, 15]), (12, &[13]), (13, &[14])]);
        assert_eq!(
            pane_pids(ShellParent::Login, 10, &table, &[]),
            [11, 12, 13, 14, 15]
        );
        // The terminal's members add the orphans, not login and not repeats.
        assert_eq!(
            pane_pids(ShellParent::Login, 10, &table, &[10, 14, 99]),
            [11, 12, 13, 14, 15, 99]
        );
        // The direct path starts at the child; no shell yet, nothing.
        assert_eq!(
            pane_pids(ShellParent::Direct, 11, &table, &[]),
            [11, 12, 13, 14, 15]
        );
        assert!(pane_pids(ShellParent::Login, 20, &table, &[30]).is_empty());
    }

    #[test]
    fn a_cycle_in_the_table_does_not_loop() {
        let table = tree(&[(1, &[2]), (2, &[1, 3])]);
        assert_eq!(pane_pids(ShellParent::Direct, 1, &table, &[]), [1, 2, 3]);
    }

    #[test]
    fn one_listener_per_port_and_process_the_widest_binding_kept() {
        let listener = |port, bound, pid| Listener {
            port,
            bound,
            pid,
            name: String::new(),
        };
        let merged = merge(vec![
            listener(6006, Bound::Loopback, 2),
            listener(3000, Bound::Loopback, 1),
            listener(3000, Bound::Any, 1),
            listener(3000, Bound::Any, 7),
        ]);
        assert_eq!(
            merged,
            [
                listener(3000, Bound::Any, 1),
                listener(3000, Bound::Any, 7),
                listener(6006, Bound::Loopback, 2),
            ]
        );
    }

    #[test]
    fn a_listening_socket_of_this_process_is_found_with_its_binding() {
        let loopback = std::net::TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        let any = std::net::TcpListener::bind("[::]:0").expect("bind any");
        let (loopback_port, any_port) = (
            loopback.local_addr().expect("addr").port(),
            any.local_addr().expect("addr").port(),
        );
        // A connected socket is not a listener.
        let _client = std::net::TcpStream::connect(("127.0.0.1", loopback_port)).expect("connect");
        let found = listening(std::process::id());
        assert!(
            found.contains(&(loopback_port, Bound::Loopback)),
            "{found:?}"
        );
        assert!(found.contains(&(any_port, Bound::Any)), "{found:?}");
        assert_eq!(
            found
                .iter()
                .filter(|(port, _)| *port == loopback_port)
                .count(),
            1,
            "{found:?}"
        );
    }

    #[test]
    fn a_vanished_process_lists_nothing() {
        assert!(listening(u32::MAX - 7).is_empty());
    }

    #[test]
    fn proc_net_rows_read_port_binding_state_and_inode() {
        let v4 = "   0: 0100007F:0BB8 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 12345 1 0000000000000000 100 0 0 10 0";
        assert_eq!(
            parse_tcp_row(v4),
            Some(TcpRow {
                port: 3000,
                bound: Bound::Loopback,
                listen: true,
                inode: 12345
            })
        );
        let any = "   1: 00000000:1770 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 777 1";
        assert_eq!(
            parse_tcp_row(any).map(|row| (row.port, row.bound)),
            Some((6000, Bound::Any))
        );
        let established = "   2: 0100007F:0BB8 0100007F:D431 01 00000000:00000000 00:00000000 00000000  1000        0 778 1";
        assert_eq!(
            parse_tcp_row(established).map(|row| row.listen),
            Some(false)
        );
        let v6_loopback = "   0: 00000000000000000000000001000000:1F90 00000000000000000000000000000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 999 1";
        assert_eq!(
            parse_tcp_row(v6_loopback).map(|row| (row.port, row.bound)),
            Some((8080, Bound::Loopback))
        );
        let v6_any = "   1: 00000000000000000000000000000000:1F91 00000000000000000000000000000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 998 1";
        assert_eq!(parse_tcp_row(v6_any).map(|row| row.bound), Some(Bound::Any));
        assert_eq!(socket_inode("socket:[12345]"), Some(12345));
        assert_eq!(socket_inode("pipe:[1]"), None);
    }
}
