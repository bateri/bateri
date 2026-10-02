//! Remote shell integration's wrapping decision (048): whether the user's
//! `ssh` gets bateri's bootstrap, and the argv round trip.
//!
//! **One owner.** The local zsh's `ssh` function (phase-2) asks
//! `bateri ssh-argv` ([`ssh_argv_main`]); the answer is either the wrapped
//! argv or nothing, and nothing means plain `ssh` — today's path. Every
//! condition of R1 is in [`decide`]: the call is interactive (the same walk as
//! the remote probe's, `jobs::ssh_call`), stdin and stdout are terminals (the
//! caller's bit — under `$(…)` our stdout is a pipe), the setting allows it
//! for the host as typed ([`Settings::integration_for`]), `ssh -G` does not
//! say the session runs something else ([`Session`]) and the server was
//! **learned** to have a POSIX shell ([`HostState`]; 048 Karar → "ilk
//! bağlantıda öğren").
//!
//! **The wrapped form is positional** (discussion → Muhakeme): `-t` first,
//! the user's arguments unchanged, the bootstrap command last — one argument
//! that starts with a letter, so it is the remote command whether or not the
//! user ended the options with `--` (a second `--` after a terminated walk
//! would be sent to the remote shell). [`unwrap`] checks those positions, it
//! does not search for the command.
//!
//! **The session is a master** (R7, phase-5): when the user shares no
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

use bt_core::Settings;

use crate::jobs::ssh_call;
use crate::ssh_route::{
    Account, SESSION_PERSIST, SshConfig, SshRunner, parse_config, session_socket,
};

/// The bootstrap's `$0` and the wrapped command's last word: what [`unwrap`]
/// checks at the last position, together with [`COMMAND_HEAD`].
pub const BOOT_NAME: &str = "bateri-boot";

/// The remote command's head. `exec` replaces the login shell sshd started
/// (`$SHELL -c '<command>'`, which may be fish or csh): one word, a single
/// quote and no backslash, the quoting every shell reads the same way (037's
/// upload rule).
const COMMAND_HEAD: &str = "exec sh -c '";

/// The remote command for a bootstrap script: `exec sh -c '<boot>' bateri-boot`,
/// and with the local block of the `ssh` command (048 phase-3) one more word,
/// its number: `sh`'s `$1`, the `P` of the remote blocks' `bt_remote=<P>.<S>.<n>`.
/// `boot` must not contain `'` ([`decide`] refuses one that does).
pub fn remote_command(boot: &str, parent: Option<u32>) -> String {
    match parent {
        Some(parent) => format!("{COMMAND_HEAD}{boot}' {BOOT_NAME} {parent}"),
        None => format!("{COMMAND_HEAD}{boot}' {BOOT_NAME}"),
    }
}

/// The connection sharing a wrapped session gets (phase-5): the session is a
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
    control: Option<&Control>,
) -> Vec<String> {
    let mut wrapped = Vec::with_capacity(args.len() + 8);
    wrapped.push("-t".to_owned());
    if let Some(control) = control {
        wrapped.extend(control.options());
    }
    wrapped.extend(args.iter().cloned());
    wrapped.push(remote_command(boot, parent));
    wrapped
}

/// The inverse of [`wrap`]: the user's arguments if `args` has the wrapped
/// shape, otherwise `args` itself. The shape is positional: `-t` first, then
/// [`Control`]'s six words if they are there, a bootstrap command last (with
/// or without the parent's number), and in between an interactive call with
/// no remote command of its own.
pub fn unwrap(args: &[String]) -> &[String] {
    let [first, inner @ .., last] = args else {
        return args;
    };
    let inner = match inner.split_at_checked(6) {
        Some((control, rest)) if Control::matches(control) => rest,
        _ => inner,
    };
    let boot = last.strip_prefix(COMMAND_HEAD).and_then(|rest| {
        // The parent's number, if any: ` <digits>` after the name.
        let rest = match rest.rsplit_once(' ') {
            Some((head, number))
                if !number.is_empty() && number.bytes().all(|b| b.is_ascii_digit()) =>
            {
                head
            }
            _ => rest,
        };
        rest.strip_suffix(BOOT_NAME)?.strip_suffix("' ")
    });
    let wrapped = first == "-t"
        && boot.is_some_and(|boot| !boot.contains('\''))
        && ssh_call("ssh", inner).is_some_and(|call| !call.command);
    if wrapped { inner } else { args }
}

