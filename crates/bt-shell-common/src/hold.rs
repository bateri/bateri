//! `bateri ssh-hold`: a pane's program that shows a terminal kept on a kablo
//! server.
//!
//! The pane is an ordinary one — its own pseudo-terminal, its own screen, its
//! scrollback, search and command blocks — and the program in front is this
//! pipe. It connects to the running bateri over a local socket (the
//! instance directory's `hold`, [`HOLD_SOCKET`]), asks for a terminal and then
//! only carries bytes: what the terminal's program wrote goes to standard
//! output, what the user types goes to the terminal, and the pane's size
//! follows it. It parses nothing and decides nothing; the application's end of
//! the pipe (`kablo::hub`) is the client of the server.
//!
//! **The terminal's mode.** Standard input is a terminal; for as long as the
//! terminal is shown it is **raw** (every key reaches the program as the byte
//! it is, nothing echoed, nothing translated on the way out) and the mode it had
//! comes back on every way out, the `exec` of another program included. A
//! terminal that does not exist yet is made raw only once bateri has tied the
//! pane to one ([`ToHold::Bound`]): until then the pane is an ordinary command
//! line, so Ctrl-C ends the wait and a password typed to a program bateri asked
//! for ([`ToHold::Run`]) is read the way a terminal reads it, and what is typed
//! meanwhile waits for the terminal.
//!
//! **Surviving the application.** The pane outlives bateri when the
//! update or a crash is bridged by the holder (`handover`): this program is
//! one of the processes it keeps. When the connection to bateri is lost,
//! standard input is no longer read (what the user types waits in the
//! terminal), the socket is tried again with growing pauses, and a bateri that
//! answers is asked for the terminal it was showing, from the position after
//! the last byte this program wrote ([`wire::Target::Resume`]) — the program
//! keeps that one number, the account's name and the terminal's own name (the one bateri
//! started it under, told in [`wire::ToHold::Bound`]: a server's terminals are numbered by
//! bateri, not by this program), nothing of the screen. A
//! connection lost **before** the terminal was bound to a server, or a
//! request that cannot be served, is not retried: the program replaces itself
//! with the plain `ssh` it stands in for (see [`Outcome::FallBack`]), because
//! nothing has been shown yet.
//!
//! **Ending.** When the terminal's program ends this exits with its status
//! (a signal is 128 plus its number), so the shell that started it sees what
//! `ssh` would have shown. A pane that started with this program as its **first**
//! program (`--first`: a pane opened on the same server, or restored) has no
//! shell to return to, so it runs the shell it would have started
//! ([`Outcome::Shell`]).
//!
//! **As the first program it is also the `ssh` the shell never ran.** The
//! command a typed `ssh` is, for the pane, is the shell's marks around it: the
//! probe that names the server, the remote shell's blocks, the dock and the
//! title all start from them. A first program has no shell to print them, so it
//! prints them itself ([`bt_core::first_command_begun`] before it connects,
//! [`bt_core::first_command_ended`] with the terminal's status when it is over,
//! then the shell is started), and the pane is, to everything that reads the
//! stream, a pane whose user typed `ssh`. When no terminal can be shown the
//! plain `ssh` runs as a command of that block and the shell follows it; it is
//! never `exec`ed, since nothing would be left to return to. A Ctrl-C before the
//! terminal is shown does not end it either (the pane would close with its first
//! program): it gives up waiting ([`Config::cancel`]) and the shell follows, the
//! block ending with the status 130 of an interrupted command.
//!
//! The socket's wire is [`wire`]; the arguments are [`USAGE`].

pub mod wire;

use std::ffi::OsString;
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::time::Duration;

use kablo_proto::SessionId;
use kablo_proto::v1::Size;

use wire::{CHUNK, HoldReader, OLDEST, Open, Target, ToApp, ToHold, VERSION};

/// The subcommand's word, the first argument.
pub const SUBCOMMAND: &str = "ssh-hold";

/// The listener's name in the instance directory.
pub const HOLD_SOCKET: &str = "hold";

/// The exit status of a bad command line (`EX_USAGE`).
pub const EXIT_USAGE: i32 = 64;

/// The exit status when the terminal could not be shown and there was no
/// plain `ssh` to fall back to (what `ssh` itself says when it cannot connect).
pub const EXIT_REFUSED: i32 = 255;

