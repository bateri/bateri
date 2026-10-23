//! Whether a job is running in the foreground outside a window's shell, and
//! if so, its name (the input to the close confirmation).
//!
//! **The process table is authoritative, not OSC 133**: the phase does not exist at all in a shell
//! without integration (`integration = "off"`, bash, fish), gets stuck in
//! `Running` permanently on a transition like `exec bash`, and does not name
//! the program. The terminal's foreground process group answers the same
//! question in every shell: if the group is not the shell's own group, a job
//! outside the shell is in the foreground.
//!
//! **The foreground group comes from the shell's `e_tpgid`**, not the PTY
//! child's: in an untimed session the child is `login(1)` and owned by root,
//! so `PROC_PIDTBSDINFO` returns zero bytes for it (measured). The shell
//! belongs to the user and the same call gives `ps`'s TPGID column for it.
//! Members of the foreground group are asked only for
//! short info (`PROC_PIDT_SHORTBSDINFO`), because that works on root-owned
//! processes too (a `sudo` group).
//!
//! Names come from the group's **leaves**: the leader may be a wrapper (leader
//! `bash`, the program its grandchild `claude`).
//!
//! Two halves: the pure decision ([`foreground`], whose input is a
//! [`ProcessTable`]) and the interface's system bodies ([`SystemTable`]:
//! `libc`'s `libproc` on macOS, `/proc` on Linux). The
//! decision is tested with a fake table, the body with a real PTY — a fake
//! table could not see that login is root.
//!
//! **Second consumer: the remote session** ([`remote`]). Same foreground
//! group, opposite direction: the close question names the group's
//! **leaves**, the remote session looks for the group's **topmost** ssh/mosh
//! process (`ssh -J`'s `ssh -W` child would give the jump host) and reads its
//! arguments (`KERN_PROCARGS2`, only for members with candidate names). The
//! probe runs on the `C` edge and, if undecided, on the next output edge
//! (`window::RemoteProbe`); known limits: a wrapper script that
//! starts ssh later locks in as "local" on the first probe, `exec ssh` does
//! not produce `C`, and `~^Z` removes the indicator until `fg`. The answer is
//! more than the host ([`Target`]): the argv that opens a second
//! door to the same place comes out of the same walk.
//!
//! **Known limits**: background jobs (`sleep 100 &`) are not in the
//! foreground and are not counted; a job running inside the shell itself (a
//! builtin loop, a function waiting on `read`) opens no separate group and
//! looks idle; `exec vim` inherits the shell's pid and group, so it is idle
//! too.

#[cfg(target_os = "macos")]
use std::ffi::{c_int, c_void};

use std::os::unix::ffi::OsStrExt as _;

use bt_core::RemoteKind;

use crate::ssh_wrap;

/// The shell's position relative to the PTY child — the side that spawns the
/// shell knows it and records it at spawn (`pane::TerminalPane::start_session`);
/// it is not guessed by comparing names. An untimed session gets it from the
/// same call as its command ([`crate::child::shell_command`]), so the two
/// cannot disagree.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShellParent {
    /// The child is `login(1)` and the shell is its child: both paths of an
    /// untimed session on macOS (`child::shell_command` and alacritty's macOS
    /// path).
    Login,
    /// The child is the shell itself: an untimed session on Linux (`$SHELL
    /// -l`, and alacritty's own path there), the timed run's fixed scripts and
    /// the real PTY test (login needs root, so it cannot be spawned in a test).
    Direct,
}

/// What is in the foreground.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Foreground {
    /// The shell is in the foreground — or there is no shell left to ask.
    Idle,
    /// A job outside the shell is in the foreground; its names in pid order,
    /// without duplicates. **An empty vector means nameless**: the table could
    /// not be read but it was counted as running.
    Running(Vec<String>),
}

/// The shell's own group and its terminal's foreground group — both from one call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Groups {
    pub(crate) own: u32,
    pub(crate) foreground: u32,
}

/// The answer of the remote session probe ([`remote`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Probe {
    /// The shell is still in the foreground or the child it forked has not
    /// `exec`ed yet: asked again on the next output edge.
    Undecided,
    /// No ssh/mosh in the foreground, not interactive, or the table is unreadable.
    Local,
    /// An interactive ssh/mosh and its target ([`Target`]).
    Remote(Target),
}

/// The remote target the probe found: the host to display and the
/// argv that opens a second door to the same place. The escaped line is
/// produced by `window::probe_remote` (`quote::command_line`), so there is
/// neither a shell nor escaping here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    /// As the user typed it; the `ssh://` scheme and the port are stripped.
    /// This is the **key** (marks, integration, saved passwords, file jobs):
    /// the port lives in [`Self::port`], never in here.
    pub host: String,
    /// The port the user asked for, if they did: the first of `-p`,
    /// `-o Port` and an `ssh://` target's, as OpenSSH reads them. Only what
    /// the argv says — a `Port` in
    /// `~/.ssh/config` is not known — and `None` for mosh, whose `-p` is a
    /// UDP port of the mosh server, not the door the user knocked on.
    pub port: Option<u16>,
    pub kind: RemoteKind,
    /// The argv to re-run ([`ssh_target`], [`mosh_argv`]).
    pub argv: Vec<String>,
    /// The nonce of a call bateri wrapped ([`ssh_wrap::nonce`]):
    /// taken from the process's argv **before** it is unwrapped, since
    /// [`Self::argv`] is the user's. The pane matches it against the
    /// bootstrap's `up`. `None` for a call bateri did not wrap and for mosh.
    pub nonce: Option<String>,
}

/// The seven things the decisions ask of the process table.
///
/// Lists are `Vec`, not `Option`: macOS's listing calls do not distinguish "no
/// such process" from "no children" (measured: zero for a nonexistent pid), so
/// there is no information for `None` to carry. The only witness to the
/// shell being alive is therefore the reader thread (`Session::reader_alive`).
pub trait ProcessTable {
    fn children(&self, pid: u32) -> Vec<u32>;
    /// Asked only of the shell (same user): long info is unreadable for root.
    fn groups(&self, shell: u32) -> Option<Groups>;
    fn members(&self, group: u32) -> Vec<u32>;
    fn parent(&self, pid: u32) -> Option<u32>;
    fn name(&self, pid: u32) -> Option<String>;
    /// The process's exec record — its executable, its argv and the
    /// environment variables named in `keys`, **no other** ([`ProcArgs`]) —
    /// in one read. `None` if unreadable, and for a process of another user
    /// whenever variables are asked (on macOS always). Asked only of members
    /// with candidate names ([`remote`], [`crate::program::find`]) and of
    /// the processes above a nested shell that bear a tool's name.
    fn procargs(&self, pid: u32, keys: &[&str]) -> Option<ProcArgs>;
    /// The process's **effective** user — what it may do, `sudo`'s `0`
    /// while its real user is the one who ran it. Readable for every
    /// process, root's included (its exec record is not): the root shell
    /// a guide bar names is known by its name and this alone
    /// ([`crate::program`]). `None` if the process is gone.
    fn uid(&self, pid: u32) -> Option<u32>;
}

/// One process's exec record ([`ProcessTable::procargs`]).
///
/// **The environment is never whole here**: only the variables the caller
/// named are kept; every other entry is skipped while the kernel's buffer
/// is parsed, before a `String` is made of it, and the buffer does not
/// outlive the read — so the rest never lands in a value, a `Debug` print or
/// a test's output. A program that reads the keyboard itself (an agent, a
/// REPL) routinely carries API keys in its environment, and bateri has no
/// business keeping them.
///
/// **The argv is not printed either** (a hand-written `Debug`: the
/// executable and the count): a command line carries passwords —
/// `mysql -psecret`, `redis-cli -a secret`, a URI's `user:secret@` — and a
/// `Debug` print ends up in a log or a test's output.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct ProcArgs {
    /// The executable as it was started. On Linux the kernel's resolved
    /// path (`/proc/<pid>/exe`); on macOS the path given to `execve`, made
    /// absolute from the executable itself when it was relative (`./node`
    /// would otherwise name a file relative to a directory we do not know).
    /// Empty when unreadable: another user's process on Linux, read without
    /// variables.
    pub exec: String,
    pub args: Vec<String>,
    /// The asked variables the process was started with, `(name, value)`
    /// in the environment's order; an asked variable that was not set is
    /// absent.
    pub env: Vec<(String, String)>,
}

impl std::fmt::Debug for ProcArgs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProcArgs")
            .field("exec", &self.exec)
            .field("argc", &self.args.len())
            .field("env", &self.env)
            .finish()
    }
}

impl ProcArgs {
    /// An asked variable's value; `None` if it was not set (or not asked).
    pub fn var(&self, key: &str) -> Option<&str> {
        self.env
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.as_str())
    }
}

/// `NAME=value` → `(NAME, value)` when `NAME` is one of `keys`; any other
/// entry is dropped here, before a `String` is made of it.
fn asked_var(entry: &[u8], keys: &[&str]) -> Option<(String, String)> {
    let eq = entry.iter().position(|&b| b == b'=')?;
    let name = keys.iter().find(|key| key.as_bytes() == &entry[..eq])?;
    Some((
        (*name).to_owned(),
        String::from_utf8_lossy(&entry[eq + 1..]).into_owned(),
    ))
}

/// The decision itself: `child` is the pid of the PTY's child.
///
/// The failure branches follow the direction of the error: a childless
/// `login` (⌘W right as the tab is born) is idle, because there is no job to
/// close; if the shell's group cannot be read it is **running without a
/// name**, because a systematic breakage should be a visible "always asks",
/// not a silent "never asks".
pub fn foreground(parent: ShellParent, child: u32, table: &impl ProcessTable) -> Foreground {
    // `login` forks a single shell; if there is none yet, there is no job to close.
    let Some(shell) = shell_pid(parent, child, table) else {
        return Foreground::Idle;
    };
    // A zero group means "the terminal has no foreground": the shell's
    // controlling terminal is not readable, and that too is the unreadable-table
    // branch — `members(0)` would be the kernel's answer, not a group's.
    let Some(groups) = table.groups(shell).filter(|groups| groups.foreground != 0) else {
        return Foreground::Running(Vec::new());
    };
    if groups.foreground == groups.own {
        return Foreground::Idle;
    }
    Foreground::Running(names(groups.foreground, table))
}

/// The shell's pid: on the direct path the child itself, on the `login` path
/// its only child (`None` if there is none yet).
pub(crate) fn shell_pid(parent: ShellParent, child: u32, table: &impl ProcessTable) -> Option<u32> {
    match parent {
        ShellParent::Direct => Some(child),
        ShellParent::Login => table.children(child).first().copied(),
    }
}

/// The process group the pane's shell leads, which the terminal's foreground group is
/// compared with to tell the shell in front of it ([`bt_core::pencere::ShellGroup`] is the
/// pane's question of it): the group of the shell [`shell_pid`] names — on the `login` path
/// the child's child, **not** the child's own pid, which names `login`'s group and made every
/// shell look like a program. `None` while there is no shell yet or its group cannot be
/// read: not known, and the caller does not take that for "a program is in front".
pub fn shell_group(parent: ShellParent, child: u32, table: &impl ProcessTable) -> Option<u32> {
    let shell = shell_pid(parent, child, table)?;
    table.groups(shell).map(|groups| groups.own)
}

/// Whether a job outside the shell's own process group holds the terminal —
/// the gate before asking whether the running program reads the keyboard
/// itself ([`bt_core::Session::note_raw`]). The criterion is [`remote`]'s
/// `Undecided` arm: the same group is the shell's own.
///
/// `false` is "no answer yet", never "not a program": the shell itself in the
/// foreground (its own raw moments — ZLE, a builtin `read -k`, a shell `exec`'d
/// in its place keeps its group), `login` with no shell forked yet, and an
/// unreadable group. The caller keeps its arm and asks on the next output
/// edge, so a systematic read error costs at most one probe per delay while
/// output flows — and the dock is never hidden on a guess.
pub fn job_in_foreground(parent: ShellParent, child: u32, table: &impl ProcessTable) -> bool {
    let Some(shell) = shell_pid(parent, child, table) else {
        return false;
    };
    table
        .groups(shell)
        .is_some_and(|groups| groups.foreground != 0 && groups.foreground != groups.own)
}

/// The PTY's two terminal modes that say whether a remote session is past its
/// login ([`bt_core::TtyModes`]): `tcgetattr` on the master's copy
/// (`bt_core::Session::with_pty_fd`). On macOS and Linux the master's
/// `tcgetattr` answers with the slave's flags — the program's (measured on
/// macOS; Linux's `tty_mode_ioctl` reads the linked tty; the test below runs
/// on both). `None` if the call fails.
pub fn tty_modes(fd: std::os::fd::BorrowedFd<'_>) -> Option<bt_core::TtyModes> {
    use std::os::fd::AsRawFd;
    // SAFETY: an all-zero `termios` is a valid value of a plain C struct; the call
    // only writes it.
    let mut modes: libc::termios = unsafe { std::mem::zeroed() };
    // SAFETY: `fd` is a borrowed, open descriptor for the call's duration and
    // `modes` a valid out pointer.
    if unsafe { libc::tcgetattr(fd.as_raw_fd(), &mut modes) } != 0 {
        return None;
    }
    Some(bt_core::TtyModes {
        canonical: modes.c_lflag & libc::ICANON != 0,
        echo: modes.c_lflag & libc::ECHO != 0,
    })
}

/// Whether `session`'s remote session is past its login ([`bt_core::Session::remote_login`]
/// with the PTY's modes from [`tty_modes`]): `Some(the remote generation)`.
/// The gate of the remote files' background jobs — they do not
/// connect while ssh still asks.
pub fn remote_login(session: &bt_core::Session) -> Option<u64> {
    session.remote_login(tty_modes)
}