// ─── ssh -G ──────────────────────────────────────────────────────────────

/// What `ssh -G` says about the session the target would open (R1.2).
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
/// resolves them — the same triple as the saved password's account (047
/// Karar 6, [`Account`]), so `ssh web` and `ssh deploy@10.0.0.5` on one
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
    /// The server runs a POSIX `sh` (learned from the helper session's
    /// greeting, phase-2): a wrapped connection can start there.
    Posix,
    /// bateri's bootstrap ran there — the list of servers with bateri's files
    /// (048 Karar: the remove button comes later).
    Touched,
}

impl Fact {
    fn name(self) -> &'static str {
        match self {
            Self::Posix => "posix",
            Self::Touched => "touched",
        }
    }

    fn from_name(name: &str) -> Option<Self> {
        match name {
            "posix" => Some(Self::Posix),
            "touched" => Some(Self::Touched),
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
}

/// Reads the state file; a missing or unreadable file knows nothing, so no
/// server counts as learned (the safe direction: nothing is wrapped).
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
    if let Some(dir) = path.parent() {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)?;
    }
    let _lock = lock(path, patience)?;
    let mut state = load(path);
    state.note(fact, key, at);
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

/// R1's every condition in one place; `None` → plain `ssh`.
///
/// The cheap questions come first, so `ssh -G` (a process, and it runs the
/// config's `Match exec`) is asked only for a call that would be wrapped:
/// a usable bootstrap, terminals on both ends, an interactive call without a
/// remote command, the setting for the host as typed; then `ssh -G` and the
/// learned state of the server it resolves to. `sockets` are this bateri's
/// instance directories ([`crate::ssh_route::instance_dirs`]): with one, the
/// session also becomes a master there ([`Control`]).
#[allow(clippy::too_many_arguments)] // R1's inputs, each a different source; a struct would only rename them
pub fn decide(
    args: &[String],
    tty: bool,
    settings: &Settings,
    runner: &dyn SshRunner,
    state: &HostState,
    boot: &str,
    parent: Option<u32>,
    sockets: &[PathBuf],
) -> Option<Wrapped> {
    if boot.is_empty() || !is_inline(boot) || !tty {
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
    if !state.knows(Fact::Posix, &key) {
        return None;
    }
    let control = control(&out, sockets).filter(|_| !names_sharing(args));
    Some(Wrapped {
        args: wrap(args, boot, parent, control.as_ref()),
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

/// The session's [`Control`] (phase-5): `None` when the user's configuration
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
/// — one printable ASCII line. 037's upload rule, widened by the shells.
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
/// embedded at build time (048 phase-2 → Uygulama Notları: the subcommand runs
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
/// one it reports `decode` (`8133;f`). Both arms end in the plain login shell:
/// the payload `exec`s its own, and a payload that returned (a parse error on
/// an unusual `sh`) must not close the connection. The
/// escape bytes come from `awk`'s `%c`: a `\033` is not [`is_inline`].
fn one_liner(payload: &str) -> String {
    format!(
        "b={b64}; s=; for d in \"base64 -d\" \"base64 -D\" \"b64decode -r\" \
         \"openssl base64 -d -A\"; do s=$(printf %s \"$b\" | $d 2>/dev/null); \
         case $s in {MAGIC}*) break;; esac; s=; done; unset b d; \
         case $s in {MAGIC}*) eval \"$s\";; *) awk -v f=%c%s%c -v m=\"]8133;f;decode\" \
         \"BEGIN{{printf(f,27,m,7)}}\" 2>/dev/null;; esac; exec \"${{SHELL:-/bin/sh}}\" -l",
        b64 = base64(payload.as_bytes()),
    )
}

/// The bootstrap the wrapped `ssh` runs (built once per process): the
/// one-liner carrying the base64 payload. Always [`is_inline`].
pub fn boot() -> &'static str {
    static BOOT: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    BOOT.get_or_init(|| one_liner(&payload()))
}

// ─── learning ────────────────────────────────────────────────────────────

/// Records that the server behind `ssh` runs a POSIX `sh` (048 discussion →
/// Karar: learn on the first connection): `ssh` is the helper session's argv (the program, its
/// options, the target, no remote command) and the caller calls this once its
/// greeting arrived — the greeting comes from `sh` on the server. The key is
/// [`host_key`] of `ssh -G` for the same argv (the route's own options do not
/// change user, host or port). `Ok(true)` if a row was written; a server
/// already learned writes nothing (`Ok(false)`), and so does an argv `ssh -G`
/// cannot read.
pub fn learn(runner: &dyn SshRunner, ssh: &[String], path: &Path) -> io::Result<bool> {
    let Some((program, args)) = ssh.split_first() else {
        return Ok(false);
    };
    let mut argv = vec![program.clone(), "-G".to_owned()];
    argv.extend(args.iter().cloned());
    let Some(key) = runner
        .run(&argv)
        .ok()
        .filter(|(code, _, _)| *code == Some(0))
        .and_then(|(_, out, _)| host_key(&out))
    else {
        return Ok(false);
    };
    if load(path).knows(Fact::Posix, &key) {
        return Ok(false);
    }
    record(path, Fact::Posix, &key, unix_now(), LOCK_PATIENCE).map(|()| true)
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
/// (`BATERI_SSH_INSTANCE`, phase-5), looked for under `roots`
/// ([`crate::ssh_route::socket_bases`]); without it, or without the
/// directory, the session is no master ([`Control`]). `settings` is the settings file's launch
/// reading (an unusable file turns the integration off); `state_path` is the
/// platform's state file. A wrapped connection is recorded as
/// [`Fact::Touched`] **before** it is printed: if the record fails nothing is
/// printed, because the touched list is the user's account of where bateri
/// wrote.
pub fn ssh_argv_main(
    argv: &[String],
    settings: &Settings,
    runner: &dyn SshRunner,
    state_path: &Path,
    roots: &[PathBuf],
    boot: &str,
    out: &mut impl Write,
) -> i32 {
    let (tty, rest) = match argv {
        [flag, rest @ ..] if flag == "--tty" => (true, rest),
        rest => (false, rest),
    };
    // `--block N`: the local block of the `ssh` command (048 phase-3), the
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
    let Some(wrapped) = decide(args, tty, settings, runner, &state, boot, parent, &sockets) else {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    use bt_core::{HostMark, HostRule};

    fn words(list: &[&str]) -> Vec<String> {
        list.iter().map(|&word| word.to_owned()).collect()
    }

    const BOOT_STUB: &str = "echo hi";

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
            for parent in [None, Some(7), Some(u32::MAX)] {
                for control in [None, Some(&control)] {
                    let wrapped = wrap(&args, BOOT_STUB, parent, control);
                    assert_eq!(wrapped.first().map(String::as_str), Some("-t"));
                    assert_eq!(unwrap(&wrapped), &args[..], "{wrapped:?}");
                }
            }
            // An argv that was never wrapped is itself.
            assert_eq!(unwrap(&args), &args[..]);
        }
    }

    #[test]
    fn unwrap_reads_positions_not_contents() {
        // The command somewhere other than last, or no leading `-t`: not ours.
        let command = remote_command(BOOT_STUB, None);
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
        }
    }

    #[test]
    fn the_wrapped_call_still_reads_as_the_users() {
        // The remote probe sees the wrapped process; its target is the typed one.
        let wrapped = wrap(
            &words(&["-o", "User=x", "--", "prod"]),
            BOOT_STUB,
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
                    touched\tb@h:22\t5\textra\ntouched\tb@h:22\t7\n";
        let state = HostState::parse(text);
        assert!(state.knows(Fact::Posix, "a@h:22"));
        assert!(!state.knows(Fact::Touched, "a@h:22"));
        assert!(state.knows(Fact::Touched, "b@h:22"));
        assert_eq!(state.render(), "posix\ta@h:22\t100\ntouched\tb@h:22\t7\n");
        let mut state = state;
        state.note(Fact::Touched, "b@h:22", 9);
        state.note(Fact::Posix, "c@h:22", 10);
        assert_eq!(
            HostState::parse(&state.render()),
            state,
            "one row per fact and key"
        );
        assert_eq!(state.rows.len(), 3);
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
                &[]
            ),
            Some(Wrapped {
                args: wrap(&args, BOOT_STUB, None, None),
                key: "u@h:22".to_owned(),
            })
        );
        assert_eq!(
            runner.calls.borrow().last(),
            Some(&words(&["ssh", "-G", "prod"])),
            "ssh -G sees the user's arguments"
        );

        // R1.1: no terminal, a remote command (forced tty too), a non-interactive flag.
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
                decide(&args, true, &settings, &runner, &learned(), boot, None, &[]),
                None,
                "{boot:?}"
            );
        }
    }

    #[test]
    fn decide_asks_the_config_the_setting_and_the_state() {
        let args = words(&["prod"]);
        let on = Settings::default();
        // R1.2: the config runs something else.
        for out in [
            "user u\nhostname h\nport 22\nremotecommand tmux\n",
            "user u\nhostname h\nport 22\nrequesttty false\n",
            "user u\nhostname h\nport 22\nsessiontype none\n",
        ] {
            let runner = Gconfig::new(out);
            assert_eq!(
                decide(&args, true, &on, &runner, &learned(), BOOT_STUB, None, &[]),
                None,
                "{out}"
            );
        }
        // R1.4: an unlearned server.
        let runner = Gconfig::new(PLAIN);
        assert_eq!(
            decide(
                &args,
                true,
                &on,
                &runner,
                &HostState::default(),
                BOOT_STUB,
                None,
                &[],
            ),
            None
        );
        // R1.3: the setting is asked before `ssh -G`.
        let off = Settings {
            remote_integration: false,
            ..Settings::default()
        };
        let runner = Gconfig::new(PLAIN);
        assert_eq!(
            decide(&args, true, &off, &runner, &learned(), BOOT_STUB, None, &[]),
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
                &mut out,
            );
            assert_eq!(code, 0);
            out
        };
        // `--block` carries the parent; a malformed one wraps without it.
        for (block, parent) in [("7", Some(7)), ("", None), ("x", None)] {
            assert_eq!(
                run(&["--tty", "--block", block, "--", "prod"], BOOT_STUB),
                wrap(&words(&["prod"]), BOOT_STUB, parent, None)
                    .iter()
                    .flat_map(|arg| arg.bytes().chain([0]))
                    .collect::<Vec<u8>>(),
                "{block:?}"
            );
        }
        let printed = run(&["--tty", "--", "-p", "2", "prod"], BOOT_STUB);
        let expected: Vec<u8> = wrap(&words(&["-p", "2", "prod"]), BOOT_STUB, None, None)
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

    /// Phase-5 (R7): with bateri's instance directory, the session becomes a
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
            wrap(&words(&["prod"]), BOOT_STUB, None, Some(&control))
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
            wrap(&words(&["prod"]), BOOT_STUB, Some(4), Some(&control))
        );
        // The user's own sharing, an unknown or a missing instance: no sharing,
        // still wrapped.
        let plain = wrap(&words(&["prod"]), BOOT_STUB, None, None);
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
                wrap(&words(typed), BOOT_STUB, None, None),
                "{typed:?}"
            );
        }
        fs::remove_dir_all(&root).unwrap();
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
        let command = remote_command(boot, Some(u32::MAX));
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

    /// The greeting's learning (phase-2): one `posix` row per server, the
    /// second greeting writes nothing, an argv `ssh -G` cannot read learns
    /// nothing — and an unreadable state file wraps nothing.
    #[test]
    fn a_greeting_teaches_the_server_once() {
        let dir = scratch("learn");
        let path = dir.join("remote-hosts");
        let ssh = words(&["/usr/bin/ssh", "-o", "ControlPath=/s/%C", "prod"]);
        let runner = Gconfig::new(PLAIN);
        assert!(learn(&runner, &ssh, &path).unwrap());
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
        assert!(!learn(&runner, &ssh, &path).unwrap(), "learned already");
        assert_eq!(fs::read_to_string(&path).unwrap(), written);
        assert!(!learn(&Gconfig::new("garbage"), &ssh, &path).unwrap());
        assert!(!learn(&runner, &[], &path).unwrap());
        // A state file that cannot be read knows no server: nothing is wrapped.
        let unreadable = dir.join("a-directory");
        fs::create_dir_all(&unreadable).unwrap();
        let args = words(&["prod"]);
        assert_eq!(
            decide(
                &args,
                true,
                &Settings::default(),
                &runner,
                &load(&unreadable),
                boot(),
                None,
                &[],
            ),
            None
        );
        fs::remove_dir_all(&dir).unwrap();
    }
}
/// The bootstrap on real shells (048 phase-2): what sshd does with the wrapped
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

    /// The remote blocks end to end (048 phase-3): the server's `true`,
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
                    vec!["-c".to_owned(), remote_command(boot(), Some(PARENT))],
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
                tab_id: None,
                hostname: None,
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
}
