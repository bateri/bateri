//! The holders of the bateri that kept its programs in a process of its own.
//!
//! **Transitional: delete this module, and the sweep's use of
//! [`holder_listening`], one release after the one that brings it.** The
//! bateri before the one that keeps its terminals in kablo (the 0.22 line)
//! spawned a `bateri hold` process whenever programs had to outlive it: at the
//! moment of an update, and — when `keep_running` asked — beside every
//! running bateri, to be left holding the programs of a crash or a quit. Such
//! a holder keeps a copy of each pane's PTY master and waits for the next
//! bateri to take them, which nothing does any more: left alone it holds the
//! programs for good (the update's for two minutes, the others without a
//! limit), out of sight, with their ports.
//!
//! The first launch therefore ends the holders it can tell are the ones this
//! bateri replaces ([`end`]); the update's holder, while it waits, is also how
//! that launch knows it is an update's arrival ([`update_waiting`]: the ledger of
//! that bateri does not say so). The programs end with them, once: the holder's
//! copy of a master is the last one the kernel counts, so closing it hangs the
//! terminal up. A holder is told by what the kernel says of it, never by what
//! its socket says: the holder's sockets are `handover` and `handover-<pid>`
//! in an instance directory, the process at the other end of one is read from
//! the connection's credentials, and it is ended only if
//!
//! - it is this user's,
//! - it runs the executable this process runs, with `hold` as its first
//!   argument (a holder of another build, of an installed bateri while this is
//!   a development build, is not ours to end),
//! - and its spawner is gone (it was inherited by `launchd`): a bound holder
//!   of a bateri that is still running is that bateri's safety net.
//!
//! A holder that fails any of these stays, and so does its directory
//! ([`holder_listening`] is what keeps the ssh sweep away from it).

use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::jobs::{self, ProcessTable};
use crate::ssh_route;

/// The socket of a holder born at the moment of an update, in an instance
/// directory.
const UPDATE_SOCKET: &str = "handover";

/// A bound holder's socket: this prefix and the holder's pid.
const BOUND_SOCKET_PREFIX: &str = "handover-";

/// How long a holder is given to go after `SIGTERM` before it is killed.
const GRACE: Duration = Duration::from_millis(1500);

/// The pid `launchd` (and `init`) adopts a process whose parent has gone to.
const INIT: u32 = 1;

/// Whether `name` is a holder's socket: `handover`, or a bound holder's
/// `handover-<pid>`.
pub fn is_holder_socket_name(name: &str) -> bool {
    name == UPDATE_SOCKET
        || name.strip_prefix(BOUND_SOCKET_PREFIX).is_some_and(|pid| {
            (1..=10).contains(&pid.len()) && pid.bytes().all(|byte| byte.is_ascii_digit())
        })
}

/// The holder sockets in `dir`.
fn holder_sockets(dir: &Path) -> Vec<PathBuf> {
    use std::os::unix::fs::FileTypeExt;
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|entry| {
            entry.file_type().is_ok_and(|kind| kind.is_socket())
                && entry
                    .file_name()
                    .to_str()
                    .is_some_and(is_holder_socket_name)
        })
        .map(|entry| entry.path())
        .collect()
}

/// Whether a holder still listens in `dir`. A probe: the holder closes the
/// connection unanswered. The ssh sweep leaves such a directory alone — the
/// holder's socket is the only way to find it.
pub fn holder_listening(dir: &Path) -> bool {
    holder_sockets(dir)
        .iter()
        .any(|socket| UnixStream::connect(socket).is_ok())
}

/// Whether `process` is the holder this bateri retires ([the module](self)'s
/// three conditions), given what the kernel says of it.
fn retires(table: &impl ProcessTable, pid: u32, exe: &Path, uid: u32) -> bool {
    table.uid(pid) == Some(uid)
        && table.parent(pid) == Some(INIT)
        && table.procargs(pid, &[]).is_some_and(|process| {
            process.args.get(1).map(String::as_str) == Some("hold")
                && same_file(Path::new(&process.exec), exe)
        })
}

