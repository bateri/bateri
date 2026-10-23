//! Remote shell integration's decision: whether the user's `ssh` shows a
//! terminal the server keeps (kablo) or is the plain `ssh`, the bootstrap that
//! terminal runs, and the readers of the wrapped form an older bateri's shell
//! functions produced.
//!
//! **One owner.** The local shells' `ssh` function runs `bateri ssh-hold`, and
//! the running bateri decides there ([`kablo_path`]): the call is interactive
//! (the same walk as the remote probe's, `jobs::ssh_call`) and runs no remote
//! command, the setting allows the host as typed
//! ([`Settings::integration_for`]), `ssh -G` does not say the session runs
//! something else ([`Session`]) and the server is neither [`Fact::Plain`] (no
//! shell to boot) nor [`Fact::NoKablo`] (kablo cannot be kept on it). What
//! fails any of them is the plain `ssh`, with nothing of bateri's on the
//! server and no connection of bateri's to it. [`kablo_argv`] is the
//! bootstrap the terminal runs there; the files its shell is set up from are
//! not in it but written beforehand, over the connection the user made
//! ([`files_script`]).
//!
//! **What stays of the wrapped form**, for one purpose: a shell started before
//! an update still has the previous `ssh` function, which asks `bateri
//! ssh-argv` and `bateri ssh-fell-back`. `ssh-argv` answers nothing (the
//! subcommand is in the shell crate: it prints no wrapped argv whatever the
//! call, and nothing is the plain `ssh`), so that shell's next `ssh` is a plain
//! one; [`ssh_fell_back_main`] answers an attempt that function wrapped before
//! the update and that ended after it. [`unwrap`] and [`nonce`] read the wrapped
//! argv of such an `ssh` that is still running, which the remote probe sees in
//! the process table.
//!
//! **The wrapped form is positional**: `-t` first,
//! the user's arguments unchanged, the bootstrap command last — one argument
//! that starts with a letter, so it is the remote command whether or not the
//! user ended the options with `--` (a second `--` after a terminated walk
//! would be sent to the remote shell). [`unwrap`] checks those positions, it
//! does not search for the command.
//!
//! **The session was a master**: when the user shared no
//! connections themselves (`ssh -G`, [`SshConfig::shares_connections`]) and
//! bateri's instance directory was there, the wrapped call also carried
//! `ControlMaster=auto` at [`session_socket`] with a short
//! [`SESSION_PERSIST`] ([`Control`]). Those three options sit right after `-t`,
//! again by position; the fallback gives its plain rerun the same ones.
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

/// The remote command of the wrapped form, for a bootstrap script — what an
/// older bateri's shell function ran, kept as the tests' maker of the argv
/// [`unwrap`] and [`nonce`] read:
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
/// `boot` must not contain `'`.
#[cfg(test)]
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

/// The connection sharing a wrapped session got: the session is a
/// master at `socket` (`ControlMaster=auto` — a second `ssh` to the host from
/// this bateri joins it), detached after [`SESSION_PERSIST`]. Ours come
/// **before** the user's arguments: ssh keeps a key's first value. Only for a
/// user who shares no connections themselves ([`control`]).
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

/// The user's ssh arguments (without the program) → the wrapped arguments an
/// older bateri's shell function ran: `-t`, [`Control`]'s options when there
/// are any, then `args` as they are, then [`remote_command`]. The user's own
/// `-t` stays (`-t -t` still asks for a terminal). Nothing makes this form any
/// more; the tests do, to prove [`unwrap`] and the fallback read it.
#[cfg(test)]
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
/// machine are learned once. Behind a jump host or a proxy command the route
/// follows (`deploy@10.0.0.5:22 via proxyjump bastion`), as it does in the
/// account: the same inner address through another bastion is another
/// server, and what one runs says nothing of the other; a direct host's key
/// is the triple it always was. `None` without a user, a readable port or a
/// host name, and for a value that would break a row (a tab or a line break).
pub fn host_key(out: &str) -> Option<String> {
    let account = Account::from_config(&parse_config(out)?)?;
    let mut key = format!("{}@{}:{}", account.user, account.host, account.port);
    if let Some(route) = &account.route {
        key.push_str(" via ");
        key.push_str(route);
    }
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
    /// kablo cannot be kept on the server: a platform it is not built for, a
    /// home directory that cannot be written, a file that will not run
    /// ([`crate::kablo::install`]). The server's `ssh` is a plain one from then
    /// on and kablo is not tried again. A reading build that does not know the
    /// name skips the row ([`HostState::parse`]), so an older bateri
    /// is unaffected by it.
    NoKablo,
}

