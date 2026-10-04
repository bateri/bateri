//! Which ssh connection a remote file job rides on: bateri's own master
//! connection, the user's, or a new master of ours — decided **once, before the
//! job starts**.
//!
//! The pieces here are the pure and the process half of that gate; every
//! remote file job goes through [`Masters::ensure`]:
//!
//! - **Route** ([`Route`], [`plan`]): our socket alive (`-O check`) → ours; the
//!   user's own master alive (`ssh -G`'s `ControlPath` + `-O check`) → today's
//!   argv; otherwise open ours. The process calls are behind a seam
//!   ([`SshRunner`]); [`SystemSsh`] is the real body.
//! - **Socket path** ([`socket_path`], [`host_key`]): a short base + a 16-digit
//!   digest of the canonical (user, host, port, jump) quadruple, checked against
//!   the unix socket limit **including** ssh's temporary suffix; a fallback base,
//!   and [`Route::Direct`] (today's behaviour) when neither fits.
//! - **Master connection** ([`master_argv`], [`askpass_env`]): `-M -N -f` with
//!   `BatchMode=no` and `StrictHostKeyChecking=yes`; the askpass variables are a
//!   value for **that** `Command` only — nobody calls `set_var`.
//! - **Askpass** ([`classify`], [`run_askpass`], [`read_request`],
//!   [`write_answer`]): the prompt's class and the wire between the askpass
//!   helper (the same `bateri` binary) and the application. The wire's single
//!   owner is this module.
//! - **The user's terminal session** ([`session_socket`]): a
//!   wrapped interactive `ssh` is a master at `u-<key>` in this instance's
//!   directory ([`crate::ssh_wrap`] adds the options) and a job rides it
//!   ([`Route::Ours`]) before ours is opened. Its lifetime is the user's: a
//!   short [`SESSION_PERSIST`] detaches it (the user's `exit` does not wait for
//!   our jobs) and ends it once the user's session and our jobs are gone —
//!   bateri never sends it `-O exit`, so neither a session end nor ⌘Q can cut
//!   the user's terminal.
//! - **Saved passwords** ([`PasswordStore`], [`Account`]): the
//!   platform shell's store (the macOS Keychain) behind a trait. A saved
//!   password answers the first password prompt without a sheet; a background
//!   job opens a master **only** with one (one prompt), and a saved password
//!   the server refused is not tried again in the background until the user
//!   signs in ([`Masters`]'s flag) — a stale password must not lock the
//!   account out.

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock, PoisonError, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use bt_core::RemoteTarget;

use crate::focus::FOCUS_SOCKET;
use crate::handover::{holder_listening, is_holder_socket_name};
use crate::upload::connection;

// ─── route ───────────────────────────────────────────────────────────────

/// The connection a job's stream rides on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Route {
    /// bateri's own master connection: the stream gets `-o ControlPath=<socket>`.
    Ours(PathBuf),
    /// Today's argv: the user's own master (or a key/agent, or nothing at all).
    Direct,
}

/// What the gate decides before a job starts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Plan {
    /// A connection is there (or no socket of ours is possible): use it.
    Ready(Route),
    /// Neither master is alive: open ours at this socket, then use [`Route::Ours`].
    Open(PathBuf),
}

/// The answer of `ssh -O check` on a control socket.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Check {
    /// A master answers.
    Live,
    /// No socket there (or no control path at all).
    Absent,
    /// A socket file nobody listens on (`Connection refused`) — a master that died
    /// without removing it. It is removed: a stale socket would turn the next
    /// `-M` into a background connection without multiplexing.
    Stale,
}

/// `-O check`'s exit code and standard error as a [`Check`].
pub fn classify_check(code: Option<i32>, stderr: &str) -> Check {
    if code == Some(0) {
        Check::Live
    } else if stderr.contains("Connection refused") {
        Check::Stale
    } else {
        Check::Absent
    }
}

/// The route's **decision** half: our socket's check, the user's terminal
/// session's master if it is alive ([`session_socket`]), the
/// user's own master's check (`None` when the user has no control path) and
/// where our socket would be (`None` when no base fits the socket limit).
///
/// Ours first: once bateri has a master to the host, the user's configuration
/// need not be asked again. The user's terminal session next: it is the
/// connection the user signed in to — riding it asks for nothing. The
/// user's own master next: it is already open and the user chose it. Only then
/// a master of ours.
pub fn decide(
    ours: Check,
    session: Option<&Path>,
    user: Option<Check>,
    socket: Option<&Path>,
) -> Plan {
    let Some(socket) = socket else {
        return Plan::Ready(Route::Direct);
    };
    if ours == Check::Live {
        return Plan::Ready(Route::Ours(socket.to_owned()));
    }
    if let Some(session) = session {
        return Plan::Ready(Route::Ours(session.to_owned()));
    }
    if user == Some(Check::Live) {
        return Plan::Ready(Route::Direct);
    }
    Plan::Open(socket.to_owned())
}

/// The seam in front of the ssh process: argv in, (exit code, stdout, stderr)
/// out. [`SystemSsh`] runs it; the tests answer from a table.
pub trait SshRunner {
    fn run(&self, argv: &[String]) -> io::Result<(Option<i32>, String, String)>;
}

/// The real body: the program with no standard input, its output collected.
pub struct SystemSsh;

impl SshRunner for SystemSsh {
    fn run(&self, argv: &[String]) -> io::Result<(Option<i32>, String, String)> {
        let (program, args) = argv
            .split_first()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "empty argv"))?;
        let output = Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .output()?;
        Ok((
            output.status.code(),
            String::from_utf8_lossy(&output.stdout).into_owned(),
            String::from_utf8_lossy(&output.stderr).into_owned(),
        ))
    }
}

/// What `ssh -G` says about the target: the four parts of the socket's key and
/// the user's own connection sharing.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SshConfig {
    pub user: String,
    pub hostname: String,
    pub port: String,
    /// `none` when there is no jump host (ssh's own spelling).
    pub proxyjump: String,
    /// `None` for `none` or an absent line.
    pub control_path: Option<PathBuf>,
    /// `ControlMaster` is anything but `no` (`-G` spells it `false`): the user
    /// shares connections themselves (the session socket adds nothing then).
    pub control_master: bool,
}

impl SshConfig {
    /// Whether the user's configuration shares connections itself: a
    /// `ControlMaster` or a `ControlPath` of their own. A wrapped session then
    /// gets none of ours — the command line's `-o` would override theirs.
    pub fn shares_connections(&self) -> bool {
        self.control_master || self.control_path.is_some()
    }
}

/// `ssh -G`'s output (one `key value` per line, keys in lower case). `None`
/// without a host name: an output without one is not a configuration.
pub fn parse_config(out: &str) -> Option<SshConfig> {
    let mut config = SshConfig {
        proxyjump: "none".to_owned(),
        ..SshConfig::default()
    };
    for line in out.lines() {
        let Some((key, value)) = line.trim().split_once(' ') else {
            continue;
        };
        let value = value.trim();
        match key {
            "user" => config.user = value.to_owned(),
            "hostname" => config.hostname = value.to_owned(),
            "port" => config.port = value.to_owned(),
            "proxyjump" => config.proxyjump = value.to_owned(),
            "controlpath" if value != "none" => config.control_path = Some(PathBuf::from(value)),
            "controlmaster" => {
                config.control_master =
                    !(value.eq_ignore_ascii_case("false") || value.eq_ignore_ascii_case("no"));
            }
            _ => {}
        }
    }
    (!config.hostname.is_empty()).then_some(config)
}

/// The argv of `ssh -G`, `-O check` and the master: the user's program and
/// kept options, then ours, then the destination.
fn with_options(target: &RemoteTarget, ours: &[String]) -> Vec<String> {
    let connection = connection(target);
    let mut argv = vec![connection.program];
    argv.extend(ours.iter().cloned());
    argv.extend(connection.options);
    argv.push(connection.destination);
    argv
}

fn words(list: &[&str]) -> Vec<String> {
    list.iter().map(|&word| word.to_owned()).collect()
}

/// Whether the user's argv names a control path itself (`-S`, `-o
/// ControlPath=`). Such a target is always [`Route::Direct`]: the stream's
/// `ControlPath` goes behind the user's options and would lose to theirs, and
/// the user asked for that socket explicitly.
fn names_control_path(target: &RemoteTarget) -> bool {
    let options = connection(target).options;
    options.iter().enumerate().any(|(at, option)| {
        option == "-S"
            || (option == "-o"
                && options.get(at + 1).is_some_and(|value| {
                    value
                        .get(..11)
                        .is_some_and(|key| key.eq_ignore_ascii_case("controlpath"))
                }))
    })
}

/// The route's gate before a job ([`decide`] on real answers): `ssh -G`, our
/// socket's `-O check` and the user's. A stale socket of ours is removed here.
/// An unreadable configuration is [`Route::Direct`] — today's behaviour.
pub fn plan(runner: &dyn SshRunner, target: &RemoteTarget, bases: &[PathBuf]) -> Plan {
    resolve(runner, target, bases).0
}

/// What [`resolve`] read: the configuration ([`Account::from_config`]'s input)
/// and where our socket for it is (`None`: no base fits).
type Resolved = (Plan, Option<SshConfig>, Option<PathBuf>);

/// `ssh -G`'s configuration of the target; `None` if it cannot be read.
fn config(runner: &dyn SshRunner, target: &RemoteTarget) -> Option<SshConfig> {
    runner
        .run(&with_options(target, &words(&["-G"])))
        .ok()
        .filter(|(code, _, _)| *code == Some(0))
        .and_then(|(_, out, _)| parse_config(&out))
}

/// [`plan`] and the configuration it read — the saved password's key
/// ([`Account::from_config`]); `None` for a target that names its own socket.
fn resolve(runner: &dyn SshRunner, target: &RemoteTarget, bases: &[PathBuf]) -> Resolved {
    if names_control_path(target) {
        return (Plan::Ready(Route::Direct), None, None);
    }
    let Some(config) = config(runner, target) else {
        return (Plan::Ready(Route::Direct), None, None);
    };
    let key = host_key(&config);
    let socket = socket_path(bases, &key);
    let ours = match &socket {
        Some(socket) => check(runner, target, Some(socket)),
        None => Check::Absent,
    };
    if ours == Check::Stale
        && let Some(socket) = &socket
    {
        let _ = std::fs::remove_file(socket);
    }
    // The user's terminal session, asked only when ours is not
    // alive. A stale one is left to ssh: `ControlMaster=auto` unlinks it.
    let session = session_socket(bases, &key).filter(|session| {
        ours != Check::Live
            && session.exists()
            && check(runner, target, Some(session)) == Check::Live
    });
    // Asked only when neither is alive: the answer would not change the route.
    let user = config
        .control_path
        .as_ref()
        .filter(|path| {
            ours != Check::Live && session.is_none() && Some(path.as_path()) != socket.as_deref()
        })
        .map(|_| check(runner, target, None));
    (
        decide(ours, session.as_deref(), user, socket.as_deref()),
        Some(config),
        socket,
    )
}

/// `ssh -O {op}` on our socket (`Some`) or the user's (`None`): `check`,
/// Forget Password's `stop`. `BatchMode=yes`: a control command never asks.
fn control_argv(target: &RemoteTarget, socket: Option<&Path>, op: &str) -> Vec<String> {
    let mut ours = words(&["-o", "BatchMode=yes"]);
    if let Some(socket) = socket {
        ours.push("-o".to_owned());
        ours.push(format!("ControlPath={}", socket.display()));
    }
    ours.extend(words(&["-O", op]));
    with_options(target, &ours)
}

/// `ssh -O check` on our socket (`Some`) or on the user's own control path
/// (`None`: the target's argv and configuration decide it). `BatchMode=yes`:
/// the check never connects, and if it did, it must not ask.
fn check(runner: &dyn SshRunner, target: &RemoteTarget, socket: Option<&Path>) -> Check {
    match runner.run(&control_argv(target, socket, "check")) {
        Ok((code, _, stderr)) => classify_check(code, &stderr),
        Err(_) => Check::Absent,
    }
}

// ─── socket path ─────────────────────────────────────────────────────────

/// The digest's length in hex digits. 64 bits: two hosts colliding in one
/// user's socket directory is not a practical event, and every byte counts
/// against the socket limit.
pub const KEY_DIGITS: usize = 16;

/// ssh's temporary suffix while it binds a master socket: `.` + 16 random
/// characters (OpenSSH `mux.c`, `muxserver_listen`), renamed onto the path
/// once listening. The limit is checked against the **longer** name.
pub const SSH_TEMP_SUFFIX: usize = 17;

/// `sun_path`'s size on this platform (104 on macOS, 108 on Linux), the
/// terminating NUL included.
pub const SUN_PATH: usize =
    std::mem::size_of::<libc::sockaddr_un>() - std::mem::offset_of!(libc::sockaddr_un, sun_path);

/// The socket's name: FNV-1a 64 of the canonical (user, host name, port, jump)
/// quadruple as [`KEY_DIGITS`] hex digits. The **one** place of the algorithm —
/// the user's terminal session is a master at the same path. FNV and not `DefaultHasher`: the name must not change with the
/// Rust version, a master outlives the process that opened it.
pub fn host_key(config: &SshConfig) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for part in [
        &config.user,
        &config.hostname,
        &config.port,
        &config.proxyjump,
    ] {
        for byte in part.bytes().chain([0]) {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
    }
    let mut key = String::with_capacity(KEY_DIGITS);
    let _ = write!(key, "{hash:016x}");
    key
}

/// Whether a socket at `path` fits `sun_path` while ssh binds it under the
/// temporary suffix, and survives `-o ControlPath=` (found in
/// code review): ssh splits an option's value on whitespace and expands
/// `%` tokens — a home with a space or a `%` would make every argv carrying
/// it a configuration error. Such a base is skipped (`/tmp/bateri-$UID` next).
pub fn fits(path: &Path) -> bool {
    path.as_os_str().len() + SSH_TEMP_SUFFIX < SUN_PATH
        && path
            .to_str()
            .is_some_and(|text| !text.contains(|c: char| c.is_whitespace() || c == '%' || c == '"'))
}

/// The socket bases in order: the user's cache directory, then a short
/// `/tmp/bateri-$UID`. `None` home skips the first.
pub fn socket_bases(home: Option<&Path>, uid: u32) -> Vec<PathBuf> {
    let mut bases = Vec::new();
    if let Some(home) = home {
        #[cfg(target_os = "macos")]
        bases.push(home.join("Library/Caches/bateri/s"));
        #[cfg(not(target_os = "macos"))]
        bases.push(home.join(".cache/bateri/s"));
    }
    bases.push(PathBuf::from(format!("/tmp/bateri-{uid}")));
    bases
}

/// The first base whose socket fits the limit and whose directory is ours
/// ([`prepare_dir`]). `None`: no socket of ours is possible and the route is
/// [`Route::Direct`] — today's behaviour.
pub fn socket_path(bases: &[PathBuf], key: &str) -> Option<PathBuf> {
    bases
        .iter()
        .map(|base| base.join(key))
        .find(|path| fits(path) && path.parent().is_some_and(|dir| prepare_dir(dir).is_ok()))
}

/// The user's terminal session's socket name: [`SESSION_PREFIX`] + the key.
/// Not one of our names ([`our_socket_name`]): neither a session end, nor ⌘Q
/// ([`Masters::close_all`]), nor the sweep sends it `-O exit` — it carries the
/// user's terminal.
pub const SESSION_PREFIX: &str = "u-";

/// How long the user's terminal session's master stays with nothing on it —
/// a **design constant**. Any `ControlPersist` detaches the
/// master, so the user's `exit` returns at once (with `no` it waited for every
/// job riding it — measured: a 10 s stream held the `exit` 10 s, and the
/// helper's stream is long-lived); short, so the connection ends with the
/// user's session and our jobs on it, not later.
pub const SESSION_PERSIST: Duration = Duration::from_secs(2);

/// Where the user's terminal session to `key` is a master: the first base
/// (this instance's directories, in [`socket_bases`]' order) whose
/// `u-<key>` fits the socket limit and is a private directory. Creates
/// nothing: the wrapping (`bateri ssh-argv`, another process) only uses a
/// directory bateri already made, and the route asks the same function.
pub fn session_socket(bases: &[PathBuf], key: &str) -> Option<PathBuf> {
    bases
        .iter()
        .map(|base| base.join(format!("{SESSION_PREFIX}{key}")))
        .find(|path| fits(path) && path.parent().is_some_and(private_dir))
}

/// This instance's directories under `roots` that exist — for `bateri
/// ssh-argv`, which runs outside the instance: a private directory of this
/// user with an owner file ([`prepare_instance`] made it). `instance` must be
/// an instance name ([`INSTANCE_DIGITS`] hex digits); anything else gives none.
pub fn instance_dirs(roots: &[PathBuf], instance: &str) -> Vec<PathBuf> {
    if !hex_name(instance, INSTANCE_DIGITS) {
        return Vec::new();
    }
    roots
        .iter()
        .map(|root| root.join(instance))
        .filter(|dir| owner(dir).is_some())
        .collect()
}

/// The socket directory: created `0700`, and if it is there already it must be
/// a real directory (not a link), owned by this user and closed to everyone
/// else — `/tmp` is shared and a directory planted there would see the socket.
pub fn prepare_dir(dir: &Path) -> io::Result<()> {
    if let Some(parent) = dir.parent() {
        std::fs::create_dir_all(parent)?;
    }
    match std::fs::DirBuilder::new().mode(0o700).create(dir) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error),
    }
    let meta = std::fs::symlink_metadata(dir)?;
    // SAFETY: `getuid` has no preconditions and cannot fail.
    let uid = unsafe { libc::getuid() };
    if !meta.file_type().is_dir() || meta.uid() != uid || meta.mode() & 0o077 != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "socket directory is not private",
        ));
    }
    Ok(())
}

// ─── master connection ───────────────────────────────────────────────────

/// How long our master stays up with nothing riding on it — a **design
/// constant**: long enough that a session of drops and ⌘-clicks opens one
/// connection, short enough that an idle server is not held. It is the stop
/// condition of a master whose owner died; otherwise the user's last session
/// to the host ([`Masters::session_ended`]) and quitting
/// ([`Masters::close_all`]) end it first.
pub const CONTROL_PERSIST: Duration = Duration::from_secs(600);