/// Whether a remote session is in the foreground.
///
/// The failure semantics are the **opposite** of [`foreground`]'s: an
/// unreadable table is `Local`. There the safe direction was "always ask";
/// here it is the indicator's **absence** — and if it counted as undecided, a
/// systematic read error would produce a probe on every output edge for the
/// whole command.
///
/// `Undecided` only in two cases: the shell's group is still in the foreground
/// (`C` is printed before the fork), or no ssh/mosh was recognized and a group
/// member carries the shell's name (a forked child that has not `exec`ed yet).
/// The cost is a named limit: a loop running inside the shell itself, or
/// `zsh script`, stays undecided for the whole command and produces a probe per
/// output edge (at most one per main queue turn).
pub fn remote(parent: ShellParent, child: u32, table: &impl ProcessTable) -> Probe {
    // The pane's own program may be the pipe to a terminal a server keeps
    // (`ssh-hold --first`): there is no shell above it, so it is not found as a
    // member of the foreground group below. It is asked first and by itself —
    // after it ends the pane's program is the shell, which is not this.
    if let Some(found) = first_program(child, table) {
        return found;
    }
    let Some(shell) = shell_pid(parent, child, table) else {
        return Probe::Local;
    };
    let Some(groups) = table.groups(shell).filter(|groups| groups.foreground != 0) else {
        return Probe::Local;
    };
    if groups.foreground == groups.own {
        return Probe::Undecided;
    }
    let mut members = table.members(groups.foreground);
    if members.is_empty() {
        return Probe::Local;
    }
    members.sort_unstable();
    let names: Vec<Option<String>> = members.iter().map(|&pid| table.name(pid)).collect();
    // Recognized members and their targets; argv is asked only of candidate names.
    let recognized: Vec<(u32, Option<Target>)> = members
        .iter()
        .zip(&names)
        .filter_map(|(&pid, name)| {
            let name = name.as_deref()?;
            if !is_candidate(name) {
                return None;
            }
            let args = table.procargs(pid, &[])?.args;
            Some((pid, remote_target(name, &args)?))
        })
        .collect();
    let is_recognized = |pid: u32| recognized.iter().any(|&(member, _)| member == pid);
    // The group's topmost recognized processes — a member with a recognized
    // ancestor in the group is a substep (`ssh -J`'s `ssh -W` child, mosh's
    // bootstrap ssh). The walk is bounded by the group's size: a corrupt table
    // (a process that is its own parent) must not create a loop.
    let top = recognized.iter().filter(|(pid, _)| {
        let mut ancestor = table.parent(*pid);
        for _ in 0..members.len() {
            let Some(up) = ancestor.filter(|up| members.contains(up)) else {
                return true;
            };
            if is_recognized(up) {
                return false;
            }
            ancestor = table.parent(up);
        }
        true
    });
    // With several tops (a pipeline) the interactive one wins: in
    // `ssh backup cat dump | ssh prod` the first must not force a local answer.
    let mut found = false;
    for (_, target) in top {
        found = true;
        if let Some(target) = target {
            return Probe::Remote(target.clone());
        }
    }
    if found {
        return Probe::Local;
    }
    // Nothing was recognized but a member still carries the shell's name: a
    // forked child that has not `exec`ed yet — the other side of the pipeline
    // (`tee` in `ssh prod | tee log`) may have `exec`ed first.
    let shell_name = table.name(shell);
    if shell_name.is_some() && names.contains(&shell_name) {
        return Probe::Undecided;
    }
    Probe::Local
}

/// The answer when the pane's own program (`child`, whatever it was started as)
/// is `bateri ssh-hold …`: its target, or local for a holder that names none
/// (a command to run, no arguments). `None` for any other program, so the
/// shell's foreground group is asked instead.
fn first_program(child: u32, table: &impl ProcessTable) -> Option<Probe> {
    let name = table.name(child)?;
    if !is_bateri(&name) {
        return None;
    }
    let args = table.procargs(child, &[])?.args;
    Some(match remote_target(&name, &args)? {
        Some(target) => Probe::Remote(target),
        None => Probe::Local,
    })
}

/// A name whose argv is worth reading: ssh, mosh's client and the Perl running
/// the mosh script (`/usr/bin/perl` `exec`s into `perl5.NN`, so match by prefix).
fn is_candidate(name: &str) -> bool {
    name == "ssh" || name == "mosh-client" || name.starts_with("perl") || is_bateri(name)
}

/// Whether a process name is the application's binary, which is what a pane's
/// `ssh-hold` is (`bateri` and a renamed development copy that keeps the prefix).
fn is_bateri(name: &str) -> bool {
    name.starts_with("bateri")
}

/// The target of a recognized process: the outer `None` means "not recognized",
/// the inner `None` "recognized but not an interactive remote session".
fn remote_target(name: &str, args: &[String]) -> Option<Option<Target>> {
    let rest = args.get(1..).unwrap_or_default();
    if is_bateri(name) {
        // `bateri ssh-hold … -- <the ssh arguments>`: a pane showing a terminal
        // kept on a server is a remote session, to the server the arguments
        // name, as the `ssh` it stands in for would have been. Any other
        // subcommand of the binary is not recognized.
        if rest.first().map(String::as_str) != Some(crate::hold::SUBCOMMAND) {
            return None;
        }
        // A holder that was not started with ssh arguments (it resumes a
        // terminal it was shown) is recognized and names no target here.
        let ssh_args = crate::hold::ssh_args_in(&rest[1..]);
        return Some(ssh_args.and_then(|args| ssh_target("ssh", args)));
    }
    if name == "ssh" {
        // argv[0] as the process gives it (`ssh`, `/usr/bin/ssh`): a command
        // typed through an alias (`alias s=ssh`) is also `ssh` in the process.
        let program = args.first().map_or("ssh", String::as_str);
        return Some(ssh_target(program, rest));
    }
    if name == "mosh-client" {
        return Some(mosh_client_target(rest));
    }
    // Interpreter: the first non-option argument is the script's path.
    let script = rest.iter().position(|arg| !arg.starts_with('-'))?;
    let base = rest[script].rsplit('/').next().unwrap_or_default();
    let script_args = &rest[script + 1..];
    (base == "mosh").then(|| {
        mosh_target(script_args).map(|host| Target {
            host,
            port: None,
            kind: RemoteKind::Mosh,
            argv: mosh_argv(script_args),
            nonce: None,
        })
    })
}

/// mosh's re-run argv: `mosh` + the script's arguments, as they are. mosh has
/// no local forwarding, so there is nothing to filter out.
fn mosh_argv<S: AsRef<str>>(args: &[S]) -> Vec<String> {
    std::iter::once("mosh")
        .chain(args.iter().map(AsRef::as_ref))
        .map(str::to_owned)
        .collect()
}

/// ssh's short options that take a value (`ssh(1)`'s SYNOPSIS).
pub(crate) const SSH_VALUED: &str = "BbcDEeFIiJLlmOoPpQRSWw";
/// Options whose presence makes the session non-interactive: tunnel (`-N`),
/// forwarding (`-W`), control (`-O`), query (`-Q`, `-G`, `-V`) and no pty (`-T`).
const SSH_NON_INTERACTIVE: &str = "NWOQGVT";
/// Options **dropped** on re-run: local forwards (`-L`, `-R`,
/// `-D`, with their values) would try to bind the same local port in the
/// second session and print a warning, or with `ExitOnForwardFailure` not
/// connect at all; `-M` opens a second ControlMaster; `-f` drops to the
/// background. The `-o LocalForward=…` form is not filtered (a known limit).
const SSH_NOT_REPEATED: &str = "LRDMf";

/// ssh argv (argv[0] is `program`, `args` the rest) → the interactive
/// session's target and the re-run argv.
///
/// If there is a command after the target, the session is not interactive
/// without `-t`: `ssh prod uptime` is a one-second command and collapsing and
/// reopening the dock would be exactly the jump the user rejected.
///
/// The argv comes from the same walk ([`SshSession::options`]'s `kept`), not a
/// second parser: [`SSH_NOT_REPEATED`] is dropped, everything else stays in
/// order — including the target and a remote command with `-t`.
///
/// **A wrapped argv is unwrapped first** ([`ssh_wrap::unwrap`]): the
/// `-t` and the bootstrap command bateri added are not what the user typed,
/// so the target's line, `⏎ reconnect`, ⌘T and the file jobs see the user's
/// own argv.
fn ssh_target(program: &str, args: &[String]) -> Option<Target> {
    let call = ssh_call(program, ssh_wrap::unwrap(args))?;
    Some(Target {
        host: call.host,
        port: call.port,
        kind: RemoteKind::Ssh,
        argv: call.argv,
        nonce: ssh_wrap::nonce(args).map(str::to_owned),
    })
}

/// What one walk of an interactive ssh argv says ([`ssh_call`]).
pub(crate) struct SshCall {
    /// As typed; only the `ssh://` form drops its scheme and port ([`ssh_destination`]).
    pub(crate) host: String,
    /// The port the argv names ([`SshSession::port`]).
    pub(crate) port: Option<u16>,
    /// The re-run argv ([`ssh_target`]'s doc).
    pub(crate) argv: Vec<String>,
    /// A remote command follows the target (only with a forced tty, or the
    /// call would not be interactive).
    pub(crate) command: bool,
}

/// The walk behind [`ssh_target`], without unwrapping: `None` unless the
/// session is interactive. The wrapping decision (`ssh_wrap::decide`) asks
/// the same walk, so "interactive" has one definition.
pub(crate) fn ssh_call(program: &str, args: &[String]) -> Option<SshCall> {
    let mut session = SshSession::default();
    let mut argv = vec![program.to_owned()];
    let (index, terminated) = session.options(args, 0, &mut argv);
    let target = args.get(index)?;
    argv.push(target.clone());
    // A URI's port counts only if no option before the target named one, and
    // it comes before the options after the target (OpenSSH takes the first).
    let (host, uri_port) = ssh_destination(target);
    session.port = session.port.or(uri_port);
    // OpenSSH parses options **again** after the target (`ssh prod -p 2222`);
    // it does not if they ended with `--`.
    let rest = if terminated {
        index + 1
    } else {
        session.options(args, index + 1, &mut argv).0
    };
    let command = rest < args.len();
    if session.quiet || (command && !session.tty) {
        return None;
    }
    argv.extend(args[rest..].iter().cloned());
    Some(SshCall {
        host,
        port: session.port,
        argv,
        command,
    })
}

/// What the ssh options say about interactivity.
#[derive(Default)]
struct SshSession {
    /// `-t` or `RequestTTY=yes|force`.
    tty: bool,
    /// A non-interactive mode: [`SSH_NON_INTERACTIVE`], `RequestTTY=no` or
    /// `SessionType=none|subsystem`.
    quiet: bool,
    /// The first port the arguments name: `-p`, `-o Port` or the target's
    /// `ssh://…:port` ([`ssh_call`]). OpenSSH keeps the first too
    /// (`ssh -p 1 -p 2 host` connects to 1). A value that is not a port is
    /// skipped, so a later valid one can still be the first.
    port: Option<u16>,
}

impl SshSession {
    /// Reads the option set in `args[index..]`; returns the index of the first
    /// non-option argument and whether it ended with `--`.
    ///
    /// Writes the options it reads to `kept` for the re-run: a
    /// [`SSH_NOT_REPEATED`] flag is dropped from its cluster, and so is the
    /// value of one that takes a value (attached or a separate argument); a
    /// cluster left with no flags is dropped entirely (`-fM` → nothing,
    /// `-vL 1:x:1` → `-v`).
    fn options(
        &mut self,
        args: &[String],
        mut index: usize,
        kept: &mut Vec<String>,
    ) -> (usize, bool) {
        while let Some(arg) = args.get(index) {
            if arg == "--" {
                kept.push(arg.clone());
                return (index + 1, true);
            }
            let Some(cluster) = arg.strip_prefix('-').filter(|rest| !rest.is_empty()) else {
                break;
            };
            index += 1;
            let mut flags = String::from("-");
            let mut separate = None;
            for (at, flag) in cluster.char_indices() {
                if SSH_NON_INTERACTIVE.contains(flag) {
                    self.quiet = true;
                }
                if flag == 't' {
                    self.tty = true;
                }
                let repeated = !SSH_NOT_REPEATED.contains(flag);
                if repeated {
                    flags.push(flag);
                }
                if SSH_VALUED.contains(flag) {
                    // The value is either the rest of the cluster (`-p22`) or the next argument.
                    let attached = &cluster[at + flag.len_utf8()..];
                    let value = if attached.is_empty() {
                        index += 1;
                        let value = args.get(index - 1);
                        if repeated {
                            separate = value.cloned();
                        }
                        value.map(String::as_str)
                    } else {
                        if repeated {
                            flags.push_str(attached);
                        }
                        Some(attached)
                    };
                    match (flag, value) {
                        ('o', Some(value)) => self.config(value),
                        ('p', Some(value)) => self.name_port(value),
                        _ => {}
                    }
                    break;
                }
            }
            if flags.len() > 1 {
                kept.push(flags);
            }
            kept.extend(separate);
        }
        (index, false)
    }

    /// `-o Key=value` (or with a space): the keys that change interactivity,
    /// and `Port`.
    fn config(&mut self, option: &str) {
        let (key, value) = config_pair(option);
        if key.eq_ignore_ascii_case("RequestTTY") {
            if value.eq_ignore_ascii_case("yes") || value.eq_ignore_ascii_case("force") {
                self.tty = true;
            } else if value.eq_ignore_ascii_case("no") {
                self.quiet = true;
            }
        } else if key.eq_ignore_ascii_case("SessionType")
            && (value.eq_ignore_ascii_case("none") || value.eq_ignore_ascii_case("subsystem"))
        {
            self.quiet = true;
        } else if key.eq_ignore_ascii_case("Port") {
            self.name_port(value);
        }
    }

    /// Records `value` as the port unless one was named before it.
    fn name_port(&mut self, value: &str) {
        self.port = self.port.or_else(|| parse_port(value));
    }
}

/// An `-o` argument split the way `ssh_config(5)` spells a keyword and its
/// argument: `Key=value`, `Key value` and `Key = value`.
fn config_pair(option: &str) -> (&str, &str) {
    let option = option.trim_start();
    let end = option
        .find(|c: char| c == '=' || c.is_whitespace())
        .unwrap_or(option.len());
    let (key, rest) = option.split_at(end);
    let rest = rest.trim_start();
    (key, rest.strip_prefix('=').unwrap_or(rest).trim())
}

/// A TCP port: a decimal `1..=65535`. What is not one (`0`, `http`, `22x`) is
/// not the user's port.
fn parse_port(text: &str) -> Option<u16> {
    text.trim().parse::<u16>().ok().filter(|port| *port != 0)
}

/// The target as written and the port it names. Only in the `ssh://` form are
/// the scheme and the port stripped (the port is returned, not dropped).
fn ssh_destination(target: &str) -> (String, Option<u16>) {
    let Some(rest) = target.strip_prefix("ssh://") else {
        return (target.to_owned(), None);
    };
    let rest = rest.split('/').next().unwrap_or_default();
    let (user, host) = match rest.rsplit_once('@') {
        Some((user, host)) => (Some(user), host),
        None => (None, rest),
    };
    let (host, tail) = match host.strip_prefix('[') {
        // IPv6 is in square brackets; the port comes after the closing one.
        Some(inner) => inner.split_once(']').unwrap_or((inner, "")),
        None => host.split_at(host.find(':').unwrap_or(host.len())),
    };
    let port = tail.strip_prefix(':').and_then(parse_port);
    let host = match user {
        Some(user) => format!("{user}@{host}"),
        None => host.to_owned(),
    };
    (host, port)
}

/// The mosh script's options that take a value as a separate argument (the
/// `=` form like `--ssh=…` is already a single argument).
const MOSH_VALUED: [&str; 9] = [
    "-p",
    "--port",
    "--ssh",
    "--server",
    "--client",
    "--predict",
    "--family",
    "--bind-server",
    "--experimental-remote-ip",
];