/// The command line.
pub const USAGE: &str = "usage: bateri ssh-hold [--first] [--close-at-end] [--shell-arg ARG]... \
[--terminal ID --account ACCOUNT [--offset N]] [-- SSH-ARGUMENT...]";

/// The pauses between attempts to reach bateri again, by how many have
/// failed in a row; the last repeats.
pub const BACKOFF: [Duration; 6] = [
    Duration::from_millis(100),
    Duration::from_millis(200),
    Duration::from_millis(400),
    Duration::from_millis(800),
    Duration::from_millis(1600),
    Duration::from_millis(2000),
];

/// What to do when the pipe is done, for the process to carry out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// Exit with this status.
    Exit(i32),
    /// Replace this process with another program.
    Exec {
        /// The program, looked up on `PATH` when it has no `/`.
        program: OsString,
        /// Its arguments.
        args: Vec<OsString>,
    },
}

/// How the pipe ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Exit with this status.
    Exit(i32),
    /// Nothing was shown and the terminal cannot be: run the plain `ssh` with
    /// the arguments this was started with.
    FallBack,
    /// Run the shell the pane would have started; the terminal's status is
    /// what the block the first program stood in as ends with.
    Shell(u8),
}

/// A program and its arguments.
pub type ShellCommand = (String, Vec<String>);

/// What the process needs from its surroundings, so a test brings its own.
pub struct Env {
    /// The name of the bateri instance directory this pane was born under
    /// (`BATERI_SSH_INSTANCE`); it stays the same across an update.
    pub instance: Option<String>,
    /// The roots instance directories are looked for under.
    pub roots: Vec<PathBuf>,
    /// The plain `ssh`.
    pub ssh: OsString,
    /// The shell a pane starts, from the extra arguments it takes
    /// (`child::shell_command`).
    pub shell: fn(&[String]) -> Option<ShellCommand>,
}

impl Env {
    /// The sockets to try, in order: the instance's, under each root.
    pub fn sockets(&self) -> Vec<PathBuf> {
        let Some(instance) = self.instance.as_deref().filter(|name| plain_name(name)) else {
            return Vec::new();
        };
        self.roots
            .iter()
            .map(|root| root.join(instance).join(HOLD_SOCKET))
            .collect()
    }
}

/// A directory name that names no other directory.
fn plain_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        && !name.starts_with('.')
}

/// What the command line asks.
#[derive(Debug, PartialEq, Eq)]
struct Parsed {
    first: bool,
    close_at_end: bool,
    shell_args: Vec<String>,
    terminal: Option<String>,
    account: Option<String>,
    offset: Option<u64>,
    ssh_args: Vec<OsString>,
}

fn parse(args: &[OsString]) -> Option<Parsed> {
    let mut parsed = Parsed {
        first: false,
        close_at_end: false,
        shell_args: Vec::new(),
        terminal: None,
        account: None,
        offset: None,
        ssh_args: Vec::new(),
    };
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.to_str()? {
            "--first" => parsed.first = true,
            "--close-at-end" => parsed.close_at_end = true,
            "--shell-arg" => parsed.shell_args.push(args.next()?.to_str()?.to_owned()),
            "--terminal" => parsed.terminal = Some(args.next()?.to_str()?.to_owned()),
            "--account" => parsed.account = Some(args.next()?.to_str()?.to_owned()),
            "--offset" => parsed.offset = Some(args.next()?.to_str()?.parse().ok()?),
            "--" => {
                parsed.ssh_args = args.cloned().collect();
                break;
            }
            _ => return None,
        }
    }
    // An existing terminal is named and has an account; a new one has none of
    // the first two. The arguments of an existing terminal name the server for
    // whoever reads this process's command line (the pane's remote session is
    // read from it, [`crate::jobs`]): they are not asked for anything, the
    // account is the terminal's place.
    let existing = parsed.terminal.is_some() || parsed.account.is_some();
    if existing {
        if parsed.terminal.is_none() || parsed.account.is_none() {
            return None;
        }
    } else if parsed.offset.is_some() {
        return None;
    }
    Some(parsed)
}