/// How long our master may take to reach the server (`ConnectTimeout`) — a
/// **design constant**, the helper's `OPEN_TIMEOUT`: an unreachable host must
/// not hold the job (and the pane's helper worker behind it) for the TCP
/// timeout. It bounds the connection, not the time at the sheet.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

/// Who asked for the master: a job the user started (a sheet may ask for the
/// password) or a background job (one silent attempt from the Keychain).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Asker {
    User,
    Background,
}

/// The master connection's argv: `-M -N -f` and our options **first** (ssh
/// takes a key's first value; this master is ours and must not become
/// something else), then the user's connection options from the same filter
/// as the stream ([`crate::upload::ssh_argv_for`]), then the destination.
///
/// `BatchMode=no`: the password goes through askpass. `StrictHostKeyChecking=yes`:
/// an unknown host key is refused, never asked. A background master
/// gets **one** password prompt — a stale Keychain password must not lock the
/// account out.
pub fn master_argv(target: &RemoteTarget, socket: &Path, asker: Asker) -> Vec<String> {
    let mut ours = words(&["-M", "-N", "-f", "-o"]);
    ours.push(format!("ControlPath={}", socket.display()));
    ours.push("-o".to_owned());
    ours.push(format!("ControlPersist={}", CONTROL_PERSIST.as_secs()));
    ours.push("-o".to_owned());
    ours.push(format!("ConnectTimeout={}", CONNECT_TIMEOUT.as_secs()));
    ours.extend(words(&[
        "-o",
        "BatchMode=no",
        "-o",
        "StrictHostKeyChecking=yes",
    ]));
    if asker == Asker::Background {
        ours.extend(words(&["-o", "NumberOfPasswordPrompts=1"]));
    }
    with_options(target, &ours)
}

/// The variable carrying the askpass socket to the helper.
pub const ASKPASS_VAR: &str = "BATERI_ASKPASS";

/// The askpass environment of **one** master's `Command`: ssh runs `program`
/// (the `bateri` binary) for every prompt, `force` even with a terminal, and
/// the helper finds the application at `socket`. A value, not `set_var`: the
/// shell's environment and the stream processes never see it.
pub fn askpass_env(program: &Path, socket: &Path) -> Vec<(String, PathBuf)> {
    vec![
        ("SSH_ASKPASS".to_owned(), program.to_owned()),
        ("SSH_ASKPASS_REQUIRE".to_owned(), PathBuf::from("force")),
        (ASKPASS_VAR.to_owned(), socket.to_owned()),
    ]
}

// ─── askpass ─────────────────────────────────────────────────────────────

/// What ssh asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Prompt {
    /// The account's password — the one the Keychain may remember.
    Password,
    /// Anything else: a key's passphrase, a verification code. Never remembered.
    Other,
}

/// The prompt's class. A password prompt names the password at its end
/// (`deploy@prod's password:`, `Password:`, `(deploy@prod) Password:`, PAM's
/// `Parola:`); a word of a passphrase, a code or a key anywhere makes it
/// [`Prompt::Other`] — the wrong way round would put a passphrase or a one-time
/// code into the Keychain.
pub fn classify(prompt: &str) -> Prompt {
    let text = prompt.trim().to_lowercase();
    const OTHER: [&str; 8] = [
        "passphrase",
        "verification",
        "code",
        "token",
        "otp",
        "one-time",
        "anahtar",
        "doğrulama",
    ];
    if OTHER.iter().any(|word| text.contains(word)) {
        return Prompt::Other;
    }
    let Some(head) = text.strip_suffix(':') else {
        return Prompt::Other;
    };
    let last = head
        .rsplit(|c: char| c.is_whitespace() || c == '\'')
        .next()
        .unwrap_or_default();
    match last {
        "password" | "parola" | "şifre" | "sifre" | "parolası" | "şifresi" => Prompt::Password,
        _ => Prompt::Other,
    }
}

/// The wire's request header: `BATERI-ASKPASS 1 <length>\n`, then the prompt.
const REQUEST: &str = "BATERI-ASKPASS 1";

/// The longest prompt or answer the wire carries; a longer length is a broken
/// peer and the request is refused rather than allocated.
pub const WIRE_LIMIT: usize = 16 * 1024;

/// How long the helper waits for the answer — longer than any person at a sheet.
/// It is not the stop condition (a closed pane or ⌘Q drops the other end and
/// the read fails at once); it guards against a peer that never answers.
const ANSWER_WAIT: Duration = Duration::from_secs(30 * 60);

/// One header line + a body of the declared length.
fn read_frame(reader: &mut impl BufRead, prefix: &str) -> io::Result<Vec<u8>> {
    let broken = || io::Error::new(io::ErrorKind::InvalidData, "broken askpass frame");
    let mut header = String::new();
    let limit = (prefix.len() + 32) as u64;
    reader.by_ref().take(limit).read_line(&mut header)?;
    let length: usize = header
        .strip_suffix('\n')
        .and_then(|line| line.strip_prefix(prefix))
        .and_then(|rest| rest.strip_prefix(' '))
        .and_then(|digits| digits.parse().ok())
        .ok_or_else(broken)?;
    if length > WIRE_LIMIT {
        return Err(broken());
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body)?;
    Ok(body)
}

fn write_frame(writer: &mut impl Write, prefix: &str, body: &[u8]) -> io::Result<()> {
    writer.write_all(format!("{prefix} {}\n", body.len()).as_bytes())?;
    writer.write_all(body)?;
    writer.flush()
}

/// The application's half: the prompt the helper sent.
pub fn read_request(reader: &mut impl BufRead) -> io::Result<String> {
    let body = read_frame(reader, REQUEST)?;
    String::from_utf8(body).map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "prompt"))
}

/// The application's half: the answer, or `None` — cancelled, and the helper
/// exits non-zero. **No empty answer is ever sent for a question nobody
/// answered**: ssh would try it as a password.
pub fn write_answer(writer: &mut impl Write, answer: Option<&str>) -> io::Result<()> {
    match answer {
        Some(answer) => write_frame(writer, "OK", answer.as_bytes()),
        None => {
            writer.write_all(b"CANCEL\n")?;
            writer.flush()
        }
    }
}

/// The helper's body (`bateri` as `SSH_ASKPASS`): send the prompt to the
/// socket, write the answer and a newline to `out`. The exit code: 0 with an
/// answer, 1 when cancelled or the connection is gone — ssh then gives up on
/// that question.
pub fn run_askpass(socket: &Path, prompt: &str, out: &mut impl Write) -> i32 {
    match ask(socket, prompt) {
        Ok(Some(answer)) => {
            let written = out
                .write_all(answer.as_bytes())
                .and_then(|()| out.write_all(b"\n"))
                .and_then(|()| out.flush());
            i32::from(written.is_err())
        }
        Ok(None) | Err(_) => 1,
    }
}

fn ask(socket: &Path, prompt: &str) -> io::Result<Option<String>> {
    let mut stream = UnixStream::connect(socket)?;
    stream.set_read_timeout(Some(ANSWER_WAIT))?;
    write_frame(&mut stream, REQUEST, prompt.as_bytes())?;
    let mut reader = BufReader::new(stream);
    let mut header = String::new();
    reader.by_ref().take(64).read_line(&mut header)?;
    if header == "CANCEL\n" {
        return Ok(None);
    }
    let length: usize = header
        .strip_suffix('\n')
        .and_then(|line| line.strip_prefix("OK "))
        .and_then(|digits| digits.parse().ok())
        .filter(|&length| length <= WIRE_LIMIT)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "broken askpass answer"))?;
    let mut body = vec![0; length];
    reader.read_exact(&mut body)?;
    String::from_utf8(body)
        .map(Some)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "answer"))
}

/// The `bateri` binary as ssh's askpass: `Some(exit code)` when the
/// process was started as one ([`ASKPASS_VAR`] set), `None` otherwise. The
/// prompt is ssh's first argument; the answer goes to standard output and
/// nothing else does. No AppKit, no window server: `main` calls this first.
pub fn askpass_main() -> Option<i32> {
    let socket = std::env::var_os(ASKPASS_VAR)?;
    let prompt = std::env::args_os()
        .nth(1)
        .map(|prompt| prompt.to_string_lossy().into_owned())
        .unwrap_or_default();
    Some(run_askpass(
        Path::new(&socket),
        &prompt,
        &mut io::stdout().lock(),
    ))
}

// ─── saved passwords ─────────────────────────────────────────────────────

/// The key of one saved password: the host name, user and port
/// ssh resolves (`ssh -G`) — the alias the user typed is not the server.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Account {
    pub host: String,
    pub user: String,
    pub port: u16,
}

impl Account {
    /// From `ssh -G`'s answer; `None` without a user or a readable port.
    pub fn from_config(config: &SshConfig) -> Option<Self> {
        let port = config.port.parse().ok()?;
        (!config.user.is_empty()).then(|| Self {
            host: config.hostname.clone(),
            user: config.user.clone(),
            port,
        })
    }

    /// The item's label — a UI string: the user finds it by "bateri" in
    /// Keychain Access.
    pub fn label(&self) -> String {
        format!("bateri \u{2014} {}@{}:{}", self.user, self.host, self.port)
    }
}

/// Where the account passwords are remembered: the macOS Keychain in the
/// application process (`bt-shell-macos::keychain`), [`NoStore`] elsewhere.
/// Only the application calls it — the askpass helper never does. Writes are
/// best effort: a store that refuses leaves the password unremembered, the
/// login itself already happened.
pub trait PasswordStore: Send + Sync {
    fn read(&self, account: &Account) -> Option<String>;
    fn write(&self, account: &Account, password: &str);
    fn delete(&self, account: &Account);
    /// Whether a password is saved, without reading it (no consent prompt): the
    /// menu's Forget Password.
    fn contains(&self, account: &Account) -> bool;
}

/// No store: nothing is remembered (Linux, the tests that do not look).
pub struct NoStore;

impl PasswordStore for NoStore {
    fn read(&self, _: &Account) -> Option<String> {
        None
    }
    fn write(&self, _: &Account, _: &str) {}
    fn delete(&self, _: &Account) {}
    fn contains(&self, _: &Account) -> bool {
        false
    }
}

// ─── opening our master ──────────────────────────────────────────────────

/// A question ssh asks while our master opens.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Question {
    /// ssh's own prompt text.
    pub prompt: String,
    pub class: Prompt,
    /// A password was already typed in this attempt and ssh asks again: it was
    /// wrong (`NumberOfPasswordPrompts` gives the signal for free).
    pub again: bool,
    /// The saved password was given and ssh asks again: the saved one did not
    /// work. The sheet says so; a successful new password replaces it.
    pub stale: bool,
}

/// A typed answer: the text and whether to remember it (the sheet's Remember
/// in Keychain box; meaningful for a [`Prompt::Password`] only).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Typed {
    pub text: String,
    pub remember: bool,
}

/// Who answers a [`Question`]: the platform shell's sheet, on the job's
/// thread (it blocks until the sheet closes). `None`: nobody answered —
/// cancelled, the pane closed, the application quit.
pub type Answerer = Box<dyn FnMut(&Question) -> Option<Typed> + Send>;

/// How the gate may open a master: a job the user started asks at a sheet
/// (after the saved password); a background job (link check, load indicator)
/// opens one only with a saved password, in one silent attempt.
pub enum Ask {
    Sheet(Answerer),
    Never,
}

/// Why no route came back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Denied {
    /// A question went unanswered: the job ends with [`CANCELLED`].
    Cancelled,
    /// A background job cannot log in by itself — no saved password, or the
    /// saved one was refused: the pane offers Sign In… ([`SIGN_IN_NEEDED`]).
    SignIn,
    /// ssh refused: the job's error text.
    Failed(String),
}

/// The text of a cancelled job — the consumers recognise it and end without an
/// error sheet (one sheet just closed; a second saying "cancelled" is noise).
pub const CANCELLED: &str = "Cancelled";

/// The text of a background job that needs the user to sign in — the
/// link label says it and the pane recognises it to show the status bar's
/// Sign In… button. A UI string.
pub const SIGN_IN_NEEDED: &str = "Sign in to use remote files";

impl Denied {
    pub fn text(&self) -> String {
        match self {
            Self::Cancelled => CANCELLED.to_owned(),
            Self::SignIn => SIGN_IN_NEEDED.to_owned(),
            Self::Failed(text) => text.clone(),
        }
    }
}

/// Whether ssh's standard error says the server refused the login (not the
/// connection).
pub fn login_refused(stderr: &str) -> bool {
    stderr.contains("Permission denied")
}

/// Whether the refused login was one a password could open: the server
/// lists `password` or `keyboard-interactive` among its methods
/// (`Permission denied (publickey,password).`). A key-only refusal is not —
/// a password sheet cannot help there, ssh's own reason stays.
pub fn password_refused(stderr: &str) -> bool {
    stderr.lines().any(|line| {
        line.contains("Permission denied")
            && (line.contains("password") || line.contains("keyboard-interactive"))
    })
}

/// The text of a master that did not open, from ssh's standard error. An
/// unknown host key is refused, never asked (`StrictHostKeyChecking=yes`).
pub fn open_failure(host: &str, stderr: &str) -> String {
    let last = crate::upload::last_line(stderr);
    if stderr.contains("Host key verification failed") {
        format!(
            "The host key of {host} is not known yet — connect once in the terminal, then try \
             again."
        )
    } else if last.is_empty() {
        format!("ssh could not connect to {host}.")
    } else if login_refused(stderr) {
        format!("ssh could not log in to {host}.\n\n{last}")
    } else {
        format!("ssh could not connect to {host}.\n\n{last}")
    }
}

/// One opening in progress: whoever asks for the same socket meanwhile waits
/// for its outcome instead of opening (and asking) a second time.
#[derive(Default)]
struct Flight {
    outcome: Mutex<Option<Result<(), Denied>>>,
    done: Condvar,
}

impl Flight {
    fn finish(&self, outcome: Result<(), Denied>) {
        *lock(&self.outcome) = Some(outcome);
        self.done.notify_all();
    }

