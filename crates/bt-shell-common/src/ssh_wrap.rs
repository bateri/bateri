//! Remote shell integration's wrapping decision: whether the user's
//! `ssh` gets bateri's bootstrap, and the argv round trip.
//!
//! **One owner.** The local zsh's `ssh` function asks
//! `bateri ssh-argv` ([`ssh_argv_main`]); the answer is either the wrapped
//! argv or nothing, and nothing means plain `ssh` — today's path. Every
//! wrapping condition is in [`decide`]: the call is interactive (the same walk as
//! the remote probe's, `jobs::ssh_call`), stdin and stdout are terminals (the
//! caller's bit — under `$(…)` our stdout is a pipe), the setting allows it
//! for the host as typed ([`Settings::integration_for`]), `ssh -G` does not
//! say the session runs something else ([`Session`]) and the server is not
//! recorded as one without a shell ([`Fact::Plain`]; always wrap otherwise).
//! A server bateri knows nothing about **is** wrapped: the bootstrap's
//! nonce'd `up` proves the wrap ran ([`Fact::Posix`], the pane's), and a
//! wrapped connection that ends without it falls back to a plain rerun
//! ([`ssh_fell_back_main`]).
//!
//! **The wrapped form is positional**: `-t` first,
//! the user's arguments unchanged, the bootstrap command last — one argument
//! that starts with a letter, so it is the remote command whether or not the
//! user ended the options with `--` (a second `--` after a terminated walk
//! would be sent to the remote shell). [`unwrap`] checks those positions, it
//! does not search for the command.
//!
//! **The session is a master**: when the user shares no
//! connections themselves (`ssh -G`, [`SshConfig::shares_connections`]) and
//! bateri's instance directory is there, the wrapped call also carries
//! `ControlMaster=auto` at [`session_socket`] with a short
//! [`SESSION_PERSIST`] ([`Control`]) — bateri's file jobs ride the connection
//! the user signed in to. Those three options sit right after `-t`, again by
//! position.
//!
//! The state file's rows are `{fact}\t{key}\t{unix time}`; the key is the
//! server as `ssh -G` resolves it ([`host_key`]). Bodies with side effects (the
//! `ssh -G` process, the file's lock and rename) are thin; the rules are pure
//! and tested.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::DirBuilderExt as _;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use bt_core::{PaneUuid, Settings, TERM_PROGRAM_VERSION};

use crate::jobs::ssh_call;
use crate::ssh_route::{
    Account, SESSION_PERSIST, SshConfig, SshRunner, parse_config, session_socket,
};

/// The bootstrap's `$0` and the wrapped command's last word: what [`unwrap`]
/// checks at the last position, together with [`COMMAND_HEAD`].
pub const BOOT_NAME: &str = "bateri-boot";

/// The remote command's head. `exec` replaces the login shell sshd started
/// (`$SHELL -c '<command>'`, which may be fish or csh): one word, a single
/// quote and no backslash, the quoting every shell reads the same way (the
/// upload rule).
const COMMAND_HEAD: &str = "exec sh -c '";

/// The remote command for a bootstrap script:
/// `exec sh -c '<boot>' bateri-boot <P> <nonce> <version> <tab>` — `sh`'s `$1` is the local
/// block of the `ssh` command, the `P` of the remote blocks'
/// `bt_remote=<P>.<S>.<n>`, or [`NO_PARENT`] without one; `$2` is the
/// attempt's [`NONCE_LEN`]-digit nonce, the bootstrap's first
/// output (`8133;i;up;{nonce}`) proves the command ran. The placeholder keeps
/// the nonce at `$2`: without it a nonce of digits would read as a parent.
/// `$3` and `$4` are the terminal's identity the bootstrap exports as
/// `LC_TERMINAL_VERSION` and `LC_BATERI_TAB_URL` (`LC_TERMINAL` is
/// fixed): the version is [`bt_core::TERM_PROGRAM_VERSION`], the tab the
/// pane's `bateri://tab/<id>` or [`NO_TAB`] without one. Both words sit
/// outside the quotes and are read by any login shell, so only
/// [`is_version`]'s alphabet and [`PaneUuid::url`]'s form go there; visible in
/// `ps`, which is harmless — the tab's address only focuses.
/// `boot` must not contain `'` ([`decide`] refuses one that does).
pub fn remote_command(
    boot: &str,
    parent: Option<u32>,
    nonce: &str,
    tab: Option<&PaneUuid>,
) -> String {
    let parent = parent.map_or_else(|| NO_PARENT.to_owned(), |parent| parent.to_string());
    let tab = tab.map_or_else(|| NO_TAB.to_owned(), PaneUuid::url);
    format!("{COMMAND_HEAD}{boot}' {BOOT_NAME} {parent} {nonce} {TERM_PROGRAM_VERSION} {tab}")
}

/// The tab's placeholder in [`remote_command`]: not a `bateri://tab/` URL, so
/// the bootstrap exports no `LC_BATERI_TAB_URL`.
const NO_TAB: &str = "-";

/// Whether `word` can be [`remote_command`]'s version: a short word of
/// letters, digits and `.+-` (a SemVer with its pre-release and build parts)
/// — nothing any login shell reads specially, the bootstrap's own check.
fn is_version(word: &str) -> bool {
    (1..=64).contains(&word.len())
        && word
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'+' | b'-'))
}

/// The parent's placeholder in [`remote_command`]: not digits, so the
/// bootstrap reads it as "no blocks" (`assets/shell/remote/boot.sh`).
const NO_PARENT: &str = "-";

/// The nonce's length in lowercase hex digits (64 bits): [`new_nonce`]'s
/// form, the only one [`unwrap`] and [`nonce`] accept. `bt-core`'s scanner
/// checks only the alphabet and a bound, not this number.
pub const NONCE_LEN: usize = 16;

/// Whether `word` is a nonce of [`new_nonce`]'s form.
fn is_nonce(word: &str) -> bool {
    word.len() == NONCE_LEN && word.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// A fresh nonce for one wrapped attempt: [`NONCE_LEN`] hex digits
/// from `/dev/urandom` (no crate; both platforms have it). `None` if it cannot
/// be read — the caller then does not wrap: a wrap without a nonce could never
/// prove it ran, and the fallback would take the server for one without
/// a shell.
pub fn new_nonce() -> Option<String> {
    use std::io::Read as _;
    let mut bytes = [0u8; NONCE_LEN / 2];
    File::open("/dev/urandom")
        .and_then(|mut random| random.read_exact(&mut bytes))
        .ok()?;
    Some(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// The connection sharing a wrapped session gets: the session is a
/// master at `socket` (`ControlMaster=auto` — a second `ssh` to the host from
/// this bateri joins it), detached after [`SESSION_PERSIST`]. Ours come
/// **before** the user's arguments: ssh keeps a key's first value. Only for a
/// user who shares no connections themselves ([`decide`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Control {
    pub socket: PathBuf,
}

impl Control {
    /// The three options, `-o` each.
    fn options(&self) -> [String; 6] {
        [
            "-o".to_owned(),
            "ControlMaster=auto".to_owned(),
            "-o".to_owned(),
            format!("ControlPath={}", self.socket.display()),
            "-o".to_owned(),
            format!("ControlPersist={}", SESSION_PERSIST.as_secs()),
        ]
    }

    /// Whether `words` are [`Self::options`] for some socket — what
    /// [`unwrap`] takes off, by position.
    fn matches(words: &[String]) -> bool {
        let persist = format!("ControlPersist={}", SESSION_PERSIST.as_secs());
        matches!(words, [o1, master, o2, path, o3, keep]
            if o1 == "-o" && master == "ControlMaster=auto"
                && o2 == "-o" && path.strip_prefix("ControlPath=").is_some_and(|p| !p.is_empty())
                && o3 == "-o" && *keep == persist)
    }
}

/// The user's ssh arguments (without the program) → the wrapped arguments:
/// `-t`, [`Control`]'s options when there are any, then `args` as they are,
/// then [`remote_command`]. The user's own `-t` stays (`-t -t` still asks for
/// a terminal).
pub fn wrap(
    args: &[String],
    boot: &str,
    parent: Option<u32>,
    nonce: &str,
    tab: Option<&PaneUuid>,
    control: Option<&Control>,
) -> Vec<String> {
    let mut wrapped = Vec::with_capacity(args.len() + 8);
    wrapped.push("-t".to_owned());
    if let Some(control) = control {
        wrapped.extend(control.options());
    }
    wrapped.extend(args.iter().cloned());
    wrapped.push(remote_command(boot, parent, nonce, tab));
    wrapped
}

/// The bootstrap command's tail → its nonce, if `last` is a bootstrap
/// command at all (the outer `None`). The accepted tails after the name:
/// nothing, a parent's digits, a parent (digits or [`NO_PARENT`]) and a
/// nonce, or those two and the identity's version and tab
/// ([`remote_command`]) — the first three are older builds' forms, read
/// for a session an older build wrapped; the first two carry
/// no nonce. The script must not contain `'`.
fn boot_tail(last: &str) -> Option<Option<&str>> {
    // The script has no `'`, so the first one closes it.
    let (_, tail) = last.strip_prefix(COMMAND_HEAD)?.split_once('\'')?;
    let tail = tail.strip_prefix(' ')?.strip_prefix(BOOT_NAME)?;
    let digits = |word: &str| !word.is_empty() && word.bytes().all(|b| b.is_ascii_digit());
    let parent = |word: &str| digits(word) || word == NO_PARENT;
    if tail.is_empty() {
        return Some(None);
    }
    let words: Vec<&str> = tail.strip_prefix(' ')?.split(' ').collect();
    match words[..] {
        [first] if digits(first) => Some(None),
        [first, nonce] if parent(first) && is_nonce(nonce) => Some(Some(nonce)),
        [first, nonce, version, tab]
            if parent(first)
                && is_nonce(nonce)
                && is_version(version)
                && (tab == NO_TAB || PaneUuid::from_url(tab).is_some()) =>
        {
            Some(Some(nonce))
        }
        _ => None,
    }
}

/// The inverse of [`wrap`]: the user's arguments if `args` has the wrapped
/// shape, otherwise `args` itself. The shape is positional: `-t` first, then
/// [`Control`]'s six words if they are there, a bootstrap command last (with
/// or without the parent's number and the nonce, [`boot_tail`]), and in
/// between an interactive call with no remote command of its own.
pub fn unwrap(args: &[String]) -> &[String] {
    let [first, inner @ .., last] = args else {
        return args;
    };
    let inner = match inner.split_at_checked(6) {
        Some((control, rest)) if Control::matches(control) => rest,
        _ => inner,
    };
    let wrapped = first == "-t"
        && boot_tail(last).is_some()
        && ssh_call("ssh", inner).is_some_and(|call| !call.command);
    if wrapped { inner } else { args }
}

/// The nonce a wrapped argv carries: `Some` only when [`unwrap`]
/// takes `args` for ours and its bootstrap command has a nonce. The pane
/// matches it against the one the bootstrap printed.
pub fn nonce(args: &[String]) -> Option<&str> {
    if unwrap(args).len() == args.len() {
        return None;
    }
    boot_tail(args.last()?)?
}

// ─── ssh -G ──────────────────────────────────────────────────────────────

/// What `ssh -G` says about the session the target would open.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Session {
    /// `RemoteCommand` is set: our command would replace the user's.
    pub remote_command: bool,
    /// `RequestTTY no` (`-G` prints `false`).
    pub no_tty: bool,
    /// `SessionType` other than `default` (`none`, `subsystem`).
    pub other_type: bool,
}

impl Session {
    /// `ssh -G`'s output → the three facts. Absent lines are OpenSSH's
    /// defaults: `remotecommand` is printed only when set.
    pub fn parse(out: &str) -> Self {
        let mut session = Self {
            remote_command: false,
            no_tty: false,
            other_type: false,
        };
        for line in out.lines() {
            let Some((key, value)) = line.trim().split_once(' ') else {
                continue;
            };
            let value = value.trim();
            match key {
                "remotecommand" => session.remote_command = !value.eq_ignore_ascii_case("none"),
                // `-G` spells `no` as `false` and `yes` as `true`; both spellings read.
                "requesttty" => {
                    session.no_tty =
                        value.eq_ignore_ascii_case("no") || value.eq_ignore_ascii_case("false");
                }
                "sessiontype" => session.other_type = !value.eq_ignore_ascii_case("default"),
                _ => {}
            }
        }
        session
    }

    /// Whether our remote command may run: none of the three.
    pub fn allows_wrap(&self) -> bool {
        !(self.remote_command || self.no_tty || self.other_type)
    }
}

/// The state file's key for a server: `user@hostname:port` as `ssh -G`
/// resolves them — the same triple as the saved password's account
/// ([`Account`]), so `ssh web` and `ssh deploy@10.0.0.5` on one
/// machine are learned once. `None` without a user, a readable port or a host
/// name, and for a value that would break a row (a tab or a line break).
pub fn host_key(out: &str) -> Option<String> {
    let account = Account::from_config(&parse_config(out)?)?;
    let key = format!("{}@{}:{}", account.user, account.host, account.port);
    (!key.contains(['\t', '\n', '\r'])).then_some(key)
}

/// `ssh -G` for the user's arguments: their program and options decide the
/// configuration (`-F`, `-o`, `-p`). `None` if it fails — no wrapping.
fn config(runner: &dyn SshRunner, args: &[String]) -> Option<String> {
    let mut argv = vec!["ssh".to_owned(), "-G".to_owned()];
    argv.extend(args.iter().cloned());
    runner
        .run(&argv)
        .ok()
        .filter(|(code, _, _)| *code == Some(0))
        .map(|(_, out, _)| out)
}

// ─── host state ──────────────────────────────────────────────────────────

/// What bateri knows about a server.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fact {
    /// The server runs a POSIX `sh`: a wrapped connection started there — the
    /// bootstrap's nonce'd `8133;i;up` arrived (older builds also counted
    /// the helper session's greeting).
    Posix,
    /// bateri's bootstrap ran there — the list of servers with bateri's files
    /// (the remove button comes later).
    Touched,
    /// A wrapped connection ended without the bootstrap's `up` and not with
    /// ssh's own error ([`fell_back`]): the login shell does not run
    /// our command (a router, Windows) — the server is not wrapped again.
    Plain,
}