/// The `ssh` arguments a pipe's command line carries, read from the argument
/// vector a process table shows (`args`: what follows the subcommand's word): the
/// words after the options [`parse`] knows, with the same walk — an option's value
/// is skipped whatever it is, so a value that reads like the `--` that ends the
/// options is not taken for it. `None`: no arguments (a pipe that resumes a terminal
/// it was shown has none) or a word that is not an option.
pub fn ssh_args_in(args: &[String]) -> Option<&[String]> {
    let mut at = 0;
    while let Some(arg) = args.get(at) {
        match arg.as_str() {
            "--first" | "--close-at-end" => at += 1,
            "--shell-arg" | "--terminal" | "--account" | "--offset" => at += 2,
            "--" => return Some(&args[at + 1..]),
            _ => return None,
        }
    }
    None
}

/// A name for a new terminal: `b-` and 128 random bits as hex. Made here, not
/// by the application, so that asking again after a lost connection names the
/// same terminal.
pub(crate) fn new_terminal_name() -> io::Result<SessionId> {
    let mut bytes = [0u8; 16];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    let mut name = String::from("b-");
    for byte in bytes {
        name.push_str(&format!("{byte:02x}"));
    }
    SessionId::new(&name).map_err(|_| io::ErrorKind::InvalidData.into())
}

/// `bateri ssh-hold …`'s body, up to the point where the process replaces
/// itself or exits: parses the command line, shows the terminal and says what
/// comes next ([`execute`] does it).
pub fn hold_main(args: &[OsString], env: &Env) -> Action {
    let Some(parsed) = parse(args) else {
        eprintln!("{USAGE}");
        return Action::Exit(EXIT_USAGE);
    };
    let plain_ssh = || Action::Exec {
        program: env.ssh.clone(),
        args: parsed.ssh_args.clone(),
    };
    let (terminal, target) = match (&parsed.terminal, &parsed.account) {
        (Some(terminal), Some(account)) => {
            let Ok(terminal) = SessionId::new(terminal) else {
                eprintln!("{USAGE}");
                return Action::Exit(EXIT_USAGE);
            };
            (
                terminal,
                Target::Resume {
                    account: account.clone(),
                    offset: parsed.offset,
                },
            )
        }
        _ => {
            let Ok(terminal) = new_terminal_name() else {
                return plain_ssh();
            };
            (
                terminal,
                Target::New {
                    args: parsed
                        .ssh_args
                        .iter()
                        .map(|arg| arg.as_bytes().to_vec())
                        .collect(),
                },
            )
        }
    };
    let config = Config {
        terminal,
        target,
        first: parsed.first,
        close_at_end: parsed.close_at_end,
        sockets: env.sockets(),
        input: libc::STDIN_FILENO,
        output: libc::STDOUT_FILENO,
        backoff: BACKOFF.to_vec(),
        cancel: parsed.first.then(catch_interrupts),
    };
    // A first program stands in for the command the shell never ran: the pane
    // reads its session from the marks of that command (module docs).
    if parsed.first {
        let _ = write_all(libc::STDOUT_FILENO, &bt_core::first_command_begun());
    }
    let shell = |code: u8| {
        let _ = write_all(libc::STDOUT_FILENO, &bt_core::first_command_ended(code));
        match (env.shell)(&parsed.shell_args) {
            Some((program, args)) => Action::Exec {
                program: program.into(),
                args: args.into_iter().map(OsString::from).collect(),
            },
            None => Action::Exit(1),
        }
    };
    match run(&config) {
        Outcome::Exit(code) => Action::Exit(code),
        // The `ssh` the terminal stood in for runs as the command, and the shell
        // follows it: this process is the pane's program and the shell is what
        // is left to return to.
        Outcome::FallBack if parsed.first => {
            let code = std::process::Command::new(&env.ssh)
                .args(&parsed.ssh_args)
                .status()
                .map_or(255, status_code);
            shell(code)
        }
        Outcome::FallBack => plain_ssh(),
        Outcome::Shell(code) => shell(code),
    }
}

/// Carries out an [`Action`]: the exit status, or the program that takes the
/// process over (and the status 127 if it cannot start).
pub fn execute(action: Action) -> i32 {
    match action {
        Action::Exit(code) => code,
        Action::Exec { program, args } => {
            let error = std::process::Command::new(&program).args(&args).exec();
            eprintln!("bateri: cannot run {}: {error}", program.to_string_lossy());
            127
        }
    }
}