impl Fact {
    fn name(self) -> &'static str {
        match self {
            Self::Posix => "posix",
            Self::Touched => "touched",
            Self::Plain => "plain",
            Self::NoKablo => "nokablo",
        }
    }

    fn from_name(name: &str) -> Option<Self> {
        match name {
            "posix" => Some(Self::Posix),
            "touched" => Some(Self::Touched),
            "plain" => Some(Self::Plain),
            "nokablo" => Some(Self::NoKablo),
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
/// server is tried for kablo and none counts as `posix`.
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

/// A call the integration may take: what the conditions learned on the way.
struct Eligible {
    call: crate::jobs::SshCall,
    /// `ssh -G`'s answer for the arguments.
    config: String,
    /// The server's key ([`host_key`]).
    key: String,
}

/// The questions that decide a call, **in this order** (the cheap ones first,
/// `ssh -G` only for a call that would be taken): the call is interactive and
/// runs no remote command, the setting allows the host as typed
/// ([`Settings::integration_for`]), `ssh -G` does not say the session runs
/// something else ([`Session::allows_wrap`]) and the server is none of the
/// facts that make `ssh` a plain one ([`Fact::Plain`], [`Fact::NoKablo`]).
fn eligible(
    args: &[String],
    settings: &Settings,
    runner: &dyn SshRunner,
    state: &HostState,
) -> Option<Eligible> {
    let call = ssh_call("ssh", args).filter(|call| !call.command)?;
    if !settings.integration_for(&call.host) {
        return None;
    }
    let config = config(runner, args)?;
    if !Session::parse(&config).allows_wrap() {
        return None;
    }
    let key = host_key(&config)?;
    if [Fact::Plain, Fact::NoKablo]
        .iter()
        .any(|fact| state.knows(*fact, &key))
    {
        return None;
    }
    Some(Eligible { call, config, key })
}

/// The server a call goes to as a kablo terminal ([`kablo_path`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KabloHost {
    /// As typed ([`crate::jobs::Target::host`]).
    pub host: String,
    /// The port the arguments name.
    pub port: Option<u16>,
    /// The arguments to run again ([`crate::jobs::Target::argv`]).
    pub argv: Vec<String>,
    /// `ssh -G`'s answer, for the account and the master's socket.
    pub config: String,
    /// The server's key ([`host_key`]): the account's name on the wire.
    pub key: String,
}

/// Whether the user's `ssh` shows a terminal kept on the server (kablo) and
/// not a plain connection, and to whom: an interactive call without a remote
/// command, the setting allowing the host as typed, `ssh -G` saying the
/// session is the ordinary one, and a server that is neither [`Fact::Plain`]
/// (it has no shell to boot) nor [`Fact::NoKablo`] (kablo cannot be kept on
/// it). A `production` host the setting refuses is plain, and so is a `ssh`
/// whose terminals are not both ends of a terminal — the caller's bit, which
/// the shell function checks before it runs this at all. `None` → the plain
/// `ssh`, and from then on **nothing of bateri's is on that connection**: it
/// is not a server pane, so the add-ons that work on a server (file drops,
/// previews, the load indicator, the ports, the ⌘ path check) are
/// not offered and bateri opens no connection of its own to the host.
pub fn kablo_path(
    args: &[String],
    settings: &Settings,
    runner: &dyn SshRunner,
    state: &HostState,
) -> Option<KabloHost> {
    let Eligible { call, config, key } = eligible(args, settings, runner, state)?;
    Some(KabloHost {
        host: call.host,
        port: call.port,
        argv: call.argv,
        config,
        key,
    })
}

/// The command a new kablo terminal runs: the bootstrap
/// ([`remote_command`]'s, as an argument vector — `new` takes no shell string),
/// so the terminal's shell is the integrated one and the dock, the blocks, `⇄`
/// and `LC_BATERI_TAB_URL` work as in a wrapped session. `cwd` is the
/// starting directory ([`boot_in`]), `parent` the local block of the `ssh`
/// command, `tab` the pane's identity; the nonce is the attempt's
/// ([`new_nonce`]), which here proves nothing (a terminal the server keeps is
/// not rerun) but is the bootstrap's `$2`. The shells' files are not in it: they
/// are on the server before it runs ([`files_script`]).
pub fn kablo_argv(
    cwd: Option<&str>,
    parent: Option<u32>,
    nonce: &str,
    tab: Option<&PaneUuid>,
) -> Vec<Vec<u8>> {
    let parent = parent.map_or_else(|| NO_PARENT.to_owned(), |parent| parent.to_string());
    let tab = tab.map_or_else(|| NO_TAB.to_owned(), PaneUuid::url);
    [
        "sh",
        "-c",
        &boot_in(cwd),
        BOOT_NAME,
        &parent,
        nonce,
        TERM_PROGRAM_VERSION,
        &tab,
    ]
    .iter()
    .map(|word| word.as_bytes().to_vec())
    .collect()
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

/// The agent's command (`assets/shell/remote/bin/bateri`) and the counter it
/// and the Mac number terminals with (`next.sh`): written beside the shells'
/// files, the command executable. The command carries [`KABLO_PLACEHOLDER`]
/// where kablo's place goes.
const AGENT_COMMAND: &str = include_str!("../../../assets/shell/remote/bin/bateri");
/// The counter's text: written to the server as `next.sh`, and **the same bytes** run over the
/// ssh connection for the terminal this computer opens itself
/// ([`crate::kablo::install::next_number`]) — one rule for both, not the same rule twice.
pub const COUNTER: &str = include_str!("../../../assets/shell/remote/next.sh");

/// The word in [`AGENT_COMMAND`] that [`files_script`] replaces with kablo's place.
/// Left as it is when the place is not known (or not one [`is_kablo_path`]
/// takes): the command then looks for `kablo` on `PATH`.
const KABLO_PLACEHOLDER: &str = "__BATERI_KABLO__";

/// The payload's first line: the one-liner runs only a decoded text that
/// starts with it (a decoder that ignored `-d` prints base64, not this).
const MAGIC: &str = "bateri_boot=1";

/// The files the server's shells are set up from, under
/// `~/.local/share/bateri/shell/` ([`files_script`] writes them), embedded at
/// build time: the local zsh wrapper's four `ZDOTDIR` files and its swap
/// **verbatim**, the remote zsh body beside them, bash's `ENV` file and fish's
/// `vendor_conf.d` file, with the dock's functions each of the three shares with
/// the Mac's own wrapper (`zsh/dock.zsh`, `bash/dock.bash`, `fish/dock.fish`: the
/// same bytes, not a copy), the agent's command with its counter, and the
/// bootstrap.
const REMOTE_FILES: [(&str, &str); 14] = [
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
        "zsh/dock.zsh",
        include_str!("../../../assets/shell/zsh/dock.zsh"),
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
        "bash/dock.bash",
        include_str!("../../../assets/shell/bash/dock.bash"),
    ),
    (
        "fish/vendor_conf.d/bateri.fish",
        include_str!("../../../assets/shell/remote/fish/vendor_conf.d/bateri.fish"),
    ),
    (
        "fish/dock.fish",
        include_str!("../../../assets/shell/fish/dock.fish"),
    ),
    ("bin/bateri", AGENT_COMMAND),
    ("next.sh", COUNTER),
    // The bootstrap itself, for the terminals an agent opens without a
    // program: `bin/bateri` runs this file where the one-liner's text ran.
    ("boot.sh", BOOT_SCRIPT),
];

/// The script the server's `sh` reads from its standard input to write
/// [`REMOTE_FILES`] under `${XDG_DATA_HOME:-~/.local/share}/bateri/shell/`:
/// each file under a temporary name and renamed into place (a shell that starts
/// meanwhile reads a whole file), one that is already the same left as it is
/// (no rename on every connection), owner-only, nothing in `/tmp`, no log, the
/// agent's command executable. `kablo` is where kablo is on that server,
/// relative to home (`kablo::install::Installed::path`): the command an agent
/// runs there (`bateri`) takes it from the file ([`KABLO_PLACEHOLDER`]); a place
/// that is not one [`is_kablo_path`] takes leaves the word as it is.
///
/// **It is sent over the ssh connection the user made**, with the call that
/// looks for kablo (`kablo::install::ensure`), and not inside the bootstrap:
/// the files are far more bytes than the one argument the bootstrap travels in
/// (Linux takes 128 KiB for one word), and the connection has no such limit. It
/// is for a login shell the integration serves (zsh, bash, fish: `$SHELL`, as
/// sshd sets it); for any other nothing is written. It never fails the call:
/// what could not be written is found missing by the bootstrap, which says so.
pub fn files_script(kablo: Option<&str>) -> String {
    /// Where the files go, and the function that puts one there.
    const PRELUDE: &str = r#"case ${SHELL##*/} in zsh | bash | fish) ;; *) exit 0 ;; esac
bt_data=${XDG_DATA_HOME:-}
case $bt_data in /*) ;; *) bt_data=$HOME/.local/share ;; esac
bt_dir=$bt_data/bateri/shell
bt_put() {
  bt_tmp=$bt_dir/$1.tmp.$$
  if printf '%s' "$2" > "$bt_tmp"; then
    if cmp -s "$bt_tmp" "$bt_dir/$1"; then
      rm -f "$bt_tmp"
      return 0
    fi
    if mv -f "$bt_tmp" "$bt_dir/$1"; then
      return 0
    fi
  fi
  rm -f "$bt_tmp"
  return 1
}
"#;
    let kablo = kablo.filter(|kablo| is_kablo_path(kablo));
    let mut dirs: Vec<&str> = Vec::new();
    for (path, _) in REMOTE_FILES {
        if let Some((dir, _)) = path.rsplit_once('/') {
            if !dirs.contains(&dir) {
                dirs.push(dir);
            }
        }
    }
    let made = dirs
        .iter()
        .map(|dir| format!("\"$bt_dir/{dir}\""))
        .collect::<Vec<_>>()
        .join(" ");
    let mut text = format!("{PRELUDE}(umask 077 && mkdir -p {made} && \\\n");
    for (path, content) in REMOTE_FILES {
        let content = match kablo {
            Some(kablo) if content.contains(KABLO_PLACEHOLDER) => {
                std::borrow::Cow::Owned(content.replace(KABLO_PLACEHOLDER, kablo))
            }
            _ => std::borrow::Cow::Borrowed(content),
        };
        text.push_str(&format!(
            "  bt_put {} {} && \\\n",
            crate::upload::sq(path),
            crate::upload::sq(&content)
        ));
    }
    text.push_str("  { chmod u+x \"$bt_dir/bin/bateri\" || :; }) 2>/dev/null\nexit 0\n");
    text
}

/// What the call that looks for kablo does to a server, for a test: `sh -s` reads
/// [`files_script`] with the environment sshd gives a command (`HOME`, `SHELL`,
/// `XDG_DATA_HOME` pinned under the home, `extra` on top). The files are in place when it
/// returns; what it could not write it did not.
#[cfg(test)]
pub(crate) fn seed_files(
    shell: &std::path::Path,
    home: &std::path::Path,
    extra: &[(&str, String)],
    kablo: Option<&str>,
) -> std::process::ExitStatus {
    seed_files_with(std::path::Path::new("/bin/sh"), shell, home, extra, kablo)
}

/// [`seed_files`] through the interpreter `sh` (a server's `sh` may be dash, BusyBox's ash…).
#[cfg(test)]
pub(crate) fn seed_files_with(
    sh: &std::path::Path,
    shell: &std::path::Path,
    home: &std::path::Path,
    extra: &[(&str, String)],
    kablo: Option<&str>,
) -> std::process::ExitStatus {
    use std::io::Write as _;
    let mut child = std::process::Command::new(sh)
        .arg("-s")
        .env_clear()
        .env("HOME", home)
        .env("SHELL", shell)
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .envs(extra.iter().map(|(key, value)| (*key, value.as_str())))
        .current_dir(home)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("sh");
    // A script that ends at once (a shell the integration does not serve) leaves its input
    // unread, as it does over ssh: a broken pipe is no failure.
    let _ = child
        .stdin
        .take()
        .expect("stdin")
        .write_all(files_script(kablo).as_bytes());
    child.wait().expect("sh ended")
}

/// The bootstrap's payload: the magic line, the starting directory when
/// there is one, and the bootstrap (`assets/shell/remote/boot.sh`) itself,
/// `eval`ed on the server.
fn payload(cwd: Option<&str>) -> String {
    let mut text = format!("{MAGIC}\n");
    if let Some(cwd) = cwd.filter(|cwd| is_start_dir(cwd)) {
        text.push_str(&format!("bt_cwd={}\n", crate::upload::sq(cwd)));
    }
    text.push_str(&format!("eval {}\n", crate::upload::sq(BOOT_SCRIPT)));
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
    BOOT.get_or_init(|| one_liner(&payload(None)))
}

/// [`boot`], starting the login shell in `cwd` when it is one
/// ([`is_start_dir`]) — the remote folder ⌘T's tab was in, which the new
/// terminal asks for ([`kablo_argv`]). Still [`is_inline`]: the directory is
/// inside the base64 payload ([`payload`]).
pub fn boot_in(cwd: Option<&str>) -> std::borrow::Cow<'static, str> {
    let cwd = cwd.filter(|cwd| is_start_dir(cwd));
    if cwd.is_none() {
        return std::borrow::Cow::Borrowed(boot());
    }
    std::borrow::Cow::Owned(one_liner(&payload(cwd)))
}

/// Whether `path` can be kablo's place in the agent's command: relative to
/// home (`.local/share/kablo/0.2.0/kablo`), made of the characters a version
/// and the directories around it are made of, and nothing that steps out of
/// the directory. It goes into a script on the server, so nothing else does.
pub fn is_kablo_path(path: &str) -> bool {
    path.len() <= 256
        && !path.starts_with('/')
        && path
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'+' | b'-' | b'_' | b'/'))
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

/// The longest starting directory carried: Linux's `PATH_MAX`. A longer one
/// could not be `cd`ed to in one step anyway.
const START_DIR_MAX: usize = 4096;

/// Whether `dir` can be the server's starting directory: absolute, `~` or
/// `~/…` (the forms the dock shows: OSC 7's path or the title's folder),
/// no control character (a NUL could not even reach the shell's
/// environment) and at most [`START_DIR_MAX`] bytes. Anything else is not
/// carried: the session starts at home, as a plain `ssh` does.
pub fn is_start_dir(dir: &str) -> bool {
    (dir.starts_with('/') || dir == "~" || dir.starts_with("~/"))
        && dir.len() <= START_DIR_MAX
        && !dir.chars().any(char::is_control)
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
///   user's `args` with the **same** [`Control`] options the wrapped call got
///   ([`control`], one producer), so the rerun rides the wrapped session's
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
/// `ssh-fell-back`), asked by the previous `ssh` function of a shell started
/// before an update, after a wrapped `ssh` ended with `N` — an `ssh` that
/// function wrapped before the update (`ssh-argv` wraps nothing any more). The arguments are the **wrapped** ones, as `ssh-argv` printed
/// them: [`unwrap`] gives back the user's and [`nonce`] the attempt's — one
/// parser, the shell does not take our command apart. Writes the plain
/// rerun's arguments to `out` ([`fell_back`]), each followed by a NUL, or
/// nothing — `ssh-argv`'s wire; the exit code is always zero. `--instance` is
/// `ssh-argv`'s, so the rerun's sharing options are the wrapped call's.
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
    /// binary wrapped. Frozen here: the function's two calls as the last script
    /// that made them did and a wrapped argv of an older version (another
    /// version word). **The rule:** a flag or a tail form is added beside
    /// these, never changed; this fixture goes two versions after the last
    /// script that made the calls. Today's script does not make them: its `ssh`
    /// function runs `bateri ssh-hold` and the plain `ssh` (the wrapped
    /// session is gone, `ssh_wrap::kablo_path`), so the subcommands answer
    /// the shells that were started before the update and nobody else: `ssh-argv`
    /// with nothing, whatever the call (the shell crate's
    /// `the_previous_functions_ssh_argv_calls_get_the_plain_ssh`), and
    /// `ssh-fell-back` for an `ssh` that was wrapped before the update, which
    /// reads the older wrapped argv below.
    #[test]
    fn the_previous_functions_calls_still_parse() {
        let script = include_str!("../../../assets/shell/zsh/bateri.zsh");
        for call in [
            r#"ssh-argv $tty --block "$__bateri_block" "${instance[@]}" "${cwd[@]}" -- "$@""#,
            r#"ssh-fell-back --rc $rc "${instance[@]}" -- "${wrapped[@]}""#,
        ] {
            assert!(
                !script.contains(call),
                "today's script makes a call the wrapped session's: {call}"
            );
        }
        assert!(
            script.contains("ssh-hold -- \"$@\""),
            "today's function runs the pane's program in the `ssh`'s place"
        );
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
        assert_eq!(
            host_key("user deploy\nhostname 10.0.0.5\nport 22\nproxyjump none\n").as_deref(),
            Some("deploy@10.0.0.5:22")
        );
        assert_eq!(
            host_key("user deploy\nhostname 10.0.0.5\nport 22\nproxyjump prod-bastion\n")
                .as_deref(),
            Some("deploy@10.0.0.5:22 via proxyjump prod-bastion")
        );
        assert_eq!(
            host_key("user deploy\nhostname 10.0.0.5\nport 22\nproxycommand nc\tx %h %p\n"),
            None
        );
        // `%n` is the host as typed: one wildcard block's bastions differ.
        let typed = |host: &str| {
            host_key(&format!(
                "host {host}\nuser deploy\nhostname 10.0.0.5\nport 22\n\
                 proxycommand ssh %n-bastion -W %h:%p\n"
            ))
        };
        assert_eq!(
            typed("prod-app").as_deref(),
            Some("deploy@10.0.0.5:22 via proxycommand ssh prod-app-bastion -W %h:%p")
        );
        assert_ne!(typed("prod-app"), typed("stage-app"));
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

    #[test]
    fn kablo_path_takes_an_interactive_call_the_setting_and_the_server_allow_and_everything_else_is_the_plain_ssh()
     {
        let settings = Settings::default();
        let args = words(&["-p", "2222", "prod"]);
        let runner = Gconfig::new(PLAIN);
        let host = kablo_path(&args, &settings, &runner, &HostState::default()).expect("kablo");
        assert_eq!(
            (host.host.as_str(), host.port, host.key.as_str()),
            ("prod", Some(2222), "u@h:22")
        );
        assert_eq!(host.argv, words(&["ssh", "-p", "2222", "prod"]));
        assert_eq!(host.config, PLAIN);
        assert_eq!(
            runner.calls.borrow().last(),
            Some(&words(&["ssh", "-G", "-p", "2222", "prod"])),
            "ssh -G sees the user's arguments"
        );

        // Everything else is the plain `ssh`, and nothing is asked that need
        // not be: no process for a call that runs a command, a flag that is not
        // a session, or a host the setting refuses.
        let off = Settings {
            remote_integration: false,
            ..Settings::default()
        };
        let production = Settings {
            remote_hosts: vec![HostRule {
                pattern: "prod".to_owned(),
                mark: Some(HostMark::Production),
                integration: None,
            }],
            ..Settings::default()
        };
        for (settings, args) in [
            (&settings, &["prod", "uptime"][..]),
            (&settings, &["-t", "prod", "tmux", "attach"]),
            (&settings, &["-N", "prod"]),
            (&settings, &["-T", "prod"]),
            (&settings, &["-W", "x:22", "prod"]),
            (&settings, &["-O", "check", "prod"]),
            (&settings, &["-o", "RequestTTY=no", "prod"]),
            (&settings, &["-V"]),
            (&off, &["prod"]),
            (&production, &["prod"]),
        ] {
            let runner = Gconfig::new(PLAIN);
            assert_eq!(
                kablo_path(&words(args), settings, &runner, &HostState::default()),
                None,
                "{args:?}"
            );
            assert!(runner.calls.borrow().is_empty(), "{args:?}: no ssh -G");
        }
        for out in [
            "user u\nhostname h\nport 22\nremotecommand tmux\n",
            "user u\nhostname h\nport 22\nrequesttty false\n",
            "user u\nhostname h\nport 22\nsessiontype none\n",
        ] {
            let runner = Gconfig::new(out);
            assert_eq!(
                kablo_path(&words(&["prod"]), &settings, &runner, &HostState::default()),
                None,
                "{out}"
            );
        }

        // The state file: a server without a shell and one kablo cannot be kept
        // on are plain; a row for another account is not this one's.
        let runner = Gconfig::new(PLAIN);
        let with = |fact: Fact, key: &str| {
            let mut state = HostState::default();
            state.note(fact, key, 1);
            kablo_path(&words(&["prod"]), &settings, &runner, &state).map(|host| host.key)
        };
        assert_eq!(with(Fact::Plain, "u@h:22"), None);
        assert_eq!(with(Fact::NoKablo, "u@h:22"), None);
        assert_eq!(with(Fact::NoKablo, "v@h:22"), Some("u@h:22".to_owned()));
        assert_eq!(with(Fact::Posix, "u@h:22"), Some("u@h:22".to_owned()));
        assert_eq!(with(Fact::Touched, "u@h:22"), Some("u@h:22".to_owned()));
    }

    #[test]
    fn the_no_kablo_row_is_a_row_an_older_reader_skips() {
        let mut state = HostState::default();
        state.note(Fact::NoKablo, "u@h:22", 7);
        state.note(Fact::Posix, "u@h:22", 8);
        let text = state.render();
        assert_eq!(text, "nokablo\tu@h:22\t7\nposix\tu@h:22\t8\n");
        assert_eq!(HostState::parse(&text), state);
        // What an older build reads of the same file: the rows it knows.
        let older = text
            .lines()
            .filter(|line| !line.starts_with("nokablo"))
            .collect::<Vec<_>>()
            .join("\n");
        let mut known = HostState::default();
        known.note(Fact::Posix, "u@h:22", 8);
        assert_eq!(HostState::parse(&older), known);
        state.forget(Fact::NoKablo, "u@h:22");
        assert!(!state.knows(Fact::NoKablo, "u@h:22"));
    }

    #[test]
    fn a_new_terminal_runs_the_bootstrap_as_an_argument_vector() {
        let tab =
            PaneUuid::from_url("bateri://tab/00000000-0000-4000-8000-000000000001").expect("a tab");
        let argv = kablo_argv(None, Some(7), NONCE, Some(&tab));
        let text: Vec<String> = argv
            .iter()
            .map(|word| String::from_utf8(word.clone()).expect("text"))
            .collect();
        assert_eq!(&text[..2], ["sh", "-c"]);
        assert!(
            text[2].starts_with("b="),
            "the one-liner around the payload"
        );
        assert!(!text[2].contains('\''), "{}", text[2]);
        assert_eq!(
            &text[3..],
            [
                BOOT_NAME,
                "7",
                NONCE,
                TERM_PROGRAM_VERSION,
                "bateri://tab/00000000-0000-4000-8000-000000000001"
            ]
        );
        // The same words in the remote command a login shell would run.
        let command = remote_command(&text[2], Some(7), NONCE, Some(&tab));
        assert!(command.ends_with(&format!(
            "' {BOOT_NAME} 7 {NONCE} {TERM_PROGRAM_VERSION} bateri://tab/00000000-0000-4000-8000-000000000001"
        )));
        // Without a block or a tab the placeholders the bootstrap reads.
        let bare = kablo_argv(None, None, NONCE, None);
        assert_eq!(bare[4], b"-");
        assert_eq!(bare[7], b"-");
        // A starting directory rides in the payload.
        let in_dir = kablo_argv(Some("/srv/app"), None, NONCE, None);
        assert_ne!(in_dir[2], bare[2]);
    }

    #[test]
    fn a_starting_directory_is_absolute_or_home_rooted() {
        for dir in ["/", "/srv/my app", "/home/ömer/proje", "~", "~/src", "/a'b"] {
            assert!(is_start_dir(dir), "{dir:?}");
        }
        let long = format!("/{}", "a".repeat(START_DIR_MAX));
        for dir in [
            "", "src", "~user/x", "~x", "-r", "/a\nb", "/a\0b", "/a\x1b]b", &long,
        ] {
            assert!(!is_start_dir(dir), "{dir:?}");
        }
        // One the server could not be sent builds no other bootstrap.
        assert_eq!(boot_in(Some("src")), boot());
        assert_eq!(boot_in(None), boot());
        assert_ne!(boot_in(Some("/srv")), boot());
        assert!(is_inline(&boot_in(Some("/a'b \"c\" $d !e \\f"))));
    }

    /// With bateri's instance directory, the wrapped session became a
    /// master there — unless the user shared connections themselves. The
    /// fallback gives its plain rerun the same options, from the same
    /// producer ([`control`], [`names_sharing`]).
    #[test]
    fn the_wrapped_sessions_sharing_is_the_instance_directorys_unless_the_user_shares() {
        let root = PathBuf::from(format!("/tmp/bt-wrap-ctl-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let roots = vec![root.join("r")];
        let dir = crate::ssh_route::prepare_instance(&roots[0], "0a1b2c3d").unwrap();
        let sockets = crate::ssh_route::instance_dirs(&roots, "0a1b2c3d");
        let shared = |config: &str, typed: &[&str], sockets: &[PathBuf]| {
            control(config, sockets).filter(|_| !names_sharing(&words(typed)))
        };
        let key = crate::ssh_route::host_key(&parse_config(PLAIN).unwrap());
        let control = Control {
            socket: dir.join(format!("u-{key}")),
        };
        let wrapped = wrap(
            &words(&["prod"]),
            BOOT_STUB,
            None,
            NONCE,
            None,
            shared(PLAIN, &["prod"], &sockets).as_ref(),
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
        assert_eq!(master_socket(&wrapped), control.socket.to_str());
        // The user's own sharing, an unknown or a missing instance: no sharing.
        for config in [
            "user u\nhostname h\nport 22\ncontrolmaster auto\n",
            "user u\nhostname h\nport 22\ncontrolpath /u/cm-%C\n",
        ] {
            assert_eq!(shared(config, &["prod"], &sockets), None, "{config}");
        }
        for instance in ["ffffffff", "../r", "x"] {
            let sockets = crate::ssh_route::instance_dirs(&roots, instance);
            assert_eq!(shared(PLAIN, &["prod"], &sockets), None, "{instance}");
        }
        assert_eq!(shared(PLAIN, &["prod"], &[]), None);
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
            assert_eq!(shared(PLAIN, typed, &sockets), None, "{typed:?}");
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
        let wrapped = wrap(
            &args,
            BOOT_STUB,
            None,
            NONCE,
            None,
            control(PLAIN, &sockets).as_ref(),
        );
        let rerun = fell_back(&args, 1, false, PLAIN, &touched(false), &sockets).expect("rerun");
        assert_eq!(rerun.args[..6], wrapped[1..7], "the wrapped call's options");
        assert_eq!(rerun.args[6..], args[..]);
        assert_eq!(unwrap(&rerun.args), &rerun.args[..], "not wrapped");
        // The user's own sharing choice: none of ours, as in the wrapped call.
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

    /// The most one argument of the command a new terminal runs may be: Linux's
    /// `MAX_ARG_STRLEN` is 128 KiB for one word, and the bootstrap is one word
    /// (`sh -c <it>`). It carries the bootstrap and nothing else — the shells' files are
    /// written over the ssh connection ([`files_script`]), because they are more bytes than
    /// that one word could hold — so the bound is well under the limit, and a test that fails
    /// here means the bootstrap grew, not that the number should.
    const ONE_ARGUMENT: usize = 32 * 1024;

    /// The real bootstrap (not a stub) survives every login shell's quoting,
    /// round-trips through the wrapped form (`unwrap` reads what an older
    /// bateri ran) and stays below Linux's one-argument limit
    /// ([`ONE_ARGUMENT`]) — the remote command is one argument on the server.
    #[test]
    fn the_real_bootstrap_is_one_quotable_line() {
        let boot = boot();
        assert!(
            is_inline(boot),
            "the one-liner breaks a login shell's quoting"
        );
        assert!(payload(None).starts_with(&format!("{MAGIC}\n")));
        let command = remote_command(boot, Some(u32::MAX), NONCE, None);
        assert!(command.len() < ONE_ARGUMENT, "{} bytes", command.len());
        // The longest starting directory it carries, too.
        let deepest = format!("/{}", "a".repeat(START_DIR_MAX - 1));
        let command = remote_command(&boot_in(Some(&deepest)), Some(u32::MAX), NONCE, None);
        assert!(command.len() < ONE_ARGUMENT, "{} bytes", command.len());
        let whole = boot_in(Some(&deepest));
        assert!(is_inline(&whole));
        assert!(whole.len() < ONE_ARGUMENT, "{} bytes", whole.len());
        let args = words(&["-p", "2222", "prod"]);
        let wrapped = wrap(&args, boot, None, NONCE, None, None);
        assert_eq!(unwrap(&wrapped), &args[..]);
        // And as the argument vector a new terminal runs (nothing is quoted).
        let argv = kablo_argv(Some(&deepest), Some(u32::MAX), NONCE, None);
        assert_eq!(argv[2], boot_in(Some(&deepest)).as_bytes());
        // Every file the server's shells are set up from is embedded, the shared swap is the
        // local wrapper's own, and so are the dock's functions of the three shells.
        assert!(REMOTE_FILES.iter().all(|(_, content)| !content.is_empty()));
        for (path, local) in [
            (
                "zsh/zdotdir.zsh",
                include_str!("../../../assets/shell/zsh/zdotdir.zsh"),
            ),
            (
                "zsh/dock.zsh",
                include_str!("../../../assets/shell/zsh/dock.zsh"),
            ),
            (
                "bash/dock.bash",
                include_str!("../../../assets/shell/bash/dock.bash"),
            ),
            (
                "fish/dock.fish",
                include_str!("../../../assets/shell/fish/dock.fish"),
            ),
        ] {
            assert!(
                REMOTE_FILES
                    .iter()
                    .any(|(at, content)| *at == path && *content == local),
                "{path} is not the local wrapper's file"
            );
        }
        // The files are what does not fit one argument: they are in no part of it.
        let files: usize = REMOTE_FILES.iter().map(|(_, content)| content.len()).sum();
        assert!(files > ONE_ARGUMENT, "{files} bytes of files");
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
        Blocks, CaretShape, CursorBlink, DockStatus, Osc52, RemoteKind, RemoteSetupFault,
        RemoteTarget, Session, SessionOptions, TerminalOptions, Theme,
    };

    use super::{boot, boot_in, remote_command};
    use crate::child::tests::{click_dock, read_dock, screen};
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
    /// marks and anchors — drawn from the remote trail (the harness's local
    /// `ssh` block prints no anchor of its own).
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
    ///
    /// It runs where the pane would see it: under the local shell's `ssh`
    /// command (block [`PARENT`]: our identified `A`, then `C`) with the
    /// remote probe's answer in (`set_remote`). Only then do the server's own
    /// identity-less marks — fish 4 prints OSC 133 around every prompt — fall
    /// outside the local trail; without the remote state its `A` ends the
    /// command and clears the remote folder a moment after our OSC 7 set it.
    /// The stand-in for `ssh` holds the server's shell on a `read` until the
    /// probe is in, so its first prompt cannot beat it.
    fn sshd(shell: &Path, home: &Path, extra: &[(&str, String)]) -> Session {
        sshd_with(shell, home, extra, false)
    }

    /// [`sshd`], and with `dock` the session is a pane that shows a server's terminal and has
    /// the dock's payload (`set_server`): the one place the server shell's dock events are taken.
    fn sshd_with(shell: &Path, home: &Path, extra: &[(&str, String)], dock: bool) -> Session {
        // The files are on the server before the terminal starts: the call that looks for kablo
        // wrote them.
        super::seed_files(shell, home, extra, None);
        sshd_with_seeded(shell, home, extra, dock)
    }

    /// [`sshd_with`] for files that are on the server already (a test that took one away).
    fn sshd_with_seeded(
        shell: &Path,
        home: &Path,
        extra: &[(&str, String)],
        dock: bool,
    ) -> Session {
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
        let ssh = format!(
            "printf '\\033]133;A;bt_block={PARENT}\\007\\033]133;C\\007'; \
             read -r _; exec \"$0\" \"$@\""
        );
        let session = Session::spawn(
            SessionOptions {
                command: Some((
                    "/bin/sh".to_owned(),
                    vec![
                        "-c".to_owned(),
                        ssh,
                        shell.display().to_string(),
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
                dock,
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
        wait_until("the local `ssh` did not start", || {
            session.running_command().is_some()
        });
        let command = session.running_command().expect("a running command");
        let target = RemoteTarget {
            host: "server".to_owned(),
            port: None,
            kind: RemoteKind::Ssh,
            argv: vec!["ssh".to_owned(), "server".to_owned()],
            line: "ssh server".to_owned(),
        };
        assert!(
            session.set_remote(command, Some(&target)),
            "the probe's answer was not taken"
        );
        if dock {
            assert!(
                session.set_server(command, true),
                "the pane's terminal was not taken"
            );
        }
        session.write(b"\n");
        session
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
        integrates_with(name, login, rc, false)
    }

    /// [`integrates`], and the agent's command (`bin/bateri`) on `PATH`: once,
    /// also when the environment the shell starts with has the directory
    /// already (`preseed`).
    fn integrates_with(
        name: &str,
        login: &str,
        rc: &[(&str, &str)],
        preseed: bool,
    ) -> Option<(Session, TempRoot)> {
        let Some(shell) = which(name) else {
            println!("SKIPPED: {name} is not installed");
            return None;
        };
        let root = TempRoot::new(&format!(
            "remote-{name}{}",
            if preseed { "-seeded" } else { "" }
        ));
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

        let agent_bin = home.join(".local/share/bateri/shell/bin");
        let mut extra = Vec::new();
        if preseed {
            extra.push((
                "PATH",
                format!(
                    "{}:{}",
                    agent_bin.display(),
                    std::env::var("PATH").unwrap_or_default()
                ),
            ));
        }
        let session = sshd(&shell, &home, &extra);
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
        // The agent's command: found where the bootstrap wrote it, and the
        // directory is in `PATH` once.
        let found = agent_bin.join("bateri");
        let (test, once) = if name == "fish" {
            (
                format!("test (command -v bateri) = '{}'; and ", found.display()),
                "echo \"[agent-on-path-\"(printf '%s\\n' $PATH | grep -c '/bateri/shell/bin$')\"]\"",
            )
        } else {
            (
                format!("test \"$(command -v bateri)\" = '{}' && ", found.display()),
                "echo \"[agent-on-path-$(echo \"$PATH\" | tr : '\\n' | grep -c '/bateri/shell/bin$')]\"",
            )
        };
        session.write(format!("{test}{once}\n").as_bytes());
        wait_until(&format!("{name}: bateri not on PATH once"), || {
            screen_has(&session, "[agent-on-path-1]")
        });
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

    /// The call that looks for kablo writes the shells' files, and writes them as the bootstrap
    /// used to: every file with the bytes it was built with (kablo's place in the agent's
    /// command), owner-only, the command runnable, a file that is the same left alone and one
    /// that is not put back, nothing for a login shell the integration does not serve, a relative
    /// `XDG_DATA_HOME` ignored — through the `sh`s a server may have — and a call that cannot
    /// write ends well (the bootstrap says what is missing).
    #[test]
    fn the_files_script_writes_every_file_once_and_only_for_a_shell_it_serves() {
        use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
        let place = ".local/share/kablo/0.2.0/kablo";
        let mut shells = vec![PathBuf::from("/bin/sh")];
        shells.extend(which("dash"));
        for sh in shells {
            let name = sh.display().to_string();
            let root = TempRoot::new("remote-files-script");
            let home = std::fs::canonicalize(&root.0).expect("root").join("home");
            std::fs::create_dir_all(&home).expect("home");
            let shell = Path::new("/usr/bin/bash");
            let dir = home.join(".local/share/bateri/shell");
            let serve = |extra: &[(&str, String)]| {
                super::seed_files_with(&sh, shell, &home, extra, Some(place))
            };
            assert!(serve(&[]).success(), "{name}");
            let mut stamps = Vec::new();
            for (path, content) in super::REMOTE_FILES {
                let expected = content.replace(super::KABLO_PLACEHOLDER, place);
                let file = dir.join(path);
                assert_eq!(
                    std::fs::read_to_string(&file).unwrap_or_default(),
                    expected,
                    "{name}: {path}"
                );
                let meta = std::fs::metadata(&file).expect("file");
                let want = if path == "bin/bateri" { 0o700 } else { 0o600 };
                assert_eq!(meta.permissions().mode() & 0o777, want, "{name}: {path}");
                stamps.push((file, meta.ino()));
            }
            assert_eq!(
                std::fs::metadata(dir.join("bash"))
                    .expect("dir")
                    .permissions()
                    .mode()
                    & 0o777,
                0o700,
                "{name}: the directories are the owner's"
            );
            let strays = |dir: &Path| {
                let mut found = Vec::new();
                let mut todo = vec![dir.to_path_buf()];
                while let Some(next) = todo.pop() {
                    for entry in std::fs::read_dir(&next).expect("dir").flatten() {
                        if entry.path().is_dir() {
                            todo.push(entry.path());
                        } else if entry.file_name().to_string_lossy().contains(".tmp.") {
                            found.push(entry.path());
                        }
                    }
                }
                found
            };
            assert!(strays(&dir).is_empty(), "{name}: a temporary name stayed");

            // The same bytes again: no file is replaced. Changed bytes: put back.
            assert!(serve(&[]).success(), "{name}");
            for (file, inode) in &stamps {
                assert_eq!(
                    std::fs::metadata(file).expect("file").ino(),
                    *inode,
                    "{name}: {} was written again",
                    file.display()
                );
            }
            std::fs::write(dir.join("bash/dock.bash"), "edited").expect("edit");
            assert!(serve(&[]).success(), "{name}");
            assert_eq!(
                std::fs::read_to_string(dir.join("bash/dock.bash")).expect("dock"),
                include_str!("../../../assets/shell/bash/dock.bash"),
                "{name}"
            );

            // A relative `XDG_DATA_HOME` is ignored, as the spec says; an absolute one is used.
            let other = root.0.join("home2");
            std::fs::create_dir_all(&other).expect("home2");
            let other = std::fs::canonicalize(&other).expect("home2");
            let wrote = super::seed_files_with(
                &sh,
                shell,
                &other,
                &[("XDG_DATA_HOME", "relative/data".to_owned())],
                None,
            );
            assert!(wrote.success(), "{name}");
            assert!(other.join(".local/share/bateri/shell/boot.sh").is_file());
            assert!(!other.join("relative").exists(), "{name}");
            let elsewhere = root.0.join("data");
            super::seed_files_with(
                &sh,
                shell,
                &other,
                &[("XDG_DATA_HOME", elsewhere.display().to_string())],
                None,
            );
            assert!(
                elsewhere.join("bateri/shell/bin/bateri").is_file(),
                "{name}"
            );
            // Without kablo's place the agent's command keeps the word it looks for kablo by.
            let bare = std::fs::read_to_string(other.join(".local/share/bateri/shell/bin/bateri"))
                .expect("command");
            assert!(bare.contains(super::KABLO_PLACEHOLDER), "{name}");

            // A shell the integration does not serve gets nothing, and a place that cannot be
            // written to is no failure of the call.
            let third = root.0.join("home3");
            std::fs::create_dir_all(third.join(".local/share")).expect("home3");
            for plain in ["/bin/sh", "/bin/dash", "/usr/bin/tcsh", ""] {
                let status = super::seed_files_with(&sh, Path::new(plain), &third, &[], None);
                assert!(status.success(), "{name}: {plain}");
                assert!(
                    !third.join(".local/share/bateri").exists(),
                    "{name}: {plain}"
                );
            }
            std::fs::write(third.join(".local/share/bateri"), "").expect("blocker");
            assert!(
                super::seed_files_with(&sh, shell, &third, &[], None).success(),
                "{name}: a write that fails"
            );
        }
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
            // ...and fish still has the directories it would have used for an unset
            // variable: the bootstrap wrote them after ours, so fish's search paths
            // (computed at startup) did not shrink to ours alone.
            session.write(
                b"echo vendor=(contains /usr/share/fish/vendor_conf.d $__fish_vendor_confdirs; and echo kept; or echo lost)\n",
            );
            wait_until("fish: the default vendor directory was lost", || {
                screen_has(&session, "vendor=kept")
            });
            draws_remote_blocks(&session, "fish");
            close(&session);
        }
    }

    /// A server shell in the pane that shows its terminal (`sshd_with`'s dock) with the files the
    /// test writes to the home (`files`: name and text); `None` (and a line that says why) where
    /// the shell is not on this machine.
    fn docked(
        name: &str,
        tier: &str,
        files: &[(&str, &str)],
        extra: &[(&str, String)],
    ) -> Option<(Session, TempRoot)> {
        let Some(shell) = which(name) else {
            println!("SKIPPED: {name} is not installed");
            return None;
        };
        if name == "bash"
            && !crate::child::bash_version(&shell)
                .is_some_and(crate::child::BashVersion::integrates)
        {
            println!("SKIPPED: this bash is older than the integration's 4.4");
            return None;
        }
        // A root of its own for each kind of session: tests run side by side, and a root with
        // the same name is one home.
        let root = TempRoot::new(&format!("remote-{tier}-{name}"));
        let home = root.0.join("home");
        std::fs::create_dir_all(&home).expect("home");
        let home = std::fs::canonicalize(&home).expect("home");
        for (file, text) in files {
            let path = home.join(file);
            std::fs::create_dir_all(path.parent().expect("a directory")).expect("rc directory");
            std::fs::write(path, text).expect("rc file");
        }
        let session = sshd_with(&shell, &home, extra, true);
        Some((session, root))
    }

    /// The prompt a user's files write, and a theme's rewrite of it at every prompt: neither may
    /// be drawn where the terminal draws the dock.
    const BASH_THEME: &[(&str, &str)] = &[(
        ".bash_profile",
        "PS1='USERPS1XY> '\nPROMPT_COMMAND='PS1=\"THEMEXY> \"'\n",
    )];
    /// zsh's: the prompt, the right prompt and a `precmd` that writes the prompt again at every
    /// prompt (p10k and starship do the like from their own hooks).
    const ZSH_THEME: &[(&str, &str)] = &[(
        ".zshrc",
        "PS1='USERPS1XY> '\nRPROMPT=THEMEXY\nprecmd() { PS1='THEMEXY> ' }\n",
    )];
    const FISH_THEME: &[(&str, &str)] = &[(
        ".config/fish/config.fish",
        "function fish_prompt\n  echo -n 'USERPS1XY> '\nend\n\
         function fish_right_prompt\n  echo -n 'THEMEXY'\nend\n",
    )];

    /// The dock of a server shell, end to end, for a shell that reads its line from the grid:
    /// the prompt the user's files write is not drawn, the typed line is in the dock with the
    /// caret after it and not on the grid, a click in the dock moves the shell's caret (bash's
    /// widget; fish has none, so `clicks` is false there), Enter makes the command a block and the
    /// next prompt a dock again. All of it through the identity the server's shell prints: the
    /// terminal takes no dock event of anybody else's.
    fn the_server_shell_has_a_dock(name: &str, session: &Session, clicks: bool) {
        let blocks = std::cell::RefCell::new(Blocks::default());
        let dock = || read_dock(session, &blocks);
        let rows = || screen(session, &mut blocks.borrow_mut());
        wait_until(&format!("{name}: the first prompt was not a dock"), || {
            let dock = dock();
            dock.status == DockStatus::Live && dock.buffer.is_empty()
        });
        session.write(b"echo su");
        wait_until(
            &format!("{name}: the dock did not show the typed line"),
            || {
                let dock = dock();
                dock.status == DockStatus::Live && dock.buffer == "echo su"
            },
        );
        assert_eq!(dock().cursor, 7, "{name}: the caret is after the line");
        let drawn = rows().join("\n");
        assert!(
            !drawn.contains("USERPS1XY")
                && !drawn.contains("THEMEXY")
                && !drawn.contains("echo su"),
            "{name}: the grid shows what the dock is for:\n{drawn}"
        );
        if clicks {
            for index in [3usize, 0, 7] {
                click_dock(session, &blocks, index as u16);
                wait_until(
                    &format!("{name}: the caret did not follow the click"),
                    || dock().cursor == index,
                );
            }
            assert_eq!(dock().buffer, "echo su", "{name}: the click edited");
        }
        session.write(b"\n");
        wait_until(&format!("{name}: the command did not run"), || {
            rows().iter().any(|row| row == "su")
        });
        let mut now = Blocks::default();
        screen(session, &mut now);
        assert!(
            !now.as_slice().is_empty(),
            "{name}: the command's anchor was not on the grid"
        );
        wait_until(&format!("{name}: the next prompt was not a dock"), || {
            let dock = dock();
            dock.status == DockStatus::Live && dock.buffer.is_empty()
        });
        // The blocks are the same blocks: a failing command, a long one and their counters.
        draws_remote_blocks(session, name);
    }

    #[test]
    fn a_server_bash_has_the_dock() {
        let Some((session, _root)) = docked("bash", "dock", BASH_THEME, &[]) else {
            return;
        };
        the_server_shell_has_a_dock("bash", &session, true);
        close(&session);
    }

    /// zsh keeps the display of its line editor on a mirror (`8133;u` and the rest, which the
    /// terminal takes through the server shell's identity), not on the grid: the typed line is in
    /// the dock, a click moves the shell's caret through the widget, and a completion list that
    /// would not fit the screen is asked about first (`LISTMAX`).
    #[test]
    fn a_server_zsh_has_the_dock() {
        let Some((session, _root)) = docked("zsh", "dock", ZSH_THEME, &[]) else {
            return;
        };
        the_server_shell_has_a_dock("zsh", &session, true);
        session.write(b"echo \"[lm=$LISTMAX]\"\n");
        wait_until("zsh: LISTMAX is not the dock's", || {
            screen_has(&session, "[lm=0]")
        });
        close(&session);
    }

    /// The edit widget is bound at every prompt and in every insert keymap, so a keymap the user
    /// (or a plugin) changes later does not leave the dock without its clicks: the gate opens at
    /// the next prompt and a click moves the caret.
    #[test]
    fn a_server_zsh_binds_its_widget_again_when_the_keymap_changes() {
        for (index, setup) in [
            "bindkey -v",
            "bindkey -N mymap emacs; bindkey -A mymap main",
        ]
        .into_iter()
        .enumerate()
        {
            let Some((session, _root)) = docked("zsh", &format!("keymap{index}"), ZSH_THEME, &[])
            else {
                return;
            };
            let blocks = std::cell::RefCell::new(Blocks::default());
            let dock = || read_dock(&session, &blocks);
            session.write(format!("{setup}\n").as_bytes());
            wait_until(&format!("{setup}: the next prompt was not a dock"), || {
                let dock = dock();
                dock.status == DockStatus::Live && dock.buffer.is_empty()
            });
            wait_until(&format!("{setup}: the gate did not open"), || {
                dock();
                session.can_edit_dock()
            });
            session.write("abc dëf".as_bytes());
            wait_until(&format!("{setup}: the line was not in the dock"), || {
                dock().buffer == "abc dëf"
            });
            for at in [3usize, 0, 7] {
                click_dock(&session, &blocks, at as u16);
                wait_until(&format!("{setup}: the caret did not follow"), || {
                    dock().cursor == at
                });
            }
            close(&session);
        }
    }

    /// A server whose zsh is too old for `add-zle-hook-widget`, or that lacks the dock's file,
    /// is a shell without a dock: its prompt, its blocks, nothing of the Mac's.
    #[test]
    fn a_server_zsh_without_the_docks_file_keeps_its_prompt_and_its_blocks() {
        let Some(shell) = which("zsh") else {
            println!("SKIPPED: zsh is not installed");
            return;
        };
        let root = TempRoot::new("remote-nofile-zsh");
        let home = root.0.join("home");
        std::fs::create_dir_all(&home).expect("home");
        let home = std::fs::canonicalize(&home).expect("home");
        std::fs::write(home.join(".zshrc"), "PS1='USERPS1XY> '\n").expect("rc file");
        // The call that looks for kablo wrote every file but this one.
        super::seed_files(&shell, &home, &[], None);
        std::fs::remove_file(home.join(".local/share/bateri/shell/zsh/dock.zsh")).expect("remove");
        let session = sshd_with_seeded(&shell, &home, &[], true);
        wait_until("zsh: no prompt", || screen_has(&session, "USERPS1XY>"));
        session.write(b"echo \"[lm=$LISTMAX]\"\n");
        wait_until("zsh: LISTMAX changed", || screen_has(&session, "[lm=100]"));
        assert_ne!(
            read_dock(&session, &std::cell::RefCell::new(Blocks::default())).status,
            DockStatus::Live
        );
        draws_remote_blocks(&session, "zsh");
        close(&session);
    }

    #[test]
    fn a_server_fish_has_the_dock() {
        let Some((session, _root)) = docked("fish", "dock", FISH_THEME, &[]) else {
            return;
        };
        the_server_shell_has_a_dock("fish", &session, false);
        close(&session);
    }

    /// `[shell] integration = "blocks"` on the Mac reaches the server as `BATERI_DOCK=off` in the
    /// terminal's environment: the user's prompt is drawn with the marks around it, the line stays
    /// on the grid, and the widget is never bound (nothing answers a click).
    #[test]
    fn a_server_shell_under_the_blocks_setting_keeps_its_prompt_and_opens_no_dock() {
        // (No right prompt here: it would sit where a block's counter is drawn.)
        const FISH_PLAIN: &[(&str, &str)] = &[(
            ".config/fish/config.fish",
            "function fish_prompt\n  echo -n 'USERPS1XY> '\nend\n",
        )];
        const ZSH_PLAIN: &[(&str, &str)] = &[(".zshrc", "PS1='USERPS1XY> '\n")];
        for (name, files) in [
            ("bash", BASH_THEME),
            ("fish", FISH_PLAIN),
            ("zsh", ZSH_PLAIN),
        ] {
            let Some((session, _root)) =
                docked(name, "blocks", files, &[("BATERI_DOCK", "off".to_owned())])
            else {
                continue;
            };
            let blocks = std::cell::RefCell::new(Blocks::default());
            let rows = || screen(&session, &mut blocks.borrow_mut());
            wait_until(&format!("{name}: no prompt"), || {
                rows().iter().any(|row| row.contains("XY>"))
            });
            session.write(b"echo su");
            wait_until(
                &format!("{name}: the line was not drawn on the grid"),
                || rows().iter().any(|row| row.contains("> echo su")),
            );
            assert_eq!(
                read_dock(&session, &blocks).status,
                DockStatus::Idle,
                "{name}"
            );
            assert!(!session.can_edit_dock(), "{name}");
            // Enter runs it (a block with the success stripe), and the blocks are drawn as ever.
            session.write(b"\n");
            wait_until(&format!("{name}: the command did not run"), || {
                rows().iter().any(|row| row == "su")
            });
            draws_remote_blocks(&session, name);
            close(&session);
        }
    }

    /// The directory of the agent's command is added once: a shell that
    /// starts with it in `PATH` already does not get it twice.
    #[test]
    fn the_agents_command_is_on_the_path_once_also_when_it_was_there() {
        let zsh = [
            (".zshenv", "export ZDOTDIR=$HOME/cfg\n"),
            ("cfg/.zshrc", "PS1='zsh> '\n"),
        ];
        let bash = [(".bashrc", "PS1='bash> '\n")];
        for (name, login, rc) in [
            ("zsh", "cfg/.zprofile", &zsh[..]),
            ("bash", ".bash_profile", &bash[..]),
            ("fish", ".config/fish/config.fish", &[][..]),
        ] {
            if let Some((session, _root)) = integrates_with(name, login, rc, true) {
                close(&session);
            }
        }
    }

    /// The local wrappers are the ones that put nothing on `PATH`: the
    /// agent's command exists on a server only.
    #[test]
    fn only_the_remote_wrappers_know_the_agents_command() {
        const WORDS: &str = "agent's command";
        for local in [
            include_str!("../../../assets/shell/zsh/bateri.zsh"),
            include_str!("../../../assets/shell/bash/bateri.bash"),
            include_str!("../../../assets/shell/fish/vendor_conf.d/bateri.fish"),
        ] {
            assert!(!local.contains(WORDS) && !local.contains("bin/bateri"));
        }
        for remote in [
            include_str!("../../../assets/shell/remote/zsh/bateri.zsh"),
            include_str!("../../../assets/shell/remote/bash/bateri.bash"),
            include_str!("../../../assets/shell/remote/fish/vendor_conf.d/bateri.fish"),
        ] {
            assert!(remote.contains(WORDS));
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
        super::seed_files(shell, home, extra, None);
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
            crate::child::tests::write_program(
                &shell,
                "#!/bin/sh\nprintf 'id=%s|%s|%s.' \"${LC_TERMINAL-unset}\" \
                 \"${LC_TERMINAL_VERSION-unset}\" \"${LC_BATERI_TAB_URL-unset}\"\n",
            );
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

    /// The starting directory ([`boot_in`]): every login shell the bootstrap
    /// `exec`s — the integration's (zsh) and the plain fault's (`sh`) —
    /// starts in it, with `PWD` as the path was written (a symlink is not
    /// resolved); `~` is the server's home, and a folder that is not there
    /// leaves the shell at home. Any name survives the payload's quoting.
    #[test]
    fn the_bootstrap_starts_the_shell_in_the_directory() {
        let sh = which("sh").expect("sh");
        for name in ["zsh", "sh"] {
            let root = TempRoot::new(&format!("remote-cwd-{name}"));
            let home = root.0.join("home");
            let bin = root.0.join("bin");
            let odd = home.join("my 'app' \"$x\" !y");
            let link = root.0.join("link");
            std::fs::create_dir_all(home.join(".local/share")).expect("home");
            std::fs::create_dir_all(home.join("src")).expect("src");
            std::fs::create_dir_all(&odd).expect("odd");
            std::os::unix::fs::symlink(&odd, &link).expect("link");
            std::fs::create_dir_all(&bin).expect("bin");
            let shell = bin.join(name);
            crate::child::tests::write_program(
                &shell,
                "#!/bin/sh\nprintf 'pwd=%s|%s.' \"$PWD\" \"$(pwd -P)\"\n",
            );
            let real = |path: &Path| path.canonicalize().expect("real").display().to_string();
            let run = |cwd: Option<&str>| {
                let out = std::process::Command::new(&sh)
                    .arg("-c")
                    .arg(remote_command(&boot_in(cwd), Some(PARENT), NONCE, None))
                    .env_clear()
                    .env("HOME", &home)
                    .env("SHELL", &shell)
                    .env("XDG_DATA_HOME", home.join(".local/share"))
                    .env("PATH", std::env::var_os("PATH").unwrap_or_default())
                    .current_dir(&home)
                    .stdin(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .output()
                    .expect("sh");
                let text = String::from_utf8_lossy(&out.stdout).into_owned();
                text[text.find("pwd=").unwrap_or(0)..].to_owned()
            };
            let (odd, link) = (odd.display().to_string(), link.display().to_string());
            assert_eq!(
                run(Some(&odd)),
                format!("pwd={odd}|{}.", real(Path::new(&odd))),
                "{name}"
            );
            assert_eq!(
                run(Some(&link)),
                format!("pwd={link}|{}.", real(Path::new(&odd))),
                "{name}: the symlink stays itself"
            );
            assert_eq!(
                run(Some("~/src")),
                format!("pwd={}/src|{}.", home.display(), real(&home.join("src"))),
                "{name}"
            );
            assert_eq!(
                run(Some("~")),
                format!("pwd={}|{}.", home.display(), real(&home)),
                "{name}"
            );
            for cwd in [None, Some("/no/such/folder"), Some("src")] {
                let text = run(cwd);
                assert!(
                    text.ends_with(&format!("|{}.", real(&home))),
                    "{name} {cwd:?}: {text}"
                );
            }
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