impl Fact {
    fn name(self) -> &'static str {
        match self {
            Self::Posix => "posix",
            Self::Touched => "touched",
            Self::Plain => "plain",
        }
    }

    fn from_name(name: &str) -> Option<Self> {
        match name {
            "posix" => Some(Self::Posix),
            "touched" => Some(Self::Touched),
            "plain" => Some(Self::Plain),
            _ => None,
        }
    }
}

/// The state file's rows, in file order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HostState {
    rows: Vec<(Fact, String, u64)>,
}

impl HostState {
    /// The file's text → rows; a malformed row is skipped.
    pub fn parse(text: &str) -> Self {
        let rows = text
            .lines()
            .filter_map(|line| {
                let mut fields = line.split('\t');
                let fact = Fact::from_name(fields.next()?)?;
                let key = fields.next().filter(|key| !key.is_empty())?;
                let at = fields.next()?.parse().ok()?;
                fields.next().is_none().then(|| (fact, key.to_owned(), at))
            })
            .collect();
        Self { rows }
    }

    /// The rows as the file holds them.
    pub fn render(&self) -> String {
        self.rows
            .iter()
            .map(|(fact, key, at)| format!("{}\t{key}\t{at}\n", fact.name()))
            .collect()
    }

    pub fn knows(&self, fact: Fact, key: &str) -> bool {
        self.rows.iter().any(|(f, k, _)| *f == fact && k == key)
    }

    /// Records `fact` for `key` at `at`: a new row, or the existing row's time
    /// (one row per fact and key).
    pub fn note(&mut self, fact: Fact, key: &str, at: u64) {
        match self
            .rows
            .iter_mut()
            .find(|(f, k, _)| *f == fact && k == key)
        {
            Some(row) => row.2 = at,
            None => self.rows.push((fact, key.to_owned(), at)),
        }
    }

    /// Removes `fact`'s row for `key`, if any.
    pub fn forget(&mut self, fact: Fact, key: &str) {
        self.rows.retain(|(f, k, _)| !(*f == fact && k == key));
    }
}

/// Reads the state file; a missing or unreadable file knows nothing: every
/// server is wrapped and none counts as `posix` — and [`ssh_argv_main`]
/// prints nothing when the `touched` row cannot be written, so an unwritable
/// file still means plain `ssh`.
pub fn load(path: &Path) -> HostState {
    fs::read_to_string(path)
        .map(|text| HostState::parse(&text))
        .unwrap_or_default()
}

/// How long [`record`] waits for another writer's lock. The holder is always a
/// short read-modify-rename; a holder that is stuck must not hang the user's
/// `ssh`. A design constant, not a measurement.
pub const LOCK_PATIENCE: Duration = Duration::from_millis(500);

/// The lock file beside the state file. The state file itself cannot carry
/// the lock: every write renames a new inode onto its path.
fn lock_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".lock");
    path.with_file_name(name)
}

/// Takes the exclusive lock, waiting at most `patience`.
fn lock(path: &Path, patience: Duration) -> io::Result<File> {
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(lock_path(path))?;
    let deadline = Instant::now() + patience;
    loop {
        // SAFETY: `flock` on a descriptor this function owns; no memory is passed.
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
            return Ok(file);
        }
        let err = io::Error::last_os_error();
        if err.kind() != io::ErrorKind::WouldBlock {
            return Err(err);
        }
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "state file locked",
            ));
        }
        thread::sleep(Duration::from_millis(10));
    }
}

/// Adds `fact` for `key` to the state file under its lock: read, note, write
/// a temporary name, rename. The directory is created (owner only). The lock
/// is released when the lock file closes.
pub fn record(path: &Path, fact: Fact, key: &str, at: u64, patience: Duration) -> io::Result<()> {
    rewrite(path, patience, |state| state.note(fact, key, at))
}

/// [`record`]'s body for any change of the rows: under the lock (waiting at
/// most `patience`), read, `change`, write a temporary name, rename.
fn rewrite(path: &Path, patience: Duration, change: impl FnOnce(&mut HostState)) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)?;
    }
    let _lock = lock(path, patience)?;
    let mut state = load(path);
    change(&mut state);
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".tmp.{}", std::process::id()));
    let temporary = path.with_file_name(name);
    let written = File::create(&temporary)
        .and_then(|mut file| file.write_all(state.render().as_bytes()))
        .and_then(|()| fs::rename(&temporary, path));
    if written.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    written
}

/// Now as Unix seconds (a clock before 1970 reads as zero).
pub fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

// ─── the decision ────────────────────────────────────────────────────────

/// A wrapped connection: the arguments to run and the server's key (the
/// caller records [`Fact::Touched`] for it before running them).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Wrapped {
    pub args: Vec<String>,
    pub key: String,
}

/// Every wrapping condition in one place; `None` → plain `ssh`.
///
/// The cheap questions come first, so `ssh -G` (a process, and it runs the
/// config's `Match exec`) is asked only for a call that would be wrapped:
/// a usable bootstrap and nonce, terminals on both ends, an interactive call without a
/// remote command, the setting for the host as typed; then `ssh -G` and the
/// state of the server it resolves to — only a [`Fact::Plain`] server is not
/// wrapped, an unknown one is. `sockets` are this bateri's
/// instance directories ([`crate::ssh_route::instance_dirs`]): with one, the
/// session also becomes a master there ([`Control`]). `tab` is the pane's
/// identity the bootstrap exports ([`remote_command`]); it decides
/// nothing.
#[allow(clippy::too_many_arguments)] // The conditions' inputs, each a different source; a struct would only rename them
pub fn decide(
    args: &[String],
    tty: bool,
    settings: &Settings,
    runner: &dyn SshRunner,
    state: &HostState,
    boot: &str,
    parent: Option<u32>,
    nonce: &str,
    tab: Option<&PaneUuid>,
    sockets: &[PathBuf],
) -> Option<Wrapped> {
    if boot.is_empty() || !is_inline(boot) || !is_nonce(nonce) || !tty {
        return None;
    }
    let call = ssh_call("ssh", args).filter(|call| !call.command)?;
    if !settings.integration_for(&call.host) {
        return None;
    }
    let out = config(runner, args)?;
    if !Session::parse(&out).allows_wrap() {
        return None;
    }
    let key = host_key(&out)?;
    if state.knows(Fact::Plain, &key) {
        return None;
    }
    let control = control(&out, sockets).filter(|_| !names_sharing(args));
    Some(Wrapped {
        args: wrap(args, boot, parent, nonce, tab, control.as_ref()),
        key,
    })
}

/// Whether the user's arguments choose connection sharing themselves — `-S`,
/// or a `ControlMaster`/`ControlPath`/`ControlPersist` option, an opt-out
/// (`-o ControlMaster=no`, `-S none`) included: `ssh -G` prints an opt-out as
/// the default, and our options, being first, would override it.
fn names_sharing(args: &[String]) -> bool {
    let sharing_key = |value: &str| {
        let key: String = value
            .trim_start()
            .chars()
            .take_while(|c| c.is_ascii_alphabetic())
            .collect::<String>()
            .to_ascii_lowercase();
        matches!(
            key.as_str(),
            "controlmaster" | "controlpath" | "controlpersist"
        )
    };
    args.iter().enumerate().any(|(at, arg)| {
        arg.starts_with("-S")
            || (arg == "-o" && args.get(at + 1).is_some_and(|value| sharing_key(value)))
            || arg
                .strip_prefix("-o")
                .is_some_and(|value| !value.is_empty() && sharing_key(value))
    })
}

/// The session's [`Control`]: `None` when the user's configuration
/// shares connections itself, when `ssh -G` cannot be read as a configuration
/// or when no instance directory holds the socket ([`session_socket`]).
fn control(out: &str, sockets: &[PathBuf]) -> Option<Control> {
    let config: SshConfig = parse_config(out)?;
    if config.shares_connections() {
        return None;
    }
    let socket = session_socket(sockets, &crate::ssh_route::host_key(&config))?;
    Some(Control { socket })
}

// ─── the bootstrap ───────────────────────────────────────────────────────

/// Whether `boot` can sit inside the remote command's single quotes for every
/// login shell sshd may run it through (`$SHELL -c`): no `'` (it would close
/// the quote), no `\` (fish reads `\'` and `\\` inside single quotes), no `!`
/// (csh's history expansion) and no line break (csh refuses one inside quotes)
/// — one printable ASCII line. The upload rule, widened by the shells.
pub fn is_inline(boot: &str) -> bool {
    boot.bytes()
        .all(|b| (b' '..=b'~').contains(&b) && !matches!(b, b'\'' | b'\\' | b'!'))
}

/// The bootstrap's payload (`assets/shell/remote/boot.sh`), decoded and
/// `eval`ed on the server.
const BOOT_SCRIPT: &str = include_str!("../../../assets/shell/remote/boot.sh");

/// The payload's first line: the one-liner runs only a decoded text that
/// starts with it (a decoder that ignored `-d` prints base64, not this).
const MAGIC: &str = "bateri_boot=1";

/// The files the bootstrap writes under `~/.local/share/bateri/shell/`,
/// embedded at build time (the subcommand runs
/// on every `ssh`, so it reads no package file): the local zsh wrapper's four
/// `ZDOTDIR` files and its swap **verbatim**, the remote zsh body beside them,
/// bash's `ENV` file and fish's `vendor_conf.d` file.
const REMOTE_FILES: [(&str, &str); 8] = [
    (
        "zsh/.zshenv",
        include_str!("../../../assets/shell/zsh/.zshenv"),
    ),
    (
        "zsh/.zprofile",
        include_str!("../../../assets/shell/zsh/.zprofile"),
    ),
    (
        "zsh/.zshrc",
        include_str!("../../../assets/shell/zsh/.zshrc"),
    ),
    (
        "zsh/.zlogin",
        include_str!("../../../assets/shell/zsh/.zlogin"),
    ),
    (
        "zsh/zdotdir.zsh",
        include_str!("../../../assets/shell/zsh/zdotdir.zsh"),
    ),
    (
        "zsh/bateri.zsh",
        include_str!("../../../assets/shell/remote/zsh/bateri.zsh"),
    ),
    (
        "bash/bateri.bash",
        include_str!("../../../assets/shell/remote/bash/bateri.bash"),
    ),
    (
        "fish/vendor_conf.d/bateri.fish",
        include_str!("../../../assets/shell/remote/fish/vendor_conf.d/bateri.fish"),
    ),
];

/// The payload: [`MAGIC`], `bt_files` (one `bt_put` per [`REMOTE_FILES`]
/// entry, its content a single-quoted literal (`upload::sq`, the crate's one
/// quoting rule) — no here-document: bash as
/// `sh` would write one to `/tmp`), then [`BOOT_SCRIPT`].
fn payload() -> String {
    let mut text = format!("{MAGIC}\nbt_files() {{\n");
    for (path, content) in REMOTE_FILES {
        text.push_str(&format!(
            "  bt_put {} {} || return 1\n",
            crate::upload::sq(path),
            crate::upload::sq(content)
        ));
    }
    text.push_str("}\n");
    text.push_str(BOOT_SCRIPT);
    text
}