/// Everything [`run`] needs.
pub struct Config {
    /// The terminal's name: what a new terminal is asked for with, and what a terminal that
    /// bateri has not renamed ([`ToHold::Bound::terminal`]) is asked for again with.
    pub terminal: SessionId,
    /// What to ask for.
    pub target: Target,
    /// The pane's first program: when the terminal ends, a shell follows.
    pub first: bool,
    /// The pane is the terminal's and no more: when the terminal ends, or is gone when asked for,
    /// this ends with the status and the pane closes with it, even for a first program (a pane a
    /// program on a server asked for, whose program was the point of it). The user's Ctrl-C
    /// before the terminal is shown still leaves the shell a first program leaves.
    pub close_at_end: bool,
    /// The sockets of bateri to try, in order.
    pub sockets: Vec<PathBuf>,
    /// The terminal the user types on.
    pub input: RawFd,
    /// The terminal the program's bytes are written to.
    pub output: RawFd,
    /// The pauses between attempts to reach bateri again.
    pub backoff: Vec<Duration>,
    /// Set when the user gave up waiting for the terminal (Ctrl-C before it is shown): the
    /// pipe stops asking and ends. Only the pane's first program has one — it is the whole
    /// pane, and a Ctrl-C that killed it would close the pane — while a typed `ssh`'s
    /// pipe is killed by the signal as `ssh` would be.
    pub cancel: Option<&'static AtomicBool>,
}

/// What the pipe keeps between connections: the name of the server's account
/// once there is one, the name the terminal has when bateri gave it another than
/// the one asked with, and the position after the last byte it wrote.
struct State {
    account: Option<String>,
    /// The terminal's name as bateri told it ([`ToHold::Bound`]); the one in
    /// [`Config::terminal`] until it does.
    terminal: Option<SessionId>,
    offset: Option<u64>,
    open: Target,
    size: Size,
}

impl State {
    fn new(config: &Config) -> State {
        let (account, offset) = match &config.target {
            Target::New { .. } => (None, Some(0)),
            Target::Resume { account, offset } => (Some(account.clone()), *offset),
        };
        State {
            account,
            terminal: None,
            offset,
            open: config.target.clone(),
            size: window_size(config.input),
        }
    }

    /// Whether the terminal is tied to a server, so that bateri can be asked
    /// for it again.
    fn committed(&self) -> bool {
        self.account.is_some()
    }

    /// What to ask bateri for now.
    fn request(&self, config: &Config) -> Open {
        let target = match &self.account {
            Some(account) => Target::Resume {
                account: account.clone(),
                offset: self.offset,
            },
            None => self.open.clone(),
        };
        Open {
            terminal: self
                .terminal
                .clone()
                .unwrap_or_else(|| config.terminal.clone()),
            size: self.size,
            target,
        }
    }
}

/// How the pipe ended and what to say to the user before the process goes on.
struct Finish {
    outcome: Outcome,
    note: Option<String>,
}

impl Finish {
    fn quiet(outcome: Outcome) -> Finish {
        Finish {
            outcome,
            note: None,
        }
    }
}

enum Served {
    /// The pipe is over.
    Done(Finish),
    /// The connection to bateri was lost; `welcomed`: after it had agreed to
    /// serve (so the pauses start over).
    Lost { welcomed: bool },
}

/// Shows the terminal until it ends. Puts the terminal raw for the time,
/// follows bateri across restarts, and gives the terminal back before it
/// returns (and before the note, which is printed cooked).
pub fn run(config: &Config) -> Outcome {
    // SAFETY: `isatty` has no preconditions.
    let terminal = unsafe { libc::isatty(config.input) } != 0;
    let winch = terminal.then(|| Winch::install().ok()).flatten();
    let mut state = State::new(config);
    // A terminal tied to a server already is shown at once; a new one is
    // shown raw when bateri says it exists.
    let mut raw = state
        .committed()
        .then(|| Raw::enter(config.input))
        .flatten();
    let mut failures = 0usize;
    let finish = loop {
        let mut welcomed = false;
        if let Ok(mut stream) = connect(&config.sockets) {
            match serve(config, &mut state, &mut stream, winch.as_ref(), &mut raw) {
                Served::Done(finish) => break finish,
                Served::Lost { welcomed: yes } => welcomed = yes,
            }
        }
        if !state.committed() {
            // Nothing has been shown: the plain `ssh` is still possible.
            break Finish::quiet(Outcome::FallBack);
        }
        failures = if welcomed { 0 } else { failures + 1 };
        let pause = config
            .backoff
            .get(failures.saturating_sub(1))
            .or(config.backoff.last())
            .copied()
            .unwrap_or(Duration::from_secs(1));
        wait(winch.as_ref(), pause);
    };
    drop(winch);
    drop(raw);
    if let Some(note) = finish.note.filter(|note| !note.is_empty()) {
        let _ = write_all(config.output, format!("{note}\n").as_bytes());
    }
    finish.outcome
}

