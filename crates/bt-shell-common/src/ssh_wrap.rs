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
use crate::ssh_route::{Account, SshRunner, parse_config};

/// The bootstrap's `$0` and the wrapped command's last word: what [`unwrap`]
/// checks at the last position, together with [`COMMAND_HEAD`].
pub const BOOT_NAME: &str = "bateri-boot";

/// The remote command's head. `exec` replaces the login shell sshd started
/// (`$SHELL -c '<command>'`, which may be fish or csh): one word, a single
/// quote and no backslash, the quoting every shell reads the same way (037's
/// upload rule).
const COMMAND_HEAD: &str = "exec sh -c '";

/// The remote command for a bootstrap script: `exec sh -c '<boot>' bateri-boot`.
/// `boot` must not contain `'` ([`decide`] refuses one that does).
pub fn remote_command(boot: &str) -> String {
    format!("{COMMAND_HEAD}{boot}' {BOOT_NAME}")
}

/// The user's ssh arguments (without the program) → the wrapped arguments:
/// `-t`, then `args` as they are, then [`remote_command`]. The user's own
/// `-t` stays (`-t -t` still asks for a terminal).
pub fn wrap(args: &[String], boot: &str) -> Vec<String> {
    let mut wrapped = Vec::with_capacity(args.len() + 2);
    wrapped.push("-t".to_owned());
    wrapped.extend(args.iter().cloned());
    wrapped.push(remote_command(boot));
    wrapped
}

/// The inverse of [`wrap`]: the user's arguments if `args` has the wrapped
/// shape, otherwise `args` itself. The shape is positional: `-t` first, a
/// bootstrap command last, and in between an interactive call with no remote
/// command of its own.
pub fn unwrap(args: &[String]) -> &[String] {
    let [first, inner @ .., last] = args else {
        return args;
    };
    let boot = last
        .strip_prefix(COMMAND_HEAD)
        .and_then(|rest| rest.strip_suffix(BOOT_NAME))
        .and_then(|rest| rest.strip_suffix("' "));
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
/// learned state of the server it resolves to.
pub fn decide(
    args: &[String],
    tty: bool,
    settings: &Settings,
    runner: &dyn SshRunner,
    state: &HostState,
    boot: &str,
) -> Option<Wrapped> {
    if boot.is_empty() || boot.contains('\'') || !tty {
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
    state.knows(Fact::Posix, &key).then(|| Wrapped {
        args: wrap(args, boot),
        key,
    })
}

/// The bootstrap script (phase-2 embeds it). Empty until then, so [`decide`]
/// wraps nothing and the user sees no difference.
pub const BOOT: &str = "";

/// `bateri ssh-argv [--tty] -- <ssh arguments…>`: the subcommand's body
/// (`argv` is what follows `ssh-argv`). Writes the wrapped arguments to `out`,
/// each followed by a NUL, or nothing; the exit code is always zero and every
/// failure is "nothing" — the caller runs plain `ssh`.
///
/// `--tty` says stdin and stdout are terminals: the caller asks, because our
/// own stdout is the caller's pipe. `settings` is the settings file's launch
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
    boot: &str,
    out: &mut impl Write,
) -> i32 {
    let (tty, rest) = match argv {
        [flag, rest @ ..] if flag == "--tty" => (true, rest),
        rest => (false, rest),
    };
    let Some(("--", args)) = rest.split_first().map(|(head, tail)| (head.as_str(), tail)) else {
        return 0;
    };
    let state = load(state_path);
    let Some(wrapped) = decide(args, tty, settings, runner, &state, boot) else {
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
            let wrapped = wrap(&args, BOOT_STUB);
            assert_eq!(wrapped.first().map(String::as_str), Some("-t"));
            assert_eq!(unwrap(&wrapped), &args[..], "{wrapped:?}");
            // An argv that was never wrapped is itself.
            assert_eq!(unwrap(&args), &args[..]);
        }
    }

    #[test]
    fn unwrap_reads_positions_not_contents() {
        // The command somewhere other than last, or no leading `-t`: not ours.
        let command = remote_command(BOOT_STUB);
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
        ] {
            assert_eq!(unwrap(&args), &args[..], "{args:?}");
        }
    }

    #[test]
    fn the_wrapped_call_still_reads_as_the_users() {
        // The remote probe sees the wrapped process; its target is the typed one.
        let wrapped = wrap(&words(&["-o", "User=x", "--", "prod"]), BOOT_STUB);
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
            decide(&args, true, &settings, &runner, &learned(), BOOT_STUB),
            Some(Wrapped {
                args: wrap(&args, BOOT_STUB),
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
            let wrapped = decide(&words(args), tty, &settings, &runner, &learned(), BOOT_STUB);
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
        // The bootstrap is still a placeholder (phase-2): nothing is wrapped.
        assert_eq!(
            decide(&args, true, &settings, &runner, &learned(), BOOT),
            None
        );
        assert_eq!(
            decide(&args, true, &settings, &runner, &learned(), "it's"),
            None
        );
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
                decide(&args, true, &on, &runner, &learned(), BOOT_STUB),
                None,
                "{out}"
            );
        }
        // R1.4: an unlearned server.
        let runner = Gconfig::new(PLAIN);
        assert_eq!(
            decide(&args, true, &on, &runner, &HostState::default(), BOOT_STUB),
            None
        );
        // R1.3: the setting is asked before `ssh -G`.
        let off = Settings {
            remote_integration: false,
            ..Settings::default()
        };
        let runner = Gconfig::new(PLAIN);
        assert_eq!(
            decide(&args, true, &off, &runner, &learned(), BOOT_STUB),
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
            decide(&args, true, &production, &runner, &learned(), BOOT_STUB),
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
                boot,
                &mut out,
            );
            assert_eq!(code, 0);
            out
        };
        let printed = run(&["--tty", "--", "-p", "2", "prod"], BOOT_STUB);
        let expected: Vec<u8> = wrap(&words(&["-p", "2", "prod"]), BOOT_STUB)
            .iter()
            .flat_map(|arg| arg.bytes().chain([0]))
            .collect();
        assert_eq!(printed, expected);
        assert!(load(&path).knows(Fact::Touched, "u@h:22"));
        // Without the terminal bit, without `--`, with today's placeholder: nothing.
        assert!(run(&["--", "prod"], BOOT_STUB).is_empty());
        assert!(run(&["--tty", "prod"], BOOT_STUB).is_empty());
        assert!(run(&["--tty", "--", "prod"], BOOT).is_empty());
        // The touched row cannot be written (another writer holds the lock past
        // the patience): nothing either.
        let held = lock(&path, LOCK_PATIENCE).unwrap();
        assert!(run(&["--tty", "--", "prod"], BOOT_STUB).is_empty());
        drop(held);
        fs::remove_dir_all(&dir).unwrap();
    }
}