/// Standard base64 with padding, one line: the payload's encoding (no crate
/// for 15 lines; the server's decoders read this form).
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for (i, shift) in [18, 12, 6, 0].into_iter().enumerate() {
            if i <= chunk.len() {
                out.push(char::from(ALPHABET[((n >> shift) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// The one-liner around the payload — the text [`remote_command`] quotes.
/// It finds a decoder among `base64 -d` (GNU, BusyBox, macOS 13+),
/// `base64 -D` (older macOS), `b64decode -r` (BSD) and `openssl base64 -d -A`,
/// keeps the first answer that starts with [`MAGIC`] and `eval`s it; without
/// one it reports `decode` (`8133;f`) — after the attempt's `8133;i;up`,
/// which the payload would have printed first: our `sh -c` ran, so the
/// server has a shell, it only lacks a decoder, and the user gets a working
/// plain login shell (the fallback must not take it for a shell-less
/// endpoint and reconnect the user after their `exit`). The nonce is `$2`,
/// checked by `awk`'s regex (no `[!…]` here: [`is_inline`]). Both arms end in the plain login shell:
/// the payload `exec`s its own, and a payload that returned (a parse error on
/// an unusual `sh`) must not close the connection. The
/// escape bytes come from `awk`'s `%c`: a `\033` is not [`is_inline`].
fn one_liner(payload: &str) -> String {
    format!(
        "b={b64}; s=; for d in \"base64 -d\" \"base64 -D\" \"b64decode -r\" \
         \"openssl base64 -d -A\"; do s=$(printf %s \"$b\" | $d 2>/dev/null); \
         case $s in {MAGIC}*) break;; esac; s=; done; unset b d; \
         case $s in {MAGIC}*) eval \"$s\";; *) awk -v f=%c%s%c -v u=\"$2\" \
         -v p=\"]8133;i;up;\" -v m=\"]8133;f;decode\" \
         \"BEGIN{{if(u~/^[0-9a-f]+$/)printf(f,27,p u,7);printf(f,27,m,7)}}\" 2>/dev/null;; \
         esac; exec \"${{SHELL:-/bin/sh}}\" -l",
        b64 = base64(payload.as_bytes()),
    )
}

/// The bootstrap the wrapped `ssh` runs (built once per process): the
/// one-liner carrying the base64 payload. Always [`is_inline`].
pub fn boot() -> &'static str {
    static BOOT: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    BOOT.get_or_init(|| one_liner(&payload()))
}

// ─── the bootstrap's proof ───────────────────────────────────────────────

/// [`host_key`] of `ssh -G` for `ssh` (the program first); `None` if it
/// fails.
fn key_of(runner: &dyn SshRunner, ssh: &[String]) -> Option<String> {
    let (program, args) = ssh.split_first()?;
    let mut argv = vec![program.clone(), "-G".to_owned()];
    argv.extend(args.iter().cloned());
    runner
        .run(&argv)
        .ok()
        .filter(|(code, _, _)| *code == Some(0))
        .and_then(|(_, out, _)| host_key(&out))
}

/// Records that the server behind `ssh` runs a POSIX `sh`: `ssh` is an argv
/// to the server (the program, its options, the target, no remote command)
/// and the caller calls this once the server proved it — the wrapped call's
/// bootstrap said `up` with the attempt's nonce (the pane's
/// `check_remote_up`, with the user's argv); nothing else writes `posix`
/// The key is
/// [`host_key`] of `ssh -G` for the same argv (the route's own options do not
/// change user, host or port). `Ok(true)` if a row was written; a server
/// already recorded writes nothing (`Ok(false)`), and so does an argv `ssh -G`
/// cannot read.
pub fn record_posix(runner: &dyn SshRunner, ssh: &[String], path: &Path) -> io::Result<bool> {
    let Some(key) = key_of(runner, ssh) else {
        return Ok(false);
    };
    if load(path).knows(Fact::Posix, &key) {
        return Ok(false);
    }
    record(path, Fact::Posix, &key, unix_now(), LOCK_PATIENCE).map(|()| true)
}

/// The directory of the seen nonces beside the state file:
/// `{state file}.up/`, one empty file per nonce.
pub fn up_dir(state_path: &Path) -> PathBuf {
    let mut name = state_path.file_name().unwrap_or_default().to_os_string();
    name.push(".up");
    state_path.with_file_name(name)
}

/// How long a seen nonce's file is kept when nobody consumed it (an `ssh`
/// that ended with [`SSH_FAILURE`], a shell killed before its `ssh`
/// returned): every [`mark_up`] sweeps older ones. A session that outlives it
/// loses only this proof — its server is `posix` by then, which
/// [`fell_back`] asks as well. A design constant.
pub const UP_KEEP: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// Records that the bootstrap of the attempt `nonce` said `up` (the
/// fallback's first question): an empty file in [`up_dir`], created
/// without the state file's lock and **before** `ssh -G` — the pane calls it
/// as soon as `up` arrives, so a session that ends at once (a login file that
/// exits) or a slow `ssh -G` (`Match exec`) cannot outrun the proof and have
/// its server branded `plain`. Only a nonce of [`new_nonce`]'s form makes a
/// file name (no path can be smuggled in); older unconsumed files are swept
/// ([`UP_KEEP`]).
pub fn mark_up(state_path: &Path, nonce: &str) -> io::Result<()> {
    if !is_nonce(nonce) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "not a nonce"));
    }
    let dir = up_dir(state_path);
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&dir)?;
    File::create(dir.join(nonce))?;
    if let Ok(entries) = fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let stale = entry
                .metadata()
                .and_then(|meta| meta.modified())
                .ok()
                .and_then(|at| at.elapsed().ok())
                .is_some_and(|age| age > UP_KEEP);
            if stale {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
    Ok(())
}

/// The suffix of [`mark_used`]'s file beside [`mark_up`]'s.
const USED_SUFFIX: &str = ".used";

/// The suffix of [`mark_login`]'s file.
const LOGIN_SUFFIX: &str = ".login";

/// An empty `{nonce}{suffix}` in [`up_dir`]: [`mark_up`]'s form check and
/// directory, without its sweep (an unconsumed file goes in the next
/// [`mark_up`]'s).
fn mark_with(state_path: &Path, nonce: &str, suffix: &str) -> io::Result<()> {
    if !is_nonce(nonce) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "not a nonce"));
    }
    let dir = up_dir(state_path);
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&dir)?;
    File::create(dir.join(format!("{nonce}{suffix}"))).map(drop)
}

/// Records that the user typed into the attempt `nonce`'s session after its
/// login: an empty `{nonce}.used` beside [`mark_up`]'s file. The
/// pane calls it at the first input after the login edge; the fallback then
/// takes the session for the user's — a `ForceCommand` CLI ignores our
/// command and never says `up`, but its user's `exit` must not reconnect
/// them, nor brand the server `plain`.
pub fn mark_used(state_path: &Path, nonce: &str) -> io::Result<()> {
    mark_with(state_path, nonce, USED_SUFFIX)
}

/// Records that the attempt `nonce`'s session got past its login (the
/// pane's login edge, from the terminal modes): an
/// empty `{nonce}.login`. It turns the fallback's reading of
/// [`SSH_FAILURE`]: before the login 255 is ssh's own error (a password, the
/// network, a host key) and says nothing; after it, with no `up` and no
/// input, it is an endpoint that refused our command (an `exec` request
/// refused, a channel closed without an exit status) and falls back like any
/// other code.
pub fn mark_login(state_path: &Path, nonce: &str) -> io::Result<()> {
    mark_with(state_path, nonce, LOGIN_SUFFIX)
}

/// The wrapped call's own master socket ([`Control`], the options [`wrap`]
/// put after `-t`); `None` without them (the user shares connections
/// themselves, no instance directory).
fn master_socket(wrapped: &[String]) -> Option<&str> {
    wrapped
        .get(1..7)
        .filter(|words| Control::matches(words))
        .and_then(|options| options[3].strip_prefix("ControlPath="))
}

/// Whether the wrapped call's master socket ([`master_socket`]) answers a
/// connection (`remove_instance`'s liveness test — a socket file alone may be
/// a stale one): ssh opens it only after the authentication and keeps it
/// [`SESSION_PERSIST`] past the session, so a live one says the server was
/// logged in to — the race-free twin of [`mark_login`] for a session too
/// short for the pane's login probe (an endpoint that refuses our command at
/// once; measured in an end-to-end run, where the probe never saw
/// it). **Known limit**: the path is per host, not per attempt — another
/// pane's live master to the same server, logged in while this attempt was
/// still at its own password prompt, reads the same; the cost is one plain
/// rerun that asks again, and a `plain` row only for a server not `posix`
/// (one whose wrap gained nothing).
fn master_listening(wrapped: &[String]) -> bool {
    master_socket(wrapped).is_some_and(|path| std::os::unix::net::UnixStream::connect(path).is_ok())
}

/// Whether [`mark_login`] recorded `nonce`; consumed.
fn take_login(state_path: &Path, nonce: &str) -> bool {
    is_nonce(nonce)
        && fs::remove_file(up_dir(state_path).join(format!("{nonce}{LOGIN_SUFFIX}"))).is_ok()
}

/// Whether the attempt `nonce` was the user's: [`mark_up`] or [`mark_used`]
/// recorded it. Both files are consumed (removed) — one wrapped attempt, one
/// question.
fn take_up(state_path: &Path, nonce: &str) -> bool {
    if !is_nonce(nonce) {
        return false;
    }
    let dir = up_dir(state_path);
    let up = fs::remove_file(dir.join(nonce)).is_ok();
    let used = fs::remove_file(dir.join(format!("{nonce}{USED_SUFFIX}"))).is_ok();
    up || used
}

/// `bateri ssh-argv [--tty] [--block N] [--instance I] -- <ssh arguments…>`: the subcommand's body
/// (`argv` is what follows `ssh-argv`). Writes the wrapped arguments to `out`,
/// each followed by a NUL, or nothing; the exit code is always zero and every
/// failure is "nothing" — the caller runs plain `ssh`.
///
/// `--tty` says stdin and stdout are terminals: the caller asks, because our
/// own stdout is the caller's pipe. `--block N` is the local block running the
/// `ssh` (`$__bateri_block`), carried to the server as the remote blocks'
/// parent ([`remote_command`]); without it the server prints no block marks.
/// `--instance I` is the running bateri's instance directory name
/// (`BATERI_SSH_INSTANCE`), looked for under `roots`
/// ([`crate::ssh_route::socket_bases`]); without it, or without the
/// directory, the session is no master ([`Control`]). `settings` is the settings file's launch
/// reading (an unusable file turns the integration off); `state_path` is the
/// platform's state file. A wrapped connection is recorded as
/// [`Fact::Touched`] **before** it is printed: if the record fails nothing is
/// printed, because the touched list is the user's account of where bateri
/// wrote. `nonce` is this attempt's ([`new_nonce`]); without one nothing is
/// printed either. `tab` is the calling pane's identity (`BATERI_TAB_URL`,
/// carried to the server ([`remote_command`]).
#[allow(clippy::too_many_arguments)] // the platform's inputs, each a different source
pub fn ssh_argv_main(
    argv: &[String],
    settings: &Settings,
    runner: &dyn SshRunner,
    state_path: &Path,
    roots: &[PathBuf],
    boot: &str,
    nonce: Option<&str>,
    tab: Option<&PaneUuid>,
    out: &mut impl Write,
) -> i32 {
    let Some(nonce) = nonce else {
        return 0;
    };
    let (tty, rest) = match argv {
        [flag, rest @ ..] if flag == "--tty" => (true, rest),
        rest => (false, rest),
    };
    // `--block N`: the local block of the `ssh` command, the
    // remote blocks' parent. A malformed one (an empty `__bateri_block`) costs
    // only the blocks: the connection is still wrapped, without a parent.
    let (parent, rest) = match rest {
        [flag, number, rest @ ..] if flag == "--block" => (number.parse().ok(), rest),
        rest => (None, rest),
    };
    let (sockets, rest) = match rest {
        [flag, instance, rest @ ..] if flag == "--instance" => {
            (crate::ssh_route::instance_dirs(roots, instance), rest)
        }
        rest => (Vec::new(), rest),
    };
    let Some(("--", args)) = rest.split_first().map(|(head, tail)| (head.as_str(), tail)) else {
        return 0;
    };
    let state = load(state_path);
    let Some(wrapped) = decide(
        args, tty, settings, runner, &state, boot, parent, nonce, tab, &sockets,
    ) else {
        return 0;
    };
    if record(
        state_path,
        Fact::Touched,
        &wrapped.key,
        unix_now(),
        LOCK_PATIENCE,
    )
    .is_err()
    {
        return 0;
    }
    let mut bytes = Vec::new();
    for arg in &wrapped.args {
        bytes.extend_from_slice(arg.as_bytes());
        bytes.push(0);
    }
    let _ = out.write_all(&bytes).and_then(|()| out.flush());
    0
}

// ─── the fallback ────────────────────────────────────────────────────────

/// ssh's own exit code for a connection or authentication error (`ssh(1)`):
/// never the remote shell's, so it says nothing about the server's shell.
pub const SSH_FAILURE: i32 = 255;

/// Whether a wrapped `ssh`'s code says nothing about the server's shell:
/// [`SSH_FAILURE`] before the login (`logged_in`, [`mark_login`]), or a
/// signal's (`128 + N`, the shell's spelling — a Ctrl-C at the password or
/// host-key prompt is 130, before our command could run). A cancelled
/// connection must neither brand the server `plain` nor open again; a 255
/// after the login is an endpoint that refused our command and does fall
/// back — otherwise it
/// would be wrapped, and broken, on every connection.
fn says_nothing(rc: i32, logged_in: bool) -> bool {
    if rc == SSH_FAILURE {
        !logged_in
    } else {
        rc > 128
    }
}

/// A wrapped connection that fell back: the arguments to run again,
/// plain, and the server's key (the caller records [`Fact::Plain`] for it).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FellBack {
    pub args: Vec<String>,
    pub key: String,
}

/// The fallback's decision once the wrapped `ssh` ended with `rc` — pure: `config` is `ssh -G`'s output for the user's `args` and
/// `state` the state file as read now.
///
/// - `rc` is a signal's (`> 128`), or [`SSH_FAILURE`] and not `logged_in`
///   ([`says_nothing`]): `None` — a refused password, an unreachable host or
///   a Ctrl-C at the prompt says nothing about the shell, nothing is recorded
///   or rerun. A 255 after the login goes on like any other code.
/// - The server is [`Fact::Posix`]: `None` — the bootstrap's `up` arrived, the
///   session was the user's and its code is theirs.
/// - The server is [`Fact::Touched`] and not `posix`: the plain rerun — the
///   user's `args` with the **same** [`Control`] options [`decide`] gave the
///   wrapped call (one producer), so the rerun rides the wrapped session's
///   master while its [`SESSION_PERSIST`] lasts and asks no second password.
/// - Anything else (never wrapped, an unreadable configuration): `None`.
pub fn fell_back(
    args: &[String],
    rc: i32,
    logged_in: bool,
    config: &str,
    state: &HostState,
    sockets: &[PathBuf],
) -> Option<FellBack> {
    if says_nothing(rc, logged_in) {
        return None;
    }
    let key = host_key(config)?;
    if state.knows(Fact::Posix, &key) || !state.knows(Fact::Touched, &key) {
        return None;
    }
    let control = control(config, sockets).filter(|_| !names_sharing(args));
    let mut plain = Vec::with_capacity(args.len() + 6);
    if let Some(control) = control {
        plain.extend(control.options());
    }
    plain.extend(args.iter().cloned());
    Some(FellBack { args: plain, key })
}