fn connect(sockets: &[PathBuf]) -> io::Result<UnixStream> {
    let mut last = io::Error::from(io::ErrorKind::NotFound);
    for socket in sockets {
        match UnixStream::connect(socket) {
            Ok(stream) => return Ok(stream),
            Err(error) => last = error,
        }
    }
    Err(last)
}

/// One connection's conversation.
fn serve(
    config: &Config,
    state: &mut State,
    stream: &mut UnixStream,
    winch: Option<&Winch>,
    raw: &mut Option<Raw>,
) -> Served {
    let lost = |welcomed| Served::Lost { welcomed };
    let hello = ToApp::Hello {
        newest: VERSION,
        oldest: OLDEST,
    };
    if send(stream, &hello).is_err() {
        return lost(false);
    }
    let mut reader = HoldReader::new();
    let mut buffer = vec![0u8; CHUNK];
    let mut welcomed = false;
    loop {
        if cancelled(config, raw) {
            // The user gave up: what was asked of bateri is let go of with the
            // connection, and the pane gets its shell with the status of an
            // interrupted command.
            return Served::Done(Finish::quiet(if config.first {
                Outcome::Shell(STATUS_INTERRUPTED)
            } else {
                Outcome::Exit(i32::from(STATUS_INTERRUPTED))
            }));
        }
        let mut fds = [
            poll_in(stream.as_raw_fd()),
            poll_in(winch.map_or(-1, |winch| winch.read)),
            // What is typed before the terminal exists waits for it.
            poll_in(if welcomed && state.committed() {
                config.input
            } else {
                -1
            }),
        ];
        // A signal wakes the wait by itself; while the user can still cancel, it
        // is also looked at now and then (the flag is a plain one).
        let patience = if config.cancel.is_some() && raw.is_none() {
            CANCEL_LOOK
        } else {
            -1
        };
        if poll(&mut fds, patience).is_err() {
            return lost(welcomed);
        }
        if ready(&fds[1]) {
            winch.inspect(|winch| winch.drain());
            let size = window_size(config.input);
            if size != state.size {
                state.size = size;
                if welcomed && send(stream, &ToApp::Resize(size)).is_err() {
                    return lost(welcomed);
                }
            }
        }
        if ready(&fds[0]) {
            match stream.read(&mut buffer) {
                Ok(0) => return lost(welcomed),
                Ok(n) => reader.push(&buffer[..n]),
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(_) => return lost(welcomed),
            }
            loop {
                let message = match reader.read() {
                    Ok(Some(message)) => message,
                    Ok(None) => break,
                    Err(_) => return lost(welcomed),
                };
                match message {
                    ToHold::Welcome { version } => {
                        if welcomed || !(OLDEST..=VERSION).contains(&version) {
                            return lost(welcomed);
                        }
                        welcomed = true;
                        state.size = window_size(config.input);
                        let open = ToApp::Open(state.request(config));
                        if send(stream, &open).is_err() {
                            return lost(welcomed);
                        }
                    }
                    _ if !welcomed => return lost(welcomed),
                    ToHold::Refused { fall_back, text } => {
                        return Served::Done(refused(config, state, fall_back, text));
                    }
                    ToHold::Bound { account, terminal } => {
                        state.account = Some(account);
                        if terminal.is_some() {
                            state.terminal = terminal;
                        }
                        raw_if_needed(config, raw);
                    }
                    ToHold::Run { argv } => {
                        let status = run_program(config, &argv, raw);
                        if send(stream, &ToApp::Ran { status }).is_err() {
                            return lost(welcomed);
                        }
                    }
                    ToHold::Note { text } => {
                        let line = format!("\x1b[0m\x1b[2m{text}\x1b[0m\r\n");
                        let _ = write_all(config.output, line.as_bytes());
                    }
                    ToHold::Output { end, data } => {
                        raw_if_needed(config, raw);
                        if write_all(config.output, &data).is_err() {
                            return Served::Done(Finish::quiet(Outcome::Exit(1)));
                        }
                        state.offset = Some(end);
                    }
                    ToHold::End { code, note } => {
                        let outcome = if config.first && !config.close_at_end {
                            Outcome::Shell(code)
                        } else {
                            Outcome::Exit(i32::from(code))
                        };
                        return Served::Done(Finish { outcome, note });
                    }
                    ToHold::Bell => {
                        let _ = write_all(config.output, b"\x07");
                    }
                }
            }
        }
        if welcomed && state.committed() && ready(&fds[2]) {
            match read_fd(config.input, &mut buffer) {
                // The pane's terminal is gone (hung up): there is nobody to show
                // the terminal to. The connection closing lets bateri go of it.
                Ok(0) | Err(_) => return Served::Done(Finish::quiet(Outcome::Exit(1))),
                Ok(n) => {
                    if send(stream, &ToApp::Input(buffer[..n].to_vec())).is_err() {
                        return lost(welcomed);
                    }
                }
            }
        }
    }
}

