//! `ssh-hold` against an application of its own: a real unix socket in a
//! directory of the test's, the hub of `kablo::hub` behind it and the fake
//! server behind that. The pipe runs on pipes in a thread, or as a process of
//! its own on a real pseudo-terminal where the terminal's mode and the window
//! size signal matter.

use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::ffi::OsStringExt;
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use kablo_proto::v1::Exit;

use super::*;
use crate::kablo::fake::{Age, Bridge};
use crate::kablo::hub::{HoldId, Hub, HubEvent};
use crate::kablo::{Client, NewSession};

const ACCOUNT: &str = "rasp";

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("bt-hold-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a directory");
    dir
}

/// Waits until `ready` says true; panics with `message` after ten seconds.
fn until(message: &str, mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if ready() {
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("{message}");
}

/// What the application does when a holder asks for a terminal.
#[derive(Clone, Copy)]
enum Policy {
    /// Start a new one and resume an existing one.
    Serve,
    /// Start a new one under the number `t{n}` (the holder asked with a name of its own) and
    /// resume an existing one.
    Number(u64),
    /// Refuse with a plain `ssh` to fall back to.
    FallBack,
    /// Refuse with a line to show.
    Refuse,
    /// Say nothing, and close the connection at once.
    Hang,
    /// Say nothing and keep the connection: the request is being worked on.
    Silent,
}

/// The application, as far as the pipe can tell: a listener, the hub and the
/// server it rides on, stepped by hand.
struct App {
    listener: UnixListener,
    hub: Hub,
    bridge: Bridge,
    now: Instant,
    holds: Vec<(HoldId, UnixStream)>,
    policy: Policy,
    /// The holders that asked, in order.
    asked: Vec<HoldId>,
}

impl App {
    fn new(socket: &Path, mut bridge: Bridge, policy: Policy) -> App {
        let _ = std::fs::remove_file(socket);
        let listener = UnixListener::bind(socket).expect("a socket");
        listener.set_nonblocking(true).expect("non-blocking");
        let now = Instant::now();
        let mut hub = Hub::new();
        hub.add_client(ACCOUNT, Client::new(Some("MacBook"), now));
        // A fresh connection to the server, as a restarted application makes.
        bridge.disconnect();
        let mut app = App {
            listener,
            hub,
            bridge,
            now,
            holds: Vec::new(),
            policy,
            asked: Vec::new(),
        };
        app.hub.client_mut(ACCOUNT).expect("client").connected(now);
        app
    }

    /// The application goes: its connections close and its client is gone. The
    /// server is what is left.
    fn quit(self) -> Bridge {
        self.bridge
    }

    fn step(&mut self) {
        while let Ok((stream, _)) = self.listener.accept() {
            stream.set_nonblocking(false).expect("blocking");
            let hold = self.hub.open();
            self.holds.push((hold, stream));
        }
        let mut closed = Vec::new();
        let mut buffer = [0u8; 65536];
        for (hold, stream) in &mut self.holds {
            let mut fds = [libc::pollfd {
                fd: stream.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            }];
            // SAFETY: one valid descriptor, no wait.
            let n = unsafe { libc::poll(fds.as_mut_ptr(), 1, 0) };
            if n > 0 {
                match stream.read(&mut buffer) {
                    Ok(0) | Err(_) => closed.push(*hold),
                    Ok(n) => self.hub.push(*hold, &buffer[..n]),
                }
            }
        }
        for hold in closed {
            self.holds.retain(|(h, _)| *h != hold);
            self.hub.closed(hold);
        }
        while let Some(event) = self.hub.next_event() {
            if let HubEvent::Wants { hold, open } = event {
                self.asked.push(hold);
                self.answer(hold, &open);
            }
        }
        loop {
            let mut moved = false;
            let out = self
                .hub
                .client_mut(ACCOUNT)
                .expect("client")
                .take_outgoing();
            if !out.is_empty() {
                self.bridge.feed(&out);
                moved = true;
            }
            let back = self.bridge.take_output();
            if !back.is_empty() {
                let now = self.now;
                self.hub
                    .client_mut(ACCOUNT)
                    .expect("client")
                    .push(&back, now);
                moved = true;
            }
            self.hub.pump(ACCOUNT);
            if !moved {
                break;
            }
        }
        let mut finished = Vec::new();
        for (hold, stream) in &mut self.holds {
            let bytes = self.hub.take_outgoing(*hold);
            if !bytes.is_empty() {
                let _ = stream.write_all(&bytes);
            }
            if self.hub.is_finished(*hold) {
                finished.push(*hold);
            }
        }
        for hold in finished {
            self.holds.retain(|(h, _)| *h != hold);
            self.hub.closed(hold);
        }
    }

    fn answer(&mut self, hold: HoldId, open: &Open) {
        match (self.policy, &open.target) {
            (Policy::Serve, Target::New { .. }) => {
                let spec = NewSession::default();
                self.hub.start(hold, ACCOUNT, spec, None).expect("started");
            }
            (Policy::Number(number), Target::New { .. }) => {
                let spec = NewSession::default();
                let name = kablo_proto::SessionId::new(&format!("t{number}")).expect("a name");
                self.hub
                    .start(hold, ACCOUNT, spec, Some(name))
                    .expect("started");
            }
            (Policy::Serve | Policy::Number(_), Target::Resume { .. }) => {
                self.hub.resume(hold, ACCOUNT, false).expect("resumed");
            }
            (Policy::FallBack, _) => self.hub.refuse(hold, true, "no kablo on this server"),
            (Policy::Refuse, _) => self.hub.refuse(hold, false, "the terminal is gone"),
            (Policy::Hang, _) => {
                self.holds.retain(|(h, _)| *h != hold);
                self.hub.closed(hold);
            }
            (Policy::Silent, _) => {}
        }
    }

    /// Steps until `ready` is true.
    fn until(&mut self, message: &str, mut ready: impl FnMut(&mut App) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            self.step();
            if ready(self) {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("{message}");
    }

    /// The terminal the first (or only) bound holder shows.
    fn terminal(&mut self) -> kablo_proto::SessionId {
        self.until("a holder bound to a terminal", |app| {
            app.holds
                .iter()
                .any(|(hold, _)| app.hub.terminal_of(*hold).is_some())
        });
        let hold = self
            .holds
            .iter()
            .map(|(hold, _)| *hold)
            .find(|hold| self.hub.terminal_of(*hold).is_some())
            .expect("a bound holder");
        self.hub.terminal_of(hold).expect("bound").1.clone()
    }
}

/// A pipe: the read end and the write end.
fn pipe() -> (OwnedFd, OwnedFd) {
    let mut fds = [0 as libc::c_int; 2];
    // SAFETY: `pipe` fills two descriptors that are ours from now on.
    unsafe {
        assert_eq!(libc::pipe(fds.as_mut_ptr()), 0);
        for fd in fds {
            libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC);
        }
        (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1]))
    }
}