/// Whether two paths name one file: the same path, or the same file once the
/// links are resolved (a bundle replaced in place by an update keeps its path;
/// one reached through a link is the same file).
fn same_file(left: &Path, right: &Path) -> bool {
    if left == right {
        return true;
    }
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

/// The processes with a holder's socket in the instance directories under
/// `roots`, each with the socket it was found by: the process at the other end
/// of a connection to the socket, read from the connection's credentials.
/// Only a process of this user's `uid` and not `me`.
fn listening(roots: &[PathBuf], uid: u32, me: u32) -> Vec<(u32, PathBuf)> {
    let mut found: Vec<(u32, PathBuf)> = Vec::new();
    for entry in ssh_route::instance_directories(roots) {
        for socket in holder_sockets(&entry) {
            let Ok(stream) = UnixStream::connect(&socket) else {
                continue;
            };
            let Ok((peer_uid, pid)) = jobs::peer(&stream) else {
                continue;
            };
            if peer_uid == uid && pid != me && !found.iter().any(|(known, _)| *known == pid) {
                found.push((pid, socket));
            }
        }
    }
    found
}

/// Ends the holders `(pid, start)`, each only if it is still the process that started at
/// `start`: `SIGTERM` to all, then `SIGKILL` after one [`GRACE`] to those still there. The
/// number gone.
fn terminate(holders: &[(u32, u64)]) -> usize {
    let same = |&(pid, start): &(u32, u64)| jobs::start_time(pid) == Some(start);
    let signal = |&(pid, _): &(u32, u64), signal: libc::c_int| {
        if let Ok(raw) = libc::pid_t::try_from(pid) {
            // SAFETY: `raw` is the pid of a process just checked to be the one that started
            // at the time the holder was verified at; a signal has no memory preconditions.
            unsafe { libc::kill(raw, signal) };
        }
    };
    let mut standing: Vec<(u32, u64)> = holders.iter().copied().filter(same).collect();
    for holder in &standing {
        signal(holder, libc::SIGTERM);
    }
    let deadline = Instant::now() + GRACE;
    while !standing.is_empty() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
        standing.retain(same);
    }
    for holder in &standing {
        // The start time is checked again just now.
        if same(holder) {
            signal(holder, libc::SIGKILL);
        }
    }
    if !standing.is_empty() {
        std::thread::sleep(Duration::from_millis(50));
    }
    holders.iter().filter(|holder| !same(holder)).count()
}

/// The holder this bateri retires among those listening in `roots`, as `(pid, start time)`:
/// the start time is read **before** the process is judged and is what [`terminate`] holds the
/// signal to, so a holder that went and a pid that was given to another process in between
/// is not signalled.
fn retired(
    roots: &[PathBuf],
    exe: &Path,
    uid: u32,
    me: u32,
    table: &impl ProcessTable,
    only: impl Fn(&Path) -> bool,
) -> Vec<(u32, u64)> {
    let mut found = Vec::new();
    for (pid, socket) in listening(roots, uid, me) {
        if !only(&socket) {
            continue;
        }
        let Some(start) = jobs::start_time(pid) else {
            continue;
        };
        if !retires(table, pid, exe, uid) {
            eprintln!(
                "bateri: a holder of programs at {} is not one this bateri retires, left alone",
                socket.display()
            );
            continue;
        }
        found.push((pid, start));
    }
    found
}

/// Whether the **update's** holder of the bateri before kablo — the one that was born at the
/// moment of an update and waits for a bateri to take its programs — is there: this launch is
/// that update's arrival. Asked **before** [`end`] runs, which ends it; the answer is what
/// tells a first launch after such an update from the launch after a crash.
pub fn update_waiting(roots: &[PathBuf], exe: &Path) -> bool {
    // SAFETY: `getuid` has no preconditions and cannot fail.
    let uid = unsafe { libc::getuid() };
    update_waiting_with(roots, exe, uid, std::process::id(), &jobs::SystemTable)
}

/// [`update_waiting`] with the process table given.
fn update_waiting_with(
    roots: &[PathBuf],
    exe: &Path,
    uid: u32,
    me: u32,
    table: &impl ProcessTable,
) -> bool {
    !retired(roots, exe, uid, me, table, |socket| {
        socket.file_name().and_then(|name| name.to_str()) == Some(UPDATE_SOCKET)
    })
    .is_empty()
}

/// Ends the retired holders listening in the instance directories under
/// `roots` (the ssh sockets' roots), those of the executable `exe`. The
/// number of holders gone. Slow (a connection per socket, a wait for the
/// holders): not on the main thread, and **before** the ssh sweep, which
/// would remove the directories of a dead bateri whose holder still listens in
/// them.
pub fn end(roots: &[PathBuf], exe: &Path) -> usize {
    // SAFETY: `getuid` has no preconditions and cannot fail.
    let uid = unsafe { libc::getuid() };
    end_with(
        roots,
        exe,
        uid,
        std::process::id(),
        &jobs::SystemTable,
        &mut terminate,
    )
}

