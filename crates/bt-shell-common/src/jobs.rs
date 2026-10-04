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
    pub host: String,
    pub kind: RemoteKind,
    /// The argv to re-run ([`ssh_target`], [`mosh_argv`]).
    pub argv: Vec<String>,
    /// The nonce of a call bateri wrapped ([`ssh_wrap::nonce`]):
    /// taken from the process's argv **before** it is unwrapped, since
    /// [`Self::argv`] is the user's. The pane matches it against the
    /// bootstrap's `up`. `None` for a call bateri did not wrap and for mosh.
    pub nonce: Option<String>,
}

/// The six things the decision asks of the process table.
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
    /// The process's argv; `None` if unreadable. Asked only of members with
    /// candidate names ([`remote`]).
    fn args(&self, pid: u32) -> Option<Vec<String>>;
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
fn shell_pid(parent: ShellParent, child: u32, table: &impl ProcessTable) -> Option<u32> {
    match parent {
        ShellParent::Direct => Some(child),
        ShellParent::Login => table.children(child).first().copied(),
    }
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
            let args = table.args(pid)?;
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

/// A name whose argv is worth reading: ssh, mosh's client and the Perl running
/// the mosh script (`/usr/bin/perl` `exec`s into `perl5.NN`, so match by prefix).
fn is_candidate(name: &str) -> bool {
    name == "ssh" || name == "mosh-client" || name.starts_with("perl")
}

/// The target of a recognized process: the outer `None` means "not recognized",
/// the inner `None` "recognized but not an interactive remote session".
fn remote_target(name: &str, args: &[String]) -> Option<Option<Target>> {
    let rest = args.get(1..).unwrap_or_default();
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
        kind: RemoteKind::Ssh,
        argv: call.argv,
        nonce: ssh_wrap::nonce(args).map(str::to_owned),
    })
}

/// What one walk of an interactive ssh argv says ([`ssh_call`]).
pub(crate) struct SshCall {
    /// As typed; only the `ssh://` form drops its scheme and port ([`ssh_host`]).
    pub(crate) host: String,
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
        host: ssh_host(target),
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
                    if flag == 'o'
                        && let Some(value) = value
                    {
                        self.config(value);
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

    /// `-o Key=value` (or with a space): the two keys that change interactivity.
    fn config(&mut self, option: &str) {
        let (key, value) = option
            .split_once(['=', ' ', '\t'])
            .map_or((option, ""), |(key, value)| (key, value.trim()));
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
        }
    }
}

/// The target as written; only in the `ssh://` form are the scheme and port stripped.
fn ssh_host(target: &str) -> String {
    let Some(rest) = target.strip_prefix("ssh://") else {
        return target.to_owned();
    };
    let rest = rest.split('/').next().unwrap_or_default();
    let (user, host) = match rest.rsplit_once('@') {
        Some((user, host)) => (Some(user), host),
        None => (None, rest),
    };
    let host = match host.strip_prefix('[') {
        // IPv6 is in square brackets; the port comes after the closing one.
        Some(inner) => inner.split(']').next().unwrap_or_default(),
        None => host.split(':').next().unwrap_or_default(),
    };
    match user {
        Some(user) => format!("{user}@{host}"),
        None => host.to_owned(),
    }
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

    fn args(&self, pid: u32) -> Option<Vec<String>> {
        process_args(pid)
    }
}

/// `sysctl(KERN_PROCARGS2)`: the layout is `argc` (4 bytes), the exec path, NUL
/// padding, `argc` NUL-terminated arguments — then the **environment**
/// follows, which is not read.
#[cfg(target_os = "macos")]
fn process_args(pid: u32) -> Option<Vec<String>> {
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
    parse_procargs(buf.get(..len)?)
}