/// The mosh script's argv (after the script's path) → the target: the first
/// non-option argument. In mosh a remote command also runs in an interactive
/// terminal, so it does not disqualify it.
fn mosh_target<S: AsRef<str>>(args: &[S]) -> Option<String> {
    let mut words = args.iter().map(AsRef::as_ref);
    while let Some(word) = words.next() {
        if word == "--" {
            return words.next().map(str::to_owned);
        }
        if !word.starts_with('-') {
            return Some(word.to_owned());
        }
        if MOSH_VALUED.contains(&word) {
            words.next();
        }
    }
    None
}

/// `mosh-client`'s `-#`: the script passes it its own command line in a single
/// argument (`"-# {argv} |"`), so the target comes from parsing that line as
/// mosh would.
///
/// The re-run argv also comes from that line: `mosh` + its whitespace-split
/// words. **Known limit:** the script joins the line without
/// quoting, so a value with spaces (`--ssh="ssh -i k"`) cannot be rebuilt and
/// is written in its split form — the same root as the host's limit.
fn mosh_client_target(args: &[String]) -> Option<Target> {
    let at = args.iter().position(|arg| arg.starts_with("-#"))?;
    let mut line = args[at].strip_prefix("-#").unwrap_or_default().trim();
    if line.is_empty() {
        line = args.get(at + 1)?.trim();
    }
    let line = line.strip_suffix('|').unwrap_or(line);
    // The line was joined without quoting: `--ssh="ssh -i ~/.ssh/k"` is split
    // into words and its value leaves a non-option word behind. A word carrying
    // a character that cannot appear in a host is therefore skipped.
    let words: Vec<&str> = line
        .split_whitespace()
        .filter(|word| !word.contains(['/', '=', '~']))
        .collect();
    let host = mosh_target(&words)?;
    let all: Vec<&str> = line.split_whitespace().collect();
    Some(Target {
        host,
        port: None,
        kind: RemoteKind::Mosh,
        argv: mosh_argv(&all),
        nonce: None,
    })
}

/// The foreground group's names: the leaves (members that are not the parent of
/// another member of the group) in pid order and without duplicates; if there
/// are no leaves, the leader's name (the leader's pid is the group's id), and
/// if not that either, none.
fn names(group: u32, table: &impl ProcessTable) -> Vec<String> {
    let mut members = table.members(group);
    members.sort_unstable();
    let parents: Vec<Option<u32>> = members.iter().map(|&pid| table.parent(pid)).collect();
    let mut names: Vec<String> = Vec::new();
    for &pid in &members {
        if parents.contains(&Some(pid)) {
            continue;
        }
        if let Some(name) = table.name(pid)
            && !names.contains(&name)
        {
            names.push(name);
        }
    }
    if names.is_empty() {
        names.extend(table.name(group));
    }
    names
}

/// The process table of the platform this build targets: `Libproc` on
/// macOS, `Procfs` on Linux. Callers name this alias, not a body, so the
/// platform shell reads the same line on both systems. A renaming re-export,
/// not a `type` alias: the bodies are unit structs and callers use the value.
#[cfg(target_os = "macos")]
pub use self::Libproc as SystemTable;
/// The process table of the platform this build targets (see the macOS alias).
#[cfg(target_os = "linux")]
pub use self::Procfs as SystemTable;

/// [`ProcessTable`]'s macOS body: `libc`'s Apple half (`libproc`). Not on the
/// frame path: a few system calls on the main thread at close time and when a
/// command starts (the remote session probe).
#[cfg(target_os = "macos")]
pub struct Libproc;

#[cfg(target_os = "macos")]
impl ProcessTable for Libproc {
    fn children(&self, pid: u32) -> Vec<u32> {
        pid_list(libc::proc_listchildpids, pid)
    }

    fn groups(&self, shell: u32) -> Option<Groups> {
        // SAFETY: `proc_bsdinfo` is a plain C struct holding only integers and
        // `c_char` arrays; all zero bytes is a valid value of it.
        let info: libc::proc_bsdinfo = unsafe { pid_info(shell, libc::PROC_PIDTBSDINFO)? };
        Some(Groups {
            own: info.pbi_pgid,
            foreground: info.e_tpgid,
        })
    }

    fn members(&self, group: u32) -> Vec<u32> {
        pid_list(libc::proc_listpgrppids, group)
    }

    fn parent(&self, pid: u32) -> Option<u32> {
        short_info(pid).map(|info| info.pbsi_ppid)
    }

    /// First `proc_name` (the long name), otherwise the short info's `comm`: the
    /// former fails on root-owned processes (measured, zero on `launchd`), the
    /// latter is truncated at 16 characters but readable on every process.
    /// Both are the executed file's name, not argv[0]: a program started
    /// through a link to a versioned file is called by its version.
    fn name(&self, pid: u32) -> Option<String> {
        let c_pid = c_int::try_from(pid).ok()?;
        // The size of `pbi_name`: twice `MAXCOMLEN`.
        let mut buf = [0u8; 2 * libc::MAXCOMLEN];
        // SAFETY: the buffer belongs to this frame and its size goes to the call
        // as is; the kernel writes at most that many bytes.
        let len = unsafe { libc::proc_name(c_pid, buf.as_mut_ptr().cast(), buf.len() as u32) };
        match usize::try_from(len) {
            Ok(len) if len > 0 => {
                Some(String::from_utf8_lossy(&buf[..len.min(buf.len())]).into_owned())
            }
            _ => {
                let info = short_info(pid)?;
                let bytes: Vec<u8> = info
                    .pbsi_comm
                    .iter()
                    .take_while(|&&c| c != 0)
                    .map(|&c| c as u8)
                    .collect();
                (!bytes.is_empty()).then(|| String::from_utf8_lossy(&bytes).into_owned())
            }
        }
    }

    /// The short info's `pbsi_uid`: the effective user (measured: `sudo`
    /// waiting at its prompt reads `0`, its `pbsi_ruid` the user's).
    fn uid(&self, pid: u32) -> Option<u32> {
        short_info(pid).map(|info| info.pbsi_uid)
    }

    fn procargs(&self, pid: u32, keys: &[&str]) -> Option<ProcArgs> {
        let mut record = process_args(pid, keys)?;
        // The kernel keeps the path `execve` was given; a relative one
        // (`./node`) is the executable's own path instead.
        if !record.exec.starts_with('/') {
            record.exec = executable_path(pid).unwrap_or_default();
        }
        Some(record)
    }
}

/// `proc_pidpath`: the executable's absolute path, symlinks resolved.
#[cfg(target_os = "macos")]
fn executable_path(pid: u32) -> Option<String> {
    let pid = c_int::try_from(pid).ok()?;
    let mut buf = vec![0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    // SAFETY: the buffer belongs to this frame and its size goes to the call
    // as is; the kernel writes at most that many bytes.
    let len = unsafe { libc::proc_pidpath(pid, buf.as_mut_ptr().cast(), buf.len() as u32) };
    let len = usize::try_from(len).ok().filter(|&len| len > 0)?;
    Some(String::from_utf8_lossy(&buf[..len.min(buf.len())]).into_owned())
}

/// `sysctl(KERN_PROCARGS2)`: the layout is `argc` (4 bytes), the exec path, NUL
/// padding, `argc` NUL-terminated arguments — then the **environment**
/// follows, of which only `keys` are kept ([`parse_procargs`]). The call
/// fails for another user's process, so that is `None`.
#[cfg(target_os = "macos")]
fn process_args(pid: u32, keys: &[&str]) -> Option<ProcArgs> {
    let pid = c_int::try_from(pid).ok()?;
    // The buffer is `kern.argmax` long: a query with an empty buffer gives this
    // ceiling, not the real size.
    // The ceiling does not change over the process lifetime: asked once.
    static ARGMAX: std::sync::OnceLock<Option<usize>> = std::sync::OnceLock::new();
    let capacity = (*ARGMAX.get_or_init(|| {
        let mut argmax: c_int = 0;
        let mut size = size_of::<c_int>();
        let mut mib = [libc::CTL_KERN, libc::KERN_ARGMAX];
        // SAFETY: `mib` has two elements and belongs to the frame; the output is a
        // single `c_int` and its size goes to the call as is.
        let status = unsafe {
            libc::sysctl(
                mib.as_mut_ptr(),
                2,
                (&raw mut argmax).cast(),
                &raw mut size,
                std::ptr::null_mut(),
                0,
            )
        };
        usize::try_from(argmax).ok().filter(|_| status == 0)
    }))?;
    let mut buf = vec![0u8; capacity];
    let mut len = capacity;
    let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid];
    // SAFETY: the buffer is `capacity` bytes and belongs to this frame; the
    // kernel writes at most `len` bytes and stores what it wrote in `len`.
    let status = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            3,
            buf.as_mut_ptr().cast(),
            &raw mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    if status != 0 {
        return None;
    }
    parse_procargs(buf.get(..len)?, keys)
}

/// The pure half of [`process_args`]. The environment's strings follow the
/// arguments up to the first empty string (the kernel pads the area with
/// NULs, and `apple[]` strings come after one); each is checked against
/// `keys` **before** it becomes a `String`, so an unasked variable is never
/// copied.
#[cfg(target_os = "macos")]
fn parse_procargs(buf: &[u8], keys: &[&str]) -> Option<ProcArgs> {
    let argc = usize::try_from(i32::from_ne_bytes(buf.get(..4)?.try_into().ok()?)).ok()?;
    let rest = &buf[4..];
    // The exec path, then NUL padding: argv[0] cannot be empty, so the first
    // non-NUL byte is the start of argv.
    let path_end = rest.iter().position(|&b| b == 0)?;
    let exec = String::from_utf8_lossy(&rest[..path_end]).into_owned();
    let start = path_end + rest[path_end..].iter().position(|&b| b != 0)?;
    let mut args = Vec::with_capacity(argc);
    let mut fields = rest[start..].split(|&b| b == 0);
    for _ in 0..argc {
        args.push(String::from_utf8_lossy(fields.next()?).into_owned());
    }
    let env = if keys.is_empty() {
        Vec::new()
    } else {
        fields
            .take_while(|entry| !entry.is_empty())
            .filter_map(|entry| asked_var(entry, keys))
            .collect()
    };
    Some(ProcArgs { exec, args, env })
}

/// The common signature of `proc_listchildpids` and `proc_listpgrppids`.
#[cfg(target_os = "macos")]
type PidLister = unsafe extern "C" fn(libc::pid_t, *mut c_void, c_int) -> c_int;

/// The pids of a listing call.
///
/// **The buffer size comes from the call's own answer**: a call with an empty
/// buffer gives an upper bound, which, as measured, is the number of all
/// processes on the system — there is room even for a child born between the
/// two calls. A fixed small buffer would be truncated **silently** (measured: 1
/// for `launchd`'s children with a one-pid buffer). The return value is the
/// **number of pids**, not bytes (measured).
#[cfg(target_os = "macos")]
fn pid_list(list: PidLister, key: u32) -> Vec<u32> {
    let Ok(key) = libc::pid_t::try_from(key) else {
        return Vec::new();
    };
    // SAFETY: empty buffer, zero size: the call only returns an estimate and
    // writes nothing.
    let estimate = unsafe { list(key, std::ptr::null_mut(), 0) };
    let capacity = usize::try_from(estimate).unwrap_or(0);
    let Ok(bytes) = c_int::try_from(capacity * size_of::<libc::pid_t>()) else {
        return Vec::new();
    };
    if capacity == 0 {
        return Vec::new();
    }
    let mut pids: Vec<libc::pid_t> = vec![0; capacity];
    // SAFETY: the buffer is `capacity` pids long and belongs to this frame; the
    // size passed to the call is exactly that many bytes.
    let filled = unsafe { list(key, pids.as_mut_ptr().cast(), bytes) };
    pids.truncate(usize::try_from(filled).unwrap_or(0).min(capacity));
    pids.into_iter()
        .filter_map(|pid| u32::try_from(pid).ok())
        .filter(|&pid| pid > 0)
        .collect()
}

/// Short info: parent and `comm` — readable on root-owned processes too.
#[cfg(target_os = "macos")]
fn short_info(pid: u32) -> Option<libc::proc_bsdshortinfo> {
    // SAFETY: `proc_bsdshortinfo` is a plain C struct holding only integers
    // and `c_char` arrays; all zero bytes is a valid value of it.
    unsafe { pid_info(pid, libc::PROC_PIDT_SHORTBSDINFO) }
}

/// The single-struct flavors of `proc_pidinfo`. Only a **complete** fill is
/// success: on a root-owned process the long info returns zero bytes, and a
/// half-filled struct would read as "group 0" from its zeros.
///
/// # Safety
///
/// `T` must be a plain C struct for which the all-zero-bits value is valid, and
/// `flavor` must be the flavor for which the kernel writes that struct.
#[cfg(target_os = "macos")]
pub(crate) unsafe fn pid_info<T>(pid: u32, flavor: c_int) -> Option<T> {
    let pid = c_int::try_from(pid).ok()?;
    let size = c_int::try_from(size_of::<T>()).ok()?;
    // SAFETY: the caller's promise — a zeroed `T` is valid.
    let mut info: T = unsafe { std::mem::zeroed() };
    // SAFETY: the buffer is a whole `T` and its size goes to the call as is; the
    // kernel writes at most that many bytes.
    let written = unsafe { libc::proc_pidinfo(pid, flavor, 0, (&raw mut info).cast(), size) };
    (written == size).then_some(info)
}

/// [`ProcessTable`]'s Linux body: `/proc`. Same contract as `Libproc` — an
/// unreadable or vanished process is `None` (or an empty list), never a panic.
///
/// Listings (`children`, `members`) scan every `/proc/<pid>/stat` instead of
/// reading `/proc/<pid>/task/<tid>/children`: the latter needs
/// `CONFIG_PROC_CHILDREN` and lists only one thread's children. A scan is a
/// few hundred small reads, and the table is asked only at close time and when
/// a command starts — never on the frame path.
///
/// The foreground group is the shell's `tpgid` (field 8 of `stat`), the same
/// quantity as macOS's `e_tpgid`; `-1` (no controlling terminal) becomes `0`,
/// which [`foreground`] already reads as "unreadable".
#[cfg(target_os = "linux")]
pub struct Procfs;

#[cfg(target_os = "linux")]
impl ProcessTable for Procfs {
    fn children(&self, pid: u32) -> Vec<u32> {
        scan(|stat| stat.parent == pid)
    }

    fn groups(&self, shell: u32) -> Option<Groups> {
        let stat = read_stat(shell)?;
        Some(Groups {
            own: stat.group,
            foreground: stat.terminal_group,
        })
    }

    fn members(&self, group: u32) -> Vec<u32> {
        scan(|stat| stat.group == group)
    }

    fn parent(&self, pid: u32) -> Option<u32> {
        read_stat(pid).map(|stat| stat.parent)
    }

    /// `/proc/<pid>/comm`: the kernel's name, truncated at 15 bytes (macOS's
    /// short-info fallback is truncated the same way).
    fn name(&self, pid: u32) -> Option<String> {
        let comm = std::fs::read(format!("/proc/{pid}/comm")).ok()?;
        let comm = comm.strip_suffix(b"\n").unwrap_or(&comm);
        (!comm.is_empty()).then(|| String::from_utf8_lossy(comm).into_owned())
    }