/// What ends the holders `(pid, start time)` and says how many are gone ([`terminate`]).
type Ending<'a> = &'a mut dyn FnMut(&[(u32, u64)]) -> usize;

/// [`end`] with the process table and the ending given.
fn end_with(
    roots: &[PathBuf],
    exe: &Path,
    uid: u32,
    me: u32,
    table: &impl ProcessTable,
    end: Ending<'_>,
) -> usize {
    let holders = retired(roots, exe, uid, me, table, |_| true);
    if holders.is_empty() {
        return 0;
    }
    end(&holders)
}

#[cfg(test)]
mod tests {
    use std::os::unix::net::UnixListener;
    use std::process::Command;
    use std::sync::atomic::{AtomicU32, Ordering};

    use super::*;
    use crate::jobs::{Groups, ProcArgs};

    /// A private scratch directory under the system's — short, the sockets in it
    /// must fit `sun_path` — removed on drop.
    struct Dir(PathBuf);

    impl Dir {
        fn new() -> Dir {
            use std::os::unix::fs::PermissionsExt;
            static NEXT: AtomicU32 = AtomicU32::new(0);
            let path = std::env::temp_dir().join(format!(
                "rh{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&path).expect("a directory");
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))
                .expect("private");
            Dir(path)
        }
    }

    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// An instance directory under `root` with a holder's socket this process
    /// listens on: the process at its other end is the test's own.
    fn listening_instance(root: &Path, socket: &str) -> (PathBuf, UnixListener) {
        use std::os::unix::fs::PermissionsExt;
        let dir = root.join("0a1b2c3d");
        std::fs::create_dir_all(&dir).expect("an instance directory");
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).expect("private");
        let listener = UnixListener::bind(dir.join(socket)).expect("a socket");
        (dir, listener)
    }

    fn user() -> u32 {
        // SAFETY: `getuid` has no preconditions and cannot fail.
        unsafe { libc::getuid() }
    }

    /// What the kernel says of one process, whichever pid is asked.
    struct Process {
        uid: Option<u32>,
        parent: Option<u32>,
        exec: &'static str,
        args: Vec<&'static str>,
    }

    const BATERI: &str = "/Applications/bateri.app/Contents/MacOS/bateri";
    const DEV: &str = "/Applications/bateri-dev.app/Contents/MacOS/bateri";

    /// A holder of `BATERI` whose spawner is gone, that belongs to this user.
    fn holder() -> Process {
        Process {
            uid: Some(user()),
            parent: Some(INIT),
            exec: BATERI,
            args: vec![BATERI, "hold", "--fd", "3"],
        }
    }

    impl ProcessTable for Process {
        fn children(&self, _: u32) -> Vec<u32> {
            Vec::new()
        }
        fn groups(&self, _: u32) -> Option<Groups> {
            None
        }
        fn members(&self, _: u32) -> Vec<u32> {
            Vec::new()
        }
        fn parent(&self, _: u32) -> Option<u32> {
            self.parent
        }
        fn name(&self, _: u32) -> Option<String> {
            None
        }
        fn procargs(&self, _: u32, _: &[&str]) -> Option<ProcArgs> {
            Some(ProcArgs {
                exec: self.exec.to_owned(),
                args: self.args.iter().map(|arg| (*arg).to_owned()).collect(),
                env: Vec::new(),
            })
        }
        fn uid(&self, _: u32) -> Option<u32> {
            self.uid
        }
    }

    #[test]
    fn the_names_of_a_holders_sockets_are_told_from_any_other() {
        assert!(is_holder_socket_name("handover"));
        assert!(is_holder_socket_name("handover-4242"));
        assert!(!is_holder_socket_name("handover-"));
        assert!(!is_holder_socket_name("handover-4x"));
        assert!(!is_holder_socket_name("handover-12345678901"));
        assert!(!is_holder_socket_name("hold"));
        assert!(!is_holder_socket_name("u-handover"));
    }

    #[test]
    fn a_directory_is_held_while_something_listens_in_it_and_not_after() {
        let root = Dir::new();
        let (dir, listener) = listening_instance(&root.0, "handover-4242");
        assert!(holder_listening(&dir));
        drop(listener);
        assert!(
            !holder_listening(&dir),
            "the socket file stays, nobody listens"
        );
        let other = Dir::new();
        let (dir, _kept) = listening_instance(&other.0, "hold");
        assert!(
            !holder_listening(&dir),
            "the pane's own socket is not a holder's"
        );
    }

    #[test]
    fn the_holder_this_bateri_retires_is_the_one_that_matches_all_three() {
        let exe = Path::new(BATERI);
        assert!(retires(&holder(), 4242, exe, user()));
        // Another user's.
        assert!(!retires(&holder(), 4242, exe, user() + 1));
        assert!(!retires(
            &Process {
                uid: Some(user() + 1),
                ..holder()
            },
            4242,
            exe,
            user()
        ));
        // Another build's: a development build does not end the installed one's.
        assert!(!retires(
            &Process {
                exec: DEV,
                args: vec![DEV, "hold"],
                ..holder()
            },
            4242,
            exe,
            user()
        ));
        // One whose spawner is alive.
        assert!(!retires(
            &Process {
                parent: Some(777),
                ..holder()
            },
            4242,
            exe,
            user()
        ));
        // Another command of the same executable.
        assert!(!retires(
            &Process {
                args: vec![BATERI, "ssh-hold"],
                ..holder()
            },
            4242,
            exe,
            user()
        ));
        // Unreadable: left alone.
        assert!(!retires(
            &Process {
                uid: None,
                parent: None,
                ..holder()
            },
            4242,
            exe,
            user()
        ));
    }

    /// `end_with` over a holder's socket this process listens on, with `table`
    /// describing the process behind it: the pids that were ended.
    fn ended_with(table: &Process, me: u32) -> Vec<u32> {
        let root = Dir::new();
        let (_dir, _listener) = listening_instance(&root.0, "handover");
        let mut ended = Vec::new();
        end_with(
            std::slice::from_ref(&root.0),
            Path::new(BATERI),
            user(),
            me,
            table,
            &mut |holders| {
                ended.extend(holders.iter().map(|&(pid, _)| pid));
                holders.len()
            },
        );
        ended
    }

    #[test]
    fn a_holder_that_matches_is_ended_and_one_that_does_not_stays() {
        let this = std::process::id();
        // The listener is this process: it stands in for the holder, and the
        // process asking is another number.
        assert_eq!(ended_with(&holder(), this + 1), [this]);
        // The same socket, the process of another build, or of a live spawner.
        assert!(
            ended_with(
                &Process {
                    exec: DEV,
                    args: vec![DEV, "hold"],
                    ..holder()
                },
                this + 1
            )
            .is_empty()
        );
        assert!(
            ended_with(
                &Process {
                    parent: Some(this + 2),
                    ..holder()
                },
                this + 1
            )
            .is_empty()
        );
        // And the asking process is never a holder to end.
        assert!(ended_with(&holder(), this).is_empty());
    }

    #[test]
    fn a_process_is_ended_and_a_reused_pid_is_not() {
        use std::os::unix::process::CommandExt as _;
        let mut command = Command::new("/bin/sleep");
        command.arg("60");
        // SAFETY: the closure runs in the forked child before `exec` and only calls `close`,
        // which is async-signal-safe. The child holds no descriptor of the other tests (a
        // terminal's slave copied into it would keep that terminal from ever ending).
        unsafe {
            command.pre_exec(|| {
                for fd in 3..1024 {
                    libc::close(fd);
                }
                Ok(())
            });
        }
        let mut child = command.spawn().expect("a child");
        let pid = child.id();
        let start = jobs::start_time(pid).expect("a start time");
        // Not the process that started then: nothing is signalled, and it counts as gone.
        assert_eq!(terminate(&[(pid, start + 1)]), 1);
        assert_eq!(jobs::start_time(pid), Some(start), "still running");
        // The process it was: gone. (A holder is an orphan its init reaps; the test's child
        // is reaped by a thread, or it would stay a zombie that still has a start time.)
        let reaper = std::thread::spawn(move || child.wait());
        assert_eq!(terminate(&[(pid, start)]), 1);
        assert!(!reaper.join().expect("joined").expect("reaped").success());
    }

    #[test]
    fn only_the_update_holder_says_an_update_is_waiting() {
        let this = std::process::id();
        let waiting = |socket: &str, table: &Process| {
            let root = Dir::new();
            let (_dir, _listener) = listening_instance(&root.0, socket);
            update_waiting_with(
                std::slice::from_ref(&root.0),
                Path::new(BATERI),
                user(),
                this + 1,
                table,
            )
        };
        assert!(waiting("handover", &holder()), "the update's holder");
        assert!(
            !waiting("handover-4242", &holder()),
            "a bound holder is a crash's or a quit's"
        );
        assert!(
            !waiting(
                "handover",
                &Process {
                    exec: DEV,
                    args: vec![DEV, "hold"],
                    ..holder()
                }
            ),
            "another build's"
        );
    }
}