/// The status of a command the user interrupted, as a shell reads it.
const STATUS_INTERRUPTED: u8 = 130;

/// How often, in milliseconds, the wait for the terminal looks at [`Config::cancel`].
const CANCEL_LOOK: i32 = 250;

/// Whether the user gave up before the terminal was shown. Once the pipe has the
/// terminal raw an interrupt is the terminal's own byte (`^C` goes to the
/// server), so a flag set by a stray signal then means nothing and is cleared.
fn cancelled(config: &Config, raw: &Option<Raw>) -> bool {
    let Some(flag) = config.cancel else {
        return false;
    };
    flag.swap(false, Ordering::AcqRel) && raw.is_none()
}

/// Set by `SIGINT`'s handler.
static INTERRUPTED: AtomicBool = AtomicBool::new(false);

extern "C" fn on_interrupt(_: libc::c_int) {
    INTERRUPTED.store(true, Ordering::Release);
}

/// Has `SIGINT` set [`INTERRUPTED`] instead of ending the process, and gives the
/// flag. The handler is not restarted, so a wait is woken by it; a program run
/// for bateri (the `ssh` that makes the connection) gets the default again at
/// its `exec`, and the signal reaches it as it reaches this process: both are in
/// the terminal's foreground group.
fn catch_interrupts() -> &'static AtomicBool {
    // SAFETY: the handler is an `extern "C"` function that only stores to an
    // atomic; `sigaction` is given a zeroed struct with the fields set.
    unsafe {
        let mut action: libc::sigaction = std::mem::zeroed();
        let handler: extern "C" fn(libc::c_int) = on_interrupt;
        action.sa_sigaction = handler as usize;
        action.sa_flags = 0;
        libc::sigemptyset(&mut action.sa_mask);
        libc::sigaction(libc::SIGINT, &action, std::ptr::null_mut());
    }
    &INTERRUPTED
}

/// The terminal raw, if it is not already (and is one).
fn raw_if_needed(config: &Config, raw: &mut Option<Raw>) {
    if raw.is_none() {
        *raw = Raw::enter(config.input);
    }
}

/// Runs the program bateri asked for on the pane's terminal and says how it
/// ended: its status, 128 plus the signal that ended it, or 255 when it could
/// not be started. The terminal has the mode it had when this started for the
/// time (a terminal that was raw is raw again after).
fn run_program(config: &Config, argv: &[Vec<u8>], raw: &mut Option<Raw>) -> u8 {
    let was_raw = raw.take().is_some();
    let status = match argv.split_first() {
        Some((program, args)) => std::process::Command::new(std::ffi::OsStr::from_bytes(program))
            .args(args.iter().map(|arg| std::ffi::OsStr::from_bytes(arg)))
            .status()
            .ok()
            .map(status_code),
        None => None,
    };
    if was_raw {
        *raw = Raw::enter(config.input);
    }
    status.unwrap_or(255)
}

/// How a program ended as the status a shell would see: its code, 128 plus the
/// signal that ended it, or 255 when it says neither.
fn status_code(status: std::process::ExitStatus) -> u8 {
    use std::os::unix::process::ExitStatusExt;
    match (status.code(), status.signal()) {
        (Some(code), _) => u8::try_from(code).unwrap_or(1),
        (None, Some(signal)) => u8::try_from(128 + signal).unwrap_or(255),
        (None, None) => 255,
    }
}