    /// `/proc/<pid>/exe`, `cmdline` and — when variables are asked —
    /// `environ`. `exe` and `environ` are readable only for the same user
    /// (or root), `cmdline` for anyone: with variables asked the record is
    /// `None` unless all three read — another user's process, as on macOS;
    /// without, the argv still reads and an unreadable executable is empty
    /// (the remote probe finds `sudo ssh host` by its argv, as it did before
    /// the record had an executable). An empty `cmdline` (a zombie, a kernel
    /// thread) is `None` — there is no argv to read.
    fn procargs(&self, pid: u32, keys: &[&str]) -> Option<ProcArgs> {
        let exec = match std::fs::read_link(format!("/proc/{pid}/exe")) {
            Ok(exec) => exec.to_string_lossy().into_owned(),
            Err(_) if keys.is_empty() => String::new(),
            Err(_) => return None,
        };
        let raw = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
        let raw = raw.strip_suffix(b"\0").unwrap_or(&raw);
        if raw.is_empty() {
            return None;
        }
        let args = raw
            .split(|&b| b == 0)
            .map(|arg| String::from_utf8_lossy(arg).into_owned())
            .collect();
        let env = if keys.is_empty() {
            Vec::new()
        } else {
            std::fs::read(format!("/proc/{pid}/environ"))
                .ok()?
                .split(|&b| b == 0)
                .filter_map(|entry| asked_var(entry, keys))
                .collect()
        };
        Some(ProcArgs { exec, args, env })
    }

    /// `/proc/<pid>/status`'s `Uid:` line — readable for every process.
    fn uid(&self, pid: u32) -> Option<u32> {
        parse_status_uid(&std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?)
    }
}

/// The effective user in a `/proc/<pid>/status` text: the `Uid:` line's
/// second field (real, **effective**, saved, file system).
#[cfg(any(target_os = "linux", test))]
fn parse_status_uid(status: &str) -> Option<u32> {
    status
        .lines()
        .find_map(|line| line.strip_prefix("Uid:"))?
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()
}

/// The three fields of `/proc/<pid>/stat` the table asks for.
#[cfg(any(target_os = "linux", test))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Stat {
    parent: u32,
    group: u32,
    /// `tpgid`; `0` when the process has no controlling terminal (`-1`).
    terminal_group: u32,
}

/// One process's `stat`, `None` if it vanished or cannot be read.
#[cfg(target_os = "linux")]
fn read_stat(pid: u32) -> Option<Stat> {
    parse_stat(&std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?)
}

/// The pids in `/proc` whose `stat` matches `keep`, in ascending order. A
/// process that exits mid-scan is skipped.
#[cfg(target_os = "linux")]
fn scan(keep: impl Fn(&Stat) -> bool) -> Vec<u32> {
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    let mut pids: Vec<u32> = entries
        .filter_map(|entry| entry.ok()?.file_name().to_str()?.parse::<u32>().ok())
        .filter(|&pid| read_stat(pid).is_some_and(|stat| keep(&stat)))
        .collect();
    pids.sort_unstable();
    pids
}

/// The pure half of [`read_stat`]. The layout is `pid (comm) state ppid pgrp
/// session tty_nr tpgid …`; `comm` may itself contain spaces and `)`, so the
/// fields are counted from the **last** `)`.
#[cfg(any(target_os = "linux", test))]
fn parse_stat(line: &str) -> Option<Stat> {
    let rest = &line[line.rfind(')')? + 1..];
    let mut fields = rest.split_whitespace();
    let _state = fields.next()?;
    let parent = fields.next()?.parse().ok()?;
    let group = fields.next()?.parse().ok()?;
    let _session = fields.next()?;
    let _tty = fields.next()?;
    let terminal_group: i64 = fields.next()?.parse().ok()?;
    Some(Stat {
        parent,
        group,
        terminal_group: u32::try_from(terminal_group).unwrap_or(0),
    })
}

// ─── terminals ───────────────────────────────────────────────────────────

/// The terminal a process is attached to — its controlling terminal — as a
/// number [`pty_device`] gives the same way for the pseudo-terminal it is on:
/// the two are equal exactly when the process is in that terminal. `None` for a
/// process with no terminal, or one that cannot be read (gone, another user's
/// on Linux).
///
/// This is how bateri finds out **which pane** a process is in, from the
/// kernel's record and not from anything the process says (an environment
/// variable, an argument): a program in another terminal cannot make itself
/// the pane's.
#[cfg(target_os = "macos")]
pub fn controlling_terminal(pid: u32) -> Option<u64> {
    // SAFETY: `proc_bsdinfo` is a plain C struct holding only integers and
    // `c_char` arrays; all zero bytes is a valid value of it.
    let info: libc::proc_bsdinfo = unsafe { pid_info(pid, libc::PROC_PIDTBSDINFO)? };
    // No terminal is `NODEV`, all ones (and zero is no device either).
    (info.e_tdev != 0 && info.e_tdev != u32::MAX).then_some(u64::from(info.e_tdev))
}

/// [`controlling_terminal`] on Linux: `tty_nr`, the fourth field after the
/// command of `/proc/<pid>/stat`.
#[cfg(target_os = "linux")]
pub fn controlling_terminal(pid: u32) -> Option<u64> {
    let line = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let rest = &line[line.rfind(')')? + 1..];
    let tty: u64 = rest.split_whitespace().nth(4)?.parse().ok()?;
    (tty != 0).then_some(tty)
}

/// The terminal device the pseudo-terminal whose master is `master` is, in
/// [`controlling_terminal`]'s number: the slave's device. `None` if `master`
/// is not a pseudo-terminal master.
pub fn pty_device(master: std::os::fd::BorrowedFd<'_>) -> Option<u64> {
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::MetadataExt;
    // `ptsname` answers in a buffer of its own: one caller at a time, and the
    // name copied before the next.
    static ONE: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let name = {
        let _one = ONE
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // SAFETY: `ptsname` reads the descriptor and returns null or a
        // NUL-terminated name that stays valid until the next call, copied here.
        let name = unsafe { libc::ptsname(master.as_raw_fd()) };
        if name.is_null() {
            return None;
        }
        // SAFETY: not null, NUL-terminated, copied while the lock is held.
        unsafe { std::ffi::CStr::from_ptr(name) }.to_owned()
    };
    let rdev = std::fs::metadata(std::ffi::OsStr::from_bytes(name.to_bytes()))
        .ok()?
        .rdev();
    Some(device_number(rdev))
}

/// A device number in the form [`controlling_terminal`] reports it.
#[cfg(target_os = "macos")]
fn device_number(rdev: u64) -> u64 {
    // `dev_t` is 32 bits wide here; `stat` widens it.
    u64::from(rdev as u32)
}

/// A device number in the form Linux's `/proc/<pid>/stat` reports it: the
/// major number in bits 8 to 15, the minor in 0 to 7 and 20 to 31.
#[cfg(target_os = "linux")]
fn device_number(rdev: u64) -> u64 {
    let (major, minor) = (libc::major(rdev) as u64, libc::minor(rdev) as u64);
    (major << 8) | (minor & 0xff) | ((minor & !0xff) << 12)
}

// ─── the terminal of a pane whose PTY this process does not hold ─────────

/// The path of terminal device `device` (in [`controlling_terminal`]'s number): macOS names a
/// device with `devname`, which answers in a buffer of its own — one caller at a time, the name
/// copied before the next. Checked against the file the path names: the number of a terminal
/// that has gone may be a name nobody has, or another terminal's.
#[cfg(target_os = "macos")]
fn terminal_path(device: u64) -> Option<std::path::PathBuf> {
    if device == 0 {
        return None;
    }
    static ONE: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let dev = libc::dev_t::from_ne_bytes(u32::try_from(device).ok()?.to_ne_bytes());
    let name = {
        let _one = ONE
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // SAFETY: `devname` returns null or a NUL-terminated name in a buffer of its own that
        // stays valid until the next call, copied here under the lock.
        let name = unsafe { libc::devname(dev, libc::S_IFCHR) };
        if name.is_null() {
            return None;
        }
        // SAFETY: not null, NUL-terminated, copied while the lock is held.
        unsafe { std::ffi::CStr::from_ptr(name) }.to_owned()
    };
    let path = std::path::Path::new("/dev").join(std::ffi::OsStr::from_bytes(name.to_bytes()));
    terminal_at(&path, device).then_some(path)
}

/// [`terminal_path`] on Linux: a pseudo-terminal is `/dev/pts/N`, and `tty_nr` says N as the
/// minor of a major in the Unix98 range.
#[cfg(target_os = "linux")]
fn terminal_path(device: u64) -> Option<std::path::PathBuf> {
    let major = (device >> 8) & 0xfff;
    let minor = (device & 0xff) | ((device >> 12) & 0xfff00);
    if !(136..=143).contains(&major) {
        return None;
    }
    let path = std::path::PathBuf::from(format!("/dev/pts/{}", (major - 136) * 256 + minor));
    terminal_at(&path, device).then_some(path)
}

/// Whether `path` is the character device `device`.
fn terminal_at(path: &std::path::Path, device: u64) -> bool {
    use std::os::unix::fs::{FileTypeExt, MetadataExt};
    std::fs::metadata(path)
        .is_ok_and(|meta| meta.file_type().is_char_device() && device_number(meta.rdev()) == device)
}

/// The two modes of terminal device `device` ([`tty_modes`]), read from the outside: the
/// device is opened **without becoming anyone's controlling terminal** and without waiting,
/// read, and closed again before this returns. It must not stay open: a kablo terminal's
/// end is seen when the last descriptor of its device goes, and a descriptor kept here would
/// keep a finished program's terminal alive.
fn terminal_modes(device: u64) -> Option<bt_core::TtyModes> {
    use std::os::fd::AsFd;
    use std::os::unix::fs::OpenOptionsExt;
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOCTTY | libc::O_NONBLOCK)
        .open(terminal_path(device)?)
        .ok()?;
    tty_modes(file.as_fd())
}

/// The process of a pane whose terminal questions are asked, and the terminal's device: the
/// PTY's child when the kernel lets it be read, else the shell below it. `login(1)` is root's
/// and its long info reads as nothing ([`ProcessTable::groups`]); a child that is the pane's
/// own program (`ssh-hold --first`) is the user's and answers.
fn terminal_process(
    parent: ShellParent,
    child: u32,
    table: &impl ProcessTable,
) -> Option<(u32, u64)> {
    [Some(child), shell_pid(parent, child, table)]
        .into_iter()
        .flatten()
        .find_map(|pid| controlling_terminal(pid).map(|device| (pid, device)))
}

/// The device of the terminal the pane whose PTY child is `child` is on. The question a
/// stream asks where a spawned pane asks its master ([`pty_device`]): both give the number
/// [`controlling_terminal`] gives for a process in the pane.
pub fn pane_terminal(parent: ShellParent, child: u32) -> Option<u64> {
    terminal_process(parent, child, &SystemTable).map(|(_, device)| device)
}

/// The terminal of the pane whose PTY child is `child`, read from the outside: its two modes
/// and its foreground group (the kernel's record of the pane's process, not `tcgetpgrp`,
/// which a process outside the terminal's session cannot ask). `None` while no process of
/// the pane has a terminal that can be read.
pub fn pane_tty(parent: ShellParent, child: u32) -> Option<bt_core::TtyReading> {
    let (pid, device) = terminal_process(parent, child, &SystemTable)?;
    Some(bt_core::TtyReading {
        modes: terminal_modes(device)?,
        foreground: SystemTable
            .groups(pid)
            .map(|groups| groups.foreground)
            .filter(|&group| group != 0),
    })
}

/// The device of the terminal `session`'s program is on: the master's slave where the
/// session holds the master, [`pane_terminal`] by way of the stream's ops where it does not.
/// The number a process in the pane gives ([`controlling_terminal`]).
pub fn session_device(session: &bt_core::Session) -> Option<u64> {
    if session.is_streamed() {
        session.terminal_device()
    } else {
        session.with_pty_fd(pty_device)
    }
}

/// [`bt_core::PtyOps`] of a stream whose program is a child of a kablo terminal: what the
/// stream's owner does for the size and the end, and the pane's terminal read from the
/// outside by the program's pid.
pub struct StreamTty {
    inner: std::sync::Arc<dyn bt_core::PtyOps>,
    parent: ShellParent,
}

impl StreamTty {
    /// `inner`'s ops, with the terminal read the way `parent` says the shell is found.
    pub fn new(inner: std::sync::Arc<dyn bt_core::PtyOps>, parent: ShellParent) -> StreamTty {
        StreamTty { inner, parent }
    }
}

impl bt_core::PtyOps for StreamTty {
    fn resize(&self, master: std::os::fd::BorrowedFd<'_>, size: bt_core::PtySize) {
        self.inner.resize(master, size);
    }

    fn hangup(&self, pid: u32, exit: std::os::fd::BorrowedFd<'_>) {
        self.inner.hangup(pid, exit);
    }

    fn tty(&self, pid: u32) -> Option<bt_core::TtyReading> {
        pane_tty(self.parent, pid)
    }

    fn terminal(&self, pid: u32) -> Option<u64> {
        pane_terminal(self.parent, pid)
    }
}

/// The uid and pid of the process at the other end of `stream`.
#[cfg(target_os = "macos")]
pub(crate) fn peer(stream: &std::os::unix::net::UnixStream) -> std::io::Result<(u32, u32)> {
    use std::os::fd::AsRawFd;
    // SAFETY: an all-zero `xucred` is valid; the kernel fills it.
    let mut cred: libc::xucred = unsafe { std::mem::zeroed() };
    let mut len = size_of::<libc::xucred>() as libc::socklen_t;
    // SAFETY: `cred` and `len` belong to this frame and `len` is its size.
    let status = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_LOCAL,
            libc::LOCAL_PEERCRED,
            (&raw mut cred).cast(),
            &raw mut len,
        )
    };
    if status != 0 {
        return Err(std::io::Error::last_os_error());
    }
    if cred.cr_version != libc::XUCRED_VERSION {
        return Err(std::io::Error::other("unknown xucred version"));
    }
    let mut pid: libc::pid_t = 0;
    let mut len = size_of::<libc::pid_t>() as libc::socklen_t;
    // SAFETY: as above.
    let status = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_LOCAL,
            libc::LOCAL_PEERPID,
            (&raw mut pid).cast(),
            &raw mut len,
        )
    };
    if status != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok((
        cred.cr_uid,
        u32::try_from(pid).map_err(std::io::Error::other)?,
    ))
}

/// The uid and pid of the process at the other end of `stream`.
#[cfg(target_os = "linux")]
pub(crate) fn peer(stream: &std::os::unix::net::UnixStream) -> std::io::Result<(u32, u32)> {
    use std::os::fd::AsRawFd;
    // SAFETY: an all-zero `ucred` is valid; the kernel fills it.
    let mut cred: libc::ucred = unsafe { std::mem::zeroed() };
    let mut len = size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: `cred` and `len` belong to this frame and `len` is its size.
    let status = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&raw mut cred).cast(),
            &raw mut len,
        )
    };
    if status != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok((
        cred.uid,
        u32::try_from(cred.pid).map_err(std::io::Error::other)?,
    ))
}