    fn wait(&self) -> Result<(), Denied> {
        let mut outcome = lock(&self.outcome);
        loop {
            if let Some(outcome) = outcome.as_ref() {
                return outcome.clone();
            }
            outcome = self
                .done
                .wait(outcome)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// bateri's masters: the application owns one (`PaneLaunch` hands it to the
/// panes — not a `static`), the gate runs through it on the jobs' threads.
/// Single flight per socket: two jobs to the same host open **one** master
/// and show one sheet.
pub struct Masters {
    /// The askpass program — the running `bateri` binary.
    askpass: PathBuf,
    /// The socket roots ([`socket_bases`]); this instance's directory is under
    /// each ([`prepare_instance`]).
    roots: Vec<PathBuf>,
    /// This instance's directory name: random, chosen at birth.
    instance: String,
    /// The socket bases: this instance's directory under each root that could
    /// hold one — prepared once, on the first job's thread.
    bases: OnceLock<Vec<PathBuf>>,
    store: Arc<dyn PasswordStore>,
    flights: Mutex<HashMap<PathBuf, Arc<Flight>>>,
    /// The accounts whose saved password the server refused: a
    /// background job does not try them again. In memory, for the
    /// application's lifetime; neither a new remote generation nor time clears
    /// it — only the user's successful sign-in and Forget Password do.
    rejected: Mutex<HashSet<Account>>,
    /// The account each target resolved to, by its argv — the main thread's
    /// question (Forget Password's menu item) must not start `ssh -G`.
    accounts: Mutex<HashMap<Vec<String>, Account>>,
    /// Whether each account has a saved password, as the store last said —
    /// the menu's question is answered from here, so the main thread never
    /// waits on the Keychain (a locked keychain would prompt).
    known: Mutex<HashMap<Account, bool>>,
    /// The socket each target resolved to, by its argv — which panes share a
    /// master ([`Self::session_ended`]).
    sockets: Mutex<HashMap<Vec<String>, PathBuf>>,
    /// The panes whose terminal is in a remote session, and its target.
    sessions: Mutex<HashMap<u64, RemoteTarget>>,
    /// The application quits ([`Self::begin_quit`]): no pane's session end
    /// sends its own `exit` — [`Self::close_all`] does it under the deadline —
    /// and a master that comes up now is ended at once.
    closing: AtomicBool,
    /// How many jobs joined someone else's flight (the single-flight test
    /// waits for it).
    #[cfg(test)]
    joined: std::sync::atomic::AtomicUsize,
}

impl Masters {
    /// The registry over `roots` ([`socket_bases`]), with a fresh instance
    /// directory name.
    pub fn new(askpass: PathBuf, roots: Vec<PathBuf>, store: Arc<dyn PasswordStore>) -> Self {
        Self::with_instance(askpass, roots, store, new_instance())
    }

    /// [`Self::new`] with the instance directory's name given: an instance
    /// carried over an update and the tests.
    pub fn with_instance(
        askpass: PathBuf,
        roots: Vec<PathBuf>,
        store: Arc<dyn PasswordStore>,
        instance: String,
    ) -> Self {
        Self {
            askpass,
            roots,
            instance,
            bases: OnceLock::new(),
            store,
            flights: Mutex::new(HashMap::new()),
            rejected: Mutex::new(HashSet::new()),
            accounts: Mutex::new(HashMap::new()),
            known: Mutex::new(HashMap::new()),
            sockets: Mutex::new(HashMap::new()),
            sessions: Mutex::new(HashMap::new()),
            closing: AtomicBool::new(false),
            #[cfg(test)]
            joined: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    /// The gate before a remote job ([`plan`], then the opening): our master
    /// alive → ours; the user's alive → today's argv; otherwise open ours.
    /// Blocks (ssh processes, a sheet, the Keychain): never on the main thread.
    ///
    /// - [`Ask::Sheet`]: the saved password answers the first prompt, the sheet
    ///   the rest.
    /// - [`Ask::Never`]: with a saved password not yet refused, one silent
    ///   attempt (`NumberOfPasswordPrompts=1`, no sheet); a refusal marks the
    ///   account and gives [`Denied::SignIn`], as does an account already
    ///   marked. Without a saved password: today's argv (a key, an agent).
    ///
    /// A job that finds an opening in progress waits for it — a background job
    /// too, or its today's-argv failure would be held ([`crate::remote_helper::RETRY_AFTER`])
    /// past the master coming up. An opening **cancelled** at someone else's
    /// sheet (another pane's), or a background attempt that could not log in,
    /// is not a user's answer: a user's job then asks at its own sheet.
    pub fn ensure(&self, target: &RemoteTarget, ask: Ask) -> Result<Route, Denied> {
        let user = matches!(ask, Ask::Sheet(_));
        loop {
            let (plan, account) = self.resolve(target);
            let socket = match plan {
                Plan::Ready(route) => return Ok(route),
                Plan::Open(socket) => socket,
            };
            let rejected = account
                .as_ref()
                .is_some_and(|account| lock(&self.rejected).contains(account));
            if !user && rejected {
                return Err(Denied::SignIn);
            }
            // An opening in progress is joined before anything is read: the
            // Keychain is not asked for a password this job would not send.
            let existing = lock(&self.flights).get(&socket).cloned();
            if let Some(flight) = existing {
                #[cfg(test)]
                self.joined
                    .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                match flight.wait() {
                    Ok(()) => return Ok(Route::Ours(socket)),
                    Err(Denied::Cancelled | Denied::SignIn) if user => continue,
                    Err(Denied::SignIn) => return Err(Denied::SignIn),
                    Err(_) if !user => return Ok(Route::Direct),
                    Err(denied) => return Err(denied),
                }
            }
            // The saved password — not one the server already refused: sending
            // it again would be one more failed login.
            let saved = account
                .as_ref()
                .filter(|_| !rejected)
                .and_then(|account| self.read_saved(account));
            if !user && saved.is_none() {
                return Ok(Route::Direct);
            }
            let flight = {
                let mut flights = lock(&self.flights);
                if flights.contains_key(&socket) {
                    // Another job started between the two looks: join it.
                    continue;
                }
                let flight = Arc::new(Flight::default());
                flights.insert(socket.clone(), Arc::clone(&flight));
                flight
            };
            let sheet = match ask {
                Ask::Sheet(sheet) => Some(sheet),
                Ask::Never => None,
            };
            // An account already marked: its saved password is known to be
            // refused — the sheet says so at once and a new password replaces it.
            let responder = Responder::new(sheet, saved, rejected);
            let outcome = self.open_flight(target, &socket, &flight, account.as_ref(), responder);
            return match outcome {
                Ok(()) => Ok(Route::Ours(socket)),
                // A background attempt that failed for another reason (the
                // host is unreachable): today's argv says so in today's words.
                Err(Denied::Failed(_)) if !user => Ok(Route::Direct),
                Err(denied) => Err(denied),
            };
        }
    }

    /// The store's saved password; whether there is one is remembered for
    /// [`Self::has_saved`].
    fn read_saved(&self, account: &Account) -> Option<String> {
        let saved = self.store.read(account);
        lock(&self.known).insert(account.clone(), saved.is_some());
        saved
    }

    /// Writes or deletes the saved password, and remembers which.
    fn keep(&self, account: &Account, password: Option<&str>) {
        match password {
            Some(password) => self.store.write(account, password),
            None => self.store.delete(account),
        }
        lock(&self.known).insert(account.clone(), password.is_some());
    }

    /// The socket bases: this instance's directory under each root that can
    /// hold one, created on the first call (a job's thread — directories and a
    /// rename, never the main thread). An empty list: no socket of ours is
    /// possible, every route is [`Route::Direct`]. The first is also where the
    /// focus listener lives ([`crate::focus::serve`]).
    pub fn bases(&self) -> &[PathBuf] {
        self.bases.get_or_init(|| {
            self.roots
                .iter()
                .filter_map(|root| prepare_instance(root, &self.instance).ok())
                .collect()
        })
    }

    /// [`resolve`] through the real ssh; the account and the socket go to the
    /// caches.
    fn resolve(&self, target: &RemoteTarget) -> (Plan, Option<Account>) {
        let (plan, config, socket) = resolve(&SystemSsh, target, self.bases());
        let account = config.as_ref().and_then(Account::from_config);
        if let Some(account) = &account {
            lock(&self.accounts).insert(target.argv.clone(), account.clone());
        }
        if let Some(socket) = socket {
            lock(&self.sockets).insert(target.argv.clone(), socket);
        }
        (plan, account)
    }

    /// The owner's half of [`Masters::ensure`]: open, settle the store and the
    /// refused mark, tell the joiners.
    fn open_flight(
        &self,
        target: &RemoteTarget,
        socket: &Path,
        flight: &Flight,
        account: Option<&Account>,
        mut responder: Responder,
    ) -> Result<(), Denied> {
        let background = responder.sheet.is_none();
        // Between the plan and the registry another flight may have finished:
        // its master is up and asking again would be a second sheet.
        let outcome = if check(&SystemSsh, target, Some(socket)) == Check::Live {
            Ok(())
        } else {
            let asker = if background {
                Asker::Background
            } else {
                Asker::User
            };
            open_master(target, socket, &self.askpass, &mut responder, asker)
                .and_then(|opened| self.settle(target, account, &responder, opened))
        };
        // Opened while the application quits ([`Self::close_all`] already
        // looked): it must not outlive bateri.
        if outcome.is_ok() && self.closing.load(Ordering::SeqCst) {
            let _ = SystemSsh.run(&exit_argv(&connection(target).program, socket));
        }
        lock(&self.flights).remove(socket);
        flight.finish(outcome.clone());
        outcome
    }

    /// What an attempt leaves behind: a typed password that
    /// opened the master is saved if the box was ticked; an unticked one that
    /// replaced a refused saved password removes it (the next background
    /// attempt would replay a known-bad password). A saved password the server
    /// refused marks the account; the user's success clears the mark. **Only
    /// after the master is up** is anything written — a wrong password is never
    /// saved.
    fn settle(
        &self,
        target: &RemoteTarget,
        account: Option<&Account>,
        responder: &Responder,
        opened: Opened,
    ) -> Result<(), Denied> {
        let background = responder.sheet.is_none();
        let refused = match &opened {
            Opened::Refused(stderr) => login_refused(stderr),
            _ => false,
        };
        let saved_refused = responder.saved_refused
            || (refused && responder.offered_saved && responder.typed.is_none())
            // A background attempt cannot finish the login alone (a code, a
            // passphrase after the password): it is not repeated either.
            || (background && responder.offered_saved && matches!(opened, Opened::Cancelled));
        if let Some(account) = account {
            if let Opened::Up = opened {
                match &responder.typed {
                    Some(typed) if typed.remember => self.keep(account, Some(&typed.text)),
                    Some(_) if responder.saved_refused => self.keep(account, None),
                    _ => {}
                }
                if !background {
                    lock(&self.rejected).remove(account);
                }
            } else if saved_refused {
                lock(&self.rejected).insert(account.clone());
            }
        }
        match opened {
            Opened::Up => Ok(()),
            Opened::Cancelled if background => Err(Denied::SignIn),
            Opened::Cancelled => Err(Denied::Cancelled),
            Opened::Refused(_) if background && refused => Err(Denied::SignIn),
            Opened::Refused(stderr) => Err(Denied::Failed(open_failure(&target.host, &stderr))),
        }
    }

    /// Whether a password is saved for `target` (Shell ▸ Forget Password's
    /// enablement): from the account a job already resolved — the main thread
    /// does not start `ssh -G` — so `false` until the first remote job.
    pub fn has_saved(&self, target: &RemoteTarget) -> bool {
        let account = lock(&self.accounts).get(&target.argv).cloned();
        account.is_some_and(|account| {
            let known = lock(&self.known).get(&account).copied();
            known.unwrap_or_else(|| {
                // Not asked yet this run: once, attributes only, off the cache's lock.
                let saved = self.store.contains(&account);
                lock(&self.known).insert(account, saved);
                saved
            })
        })
    }

    /// Shell ▸ Forget Password: the saved password goes, the refused
    /// mark with it, and our master for the account stops taking new jobs
    /// (`-O stop` — the transfers on it finish; never the user's master) so the
    /// next background job finds no login and the pane offers Sign In…. Runs
    /// ssh: never on the main thread.
    pub fn forget(&self, target: &RemoteTarget) {
        let Some(config) = config(&SystemSsh, target) else {
            return;
        };
        if let Some(account) = Account::from_config(&config) {
            self.keep(&account, None);
            lock(&self.rejected).remove(&account);
        }
        if names_control_path(target) {
            return;
        }
        let Some(socket) = socket_path(self.bases(), &host_key(&config)) else {
            return;
        };
        if check(&SystemSsh, target, Some(&socket)) == Check::Live {
            let _ = SystemSsh.run(&control_argv(target, Some(&socket), "stop"));
        }
    }

    /// The startup sweep: what a dead bateri left behind ([`sweep`]) — never
    /// this instance's directory — then this instance's directories, made now
    /// so the user's first wrapped `ssh` finds them, since `bateri
    /// ssh-argv` only uses a directory that is there ([`instance_dirs`]).
    pub fn sweep(&self) {
        // First: the sweep runs ssh per dead socket and the user's first
        // `ssh` must not miss the directory meanwhile (it skips our own).
        // An instance carried over an update has its masters in
        // them already: a live one is ours again — [`resolve`]'s `-O check`
        // finds it, no password asked — and a dead one goes now.
        for dir in self.bases() {
            remove_dead_sockets(dir);
        }
        sweep(&self.roots, &self.instance);
    }

    /// This instance's directory name — the panes' shells get it
    /// (`BATERI_SSH_INSTANCE`), so a wrapped `ssh` becomes a master in it
    /// ([`session_socket`]).
    pub fn instance(&self) -> &str {
        &self.instance
    }

    // ─── the user's sessions ──────────────────────────────────────────────

    /// Pane `pane`'s terminal is in a remote session to `target` (the remote
    /// edge): while it lasts, our master for the host stays. Which socket the
    /// target resolves to is learnt on its own thread (`ssh -G`) if no job
    /// has resolved it yet — an alias of the same host must hold the master
    /// too ([`Self::release`]).
    pub fn session_started(self: &Arc<Self>, pane: u64, target: &RemoteTarget) {
        lock(&self.sessions).insert(pane, target.clone());
        if lock(&self.sockets).contains_key(&target.argv) {
            return;
        }
        let (masters, target) = (Arc::clone(self), target.clone());
        let _ = thread::Builder::new()
            .name("ssh socket of a session".into())
            .spawn(move || masters.learn_socket(&target));
    }

    /// The socket `target` resolves to, into the cache — [`resolve`]'s first
    /// half without the checks.
    fn learn_socket(&self, target: &RemoteTarget) {
        if names_control_path(target) || self.bases().is_empty() {
            return;
        }
        let Some(config) = config(&SystemSsh, target) else {
            return;
        };
        if let Some(socket) = socket_path(self.bases(), &host_key(&config)) {
            lock(&self.sockets).insert(target.argv.clone(), socket);
        }
    }

    /// ⌘Q begins: the panes' session ends that follow send nothing,
    /// [`Self::close_all`] ends every master under the shared deadline.
    pub fn begin_quit(&self) {
        self.closing.store(true, Ordering::SeqCst);
    }

    /// Pane `pane`'s remote session ended (the remote edge, the pane's close):
    /// if no other pane of this instance is in a session to the same master,
    /// it is ended (`-O exit`, on its own thread — ssh, never the main
    /// thread). The transfers riding on it end with it: their session is gone.
    pub fn session_ended(&self, pane: u64) {
        let Some((target, socket)) = self.release(pane) else {
            return;
        };
        if self.closing.load(Ordering::SeqCst) {
            return;
        }
        if !socket.exists() {
            return;
        }
        let _ = thread::Builder::new()
            .name("ssh master exit".into())
            .spawn(move || {
                let _ = SystemSsh.run(&exit_argv(&connection(&target).program, &socket));
            });
    }

    /// [`Self::session_ended`]'s bookkeeping: the pane's session goes, and the
    /// master to end — `None` if the pane had none, its target never resolved
    /// to a socket of ours, or another pane's session resolves to the same
    /// socket, is the same argv, or is not resolved yet (it may be an alias of
    /// the same host: kept — the wrong way round would end a live session's
    /// master; the cost is a master left to its `ControlPersist`).
    fn release(&self, pane: u64) -> Option<(RemoteTarget, PathBuf)> {
        let mut sessions = lock(&self.sessions);
        let target = sessions.remove(&pane)?;
        let sockets = lock(&self.sockets);
        let socket = sockets.get(&target.argv)?.clone();
        let shared = sessions.values().any(|other| {
            other.argv == target.argv
                || sockets
                    .get(&other.argv)
                    .is_none_or(|theirs| *theirs == socket)
        });
        (!shared).then_some((target, socket))
    }

    /// ⌘Q: every master in this instance's directories is ended
    /// (`-O exit`, in parallel), waiting until `deadline` — the panes' close
    /// shares it. A master that does not answer in time is left to its
    /// `ControlPersist`; the directory keeps its owner file then, so the next
    /// start's sweep ends it. A clean directory is removed.
    pub fn close_all(&self, deadline: Instant) {
        self.begin_quit();
        let Some(bases) = self.bases.get() else {
            return;
        };
        let (done, results) = mpsc::channel();
        let mut waiting = 0;
        for dir in bases {
            for socket in our_sockets(dir) {
                if UnixStream::connect(&socket).is_err() {
                    let _ = std::fs::remove_file(&socket);
                    continue;
                }
                let done = done.clone();
                let spawned =
                    thread::Builder::new()
                        .name("ssh master exit".into())
                        .spawn(move || {
                            let _ = SystemSsh.run(&exit_argv("ssh", &socket));
                            let _ = done.send(());
                        });
                waiting += usize::from(spawned.is_ok());
            }
        }
        drop(done);
        for _ in 0..waiting {
            let left = deadline.saturating_duration_since(Instant::now());
            if results.recv_timeout(left).is_err() {
                return;
            }
        }
        // A master still opening ends itself ([`Self::open_flight`]); its
        // directory keeps the owner file, in case it does not make it.
        if !lock(&self.flights).is_empty() {
            return;
        }
        for dir in bases {
            remove_instance(dir);
        }
    }
}

/// A job's stream argv through the gate: [`Masters::ensure`], then
/// [`crate::upload::ssh_argv_for`]. No masters (the timed run): today's argv.
pub fn dial(
    masters: Option<&Masters>,
    target: &RemoteTarget,
    ask: Ask,
) -> Result<Vec<String>, Denied> {
    let route = match masters {
        Some(masters) => masters.ensure(target, ask)?,
        None => Route::Direct,
    };
    Ok(crate::upload::ssh_argv_for(target, &route))
}

/// Who answers one attempt's prompts: the saved password **once** (the first
/// password prompt), then the sheet — or, for a background job, nobody: the
/// second password prompt and every other prompt (a key's passphrase, a code)
/// go unanswered.
struct Responder {
    /// `None`: a background job.
    sheet: Option<Answerer>,
    saved: Option<String>,
    /// The saved password was given in this attempt.
    offered_saved: bool,
    /// ssh asked for the password again after the saved one: it was refused.
    saved_refused: bool,
    /// The last typed password — on success, the one that opened the master.
    typed: Option<Typed>,
}

impl Responder {
    /// `refused`: the saved password was already refused (not offered again).
    fn new(sheet: Option<Answerer>, saved: Option<String>, refused: bool) -> Self {
        Self {
            sheet,
            saved,
            offered_saved: false,
            saved_refused: refused,
            typed: None,
        }
    }

    /// The answer to one prompt; `None` — unanswered.
    fn answer(&mut self, prompt: String) -> Option<String> {
        let class = classify(&prompt);
        let password = class == Prompt::Password;
        if password {
            if self.offered_saved && self.typed.is_none() {
                self.saved_refused = true;
            }
            if let Some(saved) = self.saved.take() {
                self.offered_saved = true;
                return Some(saved);
            }
        }
        let sheet = self.sheet.as_mut()?;
        let question = Question {
            prompt,
            class,
            again: password && self.typed.is_some(),
            stale: password && self.saved_refused && self.typed.is_none(),
        };
        let typed = sheet(&question)?;
        let text = typed.text.clone();
        if password {
            self.typed = Some(typed);
        }
        Some(text)
    }
}

/// The askpass socket's and its error file's prefix: `q-` + 16 random hex
/// digits, next to the master sockets in the private directory.
const ASKPASS_PREFIX: &str = "q-";

/// 64 random bits as hex, from the system's generator.
fn random_hex() -> io::Result<String> {
    let mut bytes = [0u8; 8];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    Ok(bytes.iter().fold(String::new(), |mut hex, byte| {
        let _ = write!(hex, "{byte:02x}");
        hex
    }))
}

/// Opens our master at `socket`: a fresh askpass socket for this attempt only
/// (`0700` directory, random name, removed when the attempt ends), the master
/// with the askpass variables in **its** environment, and every prompt handed
/// to `responder` until ssh's foreground half exits (`-f`).
///
/// An unanswered question **stops ssh first**, then the helper is told: ssh
/// turns a failed askpass into an empty password and would try it on the
/// server (`readpass.c`). ssh's standard error goes to a file, not a pipe — the
/// backgrounded master inherits the descriptor and a pipe's end might never come.
fn open_master(
    target: &RemoteTarget,
    socket: &Path,
    askpass: &Path,
    responder: &mut Responder,
    asker: Asker,
) -> Result<Opened, Denied> {
    let failed = |error: io::Error| {
        Denied::Failed(format!(
            "ssh could not be started for {}: {error}",
            target.host
        ))
    };
    let dir = socket
        .parent()
        .ok_or_else(|| failed(io::Error::from(io::ErrorKind::NotFound)))?;
    let stem = format!("{ASKPASS_PREFIX}{}", random_hex().map_err(failed)?);
    let ask_socket = dir.join(&stem);
    if !fits(&ask_socket) {
        return Err(failed(io::Error::from(io::ErrorKind::InvalidInput)));
    }
    let listener = UnixListener::bind(&ask_socket).map_err(failed)?;
    let err_path = dir.join(format!("{stem}.err"));
    let outcome = std::fs::File::create(&err_path)
        .map_err(failed)
        .and_then(|err_file| {
            let argv = master_argv(target, socket, asker);
            run_master(
                target,
                &argv,
                askpass,
                &ask_socket,
                &listener,
                err_file,
                responder,
            )
        });
    drop(listener);
    let stderr = std::fs::read_to_string(&err_path).unwrap_or_default();
    let _ = std::fs::remove_file(&ask_socket);
    let _ = std::fs::remove_file(&err_path);
    Ok(match outcome? {
        Run::Up => Opened::Up,
        Run::Cancelled => Opened::Cancelled,
        Run::Refused => Opened::Refused(stderr),
    })
}

/// How an attempt ended; a refusal carries ssh's standard error.
enum Opened {
    Up,
    Cancelled,
    Refused(String),
}

/// [`run_master`]'s answer (the error file is read after it).
enum Run {
    Up,
    Cancelled,
    Refused,
}

/// [`open_master`]'s process half: spawn, serve the prompts, wait.
fn run_master(
    target: &RemoteTarget,
    argv: &[String],
    askpass: &Path,
    ask_socket: &Path,
    listener: &UnixListener,
    err_file: std::fs::File,
    responder: &mut Responder,
) -> Result<Run, Denied> {
    let (program, args) = argv
        .split_first()
        .ok_or_else(|| Denied::Failed(format!("No ssh command for {}", target.host)))?;
    let mut child = Command::new(program)
        .args(args)
        .envs(askpass_env(askpass, ask_socket))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(err_file)
        .spawn()
        .map_err(|error| {
            Denied::Failed(format!(
                "ssh could not be started for {}: {error}",
                target.host
            ))
        })?;
    let pid = child.id();
    let done = Arc::new(AtomicBool::new(false));
    // The waiter wakes the blocking `accept` once ssh's foreground half is gone.
    let waiter = {
        let done = Arc::clone(&done);
        let ask_socket = ask_socket.to_owned();
        thread::Builder::new()
            .name("ssh master".into())
            .spawn(move || {
                let status = child.wait();
                done.store(true, Ordering::SeqCst);
                drop(UnixStream::connect(&ask_socket));
                status
            })
    };
    let waiter = match waiter {
        Ok(waiter) => waiter,
        Err(error) => {
            // SAFETY: `pid` is our unreaped child (nobody waits on it yet).
            unsafe { libc::kill(pid as libc::pid_t, libc::SIGTERM) };
            return Err(Denied::Failed(format!(
                "ssh could not be started for {}: {error}",
                target.host
            )));
        }
    };
    let stop = || {
        if !done.load(Ordering::SeqCst) {
            // SAFETY: `pid` is our child; until the waiter has reaped it (and set
            // `done`) the pid cannot belong to another process.
            unsafe { libc::kill(pid as libc::pid_t, libc::SIGTERM) };
        }
    };
    let mut cancelled = false;
    loop {
        let Ok((stream, _)) = listener.accept() else {
            // No more prompts can be served: ssh would wait for an answer forever.
            stop();
            break;
        };
        if done.load(Ordering::SeqCst) {
            break;
        }
        let Ok(mut reader) = stream.try_clone().map(BufReader::new) else {
            continue;
        };
        let Ok(prompt) = read_request(&mut reader) else {
            continue;
        };
        let answer = if cancelled {
            None
        } else {
            responder.answer(prompt)
        };
        if answer.is_none() && !cancelled {
            cancelled = true;
            stop();
        }
        let mut writer = stream;
        let _ = write_answer(&mut writer, answer.as_deref());
    }
    let status = waiter.join().ok().and_then(Result::ok);
    Ok(if cancelled {
        Run::Cancelled
    } else if status.is_some_and(|status| status.success()) {
        Run::Up
    } else {
        Run::Refused
    })
}

/// Whether a socket directory entry has one of our names: a master's
/// ([`KEY_DIGITS`] hex digits) or an attempt's askpass socket (`q-` + hex).
fn our_socket_name(name: &str) -> bool {
    hex_name(
        name.strip_prefix(ASKPASS_PREFIX).unwrap_or(name),
        KEY_DIGITS,
    )
}

/// Whether `name` is an attempt's error file (`q-<hex>.err`).
fn attempt_error_file(name: &str) -> bool {
    name.strip_suffix(".err")
        .is_some_and(|stem| stem.starts_with(ASKPASS_PREFIX) && our_socket_name(stem))
}

/// Whether `dir` is a real directory of this user, closed to everyone else.
pub(crate) fn private_dir(dir: &Path) -> bool {
    // SAFETY: `getuid` has no preconditions and cannot fail.
    let uid = unsafe { libc::getuid() };
    std::fs::symlink_metadata(dir).is_ok_and(|meta| {
        meta.file_type().is_dir() && meta.uid() == uid && meta.mode() & 0o077 == 0
    })
}

// ─── this instance's directory ─────────────────────────────────────────

/// An instance directory's owner file: the owning bateri's pid, in decimal.
const OWNER_FILE: &str = "pid";

/// An instance directory's name length in hex digits: 32 bits — two live
/// bateri instances of one user colliding is not a practical event, and the
/// name counts against the socket limit.
pub const INSTANCE_DIGITS: usize = 8;

/// A fresh instance directory name ([`INSTANCE_DIGITS`] hex digits).
pub fn new_instance() -> String {
    random_hex().map_or_else(
        // No system generator: the pid still separates the live instances.
        |_| format!("{:08x}", std::process::id()),
        |hex| hex[..INSTANCE_DIGITS].to_owned(),
    )
}

fn hex_name(name: &str, digits: usize) -> bool {
    name.len() == digits && name.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Whether `name` is an instance directory's — or one being born
/// ([`prepare_instance`]'s hidden `.<instance>-<hex>`).
fn instance_entry(name: &str) -> bool {
    hex_name(name, INSTANCE_DIGITS)
        || name.strip_prefix('.').is_some_and(|rest| {
            rest.split_once('-').is_some_and(|(instance, tail)| {
                hex_name(instance, INSTANCE_DIGITS) && hex_name(tail, KEY_DIGITS)
            })
        })
}

/// The pid that owns the instance directory `dir`: a private directory of
/// this user with an owner file. `None` otherwise — such a directory is never
/// swept (the wrong way round would remove a live instance's sockets).
fn owner(dir: &Path) -> Option<u32> {
    if !private_dir(dir) {
        return None;
    }
    std::fs::read_to_string(dir.join(OWNER_FILE))
        .ok()?
        .trim()
        .parse()
        .ok()
}

/// Whether process `pid` exists and is this user's (`kill(pid, 0)`). A pid
/// reused by another user's process counts as dead: the directory is ours.
fn alive(pid: u32) -> bool {
    let Ok(pid) = libc::pid_t::try_from(pid) else {
        return false;
    };
    // SAFETY: signal 0 only checks; it sends nothing.
    pid > 0 && unsafe { libc::kill(pid, 0) } == 0
}

/// This instance's directory under `root`, created if it is not there: the
/// root private ([`prepare_dir`]), the directory born **whole** — made under
/// a hidden name, the owner file written, then renamed into place — so another
/// instance's sweep never sees it without an owner. An existing one must be
/// this process's.
pub fn prepare_instance(root: &Path, instance: &str) -> io::Result<PathBuf> {
    prepare_dir(root)?;
    let dir = root.join(instance);
    let pid = std::process::id();
    if owner(&dir) == Some(pid) {
        return Ok(dir);
    }
    let temp = root.join(format!(".{instance}-{}", random_hex()?));
    std::fs::DirBuilder::new().mode(0o700).create(&temp)?;
    let born = std::fs::write(temp.join(OWNER_FILE), pid.to_string())
        .and_then(|()| std::fs::rename(&temp, &dir));
    match born {
        Ok(()) => Ok(dir),
        Err(error) => {
            let _ = std::fs::remove_dir_all(&temp);
            // Another job of this process won the rename.
            if owner(&dir) == Some(pid) {
                Ok(dir)
            } else {
                Err(error)
            }
        }
    }
}

/// Hands an existing instance directory to `to`: the update's holder takes the old bateri's directories
/// (`from` = its spawner), the new bateri takes them from the holder (`from`
/// = the holder's pid, from the connection's credentials) — **before** its
/// sweep, or the sweep of a dead owner's directory would end the live ssh's
/// `u-<key>` socket and the attempts' files. Admitted only for a private
/// directory of this user with an owner file whose owner is `to` already,
/// **dead**, or exactly `from`: a living bateri's directory is never taken
/// by anyone it did not hand it to. The owner file is written whole (a
/// hidden name, then renamed), so no reader sees it half written.
/// [`prepare_instance`]'s contract is unchanged: it takes the directory this
/// returns as its own.
pub fn adopt_instance(dir: &Path, from: Option<u32>, to: u32) -> io::Result<()> {
    let refused = |why: &str| io::Error::new(io::ErrorKind::PermissionDenied, why.to_owned());
    let Some(current) = owner(dir) else {
        return Err(refused("not an instance directory of this user"));
    };
    if current == to {
        return Ok(());
    }
    if alive(current) && from != Some(current) {
        return Err(refused("the instance's owner is alive"));
    }
    let temp = dir.join(format!(".{OWNER_FILE}-{}", random_hex()?));
    let written = std::fs::write(&temp, to.to_string())
        .and_then(|()| std::fs::rename(&temp, dir.join(OWNER_FILE)));
    if written.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    written
}

/// `ssh -O exit` on `socket` without a destination of the user's: `-F
/// /dev/null` (the user's configuration is not read — a control command
/// rides the socket), `BatchMode=yes`, a placeholder host.
fn exit_argv(program: &str, socket: &Path) -> Vec<String> {
    vec![
        program.to_owned(),
        "-F".to_owned(),
        "/dev/null".to_owned(),
        "-o".to_owned(),
        "BatchMode=yes".to_owned(),
        "-o".to_owned(),
        format!("ControlPath={}", socket.display()),
        "-O".to_owned(),
        "exit".to_owned(),
        "bateri".to_owned(),
    ]
}

/// The master sockets in `dir` ([`KEY_DIGITS`] hex digits, a socket).
fn our_sockets(dir: &Path) -> Vec<PathBuf> {
    use std::os::unix::fs::FileTypeExt;
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .filter(|entry| {
                    entry.file_type().is_ok_and(|kind| kind.is_socket())
                        && entry
                            .file_name()
                            .to_str()
                            .is_some_and(|name| hex_name(name, KEY_DIGITS))
                })
                .map(|entry| entry.path())
                .collect()
        })
        .unwrap_or_default()
}

/// The master sockets in `dir` nobody listens on any more (`ECONNREFUSED`):
/// removed. A live one stays — it is a master this instance owns.
fn remove_dead_sockets(dir: &Path) {
    for socket in our_sockets(dir) {
        if UnixStream::connect(&socket)
            .is_err_and(|error| error.kind() == io::ErrorKind::ConnectionRefused)
        {
            let _ = std::fs::remove_file(&socket);
        }
    }
}

/// Removes an instance directory whose masters are gone: our names only
/// (sockets, an attempt's files, the focus listener, the owner file), then the directory — which
/// stays if anything else is in it. A user's terminal session socket
/// ([`SESSION_PREFIX`]) nobody listens on goes too; a live one keeps the
/// directory **and its owner file** — it ends by itself ([`SESSION_PERSIST`])
/// and the next start's sweep removes what is left.
fn remove_instance(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut live = false;
    for entry in entries.flatten() {
        let name = entry.file_name();
        if name
            .to_str()
            .and_then(|name| name.strip_prefix(SESSION_PREFIX))
            .is_some_and(|key| hex_name(key, KEY_DIGITS))
        {
            if UnixStream::connect(entry.path()).is_ok() {
                live = true;
            } else {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
    if live {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        // The focus listener by name only: on ⌘Q it is this process's
        // and still listening, and it goes with the directory.
        // A holder's socket likewise (either kind): a holder that ended by
        // its limit leaves the directory to the next start's sweep.
        let ours = name == OWNER_FILE
            || name == FOCUS_SOCKET
            || is_holder_socket_name(name)
            || our_socket_name(name)
            || attempt_error_file(name);
        if ours {
            let _ = std::fs::remove_file(entry.path());
        }
    }
    let _ = std::fs::remove_dir(dir);
}

/// What a dead bateri left behind: under every root, an instance
/// directory (or one half born) whose owner is **dead** has its live masters
/// ended (`-O exit` — a connection must not outlive the bateri that opened it)
/// and is removed. A living instance's directory, this instance's (`own`),
/// one without an owner file and any name that is not ours stay. Sockets
/// directly in a root (the layout before instance directories) keep the old
/// rule: removed only when nobody listens. A root that is not a private
/// directory of this user is not looked into. Runs ssh: never on the main thread.
pub fn sweep(roots: &[PathBuf], own: &str) {
    for root in roots.iter().filter(|root| private_dir(root)) {
        sweep_flat(root);
    }
    for entry in instance_entries(roots) {
        // A live holder keeps its directory whoever the owner file
        // names: a new bateri that adopted it may have died before its ACK,
        // and the holder still waits for the next one.
        if entry.name == own || entry.owner.is_none_or(alive) || holder_listening(&entry.dir) {
            continue;
        }
        for socket in our_sockets(&entry.dir) {
            if UnixStream::connect(&socket).is_ok() {
                let _ = SystemSsh.run(&exit_argv("ssh", &socket));
            }
            let _ = std::fs::remove_file(&socket);
        }
        remove_instance(&entry.dir);
    }
}

/// One instance directory entry under a root ([`instance_entries`]).
struct InstanceEntry {
    name: String,
    dir: PathBuf,
    /// [`owner`]: `None` for one without an owner file or not private.
    owner: Option<u32>,
}

/// The **one** walk over the instance directories: under every root that is a
/// private directory of this user, each entry with an instance name — whole
/// or half born ([`instance_entry`]) — and its owner. [`sweep`] retires the
/// dead ones, [`live_instances`] gives the living ones.
fn instance_entries(roots: &[PathBuf]) -> Vec<InstanceEntry> {
    let mut found = Vec::new();
    for root in roots.iter().filter(|root| private_dir(root)) {
        let Ok(entries) = std::fs::read_dir(root) else {
            continue;
        };
        for entry in entries.flatten() {
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if !instance_entry(&name) {
                continue;
            }
            let dir = entry.path();
            let owner = owner(&dir);
            found.push(InstanceEntry { name, dir, owner });
        }
    }
    found
}

/// A living bateri instance: its pid and its directories, in the roots' order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Instance {
    pub pid: u32,
    pub dirs: Vec<PathBuf>,
}

/// The living instances under `roots` (`bateri focus`, outside any
/// instance): whole instance directories whose owner is alive, one
/// [`Instance`] per pid — the same pid's directories under two roots are one
/// instance. In the order first seen.
pub fn live_instances(roots: &[PathBuf]) -> Vec<Instance> {
    let mut instances: Vec<Instance> = Vec::new();
    for entry in instance_entries(roots) {
        let Some(pid) = entry.owner.filter(|&pid| alive(pid)) else {
            continue;
        };
        if !hex_name(&entry.name, INSTANCE_DIGITS) {
            continue;
        }
        match instances.iter_mut().find(|instance| instance.pid == pid) {
            Some(instance) => instance.dirs.push(entry.dir),
            None => instances.push(Instance {
                pid,
                dirs: vec![entry.dir],
            }),
        }
    }
    instances
}

/// The flat half of [`sweep`]: a socket of ours directly in `base` **nobody
/// listens on** (`ECONNREFUSED`) and an attempt's error file whose socket is
/// gone. A live one stays (an older bateri's, until its `ControlPersist`).
fn sweep_flat(base: &Path) {
    use std::os::unix::fs::FileTypeExt;
    let Ok(entries) = std::fs::read_dir(base) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let path = entry.path();
        if let Some(stem) = name.strip_suffix(".err") {
            if attempt_error_file(name) && !base.join(stem).exists() {
                let _ = std::fs::remove_file(&path);
            }
            continue;
        }
        let socket = entry.file_type().is_ok_and(|kind| kind.is_socket());
        if socket
            && our_socket_name(name)
            && UnixStream::connect(&path)
                .is_err_and(|error| error.kind() == io::ErrorKind::ConnectionRefused)
        {
            let _ = std::fs::remove_file(&path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handover::HANDOVER_SOCKET;
    use std::cell::RefCell;
    use std::thread;

    use bt_core::RemoteKind;

    use crate::upload::{ssh_argv, ssh_argv_for};

    fn target(argv: &[&str]) -> RemoteTarget {
        RemoteTarget {
            host: "prod".to_owned(),
            kind: RemoteKind::Ssh,
            argv: words(argv),
            line: String::new(),
        }
    }

    fn scratch(name: &str) -> PathBuf {
        // Short on purpose: the askpass test binds a socket here.
        let dir = PathBuf::from(format!("/tmp/bt-sr-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp directory");
        dir
    }

    #[test]
    fn the_decision_has_five_arms() {
        let socket = Path::new("/tmp/s/0123456789abcdef");
        let session = Path::new("/tmp/s/u-0123456789abcdef");
        let ours = Plan::Ready(Route::Ours(socket.to_owned()));
        let riding = Plan::Ready(Route::Ours(session.to_owned()));
        // Our master alive: ours, whatever the others say.
        assert_eq!(
            decide(Check::Live, Some(session), Some(Check::Live), Some(socket)),
            ours
        );
        assert_eq!(decide(Check::Live, None, None, Some(socket)), ours);
        // The user's terminal session alive: ride it, before the
        // user's own master and before opening ours.
        assert_eq!(
            decide(
                Check::Absent,
                Some(session),
                Some(Check::Live),
                Some(socket)
            ),
            riding
        );
        assert_eq!(
            decide(Check::Stale, Some(session), None, Some(socket)),
            riding
        );
        // The user's master alive: today's argv.
        assert_eq!(
            decide(Check::Absent, None, Some(Check::Live), Some(socket)),
            Plan::Ready(Route::Direct)
        );
        // Neither (a stale socket counts as none): open ours.
        for (mine, theirs) in [
            (Check::Absent, None),
            (Check::Stale, Some(Check::Absent)),
            (Check::Absent, Some(Check::Stale)),
        ] {
            assert_eq!(
                decide(mine, None, theirs, Some(socket)),
                Plan::Open(socket.to_owned())
            );
        }
        // No socket of ours possible: today's behaviour.
        assert_eq!(
            decide(Check::Absent, None, None, None),
            Plan::Ready(Route::Direct)
        );
    }

    #[test]
    fn check_answers_are_classified() {
        assert_eq!(
            classify_check(Some(0), "Master running (pid=42)\r\n"),
            Check::Live
        );
        assert_eq!(
            classify_check(
                Some(255),
                "Control socket connect(/tmp/s/x): Connection refused\r\n"
            ),
            Check::Stale
        );
        assert_eq!(
            classify_check(
                Some(255),
                "Control socket connect(/tmp/s/x): No such file or directory\r\n"
            ),
            Check::Absent
        );
        assert_eq!(classify_check(None, ""), Check::Absent);
    }

    #[test]
    fn direct_argv_is_todays_byte_for_byte() {
        for argv in [
            &["ssh", "-p", "2222", "-l", "deploy", "-J", "jump", "prod"][..],
            &["ssh", "-tv", "-o", "RequestTTY=force", "prod", "tmux"][..],
            &["/opt/ssh", "-Ap2222", "prod", "-i", "k"][..],
        ] {
            let target = target(argv);
            assert_eq!(ssh_argv_for(&target, &Route::Direct), ssh_argv(&target));
        }
    }

    #[test]
    fn ours_puts_the_control_path_behind_the_users_options() {
        let target = target(&["ssh", "-o", "ControlPath=/mine/%C", "-p", "2222", "prod"]);
        let argv = ssh_argv_for(&target, &Route::Ours(PathBuf::from("/tmp/s/k")));
        assert_eq!(
            argv,
            words(&[
                "ssh",
                "-T",
                "-o",
                "BatchMode=yes",
                "-o",
                "ControlMaster=no",
                "-o",
                "ControlPath=/mine/%C",
                "-p",
                "2222",
                "-o",
                "ControlPath=/tmp/s/k",
                "prod",
            ])
        );
        // ssh takes a key's first value: the user's typed control path still wins.
        let first = argv
            .iter()
            .position(|arg| arg.starts_with("ControlPath="))
            .unwrap();
        assert_eq!(argv[first], "ControlPath=/mine/%C");
    }

    #[test]
    fn the_master_argv_puts_ours_first() {
        let target = target(&["ssh", "-p", "2222", "-tt", "deploy@prod", "uptime"]);
        let socket = Path::new("/tmp/s/k");
        let expected = |extra: &[&str]| {
            let mut argv = words(&[
                "ssh",
                "-M",
                "-N",
                "-f",
                "-o",
                "ControlPath=/tmp/s/k",
                "-o",
                "ControlPersist=600",
                "-o",
                "ConnectTimeout=15",
                "-o",
                "BatchMode=no",
                "-o",
                "StrictHostKeyChecking=yes",
            ]);
            argv.extend(words(extra));
            argv.extend(words(&["-p", "2222", "deploy@prod"]));
            argv
        };
        assert_eq!(master_argv(&target, socket, Asker::User), expected(&[]));
        assert_eq!(
            master_argv(&target, socket, Asker::Background),
            expected(&["-o", "NumberOfPasswordPrompts=1"])
        );
    }

    #[test]
    fn askpass_env_is_a_value_for_one_command() {
        let env = askpass_env(Path::new("/A/bateri"), Path::new("/tmp/q/x"));
        assert_eq!(
            env,
            vec![
                ("SSH_ASKPASS".to_owned(), PathBuf::from("/A/bateri")),
                ("SSH_ASKPASS_REQUIRE".to_owned(), PathBuf::from("force")),
                ("BATERI_ASKPASS".to_owned(), PathBuf::from("/tmp/q/x")),
            ]
        );
    }

    #[test]
    fn ssh_g_output_is_parsed() {
        let out = "user deploy\nhostname 10.0.0.5\nport 2222\nproxyjump none\n\
                   controlpath /Users/u/.ssh/cm-deploy@10.0.0.5:2222\nbatchmode no\n";
        let config = parse_config(out).unwrap();
        assert_eq!(config.user, "deploy");
        assert_eq!(config.hostname, "10.0.0.5");
        assert_eq!(config.port, "2222");
        assert_eq!(config.proxyjump, "none");
        assert_eq!(
            config.control_path,
            Some(PathBuf::from("/Users/u/.ssh/cm-deploy@10.0.0.5:2222"))
        );
        let none = parse_config("user u\nhostname h\nport 22\ncontrolpath none\n").unwrap();
        assert_eq!(none.control_path, None);
        assert!(!config.shares_connections() || config.control_path.is_some());
        // `-G` spells the default `false`; any other value is the user's own.
        assert!(!none.shares_connections());
        for master in ["auto", "yes", "true", "ask", "autoask"] {
            let config = parse_config(&format!("hostname h\ncontrolmaster {master}\n")).unwrap();
            assert!(config.shares_connections(), "{master}");
        }
        for master in ["false", "no"] {
            let config = parse_config(&format!("hostname h\ncontrolmaster {master}\n")).unwrap();
            assert!(!config.shares_connections(), "{master}");
        }
        assert_eq!(none.proxyjump, "none");
        assert_eq!(parse_config("garbage"), None);
    }

    #[test]
    fn the_host_key_is_stable_and_separates_the_quadruple() {
        let config = SshConfig {
            user: "deploy".into(),
            hostname: "prod".into(),
            port: "22".into(),
            proxyjump: "none".into(),
            control_path: None,
            control_master: false,
        };
        let key = host_key(&config);
        assert_eq!(key.len(), KEY_DIGITS);
        assert!(key.bytes().all(|byte| byte.is_ascii_hexdigit()));
        // Pinned: the name outlives the process (and the `ssh` wrapper computes it too).
        assert_eq!(key, "fbe5e00e347eecdb");
        let other = |edit: fn(&mut SshConfig)| {
            let mut changed = config.clone();
            edit(&mut changed);
            host_key(&changed)
        };
        assert_ne!(other(|c| c.port = "2222".into()), key);
        assert_ne!(other(|c| c.proxyjump = "jump".into()), key);
        assert_ne!(other(|c| c.user = "root".into()), key);
        // The separator keeps the parts apart: ("ab", "c") is not ("a", "bc").
        let a = SshConfig {
            user: "ab".into(),
            hostname: "c".into(),
            ..config.clone()
        };
        let b = SshConfig {
            user: "a".into(),
            hostname: "bc".into(),
            ..config.clone()
        };
        assert_ne!(host_key(&a), host_key(&b));
    }

    #[test]
    fn the_path_budget_holds_and_falls_back() {
        let key = "0123456789abcdef";
        // The fallback base is always within the limit, for the largest uid.
        let fallback = PathBuf::from(format!("/tmp/bateri-{}", u32::MAX))
            .join("0".repeat(INSTANCE_DIGITS))
            .join(key);
        assert!(fits(&fallback), "{}", fallback.display());
        // A path ssh would split or expand in `-o ControlPath=`.
        for odd in ["/Users/a b/s/k", "/Users/a%b/s/k", "/Users/a\tb/s/k"] {
            assert!(!fits(Path::new(odd)), "{odd:?}");
        }
        let instance = "0".repeat(INSTANCE_DIGITS);
        // The longest home whose cache socket still fits, then one byte more —
        // under this instance's directory.
        let tail = socket_bases(Some(Path::new("/")), 0)[0]
            .strip_prefix("/")
            .unwrap()
            .join(&instance)
            .join(key);
        let longest = SUN_PATH - SSH_TEMP_SUFFIX - 1 - tail.as_os_str().len() - 1;
        let home = |len: usize| PathBuf::from(format!("/{}", "u".repeat(len - 1)));
        assert!(fits(&home(longest).join(&tail)));
        assert!(!fits(&home(longest + 1).join(&tail)));
        // The default home on macOS (/Users/<name>) leaves room for a long name
        // (29 characters on macOS; a longer one falls back to `/tmp`).
        assert!(longest >= "/Users/".len() + 24, "{longest}");

        // With a home that does not fit, the socket lands under the fallback.
        let root = scratch("budget");
        let bases = vec![
            home(longest + 1).join(tail.parent().unwrap()),
            root.join("fallback"),
        ];
        assert_eq!(
            socket_path(&bases, key),
            Some(root.join("fallback").join(key))
        );
        // Neither fits: no socket of ours, the route is Direct.
        let deep = root.join("x".repeat(SUN_PATH));
        assert_eq!(socket_path(&[deep], key), None);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_socket_directory_open_to_others_is_refused() {
        use std::os::unix::fs::PermissionsExt;
        let root = scratch("private");
        let dir = root.join("s");
        prepare_dir(&dir).unwrap();
        assert_eq!(std::fs::metadata(&dir).unwrap().mode() & 0o777, 0o700);
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(prepare_dir(&dir).is_err());
        // A link in place of the directory is refused too.
        let link = root.join("link");
        std::os::unix::fs::symlink(&root, &link).unwrap();
        assert!(prepare_dir(&link).is_err());
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// Answers argv by its shape: `-G`, `-O check` with or without our socket.
    struct Table {
        config: &'static str,
        ours: (Option<i32>, &'static str),
        user: (Option<i32>, &'static str),
        calls: RefCell<Vec<Vec<String>>>,
    }

    impl SshRunner for Table {
        fn run(&self, argv: &[String]) -> io::Result<(Option<i32>, String, String)> {
            self.calls.borrow_mut().push(argv.to_vec());
            let reply = |(code, err): (Option<i32>, &str)| Ok((code, String::new(), err.into()));
            if argv.iter().any(|arg| arg == "-G") {
                return Ok((Some(0), self.config.to_owned(), String::new()));
            }
            if argv.iter().any(|arg| arg.starts_with("ControlPath=")) {
                reply(self.ours)
            } else {
                reply(self.user)
            }
        }
    }

    const ABSENT: (Option<i32>, &str) = (Some(255), "No such file or directory");
    const LIVE: (Option<i32>, &str) = (Some(0), "Master running");

    #[test]
    fn the_gate_runs_the_checks_and_removes_a_stale_socket() {
        let root = scratch("gate");
        let bases = vec![root.join("s")];
        let config = "user u\nhostname prod\nport 22\nproxyjump none\ncontrolpath /u/cm\n";
        let table = |ours, user| Table {
            config,
            ours,
            user,
            calls: RefCell::new(Vec::new()),
        };
        let target = target(&["ssh", "prod"]);
        let key = host_key(&parse_config(config).unwrap());
        let socket = root.join("s").join(&key);

        assert_eq!(
            plan(&table(LIVE, ABSENT), &target, &bases),
            Plan::Ready(Route::Ours(socket.clone()))
        );
        assert_eq!(
            plan(&table(ABSENT, LIVE), &target, &bases),
            Plan::Ready(Route::Direct)
        );
        let runner = table(ABSENT, ABSENT);
        assert_eq!(plan(&runner, &target, &bases), Plan::Open(socket.clone()));
        // The checks never ask: BatchMode=yes, ahead of the user's options.
        for call in runner.calls.borrow().iter().skip(1) {
            assert_eq!(call[1..3], words(&["-o", "BatchMode=yes"]));
        }

        std::fs::write(&socket, "").unwrap();
        let stale = (Some(255), "Control socket connect: Connection refused");
        assert_eq!(
            plan(&table(stale, ABSENT), &target, &bases),
            Plan::Open(socket.clone())
        );
        assert!(!socket.exists(), "the stale socket stays");

        // A control path the user typed: always theirs, no check run.
        let typed = Table {
            config,
            ours: LIVE,
            user: LIVE,
            calls: RefCell::new(Vec::new()),
        };
        assert_eq!(
            plan(&typed, &self::target(&["ssh", "-S", "/x", "prod"]), &bases),
            Plan::Ready(Route::Direct)
        );
        assert!(typed.calls.borrow().is_empty());
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// `-O check` answered by which socket it asks: the user's terminal
    /// session's (`u-…`), ours, or the user's own.
    struct SessionTable {
        ours: (Option<i32>, &'static str),
        session: (Option<i32>, &'static str),
        calls: RefCell<Vec<Vec<String>>>,
    }

    impl SshRunner for SessionTable {
        fn run(&self, argv: &[String]) -> io::Result<(Option<i32>, String, String)> {
            self.calls.borrow_mut().push(argv.to_vec());
            if argv.iter().any(|arg| arg == "-G") {
                let config = "user u\nhostname prod\nport 22\nproxyjump none\ncontrolpath /u/cm\n";
                return Ok((Some(0), config.to_owned(), String::new()));
            }
            let (code, err) = match argv.iter().find_map(|arg| arg.strip_prefix("ControlPath=")) {
                Some(path) if path.contains(SESSION_PREFIX) => self.session,
                Some(_) => self.ours,
                None => LIVE,
            };
            Ok((code, String::new(), err.to_owned()))
        }
    }

    /// The user's terminal session's master is a route of ours —
    /// after our own master, before the user's and before opening ours; its
    /// socket is checked only when it is there.
    #[test]
    fn the_gate_rides_the_users_terminal_session() {
        let root = scratch("session-gate");
        let bases = vec![root.join("s")];
        let target = target(&["ssh", "prod"]);
        let config = parse_config("user u\nhostname prod\nport 22\nproxyjump none\n").unwrap();
        let key = host_key(&config);
        let socket = root.join("s").join(&key);
        let session = root.join("s").join(format!("{SESSION_PREFIX}{key}"));
        let table = |ours, session| SessionTable {
            ours,
            session,
            calls: RefCell::new(Vec::new()),
        };
        // No session socket: today's gate (the user's own master answers).
        let runner = table(ABSENT, LIVE);
        assert_eq!(plan(&runner, &target, &bases), Plan::Ready(Route::Direct));
        assert!(
            runner
                .calls
                .borrow()
                .iter()
                .all(|call| !call.iter().any(|arg| arg.contains(SESSION_PREFIX))),
            "an absent session socket is not asked"
        );
        // The session is up: ride it, the user's own master is not asked.
        std::fs::write(&session, "").unwrap();
        let runner = table(ABSENT, LIVE);
        assert_eq!(
            plan(&runner, &target, &bases),
            Plan::Ready(Route::Ours(session.clone()))
        );
        assert!(
            runner.calls.borrow().iter().all(|call| call
                .iter()
                .any(|arg| arg.starts_with("ControlPath="))
                || call.contains(&"-G".to_owned())),
            "the user's own master is not asked"
        );
        // Ours first.
        assert_eq!(
            plan(&table(LIVE, LIVE), &target, &bases),
            Plan::Ready(Route::Ours(socket.clone()))
        );
        // A session socket nobody answers on is not ridden (the user's own
        // master answers here); the file stays for ssh's `auto` to unlink.
        assert_eq!(
            plan(&table(ABSENT, ABSENT), &target, &bases),
            Plan::Ready(Route::Direct)
        );
        let _ = socket;
        assert!(session.exists());
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// The session socket is found only in a private directory with room for
    /// the prefix, and only an instance's directory is handed to `ssh-argv`.
    #[test]
    fn the_session_socket_lives_only_in_an_instance_directory() {
        let root = scratch("session-socket");
        let roots = vec![root.join("r")];
        assert!(instance_dirs(&roots, "0a1b2c3d").is_empty(), "not made yet");
        let dir = prepare_instance(&roots[0], "0a1b2c3d").unwrap();
        assert_eq!(instance_dirs(&roots, "0a1b2c3d"), vec![dir.clone()]);
        for bad in ["../x", "0A1B2C3", "", "0a1b2c3d/.."] {
            assert!(instance_dirs(&roots, bad).is_empty(), "{bad:?}");
        }
        assert_eq!(
            session_socket(std::slice::from_ref(&dir), "0123456789abcdef"),
            Some(dir.join("u-0123456789abcdef"))
        );
        // Open to others: none.
        std::fs::set_permissions(&dir, std::os::unix::fs::PermissionsExt::from_mode(0o755))
            .unwrap();
        assert_eq!(
            session_socket(std::slice::from_ref(&dir), "0123456789abcdef"),
            None
        );
        assert!(instance_dirs(&roots, "0a1b2c3d").is_empty());
        std::fs::set_permissions(&dir, std::os::unix::fs::PermissionsExt::from_mode(0o700))
            .unwrap();
        // Past the socket limit: none.
        let long = root.join("x".repeat(SUN_PATH));
        assert_eq!(session_socket(&[long], "0123456789abcdef"), None);
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// A user's session socket is never ended by bateri: ⌘Q's `close_all`
    /// leaves a live one (and the directory with its owner file), a stale one
    /// goes with the directory.
    #[test]
    fn closing_leaves_the_users_session_alone() {
        let scratch = scratch("session-close");
        let root = scratch.join("r");
        let dir = prepare_instance(&root, "0a1b2c3d").unwrap();
        let session = dir.join("u-0123456789abcdef");
        let listener = UnixListener::bind(&session).unwrap();
        let masters = Masters::with_instance(
            PathBuf::from("/nonexistent"),
            vec![root.clone()],
            Arc::new(NoStore),
            "0a1b2c3d".to_owned(),
        );
        let _ = masters.bases();
        masters.close_all(Instant::now() + Duration::from_secs(5));
        assert!(session.exists(), "a live session socket stays");
        assert!(dir.join(OWNER_FILE).exists(), "so does its owner file");
        drop(listener);
        masters.close_all(Instant::now() + Duration::from_secs(5));
        assert!(!dir.exists(), "a stale one goes with the directory");
        std::fs::remove_dir_all(&scratch).unwrap();
    }

    #[test]
    fn prompts_are_classified() {
        for prompt in [
            "deploy@prod's password: ",
            "Password:",
            "(deploy@prod) Password: ",
            "Parola: ",
            "Şifre:",
            "deploy@10.0.0.5 için parola:",
        ] {
            assert_eq!(classify(prompt), Prompt::Password, "{prompt}");
        }
        for prompt in [
            "Enter passphrase for key '/Users/u/.ssh/id_ed25519': ",
            "Verification code: ",
            "One-time password (OATH) for `deploy': ",
            "Anahtar parolası: ",
            "Doğrulama kodu: ",
            "Are you sure you want to continue connecting (yes/no)? ",
            "",
        ] {
            assert_eq!(classify(prompt), Prompt::Other, "{prompt}");
        }
    }

    /// One request served by `answer`, the prompt the server saw back.
    fn serve(
        socket: &Path,
        answer: Option<&'static str>,
    ) -> thread::JoinHandle<io::Result<String>> {
        let listener = UnixListener::bind(socket).unwrap();
        thread::spawn(move || {
            let (stream, _) = listener.accept()?;
            let mut reader = BufReader::new(stream.try_clone()?);
            let prompt = read_request(&mut reader)?;
            let mut writer = stream;
            write_answer(&mut writer, answer)?;
            Ok(prompt)
        })
    }

    #[test]
    fn askpass_round_trips_and_fails_without_an_answer() {
        let root = scratch("askpass");
        let socket = root.join("a");

        let server = serve(&socket, Some("s3cr3t \n✓"));
        let mut out = Vec::new();
        assert_eq!(
            run_askpass(&socket, "deploy@prod's password: ", &mut out),
            0
        );
        assert_eq!(out, "s3cr3t \n✓\n".as_bytes());
        assert_eq!(server.join().unwrap().unwrap(), "deploy@prod's password: ");
        std::fs::remove_file(&socket).unwrap();

        // Cancelled: non-zero and nothing on stdout — no empty answer.
        let server = serve(&socket, None);
        let mut out = Vec::new();
        assert_eq!(run_askpass(&socket, "Password:", &mut out), 1);
        assert!(out.is_empty());
        server.join().unwrap().unwrap();
        std::fs::remove_file(&socket).unwrap();

        // The application dropped the connection (pane closed, ⌘Q).
        let listener = UnixListener::bind(&socket).unwrap();
        let dropper = thread::spawn(move || drop(listener.accept()));
        let mut out = Vec::new();
        assert_eq!(run_askpass(&socket, "Password:", &mut out), 1);
        assert!(out.is_empty());
        dropper.join().unwrap();
        std::fs::remove_file(&socket).unwrap();

        // Nobody listens at all.
        assert_eq!(run_askpass(&socket, "Password:", &mut Vec::new()), 1);
        std::fs::remove_dir_all(&root).unwrap();
    }

    // ─── opening our master, end to end ─────────────────────────────────

    /// A fake `ssh` that behaves like the real one where the gate looks: `-G`
    /// prints a configuration, `-O check` answers by the marker at the
    /// `ControlPath` (`-O stop` removes it), and `-M` asks through
    /// `$SSH_ASKPASS` (`NumberOfPasswordPrompts` times, three by default),
    /// then plays `-f` — the marker, exit 0.
    /// The host `stranger` has no known host key. POSIX `sh`: Debian's is dash.
    ///
    /// It records what the tests assert: every `-M` (`opens`), an askpass that
    /// failed and was followed by another try — ssh's empty password
    /// (`empty`), the askpass variables seen outside the master (`leak`) and
    /// every `-O stop` (`stops`).
    fn fake_ssh(root: &Path, password: &str) -> PathBuf {
        let script = format!(
            r#"#!/bin/sh
root='{root}'
cp=''; mode=''; dest=''; prev=''; op=''; prompts=''
for arg in "$@"; do
  if [ "$prev" = "-o" ]; then
    case "$arg" in
      ControlPath=*) [ -z "$cp" ] && cp="${{arg#ControlPath=}}";;
      NumberOfPasswordPrompts=*) [ -z "$prompts" ] && prompts="${{arg#NumberOfPasswordPrompts=}}";;
    esac
    prev=''; continue
  fi
  if [ "$prev" = "-O" ]; then op="$arg"; prev=''; continue; fi
  case "$arg" in
    -G) mode=G;; -M) mode=M;; -O) mode=O; prev=-O;; -o) prev=-o;; -*) ;; *) dest="$arg";;
  esac
done
if [ "$mode" != M ] && [ -n "$SSH_ASKPASS$BATERI_ASKPASS" ]; then echo "$mode" >> "$root/leak"; fi
case "$mode" in
  G) printf 'user u\nhostname %s\nport 22\nproxyjump none\n' "$dest"; exit 0;;
  O) if [ "$op" = stop ]; then echo "$cp" >> "$root/stops"; rm -f "$cp"; exit 0; fi
     if [ -e "$cp" ]; then echo 'Master running' >&2; exit 0; fi
     echo "Control socket connect($cp): No such file or directory" >&2; exit 255;;
  M) echo "$dest" >> "$root/opens"
     if [ "$dest" = stranger ]; then
       echo 'No ED25519 host key is known for stranger and you have requested strict checking.' >&2
       echo 'Host key verification failed.' >&2; exit 255
     fi
     n=0
     while [ $n -lt "${{prompts:-3}}" ]; do
       n=$((n + 1))
       if ! answer=$("$SSH_ASKPASS" "u@$dest's password: "); then echo x >> "$root/empty"; answer=''; fi
       if [ "$answer" = '{password}' ]; then
         if [ "$dest" = twofactor ] && ! "$SSH_ASKPASS" 'Verification code: ' >/dev/null; then
           echo 'Permission denied (keyboard-interactive).' >&2; exit 255
         fi
         : > "$cp"; exit 0
       fi
       echo 'Permission denied, please try again.' >&2
     done
     echo "u@$dest: Permission denied (publickey,password)." >&2; exit 255;;
esac
exit 255
"#,
            root = root.display()
        );
        executable(&root.join("ssh"), &script)
    }

    /// The askpass program: the **test binary itself** runs [`askpass_child`]
    /// — the real [`run_askpass`] on the wire — writing its answer to a file
    /// (the harness's own header owns standard output), then the wrapper
    /// prints it with the child's exit code.
    fn askpass_wrapper(root: &Path) -> PathBuf {
        let binary = std::env::current_exe().expect("the test binary");
        let script = format!(
            r#"#!/bin/sh
out='{root}/answer.'$$
BT_ASKPASS_PROMPT="$1" BT_ASKPASS_OUT="$out" '{binary}' ssh_route::tests::askpass_child --exact --ignored -q >/dev/null 2>&1
code=$?
cat "$out" 2>/dev/null; rm -f "$out"
exit $code
"#,
            root = root.display(),
            binary = binary.display()
        );
        executable(&root.join("askpass"), &script)
    }

    fn executable(path: &Path, script: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        std::fs::write(path, script).expect("script");
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).expect("mode");
        path.to_owned()
    }

    /// The other half of [`askpass_wrapper`]: does nothing unless started by it.
    #[test]
    #[ignore = "the askpass helper of the master tests; they start it themselves"]
    fn askpass_child() {
        let (Some(socket), Ok(prompt), Some(out)) = (
            std::env::var_os(ASKPASS_VAR),
            std::env::var("BT_ASKPASS_PROMPT"),
            std::env::var_os("BT_ASKPASS_OUT"),
        ) else {
            return;
        };
        let mut file = std::fs::File::create(out).expect("answer file");
        let code = run_askpass(Path::new(&socket), &prompt, &mut file);
        drop(file);
        std::process::exit(code);
    }

    /// A fake ssh, an askpass wrapper and the registry over `root/s`, with
    /// nothing saved.
    fn rig(name: &str, password: &str) -> (PathBuf, Masters) {
        let (root, masters, _) = rig_saved(name, password, None);
        (root, masters)
    }

    /// [`rig`] with a store holding `saved` for the fake's account.
    fn rig_saved(
        name: &str,
        password: &str,
        saved: Option<&str>,
    ) -> (PathBuf, Masters, Arc<MemoryStore>) {
        let root = scratch(name);
        fake_ssh(&root, password);
        let store = Arc::new(MemoryStore::default());
        if let Some(saved) = saved {
            lock(&store.saved).insert(account(), saved.to_owned());
        }
        let masters = Masters::with_instance(
            askpass_wrapper(&root),
            vec![root.join("s")],
            Arc::clone(&store) as Arc<dyn PasswordStore>,
            INSTANCE.to_owned(),
        );
        (root, masters, store)
    }

    /// The fake's account: `ssh -G` says user `u`, port 22.
    fn account() -> Account {
        Account {
            host: "prod".to_owned(),
            user: "u".to_owned(),
            port: 22,
        }
    }

    /// The Keychain's stand-in: a map, and every write in order.
    #[derive(Default)]
    struct MemoryStore {
        saved: Mutex<HashMap<Account, String>>,
        writes: Mutex<Vec<String>>,
    }

    impl PasswordStore for MemoryStore {
        fn read(&self, account: &Account) -> Option<String> {
            lock(&self.saved).get(account).cloned()
        }
        fn write(&self, account: &Account, password: &str) {
            lock(&self.writes).push(password.to_owned());
            lock(&self.saved).insert(account.clone(), password.to_owned());
        }
        fn delete(&self, account: &Account) {
            lock(&self.saved).remove(account);
        }
        fn contains(&self, account: &Account) -> bool {
            lock(&self.saved).contains_key(account)
        }
    }

    fn host_target(root: &Path, host: &str) -> RemoteTarget {
        let ssh = root.join("ssh").display().to_string();
        RemoteTarget {
            host: host.to_owned(),
            kind: RemoteKind::Ssh,
            argv: vec![ssh, host.to_owned()],
            line: String::new(),
        }
    }

    /// Answers from a list with the Remember box clear, recording the questions.
    fn scripted(answers: &[Option<&str>]) -> (Answerer, Arc<Mutex<Vec<Question>>>) {
        answering(answers, false)
    }

    /// [`scripted`] with the Remember box ticked.
    fn remembering(answers: &[Option<&str>]) -> (Answerer, Arc<Mutex<Vec<Question>>>) {
        answering(answers, true)
    }

    fn answering(
        answers: &[Option<&str>],
        remember: bool,
    ) -> (Answerer, Arc<Mutex<Vec<Question>>>) {
        let asked = Arc::new(Mutex::new(Vec::new()));
        let mut answers: Vec<Option<Typed>> = answers
            .iter()
            .map(|a| {
                a.map(|text| Typed {
                    text: text.to_owned(),
                    remember,
                })
            })
            .collect();
        answers.reverse();
        let log = Arc::clone(&asked);
        let asker: Answerer = Box::new(move |question: &Question| {
            lock(&log).push(question.clone());
            answers.pop().flatten()
        });
        (asker, asked)
    }

    fn lines(path: &Path) -> usize {
        std::fs::read_to_string(path).map_or(0, |text| text.lines().count())
    }

    /// The rigs' instance directory name.
    const INSTANCE: &str = "0000000a";

    /// The rigs' socket directory: the instance's under `root/s`.
    fn sockets(root: &Path) -> PathBuf {
        root.join("s").join(INSTANCE)
    }

    /// Only our master's marker is left in the socket directory: the attempt's
    /// askpass socket and error file went with it (the owner file aside).
    fn leftovers(root: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(sockets(root))
            .map(|entries| {
                entries
                    .flatten()
                    .map(|entry| entry.file_name().to_string_lossy().into_owned())
                    .filter(|name| name != OWNER_FILE)
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        names
    }

    #[test]
    fn a_master_opens_through_askpass_end_to_end() {
        let (root, masters) = rig("open", "s3cr3t");
        let target = host_target(&root, "prod");
        let (asker, asked) = scripted(&[Some("wrong"), Some("s3cr3t")]);
        let route = masters.ensure(&target, Ask::Sheet(asker)).expect("opened");
        let Route::Ours(socket) = &route else {
            panic!("expected our route, got {route:?}");
        };
        // The wrong password was told on the second prompt.
        let asked = lock(&asked).clone();
        assert_eq!(asked.len(), 2);
        assert_eq!(asked[0].prompt, "u@prod's password: ");
        assert_eq!(asked[0].class, Prompt::Password);
        assert!(!asked[0].again && asked[1].again);
        assert_eq!(lines(&root.join("opens")), 1);
        assert_eq!(
            leftovers(&root),
            vec![socket.file_name().unwrap().to_string_lossy().into_owned()]
        );
        // The next job rides it: no question, no second master.
        let (asker, asked) = scripted(&[]);
        assert_eq!(
            masters.ensure(&target, Ask::Sheet(asker)),
            Ok(route.clone())
        );
        assert!(lock(&asked).is_empty());
        assert_eq!(lines(&root.join("opens")), 1);
        // The stream argv names our socket; the askpass variables went to the
        // master's environment only — not to the checks, not to this process.
        let argv = dial(Some(&masters), &target, Ask::Never).unwrap();
        assert!(argv.contains(&format!("ControlPath={}", socket.display())));
        assert!(argv.contains(&"BatchMode=yes".to_owned()));
        assert!(!root.join("leak").exists(), "askpass variables leaked");
        assert!(std::env::var_os(ASKPASS_VAR).is_none());
        assert!(std::env::var_os("SSH_ASKPASS").is_none());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_cancelled_question_stops_ssh_before_an_empty_password() {
        let (root, masters) = rig("cancel", "s3cr3t");
        let target = host_target(&root, "prod");
        let (asker, asked) = scripted(&[None]);
        assert_eq!(
            masters.ensure(&target, Ask::Sheet(asker)),
            Err(Denied::Cancelled)
        );
        assert_eq!(lock(&asked).len(), 1);
        assert!(!root.join("empty").exists(), "ssh tried an empty password");
        assert!(leftovers(&root).is_empty(), "{:?}", leftovers(&root));
        assert_eq!(Denied::Cancelled.text(), CANCELLED);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn three_wrong_passwords_fail_with_sshs_reason() {
        let (root, masters) = rig("wrong", "s3cr3t");
        let target = host_target(&root, "prod");
        let (asker, asked) = scripted(&[Some("a"), Some("b"), Some("c")]);
        let Err(Denied::Failed(text)) = masters.ensure(&target, Ask::Sheet(asker)) else {
            panic!("expected a failure");
        };
        assert!(text.contains("could not log in to prod"), "{text}");
        assert!(
            text.contains("Permission denied (publickey,password)"),
            "{text}"
        );
        assert_eq!(lock(&asked).len(), 3);
        assert!(leftovers(&root).is_empty());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn an_unknown_host_key_is_refused_with_the_terminal_hint() {
        let (root, masters) = rig("hostkey", "s3cr3t");
        let target = host_target(&root, "stranger");
        let (asker, asked) = scripted(&[]);
        let Err(Denied::Failed(text)) = masters.ensure(&target, Ask::Sheet(asker)) else {
            panic!("expected a failure");
        };
        assert!(text.contains("connect once in the terminal"), "{text}");
        assert!(lock(&asked).is_empty());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_background_job_never_opens_a_master() {
        let (root, masters) = rig("never", "s3cr3t");
        let target = host_target(&root, "prod");
        assert_eq!(masters.ensure(&target, Ask::Never), Ok(Route::Direct));
        assert_eq!(lines(&root.join("opens")), 0);
        // No registry (the timed run): today's argv, no process at all.
        assert_eq!(dial(None, &target, Ask::Never).unwrap(), ssh_argv(&target));
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// Two jobs to one host: the second joins the first's opening, one master
    /// and one question for both.
    fn one_opening_for_two_jobs(name: &str) {
        let (root, masters) = rig(name, "s3cr3t");
        let masters = Arc::new(masters);
        let target = host_target(&root, "prod");
        let (asked_tx, asked_rx) = std::sync::mpsc::channel();
        let (go_tx, go_rx) = std::sync::mpsc::channel::<()>();
        let go_rx = Mutex::new(go_rx);
        let first_asker: Answerer = Box::new(move |_: &Question| {
            let _ = asked_tx.send(());
            let _ = lock(&go_rx).recv();
            Some(Typed {
                text: "s3cr3t".to_owned(),
                remember: false,
            })
        });
        let first = {
            let (masters, target) = (Arc::clone(&masters), target.clone());
            thread::spawn(move || masters.ensure(&target, Ask::Sheet(first_asker)))
        };
        asked_rx.recv().expect("the first job asks");
        let (second_asker, second_asked) = scripted(&[Some("s3cr3t")]);
        let second = {
            let (masters, target) = (Arc::clone(&masters), target.clone());
            thread::spawn(move || masters.ensure(&target, Ask::Sheet(second_asker)))
        };
        crate::child::wait_until("the second job joins the flight", || {
            masters.joined.load(Ordering::SeqCst) == 1
        });
        go_tx.send(()).unwrap();
        let first = first.join().unwrap().expect("first");
        let second = second.join().unwrap().expect("second");
        assert_eq!(first, second);
        assert!(matches!(first, Route::Ours(_)));
        assert!(lock(&second_asked).is_empty(), "the second job asked");
        assert_eq!(lines(&root.join("opens")), 1);
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// Another pane's cancelled sheet is not this job's answer: it asks at its own.
    #[test]
    fn a_joiner_asks_itself_when_the_owner_is_cancelled() {
        let (root, masters) = rig("rejoin", "s3cr3t");
        let masters = Arc::new(masters);
        let target = host_target(&root, "prod");
        let (asked_tx, asked_rx) = std::sync::mpsc::channel();
        let (go_tx, go_rx) = std::sync::mpsc::channel::<()>();
        let go_rx = Mutex::new(go_rx);
        let owner: Answerer = Box::new(move |_: &Question| {
            let _ = asked_tx.send(());
            let _ = lock(&go_rx).recv();
            None
        });
        let first = {
            let (masters, target) = (Arc::clone(&masters), target.clone());
            thread::spawn(move || masters.ensure(&target, Ask::Sheet(owner)))
        };
        asked_rx.recv().expect("the first job asks");
        let (joiner, joiner_asked) = scripted(&[Some("s3cr3t")]);
        let second = {
            let (masters, target) = (Arc::clone(&masters), target.clone());
            thread::spawn(move || masters.ensure(&target, Ask::Sheet(joiner)))
        };
        crate::child::wait_until("the second job joins the flight", || {
            masters.joined.load(Ordering::SeqCst) == 1
        });
        go_tx.send(()).unwrap();
        assert_eq!(first.join().unwrap(), Err(Denied::Cancelled));
        assert!(matches!(second.join().unwrap(), Ok(Route::Ours(_))));
        assert_eq!(lock(&joiner_asked).len(), 1);
        assert_eq!(lines(&root.join("opens")), 2);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn two_jobs_to_one_host_open_one_master() {
        one_opening_for_two_jobs("flight");
    }

    #[test]
    #[ignore = "race stress: make test-race"]
    fn race_two_jobs_to_one_host_open_one_master() {
        for round in 0..20 {
            one_opening_for_two_jobs(&format!("flight{round}"));
        }
    }

    /// The live master's socket in `root/s`, removed — as if `ControlPersist`
    /// had ended it.
    fn master_gone(root: &Path) {
        for name in leftovers(root) {
            let _ = std::fs::remove_file(sockets(root).join(name));
        }
    }

    #[test]
    fn a_saved_password_answers_without_a_sheet() {
        let (root, masters, store) = rig_saved("saved", "s3cr3t", Some("s3cr3t"));
        let target = host_target(&root, "prod");
        let (asker, asked) = remembering(&[]);
        assert!(matches!(
            masters.ensure(&target, Ask::Sheet(asker)),
            Ok(Route::Ours(_))
        ));
        assert!(lock(&asked).is_empty(), "the sheet opened");
        assert!(
            lock(&store.writes).is_empty(),
            "a saved password was saved again"
        );
        assert!(masters.has_saved(&target));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_refused_saved_password_asks_at_the_sheet_and_is_replaced_on_success() {
        let (root, masters, store) = rig_saved("stale", "s3cr3t", Some("old"));
        let target = host_target(&root, "prod");
        let (asker, asked) = remembering(&[Some("wrong"), Some("s3cr3t")]);
        assert!(matches!(
            masters.ensure(&target, Ask::Sheet(asker)),
            Ok(Route::Ours(_))
        ));
        let asked = lock(&asked).clone();
        assert_eq!(
            asked.len(),
            2,
            "the saved password answered the first prompt"
        );
        // The first sheet says the saved one failed, the second the typed one.
        assert!(asked[0].stale && !asked[0].again, "{:?}", asked[0]);
        assert!(!asked[1].stale && asked[1].again, "{:?}", asked[1]);
        // Only the password that opened the master was written.
        assert_eq!(*lock(&store.writes), ["s3cr3t"]);
        assert_eq!(store.read(&account()).as_deref(), Some("s3cr3t"));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn an_unticked_box_saves_nothing_and_drops_a_refused_password() {
        let (root, masters, store) = rig_saved("unticked", "s3cr3t", None);
        let target = host_target(&root, "prod");
        let (asker, _) = scripted(&[Some("s3cr3t")]);
        assert!(masters.ensure(&target, Ask::Sheet(asker)).is_ok());
        assert!(lock(&store.writes).is_empty());
        assert!(!masters.has_saved(&target));
        std::fs::remove_dir_all(&root).unwrap();
        // The saved password was refused and the new one is not to be kept:
        // the known-bad one goes rather than being replayed in the background.
        let (root, masters, store) = rig_saved("unticked2", "s3cr3t", Some("old"));
        let target = host_target(&root, "prod");
        let (asker, _) = scripted(&[Some("s3cr3t")]);
        assert!(masters.ensure(&target, Ask::Sheet(asker)).is_ok());
        assert!(lock(&store.writes).is_empty());
        assert_eq!(store.read(&account()), None);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_wrong_password_is_never_saved() {
        let (root, masters, store) = rig_saved("nosave", "s3cr3t", None);
        let target = host_target(&root, "prod");
        let (asker, _) = remembering(&[Some("a"), None]);
        assert_eq!(
            masters.ensure(&target, Ask::Sheet(asker)),
            Err(Denied::Cancelled)
        );
        let (asker, _) = remembering(&[Some("a"), Some("b"), Some("c")]);
        assert!(matches!(
            masters.ensure(&target, Ask::Sheet(asker)),
            Err(Denied::Failed(_))
        ));
        assert!(lock(&store.writes).is_empty());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_background_job_opens_once_with_the_saved_password() {
        let (root, masters, _) = rig_saved("bgsaved", "s3cr3t", Some("s3cr3t"));
        let target = host_target(&root, "prod");
        assert!(matches!(
            masters.ensure(&target, Ask::Never),
            Ok(Route::Ours(_))
        ));
        assert_eq!(lines(&root.join("opens")), 1);
        assert!(!root.join("empty").exists());
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// A background job tries a saved password once; refused, the
    /// account is marked and no later background job — whatever the
    /// generation, however much later — connects again. Only the user's
    /// successful sign-in clears it.
    #[test]
    fn a_refused_saved_password_is_not_retried_in_the_background() {
        let (root, masters, store) = rig_saved("bgstale", "s3cr3t", Some("old"));
        let target = host_target(&root, "prod");
        assert_eq!(masters.ensure(&target, Ask::Never), Err(Denied::SignIn));
        assert_eq!(lines(&root.join("opens")), 1);
        // One prompt only (`NumberOfPasswordPrompts=1`): no second guess, no
        // empty password.
        assert!(!root.join("empty").exists());
        for _ in 0..3 {
            assert_eq!(masters.ensure(&target, Ask::Never), Err(Denied::SignIn));
        }
        assert_eq!(
            lines(&root.join("opens")),
            1,
            "a refused password was retried"
        );
        // The user signs in: the known-bad password is not sent again — the
        // sheet says it failed at once — the typed one replaces it and the
        // mark goes.
        let (asker, asked) = remembering(&[Some("s3cr3t")]);
        assert!(masters.ensure(&target, Ask::Sheet(asker)).is_ok());
        assert!(lock(&asked)[0].stale);
        assert_eq!(lock(&asked).len(), 1, "the refused password was sent again");
        assert_eq!(store.read(&account()).as_deref(), Some("s3cr3t"));
        master_gone(&root);
        assert!(matches!(
            masters.ensure(&target, Ask::Never),
            Ok(Route::Ours(_))
        ));
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// A background attempt that the saved password cannot finish (a code
    /// follows it) is not repeated either: one half-login, then the mark.
    #[test]
    fn a_background_job_stops_at_a_second_factor_for_good() {
        let (root, masters, store) = rig_saved("bg2fa", "s3cr3t", None);
        let account = Account {
            host: "twofactor".to_owned(),
            ..account()
        };
        lock(&store.saved).insert(account, "s3cr3t".to_owned());
        let target = host_target(&root, "twofactor");
        assert_eq!(masters.ensure(&target, Ask::Never), Err(Denied::SignIn));
        assert_eq!(masters.ensure(&target, Ask::Never), Err(Denied::SignIn));
        assert_eq!(lines(&root.join("opens")), 1);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_user_job_asks_itself_after_a_refused_background_attempt() {
        let (root, masters, _) = rig_saved("bguser", "s3cr3t", Some("old"));
        let target = host_target(&root, "prod");
        assert_eq!(masters.ensure(&target, Ask::Never), Err(Denied::SignIn));
        // A marked account does not stop the user's own job.
        let (asker, asked) = scripted(&[Some("s3cr3t")]);
        assert!(masters.ensure(&target, Ask::Sheet(asker)).is_ok());
        assert_eq!(lock(&asked).len(), 1);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn forget_removes_the_password_and_stops_our_master() {
        let (root, masters, store) = rig_saved("forget", "s3cr3t", Some("s3cr3t"));
        let target = host_target(&root, "prod");
        let Ok(Route::Ours(socket)) = masters.ensure(&target, Ask::Never) else {
            panic!("expected our master");
        };
        assert!(masters.has_saved(&target));
        masters.forget(&target);
        assert_eq!(store.read(&account()), None);
        assert!(!masters.has_saved(&target));
        assert_eq!(
            std::fs::read_to_string(root.join("stops")).unwrap().trim(),
            socket.display().to_string()
        );
        // The next background job has nothing to log in with: today's argv,
        // which the server refuses — the pane's Sign In….
        assert_eq!(masters.ensure(&target, Ask::Never), Ok(Route::Direct));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn the_account_and_its_label() {
        let config = SshConfig {
            user: "tdgunes".to_owned(),
            hostname: "192.168.0.218".to_owned(),
            port: "2222".to_owned(),
            ..SshConfig::default()
        };
        let account = Account::from_config(&config).expect("an account");
        assert_eq!(
            account.label(),
            "bateri \u{2014} tdgunes@192.168.0.218:2222"
        );
        let broken = SshConfig {
            port: "ssh".to_owned(),
            ..config.clone()
        };
        assert_eq!(Account::from_config(&broken), None);
        assert!(login_refused(
            "u@prod: Permission denied (publickey,password).\r\n"
        ));
        assert!(!login_refused(
            "ssh: connect to host prod port 22: Connection refused\r\n"
        ));
        assert!(password_refused(
            "u@prod: Permission denied (publickey,password).\r\n"
        ));
        assert!(password_refused(
            "Permission denied (keyboard-interactive).\r\n"
        ));
        assert!(!password_refused(
            "u@prod: Permission denied (publickey).\r\n"
        ));
    }

    #[test]
    fn the_sweep_removes_only_our_dead_sockets() {
        let root = scratch("sweep");
        let base = root.join("s");
        prepare_dir(&base).unwrap();
        let dead = base.join("0123456789abcdef");
        drop(UnixListener::bind(&dead).unwrap());
        let dead_ask = base.join("q-0123456789abcdef");
        drop(UnixListener::bind(&dead_ask).unwrap());
        let orphan_err = base.join("q-fedcba9876543210.err");
        std::fs::write(&orphan_err, "").unwrap();
        let live = base.join("aaaaaaaaaaaaaaaa");
        let listener = UnixListener::bind(&live).unwrap();
        let foreign = base.join("not-ours");
        drop(UnixListener::bind(&foreign).unwrap());
        let plain = base.join("bbbbbbbbbbbbbbbb");
        std::fs::write(&plain, "").unwrap();
        sweep(std::slice::from_ref(&base), "ffffffff");
        assert!(!dead.exists() && !dead_ask.exists() && !orphan_err.exists());
        assert!(live.exists(), "a live master was removed");
        assert!(
            foreign.exists() && plain.exists(),
            "a name not ours was removed"
        );
        drop(listener);
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// A pid nobody has: a child that has been reaped.
    fn dead_pid() -> u32 {
        let mut child = Command::new("/usr/bin/true").spawn().expect("true");
        let pid = child.id();
        child.wait().expect("reaped");
        pid
    }

    /// An instance directory is born whole with its owner, and the
    /// sweep retires only a dead owner's — never a living instance's, this
    /// one's or one without an owner.
    #[test]
    fn the_sweep_spares_living_instances() {
        let root = scratch("instances");
        let base = root.join("s");
        let mine = prepare_instance(&base, "11111111").unwrap();
        assert_eq!(mine, base.join("11111111"));
        assert_eq!(owner(&mine), Some(std::process::id()));
        assert_eq!(prepare_instance(&base, "11111111").unwrap(), mine, "reused");
        let names = |dir: &Path| {
            let mut names: Vec<String> = std::fs::read_dir(dir)
                .unwrap()
                .flatten()
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect();
            names.sort();
            names
        };
        assert_eq!(names(&base), ["11111111"], "no half-born directory left");
        // Another living instance: this process stands in for it.
        let living = prepare_instance(&base, "22222222").unwrap();
        // A dead one, with a dead master, an attempt's files and a stranger's file.
        let dead = prepare_instance(&base, "33333333").unwrap();
        std::fs::write(dead.join(OWNER_FILE), dead_pid().to_string()).unwrap();
        for name in ["0123456789abcdef", "q-0123456789abcdef", FOCUS_SOCKET] {
            drop(UnixListener::bind(dead.join(name)).unwrap());
        }
        std::fs::write(dead.join("q-0123456789abcdef.err"), "").unwrap();
        // A dead one that holds a stranger's file: emptied of ours, kept.
        let crowded = prepare_instance(&base, "44444444").unwrap();
        std::fs::write(crowded.join(OWNER_FILE), dead_pid().to_string()).unwrap();
        drop(UnixListener::bind(crowded.join("fedcba9876543210")).unwrap());
        std::fs::write(crowded.join("notes"), "").unwrap();
        // Half born and dead; no owner at all (left alone — the safe way).
        let half = base.join(".55555555-0123456789abcdef");
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&half)
            .unwrap();
        std::fs::write(half.join(OWNER_FILE), dead_pid().to_string()).unwrap();
        let ownerless = base.join("66666666");
        prepare_dir(&ownerless).unwrap();
        // A sibling's socket in this instance's own directory stays.
        let own_socket = mine.join("aaaaaaaaaaaaaaaa");
        drop(UnixListener::bind(&own_socket).unwrap());

        sweep(std::slice::from_ref(&base), "11111111");
        assert!(own_socket.exists(), "this instance's directory was swept");
        assert!(
            living.join(OWNER_FILE).exists(),
            "a living instance was swept"
        );
        assert!(!dead.exists() && !half.exists(), "a dead instance stays");
        assert_eq!(names(&crowded), ["notes"]);
        assert!(ownerless.exists());
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// A directory passes to a new owner only from a dead owner or
    /// from the one handing it over — never from any other living bateri —
    /// and the sweep leaves a live holder's directory alone even when its
    /// owner file names a dead process.
    #[test]
    fn an_instance_is_adopted_only_from_the_dead_or_its_giver() {
        let root = scratch("adopt");
        let base = root.join("s");
        // A living owner that is not this process: a child standing in for
        // another bateri.
        let mut other = Command::new("/bin/sleep").arg("30").spawn().expect("sleep");
        let other_pid = other.id();
        let living = prepare_instance(&base, "11111111").unwrap();
        std::fs::write(living.join(OWNER_FILE), other_pid.to_string()).unwrap();
        let me = std::process::id();
        assert!(
            adopt_instance(&living, None, me).is_err(),
            "a living bateri's directory was taken"
        );
        assert!(
            adopt_instance(&living, Some(me), me).is_err(),
            "taken by naming oneself the giver"
        );
        assert_eq!(owner(&living), Some(other_pid));
        adopt_instance(&living, Some(other_pid), me).expect("handed over by its owner");
        assert_eq!(owner(&living), Some(me));
        adopt_instance(&living, None, me).expect("taking one's own again");

        // A dead owner's directory, with a user's live session socket in it.
        let dead = prepare_instance(&base, "22222222").unwrap();
        std::fs::write(dead.join(OWNER_FILE), dead_pid().to_string()).unwrap();
        let session = UnixListener::bind(dead.join("u-0123456789abcdef")).unwrap();
        adopt_instance(&dead, None, me).expect("a dead owner's directory");
        sweep(std::slice::from_ref(&base), "ffffffff");
        assert!(
            dead.join("u-0123456789abcdef").exists(),
            "the adopted directory was swept"
        );
        let left: Vec<String> = std::fs::read_dir(&dead)
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.starts_with('.'))
            .collect();
        assert!(left.is_empty(), "a temporary owner file stayed: {left:?}");
        drop(session);

        // A dead owner but a live holder: kept; once the holder is gone, swept.
        let held = prepare_instance(&base, "33333333").unwrap();
        std::fs::write(held.join(OWNER_FILE), dead_pid().to_string()).unwrap();
        let holder = UnixListener::bind(held.join(HANDOVER_SOCKET)).unwrap();
        sweep(std::slice::from_ref(&base), "ffffffff");
        assert!(
            held.join(HANDOVER_SOCKET).exists(),
            "a live holder's directory was swept"
        );
        drop(holder);
        sweep(std::slice::from_ref(&base), "ffffffff");
        assert!(!held.exists(), "a dead holder's directory stays");

        // A bound holder's socket (`handover-<pid>`) keeps the directory the
        // same way, beside a dead one of the other kind, and goes with it.
        let bound = prepare_instance(&base, "44444444").unwrap();
        std::fs::write(bound.join(OWNER_FILE), dead_pid().to_string()).unwrap();
        drop(UnixListener::bind(bound.join(HANDOVER_SOCKET)).unwrap());
        let holder = UnixListener::bind(bound.join("handover-4242")).unwrap();
        sweep(std::slice::from_ref(&base), "ffffffff");
        assert!(
            bound.join("handover-4242").exists(),
            "a live bound holder's directory was swept"
        );
        drop(holder);
        sweep(std::slice::from_ref(&base), "ffffffff");
        assert!(!bound.exists(), "a dead bound holder's directory stays");

        // Not an instance directory at all.
        let bare = root.join("bare");
        prepare_dir(&bare).unwrap();
        assert!(adopt_instance(&bare, None, me).is_err());

        let _ = other.kill();
        let _ = other.wait();
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// The living instances are the whole directories of a live owner,
    /// one per pid across roots; the dead, the half born and the ownerless
    /// are not among them.
    #[test]
    fn live_instances_are_one_per_living_pid() {
        let root = scratch("live-instances");
        let (first, second) = (root.join("a"), root.join("b"));
        let mine_a = prepare_instance(&first, "11111111").unwrap();
        let mine_b = prepare_instance(&second, "11111111").unwrap();
        let parent = prepare_instance(&first, "22222222").unwrap();
        let parent_pid = std::os::unix::process::parent_id();
        std::fs::write(parent.join(OWNER_FILE), parent_pid.to_string()).unwrap();
        let dead = prepare_instance(&first, "33333333").unwrap();
        std::fs::write(dead.join(OWNER_FILE), dead_pid().to_string()).unwrap();
        let half = first.join(".44444444-0123456789abcdef");
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&half)
            .unwrap();
        std::fs::write(half.join(OWNER_FILE), std::process::id().to_string()).unwrap();
        prepare_dir(&first.join("55555555")).unwrap();
        let mut found = live_instances(&[first, second]);
        found.sort_by_key(|instance| instance.pid != std::process::id());
        assert_eq!(
            found,
            [
                Instance {
                    pid: std::process::id(),
                    dirs: vec![mine_a, mine_b],
                },
                Instance {
                    pid: parent_pid,
                    dirs: vec![parent],
                },
            ]
        );
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// ⌘Q removes the instance directory with its live focus listener.
    #[test]
    fn closing_removes_the_directory_with_its_focus_socket() {
        let root = scratch("close-focus");
        let base = root.join("s");
        let masters = Masters::with_instance(
            PathBuf::from("/usr/bin/false"),
            vec![base.clone()],
            Arc::new(NoStore),
            "77777777".to_owned(),
        );
        masters.sweep();
        let dir = base.join("77777777");
        let _listener = UnixListener::bind(dir.join(FOCUS_SOCKET)).unwrap();
        masters.close_all(Instant::now() + Duration::from_secs(2));
        assert!(!dir.exists(), "the instance directory stayed");
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// An instance carried over an update keeps its live masters
    /// and loses its dead ones at the sweep — the sweep of other instances
    /// never looks into one's own directory.
    #[test]
    fn a_carried_instance_keeps_its_live_masters_and_loses_the_dead() {
        let root = scratch("carried");
        let base = root.join("s");
        let dir = prepare_instance(&base, "66666666").unwrap();
        let live = dir.join("0123456789abcdef");
        let dead = dir.join("fedcba9876543210");
        let _listener = UnixListener::bind(&live).unwrap();
        drop(UnixListener::bind(&dead).unwrap());
        let masters = Masters::with_instance(
            PathBuf::from("/usr/bin/false"),
            vec![base],
            Arc::new(NoStore),
            "66666666".to_owned(),
        );
        masters.sweep();
        assert!(live.exists(), "a live master was swept");
        assert!(!dead.exists(), "a dead master stayed");
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// A pane's session end ends the master only when no other pane
    /// of this instance is in a session to it — through an alias, the same
    /// argv, or one not resolved yet (kept: it may be the same host).
    #[test]
    fn a_master_shared_by_another_pane_is_not_released() {
        let masters = Arc::new(Masters::with_instance(
            PathBuf::from("/usr/bin/false"),
            Vec::new(),
            Arc::new(NoStore),
            INSTANCE.to_owned(),
        ));
        let prod = target(&["ssh", "prod"]);
        let alias = target(&["ssh", "p"]);
        let other = target(&["ssh", "other"]);
        let socket = PathBuf::from("/s/0123456789abcdef");
        lock(&masters.sockets).insert(prod.argv.clone(), socket.clone());
        lock(&masters.sockets).insert(alias.argv.clone(), socket.clone());
        lock(&masters.sockets).insert(other.argv.clone(), PathBuf::from("/s/other"));
        let released = |pane| masters.release(pane).map(|(_, socket)| socket);

        // Two panes on one host, one through an alias: the first to leave keeps it.
        masters.session_started(1, &prod);
        masters.session_started(2, &alias);
        masters.session_started(3, &other);
        assert_eq!(released(1), None, "pane 2 still rides it");
        assert_eq!(
            released(2),
            Some(socket.clone()),
            "another host does not hold it"
        );
        // The same argv holds it.
        masters.session_started(4, &prod);
        masters.session_started(6, &prod);
        assert_eq!(released(4), None, "pane 6 is the same argv");
        // A session whose socket is not known yet holds it (no bases: `ssh -G`
        // learns nothing here).
        let unresolved = target(&["ssh", "never"]);
        masters.session_started(5, &unresolved);
        assert_eq!(released(6), None, "pane 5 may be the same host");
        assert_eq!(released(5), None, "no socket of ours for it");
        masters.session_started(6, &prod);
        assert_eq!(released(6), Some(socket.clone()));
        assert_eq!(released(6), None, "released once");
        assert_eq!(released(3), Some(PathBuf::from("/s/other")));
        // Quitting: the bookkeeping goes on, the exit is close_all's.
        masters.session_started(7, &prod);
        masters.begin_quit();
        masters.session_ended(7);
        assert!(lock(&masters.sessions).is_empty());
    }

    /// The real master's life cycle against a local, user-privileged `sshd`
    /// on a high port with key login (no password: a real password prompt
    /// needs a real account — that path is checked by eye at the set's end).
    /// Everything lives in a temporary directory: its own host key, client
    /// key, `known_hosts` and `-F /dev/null` — `~/.ssh` is never read.
    #[test]
    #[ignore = "starts a local sshd: cargo test -p bt-shell-common sshd -- --ignored"]
    fn a_real_master_opens_carries_a_stream_and_is_reused() {
        let root = scratch("sshd");
        let run = |program: &str, args: &[&str]| {
            let status = Command::new(program)
                .args(args)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .expect(program);
            assert!(status.success(), "{program} {args:?}");
        };
        let path = |name: &str| root.join(name).display().to_string();
        run(
            "ssh-keygen",
            &["-q", "-t", "ed25519", "-N", "", "-f", &path("host")],
        );
        run(
            "ssh-keygen",
            &["-q", "-t", "ed25519", "-N", "", "-f", &path("client")],
        );
        std::fs::copy(root.join("client.pub"), root.join("authorized_keys")).unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let config = format!(
            "Port {port}\nListenAddress 127.0.0.1\nHostKey {host}\nPidFile {pid}\n\
             AuthorizedKeysFile {keys}\nPasswordAuthentication no\n\
             KbdInteractiveAuthentication no\nUsePAM no\nStrictModes no\n",
            host = path("host"),
            pid = path("sshd.pid"),
            keys = path("authorized_keys"),
        );
        std::fs::write(root.join("sshd_config"), config).unwrap();
        let mut sshd = Command::new("/usr/sbin/sshd")
            .args(["-D", "-e", "-f", &path("sshd_config")])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(std::fs::File::create(root.join("sshd.log")).unwrap())
            .spawn()
            .expect("sshd");
        crate::child::wait_until("sshd listens", || {
            std::net::TcpStream::connect(("127.0.0.1", port)).is_ok()
        });
        let host_pub = std::fs::read_to_string(root.join("host.pub")).unwrap();
        std::fs::write(
            root.join("known_hosts"),
            format!("[127.0.0.1]:{port} {host_pub}"),
        )
        .unwrap();
        let user = std::env::var("USER").expect("USER");
        let target = RemoteTarget {
            host: "local".to_owned(),
            kind: RemoteKind::Ssh,
            argv: vec![
                "ssh".to_owned(),
                "-F".to_owned(),
                "/dev/null".to_owned(),
                "-i".to_owned(),
                path("client"),
                "-o".to_owned(),
                "IdentitiesOnly=yes".to_owned(),
                "-o".to_owned(),
                format!("UserKnownHostsFile={}", path("known_hosts")),
                "-p".to_owned(),
                port.to_string(),
                format!("{user}@127.0.0.1"),
            ],
            line: String::new(),
        };
        let masters = Arc::new(Masters::new(
            PathBuf::from("/usr/bin/false"),
            vec![root.join("s")],
            Arc::new(NoStore),
        ));
        let (asker, asked) = scripted(&[]);
        let route = masters.ensure(&target, Ask::Sheet(asker)).expect("master");
        let Route::Ours(socket) = route.clone() else {
            panic!("expected our route, got {route:?}");
        };
        assert!(lock(&asked).is_empty(), "a key login asked");
        // The stream rides the master, the next job reuses it.
        let argv = dial(Some(&masters), &target, Ask::Never).unwrap();
        let out = Command::new(&argv[0])
            .args(&argv[1..])
            .arg("echo riding")
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "riding");
        assert_eq!(masters.ensure(&target, Ask::Never), Ok(route.clone()));
        // The user's last session to the host ends it.
        masters.session_started(1, &target);
        masters.session_ended(1);
        crate::child::wait_until("the master outlived the session", || {
            check(&SystemSsh, &target, Some(&socket)) != Check::Live
        });
        // ⌘Q: a reopened master ends within the deadline, the directory goes.
        let (asker, _) = scripted(&[]);
        assert_eq!(masters.ensure(&target, Ask::Sheet(asker)), Ok(route));
        masters.close_all(Instant::now() + Duration::from_secs(5));
        assert_ne!(check(&SystemSsh, &target, Some(&socket)), Check::Live);
        assert!(
            !socket.parent().unwrap().exists(),
            "the instance directory stays"
        );
        let _ = sshd.kill();
        let _ = sshd.wait();
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// The acceptance scenario against a **password** server: the Docker sshd
    /// of the manual check (`127.0.0.1:2222`, `deneme`/`parola123`;
    /// skipped when it does not answer). The user's ssh runs in a real PTY
    /// and a password is saved: while ssh's password prompt is on screen the
    /// background gate says "not logged in" and no socket of ours exists; once
    /// the user types it, the background job opens our master with the saved
    /// password; the session's end closes it. `-F /dev/null`, no agent, a
    /// temporary `known_hosts`: `~/.ssh` is never read.
    /// The pane's wake for [`password_sshd_jobs_ride_the_users_session`]:
    /// when the user's ssh exited.
    #[derive(Default)]
    struct ExitWake(Mutex<Option<Instant>>);

    impl bt_core::Wake for ExitWake {
        fn wake(&self) {}
        fn child_exit(&self, _code: Option<i32>) {
            *lock(&self.0) = Some(Instant::now());
        }
        fn copy_to_clipboard(&self, _text: String) {}
        fn title_changed(&self) {}
        fn search_changed(&self) {}
        fn command_started(&self) {}
        fn remote_up(&self) {}
        fn remote_typed(&self) {}
        fn link_hover_lost(&self) {}
    }

    /// The terminal session's sharing against a real password sshd (the one
    /// above): the user's
    /// wrapped `ssh` (password typed in the terminal) is a master at the
    /// session socket; a background job — no saved password, so without the
    /// session socket it had no way in — rides it with `BatchMode=yes`, i.e. without a new
    /// login; ⌘Q's `close_all` and a session end leave the user's session
    /// alone; the user's `exit` returns at once although a job still rides
    /// the connection, and the master ends after it. `-F /dev/null`, no
    /// agent, a temporary `known_hosts`: `~/.ssh` is never read.
    #[test]
    #[ignore = "needs the password sshd on 127.0.0.1:2222: cargo test -p bt-shell-common password_sshd -- --ignored"]
    fn password_sshd_jobs_ride_the_users_session() {
        use bt_core::{
            CaretShape, CursorBlink, Osc52, Session, SessionOptions, TerminalOptions, Theme,
        };
        if std::net::TcpStream::connect(("127.0.0.1", 2222)).is_err() {
            eprintln!("SKIPPED: no sshd on 127.0.0.1:2222");
            return;
        }
        let root = scratch("pwride");
        let known = root.join("known_hosts").display().to_string();
        let argv: Vec<String> = [
            "ssh",
            "-F",
            "/dev/null",
            "-o",
            &format!("UserKnownHostsFile={known}"),
            "-o",
            "IdentityAgent=none",
            "-o",
            "PubkeyAuthentication=no",
            "-p",
            "2222",
            "deneme@127.0.0.1",
        ]
        .map(str::to_owned)
        .to_vec();
        let target = RemoteTarget {
            host: "deneme@127.0.0.1".to_owned(),
            kind: RemoteKind::Ssh,
            argv: argv.clone(),
            line: String::new(),
        };
        let masters = Arc::new(Masters::with_instance(
            PathBuf::from("/nonexistent"),
            vec![root.join("s")],
            Arc::new(NoStore),
            INSTANCE.to_owned(),
        ));
        // What `bateri ssh-argv` does: the session socket in the instance directory.
        let key = host_key(&config(&SystemSsh, &target).expect("ssh -G"));
        let session_socket = session_socket(masters.bases(), &key).expect("a session socket");
        let control = crate::ssh_wrap::Control {
            socket: session_socket.clone(),
        };
        let mut user_args = argv[1..].to_vec();
        user_args.insert(0, "StrictHostKeyChecking=accept-new".to_owned());
        user_args.insert(0, "-o".to_owned());
        let wrapped = crate::ssh_wrap::wrap(
            &user_args,
            "exec /bin/sh -i",
            None,
            "0123456789abcdef",
            None,
            Some(&control),
        );
        let wake = Arc::new(ExitWake::default());
        let session = Session::spawn(
            SessionOptions {
                command: Some(("ssh".to_owned(), wrapped)),
                working_directory: None,
                home: None,
                env: HashMap::new(),
                cols: 80,
                rows: 24,
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
            },
            Arc::clone(&wake) as Arc<dyn bt_core::Wake>,
        )
        .expect("session");
        let password_prompt = bt_core::TtyModes {
            canonical: true,
            echo: false,
        };
        crate::child::wait_until("no password prompt", || {
            session.with_pty_fd(crate::jobs::tty_modes) == Some(password_prompt)
        });
        // Before the login there is nothing to ride: no saved password, so the
        // background job has today's argv.
        assert_eq!(masters.ensure(&target, Ask::Never), Ok(Route::Direct));
        session.write(b"parola123\r");
        crate::child::wait_until("the session is no master", || {
            check(&SystemSsh, &target, Some(&session_socket)) == Check::Live
        });

        // A background job rides the user's session: no new login (BatchMode).
        let route = masters.ensure(&target, Ask::Never).expect("background");
        assert_eq!(route, Route::Ours(session_socket.clone()));
        let mut stream = crate::upload::ssh_argv_for(&target, &route);
        stream.push("echo rode".to_owned());
        let out = Command::new(&stream[0])
            .args(&stream[1..])
            .stdin(Stdio::null())
            .output()
            .expect("stream");
        assert_eq!(
            String::from_utf8_lossy(&out.stdout).trim(),
            "rode",
            "{out:?}"
        );

        // A session end and ⌘Q end only ours: the user's session lives on.
        masters.session_started(1, &target);
        masters.session_ended(1);
        masters.close_all(Instant::now() + Duration::from_secs(5));
        assert_eq!(
            check(&SystemSsh, &target, Some(&session_socket)),
            Check::Live,
            "bateri ended the user's session"
        );

        // A long job on it, then the user's `exit`: it returns at once.
        let mut long = crate::upload::ssh_argv_for(&target, &route);
        long.push("sleep 6".to_owned());
        let mut riding = Command::new(&long[0])
            .args(&long[1..])
            .stdin(Stdio::null())
            .spawn()
            .expect("long stream");
        std::thread::sleep(Duration::from_millis(500));
        let asked = Instant::now();
        session.write(b"exit\r");
        crate::child::wait_until("the user's exit waited for our job", || {
            lock(&wake.0).is_some()
        });
        let exited = lock(&wake.0).unwrap();
        assert!(
            exited.duration_since(asked) < Duration::from_secs(3),
            "exit took {:?}",
            exited.duration_since(asked)
        );
        // The job finishes on the detached master; then the master goes.
        assert!(riding.wait().expect("long stream").success());
        crate::child::wait_until("the session's master outlived its jobs", || {
            !session_socket.exists()
        });
        session.shutdown();
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    #[ignore = "needs the password sshd on 127.0.0.1:2222: cargo test -p bt-shell-common password_sshd -- --ignored"]
    fn password_sshd_background_waits_for_the_users_login() {
        use bt_core::{
            CaretShape, CursorBlink, Osc52, Session, SessionOptions, TerminalOptions, Theme,
        };
        if std::net::TcpStream::connect(("127.0.0.1", 2222)).is_err() {
            eprintln!("SKIPPED: no sshd on 127.0.0.1:2222");
            return;
        }
        let root = scratch("pwsshd");
        let known = root.join("known_hosts").display().to_string();
        let argv: Vec<String> = [
            "ssh",
            "-F",
            "/dev/null",
            "-o",
            &format!("UserKnownHostsFile={known}"),
            "-o",
            "IdentityAgent=none",
            "-o",
            "PubkeyAuthentication=no",
            "-p",
            "2222",
            "deneme@127.0.0.1",
        ]
        .map(str::to_owned)
        .to_vec();
        let target = RemoteTarget {
            host: "deneme@127.0.0.1".to_owned(),
            kind: RemoteKind::Ssh,
            argv: argv.clone(),
            line: String::new(),
        };
        // The user's terminal: our `C`, then the user's ssh (a new host key is
        // accepted into the temporary file — the master checks it strictly).
        let script = format!(
            "printf '\\033]133;C\\007'; exec {} -o StrictHostKeyChecking=accept-new {}",
            argv[..argv.len() - 1].join(" "),
            argv[argv.len() - 1],
        );
        let session = Session::spawn(
            SessionOptions {
                command: Some(("/bin/sh".to_owned(), vec!["-c".to_owned(), script])),
                working_directory: None,
                home: None,
                env: HashMap::new(),
                cols: 80,
                rows: 24,
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
            },
            Arc::new(crate::child::SilentWake),
        )
        .expect("session");
        crate::child::wait_until("no `C`", || session.running_command().is_some());
        let command = session.running_command().unwrap();
        session.set_remote(command, Some(&target));

        let store = Arc::new(MemoryStore::default());
        lock(&store.saved).insert(
            Account {
                host: "127.0.0.1".to_owned(),
                user: "deneme".to_owned(),
                port: 2222,
            },
            "parola123".to_owned(),
        );
        let masters = Arc::new(Masters::with_instance(
            askpass_wrapper(&root),
            vec![root.join("s")],
            Arc::clone(&store) as Arc<dyn PasswordStore>,
            INSTANCE.to_owned(),
        ));
        masters.session_started(1, &target);
        let password_prompt = bt_core::TtyModes {
            canonical: true,
            echo: false,
        };
        crate::child::wait_until("no password prompt", || {
            session.with_pty_fd(crate::jobs::tty_modes) == Some(password_prompt)
        });
        // The pane's background gate: not logged in, so nothing dials.
        assert_eq!(crate::jobs::remote_login(&session), None);
        assert!(
            our_sockets(&sockets(&root)).is_empty(),
            "a socket before the login"
        );

        session.write(b"parola123\r");
        crate::child::wait_until("the login was not seen", || {
            crate::jobs::remote_login(&session) == Some(command)
        });
        let route = masters.ensure(&target, Ask::Never).expect("background");
        let Route::Ours(socket) = route else {
            panic!("expected our master, got {route:?}");
        };
        assert_eq!(check(&SystemSsh, &target, Some(&socket)), Check::Live);

        // The user's session ends: the remote edge releases our master.
        session.write(b"exit\r");
        masters.session_ended(1);
        crate::child::wait_until("the master outlived the session", || {
            check(&SystemSsh, &target, Some(&socket)) != Check::Live
        });
        session.shutdown();
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn open_failures_say_why() {
        assert!(
            open_failure("prod", "Host key verification failed.\r\n")
                .contains("connect once in the terminal")
        );
        assert_eq!(open_failure("prod", ""), "ssh could not connect to prod.");
        assert_eq!(
            open_failure("prod", "ssh: Could not resolve hostname prod\r\n"),
            "ssh could not connect to prod.\n\nssh: Could not resolve hostname prod"
        );
    }

    #[test]
    fn a_broken_frame_is_refused() {
        let mut long = format!("{REQUEST} {}\n", WIRE_LIMIT + 1).into_bytes();
        long.extend(vec![b'x'; 8]);
        assert!(read_request(&mut &long[..]).is_err());
        assert!(read_request(&mut &b"HELLO 3\nabc"[..]).is_err());
        assert!(read_request(&mut &format!("{REQUEST} 5\nab").as_bytes()[..]).is_err());
        assert_eq!(
            read_request(&mut &format!("{REQUEST} 3\nabc").as_bytes()[..]).unwrap(),
            "abc"
        );
    }
}