/// What is written to a descriptor, collected by a thread.
struct Capture {
    bytes: Arc<Mutex<Vec<u8>>>,
}

impl Capture {
    fn of(read: OwnedFd) -> Capture {
        let bytes = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&bytes);
        std::thread::spawn(move || {
            let mut file = File::from(read);
            let mut buffer = [0u8; 65536];
            while let Ok(n) = file.read(&mut buffer) {
                if n == 0 {
                    break;
                }
                sink.lock().expect("lock").extend_from_slice(&buffer[..n]);
            }
        });
        Capture { bytes }
    }

    fn bytes(&self) -> Vec<u8> {
        self.bytes.lock().expect("lock").clone()
    }
}

/// The pipe running on a thread, on pipes.
struct Running {
    handle: Option<JoinHandle<Outcome>>,
    typed: File,
    shown: Capture,
}

fn config(socket: &Path, target: Target, first: bool, input: RawFd, output: RawFd) -> Config {
    Config {
        terminal: SessionId::new("b-test").expect("an id"),
        target,
        first,
        sockets: vec![socket.to_path_buf()],
        input,
        output,
        backoff: vec![Duration::from_millis(20)],
        cancel: None,
    }
}

fn new_target() -> Target {
    Target::New {
        args: vec![b"rasp".to_vec()],
    }
}

fn run_on_pipes(socket: &Path, target: Target, first: bool) -> Running {
    let (stdin_read, stdin_write) = pipe();
    let (stdout_read, stdout_write) = pipe();
    let config = config(
        socket,
        target,
        first,
        stdin_read.as_raw_fd(),
        stdout_write.as_raw_fd(),
    );
    let handle = std::thread::spawn(move || {
        let outcome = run(&config);
        drop((stdin_read, stdout_write));
        outcome
    });
    Running {
        handle: Some(handle),
        typed: File::from(stdin_write),
        shown: Capture::of(stdout_read),
    }
}

impl Running {
    fn outcome(&mut self) -> Outcome {
        self.handle
            .take()
            .expect("running")
            .join()
            .expect("the pipe did not panic")
    }
}