/// bateri will not show a terminal.
fn refused(config: &Config, state: &State, fall_back: bool, text: String) -> Finish {
    if fall_back && !state.committed() {
        return Finish::quiet(Outcome::FallBack);
    }
    Finish {
        outcome: if config.first && !config.close_at_end {
            Outcome::Shell(u8::try_from(EXIT_REFUSED).unwrap_or(u8::MAX))
        } else {
            Outcome::Exit(EXIT_REFUSED)
        },
        note: Some(text),
    }
}

fn send(stream: &mut UnixStream, message: &ToApp) -> io::Result<()> {
    let mut bytes = Vec::new();
    message.encode(&mut bytes);
    stream.write_all(&bytes)
}

// The terminal

/// The terminal's size, zeros when `fd` is not a terminal.
fn window_size(fd: RawFd) -> Size {
    let mut size = libc::winsize {
        ws_row: 0,
        ws_col: 0,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    // SAFETY: `size` is a valid out-parameter for this request; a failure
    // leaves the zeros.
    unsafe { libc::ioctl(fd, libc::TIOCGWINSZ as _, &mut size) };
    Size {
        cols: size.ws_col,
        rows: size.ws_row,
        xpixel: size.ws_xpixel,
        ypixel: size.ws_ypixel,
    }
}

/// The terminal in raw mode, until it is dropped.
struct Raw {
    fd: RawFd,
    saved: libc::termios,
}

impl Raw {
    /// `None` when `fd` is not a terminal.
    fn enter(fd: RawFd) -> Option<Raw> {
        // SAFETY: `tcgetattr` fills the struct or fails; `isatty` has no
        // preconditions.
        unsafe {
            if libc::isatty(fd) == 0 {
                return None;
            }
            let mut saved: libc::termios = std::mem::zeroed();
            if libc::tcgetattr(fd, &mut saved) != 0 {
                return None;
            }
            let mut raw = saved;
            libc::cfmakeraw(&mut raw);
            if libc::tcsetattr(fd, libc::TCSANOW, &raw) != 0 {
                return None;
            }
            Some(Raw { fd, saved })
        }
    }
}

impl Drop for Raw {
    fn drop(&mut self) {
        // SAFETY: restores what `tcgetattr` returned; waits for the output
        // already written, and keeps what was typed ahead for whoever reads next.
        unsafe { libc::tcsetattr(self.fd, libc::TCSADRAIN, &self.saved) };
    }
}

/// The write end `SIGWINCH`'s handler tells; one program has one terminal.
static WINCH_WRITE: AtomicI32 = AtomicI32::new(-1);

/// A pipe the window-size signal writes a byte to, so that `poll` wakes on it.
struct Winch {
    read: RawFd,
    write: RawFd,
    previous: libc::sigaction,
}

extern "C" fn on_winch(_: libc::c_int) {
    let fd = WINCH_WRITE.load(Ordering::Relaxed);
    if fd >= 0 {
        // SAFETY: `write` is async-signal-safe; the descriptor is open while
        // it is stored here; the error number is put back for the code the
        // signal interrupted.
        unsafe {
            let errno = errno_location();
            let saved = *errno;
            libc::write(fd, [1u8].as_ptr().cast(), 1);
            *errno = saved;
        }
    }
}

#[cfg(target_os = "macos")]
unsafe fn errno_location() -> *mut libc::c_int {
    unsafe { libc::__error() }
}

#[cfg(not(target_os = "macos"))]
unsafe fn errno_location() -> *mut libc::c_int {
    unsafe { libc::__errno_location() }
}

impl Winch {
    fn install() -> io::Result<Winch> {
        let mut fds = [0 as libc::c_int; 2];
        // SAFETY: `pipe` fills two descriptors; the flags change only them; the
        // handler is an `extern "C"` function that does only what is allowed
        // in a handler.
        unsafe {
            if libc::pipe(fds.as_mut_ptr()) != 0 {
                return Err(io::Error::last_os_error());
            }
            for fd in fds {
                libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC);
                let flags = libc::fcntl(fd, libc::F_GETFL);
                libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
            }
            WINCH_WRITE.store(fds[1], Ordering::Relaxed);
            let mut action: libc::sigaction = std::mem::zeroed();
            let handler: extern "C" fn(libc::c_int) = on_winch;
            action.sa_sigaction = handler as usize;
            action.sa_flags = libc::SA_RESTART;
            libc::sigemptyset(&mut action.sa_mask);
            let mut previous: libc::sigaction = std::mem::zeroed();
            if libc::sigaction(libc::SIGWINCH, &action, &mut previous) != 0 {
                WINCH_WRITE.store(-1, Ordering::Relaxed);
                let error = io::Error::last_os_error();
                libc::close(fds[0]);
                libc::close(fds[1]);
                return Err(error);
            }
            Ok(Winch {
                read: fds[0],
                write: fds[1],
                previous,
            })
        }
    }

    /// Empties the pipe.
    fn drain(&self) {
        let mut sink = [0u8; 64];
        while matches!(read_fd(self.read, &mut sink), Ok(n) if n > 0) {}
    }
}