/// A process's start time, comparable only for equality: what
/// tells a live pid from a reused one. macOS: `p_starttime` in microseconds
/// (`sysctl(KERN_PROC_PID)`, readable on root-owned `login(1)` too, where
/// `PROC_PIDTBSDINFO` returns nothing — measured); Linux: `starttime` in
/// clock ticks since boot (`/proc/<pid>/stat`, field 22). `None` if the
/// process is gone or unreadable.
#[cfg(target_os = "macos")]
pub fn start_time(pid: u32) -> Option<u64> {
    let pid = c_int::try_from(pid).ok()?;
    // `struct kinfo_proc` (648 bytes on 64-bit macOS) is not in `libc`; its
    // first field is `kp_proc.p_un.__p_starttime`, a `timeval` (`i64`
    // seconds, `i32` microseconds). The buffer is generous and aligned.
    let mut buf = [0u64; 128];
    let mut len = size_of_val(&buf);
    let mut mib = [libc::CTL_KERN, libc::KERN_PROC, libc::KERN_PROC_PID, pid];
    // SAFETY: the buffer belongs to this frame and `len` is its size; the
    // kernel writes at most that many bytes and stores what it wrote in `len`.
    let status = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            4,
            buf.as_mut_ptr().cast(),
            &raw mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    // A vanished pid answers with zero bytes, not an error.
    if status != 0 || len < 16 {
        return None;
    }
    let seconds = u64::try_from(buf[0] as i64).ok()?;
    let micros = u64::from(buf[1] as u32);
    Some(seconds * 1_000_000 + micros)
}

/// A process's start time (see the macOS body).
#[cfg(target_os = "linux")]
pub fn start_time(pid: u32) -> Option<u64> {
    parse_start_time(&std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?)
}

/// The pure half of the Linux [`start_time`]: field 22, counted from the
/// last `)` like [`parse_stat`] (field 3 is the first after it).
#[cfg(any(target_os = "linux", test))]
fn parse_start_time(line: &str) -> Option<u64> {
    line[line.rfind(')')? + 1..]
        .split_whitespace()
        .nth(22 - 3)?
        .parse()
        .ok()
}

/// An fd that becomes **readable when `pid` exits** — the exit signal of a
/// process that is not our child:
/// macOS a `kqueue` of its own with only `EVFILT_PROC`/`NOTE_EXIT` on `pid`
/// (a user process may watch root-owned `login(1)` — measured), Linux a
/// pidfd. The reader loop registers it in the child-event pipe's place;
/// nobody drains it, so it stays readable.
///
/// `start` is [`start_time`] taken where the pid was known to be the right
/// process; it is asked **after** the registration, so a pid that died and
/// was reused before it gives `None`, never a watch on a stranger. `None`
/// too if the process is already gone or the call fails — the caller falls
/// back.
pub fn exit_fd(pid: u32, start: u64) -> Option<std::os::fd::OwnedFd> {
    let fd = watch_exit(pid)?;
    (start_time(pid)? == start).then_some(fd)
}

#[cfg(target_os = "macos")]
fn watch_exit(pid: u32) -> Option<std::os::fd::OwnedFd> {
    use std::os::fd::FromRawFd;
    // SAFETY: no arguments; the result is checked.
    let raw = unsafe { libc::kqueue() };
    if raw < 0 {
        return None;
    }
    // SAFETY: `raw` is a fresh descriptor nobody else owns. A kqueue is not
    // inherited across `fork` (kqueue(2)), so no `CLOEXEC` is needed.
    let queue = unsafe { std::os::fd::OwnedFd::from_raw_fd(raw) };
    // SAFETY: an all-zero `kevent` is a valid value of a plain C struct.
    let mut change: libc::kevent = unsafe { std::mem::zeroed() };
    change.ident = usize::try_from(pid).ok()?;
    change.filter = libc::EVFILT_PROC;
    change.flags = libc::EV_ADD;
    change.fflags = libc::NOTE_EXIT;
    // SAFETY: one change from this frame, no event buffer; `queue` is open.
    let status = unsafe {
        libc::kevent(
            raw,
            &raw const change,
            1,
            std::ptr::null_mut(),
            0,
            std::ptr::null(),
        )
    };
    (status == 0).then_some(queue)
}

#[cfg(target_os = "linux")]
fn watch_exit(pid: u32) -> Option<std::os::fd::OwnedFd> {
    use std::os::fd::FromRawFd;
    let pid = libc::pid_t::try_from(pid).ok()?;
    // SAFETY: `pidfd_open(pid, 0)`; the result is checked. The fd is
    // `CLOEXEC` by definition (pidfd_open(2)).
    let raw = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) };
    let raw = c_int_from(raw)?;
    // SAFETY: a fresh descriptor nobody else owns.
    Some(unsafe { std::os::fd::OwnedFd::from_raw_fd(raw) })
}

#[cfg(target_os = "linux")]
fn c_int_from(raw: libc::c_long) -> Option<libc::c_int> {
    libc::c_int::try_from(raw).ok().filter(|&fd| fd >= 0)
}