#[test]
fn the_pipe_carries_the_bytes_and_the_keys_and_ends_with_the_programs_status() {
    let dir = scratch("carry");
    let socket = dir.join("hold");
    let mut app = App::new(&socket, Bridge::new(Age::Current), Policy::Serve);
    let mut hold = run_on_pipes(&socket, new_target(), false);
    let terminal = app.terminal();

    let mut expected = b"ready\r\n\x1b[31mred\x1b[0m".to_vec();
    expected.extend(0..=255u8);
    app.bridge.write(&terminal, &expected);
    app.until("the output shown", |_| hold.shown.bytes() == expected);

    hold.typed.write_all(b"ls -la\r").expect("typed");
    app.until("the keys typed", |app| {
        app.bridge.typed(&terminal) == b"ls -la\r"
    });

    app.bridge.exit(&terminal, Exit::Code(7));
    app.until("the pipe ended", |_| {
        hold.handle.as_ref().is_some_and(JoinHandle::is_finished)
    });
    assert_eq!(hold.outcome(), Outcome::Exit(7));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn without_an_application_the_plain_ssh_runs() {
    let dir = scratch("alone");
    // No listener at all, and a socket file nobody listens on.
    let mut hold = run_on_pipes(&dir.join("hold"), new_target(), false);
    assert_eq!(hold.outcome(), Outcome::FallBack);
    drop(UnixListener::bind(dir.join("stale")).expect("a socket"));
    let mut hold = run_on_pipes(&dir.join("stale"), new_target(), false);
    assert_eq!(hold.outcome(), Outcome::FallBack);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_request_that_cannot_be_served_falls_back_before_anything_is_shown() {
    let dir = scratch("refused");
    let socket = dir.join("hold");
    let mut app = App::new(&socket, Bridge::new(Age::Current), Policy::FallBack);
    let mut hold = run_on_pipes(&socket, new_target(), false);
    app.until("the pipe ended", |_| {
        hold.handle.as_ref().is_some_and(JoinHandle::is_finished)
    });
    assert_eq!(hold.outcome(), Outcome::FallBack);
    assert!(hold.shown.bytes().is_empty(), "nothing is printed");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_application_that_goes_before_answering_is_the_same_as_none() {
    let dir = scratch("hang");
    let socket = dir.join("hold");
    let mut app = App::new(&socket, Bridge::new(Age::Current), Policy::Hang);
    let mut hold = run_on_pipes(&socket, new_target(), false);
    app.until("the pipe ended", |_| {
        hold.handle.as_ref().is_some_and(JoinHandle::is_finished)
    });
    assert_eq!(hold.outcome(), Outcome::FallBack);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_terminal_that_is_gone_says_so_and_the_first_program_runs_a_shell() {
    let dir = scratch("gone");
    let socket = dir.join("hold");
    let mut app = App::new(&socket, Bridge::new(Age::Current), Policy::Refuse);
    let target = Target::Resume {
        account: ACCOUNT.into(),
        offset: Some(0),
    };
    let mut hold = run_on_pipes(&socket, target.clone(), true);
    app.until("the pipe ended", |_| {
        hold.handle.as_ref().is_some_and(JoinHandle::is_finished)
    });
    // The block the first program stood in as ends with what `ssh` says of a failure.
    assert_eq!(hold.outcome(), Outcome::Shell(255));
    until("the line shown", || {
        hold.shown.bytes() == b"the terminal is gone\n"
    });

    // Not as the first program: the shell that started it gets a status.
    let mut hold = run_on_pipes(&socket, target, false);
    app.until("the pipe ended", |_| {
        hold.handle.as_ref().is_some_and(JoinHandle::is_finished)
    });
    assert_eq!(hold.outcome(), Outcome::Exit(EXIT_REFUSED));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_ctrl_c_before_the_terminal_is_shown_gives_the_first_program_its_shell() {
    // The signal handler sets this flag and the pipe looks at it while it waits;
    // the pane's first program must not die of the interrupt (the pane would
    // close), it ends as an interrupted command and the shell follows.
    static CANCEL: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    let dir = scratch("cancel");
    let socket = dir.join("hold");
    let mut app = App::new(&socket, Bridge::new(Age::Current), Policy::Silent);
    let (stdin_read, stdin_write) = pipe();
    let (stdout_read, stdout_write) = pipe();
    let mut config = config(
        &socket,
        new_target(),
        true,
        stdin_read.as_raw_fd(),
        stdout_write.as_raw_fd(),
    );
    config.cancel = Some(&CANCEL);
    let handle = std::thread::spawn(move || {
        let outcome = run(&config);
        drop((stdin_read, stdout_write));
        outcome
    });
    app.until("the request waits", |app| !app.asked.is_empty());
    assert!(!handle.is_finished(), "the pipe waits for the terminal");
    CANCEL.store(true, std::sync::atomic::Ordering::Release);
    app.until("the pipe ended", |_| handle.is_finished());
    assert_eq!(handle.join().expect("no panic"), Outcome::Shell(130));
    drop((stdin_write, stdout_read));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_pane_survives_the_application_and_picks_up_where_it_stopped() {
    let dir = scratch("survive");
    let socket = dir.join("hold");
    let mut app = App::new(&socket, Bridge::new(Age::Current), Policy::Serve);
    let mut hold = run_on_pipes(&socket, new_target(), false);
    let terminal = app.terminal();
    app.bridge.write(&terminal, b"abc");
    app.until("abc shown", |_| hold.shown.bytes() == b"abc");

    // The application goes (an update, a crash): its sockets close with it.
    // The program on the server goes on writing.
    let mut bridge = app.quit();
    bridge.write(&terminal, b"def");
    assert!(
        hold.handle.as_ref().is_some_and(|h| !h.is_finished()),
        "the pipe waits for the application"
    );

    // A new application, with a client that has never heard of the terminal.
    let mut app = App::new(&socket, bridge, Policy::Serve);
    app.until("def shown", |_| hold.shown.bytes() == b"abcdef");
    app.bridge.write(&terminal, b"ghi");
    app.until("ghi shown", |_| hold.shown.bytes() == b"abcdefghi");

    // What is typed reaches the program again.
    hold.typed.write_all(b"x").expect("typed");
    app.until("the key typed", |app| app.bridge.typed(&terminal) == b"x");
    app.bridge.exit(&terminal, Exit::Signal(1));
    app.until("the pipe ended", |_| {
        hold.handle.as_ref().is_some_and(JoinHandle::is_finished)
    });
    assert_eq!(hold.outcome(), Outcome::Exit(129));
    assert_eq!(
        hold.shown.bytes(),
        b"abcdefghi",
        "no byte twice, none missing"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// The application numbers the terminal: the pipe asked with `b-…` of its own and the server has
/// `t7`. The pipe is told, and when the application comes back it asks for the terminal by the
/// number — by its own name it would find nothing.
#[test]
fn the_pane_asks_for_the_number_it_was_told_when_the_application_comes_back() {
    let dir = scratch("numbered");
    let socket = dir.join("hold");
    let mut app = App::new(&socket, Bridge::new(Age::Current), Policy::Number(7));
    let mut hold = run_on_pipes(&socket, new_target(), false);
    let terminal = app.terminal();
    assert_eq!(terminal.as_str(), "t7");
    app.bridge.write(&terminal, b"abc");
    app.until("abc shown", |_| hold.shown.bytes() == b"abc");

    let mut bridge = app.quit();
    bridge.write(&terminal, b"def");
    let mut app = App::new(&socket, bridge, Policy::Serve);
    app.until("def shown", |_| hold.shown.bytes() == b"abcdef");
    assert_eq!(app.terminal().as_str(), "t7");
    app.bridge.write(&terminal, b"ghi");
    app.until("ghi shown", |_| hold.shown.bytes() == b"abcdefghi");
    app.bridge.exit(&terminal, Exit::Code(0));
    app.until("the pipe ended", |_| {
        hold.handle.as_ref().is_some_and(JoinHandle::is_finished)
    });
    assert_eq!(hold.outcome(), Outcome::Exit(0));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_pipe_ends_when_the_pane_goes() {
    let dir = scratch("pane");
    let socket = dir.join("hold");
    let mut app = App::new(&socket, Bridge::new(Age::Current), Policy::Serve);
    let Running {
        mut handle,
        typed,
        shown: _,
    } = run_on_pipes(&socket, new_target(), false);
    let terminal = app.terminal();
    // The pane's terminal hangs up: nobody to show the terminal to.
    drop(typed);
    app.until("the pipe ended", |_| {
        handle.as_ref().is_some_and(JoinHandle::is_finished)
    });
    assert_eq!(
        handle.take().expect("running").join().expect("no panic"),
        Outcome::Exit(1)
    );
    // And the terminal lives on: the server still has it, and nobody is attached.
    app.until("the holder let go", |app| app.holds.is_empty());
    assert!(app.bridge.has(&terminal));
    assert!(!app.bridge.is_attached(&terminal));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_process_table_reader_walks_the_options_the_parser_walks() {
    let words = |list: &[&str]| list.iter().map(|&w| w.to_owned()).collect::<Vec<_>>();
    // Same words, same answer: what `parse` takes as the arguments is what the
    // pane's remote session is read from.
    for list in [
        &["--", "-p", "2222", "prod"][..],
        &["--first", "--", "prod"],
        &["--first", "--shell-arg", "-l", "--", "prod"],
        // An option's value that reads like the end of the options.
        &["--first", "--shell-arg", "--", "--", "prod"],
        &[
            "--terminal",
            "b-1",
            "--account",
            "u@h:22",
            "--offset",
            "5",
            "--",
            "prod",
        ],
    ] {
        let parsed = parse(&list.iter().map(|&w| os(w)).collect::<Vec<_>>()).expect("parses");
        let owned = words(list);
        let read = ssh_args_in(&owned).expect("arguments");
        assert_eq!(
            read.iter().map(OsString::from).collect::<Vec<_>>(),
            parsed.ssh_args,
            "{list:?}"
        );
    }
    // A pipe that resumes a terminal and names nothing, and a word that is no option.
    assert_eq!(
        ssh_args_in(&words(&["--first", "--terminal", "b-1", "--account", "a"])),
        None
    );
    assert_eq!(ssh_args_in(&words(&["--bogus", "--", "prod"])), None);
}

#[test]
fn the_command_line() {
    let ok = |args: &[&str]| parse(&args.iter().map(|a| os(a)).collect::<Vec<_>>());
    let new = ok(&["--", "rasp", "-p", "2222"]).expect("a new terminal");
    assert_eq!(new.ssh_args, [os("rasp"), os("-p"), os("2222")]);
    assert!(!new.first && new.terminal.is_none());
    let first = ok(&["--first", "--shell-arg", "--posix", "--", "rasp"]).expect("first");
    assert!(first.first);
    assert_eq!(first.shell_args, ["--posix"]);
    let existing = ok(&["--terminal", "b-1", "--account", "rasp", "--offset", "42"]).expect("one");
    assert_eq!(existing.offset, Some(42));
    // The arguments of an existing terminal name its server for whoever reads the process's
    // command line; nothing is asked of them.
    let named = ok(&[
        "--first",
        "--terminal",
        "b-1",
        "--account",
        "rasp",
        "--",
        "-p",
        "2222",
        "rasp",
    ])
    .expect("an existing terminal and the server's name");
    assert_eq!(named.ssh_args, [os("-p"), os("2222"), os("rasp")]);
    // An argument of ssh that looks like ours stays ssh's.
    let odd = ok(&["--", "--first", "-oX=y"]).expect("odd");
    assert!(!odd.first);
    assert_eq!(odd.ssh_args, [os("--first"), os("-oX=y")]);

    for broken in [
        &["--bogus"][..],
        &["--terminal", "b-1"][..],
        &["--account", "rasp"][..],
        &["--offset", "1", "--", "rasp"][..],
        &["--offset", "x", "--terminal", "b-1", "--account", "a"][..],
        &["--terminal"][..],
    ] {
        assert_eq!(ok(broken), None, "{broken:?}");
    }
}

fn no_shell(_: &[String]) -> Option<ShellCommand> {
    None
}

#[test]
fn the_process_without_an_application_runs_plain_ssh_with_the_arguments_as_typed() {
    let dir = scratch("main");
    let env = Env {
        instance: Some("0a1b2c3d".into()),
        roots: vec![dir.clone()],
        ssh: os("ssh"),
        shell: no_shell,
    };
    assert_eq!(env.sockets(), [dir.join("0a1b2c3d").join(HOLD_SOCKET)]);
    let action = hold_main(
        &[
            os("--"),
            os("-p"),
            os("2222"),
            os("user@rasp"),
            os("uptime"),
        ],
        &env,
    );
    assert_eq!(
        action,
        Action::Exec {
            program: os("ssh"),
            args: vec![os("-p"), os("2222"), os("user@rasp"), os("uptime")]
        }
    );
    // Arguments that are not text reach ssh as they are.
    let raw = OsString::from_vec(vec![b'h', 0xff, b'x']);
    assert_eq!(
        hold_main(&[os("--"), raw.clone()], &env),
        Action::Exec {
            program: os("ssh"),
            args: vec![raw]
        }
    );
    // A bad command line is a usage error, not a window.
    assert_eq!(hold_main(&[os("--bogus")], &env), Action::Exit(EXIT_USAGE));
    assert_eq!(
        hold_main(&[], &env),
        Action::Exec {
            program: os("ssh"),
            args: vec![]
        }
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_sockets_are_the_instances_and_nothing_a_name_can_escape_to() {
    let env = |instance: Option<&str>| Env {
        instance: instance.map(str::to_owned),
        roots: vec![PathBuf::from("/a"), PathBuf::from("/b")],
        ssh: os("ssh"),
        shell: no_shell,
    };
    assert_eq!(
        env(Some("0a1b2c3d")).sockets(),
        [
            PathBuf::from("/a/0a1b2c3d/hold"),
            PathBuf::from("/b/0a1b2c3d/hold")
        ]
    );
    for bad in [
        None,
        Some(""),
        Some("../x"),
        Some("a/b"),
        Some(".hidden"),
        Some("a b"),
    ] {
        assert!(env(bad).sockets().is_empty(), "{bad:?}");
    }
}

// The pipe as a process, on a real terminal

/// The environment variable that makes this test binary act as `ssh-hold`.
const PROCESS_ENV: &str = "BT_TEST_SSH_HOLD";

/// Not a test of its own: the process the tests below start. The variable
/// holds `mode`, `socket` and the arguments, separated by US (0x1f). Without
/// it this does nothing.
///
/// - `pipe`: runs the pipe on the terminal it was started on and exits with
///   its outcome (a status; 90 plain `ssh`, 91 a shell, 92 the terminal's mode
///   was not given back).
/// - `main`: `hold_main` and `execute`, with `/bin/echo` for `ssh` and a
///   stand-in for the shell.
#[test]
fn ssh_hold_process() {
    let Ok(spec) = std::env::var(PROCESS_ENV) else {
        return;
    };
    let mut parts = spec.split('\u{1f}');
    let mode = parts.next().unwrap_or_default().to_owned();
    let socket = PathBuf::from(parts.next().unwrap_or_default());
    let rest: Vec<OsString> = parts.map(OsString::from).collect();
    let code = match mode.as_str() {
        "pipe" => {
            let config = Config {
                terminal: SessionId::new("b-proc").expect("an id"),
                target: new_target(),
                first: false,
                sockets: vec![socket],
                input: libc::STDIN_FILENO,
                output: libc::STDOUT_FILENO,
                backoff: vec![Duration::from_millis(20)],
                cancel: None,
            };
            let before = modes(libc::STDIN_FILENO);
            let outcome = run(&config);
            // The terminal is given back as it was, checked where it is still
            // the process's own (a terminal whose session leader has exited is
            // hung up).
            let after = modes(libc::STDIN_FILENO);
            match outcome {
                _ if !same_modes(&before, &after) => 92,
                Outcome::Exit(code) => code,
                Outcome::FallBack => 90,
                Outcome::Shell(_) => 91,
            }
        }
        _ => {
            fn stand_in_shell(args: &[String]) -> Option<ShellCommand> {
                let mut sh = vec![
                    "-c".to_owned(),
                    "echo started \"$@\"".to_owned(),
                    "sh".to_owned(),
                ];
                sh.extend_from_slice(args);
                Some(("/bin/sh".to_owned(), sh))
            }
            let env = Env {
                instance: Some("0a1b2c3d".into()),
                roots: vec![socket],
                ssh: os("/bin/echo"),
                shell: stand_in_shell,
            };
            execute(hold_main(&rest, &env))
        }
    };
    std::process::exit(code);
}

fn process(spec: &str) -> Command {
    let mut command = Command::new(std::env::current_exe().expect("this binary"));
    command
        .args(["--exact", "hold::tests::ssh_hold_process", "--nocapture"])
        .args(["--test-threads", "1", "-q"])
        .env(PROCESS_ENV, spec);
    command
}

#[test]
fn the_process_runs_plain_ssh_or_the_shell_in_its_place() {
    let dir = scratch("exec");
    let out = |spec: &str| {
        let output = process(spec)
            .stdin(Stdio::null())
            .output()
            .expect("the process");
        (
            output.status.code(),
            String::from_utf8_lossy(&output.stdout).replace('\r', ""),
        )
    };
    let sep = '\u{1f}';
    // Nobody listens: `ssh` is run, in the process's place, with the arguments.
    let (code, stdout) = out(&format!(
        "main{sep}{}{sep}--{sep}-p{sep}22{sep}rasp",
        dir.display()
    ));
    assert_eq!(code, Some(0));
    assert!(stdout.contains("-p 22 rasp"), "{stdout:?}");

    // A first program whose terminal is gone: the line, then the shell, with
    // the arguments the pane would have started it with. (Its standard input is
    // a pipe that stays open: the pipe ends when the terminal it types on goes.)
    let instance = dir.join("0a1b2c3d");
    std::fs::create_dir_all(&instance).expect("an instance directory");
    let mut app = App::new(
        &instance.join(HOLD_SOCKET),
        Bridge::new(Age::Current),
        Policy::Refuse,
    );
    let child = process(&format!(
        "main{sep}{}{sep}--first{sep}--shell-arg{sep}--posix{sep}--terminal{sep}b-1{sep}--account{sep}rasp",
        dir.display()
    ))
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .spawn()
    .expect("the process");
    let mut child = Reaper(Some(child));
    app.until("the process ended", |_| {
        child
            .0
            .as_mut()
            .is_some_and(|c| matches!(c.try_wait(), Ok(Some(_))))
    });
    let mut stdout = String::new();
    child
        .0
        .as_mut()
        .and_then(|c| c.stdout.take())
        .expect("stdout")
        .read_to_string(&mut stdout)
        .expect("read");
    assert!(stdout.contains("the terminal is gone"), "{stdout:?}");
    assert!(stdout.contains("started --posix"), "{stdout:?}");
    // The pane reads this first program as the `ssh` its shell never ran: the marks of
    // that command come before anything else and its `D` carries the status the failure
    // had, then the shell starts.
    let begun = bt_core::first_command_begun();
    let ended = bt_core::first_command_ended(255);
    let (begun, ended) = (
        String::from_utf8_lossy(&begun).into_owned(),
        String::from_utf8_lossy(&ended).into_owned(),
    );
    let at_begin = stdout.find(&begun).expect("the block began");
    let at_end = stdout.find(&ended).expect("the block ended");
    assert!(
        at_begin < stdout.find("the terminal is gone").expect("the note")
            && stdout.find("the terminal is gone") < Some(at_end),
        "the note is the command's, between its `A` and its `D`: {stdout:?}"
    );
    assert!(
        at_end < stdout.find("started --posix").expect("the shell"),
        "the shell follows the `D`: {stdout:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_first_program_that_cannot_show_a_terminal_runs_ssh_as_a_command_and_then_the_shell() {
    let dir = scratch("first-ssh");
    let sep = '\u{1f}';
    // Nobody listens: the plain `ssh` (here `echo`) runs as a child, not in the process's
    // place, and the shell is what follows it; the marks bracket it.
    let output = process(&format!(
        "main{sep}{}{sep}--first{sep}--shell-arg{sep}--posix{sep}--{sep}-p{sep}22{sep}rasp",
        dir.display()
    ))
    .stdin(Stdio::null())
    .output()
    .expect("the process");
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout).replace('\r', "");
    let begun = String::from_utf8_lossy(&bt_core::first_command_begun()).into_owned();
    let ended = String::from_utf8_lossy(&bt_core::first_command_ended(0)).into_owned();
    let began = stdout.find(&begun).expect("the block began");
    let ssh = stdout.find("-p 22 rasp").expect("ssh ran");
    let end = stdout
        .find(&ended)
        .expect("the block ended with ssh's status");
    let shell = stdout.find("started --posix").expect("the shell ran");
    assert!(began < ssh && ssh < end && end < shell, "{stdout:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Kills a child that is still there when the test is over.
struct Reaper(Option<Child>);

impl Drop for Reaper {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            let _ = child.kill();
            // A process cannot finish exiting while output it wrote waits
            // unread on a terminal's master that someone keeps open: do not wait
            // for it longer than the test can afford.
            for _ in 0..100 {
                if matches!(child.try_wait(), Ok(Some(_))) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        }
    }
}

/// `ptsname` is not reentrant.
static PTSNAME: Mutex<()> = Mutex::new(());

/// A pseudo-terminal: the master (non-blocking) and the slave.
fn open_pty() -> (File, File) {
    let master = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_NOCTTY)
        .open("/dev/ptmx")
        .expect("ptmx");
    let fd = master.as_raw_fd();
    // SAFETY: `fd` is a PTY master; the name is copied out under the lock.
    let name = unsafe {
        assert_eq!(libc::grantpt(fd), 0);
        assert_eq!(libc::unlockpt(fd), 0);
        let _guard = PTSNAME.lock().expect("lock");
        std::ffi::CStr::from_ptr(libc::ptsname(fd))
            .to_string_lossy()
            .into_owned()
    };
    // SAFETY: only the status flags change.
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFL);
        libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
    }
    let slave = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_NOCTTY)
        .open(name)
        .expect("the slave");
    (master, slave)
}

fn modes(fd: RawFd) -> libc::termios {
    // SAFETY: `tcgetattr` fills the struct or fails.
    unsafe {
        let mut modes: libc::termios = std::mem::zeroed();
        let rc = libc::tcgetattr(fd, &mut modes);
        assert_eq!(rc, 0, "tcgetattr: {}", std::io::Error::last_os_error());
        modes
    }
}

/// What can be compared of two terminal modes: everything but the two
/// bits of the line discipline's own state (input waiting to be retyped,
/// output being flushed), which the kernel sets as the mode changes under
/// pending input and which are not a mode.
fn same_modes(a: &libc::termios, b: &libc::termios) -> bool {
    let state = libc::PENDIN | libc::FLUSHO;
    a.c_iflag == b.c_iflag
        && a.c_oflag == b.c_oflag
        && a.c_cflag == b.c_cflag
        && a.c_lflag & !state == b.c_lflag & !state
        && a.c_cc == b.c_cc
}

#[test]
fn on_a_real_terminal_the_pipe_is_raw_follows_the_size_and_gives_the_terminal_back() {
    let dir = scratch("tty");
    let socket = dir.join("hold");
    let mut app = App::new(&socket, Bridge::new(Age::Current), Policy::Serve);
    let (master, slave) = open_pty();
    let mut size = libc::winsize {
        ws_row: 24,
        ws_col: 80,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    // SAFETY: a valid out-parameter for this request.
    unsafe { libc::ioctl(master.as_raw_fd(), libc::TIOCSWINSZ as _, &size) };

    let mut command = process(&format!("pipe\u{1f}{}", socket.display()));
    command
        .stdin(slave.try_clone().expect("clone"))
        .stdout(slave.try_clone().expect("clone"))
        .stderr(slave.try_clone().expect("clone"));
    // SAFETY: only async-signal-safe calls between fork and exec. The child
    // leads a session of its own with the terminal as its controlling one, so
    // it is the terminal's foreground group and the size signal reaches it.
    unsafe {
        command.pre_exec(|| {
            libc::setsid();
            if libc::ioctl(0, libc::TIOCSCTTY as _, 0) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            for fd in 3..1024 {
                libc::close(fd);
            }
            Ok(())
        });
    }
    let mut child = Reaper(Some(command.spawn().expect("the process")));
    let terminal = app.terminal();
    let read = |wanted: usize| {
        let mut got = Vec::new();
        let mut master = &master;
        until("the bytes at the terminal", || {
            let mut buffer = [0u8; 4096];
            if let Ok(n) = master.read(&mut buffer) {
                got.extend_from_slice(&buffer[..n]);
            }
            got.len() >= wanted
        });
        got
    };

    // What the test harness of the process printed before the pipe began is
    // not the pipe's: take it off the terminal.
    {
        let mut master = &master;
        let mut buffer = [0u8; 4096];
        while matches!(master.read(&mut buffer), Ok(n) if n > 0) {}
    }

    // The terminal is raw while the terminal is shown: from the moment the
    // pane is told the terminal exists (before that it is the command line it was).
    until("the terminal is raw", || {
        modes(master.as_raw_fd()).c_lflag & libc::ICANON == 0
    });
    let raw = modes(master.as_raw_fd());
    assert_eq!(raw.c_lflag & (libc::ECHO | libc::ICANON | libc::ISIG), 0);
    assert_eq!(raw.c_oflag & libc::OPOST, 0);

    // Bytes arrive as the program wrote them: a bare line feed stays one.
    let written = b"a\nb\r\n\x1b[1mc\x1b[0m\x07".to_vec();
    app.bridge.write(&terminal, &written);
    app.step();
    assert_eq!(read(written.len()), written);

    // Keys arrive as typed: a carriage return stays one, Ctrl-C is a byte.
    let mut master_w = &master;
    master_w.write_all(b"ls\r\x03").expect("typed");
    app.until("the keys at the program", |app| {
        app.bridge.typed(&terminal) == b"ls\r\x03"
    });

    // The window changes size: the signal reaches the pipe, and the program's
    // terminal follows.
    size.ws_col = 100;
    size.ws_row = 30;
    size.ws_xpixel = 800;
    size.ws_ypixel = 600;
    // SAFETY: a valid in-parameter for this request.
    unsafe { libc::ioctl(master.as_raw_fd(), libc::TIOCSWINSZ as _, &size) };
    app.until("the program's size follows", |app| {
        app.bridge
            .size_of(&terminal)
            .is_some_and(|s| (s.cols, s.rows, s.xpixel, s.ypixel) == (100, 30, 800, 600))
    });

    // The program ends: the pipe exits with its status and the terminal is as it was.
    app.bridge.exit(&terminal, Exit::Code(7));
    app.until("the process ended", |_| {
        child
            .0
            .as_mut()
            .is_some_and(|c| matches!(c.try_wait(), Ok(Some(_))))
    });
    let status = child.0.as_mut().expect("child").wait().expect("status");
    assert_eq!(status.code(), Some(7));
    // 92 would say the terminal was not given back as it was.
    let _ = std::fs::remove_dir_all(&dir);
}