/// How long [`ssh_fell_back_main`] waits for the `posix` row before it calls a
/// server shell-less: the row is the pane's,
/// written when the bootstrap's `up` arrives — a main-queue turn, then `ssh -G`
/// and the state file's lock on a thread of its own — and a session that ends
/// within that chain (a login file that exits at once) would otherwise find
/// no row yet and brand a server with a shell `plain`, which is the wrong
/// direction and lasting. Only the "touched, not posix" answer waits: the
/// shell-less endpoint pays it once (its next connection is plain from the
/// start). A design constant, not a measurement.
pub const FELL_BACK_PATIENCE: Duration = Duration::from_millis(500);

/// The re-read interval within [`FELL_BACK_PATIENCE`].
const FELL_BACK_POLL: Duration = Duration::from_millis(25);

/// `bateri ssh-fell-back --rc N [--instance I] -- <wrapped ssh arguments…>`
/// The subcommand's body (`argv` is what follows
/// `ssh-fell-back`), asked by the local zsh's `ssh` function after a wrapped
/// `ssh` ended with `N`. The arguments are the **wrapped** ones, as
/// `ssh-argv` printed them: [`unwrap`] gives back the user's and [`nonce`]
/// the attempt's — one parser, the shell does not take our command apart.
/// Writes the plain rerun's arguments to `out` ([`fell_back`]), each followed
/// by a NUL, or nothing — `ssh-argv`'s wire; the exit code is always zero.
/// `--instance` is `ssh-argv`'s, so the rerun's sharing options are the
/// wrapped call's.
///
/// **The wrong direction is a rerun**: a
/// server with a shell branded `plain` loses its integration, and a user who
/// typed `exit` would be connected again. So the first question is the
/// attempt's own proof ([`mark_up`], written by the pane the moment `up`
/// arrives, before any `ssh -G`; or [`mark_used`], the user typed after the
/// login): seen → nothing. Then [`fell_back`] (`posix`
/// also says nothing). Every unknown says nothing: arguments that are not
/// ours, no nonce, a malformed call, a signal's code, an [`SSH_FAILURE`] before
/// the login (neither [`mark_login`] nor the wrapped call's master listening,
/// [`master_listening`]), an unreadable
/// configuration. Only a wrapped attempt whose proof did not arrive within
/// `patience` ([`FELL_BACK_PATIENCE`]; the proof and the state file are
/// re-read meanwhile) falls back. The [`Fact::Plain`] row is written before
/// printing, but a failed write still prints: the reconnection is the user's,
/// the row only saves the next one's detour.
pub fn ssh_fell_back_main(
    argv: &[String],
    runner: &dyn SshRunner,
    state_path: &Path,
    roots: &[PathBuf],
    patience: Duration,
    out: &mut impl Write,
) -> i32 {
    let Some((rc, rest)) = (match argv {
        [flag, code, rest @ ..] if flag == "--rc" => code.parse::<i32>().ok().map(|rc| (rc, rest)),
        _ => None,
    }) else {
        return 0;
    };
    let (sockets, rest) = match rest {
        [flag, instance, rest @ ..] if flag == "--instance" => {
            (crate::ssh_route::instance_dirs(roots, instance), rest)
        }
        rest => (Vec::new(), rest),
    };
    let Some(("--", wrapped)) = rest.split_first().map(|(head, tail)| (head.as_str(), tail)) else {
        return 0;
    };
    let Some(attempt) = nonce(wrapped) else {
        return 0;
    };
    let args = unwrap(wrapped);
    let mut logged_in =
        take_login(state_path, attempt) || (rc == SSH_FAILURE && master_listening(wrapped));
    // Without our master the pane's login mark is the only proof, and it
    // comes from a probe on a thread of its own: a 255 waits `patience` for
    // it (with our master the socket already answered, and an ssh error
    // returns at once).
    if rc == SSH_FAILURE && !logged_in && master_socket(wrapped).is_none() {
        let deadline = Instant::now() + patience;
        while !logged_in && Instant::now() < deadline {
            thread::sleep(FELL_BACK_POLL);
            logged_in = take_login(state_path, attempt);
        }
    }
    if says_nothing(rc, logged_in) || take_up(state_path, attempt) {
        return 0;
    }
    let Some(resolved) = config(runner, args) else {
        return 0;
    };
    let deadline = Instant::now() + patience;
    let plain = loop {
        let Some(plain) = fell_back(args, rc, logged_in, &resolved, &load(state_path), &sockets)
        else {
            return 0;
        };
        if take_up(state_path, attempt) {
            return 0;
        }
        if Instant::now() >= deadline {
            break plain;
        }
        thread::sleep(FELL_BACK_POLL);
    };
    let _ = record(
        state_path,
        Fact::Plain,
        &plain.key,
        unix_now(),
        LOCK_PATIENCE,
    );
    let mut bytes = Vec::new();
    for arg in &plain.args {
        bytes.extend_from_slice(arg.as_bytes());
        bytes.push(0);
    }
    let _ = out.write_all(&bytes).and_then(|()| out.flush());
    0
}

