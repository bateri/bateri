//! Which ssh connection a remote file job rides on (047): bateri's own master
//! connection, the user's, or a new master of ours — decided **once, before the
//! job starts** (`.tasks/047-ssh-parola-ve-keychain/discussion.md` → Karar).
//!
//! The pieces here are the pure and the process half of that gate; nothing is
//! wired to a consumer yet (047 phase-2 does):
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

use std::fmt::Write as _;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use bt_core::RemoteTarget;

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

/// The route's **decision** half: our socket's check, the user's master's
/// check (`None` when the user has no control path) and where our socket would
/// be (`None` when no base fits the socket limit).
///
/// Ours first: once bateri has a master to the host, the user's configuration
/// need not be asked again. The user's master next: it is already open and the
/// user chose it. Only then a master of ours.
pub fn decide(ours: Check, user: Option<Check>, socket: Option<&Path>) -> Plan {
    let Some(socket) = socket else {
        return Plan::Ready(Route::Direct);
    };
    if ours == Check::Live {
        return Plan::Ready(Route::Ours(socket.to_owned()));
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
/// the user's own control path.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SshConfig {
    pub user: String,
    pub hostname: String,
    pub port: String,
    /// `none` when there is no jump host (ssh's own spelling).
    pub proxyjump: String,
    /// `None` for `none` or an absent line.
    pub control_path: Option<PathBuf>,
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
    if names_control_path(target) {
        return Plan::Ready(Route::Direct);
    }
    let config = runner
        .run(&with_options(target, &words(&["-G"])))
        .ok()
        .filter(|(code, _, _)| *code == Some(0))
        .and_then(|(_, out, _)| parse_config(&out));
    let Some(config) = config else {
        return Plan::Ready(Route::Direct);
    };
    let socket = socket_path(bases, &host_key(&config));
    let ours = match &socket {
        Some(socket) => check(runner, target, Some(socket)),
        None => Check::Absent,
    };
    if ours == Check::Stale
        && let Some(socket) = &socket
    {
        let _ = std::fs::remove_file(socket);
    }
    // Asked only when ours is not alive: the answer would not change the route.
    let user = config
        .control_path
        .as_ref()
        .filter(|path| ours != Check::Live && Some(path.as_path()) != socket.as_deref())
        .map(|_| check(runner, target, None));
    decide(ours, user, socket.as_deref())
}

/// `ssh -O check` on our socket (`Some`) or on the user's own control path
/// (`None`: the target's argv and configuration decide it). `BatchMode=yes`:
/// the check never connects, and if it did, it must not ask.
fn check(runner: &dyn SshRunner, target: &RemoteTarget, socket: Option<&Path>) -> Check {
    let mut ours = words(&["-o", "BatchMode=yes"]);
    if let Some(socket) = socket {
        ours.push("-o".to_owned());
        ours.push(format!("ControlPath={}", socket.display()));
    }
    ours.extend(words(&["-O", "check"]));
    match runner.run(&with_options(target, &ours)) {
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
/// 048 makes the user's terminal session a master at the same path (discussion
/// → Karar 9). FNV and not `DefaultHasher`: the name must not change with the
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
/// temporary suffix.
pub fn fits(path: &Path) -> bool {
    path.as_os_str().len() + SSH_TEMP_SUFFIX < SUN_PATH
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
/// connection, short enough that an idle server is not held. bateri does not
/// close masters on quit (another bateri instance or 048's session may ride
/// on it); this is the stop condition.
pub const CONTROL_PERSIST: Duration = Duration::from_secs(600);

/// Who asked for the master: a job the user started (a sheet may ask for the
/// password) or a background job (one silent attempt from the Keychain, 047
/// phase-3).
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
/// an unknown host key is refused, never asked (Karar 7). A background master
/// gets **one** password prompt — a stale Keychain password must not lock the
/// account out.
pub fn master_argv(target: &RemoteTarget, socket: &Path, asker: Asker) -> Vec<String> {
    let mut ours = words(&["-M", "-N", "-f", "-o"]);
    ours.push(format!("ControlPath={}", socket.display()));
    ours.push("-o".to_owned());
    ours.push(format!("ControlPersist={}", CONTROL_PERSIST.as_secs()));
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::os::unix::net::UnixListener;
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
    fn the_decision_has_four_arms() {
        let socket = Path::new("/tmp/s/0123456789abcdef");
        let ours = Plan::Ready(Route::Ours(socket.to_owned()));
        // Our master alive: ours, whatever the user's says.
        assert_eq!(decide(Check::Live, Some(Check::Live), Some(socket)), ours);
        assert_eq!(decide(Check::Live, None, Some(socket)), ours);
        // The user's master alive: today's argv.
        assert_eq!(
            decide(Check::Absent, Some(Check::Live), Some(socket)),
            Plan::Ready(Route::Direct)
        );
        // Neither (a stale socket counts as none): open ours.
        for (mine, theirs) in [
            (Check::Absent, None),
            (Check::Stale, Some(Check::Absent)),
            (Check::Absent, Some(Check::Stale)),
        ] {
            assert_eq!(
                decide(mine, theirs, Some(socket)),
                Plan::Open(socket.to_owned())
            );
        }
        // No socket of ours possible: today's behaviour.
        assert_eq!(
            decide(Check::Absent, None, None),
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
        };
        let key = host_key(&config);
        assert_eq!(key.len(), KEY_DIGITS);
        assert!(key.bytes().all(|byte| byte.is_ascii_hexdigit()));
        // Pinned: the name outlives the process (and 048 computes it too).
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
        let fallback = PathBuf::from(format!("/tmp/bateri-{}", u32::MAX)).join(key);
        assert!(fits(&fallback), "{}", fallback.display());
        // The longest home whose cache socket still fits, then one byte more.
        let tail = socket_bases(Some(Path::new("/")), 0)[0]
            .strip_prefix("/")
            .unwrap()
            .join(key);
        let longest = SUN_PATH - SSH_TEMP_SUFFIX - 1 - tail.as_os_str().len() - 1;
        let home = |len: usize| PathBuf::from(format!("/{}", "u".repeat(len - 1)));
        assert!(fits(&home(longest).join(&tail)));
        assert!(!fits(&home(longest + 1).join(&tail)));
        // The default home on macOS (/Users/<name>) leaves room for a long name.
        assert!(longest >= "/Users/".len() + 32, "{longest}");

        // With a home that does not fit, the socket lands under the fallback.
        let root = scratch("budget");
        let bases = vec![
            home(longest + 1).join("Library/Caches/bateri/s"),
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