/// The pure half of [`process_args`].
#[cfg(target_os = "macos")]
fn parse_procargs(buf: &[u8]) -> Option<Vec<String>> {
    let argc = usize::try_from(i32::from_ne_bytes(buf.get(..4)?.try_into().ok()?)).ok()?;
    let rest = &buf[4..];
    // The exec path, then NUL padding: argv[0] cannot be empty, so the first
    // non-NUL byte is the start of argv.
    let path_end = rest.iter().position(|&b| b == 0)?;
    let start = path_end + rest[path_end..].iter().position(|&b| b != 0)?;
    let mut args = Vec::with_capacity(argc);
    let mut fields = rest[start..].split(|&b| b == 0);
    for _ in 0..argc {
        args.push(String::from_utf8_lossy(fields.next()?).into_owned());
    }
    Some(args)
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
unsafe fn pid_info<T>(pid: u32, flavor: c_int) -> Option<T> {
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

    /// `/proc/<pid>/cmdline`: NUL-separated arguments. Empty for a zombie or a
    /// kernel thread, which is `None` — there is no argv to read.
    fn args(&self, pid: u32) -> Option<Vec<String>> {
        let raw = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
        let raw = raw.strip_suffix(b"\0").unwrap_or(&raw);
        if raw.is_empty() {
            return None;
        }
        Some(
            raw.split(|&b| b == 0)
                .map(|arg| String::from_utf8_lossy(arg).into_owned())
                .collect(),
        )
    }
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
/// pidfd. The reader loop registers it in the child-event pipe's place
/// ([`bt_core::Adoption::exit`]); nobody drains it, so it stays readable.
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

/// [`bt_core::PtyOps`]'s body: the two syscalls of an adopted PTY.
pub struct SystemPty;

impl bt_core::PtyOps for SystemPty {
    fn resize(&self, master: std::os::fd::BorrowedFd<'_>, size: bt_core::PtySize) {
        use std::os::fd::AsRawFd;
        let window = libc::winsize {
            ws_row: size.rows,
            ws_col: size.cols,
            ws_xpixel: size.cols.saturating_mul(size.cell_width),
            ws_ypixel: size.rows.saturating_mul(size.cell_height),
        };
        // SAFETY: `master` is open for the call and `window` a valid input.
        // A failure is left as is: the old size stays (alacritty dies here,
        // the no-panic rule does not).
        if unsafe { libc::ioctl(master.as_raw_fd(), libc::TIOCSWINSZ, &raw const window) } != 0 {
            eprintln!(
                "bateri: resizing the adopted PTY failed: {}",
                std::io::Error::last_os_error()
            );
        }
    }

    fn hangup(&self, pid: u32, exit: std::os::fd::BorrowedFd<'_>) {
        use std::os::fd::AsRawFd;
        let mut poll = libc::pollfd {
            fd: exit.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: one `pollfd` from this frame, zero timeout.
        let ready = unsafe { libc::poll(&raw mut poll, 1, 0) };
        // Exited (or unknowable): the pid may be another process's now.
        if ready != 0 {
            return;
        }
        let Ok(pid) = libc::pid_t::try_from(pid) else {
            return;
        };
        // SAFETY: a signal to a pid whose exit fd says it is still the
        // process we adopted.
        unsafe { libc::kill(pid, libc::SIGHUP) };
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use bt_core::{
        CaretShape, CursorBlink, Osc52, Session, SessionOptions, TerminalOptions, Theme, TtyModes,
    };

    use super::*;
    use crate::child::{SilentWake, wait_until};

    /// Fake process: parent, group, name (`None` → the name is unreadable).
    struct Proc {
        parent: u32,
        group: u32,
        name: Option<&'static str>,
        args: Option<Vec<&'static str>>,
    }

    /// Fake table. `terminal` is the shell's `e_tpgid`; `None` → the shell's long
    /// info is unreadable.
    struct Table {
        procs: HashMap<u32, Proc>,
        terminal: Option<u32>,
        members_readable: bool,
    }

    impl Table {
        fn new(terminal: Option<u32>) -> Self {
            Self {
                procs: HashMap::new(),
                terminal,
                members_readable: true,
            }
        }

        fn with(mut self, pid: u32, parent: u32, group: u32, name: &'static str) -> Self {
            self.procs.insert(
                pid,
                Proc {
                    parent,
                    group,
                    name: Some(name),
                    args: None,
                },
            );
            self
        }

        /// A process with an argv; argv[0] is the name itself.
        fn run(mut self, pid: u32, parent: u32, group: u32, argv: &[&'static str]) -> Self {
            let name = argv[0].rsplit('/').next().expect("name");
            let name: &'static str = Box::leak(name.to_owned().into_boxed_str());
            self.procs.insert(
                pid,
                Proc {
                    parent,
                    group,
                    name: Some(name),
                    args: Some(argv.to_vec()),
                },
            );
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

        fn args(&self, pid: u32) -> Option<Vec<String>> {
            let args = self.procs.get(&pid)?.args.as_ref()?;
            Some(args.iter().map(|&arg| arg.to_owned()).collect())
        }
    }

    fn running(names: &[&str]) -> Foreground {
        Foreground::Running(names.iter().map(|&name| name.to_owned()).collect())
    }

    /// `login` 100 → `zsh` 101 (group 101); the foreground group is the caller's.
    fn login_shell(terminal: u32) -> Table {
        Table::new(Some(terminal))
            .with(100, 1, 100, "login")
            .with(101, 100, 101, "zsh")
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

    #[cfg(target_os = "macos")]
    #[test]
    fn procargs_layout_yields_argv_without_the_environment() {
        let mut buf = 2i32.to_ne_bytes().to_vec();
        buf.extend_from_slice(b"/bin/sleep\0\0\0\0/bin/sleep\0\x33\x30\0HOME=/x\0");
        assert_eq!(
            parse_procargs(&buf),
            Some(vec!["/bin/sleep".to_owned(), "30".to_owned()])
        );
        assert_eq!(parse_procargs(&buf[..3]), None);
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

    #[test]
    fn the_process_table_reads_a_real_argv() {
        // The witness for the argv body (`KERN_PROCARGS2` on macOS, `cmdline`
        // on Linux): a child of the same user with a known argv.
        let mut child = std::process::Command::new("/bin/sleep")
            .arg("30")
            .spawn()
            .expect("sleep did not spawn");
        let expected = Some(vec!["/bin/sleep".to_owned(), "30".to_owned()]);
        // `spawn` returns once the child forked, not once it exec'd: before
        // the exec the argv is still the test runner's (seen on Linux).
        wait_until("the child's argv not seen", || {
            SystemTable.args(child.id()) == expected
        });
        let _ = child.kill();
        let _ = child.wait();
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

    /// Records the child's exit news.
    #[derive(Default)]
    struct ExitWake(std::sync::Mutex<Option<Option<i32>>>);

    impl bt_core::Wake for ExitWake {
        fn wake(&self) {}
        fn child_exit(&self, code: Option<i32>) {
            *self
                .0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(code);
        }
        fn copy_to_clipboard(&self, _text: String) {}
        fn title_changed(&self) {}
        fn search_changed(&self) {}
        fn command_started(&self) {}
        fn remote_up(&self) {}
        fn remote_typed(&self) {}
        fn link_hover_lost(&self) {}
        fn phase_edge(&self) {}
        fn mirror_changed(&self) {}
    }

    impl ExitWake {
        fn exited(&self) -> Option<Option<i32>> {
            *self
                .0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
        }
    }

    fn handover_options(
        command: Option<(String, Vec<String>)>,
        cols: u16,
        rows: u16,
    ) -> SessionOptions {
        SessionOptions {
            command,
            working_directory: None,
            home: None,
            env: HashMap::new(),
            cols,
            rows,
            cell_px: (9, 18),
            terminal: TerminalOptions {
                scrollback: 10_000,
                osc52: Osc52::Off,
                cursor: CaretShape::default(),
                blink: CursorBlink::default(),
            },
            theme: Theme::BATERI,
            dock: false,
            cluster: false,
            initial_input: None,
            shell_marks: false,
            tab_id: None,
            hostname: None,
            replay: None,
            journal: None,
        }
    }

    /// The whole scrollback and screen as text.
    fn all_text(session: &Session) -> String {
        session.select_all();
        session.selection_text().unwrap_or_default()
    }

    /// The `nK` counter lines of `text`, in order.
    fn counters(text: &str) -> Vec<u64> {
        text.lines()
            .filter_map(|line| line.trim().strip_prefix('n')?.parse().ok())
            .collect()
    }

    /// Freezes a session running `script` and adopts it into a new one.
    fn hand_over(
        script: &str,
        ready: &str,
        held: &[u8],
    ) -> (Session, Session, u32, u64, Arc<ExitWake>) {
        let old = Session::spawn(
            handover_options(
                Some((
                    "/bin/sh".to_owned(),
                    vec!["-c".to_owned(), script.to_owned()],
                )),
                40,
                10,
            ),
            Arc::new(SilentWake),
        )
        .expect("session did not open");
        wait_until("the script did not start", || {
            all_text(&old).contains(ready)
        });
        let pid = old.child_pid();
        let start = start_time(pid).expect("the start time not read");
        let frozen = old.freeze().expect("the session did not freeze");
        assert_eq!(frozen.pid, pid);
        assert_eq!((frozen.cols, frozen.rows), (40, 10));
        // The holder's gap: the child keeps writing into the kernel's buffer.
        std::thread::sleep(std::time::Duration::from_millis(300));
        assert_eq!(start_time(pid), Some(start), "the freeze hung the child up");
        let exit = exit_fd(pid, start).expect("no exit fd");
        let wake = Arc::new(ExitWake::default());
        let new = Session::adopt(
            handover_options(None, frozen.cols, frozen.rows),
            bt_core::Adoption {
                master: frozen.master,
                exit,
                pid,
                vt: frozen.vt,
                blob: frozen.blob,
                // The holder's buffer stands behind the tail.
                prefix: [frozen.tail.as_slice(), held].concat(),
                input: frozen.input,
                ops: Arc::new(SystemPty),
                mode: bt_core::AdoptMode::Update,
            },
            Arc::clone(&wake) as Arc<dyn bt_core::Wake>,
        )
        .expect("the session was not adopted");
        assert_eq!(new.child_pid(), pid);
        (old, new, pid, start, wake)
    }

    /// The handover's acceptance: a live `/bin/sh` crosses from one session to
    /// another without a `SIGHUP`, its output has no gap, input reaches it and
    /// its exit is the adopting session's `child_exit(None)`.
    #[test]
    fn a_frozen_shell_lives_on_in_the_adopting_session() {
        let script = "i=0; (while [ $i -lt 2000 ]; do i=$((i+1)); echo n$i; sleep 0.01; done) & bg=$!; \
            while read line; do echo got:$line; [ \"$line\" = quit ] && { kill $bg; exit 0; }; done";
        let (old, new, _pid, _start, wake) = hand_over(script, "n3", b"");
        drop(old);
        let before = counters(&all_text(&new)).last().copied().unwrap_or(0);
        wait_until("the output did not continue", || {
            counters(&all_text(&new))
                .last()
                .is_some_and(|&n| n > before + 20)
        });
        let seen = counters(&all_text(&new));
        assert_eq!(
            seen.first(),
            Some(&1),
            "the replayed history lost its start"
        );
        for pair in seen.windows(2) {
            assert_eq!(pair[1], pair[0] + 1, "a gap or a repeat in {seen:?}");
        }
        new.write(b"hello\n");
        wait_until("the input did not reach the shell", || {
            all_text(&new).contains("got:hello")
        });
        new.write(b"quit\n");
        wait_until("no exit news", || wake.exited().is_some());
        assert_eq!(wake.exited(), Some(None));
        wait_until("the reader did not end", || !new.reader_alive());
    }

    /// A program on the alternate screen crosses on it (vim across the
    /// update), and the frozen pane's history — what a fallback replays —
    /// is still the primary screen: read from the frozen VT, since the
    /// freeze's snapshot drove the live `Term` destructively.
    #[test]
    fn the_alternate_screen_crosses_and_the_history_is_the_primary() {
        let old = Session::spawn(
            handover_options(
                Some((
                    "/bin/sh".to_owned(),
                    vec![
                        "-c".to_owned(),
                        "echo PRIMARY; printf '\\033[?1049hALT'; while :; do sleep 1; done"
                            .to_owned(),
                    ],
                )),
                40,
                10,
            ),
            Arc::new(SilentWake),
        )
        .expect("session did not open");
        wait_until("the script did not reach the alternate screen", || {
            all_text(&old).contains("ALT")
        });
        let frozen = old.freeze().expect("the session did not freeze");
        let history = String::from_utf8_lossy(&old.frozen_history(&frozen)).into_owned();
        assert!(history.contains("PRIMARY"), "{history:?}");
        assert!(!history.contains("ALT"), "{history:?}");
        let pid = frozen.pid;
        let start = start_time(pid).expect("the start time not read");
        let new = Session::adopt(
            handover_options(None, frozen.cols, frozen.rows),
            bt_core::Adoption {
                master: frozen.master,
                exit: exit_fd(pid, start).expect("no exit fd"),
                pid,
                vt: frozen.vt,
                blob: frozen.blob,
                prefix: frozen.tail,
                input: frozen.input,
                ops: Arc::new(SystemPty),
                mode: bt_core::AdoptMode::Update,
            },
            Arc::new(SilentWake),
        )
        .expect("the session was not adopted");
        let screen = all_text(&new);
        assert!(
            screen.contains("ALT") && !screen.contains("PRIMARY"),
            "not on the alternate screen: {screen:?}"
        );
        drop(old);
        let _ = new.shutdown();
    }

    /// Dropping an adopted session hangs its child up and waits for nothing.
    #[test]
    fn an_adopted_session_hangs_up_on_shutdown_without_waiting() {
        use std::os::fd::AsFd;
        use std::time::{Duration, Instant};
        // A quiet shell: the held bytes reach the screen with no output
        // after them (the reader's first read), through the parser.
        let (_old, new, pid, start, _wake) = hand_over(
            "echo ready; while :; do sleep 1; done",
            "ready",
            b"\x1b]2;handed\x07HELD\r\n",
        );
        wait_until("the held bytes were not read", || {
            all_text(&new).contains("HELD")
        });
        assert_eq!(new.title(), "handed");
        let watch = exit_fd(pid, start).expect("no exit fd");
        let began = Instant::now();
        assert_eq!(new.shutdown(), bt_core::Teardown::HungUp);
        assert!(
            began.elapsed() < bt_core::SHUTDOWN_GRACE,
            "the shutdown waited"
        );
        assert!(
            readable(watch.as_fd(), Duration::from_secs(5)),
            "the adopted shell outlived its session"
        );
    }

    /// A program writing without pause (`yes`) across the handover: the
    /// adopted reader's first reads end with the carried prefix, not when a
    /// round comes back short — every round is full here, and a reader stuck
    /// there would never take the shutdown off its channel (found in code review).
    #[test]
    fn a_flooding_program_does_not_hold_the_adopted_reader() {
        use std::time::Instant;
        let (_old, new, _pid, _start, _wake) = hand_over("exec yes", "y", &[b'y'; 64 * 1024]);
        let began = Instant::now();
        assert_eq!(new.shutdown(), bt_core::Teardown::HungUp);
        assert!(
            began.elapsed() < bt_core::SHUTDOWN_GRACE,
            "the shutdown waited on a flooded reader"
        );
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
                tab_id: None,
                hostname: None,
                replay: None,
                journal: None,
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
                tab_id: None,
                hostname: None,
                replay: None,
                journal: None,
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