/// Forgets that the server behind `ssh` (an argv like [`record_posix`]'s) has
/// no shell: the Shell menu's integration toggle — the one way back
/// from a `plain` row learned wrongly. `Ok(true)` if a row was removed.
pub fn forget_plain(runner: &dyn SshRunner, ssh: &[String], path: &Path) -> io::Result<bool> {
    let Some(key) = key_of(runner, ssh) else {
        return Ok(false);
    };
    if !load(path).knows(Fact::Plain, &key) {
        return Ok(false);
    }
    rewrite(path, LOCK_PATIENCE, |state| state.forget(Fact::Plain, &key)).map(|()| true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    use bt_core::{HostMark, HostRule};

    fn words(list: &[&str]) -> Vec<String> {
        list.iter().map(|&word| word.to_owned()).collect()
    }

    const BOOT_STUB: &str = "echo hi";
    const NONCE: &str = "0123456789abcdef";

    /// **Wire (c), the old `ssh` function → the new binary**:
    /// after an update the carried zsh still has the **previous** version's
    /// `ssh` function, which asks the binary at the same path — now the new
    /// one — with its own flags, and hands `ssh-fell-back` the argv the old
    /// binary wrapped. Frozen here: the function's two calls as today's
    /// script makes them and a wrapped argv of an older version (another
    /// version word). **The rule:** a flag or a tail form is added beside
    /// these, never changed; this fixture goes two versions later.
    #[test]
    fn the_previous_functions_calls_still_parse() {
        let script = include_str!("../../../assets/shell/zsh/bateri.zsh");
        for call in [
            r#"ssh-argv $tty --block "$__bateri_block" "${instance[@]}" -- "$@""#,
            r#"ssh-fell-back --rc $rc "${instance[@]}" -- "${wrapped[@]}""#,
        ] {
            assert!(script.contains(call), "the function's call moved: {call}");
        }
        let older = words(&[
            "-t",
            "-o",
            "ControlMaster=auto",
            "-o",
            "ControlPath=/tmp/bateri-501/0a1b2c3d/u-0123456789abcdef",
            "-o",
            "ControlPersist=2",
            "-p",
            "2222",
            "prod",
            "exec sh -c 'echo hi' bateri-boot 7 0123456789abcdef 0.0.1 -",
        ]);
        assert_eq!(unwrap(&older), &words(&["-p", "2222", "prod"])[..]);
        assert_eq!(nonce(&older), Some("0123456789abcdef"));
        assert_eq!(
            master_socket(&older),
            Some("/tmp/bateri-501/0a1b2c3d/u-0123456789abcdef")
        );
    }

    #[test]
    fn unwrap_inverts_wrap() {
        for args in [
            &["prod"][..],
            &["-t", "prod"],
            &["-o", "User=x", "--", "prod"],
            &["-p", "2222", "-J", "jump", "-L", "8080:x:80", "prod"],
            &["prod", "-p", "2222", "-v"],
            &["-p2222", "-4v", "deploy@10.0.0.5"],
            &["ssh://deploy@prod:2222"],
            &["-i", "~/.ssh/k", "-l", "root", "prod"],
        ] {
            let args = words(args);
            let control = Control {
                socket: PathBuf::from("/tmp/bateri-501/0a1b2c3d/u-0123456789abcdef"),
            };
            let tab = PaneUuid::parse("0F1E2D3C-4B5A-6978-8796-A5B4C3D2E1F0").unwrap();
            for parent in [None, Some(7), Some(u32::MAX)] {
                for control in [None, Some(&control)] {
                    // The identity's words unwrap with the rest.
                    for tab in [None, Some(&tab)] {
                        let wrapped = wrap(&args, BOOT_STUB, parent, NONCE, tab, control);
                        assert_eq!(wrapped.first().map(String::as_str), Some("-t"));
                        assert_eq!(unwrap(&wrapped), &args[..], "{wrapped:?}");
                        assert_eq!(nonce(&wrapped), Some(NONCE), "{wrapped:?}");
                    }
                }
            }
            // An argv that was never wrapped is itself, without a nonce.
            assert_eq!(unwrap(&args), &args[..]);
            assert_eq!(nonce(&args), None);
        }
    }

    #[test]
    fn unwrap_reads_positions_not_contents() {
        // The command somewhere other than last, or no leading `-t`: not ours.
        let command = remote_command(BOOT_STUB, None, NONCE, None);
        for args in [
            vec!["prod".to_owned(), command.clone()],
            vec!["-v".to_owned(), "prod".to_owned(), command.clone()],
            vec!["-t".to_owned(), command.clone(), "prod".to_owned()],
            // A user command between the target and ours.
            vec![
                "-t".to_owned(),
                "prod".to_owned(),
                "tmux".to_owned(),
                command.clone(),
            ],
            // A look-alike that is not the exact form.
            words(&["-t", "prod", "exec sh -c 'x' other"]),
            words(&["-t", "prod", "exec sh -c 'x' bateri-boot 7x"]),
            words(&["-t", "prod", "exec sh -c 'x' bateri-boot 7 8"]),
            // A nonce of another form, or a third word.
            words(&[
                "-t",
                "prod",
                "exec sh -c 'x' bateri-boot - 0123456789ABCDEF",
            ]),
            words(&["-t", "prod", "exec sh -c 'x' bateri-boot - 0123"]),
            words(&[
                "-t",
                "prod",
                "exec sh -c 'x' bateri-boot x 0123456789abcdef",
            ]),
            words(&[
                "-t",
                "prod",
                "exec sh -c 'x' bateri-boot 7 0123456789abcdef 9",
            ]),
            // The identity's words only in their own forms: a
            // version outside `[0-9A-Za-z.+-]`, a tab that is not
            // `bateri://tab/<uuid>` or `-`, a fifth word.
            words(&[
                "-t",
                "prod",
                "exec sh -c 'x' bateri-boot 7 0123456789abcdef 1;x -",
            ]),
            words(&[
                "-t",
                "prod",
                "exec sh -c 'x' bateri-boot 7 0123456789abcdef 1.0 bateri://tab/x",
            ]),
            words(&[
                "-t",
                "prod",
                "exec sh -c 'x' bateri-boot 7 0123456789abcdef 1.0 - -",
            ]),
            // The sharing options before `-t`: not the wrapped shape.
            words(&[
                "-o",
                "ControlMaster=auto",
                "-o",
                "ControlPath=/s",
                "-o",
                "ControlPersist=2",
                "-t",
                "prod",
                "exec sh -c 'x' bateri-boot",
            ]),
        ] {
            assert_eq!(unwrap(&args), &args[..], "{args:?}");
            assert_eq!(nonce(&args), None, "{args:?}");
        }
        // The oldest forms (an older build's session) still unwrap, without a nonce.
        for last in ["exec sh -c 'x' bateri-boot", "exec sh -c 'x' bateri-boot 7"] {
            let args = words(&["-t", "prod", last]);
            assert_eq!(unwrap(&args), &words(&["prod"])[..], "{last}");
            assert_eq!(nonce(&args), None, "{last}");
        }
    }

    #[test]
    fn the_remote_command_carries_the_identity() {
        // The version and the tab after the nonce, the placeholder
        // without a tab; the workspace version is one `is_version` takes.
        assert!(is_version(TERM_PROGRAM_VERSION), "{TERM_PROGRAM_VERSION}");
        assert!(!is_version("") && !is_version("1 0") && !is_version("1!"));
        let tab = PaneUuid::parse("0F1E2D3C-4B5A-6978-8796-A5B4C3D2E1F0").unwrap();
        assert!(
            remote_command("x", Some(7), NONCE, Some(&tab)).ends_with(&format!(
                " bateri-boot 7 {NONCE} {TERM_PROGRAM_VERSION} bateri://tab/{}",
                tab.as_str()
            ))
        );
        assert!(
            remote_command("x", None, NONCE, None)
                .ends_with(&format!(" bateri-boot - {NONCE} {TERM_PROGRAM_VERSION} -"))
        );
        // The older form (no identity) still unwraps with its nonce.
        let args = words(&[
            "-t",
            "prod",
            "exec sh -c 'x' bateri-boot - 0123456789abcdef",
        ]);
        assert_eq!(unwrap(&args), &words(&["prod"])[..]);
        assert_eq!(nonce(&args), Some(NONCE));
    }

    #[test]
    fn a_nonce_is_fresh_hex() {
        let first = new_nonce().expect("/dev/urandom");
        assert!(is_nonce(&first), "{first}");
        assert_ne!(new_nonce().as_deref(), Some(first.as_str()));
        // Without a parent the placeholder keeps the nonce at `$2`.
        assert!(
            remote_command("x", None, NONCE, None).contains(" bateri-boot - 0123456789abcdef ")
        );
        assert!(
            remote_command("x", Some(7), NONCE, None).contains(" bateri-boot 7 0123456789abcdef ")
        );
    }

    #[test]
    fn the_wrapped_call_still_reads_as_the_users() {
        // The remote probe sees the wrapped process; its target is the typed one.
        let wrapped = wrap(
            &words(&["-o", "User=x", "--", "prod"]),
            BOOT_STUB,
            None,
            NONCE,
            None,
            None,
        );
        let call = ssh_call("ssh", &wrapped).expect("interactive with -t");
        assert!(call.command, "the bootstrap is a remote command");
        assert_eq!(call.host, "prod");
    }

    #[test]
    fn session_lines_decide_r1_2() {
        let plain = "user u\nhostname h\nport 22\nrequesttty auto\nsessiontype default\n";
        assert!(Session::parse(plain).allows_wrap());
        for refused in [
            "remotecommand tmux attach\n",
            "requesttty false\n",
            "requesttty no\n",
            "sessiontype none\n",
            "sessiontype subsystem\n",
        ] {
            assert!(
                !Session::parse(&format!("{plain}{refused}")).allows_wrap(),
                "{refused}"
            );
        }
        assert!(Session::parse("requesttty force\nremotecommand none\n").allows_wrap());
    }

    #[test]
    fn the_key_is_the_resolved_triple() {
        assert_eq!(
            host_key("user deploy\nhostname 10.0.0.5\nport 22\n").as_deref(),
            Some("deploy@10.0.0.5:22")
        );
        assert_eq!(host_key("user deploy\nhostname h\nport x\n"), None);
        assert_eq!(host_key("hostname h\nport 22\n"), None);
    }

    #[test]
    fn state_rows_round_trip_and_skip_malformed_lines() {
        let text = "posix\ta@h:22\t100\nbogus\tx\t1\nposix\t\t3\ntouched\ta@h:22\tnot\n\
                    touched\tb@h:22\t5\textra\ntouched\tb@h:22\t7\nplain\tr@h:22\t8\n";
        let state = HostState::parse(text);
        assert!(state.knows(Fact::Posix, "a@h:22"));
        assert!(!state.knows(Fact::Touched, "a@h:22"));
        assert!(state.knows(Fact::Touched, "b@h:22"));
        assert!(state.knows(Fact::Plain, "r@h:22"));
        assert!(!state.knows(Fact::Plain, "b@h:22"));
        assert_eq!(
            state.render(),
            "posix\ta@h:22\t100\ntouched\tb@h:22\t7\nplain\tr@h:22\t8\n"
        );
        let mut state = state;
        state.note(Fact::Touched, "b@h:22", 9);
        state.note(Fact::Posix, "c@h:22", 10);
        assert_eq!(
            HostState::parse(&state.render()),
            state,
            "one row per fact and key"
        );
        assert_eq!(state.rows.len(), 4);
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("bt-wrap-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp directory");
        dir
    }

    #[test]
    fn the_state_file_is_written_under_its_lock() {
        let dir = scratch("lock");
        let path = dir.join("state").join("remote-hosts");
        assert_eq!(
            load(&path),
            HostState::default(),
            "a missing file knows nothing"
        );
        record(&path, Fact::Posix, "a@h:22", 1, LOCK_PATIENCE).expect("first write");
        record(&path, Fact::Touched, "a@h:22", 2, LOCK_PATIENCE).expect("second write");
        assert!(load(&path).knows(Fact::Posix, "a@h:22"));
        assert!(load(&path).knows(Fact::Touched, "a@h:22"));

        // Another writer holds the lock: the record gives up, the reader still reads.
        let held = lock(&path, LOCK_PATIENCE).expect("lock");
        let refused = record(&path, Fact::Posix, "b@h:22", 3, Duration::from_millis(30));
        assert_eq!(
            refused.map_err(|err| err.kind()),
            Err(io::ErrorKind::WouldBlock)
        );
        assert!(load(&path).knows(Fact::Posix, "a@h:22"));
        drop(held);
        record(&path, Fact::Posix, "b@h:22", 3, LOCK_PATIENCE).expect("after release");
        assert!(load(&path).knows(Fact::Posix, "b@h:22"));
        // No temporary is left behind.
        let names: Vec<_> = fs::read_dir(path.parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(names.len(), 2, "{names:?}");

        // An unreadable file (a directory in its place) knows nothing.
        let unreadable = dir.join("dir");
        fs::create_dir_all(&unreadable).unwrap();
        assert_eq!(load(&unreadable), HostState::default());
        fs::remove_dir_all(&dir).unwrap();
    }

    /// Answers `ssh -G` from a fixed text and counts the calls.
    struct Gconfig {
        out: &'static str,
        calls: RefCell<Vec<Vec<String>>>,
    }

    impl Gconfig {
        fn new(out: &'static str) -> Self {
            Self {
                out,
                calls: RefCell::new(Vec::new()),
            }
        }
    }

    impl SshRunner for Gconfig {
        fn run(&self, argv: &[String]) -> io::Result<(Option<i32>, String, String)> {
            self.calls.borrow_mut().push(argv.to_vec());
            Ok((Some(0), self.out.to_owned(), String::new()))
        }
    }

    const PLAIN: &str = "user u\nhostname h\nport 22\nrequesttty auto\nsessiontype default\n";

    fn learned() -> HostState {
        let mut state = HostState::default();
        state.note(Fact::Posix, "u@h:22", 1);
        state
    }

    #[test]
    fn decide_wraps_only_when_every_condition_holds() {
        let settings = Settings::default();
        let runner = Gconfig::new(PLAIN);
        let args = words(&["prod"]);
        assert_eq!(
            decide(
                &args,
                true,
                &settings,
                &runner,
                &learned(),
                BOOT_STUB,
                None,
                NONCE,
                None,
                &[]
            ),
            Some(Wrapped {
                args: wrap(&args, BOOT_STUB, None, NONCE, None, None),
                key: "u@h:22".to_owned(),
            })
        );
        assert_eq!(
            runner.calls.borrow().last(),
            Some(&words(&["ssh", "-G", "prod"])),
            "ssh -G sees the user's arguments"
        );

        // No terminal, a remote command (forced tty too), a non-interactive flag.
        let none = |args: &[&str], tty: bool| {
            let runner = Gconfig::new(PLAIN);
            let wrapped = decide(
                &words(args),
                tty,
                &settings,
                &runner,
                &learned(),
                BOOT_STUB,
                None,
                NONCE,
                None,
                &[],
            );
            (wrapped, runner.calls.borrow().len())
        };
        assert_eq!(none(&["prod"], false), (None, 0));
        for args in [
            &["prod", "uptime"][..],
            &["-t", "prod", "tmux", "attach"],
            &["-N", "prod"],
            &["-T", "prod"],
            &["-W", "x:22", "prod"],
            &["-O", "check", "prod"],
            &["-o", "RequestTTY=no", "prod"],
            &["-V"],
        ] {
            assert_eq!(none(args, true), (None, 0), "{args:?}: no ssh -G either");
        }
        // A bootstrap that is empty or would not survive a login shell's
        // quoting is never sent.
        for boot in ["", "it's", "a\\b", "hi!", "two\nlines"] {
            assert_eq!(
                decide(
                    &args,
                    true,
                    &settings,
                    &runner,
                    &learned(),
                    boot,
                    None,
                    NONCE,
                    None,
                    &[]
                ),
                None,
                "{boot:?}"
            );
        }
    }

    #[test]
    fn decide_asks_the_config_the_setting_and_the_state() {
        let args = words(&["prod"]);
        let on = Settings::default();
        // The config runs something else.
        for out in [
            "user u\nhostname h\nport 22\nremotecommand tmux\n",
            "user u\nhostname h\nport 22\nrequesttty false\n",
            "user u\nhostname h\nport 22\nsessiontype none\n",
        ] {
            let runner = Gconfig::new(out);
            assert_eq!(
                decide(
                    &args,
                    true,
                    &on,
                    &runner,
                    &learned(),
                    BOOT_STUB,
                    None,
                    NONCE,
                    None,
                    &[]
                ),
                None,
                "{out}"
            );
        }
        // An unknown server is wrapped, one recorded `plain` is not
        // — `posix` or not.
        let runner = Gconfig::new(PLAIN);
        let with = |state: &HostState| {
            decide(
                &args,
                true,
                &on,
                &runner,
                state,
                BOOT_STUB,
                None,
                NONCE,
                None,
                &[],
            )
            .map(|wrapped| wrapped.key)
        };
        assert_eq!(with(&HostState::default()), Some("u@h:22".to_owned()));
        let mut plain = learned();
        plain.note(Fact::Plain, "u@h:22", 2);
        assert_eq!(with(&plain), None);
        let mut other = HostState::default();
        other.note(Fact::Plain, "v@h:22", 2);
        assert_eq!(with(&other), Some("u@h:22".to_owned()));
        // The setting is asked before `ssh -G`.
        let off = Settings {
            remote_integration: false,
            ..Settings::default()
        };
        let runner = Gconfig::new(PLAIN);
        assert_eq!(
            decide(
                &args,
                true,
                &off,
                &runner,
                &learned(),
                BOOT_STUB,
                None,
                NONCE,
                None,
                &[]
            ),
            None
        );
        assert!(runner.calls.borrow().is_empty());
        let production = Settings {
            remote_hosts: vec![HostRule {
                pattern: "prod".to_owned(),
                mark: Some(HostMark::Production),
                integration: None,
            }],
            ..Settings::default()
        };
        assert_eq!(
            decide(
                &args,
                true,
                &production,
                &runner,
                &learned(),
                BOOT_STUB,
                None,
                NONCE,
                None,
                &[],
            ),
            None
        );
    }

    #[test]
    fn the_subcommand_prints_nul_separated_args_or_nothing() {
        let dir = scratch("main");
        let path = dir.join("remote-hosts");
        record(&path, Fact::Posix, "u@h:22", 1, LOCK_PATIENCE).unwrap();
        let runner = Gconfig::new(PLAIN);
        let run = |argv: &[&str], boot: &str| {
            let mut out = Vec::new();
            let code = ssh_argv_main(
                &words(argv),
                &Settings::default(),
                &runner,
                &path,
                &[],
                boot,
                Some(NONCE),
                None,
                &mut out,
            );
            assert_eq!(code, 0);
            out
        };
        // `--block` carries the parent; a malformed one wraps without it.
        for (block, parent) in [("7", Some(7)), ("", None), ("x", None)] {
            assert_eq!(
                run(&["--tty", "--block", block, "--", "prod"], BOOT_STUB),
                wrap(&words(&["prod"]), BOOT_STUB, parent, NONCE, None, None)
                    .iter()
                    .flat_map(|arg| arg.bytes().chain([0]))
                    .collect::<Vec<u8>>(),
                "{block:?}"
            );
        }
        let printed = run(&["--tty", "--", "-p", "2", "prod"], BOOT_STUB);
        let expected: Vec<u8> = wrap(
            &words(&["-p", "2", "prod"]),
            BOOT_STUB,
            None,
            NONCE,
            None,
            None,
        )
        .iter()
        .flat_map(|arg| arg.bytes().chain([0]))
        .collect();
        assert_eq!(printed, expected);
        assert!(load(&path).knows(Fact::Touched, "u@h:22"));
        // Without the terminal bit, without `--`, without a bootstrap: nothing.
        assert!(run(&["--", "prod"], BOOT_STUB).is_empty());
        assert!(run(&["--tty", "prod"], BOOT_STUB).is_empty());
        assert!(run(&["--tty", "--", "prod"], "").is_empty());
        // The touched row cannot be written (another writer holds the lock past
        // the patience): nothing either.
        let held = lock(&path, LOCK_PATIENCE).unwrap();
        assert!(run(&["--tty", "--", "prod"], BOOT_STUB).is_empty());
        drop(held);
        fs::remove_dir_all(&dir).unwrap();
    }

    /// With bateri's instance directory, the session becomes a
    /// master there — unless the user shares connections themselves.
    #[test]
    fn the_session_becomes_a_master_in_the_instance_directory() {
        let root = PathBuf::from(format!("/tmp/bt-wrap-ctl-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let roots = vec![root.join("r")];
        let dir = crate::ssh_route::prepare_instance(&roots[0], "0a1b2c3d").unwrap();
        let path = root.join("remote-hosts");
        record(&path, Fact::Posix, "u@h:22", 1, LOCK_PATIENCE).unwrap();
        let run = |config: &'static str, argv: &[&str]| {
            let runner = Gconfig::new(config);
            let mut out = Vec::new();
            ssh_argv_main(
                &words(argv),
                &Settings::default(),
                &runner,
                &path,
                &roots,
                BOOT_STUB,
                Some(NONCE),
                None,
                &mut out,
            );
            let mut args: Vec<String> = out
                .split(|&byte| byte == 0)
                .map(|arg| String::from_utf8(arg.to_vec()).unwrap())
                .collect();
            args.pop();
            args
        };
        let key = crate::ssh_route::host_key(&parse_config(PLAIN).unwrap());
        let control = Control {
            socket: dir.join(format!("u-{key}")),
        };
        let wrapped = run(PLAIN, &["--tty", "--instance", "0a1b2c3d", "--", "prod"]);
        assert_eq!(
            wrapped,
            wrap(
                &words(&["prod"]),
                BOOT_STUB,
                None,
                NONCE,
                None,
                Some(&control)
            )
        );
        assert_eq!(
            wrapped[1..7],
            words(&[
                "-o",
                "ControlMaster=auto",
                "-o",
                &format!("ControlPath={}", control.socket.display()),
                "-o",
                "ControlPersist=2",
            ])
        );
        assert_eq!(unwrap(&wrapped), &words(&["prod"])[..]);
        // The block and the instance together, in the function's order.
        assert_eq!(
            run(
                PLAIN,
                &[
                    "--tty",
                    "--block",
                    "4",
                    "--instance",
                    "0a1b2c3d",
                    "--",
                    "prod"
                ]
            ),
            wrap(
                &words(&["prod"]),
                BOOT_STUB,
                Some(4),
                NONCE,
                None,
                Some(&control)
            )
        );
        // The user's own sharing, an unknown or a missing instance: no sharing,
        // still wrapped.
        let plain = wrap(&words(&["prod"]), BOOT_STUB, None, NONCE, None, None);
        for config in [
            "user u\nhostname h\nport 22\ncontrolmaster auto\n",
            "user u\nhostname h\nport 22\ncontrolpath /u/cm-%C\n",
        ] {
            assert_eq!(
                run(config, &["--tty", "--instance", "0a1b2c3d", "--", "prod"]),
                plain,
                "{config}"
            );
        }
        for instance in ["ffffffff", "../r", "x"] {
            assert_eq!(
                run(PLAIN, &["--tty", "--instance", instance, "--", "prod"]),
                plain,
                "{instance}"
            );
        }
        // The user's own choice on the command line, an opt-out included
        // (`ssh -G` prints that as the default): no sharing of ours.
        for typed in [
            &["-o", "ControlMaster=no", "prod"][..],
            &["-oControlMaster=no", "prod"],
            &["-o", "controlpath none", "prod"],
            &["-S", "none", "prod"],
            &["-Snone", "prod"],
            &["prod", "-o", "ControlPersist=no"],
        ] {
            let mut argv = vec!["--tty", "--instance", "0a1b2c3d", "--"];
            argv.extend_from_slice(typed);
            assert_eq!(
                run(PLAIN, &argv),
                wrap(&words(typed), BOOT_STUB, None, NONCE, None, None),
                "{typed:?}"
            );
        }
        fs::remove_dir_all(&root).unwrap();
    }

    /// The fallback's table — 255 says nothing about the shell,
    /// a `posix` server's session was the user's, a touched server without
    /// `posix` reruns plain with the wrapped call's sharing options.
    #[test]
    fn the_fallback_reruns_only_a_touched_server_without_posix() {
        let args = words(&["-p", "2222", "prod"]);
        let touched = |posix: bool| {
            let mut state = HostState::default();
            state.note(Fact::Touched, "u@h:22", 1);
            if posix {
                state.note(Fact::Posix, "u@h:22", 2);
            }
            state
        };
        let plain = Some(FellBack {
            args: args.clone(),
            key: "u@h:22".to_owned(),
        });
        for rc in [0, 1, 127, 128] {
            assert_eq!(
                fell_back(&args, rc, false, PLAIN, &touched(false), &[]),
                plain,
                "{rc}"
            );
            assert_eq!(
                fell_back(&args, rc, false, PLAIN, &touched(true), &[]),
                None,
                "{rc}"
            );
        }
        for rc in [SSH_FAILURE, 129, 130, 143] {
            assert_eq!(
                fell_back(&args, rc, false, PLAIN, &touched(false), &[]),
                None,
                "{rc}: ssh's own error or a signal"
            );
        }
        // After the login: 255 is an endpoint that
        // refused our command — the rerun; a signal still says nothing, and
        // `posix` still wins.
        assert_eq!(
            fell_back(&args, SSH_FAILURE, true, PLAIN, &touched(false), &[]),
            plain
        );
        assert_eq!(
            fell_back(&args, SSH_FAILURE, true, PLAIN, &touched(true), &[]),
            None
        );
        for rc in [129, 130, 143] {
            assert_eq!(
                fell_back(&args, rc, true, PLAIN, &touched(false), &[]),
                None,
                "{rc}"
            );
        }
        assert_eq!(
            fell_back(&args, 1, false, PLAIN, &HostState::default(), &[]),
            None,
            "never wrapped"
        );
        assert_eq!(
            fell_back(&args, 1, false, "garbage", &touched(false), &[]),
            None
        );

        // With the instance directory: the same `Control` the wrapped call got.
        let root = PathBuf::from(format!("/tmp/bt-wrap-fb-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let dir = crate::ssh_route::prepare_instance(&root, "0a1b2c3d").unwrap();
        let sockets = vec![dir.clone()];
        let wrapped = decide(
            &args,
            true,
            &Settings::default(),
            &Gconfig::new(PLAIN),
            &learned(),
            BOOT_STUB,
            None,
            NONCE,
            None,
            &sockets,
        )
        .expect("wrapped");
        let rerun = fell_back(&args, 1, false, PLAIN, &touched(false), &sockets).expect("rerun");
        assert_eq!(
            rerun.args[..6],
            wrapped.args[1..7],
            "the wrapped call's options"
        );
        assert_eq!(rerun.args[6..], args[..]);
        assert_eq!(unwrap(&rerun.args), &rerun.args[..], "not wrapped");
        // The user's own sharing choice: none of ours, as in `decide`.
        let typed = words(&["-S", "none", "prod"]);
        assert_eq!(
            fell_back(&typed, 1, false, PLAIN, &touched(false), &sockets).map(|fb| fb.args),
            Some(typed.clone())
        );
        fs::remove_dir_all(&root).unwrap();
    }

    /// The subcommand: it is given the **wrapped**
    /// arguments and its first question is the attempt's own proof.
    #[test]
    fn the_fallback_subcommand_records_plain_and_prints_the_rerun() {
        let dir = scratch("fell");
        let path = dir.join("remote-hosts");
        let runner = Gconfig::new(PLAIN);
        let wrapped = wrap(&words(&["prod"]), BOOT_STUB, None, NONCE, None, None);
        let call = |head: &[&str]| -> Vec<String> {
            let mut argv = words(head);
            argv.extend(wrapped.iter().cloned());
            argv
        };
        let run = |argv: &[String], patience: Duration| {
            let mut out = Vec::new();
            let code = ssh_fell_back_main(argv, &runner, &path, &[], patience, &mut out);
            assert_eq!(code, 0);
            out
        };
        let nul = |args: &[&str]| -> Vec<u8> {
            args.iter().flat_map(|arg| arg.bytes().chain([0])).collect()
        };
        // Never wrapped: nothing, and no row.
        assert!(run(&call(&["--rc", "1", "--"]), Duration::ZERO).is_empty());
        record(&path, Fact::Touched, "u@h:22", 1, LOCK_PATIENCE).unwrap();
        // ssh's own error or a signal (Ctrl-C at the prompt): nothing, and
        // not even `ssh -G`.
        let calls = runner.calls.borrow().len();
        assert!(run(&call(&["--rc", "255", "--"]), Duration::ZERO).is_empty());
        assert!(run(&call(&["--rc", "130", "--"]), Duration::ZERO).is_empty());
        assert_eq!(runner.calls.borrow().len(), calls);
        assert!(!load(&path).knows(Fact::Plain, "u@h:22"));
        // Malformed calls, and arguments that are not ours (no nonce — the
        // user's own, an older build's wrap): nothing.
        let old = {
            let mut old = wrapped.clone();
            let last = old.last_mut().unwrap();
            *last = last.replace(
                &format!(" {NO_PARENT} {NONCE} {TERM_PROGRAM_VERSION} {NO_TAB}"),
                "",
            );
            old
        };
        for argv in [
            call(&["--"]),
            call(&["--rc", "x", "--"]),
            call(&["--rc", "1"]),
            words(&["--rc", "1", "--", "prod"]),
            [words(&["--rc", "1", "--"]), old].concat(),
        ] {
            assert!(run(&argv, Duration::ZERO).is_empty(), "{argv:?}");
        }
        assert!(!load(&path).knows(Fact::Plain, "u@h:22"));
        // The attempt's `up` was seen: nothing — whatever the state file says
        // (`touched`, no `posix`) — and the mark is consumed.
        mark_up(&path, NONCE).unwrap();
        assert!(run(&call(&["--rc", "0", "--"]), Duration::ZERO).is_empty());
        assert!(!up_dir(&path).join(NONCE).exists());
        assert!(!load(&path).knows(Fact::Plain, "u@h:22"));
        // The user typed after the login: nothing either, consumed.
        mark_used(&path, NONCE).unwrap();
        assert!(run(&call(&["--rc", "0", "--"]), Duration::ZERO).is_empty());
        assert!(!up_dir(&path).join(format!("{NONCE}{USED_SUFFIX}")).exists());
        assert!(!load(&path).knows(Fact::Plain, "u@h:22"));
        // Another attempt's `up` or input proves nothing about this one.
        mark_up(&path, "ffffffffffffffff").unwrap();
        mark_used(&path, "eeeeeeeeeeeeeeee").unwrap();
        // A 255 after the login, with the attempt's `up` or input: nothing
        // either; every proof is consumed.
        mark_login(&path, NONCE).unwrap();
        mark_up(&path, NONCE).unwrap();
        assert!(run(&call(&["--rc", "255", "--"]), Duration::ZERO).is_empty());
        assert!(
            !up_dir(&path)
                .join(format!("{NONCE}{LOGIN_SUFFIX}"))
                .exists()
        );
        // Another attempt's login says nothing about this 255.
        mark_login(&path, "dddddddddddddddd").unwrap();
        assert!(run(&call(&["--rc", "255", "--"]), Duration::ZERO).is_empty());
        assert!(!load(&path).knows(Fact::Plain, "u@h:22"));
        // A signal after the login still says nothing.
        mark_login(&path, NONCE).unwrap();
        assert!(run(&call(&["--rc", "130", "--"]), Duration::ZERO).is_empty());
        assert!(!load(&path).knows(Fact::Plain, "u@h:22"));
        // 255 after the login, no `up`, no input — an endpoint that refused
        // our command: `plain` and the rerun.
        mark_login(&path, NONCE).unwrap();
        assert_eq!(
            run(&call(&["--rc", "255", "--"]), Duration::ZERO),
            nul(&["prod"])
        );
        assert!(load(&path).knows(Fact::Plain, "u@h:22"));
        rewrite(&path, LOCK_PATIENCE, |state| {
            state.forget(Fact::Plain, "u@h:22")
        })
        .unwrap();
        // Without our master a 255 waits for the pane's login mark: one that
        // lands within the patience still falls back.
        {
            let writer = {
                let path = path.clone();
                thread::spawn(move || {
                    thread::sleep(Duration::from_millis(100));
                    mark_login(&path, NONCE).unwrap();
                })
            };
            assert_eq!(
                run(&call(&["--rc", "255", "--"]), Duration::from_secs(2)),
                nul(&["prod"])
            );
            writer.join().unwrap();
            rewrite(&path, LOCK_PATIENCE, |state| {
                state.forget(Fact::Plain, "u@h:22")
            })
            .unwrap();
        }
        // The wrapped call's master listening is the same login proof — and
        // only the attempt's own socket counts.
        {
            let socket = dir.join("u-key");
            let control = Control {
                socket: socket.clone(),
            };
            let mastered = wrap(
                &words(&["prod"]),
                BOOT_STUB,
                None,
                NONCE,
                None,
                Some(&control),
            );
            let mut argv = words(&["--rc", "255", "--"]);
            argv.extend(mastered);
            assert!(run(&argv, Duration::ZERO).is_empty(), "no master yet");
            let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
            assert_eq!(run(&argv, Duration::ZERO), nul(&["prod"]));
            drop(listener);
            fs::remove_file(&socket).unwrap();
            rewrite(&path, LOCK_PATIENCE, |state| {
                state.forget(Fact::Plain, "u@h:22")
            })
            .unwrap();
        }
        // Touched, not posix, no proof: `plain` is recorded and the rerun printed.
        assert_eq!(
            run(&call(&["--rc", "1", "--"]), Duration::ZERO),
            nul(&["prod"])
        );
        assert!(load(&path).knows(Fact::Plain, "u@h:22"));

        // A proof — or a `posix` row — that lands while the subcommand waits
        // wins: nothing.
        for late in ["up", "posix"] {
            let dir2 = scratch(&format!("fell-race-{late}"));
            let path2 = dir2.join("remote-hosts");
            record(&path2, Fact::Touched, "u@h:22", 1, LOCK_PATIENCE).unwrap();
            let writer = {
                let path2 = path2.clone();
                thread::spawn(move || {
                    thread::sleep(Duration::from_millis(100));
                    if late == "up" {
                        mark_up(&path2, NONCE).unwrap();
                    } else {
                        record(&path2, Fact::Posix, "u@h:22", 2, LOCK_PATIENCE).unwrap();
                    }
                })
            };
            let mut out = Vec::new();
            ssh_fell_back_main(
                &call(&["--rc", "0", "--"]),
                &runner,
                &path2,
                &[],
                Duration::from_secs(5),
                &mut out,
            );
            writer.join().unwrap();
            assert!(out.is_empty(), "the late {late} is seen");
            assert!(!load(&path2).knows(Fact::Plain, "u@h:22"));
            fs::remove_dir_all(&dir2).unwrap();
        }
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn base64_matches_the_standard_vectors() {
        for (plain, encoded) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64(plain.as_bytes()), encoded, "{plain:?}");
        }
    }

    /// The real bootstrap (not a stub) survives every login shell's quoting,
    /// is wrapped by `decide`, round-trips through `unwrap` and stays far
    /// below Linux's one-argument limit (`MAX_ARG_STRLEN`, 128 KiB) — the
    /// remote command is one argument on the server.
    #[test]
    fn the_real_bootstrap_is_one_quotable_line() {
        let boot = boot();
        assert!(
            is_inline(boot),
            "the one-liner breaks a login shell's quoting"
        );
        assert!(payload().starts_with(&format!("{MAGIC}\n")));
        let command = remote_command(boot, Some(u32::MAX), NONCE, None);
        assert!(command.len() < 64 * 1024, "{} bytes", command.len());
        let args = words(&["-p", "2222", "prod"]);
        let runner = Gconfig::new(PLAIN);
        let wrapped = decide(
            &args,
            true,
            &Settings::default(),
            &runner,
            &learned(),
            boot,
            None,
            NONCE,
            None,
            &[],
        )
        .expect("the real bootstrap wraps");
        assert_eq!(unwrap(&wrapped.args), &args[..]);
        // Every file the bootstrap writes is embedded, and the shared swap is
        // the local wrapper's own.
        assert!(REMOTE_FILES.iter().all(|(_, content)| !content.is_empty()));
        assert!(
            REMOTE_FILES
                .iter()
                .any(|(path, content)| *path == "zsh/zdotdir.zsh"
                    && *content == include_str!("../../../assets/shell/zsh/zdotdir.zsh"))
        );
    }

    #[test]
    fn the_payloads_literals_keep_every_byte() {
        let tricky = "it's $HOME \\ `x` \"q\"\n!";
        let out = std::process::Command::new("/bin/sh")
            .args(["-c", &format!("printf %s {}", crate::upload::sq(tricky))])
            .output()
            .expect("sh");
        assert_eq!(String::from_utf8(out.stdout).unwrap(), tricky);
    }

    /// The bootstrap's proof: one `posix` row per server, a
    /// second proof writes nothing, an argv `ssh -G` cannot read records
    /// nothing. The menu's toggle forgets a `plain` row the same way.
    #[test]
    fn the_proof_records_the_server_once_and_the_toggle_forgets_plain() {
        let dir = scratch("learn");
        let path = dir.join("remote-hosts");
        let ssh = words(&["/usr/bin/ssh", "-o", "ControlPath=/s/%C", "prod"]);
        let runner = Gconfig::new(PLAIN);
        assert!(record_posix(&runner, &ssh, &path).unwrap());
        assert_eq!(
            runner.calls.borrow().last().cloned(),
            Some(words(&[
                "/usr/bin/ssh",
                "-G",
                "-o",
                "ControlPath=/s/%C",
                "prod"
            ]))
        );
        assert!(load(&path).knows(Fact::Posix, "u@h:22"));
        let written = fs::read_to_string(&path).unwrap();
        assert!(
            !record_posix(&runner, &ssh, &path).unwrap(),
            "recorded already"
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), written);
        assert!(!record_posix(&Gconfig::new("garbage"), &ssh, &path).unwrap());
        assert!(!record_posix(&runner, &[], &path).unwrap());

        // Forgetting: only the `plain` row of that server goes.
        assert!(
            !forget_plain(&runner, &ssh, &path).unwrap(),
            "nothing to forget"
        );
        record(&path, Fact::Plain, "u@h:22", 2, LOCK_PATIENCE).unwrap();
        record(&path, Fact::Plain, "v@h:22", 2, LOCK_PATIENCE).unwrap();
        assert!(forget_plain(&runner, &ssh, &path).unwrap());
        let state = load(&path);
        assert!(!state.knows(Fact::Plain, "u@h:22"));
        assert!(state.knows(Fact::Plain, "v@h:22"));
        assert!(state.knows(Fact::Posix, "u@h:22"));
        assert!(!forget_plain(&Gconfig::new("garbage"), &ssh, &path).unwrap());
        fs::remove_dir_all(&dir).unwrap();
    }

    /// The seen nonces: a file per nonce beside the state file,
    /// consumed by the one question; only a nonce of our form names a file;
    /// an old unconsumed one is swept by the next mark.
    #[test]
    fn a_seen_nonce_is_marked_once_and_consumed() {
        let dir = scratch("up");
        let path = dir.join("remote-hosts");
        assert!(!take_up(&path, NONCE));
        mark_up(&path, NONCE).unwrap();
        assert!(up_dir(&path).join(NONCE).is_file());
        assert!(take_up(&path, NONCE));
        assert!(!take_up(&path, NONCE), "consumed");
        // The login mark is its own question.
        mark_login(&path, NONCE).unwrap();
        assert!(!take_up(&path, NONCE));
        assert!(take_login(&path, NONCE));
        assert!(!take_login(&path, NONCE), "consumed");
        assert!(mark_login(&path, "../x").is_err());
        // The "used" twin answers the same question; both go.
        mark_used(&path, NONCE).unwrap();
        assert!(take_up(&path, NONCE));
        mark_up(&path, NONCE).unwrap();
        mark_used(&path, NONCE).unwrap();
        assert!(take_up(&path, NONCE));
        assert!(!take_up(&path, NONCE), "both consumed");
        for bad in ["", "../../x", "0123456789ABCDEF", "abc"] {
            assert!(mark_up(&path, bad).is_err(), "{bad:?}");
            assert!(mark_used(&path, bad).is_err(), "{bad:?}");
            assert!(!take_up(&path, bad), "{bad:?}");
        }
        // A stale file (older than `UP_KEEP`) is swept by the next mark.
        let stale = up_dir(&path).join("ffffffffffffffff");
        let file = File::create(&stale).unwrap();
        file.set_modified(SystemTime::now() - UP_KEEP - Duration::from_secs(60))
            .unwrap();
        drop(file);
        mark_up(&path, NONCE).unwrap();
        assert!(!stale.exists());
        assert!(up_dir(&path).join(NONCE).exists());
        fs::remove_dir_all(&dir).unwrap();
    }
}
/// The bootstrap on real shells: what sshd does with the wrapped
/// call — `"$SHELL" -c '<remote command>'` — in a real PTY, with a temporary
/// home. Each login shell that is not installed here is `SKIPPED` (macOS has
/// no fish or BusyBox; `make linux`'s image has all five).
#[cfg(test)]
mod remote_shells {
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    use bt_core::{
        Blocks, CaretShape, CursorBlink, Osc52, RemoteSetupFault, Session, SessionOptions,
        TerminalOptions, Theme,
    };

    use super::{boot, remote_command};
    use crate::child::{SilentWake, wait_until};
    use crate::settings::TempRoot;

    /// The first `name` on `PATH`.
    fn which(name: &str) -> Option<PathBuf> {
        std::env::var_os("PATH").and_then(|path| {
            std::env::split_paths(&path)
                .map(|dir| dir.join(name))
                .find(|candidate| candidate.is_file())
        })
    }

    /// The local `ssh` block the harness's remote shells run under: the `P`
    /// of their `bt_remote=<P>.<S>.<n>` and `bateri://rblock/<P>.<S>.<n>`.
    const PARENT: u32 = 41;
    /// The attempt's nonce the simulated sshd passes.
    const NONCE: &str = "00c0ffee00c0ffee";

    /// The remote blocks end to end: the server's `true`,
    /// `false` and `sleep` get their stripes and the counter from our remote
    /// marks and anchors — drawn with no local block at all, i.e. from the
    /// remote trail alone (the harness has no local `ssh` block, so nothing
    /// is "running"; a finished block needs no parent to be open).
    fn draws_remote_blocks(session: &Session, name: &str) {
        let stripes = |session: &Session| -> Vec<bt_core::LinearRgba> {
            let mut blocks = Blocks::default();
            crate::child::tests::screen(session, &mut blocks);
            blocks.as_slice().iter().map(|block| block.stripe).collect()
        };
        session.write(b"false\n");
        wait_until(&format!("{name}: no error stripe"), || {
            stripes(session).contains(&Theme::BATERI.error_linear())
        });
        assert!(
            stripes(session).contains(&Theme::BATERI.success_linear()),
            "{name}: no success stripe"
        );
        session.write(b"sleep 1.5\n");
        wait_until(&format!("{name}: no counter"), || {
            crate::child::tests::screen(session, &mut Blocks::default())
                .iter()
                .any(|row| {
                    row.contains("sleep 1.5")
                        && ["1.5s", "1.6s", "1.7s", "1.8s", "1.9s"]
                            .iter()
                            .any(|counter| row.trim_end().ends_with(counter))
                })
        });
    }

    /// sshd's call: `shell -c '<the wrapped remote command>'`, `HOME` and
    /// `SHELL` as sshd sets them, `XDG_DATA_HOME` pinned under the home (the
    /// test machine's must not leak in), `extra` on top.
    fn sshd(shell: &Path, home: &Path, extra: &[(&str, String)]) -> Session {
        let mut env = HashMap::from([
            ("HOME".to_owned(), home.display().to_string()),
            ("SHELL".to_owned(), shell.display().to_string()),
            (
                "XDG_DATA_HOME".to_owned(),
                home.join(".local/share").display().to_string(),
            ),
        ]);
        for (key, value) in extra {
            env.insert((*key).to_owned(), value.clone());
        }
        Session::spawn(
            SessionOptions {
                command: Some((
                    shell.display().to_string(),
                    vec![
                        "-c".to_owned(),
                        remote_command(boot(), Some(PARENT), NONCE, None),
                    ],
                )),
                working_directory: Some(home.to_path_buf()),
                home: Some(home.to_path_buf()),
                env,
                cols: 120,
                rows: 40,
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
                journal: None,
            },
            Arc::new(SilentWake),
        )
        .expect("session did not open")
    }

    fn screen_has(session: &Session, text: &str) -> bool {
        crate::child::tests::screen(session, &mut Blocks::default())
            .iter()
            .any(|row| row.contains(text))
    }

    /// The remote folder bateri read, canonical (macOS's `/var` is a link).
    fn remote_folder(session: &Session) -> Option<PathBuf> {
        let folder = session.remote_link_directory();
        (!folder.is_empty())
            .then(|| std::fs::canonicalize(&folder).ok())
            .flatten()
    }

    /// The first line of this machine's motd, if it has one — the bootstrap
    /// must print it (sshd does not when it runs a command).
    fn motd_line() -> Option<String> {
        std::fs::read_to_string("/etc/motd")
            .ok()?
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .map(str::to_owned)
    }

    /// The user's files with their contents, to show they were not written.
    fn snapshot(home: &Path, files: &[&str]) -> Vec<(String, Option<String>)> {
        files
            .iter()
            .map(|file| {
                (
                    (*file).to_owned(),
                    std::fs::read_to_string(home.join(file)).ok(),
                )
            })
            .collect()
    }

    /// One integrated login shell end to end: the prompt comes, OSC 7 reports
    /// the folder (percent-encoded through a space) and follows `cd`, the
    /// user's login file ran, nothing of theirs was written, the motd was
    /// printed and no fault was reported. The open session and its root go
    /// back for the shell's own claims; `None` if the shell is not installed.
    fn integrates(name: &str, login: &str, rc: &[(&str, &str)]) -> Option<(Session, TempRoot)> {
        let Some(shell) = which(name) else {
            println!("SKIPPED: {name} is not installed");
            return None;
        };
        let root = TempRoot::new(&format!("remote-{name}"));
        let home = root.0.join("home");
        let spaced = home.join("a ğ");
        std::fs::create_dir_all(&spaced).expect("home");
        let home = std::fs::canonicalize(&home).expect("home");
        let mut files = vec![login];
        let write = |file: &str, text: &str| {
            let path = home.join(file);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).expect("rc directory");
            }
            std::fs::write(path, text).expect("rc file");
        };
        write(login, &format!("export SEEN=login-{name}\n"));
        for (file, text) in rc {
            write(file, text);
            files.push(file);
        }
        let before = snapshot(&home, &files);

        let session = sshd(&shell, &home, &[]);
        wait_until(&format!("{name}: no OSC 7 for the home"), || {
            remote_folder(&session).as_deref() == Some(home.as_path())
        });
        session.write("cd 'a ğ'\n".as_bytes());
        wait_until(&format!("{name}: OSC 7 did not follow cd"), || {
            remote_folder(&session).as_deref() == Some(home.join("a ğ").as_path())
        });
        session.write(b"echo \"[$SEEN]\"\n");
        wait_until(&format!("{name}: the login file did not run"), || {
            screen_has(&session, &format!("[login-{name}]"))
        });
        if let Some(line) = motd_line() {
            assert!(screen_has(&session, &line), "{name}: no motd ({line})");
        }
        assert_eq!(session.remote_setup_fault(), None, "{name}");
        assert_eq!(
            snapshot(&home, &files),
            before,
            "{name}: an rc file changed"
        );
        let shell_dir = home.join(".local/share/bateri/shell");
        assert!(shell_dir.join("zsh/zdotdir.zsh").is_file(), "{name}");
        Some((session, root))
    }

    fn close(session: &Session) {
        session.write(b"exit\n");
        session.shutdown();
    }

    #[test]
    fn zsh_gets_the_integration_through_its_own_files() {
        // The common shape: `~/.zshenv` moves the configuration with
        // `ZDOTDIR` and sets a plain (not exported) variable. sshd's first hop
        // (`zsh -c`) already read it, so the bootstrap must not take that
        // `ZDOTDIR` for the user's starting value: `~/.zshenv` runs again.
        let rc = [
            (".zshenv", "export ZDOTDIR=$HOME/cfg\nZSHENV_PLAIN=1\n"),
            ("cfg/.zshrc", "PS1='zsh> '\n"),
        ];
        if let Some((session, _root)) = integrates("zsh", "cfg/.zprofile", &rc) {
            session.write(b"echo \"[z=${ZSHENV_PLAIN-} ${ZDOTDIR:t}]\"\n");
            wait_until("zsh: ~/.zshenv skipped or ZDOTDIR lost", || {
                screen_has(&session, "[z=1 cfg]")
            });
            draws_remote_blocks(&session, "zsh");
            close(&session);
        }
    }

    /// bash: a login shell from bash 4 on (POSIX mode left, `ENV` gone, the
    /// history file bash's own and not exported), `--rcfile` before it.
    #[test]
    fn bash_gets_the_integration_as_a_login_shell() {
        let Some((session, _root)) = integrates(
            "bash",
            ".bash_profile",
            &[
                (".bashrc", "PS1='bash> '\n"),
                (".profile", "export SEEN=wrong\n"),
            ],
        ) else {
            return;
        };
        let major = std::process::Command::new("bash")
            .args(["-c", "echo ${BASH_VERSINFO[0]}"])
            .output()
            .ok()
            .and_then(|out| String::from_utf8(out.stdout).ok())
            .and_then(|out| out.trim().parse::<u32>().ok())
            .expect("bash's version");
        session.write(
            b"echo \"[$(shopt -q login_shell && echo login || echo plain) \
              $(shopt -qo posix && echo posix || echo bash) ${ENV-noenv} ${HISTFILE##*/} \
              $(env | grep -c '^HISTFILE=')]\"\n",
        );
        let kind = if major >= 4 { "login" } else { "plain" };
        let expected = format!("[{kind} bash noenv .bash_history 0]");
        wait_until(&format!("bash {major}: not {expected}"), || {
            screen_has(&session, &expected)
        });
        // Blocks need `PS0` (bash 4.4): before it there is no `C` to close the
        // anchor, so the script prints no block marks (`bateri.bash`).
        let ps0 = std::process::Command::new("bash")
            .args([
                "-c",
                "(( BASH_VERSINFO[0] * 100 + BASH_VERSINFO[1] >= 404 )) && echo y",
            ])
            .output()
            .is_ok_and(|out| out.stdout.starts_with(b"y"));
        if ps0 {
            draws_remote_blocks(&session, "bash");
        } else {
            println!("SKIPPED: bash {major} has no PS0, no remote blocks");
        }
        close(&session);
    }

    #[test]
    fn fish_gets_the_integration_through_vendor_conf() {
        if let Some((session, _root)) = integrates("fish", ".config/fish/config.fish", &[]) {
            // Our directory is out of `XDG_DATA_DIRS` again (it was unset).
            session.write(b"echo \"[x=$XDG_DATA_DIRS]\"\n");
            wait_until("fish: XDG_DATA_DIRS kept ours", || {
                screen_has(&session, "[x=]")
            });
            draws_remote_blocks(&session, "fish");
            close(&session);
        }
    }

    /// A login shell that is not zsh, bash or fish: a plain login shell (its
    /// own login file runs), the reason reported, nothing written.
    fn falls_back(name: &str, shell: &Path, extra: &[(&str, String)]) {
        let root = TempRoot::new(&format!("remote-plain-{name}"));
        let home = root.0.join("home");
        std::fs::create_dir_all(&home).expect("home");
        std::fs::write(home.join(".profile"), format!("export SEEN=login-{name}\n"))
            .expect(".profile");
        let session = sshd(shell, &home, extra);
        wait_until(&format!("{name}: no fault"), || {
            session.remote_setup_fault() == Some(RemoteSetupFault::Shell)
        });
        session.write(b"echo \"[$SEEN]\"\n");
        wait_until(&format!("{name}: no plain login shell"), || {
            screen_has(&session, &format!("[login-{name}]"))
        });
        assert!(
            !home.join(".local/share/bateri").exists(),
            "{name}: files written"
        );
        assert_eq!(session.remote_link_directory(), "", "{name}: no OSC 7");
        session.write(b"exit\n");
        session.shutdown();
    }

    #[test]
    fn dash_gets_a_plain_login_shell() {
        match which("dash") {
            Some(dash) => falls_back("dash", &dash, &[]),
            None => println!("SKIPPED: dash is not installed"),
        }
    }

    /// A BusyBox server: `sh`, the login shell (`ash`) and every tool the
    /// bootstrap reaches are BusyBox's applets.
    #[test]
    fn busybox_gets_a_plain_login_shell() {
        let Some(busybox) = which("busybox") else {
            println!("SKIPPED: busybox is not installed");
            return;
        };
        let root = TempRoot::new("remote-busybox-bin");
        let bin = root.0.join("bin");
        std::fs::create_dir_all(&bin).expect("bin");
        for applet in ["sh", "ash", "awk", "base64", "cat", "mkdir", "mv", "rm"] {
            std::os::unix::fs::symlink(&busybox, bin.join(applet)).expect("applet");
        }
        falls_back(
            "busybox",
            &bin.join("ash"),
            &[("PATH", bin.display().to_string())],
        );
    }

    /// The two failure arms: the files cannot be written (a file where the
    /// directory goes — read-only for root too) and no base64 decoder on
    /// `PATH`. Both give the plain login shell and the reason.
    #[test]
    fn a_failed_setup_is_a_plain_login_shell_with_the_reason() {
        let zsh = which("zsh").expect("zsh");
        for (arm, fault) in [
            ("write", RemoteSetupFault::Write),
            ("decode", RemoteSetupFault::Decode),
        ] {
            let root = TempRoot::new(&format!("remote-fault-{arm}"));
            let home = root.0.join("home");
            std::fs::create_dir_all(home.join(".local/share")).expect("home");
            std::fs::write(home.join(".zprofile"), "export SEEN=login-plain\n").expect(".zprofile");
            let mut extra = Vec::new();
            if arm == "write" {
                std::fs::write(home.join(".local/share/bateri"), "").expect("blocker");
            } else {
                // `sh` and `awk` only: no `base64`, `b64decode` or `openssl`.
                let bin = root.0.join("bin");
                std::fs::create_dir_all(&bin).expect("bin");
                for tool in ["sh", "awk"] {
                    let path = which(tool).expect(tool);
                    std::os::unix::fs::symlink(path, bin.join(tool)).expect("tool");
                }
                extra.push(("PATH", bin.display().to_string()));
            }
            let session = sshd(&zsh, &home, &extra);
            wait_until(&format!("{arm}: no fault"), || {
                session.remote_setup_fault() == Some(fault)
            });
            session.write(b"echo \"[$SEEN]\"\n");
            wait_until(&format!("{arm}: no plain login shell"), || {
                screen_has(&session, "[login-plain]")
            });
            assert_eq!(session.remote_link_directory(), "", "{arm}: no OSC 7");
            session.write(b"exit\n");
            session.shutdown();
        }
    }

    /// sshd's call without a terminal and with stdin at its end: the login
    /// shell the bootstrap `exec`s exits at once, and the bytes it printed are
    /// the raw stream — the PTY harness above sees only the screen and the
    /// ledger, and a mark of a session with no running command is not kept.
    fn raw_output(shell: &Path, home: &Path, extra: &[(&str, String)]) -> Vec<u8> {
        use std::io::Read as _;
        let mut child = std::process::Command::new(shell)
            .arg("-c")
            .arg(remote_command(boot(), Some(PARENT), NONCE, None))
            .env_clear()
            .env("HOME", home)
            .env("SHELL", shell)
            .env("XDG_DATA_HOME", home.join(".local/share"))
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .envs(extra.iter().map(|(key, value)| (*key, value.as_str())))
            .current_dir(home)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("the login shell");
        let mut stdout = child.stdout.take().expect("stdout");
        let reader = std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let _ = stdout.read_to_end(&mut bytes);
            bytes
        });
        let child = std::cell::RefCell::new(child);
        wait_until("the login shell did not exit", || {
            child.borrow_mut().try_wait().ok().flatten().is_some()
        });
        reader.join().expect("reader")
    }

    /// Every login shell the bootstrap `exec`s — the integration's
    /// (zsh) and the plain fault's (`sh`) — gets the terminal's identity from
    /// the wrapped command's words; a tab not of `bateri://tab/<uuid>`'s form
    /// and a version outside its alphabet are not exported. The login shell
    /// is a stand-in of the right name that prints the three and exits.
    #[test]
    fn the_bootstrap_exports_the_identity() {
        let sh = which("sh").expect("sh");
        let tab = bt_core::PaneUuid::parse("0F1E2D3C-4B5A-6978-8796-A5B4C3D2E1F0").unwrap();
        let version = bt_core::TERM_PROGRAM_VERSION;
        let good = remote_command(boot(), Some(PARENT), NONCE, Some(&tab));
        let bad = format!(
            "exec sh -c '{}' bateri-boot {PARENT} {NONCE} 1.0\\;x bateri://tab/0f1e",
            boot()
        );
        for name in ["zsh", "sh"] {
            let root = TempRoot::new(&format!("remote-identity-{name}"));
            let home = root.0.join("home");
            let bin = root.0.join("bin");
            std::fs::create_dir_all(home.join(".local/share")).expect("home");
            std::fs::create_dir_all(&bin).expect("bin");
            let shell = bin.join(name);
            std::fs::write(
                &shell,
                "#!/bin/sh\nprintf 'id=%s|%s|%s.' \"${LC_TERMINAL-unset}\" \
                 \"${LC_TERMINAL_VERSION-unset}\" \"${LC_BATERI_TAB_URL-unset}\"\n",
            )
            .expect("stand-in");
            std::fs::set_permissions(&shell, std::os::unix::fs::PermissionsExt::from_mode(0o755))
                .expect("chmod");
            let run = |command: &str| {
                let out = std::process::Command::new(&sh)
                    .arg("-c")
                    .arg(command)
                    .env_clear()
                    .env("HOME", &home)
                    .env("SHELL", &shell)
                    .env("XDG_DATA_HOME", home.join(".local/share"))
                    .env("PATH", std::env::var_os("PATH").unwrap_or_default())
                    // An inherited value of another terminal is overridden.
                    .env("LC_TERMINAL", "iTerm2")
                    .stdin(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .output()
                    .expect("sh");
                let text = String::from_utf8_lossy(&out.stdout).into_owned();
                text[text.find("id=").unwrap_or(0)..].to_owned()
            };
            assert_eq!(
                run(&good),
                format!("id=bateri|{version}|{}.", tab.url()),
                "{name}"
            );
            assert_eq!(run(&bad), "id=bateri|unset|unset.", "{name}");
        }
    }

    /// The attempt's `up` is the bootstrap's **first** output —
    /// before the motd and before every fault, the decode arm's included
    /// (the one-liner prints it there itself, then `decode`).
    #[test]
    fn the_bootstrap_says_up_before_anything_else() {
        let up = format!("\x1b]8133;i;up;{NONCE}\x07");
        let zsh = which("zsh").expect("zsh");
        let sh = which("sh").expect("sh");
        for arm in ["zsh", "shell", "write", "decode"] {
            let root = TempRoot::new(&format!("remote-up-{arm}"));
            let home = root.0.join("home");
            std::fs::create_dir_all(home.join(".local/share")).expect("home");
            let mut extra = Vec::new();
            let shell = if arm == "shell" { &sh } else { &zsh };
            if arm == "write" {
                std::fs::write(home.join(".local/share/bateri"), "").expect("blocker");
            }
            if arm == "decode" {
                let bin = root.0.join("bin");
                std::fs::create_dir_all(&bin).expect("bin");
                for tool in ["sh", "awk"] {
                    let path = which(tool).expect(tool);
                    std::os::unix::fs::symlink(path, bin.join(tool)).expect("tool");
                }
                extra.push(("PATH", bin.display().to_string()));
            }
            let out = raw_output(shell, &home, &extra);
            assert!(
                out.starts_with(up.as_bytes()),
                "{arm}: {:?}",
                String::from_utf8_lossy(&out[..out.len().min(200)])
            );
            let fault = match arm {
                "zsh" => None,
                "decode" => Some("decode"),
                "write" => Some("write"),
                _ => Some("shell"),
            };
            if let Some(fault) = fault {
                let after = format!("\x1b]8133;f;{fault}\x07");
                assert!(
                    out[up.len()..]
                        .windows(after.len())
                        .any(|window| window == after.as_bytes()),
                    "{arm}: the fault follows"
                );
            }
            assert_eq!(
                out.windows(up.len())
                    .filter(|w| *w == up.as_bytes())
                    .count(),
                1,
                "{arm}: once"
            );
        }
        // A nonce that is not lowercase hex is not printed.
        let root = TempRoot::new("remote-up-bad");
        let home = root.0.join("home");
        std::fs::create_dir_all(&home).expect("home");
        let out = std::process::Command::new(&sh)
            .arg("-c")
            .arg(format!("exec sh -c '{}' bateri-boot - 'a;b'", boot()))
            .env("HOME", &home)
            .env("SHELL", &sh)
            .stdin(std::process::Stdio::null())
            .output()
            .expect("sh");
        assert!(
            !out.stdout.windows(9).any(|w| w == b"8133;i;up"),
            "{:?}",
            String::from_utf8_lossy(&out.stdout)
        );
    }
}