impl Drop for Winch {
    fn drop(&mut self) {
        // SAFETY: puts back the action that was there, and only then closes
        // what the handler writes to.
        unsafe {
            libc::sigaction(libc::SIGWINCH, &self.previous, std::ptr::null_mut());
            WINCH_WRITE.store(-1, Ordering::Relaxed);
            libc::close(self.read);
            libc::close(self.write);
        }
    }
}

/// Waits `pause`, answering the window-size signal meanwhile (the next
/// connection says the size it has then).
fn wait(winch: Option<&Winch>, pause: Duration) {
    let until = std::time::Instant::now() + pause;
    while let Some(left) = until.checked_duration_since(std::time::Instant::now()) {
        let mut fds = [poll_in(winch.map_or(-1, |winch| winch.read))];
        let millis = i32::try_from(left.as_millis()).unwrap_or(i32::MAX).max(1);
        if poll(&mut fds, millis).is_err() {
            return;
        }
        if ready(&fds[0]) {
            winch.inspect(|winch| winch.drain());
        }
    }
}

// Descriptors

fn poll_in(fd: RawFd) -> libc::pollfd {
    libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    }
}

/// Whether something happened on the descriptor that a `read` would find out
/// about: data, end of file, an error or a descriptor that is not open (which
/// `poll` would otherwise report at once, for ever).
fn ready(fd: &libc::pollfd) -> bool {
    fd.revents & (libc::POLLIN | libc::POLLHUP | libc::POLLERR | libc::POLLNVAL) != 0
}

/// `poll` that a signal does not fail: an interrupted wait reports nothing
/// ready and the caller waits again.
fn poll(fds: &mut [libc::pollfd], timeout: i32) -> io::Result<usize> {
    // SAFETY: the pointer and length are of the slice.
    let n = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, timeout) };
    if n >= 0 {
        return Ok(n as usize);
    }
    let error = io::Error::last_os_error();
    if error.kind() == io::ErrorKind::Interrupted {
        for fd in fds.iter_mut() {
            fd.revents = 0;
        }
        return Ok(0);
    }
    Err(error)
}

fn read_fd(fd: RawFd, buffer: &mut [u8]) -> io::Result<usize> {
    loop {
        // SAFETY: the pointer and length are of the buffer.
        let n = unsafe { libc::read(fd, buffer.as_mut_ptr().cast(), buffer.len()) };
        if n >= 0 {
            return Ok(n as usize);
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
}

/// Writes every byte, waiting out a full pipe.
fn write_all(fd: RawFd, mut bytes: &[u8]) -> io::Result<()> {
    while !bytes.is_empty() {
        // SAFETY: the pointer and length are of the slice.
        let n = unsafe { libc::write(fd, bytes.as_ptr().cast(), bytes.len()) };
        if n < 0 {
            let error = io::Error::last_os_error();
            match error.kind() {
                io::ErrorKind::Interrupted => continue,
                io::ErrorKind::WouldBlock => {
                    let mut fds = [libc::pollfd {
                        fd,
                        events: libc::POLLOUT,
                        revents: 0,
                    }];
                    poll(&mut fds, -1)?;
                    continue;
                }
                _ => return Err(error),
            }
        }
        bytes = &bytes[n as usize..];
    }
    Ok(())
}

/// `OsStr` for the arguments the tests build.
#[cfg(test)]
fn os(text: &str) -> OsString {
    OsString::from(text)
}

#[cfg(test)]
mod tests;