#[cfg(test)]
pub(crate) mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use bt_core::{
        CaretShape, CursorBlink, Osc52, Session, SessionOptions, TerminalOptions, Theme, TtyModes,
    };

    use super::*;
    use crate::child::{SilentWake, wait_until};

    /// Fake process: parent, group, name (`None` → the name is unreadable),
    /// its exec record's parts (`args: None` → the record is unreadable).
    struct Proc {
        parent: u32,
        group: u32,
        name: Option<&'static str>,
        args: Option<Vec<&'static str>>,
        /// The executable; argv[0] when `None`.
        exec: Option<&'static str>,
        env: Vec<(&'static str, &'static str)>,
        /// The effective user: [`USER`] unless [`Table::owned_by`] says.
        uid: u32,
    }

    /// The fake table's user: every process is theirs unless
    /// [`Table::owned_by`] gives it to another.
    pub(crate) const USER: u32 = 501;

    /// Fake table. `terminal` is the shell's `e_tpgid`; `None` → the shell's long
    /// info is unreadable.
    pub(crate) struct Table {
        procs: HashMap<u32, Proc>,
        terminal: Option<u32>,
        members_readable: bool,
        /// Another user's argv reads when no variable is asked — Linux's
        /// `cmdline`; macOS reads nothing of it ([`Self::reading_others_argv`]).
        others_argv: bool,
    }

    impl Table {
        pub(crate) fn new(terminal: Option<u32>) -> Self {
            Self {
                procs: HashMap::new(),
                terminal,
                members_readable: true,
                others_argv: false,
            }
        }

        pub(crate) fn with(
            mut self,
            pid: u32,
            parent: u32,
            group: u32,
            name: &'static str,
        ) -> Self {
            self.procs.insert(
                pid,
                Proc {
                    parent,
                    group,
                    name: Some(name),
                    args: None,
                    exec: None,
                    env: Vec::new(),
                    uid: USER,
                },
            );
            self
        }

        /// A process with an argv; argv[0] is the name itself.
        pub(crate) fn run(
            mut self,
            pid: u32,
            parent: u32,
            group: u32,
            argv: &[&'static str],
        ) -> Self {
            let name = argv[0].rsplit('/').next().expect("name");
            let name: &'static str = Box::leak(name.to_owned().into_boxed_str());
            self.procs.insert(
                pid,
                Proc {
                    parent,
                    group,
                    name: Some(name),
                    args: Some(argv.to_vec()),
                    exec: None,
                    env: Vec::new(),
                    uid: USER,
                },
            );
            self
        }

        /// A process whose name and executable are not its argv[0] (the
        /// framework Python, Ruby's rewritten title), with its environment.
        pub(crate) fn exec(
            mut self,
            pid: u32,
            parent: u32,
            group: u32,
            (name, exec): (&'static str, &'static str),
            argv: &[&'static str],
            env: &[(&'static str, &'static str)],
        ) -> Self {
            self.procs.insert(
                pid,
                Proc {
                    parent,
                    group,
                    name: Some(name),
                    args: Some(argv.to_vec()),
                    exec: Some(exec),
                    env: env.to_vec(),
                    uid: USER,
                },
            );
            self
        }

        /// `pid` runs as `uid`; a process of another user has no readable
        /// exec record (macOS's `KERN_PROCARGS2`; Linux's with variables).
        pub(crate) fn owned_by(mut self, pid: u32, uid: u32) -> Self {
            if let Some(proc) = self.procs.get_mut(&pid) {
                proc.uid = uid;
            }
            self
        }

        /// Linux's table: another user's argv reads when no variable is
        /// asked, its executable and environment do not.
        pub(crate) fn reading_others_argv(mut self) -> Self {
            self.others_argv = true;
            self
        }
    }

    impl ProcessTable for Table {
        fn children(&self, pid: u32) -> Vec<u32> {
            let mut kids: Vec<u32> = self
                .procs
                .iter()
                .filter(|(_, proc)| proc.parent == pid)
                .map(|(&kid, _)| kid)
                .collect();
            kids.sort_unstable();
            kids
        }

        fn groups(&self, shell: u32) -> Option<Groups> {
            Some(Groups {
                own: self.procs.get(&shell)?.group,
                foreground: self.terminal?,
            })
        }

        fn members(&self, group: u32) -> Vec<u32> {
            if !self.members_readable {
                return Vec::new();
            }
            // Order deliberately shuffled: the decision must establish pid order itself.
            let mut members: Vec<u32> = self
                .procs
                .iter()
                .filter(|(_, proc)| proc.group == group)
                .map(|(&pid, _)| pid)
                .collect();
            members.sort_unstable_by(|a, b| b.cmp(a));
            members
        }

        fn parent(&self, pid: u32) -> Option<u32> {
            self.procs.get(&pid).map(|proc| proc.parent)
        }

        fn name(&self, pid: u32) -> Option<String> {
            self.procs.get(&pid)?.name.map(str::to_owned)
        }

        fn procargs(&self, pid: u32, keys: &[&str]) -> Option<ProcArgs> {
            let proc = self.procs.get(&pid)?;
            let args = proc.args.as_ref()?;
            if proc.uid != USER {
                return (self.others_argv && keys.is_empty()).then(|| ProcArgs {
                    exec: String::new(),
                    args: args.iter().map(|&arg| arg.to_owned()).collect(),
                    env: Vec::new(),
                });
            }
            Some(ProcArgs {
                exec: proc.exec.unwrap_or(args[0]).to_owned(),
                args: args.iter().map(|&arg| arg.to_owned()).collect(),
                env: proc
                    .env
                    .iter()
                    .filter(|(name, _)| keys.contains(name))
                    .map(|&(name, value)| (name.to_owned(), value.to_owned()))
                    .collect(),
            })
        }

        fn uid(&self, pid: u32) -> Option<u32> {
            self.procs.get(&pid).map(|proc| proc.uid)
        }
    }

    fn running(names: &[&str]) -> Foreground {
        Foreground::Running(names.iter().map(|&name| name.to_owned()).collect())
    }

    /// `login` 100 → `zsh` 101 (group 101); the foreground group is the caller's.
    pub(crate) fn login_shell(terminal: u32) -> Table {
        Table::new(Some(terminal))
            .with(100, 1, 100, "login")
            .with(101, 100, 101, "zsh")
    }

    #[test]
    fn only_a_job_outside_the_shells_group_holds_the_terminal() {
        // The shell itself in the foreground: ZLE, `read -k`, an `exec`'d shell.
        assert!(!job_in_foreground(
            ShellParent::Login,
            100,
            &login_shell(101)
        ));
        let fish = Table::new(Some(101))
            .with(100, 1, 100, "login")
            .with(101, 100, 101, "fish");
        assert!(
            !job_in_foreground(ShellParent::Login, 100, &fish),
            "`exec fish` keeps the shell's group"
        );
        // A program in its own group.
        let python = login_shell(200).with(200, 101, 200, "python3");
        assert!(job_in_foreground(ShellParent::Login, 100, &python));
        assert!(job_in_foreground(ShellParent::Direct, 101, &python));
        // No answer: no shell yet, an unreadable group, no foreground.
        let unforked = Table::new(Some(200)).with(100, 1, 100, "login");
        assert!(!job_in_foreground(ShellParent::Login, 100, &unforked));
        let unreadable = Table::new(None)
            .with(100, 1, 100, "login")
            .with(101, 100, 101, "zsh");
        assert!(!job_in_foreground(ShellParent::Login, 100, &unreadable));
        assert!(!job_in_foreground(ShellParent::Login, 100, &login_shell(0)));
    }

    #[test]
    fn the_shells_group_is_the_shells_and_not_the_childs_where_login_forks_it() {
        // `login` 100 → `zsh` 101: the shell's group is 101, which the foreground group of
        // the terminal is when the prompt is up.
        assert_eq!(
            shell_group(ShellParent::Login, 100, &login_shell(101)),
            Some(101)
        );
        // The child's own pid is `login`'s group.
        assert_ne!(
            shell_group(ShellParent::Login, 100, &login_shell(101)),
            Some(100)
        );
        // Where the child is the shell.
        assert_eq!(
            shell_group(ShellParent::Direct, 101, &login_shell(101)),
            Some(101)
        );
        // No shell forked yet, or a group that cannot be read: not known.
        let unforked = Table::new(Some(200)).with(100, 1, 100, "login");
        assert_eq!(shell_group(ShellParent::Login, 100, &unforked), None);
        let unreadable = Table::new(None)
            .with(100, 1, 100, "login")
            .with(101, 100, 101, "zsh");
        assert_eq!(shell_group(ShellParent::Login, 100, &unreadable), None);
        // The terminal's foreground, with a program in front, is not the shell's group.
        let python = login_shell(200).with(200, 101, 200, "python3");
        assert_eq!(shell_group(ShellParent::Login, 100, &python), Some(101));
    }

    #[test]
    fn login_shell_in_the_foreground_is_idle() {
        assert_eq!(
            foreground(ShellParent::Login, 100, &login_shell(101)),
            Foreground::Idle
        );
    }

    #[test]
    fn a_program_in_the_foreground_is_running_by_name() {
        let table = login_shell(200).with(200, 101, 200, "vim");
        assert_eq!(
            foreground(ShellParent::Login, 100, &table),
            running(&["vim"])
        );
    }

    #[test]
    fn a_wrapper_leader_yields_the_name_of_its_leaf() {
        // The leader `bash` is a wrapper, the
        // program is its grandchild. The leader's name would be the wrong answer.
        let table = login_shell(300)
            .with(300, 101, 300, "bash")
            .with(301, 300, 300, "Orca")
            .with(302, 301, 300, "claude");
        assert_eq!(
            foreground(ShellParent::Login, 100, &table),
            running(&["claude"])
        );
    }

    #[test]
    fn a_pipeline_names_every_leaf_once_in_pid_order() {
        // `cat | grep a | grep b`: three leaves, two names.
        let table = login_shell(400)
            .with(400, 101, 400, "cat")
            .with(401, 101, 400, "grep")
            .with(402, 101, 400, "grep");
        assert_eq!(
            foreground(ShellParent::Login, 100, &table),
            running(&["cat", "grep"])
        );
    }

    #[test]
    fn a_login_without_a_shell_yet_is_idle() {
        // ⌘W right as the tab is born: `login` has not forked the shell yet.
        let table = Table::new(Some(100)).with(100, 1, 100, "login");
        assert_eq!(
            foreground(ShellParent::Login, 100, &table),
            Foreground::Idle
        );
    }

    #[test]
    fn an_unreadable_shell_counts_as_running_without_a_name() {
        let table = Table::new(None)
            .with(100, 1, 100, "login")
            .with(101, 100, 101, "zsh");
        assert_eq!(foreground(ShellParent::Login, 100, &table), running(&[]));
        assert_eq!(
            foreground(ShellParent::Login, 100, &login_shell(0)),
            running(&[])
        );
    }

    #[test]
    fn an_unreadable_group_falls_back_to_the_leader_then_to_no_name() {
        let mut table = login_shell(200).with(200, 101, 200, "vim");
        table.members_readable = false;
        assert_eq!(
            foreground(ShellParent::Login, 100, &table),
            running(&["vim"])
        );
        table.procs.get_mut(&200).expect("vim in the table").name = None;
        assert_eq!(foreground(ShellParent::Login, 100, &table), running(&[]));
    }

    #[test]
    fn a_direct_child_is_the_shell_itself() {
        // On the direct path `sleep`'s parent is the child itself: a flagless
        // rule like "idle if the foreground is the child's child" would count it
        // as idle (a rejected alternative).
        let idle = Table::new(Some(101)).with(101, 1, 101, "zsh");
        assert_eq!(
            foreground(ShellParent::Direct, 101, &idle),
            Foreground::Idle
        );
        let busy = Table::new(Some(200))
            .with(101, 1, 101, "zsh")
            .with(200, 101, 200, "sleep");
        assert_eq!(
            foreground(ShellParent::Direct, 101, &busy),
            running(&["sleep"])
        );
    }

    /// `login` 100 → `zsh` 101; foreground group 200 and its members with `argv`s.
    fn probe_of(procs: &[(u32, u32, &[&'static str])]) -> Probe {
        let mut table = login_shell(200);
        for &(pid, parent, argv) in procs {
            table = table.run(pid, parent, 200, argv);
        }
        remote(ShellParent::Login, 100, &table)
    }

    /// The variant of [`probe_of`] that looks **only at the host**: the host tests
    /// do not ask about the target's argv and kind (the argv tests use [`target_of`]).
    fn remote_of(procs: &[(u32, u32, &[&'static str])]) -> Probe {
        match probe_of(procs) {
            Probe::Remote(target) => remote_host(&target.host),
            other => other,
        }
    }

    fn remote_host(host: &str) -> Probe {
        Probe::Remote(Target {
            host: host.to_owned(),
            port: None,
            kind: RemoteKind::Ssh,
            argv: Vec::new(),
            nonce: None,
        })
    }

    /// The whole target of a single-process group; the test fails if not remote.
    fn target_of(argv: &[&'static str]) -> Target {
        match probe_of(&[(200, 101, argv)]) {
            Probe::Remote(target) => target,
            other => panic!("expected a remote target: {argv:?} → {other:?}"),
        }
    }

    fn words(argv: &[&str]) -> Vec<String> {
        argv.iter().map(|arg| (*arg).to_owned()).collect()
    }

    #[test]
    fn the_rerun_argv_drops_local_forwards_master_and_background() {
        // `-L -R -D` with their values, `-M` and `-f` are dropped;
        // everything else stays in order.
        let target = target_of(&["ssh", "-p", "2222", "-J", "jump", "-L", "8080:x:80", "prod"]);
        assert_eq!(target.host, "prod");
        assert_eq!(target.kind, RemoteKind::Ssh);
        assert_eq!(
            target.argv,
            words(&["ssh", "-p", "2222", "-J", "jump", "prod"])
        );
        // Combined clusters are split by the target walk: an attached value goes with
        // its flag, a cluster left with no flags is dropped entirely.
        assert_eq!(
            target_of(&[
                "ssh", "-vL", "1:x:1", "-p2222", "-MR9:y:9", "-D1080", "-4", "prod"
            ])
            .argv,
            words(&["ssh", "-v", "-p2222", "-4", "prod"])
        );
        assert_eq!(
            target_of(&["ssh", "-fM", "-o", "User=x", "--", "prod"]).argv,
            words(&["ssh", "-o", "User=x", "--", "prod"])
        );
        // Options after the target are filtered too (OpenSSH reads them again);
        // argv[0] as the process gives it.
        assert_eq!(
            target_of(&["/usr/bin/ssh", "prod", "-L", "1:x:1", "-v"]).argv,
            words(&["/usr/bin/ssh", "prod", "-v"])
        );
    }

    #[test]
    fn a_wrapped_ssh_reads_as_the_line_the_user_typed() {
        // The process runs bateri's `-t` and bootstrap; the target, the
        // re-run argv (⏎ reconnect, ⌘T) and the line are the user's.
        // With the session's connection sharing too: the job's route
        // must not see bateri's `ControlPath` as the user's own.
        let typed = ["-o", "User=x", "-L", "1:x:1", "--", "prod"];
        let control = crate::ssh_wrap::Control {
            socket: std::path::PathBuf::from("/tmp/bateri-501/0a1b2c3d/u-0123456789abcdef"),
        };
        for control in [None, Some(&control)] {
            let wrapped = crate::ssh_wrap::wrap(
                &words(&typed),
                "echo hi",
                Some(3),
                "0123456789abcdef",
                None,
                control,
            );
            let mut argv = vec!["ssh".to_owned()];
            argv.extend(wrapped);
            let leaked: Vec<&'static str> = argv
                .into_iter()
                .map(|arg| &*Box::leak(arg.into_boxed_str()))
                .collect();
            let target = target_of(&leaked);
            assert_eq!(target.host, "prod");
            assert_eq!(
                target.nonce.as_deref(),
                Some("0123456789abcdef"),
                "the nonce is read before unwrapping"
            );
            assert_eq!(target.argv, words(&["ssh", "-o", "User=x", "--", "prod"]));
            assert_eq!(
                crate::quote::command_line(&target.argv),
                "ssh -o User=x -- prod"
            );
        }
        // A call bateri did not wrap carries no nonce.
        assert_eq!(target_of(&["ssh", "prod"]).nonce, None);
    }

    #[test]
    fn a_forced_tty_command_is_kept_in_the_rerun_argv() {
        assert_eq!(
            target_of(&["ssh", "-t", "prod", "tmux", "attach"]).argv,
            words(&["ssh", "-t", "prod", "tmux", "attach"])
        );
        // `-f` stays non-interactive (the host-only answer did not change).
        assert_eq!(
            remote_of(&[(200, 101, &["ssh", "-fN", "prod"])]),
            Probe::Local
        );
    }

    #[test]
    fn mosh_reruns_as_mosh_with_the_script_arguments() {
        // If the script is visible, `mosh` + the script's arguments, as they are.
        let probe = probe_of(&[(
            200,
            101,
            &[
                "/usr/bin/perl5.34",
                "-w",
                "/opt/homebrew/bin/mosh",
                "--ssh=ssh -p 2",
                "prod",
            ],
        )]);
        let Probe::Remote(target) = probe else {
            panic!("mosh is remote: {probe:?}");
        };
        assert_eq!(target.kind, RemoteKind::Mosh);
        assert_eq!(target.host, "prod");
        assert_eq!(target.argv, words(&["mosh", "--ssh=ssh -p 2", "prod"]));
        // If only `mosh-client` is visible, the `-#` line, split on whitespace.
        let target = target_of(&[
            "mosh-client",
            "-# -p 60001 --ssh=ssh deploy@prod |",
            "10.0.0.5",
            "60001",
        ]);
        assert_eq!(target.kind, RemoteKind::Mosh);
        assert_eq!(target.host, "deploy@prod");
        assert_eq!(
            target.argv,
            words(&["mosh", "-p", "60001", "--ssh=ssh", "deploy@prod"])
        );
    }

    #[test]
    fn the_shell_in_the_foreground_is_undecided() {
        // `C` is printed before the fork: the shell is still in the foreground.
        assert_eq!(
            remote(ShellParent::Login, 100, &login_shell(101)),
            Probe::Undecided
        );
    }

    #[test]
    fn a_forked_shell_child_is_undecided() {
        // A forked child that has not `exec`ed yet carries the shell's name.
        let table = login_shell(200).with(200, 101, 200, "zsh");
        assert_eq!(remote(ShellParent::Login, 100, &table), Probe::Undecided);
    }

    #[test]
    fn an_interactive_ssh_is_remote_by_its_target() {
        assert_eq!(
            remote_of(&[(200, 101, &["ssh", "prod"])]),
            remote_host("prod")
        );
        assert_eq!(
            remote_of(&[(200, 101, &["ssh", "-p", "2222", "-v", "deploy@10.0.0.5"])]),
            remote_host("deploy@10.0.0.5")
        );
        assert_eq!(
            remote_of(&[(200, 101, &["ssh", "-p2222", "-4v", "prod"])]),
            remote_host("prod")
        );
        assert_eq!(
            remote_of(&[(200, 101, &["ssh", "-o", "User=x", "--", "prod"])]),
            remote_host("prod")
        );
    }

    #[test]
    fn a_jump_host_child_does_not_hide_the_target() {
        // `ssh -J jump prod` spawns `ssh -W prod:22 jump` in the same group;
        // the leaf rule would give the jump host.
        assert_eq!(
            remote_of(&[
                (200, 101, &["ssh", "-J", "jump", "prod"]),
                (201, 200, &["ssh", "-W", "[prod]:22", "jump"]),
            ]),
            remote_host("prod")
        );
    }

    #[test]
    fn a_pane_that_shows_a_kept_terminal_is_a_remote_session_to_the_server_it_names() {
        let target = target_of(&["bateri", "ssh-hold", "--", "-p", "2222", "deploy@prod"]);
        assert_eq!(target.host, "deploy@prod");
        assert_eq!(target.port, Some(2222));
        assert_eq!(target.kind, RemoteKind::Ssh);
        assert_eq!(target.argv, words(&["ssh", "-p", "2222", "deploy@prod"]));
        assert_eq!(target.nonce, None);
        // The path the binary was started by does not matter; its name does.
        assert_eq!(
            remote_of(&[(
                200,
                101,
                &[
                    "/Applications/bateri.app/Contents/MacOS/bateri",
                    "ssh-hold",
                    "--",
                    "prod"
                ]
            )]),
            remote_host("prod")
        );
        // The options of the holder itself come before the `--`.
        assert_eq!(
            remote_of(&[(200, 101, &["bateri", "ssh-hold", "--first", "--", "prod"])]),
            remote_host("prod")
        );
        // A holder with no arguments (it resumes a terminal) names no server here;
        // another subcommand, and another program, are nothing.
        assert_eq!(
            remote_of(&[(
                200,
                101,
                &[
                    "bateri",
                    "ssh-hold",
                    "--terminal",
                    "b-1",
                    "--account",
                    "u@h:22"
                ]
            )]),
            Probe::Local
        );
        assert_eq!(
            remote_of(&[(200, 101, &["bateri", "focus", "--", "prod"])]),
            Probe::Local
        );
        // A call that is not a session (the options say so) is not one here either.
        assert_eq!(
            remote_of(&[(200, 101, &["bateri", "ssh-hold", "--", "prod", "uptime"])]),
            Probe::Local
        );
    }

    #[test]
    fn a_pane_whose_own_program_is_the_pipe_to_a_kept_terminal_is_a_remote_session() {
        // `ssh-hold --first` is the pane's child: no shell above it, and it is not a member
        // of a foreground group the shell left. It is asked first, whatever the pane's
        // shell parent is.
        let held = |argv: &[&'static str]| {
            let table = Table::new(Some(100)).run(100, 1, 100, argv);
            (
                remote(ShellParent::Login, 100, &table),
                remote(ShellParent::Direct, 100, &table),
            )
        };
        let (login, direct) = held(&[
            "/Applications/bateri.app/Contents/MacOS/bateri",
            "ssh-hold",
            "--first",
            "--shell-arg",
            "--posix",
            "--",
            "-p",
            "2222",
            "deploy@prod",
        ]);
        for probe in [login, direct] {
            let Probe::Remote(target) = probe else {
                panic!("a first program is a remote session: {probe:?}");
            };
            assert_eq!(target.host, "deploy@prod");
            assert_eq!(target.port, Some(2222));
            assert_eq!(target.argv, words(&["ssh", "-p", "2222", "deploy@prod"]));
        }
        // A holder started to resume a terminal names its server by the arguments it was
        // given beside the terminal's place; one given none names none.
        let (login, _) = held(&[
            "bateri",
            "ssh-hold",
            "--first",
            "--terminal",
            "b-1",
            "--account",
            "u@prod:22",
            "--",
            "prod",
        ]);
        assert_eq!(login, remote_host_with_argv("prod", &["ssh", "prod"]));
        let (login, direct) = held(&[
            "bateri",
            "ssh-hold",
            "--first",
            "--terminal",
            "b-1",
            "--account",
            "a",
        ]);
        assert_eq!((login, direct), (Probe::Local, Probe::Local));
        // Any other program of the pane is the shell's business: a login shell's group is asked.
        let table = login_shell(200).run(200, 101, 200, &["ssh", "prod"]);
        assert_eq!(
            remote(ShellParent::Login, 100, &table),
            remote_host_with_argv("prod", &["ssh", "prod"])
        );
        let table = Table::new(Some(100)).run(100, 1, 100, &["bateri", "focus"]);
        assert_eq!(remote(ShellParent::Direct, 100, &table), Probe::Undecided);
    }

    fn remote_host_with_argv(host: &str, argv: &[&str]) -> Probe {
        Probe::Remote(Target {
            host: host.to_owned(),
            port: None,
            kind: RemoteKind::Ssh,
            argv: words(argv),
            nonce: None,
        })
    }

    #[test]
    fn a_remote_command_is_local_unless_forced_onto_a_tty() {
        assert_eq!(
            remote_of(&[(200, 101, &["ssh", "prod", "uptime"])]),
            Probe::Local
        );
        assert_eq!(
            remote_of(&[(200, 101, &["ssh", "-t", "prod", "tmux", "attach"])]),
            remote_host("prod")
        );
        assert_eq!(
            remote_of(&[(200, 101, &["ssh", "-N", "-L", "8080:localhost:80", "prod"])]),
            Probe::Local
        );
        assert_eq!(
            remote_of(&[(200, 101, &["ssh", "-T", "prod"])]),
            Probe::Local
        );
        assert_eq!(remote_of(&[(200, 101, &["ssh", "-V"])]), Probe::Local);
    }

    #[test]
    fn options_after_the_target_are_options() {
        // OpenSSH parses options again after the target.
        assert_eq!(
            remote_of(&[(200, 101, &["ssh", "prod", "-p", "2222", "-v"])]),
            remote_host("prod")
        );
        assert_eq!(
            remote_of(&[(200, 101, &["ssh", "prod", "-p", "2222", "uptime"])]),
            Probe::Local
        );
        assert_eq!(
            remote_of(&[(200, 101, &["ssh", "--", "prod", "-v"])]),
            Probe::Local
        );
    }

    #[test]
    fn config_options_decide_the_session_too() {
        assert_eq!(
            remote_of(&[(200, 101, &["ssh", "-o", "RequestTTY=force", "prod", "tmux"])]),
            remote_host("prod")
        );
        assert_eq!(
            remote_of(&[(
                200,
                101,
                &["ssh", "-oSessionType=none", "-L", "1:x:1", "prod"]
            )]),
            Probe::Local
        );
        assert_eq!(
            remote_of(&[(200, 101, &["ssh", "-o", "requesttty no", "prod"])]),
            Probe::Local
        );
    }

    #[test]
    fn an_interactive_ssh_wins_over_a_batch_one_in_a_pipeline() {
        assert_eq!(
            remote_of(&[
                (200, 101, &["ssh", "backup", "cat", "dump"]),
                (201, 101, &["ssh", "prod"]),
            ]),
            remote_host("prod")
        );
    }

    #[test]
    fn a_half_forked_pipeline_is_undecided() {
        // `ssh prod | tee log`: `tee` has `exec`ed, the ssh side not yet.
        let table = login_shell(200)
            .with(200, 101, 200, "zsh")
            .run(201, 101, 200, &["tee", "log"]);
        assert_eq!(remote(ShellParent::Login, 100, &table), Probe::Undecided);
    }

    #[test]
    fn an_ssh_uri_drops_the_scheme_and_the_port() {
        assert_eq!(
            remote_of(&[(200, 101, &["ssh", "ssh://deploy@h:2222"])]),
            remote_host("deploy@h")
        );
        assert_eq!(
            remote_of(&[(200, 101, &["ssh", "ssh://[::1]:22"])]),
            remote_host("::1")
        );
    }

    #[test]
    fn the_first_port_the_argv_names_wins() {
        let port_of = |argv: &[&'static str]| target_of(argv).port;
        // Every spelling OpenSSH takes, the option's key in any case.
        for argv in [
            &["ssh", "-p", "2222", "prod"][..],
            &["ssh", "-p2222", "prod"],
            &["ssh", "-vp2222", "prod"],
            &["ssh", "-o", "Port=2222", "prod"],
            &["ssh", "-o", "Port 2222", "prod"],
            &["ssh", "-oPort=2222", "prod"],
            &["ssh", "-o", "port = 2222", "prod"],
            &["ssh", "-v", "-o", "PORT=2222", "prod"],
            // Options after the target are read again.
            &["ssh", "prod", "-p", "2222"],
            &["ssh", "-p", "2222", "--", "prod"],
            &["ssh", "ssh://prod:2222"],
            &["ssh", "ssh://deploy@prod:2222/"],
        ] {
            assert_eq!(port_of(argv), Some(2222), "{argv:?}");
        }
        // The first one wins, whichever way it was given: an option before the
        // target beats the URI's, the URI's beats an option after the target.
        assert_eq!(
            port_of(&["ssh", "-p", "2222", "-p", "2223", "prod"]),
            Some(2222)
        );
        assert_eq!(
            port_of(&["ssh", "-o", "Port=2223", "-p", "2222", "prod"]),
            Some(2223)
        );
        assert_eq!(port_of(&["ssh", "-p", "22", "ssh://prod:2222"]), Some(22));
        assert_eq!(
            port_of(&["ssh", "ssh://prod:2222", "-p", "2223"]),
            Some(2222)
        );
        assert_eq!(port_of(&["ssh", "ssh://prod", "-p", "2223"]), Some(2223));
        // What is not a port is not the user's port, and does not hide a later one.
        assert_eq!(
            port_of(&["ssh", "-p", "0", "-p", "2223", "prod"]),
            Some(2223)
        );
        assert_eq!(port_of(&["ssh", "-p", "70000", "prod"]), None);
        assert_eq!(port_of(&["ssh", "-p", "ssh", "prod"]), None);
        assert_eq!(port_of(&["ssh", "ssh://prod:x"]), None);
        // Nothing named, nothing known; the key does not change.
        assert_eq!(port_of(&["ssh", "prod"]), None);
        assert_eq!(port_of(&["ssh", "-o", "User=x", "ssh://prod"]), None);
        let uri = target_of(&["ssh", "ssh://deploy@prod:2222"]);
        assert_eq!((uri.host.as_str(), uri.port), ("deploy@prod", Some(2222)));
        let v6 = target_of(&["ssh", "ssh://[::1]:2222"]);
        assert_eq!((v6.host.as_str(), v6.port), ("::1", Some(2222)));
        // The re-run argv keeps the user's own option, as it did.
        assert_eq!(
            target_of(&["ssh", "-vp2222", "prod"]).argv,
            words(&["ssh", "-vp2222", "prod"])
        );
    }

    #[test]
    fn mosh_has_no_port_to_show() {
        // mosh's `-p` is the UDP port of the mosh server, not ssh's door.
        let script = ["/usr/bin/perl5.34", "-w", "/opt/homebrew/bin/mosh"];
        let argv: Vec<&'static str> = script.into_iter().chain(["-p", "60001", "prod"]).collect();
        let target = target_of(&argv);
        assert_eq!((target.kind, target.port), (RemoteKind::Mosh, None));
    }

    #[test]
    fn an_option_is_split_the_way_ssh_config_spells_it() {
        for (option, want) in [
            ("RequestTTY=no", ("RequestTTY", "no")),
            ("RequestTTY no", ("RequestTTY", "no")),
            ("RequestTTY = no", ("RequestTTY", "no")),
            ("  Port\t2222 ", ("Port", "2222")),
            ("Port", ("Port", "")),
        ] {
            assert_eq!(config_pair(option), want, "{option:?}");
        }
        // The spaced form was missed before: it now reads as the keyword it is.
        assert!(ssh_call("ssh", &words(&["-o", "RequestTTY = no", "prod"])).is_none());
    }

    #[test]
    fn mosh_is_found_above_its_bootstrap_ssh() {
        // The Perl script; the bootstrap ssh is its child and carries a remote command.
        assert_eq!(
            remote_of(&[
                (
                    200,
                    101,
                    &[
                        "/usr/bin/perl5.34",
                        "-w",
                        "/opt/homebrew/bin/mosh",
                        "--ssh=ssh -p 2",
                        "prod"
                    ]
                ),
                (
                    201,
                    200,
                    &[
                        "ssh",
                        "-n",
                        "-tt",
                        "-S",
                        "none",
                        "prod",
                        "--",
                        "mosh-server",
                        "new"
                    ]
                ),
            ]),
            remote_host("prod")
        );
    }

    #[test]
    fn mosh_client_names_the_target_from_its_command_line() {
        assert_eq!(
            remote_of(&[(200, 101, &["mosh-client", "-# prod |", "10.0.0.5", "60001"])]),
            remote_host("prod")
        );
        assert_eq!(
            remote_of(&[(
                200,
                101,
                &[
                    "mosh-client",
                    "-# -p 60001 --ssh=ssh deploy@prod |",
                    "10.0.0.5",
                    "60001"
                ]
            )]),
            remote_host("deploy@prod")
        );
        // The quoted `--ssh` value was joined without quotes.
        assert_eq!(
            remote_of(&[(
                200,
                101,
                &[
                    "mosh-client",
                    "-# --ssh=ssh -i ~/.ssh/k -o Port=2 prod |",
                    "10.0.0.5",
                    "1"
                ]
            )]),
            remote_host("prod")
        );
    }

    #[test]
    fn other_programs_and_unreadable_groups_are_local() {
        assert_eq!(remote_of(&[(200, 101, &["cat"])]), Probe::Local);
        // Perl, but not mosh.
        assert_eq!(
            remote_of(&[(200, 101, &["perl", "script.pl", "prod"])]),
            Probe::Local
        );
        // The group is unreadable.
        let mut table = login_shell(200).run(200, 101, 200, &["ssh", "prod"]);
        table.members_readable = false;
        assert_eq!(remote(ShellParent::Login, 100, &table), Probe::Local);
        // The shell itself is unreadable.
        let table = Table::new(None)
            .with(100, 1, 100, "login")
            .with(101, 100, 101, "zsh");
        assert_eq!(remote(ShellParent::Login, 100, &table), Probe::Local);
        // An ssh whose argv is unreadable is not recognized.
        let table = login_shell(200).with(200, 101, 200, "ssh");
        assert_eq!(remote(ShellParent::Login, 100, &table), Probe::Local);
    }

    #[test]
    fn a_records_debug_print_shows_no_argument() {
        // A command line carries passwords; a `Debug` print ends in a log.
        let record = ProcArgs {
            exec: "/usr/bin/mysql".into(),
            args: vec!["mysql".into(), "-phunter2".into()],
            env: vec![("MYSQL_HOST".into(), "db".into())],
        };
        let printed = format!("{record:?}");
        assert!(!printed.contains("hunter2"), "an argument was printed");
        assert!(printed.contains("argc: 2") && printed.contains("MYSQL_HOST"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn procargs_layout_yields_the_exec_path_argv_and_only_the_asked_variables() {
        let mut buf = 2i32.to_ne_bytes().to_vec();
        buf.extend_from_slice(
            b"/opt/x/Python\0\0\0\0python3\0-q\0SECRET_TOKEN=hunter2\0HOME=/x\0\
              VIRTUAL_ENV=/p/.venv\0\0\0ptr_munge=VIRTUAL_ENV=/apple\0",
        );
        let asked = ["VIRTUAL_ENV", "HOME"];
        let record = parse_procargs(&buf, &asked).expect("record");
        assert_eq!(record.exec, "/opt/x/Python");
        assert_eq!(record.args, ["python3", "-q"]);
        // Only the asked ones, in the environment's order; the `apple[]`
        // strings after the empty entry are not the environment.
        assert_eq!(
            record.env,
            [
                ("HOME".to_owned(), "/x".to_owned()),
                ("VIRTUAL_ENV".to_owned(), "/p/.venv".to_owned()),
            ]
        );
        assert_eq!(record.var("VIRTUAL_ENV"), Some("/p/.venv"));
        // An unasked variable is in no field and no `Debug` print.
        assert!(!format!("{record:?}").contains("hunter2"));
        assert!(!format!("{record:?}").contains("SECRET_TOKEN"));
        // Nothing asked, nothing kept.
        assert_eq!(parse_procargs(&buf, &[]).expect("record").env, []);
        assert_eq!(parse_procargs(&buf[..3], &asked), None);
    }

    #[test]
    fn an_asked_variable_is_matched_by_its_whole_name() {
        assert_eq!(
            asked_var(b"NVM_DIR=/u/.nvm", &["NVM_DIR"]),
            Some(("NVM_DIR".to_owned(), "/u/.nvm".to_owned()))
        );
        assert_eq!(asked_var(b"NVM_DIRX=/u", &["NVM_DIR"]), None);
        assert_eq!(asked_var(b"NVM=/u", &["NVM_DIR"]), None);
        assert_eq!(asked_var(b"NVM_DIR", &["NVM_DIR"]), None, "no `=`");
        assert_eq!(
            asked_var(b"K==v", &["K"]),
            Some(("K".to_owned(), "=v".to_owned()))
        );
    }

    #[test]
    fn stat_fields_are_counted_from_the_last_parenthesis() {
        // A `comm` with a space and a `)` must not shift the fields.
        let line = "4242 (tmux: se) rv) S 100 4242 4242 34816 5000 4194304 0 0";
        assert_eq!(
            parse_stat(line),
            Some(Stat {
                parent: 100,
                group: 4242,
                terminal_group: 5000,
            })
        );
        // No controlling terminal: `tpgid` is -1, read as "no foreground".
        let line = "7 (kworker/0:1) I 2 0 0 0 -1 69238880 0";
        assert_eq!(parse_stat(line).map(|stat| stat.terminal_group), Some(0));
        assert_eq!(parse_stat("7 (truncated"), None);
        assert_eq!(parse_stat("7 (x) S 1"), None);
    }

    /// The variable that turns [`sleeper_process`] into a sleeping child.
    const SLEEPER_ENV: &str = "BT_JOBS_SLEEPER";

    /// Not a test of its own: the child
    /// [`the_process_table_reads_a_real_exec_record`] spawns (this binary
    /// again, run with `--exact` and [`SLEEPER_ENV`]). A process of the same
    /// user that is not a platform binary — macOS does not show a platform
    /// binary's environment (measured: `/bin/sleep`'s is empty). Without the
    /// variable it does nothing.
    #[test]
    fn sleeper_process() {
        if std::env::var_os(SLEEPER_ENV).is_some() {
            std::thread::sleep(std::time::Duration::from_secs(30));
        }
    }

    #[test]
    fn the_process_table_reads_a_real_exec_record() {
        // The witness for the record's body (`KERN_PROCARGS2` on macOS,
        // `/proc/<pid>/{exe,cmdline,environ}` on Linux): a child of the same
        // user with a known argv and environment.
        let tail = ["--exact", "jobs::tests::sleeper_process", "-q"];
        let mut child = std::process::Command::new(std::env::current_exe().expect("test binary"))
            .args(tail)
            .env(SLEEPER_ENV, "1")
            .env("BT_PROBE_ASKED", "kept")
            .env("BT_PROBE_SECRET", "never-read")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("the sleeper did not spawn");
        let keys = ["BT_PROBE_ASKED"];
        // `spawn` returns once the child forked, not once it exec'd: before
        // the exec the argv is still the test runner's (seen on Linux).
        let execd = |record: &ProcArgs| record.args.get(1..) == Some(&tail.map(str::to_owned)[..]);
        wait_until("the child's argv not seen", || {
            SystemTable
                .procargs(child.id(), &keys)
                .is_some_and(|record| execd(&record))
        });
        let record = SystemTable.procargs(child.id(), &keys).expect("record");
        let _ = child.kill();
        let _ = child.wait();
        let exec = std::path::Path::new(&record.exec);
        assert!(exec.is_absolute(), "{}", record.exec);
        assert_eq!(
            exec.file_name(),
            std::env::current_exe().expect("test binary").file_name()
        );
        assert_eq!(
            record.env,
            [("BT_PROBE_ASKED".to_owned(), "kept".to_owned())]
        );
    }

    #[test]
    fn a_relative_exec_path_is_made_absolute() {
        // `./node` is kept relative by macOS's kernel and names a file
        // relative to a directory the reader does not know: the record's
        // path is the executable's own. Spawned through `../` up to `/` and
        // down again, so the path is relative whatever the cwd.
        let exe = std::env::current_exe().expect("test binary");
        let cwd = std::env::current_dir().expect("cwd");
        let ups = cwd.components().count().saturating_sub(1);
        let relative: std::path::PathBuf =
            std::iter::repeat_n(std::path::Component::ParentDir, ups)
                .map(|up| up.as_os_str().to_owned())
                .chain(
                    exe.components()
                        .skip(1)
                        .map(|part| part.as_os_str().to_owned()),
                )
                .collect();
        assert!(relative.is_relative(), "{}", relative.display());
        let tail = ["--exact", "jobs::tests::sleeper_process", "-q"];
        let mut child = std::process::Command::new(&relative)
            .args(tail)
            .env(SLEEPER_ENV, "1")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("the sleeper did not spawn");
        let execd = |record: &ProcArgs| record.args.get(1..) == Some(&tail.map(str::to_owned)[..]);
        wait_until("the child's argv not seen", || {
            SystemTable
                .procargs(child.id(), &[])
                .is_some_and(|record| execd(&record))
        });
        let record = SystemTable.procargs(child.id(), &[]).expect("record");
        let _ = child.kill();
        let _ = child.wait();
        let exec = std::path::Path::new(&record.exec);
        assert!(exec.is_absolute(), "{}", record.exec);
        assert_eq!(exec.file_name(), exe.file_name());
    }

    #[test]
    fn the_uid_is_the_effective_user_of_any_process() {
        // Our own process: the effective user.
        // SAFETY: no arguments, no failure.
        let euid = unsafe { libc::geteuid() };
        assert_eq!(SystemTable.uid(std::process::id()), Some(euid));
        // Root's processes read too — the root shell a guide bar names has
        // no readable exec record, its user is all there is. pid 1 is
        // launchd on macOS; in the Linux gate's container it is root's too.
        #[cfg(target_os = "macos")]
        assert_eq!(SystemTable.uid(1), Some(0));
    }

    #[test]
    fn the_status_uid_is_the_second_field() {
        // `sudo` at its prompt: real 1000, effective 0.
        let status =
            "Name:\tsudo\nUmask:\t0022\nUid:\t1000\t0\t0\t0\nGid:\t1000\t1000\t1000\t1000\n";
        assert_eq!(parse_status_uid(status), Some(0));
        assert_eq!(parse_status_uid("Name:\tx\n"), None);
        assert_eq!(parse_status_uid("Uid:\t1000\n"), None);
    }

    #[test]
    fn the_start_time_is_field_twenty_two() {
        let line = "42 (a) b) S 1 42 42 0 -1 4194560 100 0 0 0 1 2 0 0 20 0 1 0 987654 1 2";
        assert_eq!(parse_start_time(line), Some(987_654));
        assert_eq!(parse_start_time("42 (a) S 1 42"), None);
    }

    /// Whether `fd` is readable within `wait`.
    fn readable(fd: std::os::fd::BorrowedFd<'_>, wait: std::time::Duration) -> bool {
        use std::os::fd::AsRawFd;
        let mut poll = libc::pollfd {
            fd: fd.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        let millis = c_int_ms(wait);
        // SAFETY: one `pollfd` from this frame.
        unsafe { libc::poll(&raw mut poll, 1, millis) > 0 }
    }

    fn c_int_ms(wait: std::time::Duration) -> libc::c_int {
        libc::c_int::try_from(wait.as_millis()).unwrap_or(libc::c_int::MAX)
    }

    #[test]
    fn the_exit_fd_reads_when_a_process_exits_and_refuses_a_stranger() {
        use std::os::fd::AsFd;
        use std::time::Duration;
        let mut child = std::process::Command::new("/bin/sleep")
            .arg("30")
            .spawn()
            .expect("sleep did not spawn");
        let pid = child.id();
        let start = start_time(pid).expect("the start time not read");
        // Another start time is another process that once had this pid.
        assert!(exit_fd(pid, start + 1).is_none());
        let fd = exit_fd(pid, start).expect("no exit fd");
        assert!(
            !readable(fd.as_fd(), Duration::ZERO),
            "readable while alive"
        );
        let _ = child.kill();
        let _ = child.wait();
        assert!(
            readable(fd.as_fd(), Duration::from_secs(5)),
            "not readable after the exit"
        );
        // Gone: nothing to watch.
        assert!(exit_fd(pid, start).is_none());
    }

    #[test]
    fn a_process_is_in_a_pane_when_its_controlling_terminal_is_the_panes_pty() {
        let session = stty_session("-echo");
        let child = session.child_pid();
        let pane = session.with_pty_fd(pty_device).expect("the pty's device");
        wait_until("the shell has its terminal", || {
            controlling_terminal(child).is_some()
        });
        assert_eq!(controlling_terminal(child), Some(pane));
        // A second pane's PTY is another device, and a process that is in
        // neither (this test, run with no terminal or with another) is not in this one.
        let other = stty_session("-echo");
        let elsewhere = other
            .with_pty_fd(pty_device)
            .expect("the other pty's device");
        assert_ne!(pane, elsewhere);
        assert_ne!(controlling_terminal(other.child_pid()), Some(pane));
        assert_ne!(controlling_terminal(std::process::id()), Some(pane));
        // A pid that is not there has no terminal.
        assert_eq!(controlling_terminal(u32::MAX - 1), None);
        // Nor does a descriptor that is not a pseudo-terminal master have a device.
        let file = std::fs::File::open("/dev/null").expect("null");
        assert_eq!(pty_device(std::os::fd::AsFd::as_fd(&file)), None);
        session.shutdown();
        other.shutdown();
    }

    /// A real PTY whose program sets `stty`'s modes and then sleeps.
    fn stty_session(modes: &str) -> Session {
        Session::spawn(
            SessionOptions {
                command: Some((
                    "/bin/sh".to_owned(),
                    vec!["-c".to_owned(), format!("stty {modes}; sleep 30")],
                )),
                working_directory: None,
                home: None,
                env: HashMap::new(),
                cols: 40,
                rows: 10,
                cell_px: (9, 18),
                terminal: TerminalOptions {
                    scrollback: 100,
                    osc52: Osc52::Off,
                    cursor: CaretShape::default(),
                    blink: CursorBlink::default(),
                },
                theme: Theme::BATERI,
                dock: false,
                cluster: false,
                initial_input: None,
                shell_marks: false,
                pane_uuid: None,
                hostname: None,
                replay: None,
                views: None,
            },
            Arc::new(SilentWake),
        )
        .expect("session did not open")
    }

    /// The master's `tcgetattr` gives the program's modes — on
    /// macOS and on Linux (`make linux`): a password prompt's (canonical, no
    /// echo) and a logged-in session's (neither).
    #[test]
    fn the_master_reads_the_programs_terminal_modes() {
        for (stty, expected) in [
            (
                "icanon -echo",
                TtyModes {
                    canonical: true,
                    echo: false,
                },
            ),
            (
                "-icanon -echo",
                TtyModes {
                    canonical: false,
                    echo: false,
                },
            ),
        ] {
            let session = stty_session(stty);
            let modes = || session.with_pty_fd(tty_modes);
            wait_until(&format!("`stty {stty}` not seen"), || {
                modes() == Some(expected)
            });
            assert_eq!(modes().map(TtyModes::logged_in), Some(!expected.canonical));
            session.shutdown();
        }
    }

    /// A pseudo-terminal pair of the test's own: the master, the slave opened through the path
    /// [`terminal_path`] makes of the device, and that device.
    fn pty_pair() -> (std::os::fd::OwnedFd, std::os::fd::OwnedFd, u64) {
        use std::os::fd::{AsFd, FromRawFd, OwnedFd};
        use std::os::unix::fs::OpenOptionsExt;
        // SAFETY: `posix_openpt` has no preconditions; the result is checked and owned at once.
        let master = unsafe { libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY) };
        assert!(
            master >= 0,
            "no pseudo-terminal: {}",
            std::io::Error::last_os_error()
        );
        // SAFETY: a descriptor just made, owned by nobody else.
        let master = unsafe { OwnedFd::from_raw_fd(master) };
        // SAFETY: the master is open for both calls.
        unsafe {
            assert_eq!(libc::grantpt(std::os::fd::AsRawFd::as_raw_fd(&master)), 0);
            assert_eq!(libc::unlockpt(std::os::fd::AsRawFd::as_raw_fd(&master)), 0);
        }
        let device = pty_device(master.as_fd()).expect("the master's slave has a device");
        let slave = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NOCTTY)
            .open(terminal_path(device).expect("the device has a path"))
            .expect("the slave");
        (master, slave.into(), device)
    }

    /// Sets the line discipline of the terminal `fd` is on to `canonical` and `echo`.
    fn set_modes(fd: &std::os::fd::OwnedFd, canonical: bool, echo: bool) {
        use std::os::fd::AsRawFd;
        // SAFETY: an all-zero `termios` is a valid value; the descriptor is open for the calls.
        unsafe {
            let mut modes: libc::termios = std::mem::zeroed();
            assert_eq!(libc::tcgetattr(fd.as_raw_fd(), &mut modes), 0);
            for (flag, on) in [(libc::ICANON, canonical), (libc::ECHO, echo)] {
                if on {
                    modes.c_lflag |= flag;
                } else {
                    modes.c_lflag &= !flag;
                }
            }
            assert_eq!(libc::tcsetattr(fd.as_raw_fd(), libc::TCSANOW, &modes), 0);
        }
    }

    /// Reads one byte of `master` without waiting: `None` when it would wait (something still
    /// holds the slave), `Some(read result)` when the answer is the end.
    fn master_says_end(master: &std::os::fd::OwnedFd) -> bool {
        use std::os::fd::AsRawFd;
        // SAFETY: `fcntl` and `read` on an open descriptor and a buffer of this frame.
        unsafe {
            let flags = libc::fcntl(master.as_raw_fd(), libc::F_GETFL);
            libc::fcntl(master.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK);
            let mut byte = 0u8;
            let got = libc::read(master.as_raw_fd(), (&raw mut byte).cast(), 1);
            if got == 0 {
                return true;
            }
            // The end of a terminal is a hang-up error on Linux, an empty read on macOS; a
            // read that would wait says someone still has the other side open.
            got < 0 && std::io::Error::last_os_error().kind() != std::io::ErrorKind::WouldBlock
        }
    }

    /// A terminal read from the outside gives its modes as they are set, and holds nothing
    /// afterwards: the end of the terminal is seen by whoever has the master as soon as the
    /// program's own descriptors are gone, which is the whole of what a kablo terminal's
    /// finishing depends on. The control is a descriptor kept open on purpose, which must read
    /// as "not ended" — without it the end proves nothing.
    #[test]
    fn a_terminal_read_from_its_device_is_not_kept_open() {
        let (master, slave, device) = pty_pair();
        set_modes(&slave, false, false);
        assert_eq!(
            terminal_modes(device),
            Some(TtyModes {
                canonical: false,
                echo: false
            })
        );
        set_modes(&slave, true, true);
        assert_eq!(
            terminal_modes(device),
            Some(TtyModes {
                canonical: true,
                echo: true
            })
        );
        // Read again and again, it leaves nothing behind either.
        for _ in 0..8 {
            assert!(terminal_modes(device).is_some());
        }
        // The control: a second descriptor of the device is open, and the master waits.
        let kept = std::fs::OpenOptions::new()
            .read(true)
            .open(terminal_path(device).expect("path"))
            .expect("a second slave");
        drop(slave);
        assert!(
            !master_says_end(&master),
            "the control did not hold the terminal open"
        );
        drop(kept);
        assert!(
            master_says_end(&master),
            "a descriptor of the device was left open"
        );
    }

    /// A device that is not there, or is not a terminal, has no path and no modes.
    #[test]
    fn a_device_that_is_no_terminal_has_no_path() {
        // Zero is "no terminal" ([`controlling_terminal`]), and on macOS the name of the console.
        assert_eq!(terminal_modes(0), None);
        assert_eq!(terminal_path(0), None);
        assert_eq!(terminal_path(u64::MAX), None);
    }

    /// The pane's terminal read by its child's pid — what a stream without a master has — is
    /// the one the master gives: the device the gate compares, the modes the remote login and
    /// the raw check read, and the foreground group the view protocol compares with the shell's.
    #[test]
    fn a_panes_terminal_by_the_childs_pid_is_the_masters() {
        let session = stty_session("-icanon -echo");
        let pid = session.child_pid();
        let read = || pane_tty(ShellParent::Direct, pid);
        wait_until("`stty` not seen from the outside", || {
            read().map(|tty| tty.modes)
                == Some(TtyModes {
                    canonical: false,
                    echo: false,
                })
        });
        assert_eq!(
            pane_terminal(ShellParent::Direct, pid),
            session.with_pty_fd(pty_device),
            "the device a process in the pane is found by"
        );
        assert_eq!(
            read().and_then(|tty| tty.foreground),
            SystemTable.groups(pid).map(|groups| groups.own),
            "the shell is in front: its group is the terminal's foreground"
        );
        // A pid that is nothing has no terminal.
        assert_eq!(pane_terminal(ShellParent::Direct, u32::MAX - 1), None);
        assert_eq!(pane_tty(ShellParent::Direct, u32::MAX - 1), None);
        session.shutdown();
    }

    #[test]
    fn the_process_table_sees_a_real_foreground_job() {
        // The reader's only witness: a fake table can see neither that login is
        // root nor which process `e_tpgid` is read from. If `e_tpgid` comes back
        // zero in an environment without a controlling terminal, the test is not
        // skipped, it fails — in that case the reader is wrong.
        let session = Session::spawn(
            SessionOptions {
                command: Some((
                    "/bin/zsh".to_owned(),
                    vec!["-f".to_owned(), "-i".to_owned()],
                )),
                working_directory: None,
                home: None,
                env: HashMap::new(),
                cols: 40,
                rows: 10,
                cell_px: (9, 18),
                terminal: TerminalOptions {
                    scrollback: 100,
                    osc52: Osc52::Off,
                    cursor: CaretShape::default(),
                    blink: CursorBlink::default(),
                },
                theme: Theme::BATERI,
                dock: false,
                cluster: false,
                initial_input: None,
                shell_marks: false,
                pane_uuid: None,
                hostname: None,
                replay: None,
                views: None,
            },
            Arc::new(SilentWake),
        )
        .expect("session did not open");
        let child = session.child_pid();
        let now = || foreground(ShellParent::Direct, child, &SystemTable);

        wait_until("shell not seen in foreground", || now() == Foreground::Idle);
        // Longer than the test's deadline: the job must not end while the claim is read.
        session.write(b"sleep 30\n");
        wait_until("foreground `sleep` not seen", || {
            now() == running(&["sleep"])
        });
        // Ctrl-C kills the job and the foreground returns to the shell: no process is left.
        session.write(b"\x03");
        wait_until("shell did not regain foreground", || {
            now() == Foreground::Idle
        });
        session.shutdown();
        assert!(!session.reader_alive(), "session did not close");
    }
}
