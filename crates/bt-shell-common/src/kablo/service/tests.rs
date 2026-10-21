//! The service against holders that are the real `ssh-hold` pipe (on pipes, in
//! threads) and a server that is the fake bridge, over a unix socket in a
//! directory of the test's.

use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::fs::DirBuilderExt;
use std::sync::atomic::{AtomicU32, AtomicU64};
use std::thread::JoinHandle;

use kablo_proto::SessionId;
use kablo_proto::v1::{Exit, stream};

use super::*;
use crate::hold::wire::Target;
use crate::hold::{Config, Outcome, run};
use crate::kablo::fake::{Activity, Age, Bridge};
use crate::kablo::install::Installed;
use crate::program::identity::{Agent, Kind};

/// A private directory with a path short enough for a socket.
fn scratch(name: &str) -> PathBuf {
    let dir = PathBuf::from(format!("/tmp/bt-svc-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&dir)
        .expect("a private directory");
    dir
}

fn until(message: &str, mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        if ready() {
            return;
        }
        thread::sleep(Duration::from_millis(5));
    }
    panic!("{message}");
}

fn host(key: &str) -> KabloHost {
    KabloHost {
        host: "rasp".into(),
        port: None,
        argv: vec!["ssh".into(), "rasp".into()],
        config: String::new(),
        key: key.into(),
    }
}

/// The world around the service, as a test sets it.
struct Fake {
    bridge: Arc<Mutex<Bridge>>,
    /// What was asked, in order.
    log: Mutex<Vec<String>>,
    pane: AtomicBool,
    /// What the pane lookup says of the pane when there is one.
    info: Mutex<PaneInfo>,
    kablo: AtomicBool,
    /// The program the master step runs on the pane, if any.
    master: Mutex<Option<Vec<String>>>,
    fresh: AtomicBool,
    /// The server cannot be renewed now ([`Backend::refresh`] has no answer).
    refresh_fails: AtomicBool,
    /// Bridges that fail to start before one does.
    carry_fails: AtomicU32,
    /// The bridges started, each with the flag that ends it.
    carried: Mutex<Vec<Arc<AtomicBool>>>,
    /// While set, no bridge can be started: the network is down (`ConnectionRefused`),
    /// or the server wants a login nobody has saved (`PermissionDenied`).
    carry_error: Mutex<Option<io::ErrorKind>>,
    /// Why a bridge's stream ends by itself, as its process says.
    eof_cause: Arc<Mutex<Cause>>,
    /// What the panes were told about their connection, in order.
    links: Mutex<Vec<(PaneUuid, PaneLink)>>,
    /// The panes that were told the terminal they show came or went, in order.
    changed: Mutex<Vec<PaneUuid>>,
    /// The panes that were told the program in front of their terminal is another one as far as
    /// an icon tells, in order.
    fronted: Mutex<Vec<PaneUuid>>,
    /// The accounts told their terminals in the background are not what they were, in order.
    background: Mutex<Vec<String>>,
    /// The accounts told their server could be read, with the host, in order.
    reached: Mutex<Vec<(String, String)>>,
    /// The application takes what the programs on a server say to it.
    mail: AtomicBool,
    /// What the programs said, not yet answered.
    letters: Mutex<Vec<Letter>>,
    /// The number the server's counter gives next; 0 is a server that cannot count.
    next_number: AtomicU64,
    /// Who opened what, as the server's records say.
    records: Mutex<Vec<(crate::agent::Terminal, crate::agent::notes::Record)>>,
    /// How long reading the records takes.
    records_take: Mutex<Duration>,
}

impl Fake {
    fn new(bridge: Bridge) -> Arc<Fake> {
        Arc::new(Fake {
            bridge: Arc::new(Mutex::new(bridge)),
            log: Mutex::new(Vec::new()),
            pane: AtomicBool::new(true),
            info: Mutex::new(PaneInfo::default()),
            kablo: AtomicBool::new(true),
            master: Mutex::new(None),
            fresh: AtomicBool::new(false),
            refresh_fails: AtomicBool::new(false),
            carry_fails: AtomicU32::new(0),
            carried: Mutex::new(Vec::new()),
            carry_error: Mutex::new(None),
            eof_cause: Arc::new(Mutex::new(Cause::Dropped)),
            links: Mutex::new(Vec::new()),
            changed: Mutex::new(Vec::new()),
            fronted: Mutex::new(Vec::new()),
            background: Mutex::new(Vec::new()),
            reached: Mutex::new(Vec::new()),
            mail: AtomicBool::new(false),
            letters: Mutex::new(Vec::new()),
            next_number: AtomicU64::new(0),
            records: Mutex::new(Vec::new()),
            records_take: Mutex::new(Duration::ZERO),
        })
    }

    /// The server's counter works and gives `n` next.
    fn counts_from(&self, n: u64) {
        self.next_number.store(n, Ordering::SeqCst);
    }

    /// The network goes (`Some`) or comes back (`None`).
    fn carry_fails_with(&self, kind: Option<io::ErrorKind>) {
        *self.carry_error.lock().expect("lock") = kind;
    }

    fn links(&self) -> Vec<(PaneUuid, PaneLink)> {
        self.links.lock().expect("lock").clone()
    }

    fn changed(&self) -> Vec<PaneUuid> {
        self.changed.lock().expect("lock").clone()
    }

    fn fronted(&self) -> usize {
        self.fronted.lock().expect("lock").len()
    }

    /// The next letter a program said to the application, once there is one.
    fn letter(&self) -> Letter {
        let mut found = None;
        until("a letter", || {
            let mut letters = self.letters.lock().expect("lock");
            if letters.is_empty() {
                return false;
            }
            found = Some(letters.remove(0));
            true
        });
        found.expect("a letter")
    }

    fn background_told(&self) -> Vec<String> {
        self.background.lock().expect("lock").clone()
    }

    fn note(&self, what: &str) {
        self.log.lock().expect("lock").push(what.to_owned());
    }

    fn log(&self) -> Vec<String> {
        self.log.lock().expect("lock").clone()
    }

    fn count(&self, what: &str) -> usize {
        self.log()
            .iter()
            .filter(|entry| entry.as_str() == what)
            .count()
    }

    /// The connection to the server breaks: the bridge's stream ends.
    fn break_connection(&self) {
        if let Some(closed) = self.carried.lock().expect("lock").last() {
            closed.store(true, Ordering::SeqCst);
        }
    }

    fn bridge(&self) -> std::sync::MutexGuard<'_, Bridge> {
        self.bridge.lock().expect("lock")
    }
}

struct FromBridge {
    bridge: Arc<Mutex<Bridge>>,
    closed: Arc<AtomicBool>,
    pending: Vec<u8>,
}

impl Read for FromBridge {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        loop {
            if self.closed.load(Ordering::SeqCst) {
                return Ok(0);
            }
            if self.pending.is_empty() {
                self.pending = self.bridge.lock().expect("lock").take_output();
            }
            if !self.pending.is_empty() {
                let n = self.pending.len().min(buffer.len());
                buffer[..n].copy_from_slice(&self.pending[..n]);
                self.pending.drain(..n);
                return Ok(n);
            }
            thread::sleep(Duration::from_millis(2));
        }
    }
}

struct ToBridge(Arc<Mutex<Bridge>>);

impl Write for ToBridge {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.lock().expect("lock").feed(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Backend for Fake {
    fn pane(&self, _: &Peer) -> Option<PaneInfo> {
        self.note("pane");
        self.pane
            .load(Ordering::SeqCst)
            .then(|| self.info.lock().expect("lock").clone())
    }

    fn route(&self, _: &[String]) -> Option<KabloHost> {
        self.note("route");
        self.kablo.load(Ordering::SeqCst).then(|| host("u@rasp:22"))
    }

    fn master(&self, _: &KabloHost) -> Result<Option<Vec<Vec<u8>>>, Refusal> {
        self.note("master");
        Ok(self
            .master
            .lock()
            .expect("lock")
            .clone()
            .map(|argv| argv.into_iter().map(String::into_bytes).collect()))
    }

    fn master_made(&self, _: &KabloHost, status: u8) -> Result<(), Refusal> {
        self.note(&format!("master_made:{status}"));
        match status {
            0 => Ok(()),
            255 => Err(Refusal::ended("")),
            _ => Err(Refusal::plain()),
        }
    }

    fn install(&self, _: &KabloHost) -> Result<Installed, Refusal> {
        self.note("install");
        Ok(Installed {
            target: kablo_proto::install::Target::LinuxX86_64,
            version: "0.2.0".into(),
            path: ".local/share/kablo/0.2.0/kablo".into(),
            fresh: self.fresh.load(Ordering::SeqCst),
        })
    }

    fn version(&self) -> Option<String> {
        Some("0.2.0".into())
    }

    /// What the real one does to a server: its relays are replaced by the ones of the
    /// release this computer brings.
    fn refresh(&self, _: &KabloHost) -> Option<Installed> {
        self.note("refresh");
        if self.refresh_fails.load(Ordering::SeqCst) {
            return None;
        }
        self.bridge().replace_relays(Age::Current);
        Some(Installed {
            target: kablo_proto::install::Target::LinuxX86_64,
            version: "0.2.0".into(),
            path: ".local/share/kablo/0.2.0/kablo".into(),
            fresh: true,
        })
    }

    fn carry(&self, _: &KabloHost, _: &Installed) -> io::Result<Carried> {
        self.note("carry");
        if let Some(kind) = *self.carry_error.lock().expect("lock") {
            return Err(io::Error::new(kind, "no bridge"));
        }
        if self
            .carry_fails
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
            .is_ok()
        {
            return Err(io::Error::new(io::ErrorKind::ConnectionRefused, "down"));
        }
        let closed = Arc::new(AtomicBool::new(false));
        // A new connection to the server: what the old one had in flight is lost.
        self.bridge().disconnect();
        self.carried.lock().expect("lock").push(Arc::clone(&closed));
        let ends = Arc::clone(&closed);
        Ok(Carried {
            reader: Box::new(FromBridge {
                bridge: Arc::clone(&self.bridge),
                closed,
                pending: Vec::new(),
            }),
            writer: Box::new(ToBridge(Arc::clone(&self.bridge))),
            close: Box::new(move || ends.store(true, Ordering::SeqCst)),
            ended: {
                let cause = Arc::clone(&self.eof_cause);
                Box::new(move || *cause.lock().expect("lock"))
            },
        })
    }

    fn name(&self) -> Option<String> {
        Some("MacBook".into())
    }

    fn link_changed(&self, pane: &PaneUuid, link: PaneLink) {
        self.links.lock().expect("lock").push((pane.clone(), link));
    }

    fn terminal_changed(&self, pane: &PaneUuid) {
        self.changed.lock().expect("lock").push(pane.clone());
    }

    fn program_changed(&self, pane: &PaneUuid) {
        self.fronted.lock().expect("lock").push(pane.clone());
    }

    fn background_changed(&self, account: &str) {
        self.background
            .lock()
            .expect("lock")
            .push(account.to_owned());
    }

    fn number(&self, _: &KabloHost) -> Option<u64> {
        self.note("number");
        match self.next_number.load(Ordering::SeqCst) {
            0 => None,
            _ => Some(self.next_number.fetch_add(1, Ordering::SeqCst)),
        }
    }

    fn takes_mail(&self) -> bool {
        self.mail.load(Ordering::SeqCst)
    }

    fn records(&self, _: &KabloHost) -> Vec<(crate::agent::Terminal, crate::agent::notes::Record)> {
        self.note("records");
        thread::sleep(*self.records_take.lock().expect("lock"));
        self.records.lock().expect("lock").clone()
    }

    fn reachable(&self, account: &str, server: &KabloHost) {
        self.reached
            .lock()
            .expect("lock")
            .push((account.to_owned(), server.host.clone()));
    }

    fn mail(&self, letter: Letter) {
        self.letters.lock().expect("lock").push(letter);
    }
}

/// A started service over a fake world.
struct Rig {
    dir: PathBuf,
    service: Service,
    world: Arc<Fake>,
}

impl Rig {
    fn new(name: &str) -> Rig {
        Rig::over(name, Fake::new(Bridge::new(Age::Current)))
    }

    fn over(name: &str, world: Arc<Fake>) -> Rig {
        Rig::graced(name, world, END_GRACE)
    }

    fn graced(name: &str, world: Arc<Fake>, grace: Duration) -> Rig {
        Rig::kept(name, world, Arc::new(Ledger::memory()), grace)
    }

    /// A service whose ledger is the test's.
    fn kept(name: &str, world: Arc<Fake>, ledger: Arc<Ledger>, grace: Duration) -> Rig {
        let dir = scratch(name);
        let service = Service::start_with(&dir, world.clone(), ledger, grace).expect("a service");
        Rig {
            dir,
            service,
            world,
        }
    }

    fn socket(&self) -> PathBuf {
        self.service.socket().to_owned()
    }

    /// The ids of the terminals the server has, once there are `n`.
    fn terminals(&self, n: usize) -> Vec<SessionId> {
        let mut found = Vec::new();
        until("the terminals on the server", || {
            found = ["b-1", "b-2", "b-3"]
                .iter()
                .filter_map(|name| {
                    let id = SessionId::new(name).ok()?;
                    self.world.bridge().has(&id).then_some(id)
                })
                .collect();
            found.len() >= n
        });
        found
    }
}

impl Rig {
    /// Whether the server has a terminal of each name, once it has all of them.
    fn has_terminals(&self, names: &[&str]) {
        until("the terminals on the server", || {
            names
                .iter()
                .all(|name| SessionId::new(name).is_ok_and(|id| self.world.bridge().has(&id)))
        });
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        self.service.stop();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

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

struct Capture(Arc<Mutex<Vec<u8>>>);

impl Capture {
    fn of(read: OwnedFd) -> Capture {
        let bytes = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&bytes);
        thread::spawn(move || {
            let mut file = File::from(read);
            let mut buffer = [0u8; 65536];
            while let Ok(n) = file.read(&mut buffer) {
                if n == 0 {
                    break;
                }
                sink.lock().expect("lock").extend_from_slice(&buffer[..n]);
            }
        });
        Capture(bytes)
    }

    fn bytes(&self) -> Vec<u8> {
        self.0.lock().expect("lock").clone()
    }
}

/// A pane's `ssh-hold`: the pipe on pipes, in a thread.
struct Holder {
    handle: Option<JoinHandle<Outcome>>,
    typed: File,
    shown: Capture,
}

impl Holder {
    fn start(socket: &Path, terminal: &str, target: Target) -> Holder {
        let (stdin_read, stdin_write) = pipe();
        let (stdout_read, stdout_write) = pipe();
        let config = Config {
            terminal: SessionId::new(terminal).expect("an id"),
            target,
            first: false,
            close_at_end: false,
            sockets: vec![socket.to_path_buf()],
            input: stdin_read.as_raw_fd() as RawFd,
            output: stdout_write.as_raw_fd() as RawFd,
            backoff: vec![Duration::from_millis(20)],
            cancel: None,
        };
        let handle = thread::spawn(move || {
            let outcome = run(&config);
            drop((stdin_read, stdout_write));
            outcome
        });
        Holder {
            handle: Some(handle),
            typed: File::from(stdin_write),
            shown: Capture::of(stdout_read),
        }
    }

    fn new(socket: &Path, terminal: &str) -> Holder {
        Holder::start(
            socket,
            terminal,
            Target::New {
                args: vec![b"rasp".to_vec()],
            },
        )
    }

    fn outcome(&mut self) -> Outcome {
        until("the holder ends", || {
            self.handle.as_ref().is_some_and(JoinHandle::is_finished)
        });
        self.handle
            .take()
            .expect("running")
            .join()
            .expect("the pipe did not panic")
    }
}

#[test]
fn a_first_terminal_is_prepared_in_order_and_its_bytes_and_keys_cross_the_service() {
    let rig = Rig::new("first");
    rig.world.fresh.store(true, Ordering::SeqCst);
    let mut holder = Holder::new(&rig.socket(), "b-1");
    let terminal = rig.terminals(1).remove(0);

    // Gate, call, master, install, number, bridge — and nothing a second time.
    assert_eq!(
        rig.world.log(),
        ["pane", "route", "master", "install", "number", "carry"]
    );

    let mut output = b"ready\r\n\x1b[31mred\x1b[0m".to_vec();
    output.extend(0..=255u8);
    rig.world.bridge().write(&terminal, &output);
    until("the output shown", || {
        holder.shown.bytes().ends_with(&output)
    });
    // The note, once, before anything the terminal said.
    let shown = String::from_utf8_lossy(&holder.shown.bytes()).into_owned();
    let note =
        "bateri: kablo is set up on rasp; if the connection drops, this terminal keeps running";
    assert!(
        shown.starts_with(&format!("\x1b[0m\x1b[2m{note}\x1b[0m\r\n")),
        "{shown:?}"
    );
    assert_eq!(shown.matches(note).count(), 1);

    holder.typed.write_all(b"ls -la\r").expect("typed");
    until("the keys typed", || {
        rig.world.bridge().typed(&terminal) == b"ls -la\r"
    });

    rig.world.bridge().exit(&terminal, Exit::Code(7));
    assert_eq!(holder.outcome(), Outcome::Exit(7));
}

/// The server numbers its terminals: a holder asks with a name of its own, the terminal is the
/// next number the server's counter gives, in the ledger and on the server under it, and the
/// holder is told. The first terminal of an account (a thread that installs) and the ones that
/// follow (a thread that only counts) both do it.
#[test]
fn a_terminal_is_started_under_the_number_the_servers_counter_gives() {
    let rig = Rig::new("numbered");
    rig.world.counts_from(7);
    let mut first = Holder::new(&rig.socket(), "b-1");
    rig.has_terminals(&["t7"]);
    let mut second = Holder::new(&rig.socket(), "b-2");
    rig.has_terminals(&["t7", "t8"]);
    for gone in ["b-1", "b-2"] {
        assert!(
            !rig.world
                .bridge()
                .has(&SessionId::new(gone).expect("a name")),
            "{gone}"
        );
    }
    // Gate, call, master, install, number — and for the second one only the number.
    assert_eq!(
        rig.world.log(),
        [
            "pane", "route", "master", "install", "number", "carry", "pane", "route", "number"
        ]
    );
    // Each holder shows its own terminal and types into it.
    let (one, two) = (
        SessionId::new("t7").expect("a name"),
        SessionId::new("t8").expect("a name"),
    );
    rig.world.bridge().write(&one, b"one");
    rig.world.bridge().write(&two, b"two");
    until("both shown", || {
        first.shown.bytes() == b"one" && second.shown.bytes() == b"two"
    });
    second.typed.write_all(b"x").expect("typed");
    until("the key at the second", || {
        rig.world.bridge().typed(&two) == b"x"
    });
    rig.world.bridge().exit(&one, Exit::Code(0));
    rig.world.bridge().exit(&two, Exit::Code(3));
    assert_eq!(first.outcome(), Outcome::Exit(0));
    assert_eq!(second.outcome(), Outcome::Exit(3));
}

/// A server that cannot count (a home it cannot write, a connection that broke) does not keep a
/// terminal from starting: it keeps the name its holder made, and is asked once per terminal.
#[test]
fn a_server_that_cannot_count_leaves_the_terminal_its_holders_name() {
    let rig = Rig::new("uncounted");
    let mut holder = Holder::new(&rig.socket(), "b-1");
    let terminal = rig.terminals(1).remove(0);
    assert_eq!(terminal.as_str(), "b-1");
    assert_eq!(rig.world.count("number"), 1);
    rig.world.bridge().write(&terminal, b"ok");
    until("shown", || holder.shown.bytes() == b"ok");
    rig.world.bridge().exit(&terminal, Exit::Code(0));
    assert_eq!(holder.outcome(), Outcome::Exit(0));
}

/// The holder is told the number, so the pane that outlives the application asks for the
/// terminal by it: a restart finds `t7` where the holder first asked for `b-1`.
#[test]
fn a_numbered_terminal_is_found_again_by_its_number_after_a_restart() {
    let ledger = Arc::new(Ledger::memory());
    let world = Fake::new(Bridge::new(Age::Current));
    world.counts_from(7);
    let pane = pane_named(&world);
    let first = Rig::kept("number-restart", world.clone(), ledger.clone(), END_GRACE);
    let socket = first.socket();
    let holder = Holder::new(&socket, "b-1");
    first.has_terminals(&["t7"]);
    let terminal = SessionId::new("t7").expect("a name");
    world.bridge().write(&terminal, b"abc");
    until("shown", || holder.shown.bytes() == b"abc");
    until("the pane found", || first.service.shown(&pane).is_some());
    assert_eq!(
        first.service.shown(&pane).expect("shown").terminal,
        terminal
    );
    assert!(ledger.keeps("u@rasp:22", &terminal));
    assert!(!ledger.keeps("u@rasp:22", &SessionId::new("b-1").expect("a name")));

    first.service.stop();
    world.bridge().write(&terminal, b"def");
    let second = Service::start_with(&first.dir, world.clone(), ledger.clone(), END_GRACE)
        .expect("a second service");
    world.bridge().write(&terminal, b"ghi");
    until("the rest, once", || holder.shown.bytes() == b"abcdefghi");
    until("the pane found again", || second.shown(&pane).is_some());
    assert_eq!(second.shown(&pane).expect("shown").terminal, terminal);
    second.stop();
}

/// A number a person took by hand (`kablo new --id t7`) is skipped: the counter is asked again,
/// the terminal starts under the next number, the holder is shown it and is not let fall back to
/// a plain `ssh`, and the row written for the number that was taken goes — that terminal is not
/// the pane's to end.
#[test]
fn a_number_a_person_took_by_hand_is_skipped_and_the_terminal_starts_under_the_next() {
    let ledger = Arc::new(Ledger::memory());
    let world = Fake::new(Bridge::new(Age::Current));
    world.counts_from(7);
    let pane = pane_named(&world);
    let hand_made = world.bridge().add("t7", Size::default());
    let rig = Rig::kept("renumber", world, ledger.clone(), END_GRACE);
    let mut holder = Holder::new(&rig.socket(), "b-1");
    rig.has_terminals(&["t7", "t8"]);
    let next = SessionId::new("t8").expect("a name");
    rig.world.bridge().write(&next, b"mine");
    // The terminal the person made is a line the holder reads first (it runs in the background);
    // what follows is the new terminal's.
    until("the holder shows the next number", || {
        holder.shown.bytes().ends_with(b"mine")
    });
    assert_eq!(rig.world.count("number"), 2, "asked again");
    until("the pane found", || rig.service.shown(&pane).is_some());
    assert_eq!(rig.service.shown(&pane).expect("shown").terminal, next);
    assert!(ledger.keeps("u@rasp:22", &next));
    assert!(
        !ledger.keeps("u@rasp:22", &hand_made),
        "the person's terminal is not the pane's"
    );
    rig.world.bridge().exit(&next, Exit::Code(0));
    assert_eq!(holder.outcome(), Outcome::Exit(0));
}

/// A server whose numbers are all taken is given up on after a few tries: the pane is the plain
/// `ssh`'s, as it is for a server that cannot count.
#[test]
fn numbers_that_are_all_taken_are_given_up_on_and_the_pane_falls_back() {
    let world = Fake::new(Bridge::new(Age::Current));
    world.counts_from(7);
    for number in 7..20 {
        world.bridge().add(&format!("t{number}"), Size::default());
    }
    let rig = Rig::over("renumber-all", world);
    let mut holder = Holder::new(&rig.socket(), "b-1");
    assert_eq!(holder.outcome(), Outcome::FallBack);
    assert_eq!(rig.world.count("number"), usize::from(RENUMBER_TRIES) + 1);
}

/// A holder of version 2 is not told a name, so it is not numbered: the server's counter is not
/// even asked, and the terminal is the one it made.
#[test]
fn a_holder_of_the_version_before_is_not_numbered() {
    use crate::hold::wire::{HoldReader, Open, ToApp, ToHold};
    let rig = Rig::new("numbered-old");
    rig.world.counts_from(7);
    let mut stream = std::os::unix::net::UnixStream::connect(rig.socket()).expect("connected");
    let say = |stream: &mut std::os::unix::net::UnixStream, message: &ToApp| {
        let mut bytes = Vec::new();
        message.encode(&mut bytes);
        stream.write_all(&bytes).expect("written");
    };
    say(
        &mut stream,
        &ToApp::Hello {
            newest: 2,
            oldest: 1,
        },
    );
    say(
        &mut stream,
        &ToApp::Open(Open {
            terminal: SessionId::new("b-1").expect("a name"),
            size: kablo_proto::v1::Size {
                cols: 80,
                rows: 24,
                xpixel: 0,
                ypixel: 0,
            },
            target: Target::New {
                args: vec![b"rasp".to_vec()],
            },
        }),
    );
    rig.has_terminals(&["b-1"]);
    assert_eq!(rig.world.count("number"), 0);
    assert!(
        !rig.world
            .bridge()
            .has(&SessionId::new("t7").expect("a name"))
    );
    // What it is sent is what version 2 always read.
    stream
        .set_read_timeout(Some(Duration::from_secs(15)))
        .expect("a timeout");
    let mut reader = HoldReader::new();
    let mut heard = Vec::new();
    let mut buffer = [0u8; 4096];
    while !heard.iter().any(|m| matches!(m, ToHold::Bound { .. })) {
        let n = stream.read(&mut buffer).expect("read");
        assert!(
            n > 0,
            "the connection closed before it was bound: {heard:?}"
        );
        reader.push(&buffer[..n]);
        while let Some(message) = reader.read().expect("whole frames") {
            heard.push(message);
        }
    }
    assert_eq!(heard[0], ToHold::Welcome { version: 2 });
    assert!(heard.contains(&ToHold::Bound {
        account: "u@rasp:22".into(),
        terminal: None,
    }));
}

#[test]
fn the_master_step_runs_on_the_pane_and_its_ending_decides() {
    // It ends well: the terminal follows.
    let rig = Rig::new("master-ok");
    *rig.world.master.lock().expect("lock") =
        Some(vec!["/bin/sh".into(), "-c".into(), "exit 0".into()]);
    let mut holder = Holder::new(&rig.socket(), "b-1");
    rig.terminals(1);
    assert_eq!(
        rig.world.log(),
        [
            "pane",
            "route",
            "master",
            "master_made:0",
            "install",
            "number",
            "carry"
        ]
    );
    rig.world
        .bridge()
        .exit(&SessionId::new("b-1").expect("id"), Exit::Code(0));
    assert_eq!(holder.outcome(), Outcome::Exit(0));
    drop(rig);

    // ssh's own failure (a refused password, an unreachable host) ends the
    // pane's command the way ssh would have, with no second ssh behind it.
    let rig = Rig::new("master-255");
    *rig.world.master.lock().expect("lock") =
        Some(vec!["/bin/sh".into(), "-c".into(), "exit 255".into()]);
    let mut holder = Holder::new(&rig.socket(), "b-1");
    assert_eq!(holder.outcome(), Outcome::Exit(crate::hold::EXIT_REFUSED));
    assert_eq!(rig.world.count("install"), 0);
    assert_eq!(rig.world.count("carry"), 0);
    drop(rig);

    // Any other ending is the plain ssh's to try.
    let rig = Rig::new("master-1");
    *rig.world.master.lock().expect("lock") =
        Some(vec!["/bin/sh".into(), "-c".into(), "exit 1".into()]);
    let mut holder = Holder::new(&rig.socket(), "b-1");
    assert_eq!(holder.outcome(), Outcome::FallBack);
    assert_eq!(rig.world.count("install"), 0);
}

#[test]
fn what_is_not_in_a_pane_or_not_a_kablo_call_is_the_plain_ssh() {
    let rig = Rig::new("plain");
    rig.world.pane.store(false, Ordering::SeqCst);
    let mut holder = Holder::new(&rig.socket(), "b-1");
    assert_eq!(holder.outcome(), Outcome::FallBack);
    assert_eq!(
        rig.world.log(),
        ["pane"],
        "nothing else is asked of a stranger"
    );

    rig.world.pane.store(true, Ordering::SeqCst);
    rig.world.kablo.store(false, Ordering::SeqCst);
    let mut holder = Holder::new(&rig.socket(), "b-2");
    assert_eq!(holder.outcome(), Outcome::FallBack);
    assert_eq!(rig.world.count("master"), 0);
    assert_eq!(rig.world.count("carry"), 0);

    // The service is not there at all: the same.
    let socket = rig.socket();
    drop(rig);
    let mut holder = Holder::new(&socket, "b-3");
    assert_eq!(holder.outcome(), Outcome::FallBack);
}

#[test]
fn the_next_terminal_of_an_account_rides_the_bridge_that_is_there() {
    let rig = Rig::new("second");
    let mut first = Holder::new(&rig.socket(), "b-1");
    rig.terminals(1);
    let mut second = Holder::new(&rig.socket(), "b-2");
    let terminals = rig.terminals(2);
    assert_eq!(rig.world.count("master"), 1, "no second question");
    assert_eq!(rig.world.count("install"), 1);
    assert_eq!(rig.world.count("carry"), 1, "one bridge for the account");
    assert_eq!(rig.world.count("route"), 2, "each call is still decided");

    // Each holder shows its own terminal and types into it.
    rig.world.bridge().write(&terminals[0], b"one");
    rig.world.bridge().write(&terminals[1], b"two");
    until("both shown", || {
        first.shown.bytes() == b"one" && second.shown.bytes() == b"two"
    });
    second.typed.write_all(b"x").expect("typed");
    until("the key at the second", || {
        rig.world.bridge().typed(&terminals[1]) == b"x"
    });
    assert!(rig.world.bridge().typed(&terminals[0]).is_empty());

    rig.world.bridge().exit(&terminals[0], Exit::Code(0));
    rig.world.bridge().exit(&terminals[1], Exit::Code(3));
    assert_eq!(first.outcome(), Outcome::Exit(0));
    assert_eq!(second.outcome(), Outcome::Exit(3));
}

#[test]
fn a_broken_bridge_is_made_again_and_the_terminal_goes_on_with_no_byte_twice() {
    let rig = Rig::new("broken");
    let mut holder = Holder::new(&rig.socket(), "b-1");
    let terminal = rig.terminals(1).remove(0);
    rig.world.bridge().write(&terminal, b"one");
    until("one shown", || holder.shown.bytes() == b"one");

    rig.world.break_connection();
    // Written while nobody is attached, kept by the server.
    rig.world.bridge().write(&terminal, b"two");
    until("the bridge is made again", || rig.world.count("carry") == 2);
    until("the missed bytes", || holder.shown.bytes() == b"onetwo");
    rig.world.bridge().write(&terminal, b"three");
    until("the rest", || holder.shown.bytes() == b"onetwothree");

    rig.world.bridge().exit(&terminal, Exit::Code(0));
    assert_eq!(holder.outcome(), Outcome::Exit(0));
}

#[test]
fn a_bridge_that_cannot_be_started_is_tried_again_and_a_server_that_speaks_no_version_is_given_up()
{
    // The first attempts fail: the terminal waits for the one that works.
    let rig = Rig::new("retry");
    rig.world.carry_fails.store(1, Ordering::SeqCst);
    let mut holder = Holder::new(&rig.socket(), "b-1");
    let terminal = rig.terminals(1).remove(0);
    assert_eq!(rig.world.count("carry"), 2);
    rig.world.bridge().exit(&terminal, Exit::Code(0));
    assert_eq!(holder.outcome(), Outcome::Exit(0));
    drop(rig);

    // A server whose kablo speaks no version this build speaks: before anything
    // was shown, the plain ssh.
    let world = Fake::new(Bridge::new(Age::Current).speaking(8, 9));
    let rig = Rig::over("incompatible", world);
    let mut holder = Holder::new(&rig.socket(), "b-1");
    assert_eq!(holder.outcome(), Outcome::FallBack);
}

#[test]
fn a_holder_asking_for_a_terminal_again_gets_it_and_an_unknown_account_is_refused() {
    let rig = Rig::new("resume");
    let mut first = Holder::new(&rig.socket(), "b-1");
    let terminal = rig.terminals(1).remove(0);
    rig.world.bridge().write(&terminal, b"abc");
    until("shown", || first.shown.bytes() == b"abc");

    // The pane's program asks again, from where it was (its connection to the
    // application was lost and made again): the newer holder shows the terminal
    // and the older one is ended.
    let mut again = Holder::start(
        &rig.socket(),
        "b-1",
        Target::Resume {
            account: "u@rasp:22".into(),
            offset: Some(3),
        },
    );
    rig.world.bridge().write(&terminal, b"def");
    until("shown again", || again.shown.bytes() == b"def");
    assert_eq!(first.outcome(), Outcome::Exit(1));

    // An account bateri has no client for (it was restarted) cannot be served.
    let mut lost = Holder::start(
        &rig.socket(),
        "b-9",
        Target::Resume {
            account: "u@elsewhere:22".into(),
            offset: Some(0),
        },
    );
    assert_eq!(lost.outcome(), Outcome::Exit(crate::hold::EXIT_REFUSED));
    rig.world.bridge().exit(&terminal, Exit::Code(0));
    assert_eq!(again.outcome(), Outcome::Exit(0));
}

#[test]
fn a_terminal_is_taken_up_again_only_by_the_pane_that_showed_it() {
    let rig = Rig::new("owner");
    let pane = pane_named(&rig.world);
    let first = Holder::new(&rig.socket(), "b-1");
    let terminal = rig.terminals(1).remove(0);
    rig.world.bridge().write(&terminal, b"abc");
    until("shown", || first.shown.bytes() == b"abc");

    // A process of another pane says the names of this terminal and its account
    // (they are in the process list): it is refused, and the pane that has the
    // terminal still has it.
    let other = PaneUuid::parse("1a2b3c4d-5e6f-4a1b-8c9d-0e1f2a3b4c5d").expect("a canonical uuid");
    rig.world.info.lock().expect("lock").tab = Some(other.clone());
    let mut taker = Holder::start(
        &rig.socket(),
        "b-1",
        Target::Resume {
            account: "u@rasp:22".into(),
            offset: Some(3),
        },
    );
    assert_eq!(taker.outcome(), Outcome::Exit(crate::hold::EXIT_REFUSED));
    assert!(rig.service.shown(&other).is_none());
    assert_eq!(
        rig.service.shown(&pane).map(|shown| shown.terminal),
        Some(terminal.clone())
    );
    rig.world.bridge().write(&terminal, b"def");
    until("the pane goes on", || first.shown.bytes() == b"abcdef");

    // The pane itself asking again (its connection to the application was made
    // again) is served.
    rig.world.info.lock().expect("lock").tab = Some(pane.clone());
    let mut again = Holder::start(
        &rig.socket(),
        "b-1",
        Target::Resume {
            account: "u@rasp:22".into(),
            offset: Some(6),
        },
    );
    rig.world.bridge().write(&terminal, b"ghi");
    until("shown again", || again.shown.bytes() == b"ghi");
    rig.world.bridge().exit(&terminal, Exit::Code(0));
    assert_eq!(again.outcome(), Outcome::Exit(0));
}

/// A terminal the ledger names a pane for is that pane's alone; once the ledger lets go of it
/// (the application does when a person picks it from the list of those nobody shows — the pane
/// the row names may be one that is gone) another pane takes it up.
#[test]
fn a_terminal_the_ledger_let_go_of_is_taken_up_by_another_pane() {
    let ledger = Arc::new(Ledger::memory());
    let rig = Rig::kept(
        "released",
        Fake::new(Bridge::new(Age::Current)),
        Arc::clone(&ledger),
        END_GRACE,
    );
    let _first = Holder::new(&rig.socket(), "b-1");
    rig.terminals(1);
    let agent = rig.world.bridge().add("agent-1", Size::default());
    let gone = PaneUuid::parse("1a2b3c4d-5e6f-4a1b-8c9d-0e1f2a3b4c5d").expect("a canonical uuid");
    ledger.shown(ACCOUNT, &agent, Some(&gone));
    let wants = || Target::Resume {
        account: ACCOUNT.into(),
        offset: Some(0),
    };

    // A pane of another identity is refused while the ledger names the first.
    rig.world.info.lock().expect("lock").tab = Some(pane_named(&rig.world));
    let mut refused = Holder::start(&rig.socket(), "agent-1", wants());
    assert_eq!(refused.outcome(), Outcome::Exit(crate::hold::EXIT_REFUSED));

    ledger.gone(ACCOUNT, &agent);
    let taker = Holder::start(&rig.socket(), "agent-1", wants());
    rig.world.bridge().write(&agent, b"mine");
    until("the pane that took it is shown it", || {
        String::from_utf8_lossy(&taker.shown.bytes()).contains("mine")
    });
    assert!(
        ledger.pane_of(ACCOUNT, &agent).is_some(),
        "and the ledger names the pane that shows it now"
    );
}

#[test]
fn the_socket_is_the_users_and_goes_with_the_service() {
    let rig = Rig::new("socket");
    let socket = rig.socket();
    let mode = std::fs::metadata(&socket).expect("the socket");
    assert!(std::os::unix::fs::FileTypeExt::is_socket(&mode.file_type()));
    assert!(UnixStream::connect(&socket).is_ok());
    // A directory that is not private is not bound in.
    let open = PathBuf::from(format!("/tmp/bt-svc-open-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&open);
    std::fs::DirBuilder::new()
        .mode(0o755)
        .create(&open)
        .expect("dir");
    assert!(
        Service::start(
            &open,
            Fake::new(Bridge::new(Age::Current)),
            Arc::new(Ledger::memory())
        )
        .is_err()
    );
    let _ = std::fs::remove_dir_all(&open);
    rig.service.stop();
    assert!(!socket.exists());
}

/// A pane's identity for the lookup to answer with.
fn pane_named(world: &Fake) -> PaneUuid {
    let pane = PaneUuid::parse("0f3c1a52-7d0e-4c1b-9b57-2f7a8c4d6e10").expect("a canonical uuid");
    world.info.lock().expect("lock").tab = Some(pane.clone());
    pane
}

fn saying(program: &str) -> kablo_proto::v1::TerminalState {
    let mut state = kablo_proto::v1::TerminalState::default();
    state.program = Some(program.as_bytes().to_vec());
    state
}

#[test]
fn a_pane_is_found_by_the_terminal_it_shows_with_the_program_in_front() {
    let rig = Rig::new("shown");
    let pane = pane_named(&rig.world);
    assert_eq!(rig.service.shown(&pane), None, "nothing is shown yet");
    let mut holder = Holder::new(&rig.socket(), "b-1");
    let terminal = rig.terminals(1).remove(0);
    until("the pane found", || rig.service.shown(&pane).is_some());
    let shown = rig.service.shown(&pane).expect("shown");
    assert_eq!(shown.account, "u@rasp:22");
    assert_eq!(shown.host, "rasp");
    assert_eq!(shown.terminal, terminal);
    assert_eq!(shown.running(), None, "no word from the server yet");

    // A program in front, named with the server it runs on.
    rig.world.bridge().set_state(&terminal, saying("npm"));
    until("the program in front", || {
        rig.service
            .shown(&pane)
            .is_some_and(|shown| shown.running().as_deref() == Some("npm on rasp"))
    });
    // The shell back in front (a login shell is named with a dash).
    rig.world.bridge().set_state(&terminal, saying("-zsh"));
    until("the shell in front", || {
        rig.service.shown(&pane).is_some_and(|shown| {
            shown.program.as_deref() == Some("-zsh") && shown.running().is_none()
        })
    });
    // A pane the service never heard of shows nothing.
    let other = PaneUuid::parse("5a7e9c40-1b2d-4e3f-8a6b-0c9d1e2f3a4b").expect("a uuid");
    assert_eq!(rig.service.shown(&other), None);

    // The terminal ending lets go of the pane.
    rig.world.bridge().exit(&terminal, Exit::Code(0));
    assert_eq!(holder.outcome(), Outcome::Exit(0));
    until("the pane let go of", || rig.service.shown(&pane).is_none());
}

/// The pane's icon is the kind of the program in front, so it is told when the **kind** is
/// another, not when the name is: the server of an older kablo says `2.1.296` for Claude Code, a
/// newer one `claude` (both are in the world), and neither is a reason to redraw when it is the
/// same nothing — and the terminal that appears with a program already in front, or goes, is one.
#[test]
fn a_pane_is_told_when_the_kind_of_the_program_in_front_changes_and_not_its_name() {
    let rig = Rig::new("fronted");
    let pane = pane_named(&rig.world);
    let mut holder = Holder::new(&rig.socket(), "b-1");
    let terminal = rig.terminals(1).remove(0);
    until("the pane found", || rig.service.shown(&pane).is_some());
    assert_eq!(rig.world.fronted(), 0, "the server has not said");

    // A program the table knows comes in front: the kind is another.
    rig.world.bridge().set_state(&terminal, saying("npm"));
    until("told of npm", || rig.world.fronted() == 1);
    assert_eq!(
        rig.service.shown(&pane).and_then(|shown| shown.kind()),
        Some(Kind::Task)
    );
    // The program Claude Code is, by the name a newer kablo gives it.
    rig.world.bridge().set_state(&terminal, saying("claude"));
    until("told of claude", || rig.world.fronted() == 2);
    assert_eq!(
        rig.service.shown(&pane).and_then(|shown| shown.kind()),
        Some(Kind::Agent(Agent::ClaudeCode))
    );
    // The file's version number is what an older kablo says: no kind, so the server's icon
    // returns — told once, and the next such name is not news.
    rig.world.bridge().set_state(&terminal, saying("2.1.296"));
    until("told of the version", || rig.world.fronted() == 3);
    assert_eq!(rig.service.shown(&pane).and_then(|s| s.kind()), None);
    rig.world.bridge().set_state(&terminal, saying("node"));
    until("the name", || {
        rig.service
            .shown(&pane)
            .is_some_and(|shown| shown.program.as_deref() == Some("node"))
    });
    // Both are no kind: the second is not news, whatever the first was.
    std::thread::sleep(Duration::from_millis(150));
    assert_eq!(
        rig.world.fronted(),
        3,
        "a name no one knows for one is no news"
    );
    rig.world.bridge().set_state(&terminal, saying("vim"));
    until("told of vim", || rig.world.fronted() == 4);
    rig.world.bridge().set_state(&terminal, saying("-zsh"));
    // The shell back in front is the server's icon as well.
    until("told of the shell", || rig.world.fronted() == 5);

    // A terminal that goes with a program in front is told too: the pane is the server's again.
    rig.world.bridge().set_state(&terminal, saying("htop"));
    until("told of htop", || rig.world.fronted() == 6);
    rig.world.bridge().exit(&terminal, Exit::Code(0));
    assert_eq!(holder.outcome(), Outcome::Exit(0));
    until("told it went", || rig.world.fronted() == 7);
    assert!(rig.service.shown(&pane).is_none());
}

#[test]
fn a_program_name_is_shown_without_the_controls_the_server_sent() {
    let rig = Rig::new("clean");
    let pane = pane_named(&rig.world);
    let _holder = Holder::new(&rig.socket(), "b-1");
    let terminal = rig.terminals(1).remove(0);
    rig.world
        .bridge()
        .set_state(&terminal, saying("vim\x1b[2J\n"));
    until("the name", || {
        rig.service
            .shown(&pane)
            .is_some_and(|shown| shown.program.is_some())
    });
    let shown = rig.service.shown(&pane).expect("shown");
    assert_eq!(shown.program.as_deref(), Some("vim[2J"));
}

#[test]
fn closing_a_pane_hangs_its_terminal_up_and_kills_it_when_it_stays() {
    let rig = Rig::graced(
        "end",
        Fake::new(Bridge::new(Age::Current).stubborn()),
        Duration::from_millis(300),
    );
    let pane = pane_named(&rig.world);
    let _holder = Holder::new(&rig.socket(), "b-1");
    let terminal = rig.terminals(1).remove(0);
    until("the pane found", || rig.service.shown(&pane).is_some());

    rig.service.end(&pane);
    until("the hang-up", || {
        rig.world.bridge().terminated(&terminal) == Some(false)
    });
    // Still there after the grace: the kill follows.
    until("the kill", || {
        rig.world.bridge().terminated(&terminal) == Some(true)
    });
}

#[test]
fn a_terminal_that_goes_by_itself_after_the_hang_up_is_not_killed() {
    let rig = Rig::graced(
        "end-went",
        Fake::new(Bridge::new(Age::Current)),
        Duration::from_millis(300),
    );
    let pane = pane_named(&rig.world);
    let mut holder = Holder::new(&rig.socket(), "b-1");
    let terminal = rig.terminals(1).remove(0);
    until("the pane found", || rig.service.shown(&pane).is_some());

    rig.service.end(&pane);
    until("the hang-up", || {
        rig.world.bridge().terminated(&terminal) == Some(false)
    });
    // The shell took the hang-up and went.
    rig.world.bridge().exit(&terminal, Exit::Code(129));
    assert_eq!(holder.outcome(), Outcome::Exit(129));
    thread::sleep(Duration::from_millis(900));
    assert_eq!(
        rig.world.bridge().terminated(&terminal),
        Some(false),
        "nothing was left to kill"
    );
}

#[test]
fn a_pane_that_shows_nothing_ends_nothing_and_a_pane_is_ended_once() {
    let rig = Rig::graced(
        "end-none",
        Fake::new(Bridge::new(Age::Current)),
        Duration::from_secs(60),
    );
    let pane = pane_named(&rig.world);
    let _holder = Holder::new(&rig.socket(), "b-1");
    let terminal = rig.terminals(1).remove(0);
    until("the pane found", || rig.service.shown(&pane).is_some());

    let other = PaneUuid::parse("5a7e9c40-1b2d-4e3f-8a6b-0c9d1e2f3a4b").expect("a uuid");
    rig.service.end(&other);
    // The loop has seen the first request before the second, and the fake
    // records only the last word about a terminal: ask twice, see one.
    rig.service.end(&pane);
    rig.service.end(&pane);
    until("the hang-up", || {
        rig.world.bridge().terminated(&terminal) == Some(false)
    });
    thread::sleep(Duration::from_millis(200));
    assert_eq!(rig.world.bridge().terminated(&terminal), Some(false));
}

/// Leave Running: the pane closes and the terminal is not asked to end — not at once, not after the
/// grace — and the ledger lets go of it, so that no quit and no launch after a crash ends it; with
/// the pane's holder gone it is in the background, and a pane that shows it again writes it back.
#[test]
fn a_terminal_left_running_is_not_ended_and_the_ledger_forgets_it() {
    let ledger = Arc::new(Ledger::memory());
    let rig = Rig::kept(
        "leave",
        Fake::new(Bridge::new(Age::Current)),
        ledger.clone(),
        Duration::from_millis(300),
    );
    let pane = pane_named(&rig.world);
    let holder = Holder::new(&rig.socket(), "b-1");
    let terminal = rig.terminals(1).remove(0);
    until("the pane found", || rig.service.shown(&pane).is_some());
    assert!(
        ledger.keeps(ACCOUNT, &terminal),
        "a shown terminal is in the ledger"
    );

    // Said before the pane's program is signalled; then the holder goes with the pane.
    rig.service.leave(&pane);
    until("the ledger lets go", || ledger.terminals().is_empty());
    let Holder { handle, typed, .. } = holder;
    drop(typed);
    until("in the background", || {
        background_names(&rig) == [terminal.as_str().to_owned()]
    });
    if let Some(handle) = handle {
        let _ = handle.join();
    }

    // Past the grace a hang-up would have been followed by a kill: nothing was asked at all.
    thread::sleep(Duration::from_millis(900));
    assert_eq!(rig.world.bridge().terminated(&terminal), None);
    assert!(rig.world.bridge().has(&terminal), "it runs on");
    assert!(ledger.terminals().is_empty(), "and no row names it");

    // Shown by a pane again, it is written back and leaves the background.
    let _again = Holder::start(
        &rig.socket(),
        terminal.as_str(),
        Target::Resume {
            account: ACCOUNT.to_owned(),
            offset: Some(0),
        },
    );
    until("written back", || ledger.keeps(ACCOUNT, &terminal));
    until("out of the background", || {
        rig.service.background(ACCOUNT).is_empty()
    });
}

/// Only the pane that was asked about: another pane's terminal keeps its row, and a pane that
/// shows nothing leaves nothing.
#[test]
fn leaving_a_pane_running_touches_no_other_terminal() {
    let ledger = Arc::new(Ledger::memory());
    let rig = Rig::kept(
        "leave-other",
        Fake::new(Bridge::new(Age::Current)),
        ledger.clone(),
        END_GRACE,
    );
    let pane = pane_named(&rig.world);
    let _holder = Holder::new(&rig.socket(), "b-1");
    let terminal = rig.terminals(1).remove(0);
    until("the pane found", || rig.service.shown(&pane).is_some());

    let other = PaneUuid::parse("5a7e9c40-1b2d-4e3f-8a6b-0c9d1e2f3a4b").expect("a uuid");
    rig.service.leave(&other);
    // The loop has seen the first request before this probe round trip.
    rig.service.refresh_background();
    thread::sleep(Duration::from_millis(200));
    assert!(ledger.keeps(ACCOUNT, &terminal));
    assert!(rig.service.shown(&pane).is_some());
}

#[test]
fn a_new_terminal_starts_in_the_folder_the_pane_lookup_names() {
    let rig = Rig::new("folder");
    pane_named(&rig.world);
    rig.world.info.lock().expect("lock").directory = Some("/srv/app".into());
    let _holder = Holder::new(&rig.socket(), "b-1");
    let terminal = rig.terminals(1).remove(0);
    let argv = rig
        .world
        .bridge()
        .argv_of(&terminal)
        .expect("started")
        .to_vec();
    // The bootstrap carries where kablo is on that server, for the agent's command.
    let kablo = Some(".local/share/kablo/0.2.0/kablo");
    assert_eq!(
        argv[2],
        ssh_wrap::boot_in(Some("/srv/app"), kablo).as_bytes(),
        "the bootstrap that goes to /srv/app"
    );
    assert_ne!(argv[2], ssh_wrap::boot_in(None, kablo).as_bytes());
}

/// The panes' connection: told once per change, from a first connection that
/// is not a lost one to a lost one that comes back by a clock the user skips.
#[test]
fn a_pane_is_told_when_its_connection_goes_and_comes_back_and_not_once_per_attempt() {
    let rig = Rig::new("passive");
    let pane = pane_named(&rig.world);
    // The first connection fails once before it works: the terminal is not
    // shown yet, the pane is waiting for its first connection and is not passive.
    rig.world.carry_fails.store(1, Ordering::SeqCst);
    let mut holder = Holder::new(&rig.socket(), "b-1");
    let terminal = rig.terminals(1).remove(0);
    rig.world.bridge().write(&terminal, b"one");
    until("one shown", || holder.shown.bytes() == b"one");
    until("the pane found live", || {
        rig.service
            .shown(&pane)
            .is_some_and(|shown| shown.link == PaneLink::Live)
    });
    assert_eq!(
        rig.world.links(),
        [],
        "a first connection is not a lost one"
    );
    // A wake from sleep and a ⏎ on a connection that is up change nothing about
    // how the pane looks: the pulse is answered and nobody is told.
    rig.service.wake();
    rig.service.retry(&pane);

    // The network goes with the connection.
    rig.world
        .carry_fails_with(Some(io::ErrorKind::ConnectionRefused));
    rig.world.break_connection();
    until("the pane told", || {
        rig.world.links() == [(pane.clone(), PaneLink::Reconnecting)]
    });
    assert_eq!(
        rig.service.shown(&pane).map(|shown| shown.link),
        Some(PaneLink::Reconnecting)
    );
    // The pauses (1 s, 2 s) go by with the attempts failing: no news in them.
    let attempts = rig.world.count("carry");
    until("two more attempts", || {
        rig.world.count("carry") >= attempts + 2
    });
    assert_eq!(rig.world.links().len(), 1, "an attempt is not news");

    // The network is back, and the user does not wait out the next pause (4 s).
    rig.world.carry_fails_with(None);
    let asked = Instant::now();
    rig.service.retry(&pane);
    until("the pane told it is back", || {
        rig.world.links().last() == Some(&(pane.clone(), PaneLink::Live))
    });
    assert!(
        asked.elapsed() < Duration::from_secs(3),
        "try now skips the pause: {:?}",
        asked.elapsed()
    );
    rig.world.bridge().write(&terminal, b"two");
    until("the bytes go on", || holder.shown.bytes() == b"onetwo");
    assert_eq!(rig.world.links().len(), 2);
    // What a program's view over the pane waits for: one return in all that, not
    // one for every attempt that failed on the way (the pane starts the view
    // again at each told return and at no other word).
    let mut stood = PaneLink::Live;
    let mut returns = 0;
    for (_, told) in rig.world.links() {
        returns += usize::from(stood.comes_back(told));
        stood = told;
    }
    assert_eq!(returns, 1, "the connection came back once");

    rig.world.bridge().exit(&terminal, Exit::Code(0));
    assert_eq!(holder.outcome(), Outcome::Exit(0));
}

/// A screen that is back is the one edge a program's view is started again at: from
/// every passive state to live, and from nothing else.
#[test]
fn a_screen_is_back_only_when_a_passive_one_goes_live() {
    use PaneLink::{Connecting, Live, NeedsLogin, Reconnecting, Taken};
    for passive in [Reconnecting, NeedsLogin, Taken] {
        assert!(passive.comes_back(Live), "{passive:?} to live");
        for other in [Reconnecting, NeedsLogin, Taken, Connecting] {
            assert!(!passive.comes_back(other), "{passive:?} to {other:?}");
        }
    }
    // The first connection was never lost, and live to live is no change.
    assert!(!Connecting.comes_back(Live));
    assert!(!Live.comes_back(Live));
    for to in [Reconnecting, NeedsLogin, Taken, Connecting] {
        assert!(!Live.comes_back(to), "live to {to:?}");
    }
}

/// The computer waking up is the same as the user asking, for every account.
#[test]
fn waking_dials_the_accounts_that_wait_for_their_pause() {
    let rig = Rig::new("wake");
    let pane = pane_named(&rig.world);
    let holder = Holder::new(&rig.socket(), "b-1");
    let terminal = rig.terminals(1).remove(0);
    rig.world.bridge().write(&terminal, b"one");
    until("one shown", || holder.shown.bytes() == b"one");

    rig.world
        .carry_fails_with(Some(io::ErrorKind::ConnectionRefused));
    rig.world.break_connection();
    until("the pane told", || {
        rig.world.links() == [(pane.clone(), PaneLink::Reconnecting)]
    });
    let attempts = rig.world.count("carry");
    until("two more attempts", || {
        rig.world.count("carry") >= attempts + 2
    });

    rig.world.carry_fails_with(None);
    let woke = Instant::now();
    rig.service.wake();
    until("the pane told it is back", || {
        rig.world.links().last() == Some(&(pane.clone(), PaneLink::Live))
    });
    assert!(
        woke.elapsed() < Duration::from_secs(3),
        "{:?}",
        woke.elapsed()
    );
}

/// A login nobody saved: nothing is tried until the user says so, and the user
/// saying so with the login still missing asks once and waits again.
#[test]
fn a_login_nobody_saved_waits_for_the_user_and_tries_nothing_meanwhile() {
    let rig = Rig::new("login");
    let pane = pane_named(&rig.world);
    let holder = Holder::new(&rig.socket(), "b-1");
    let terminal = rig.terminals(1).remove(0);
    rig.world.bridge().write(&terminal, b"one");
    until("one shown", || holder.shown.bytes() == b"one");

    rig.world
        .carry_fails_with(Some(io::ErrorKind::PermissionDenied));
    rig.world.break_connection();
    until("the pane told it needs a login", || {
        rig.world.links().last() == Some(&(pane.clone(), PaneLink::Reconnecting))
            || rig.world.links().last() == Some(&(pane.clone(), PaneLink::NeedsLogin))
    });
    until("it needs a login", || {
        rig.service.shown(&pane).map(|shown| shown.link) == Some(PaneLink::NeedsLogin)
    });
    let attempts = rig.world.count("carry");
    // No clock runs: the pauses of a network that is down would have tried again.
    thread::sleep(Duration::from_millis(2500));
    assert_eq!(
        rig.world.count("carry"),
        attempts,
        "nothing is tried meanwhile"
    );

    // ⏎ without the login in place: one try, and the wait again.
    rig.service.retry(&pane);
    until("the one try", || rig.world.count("carry") == attempts + 1);
    until("waiting again", || {
        rig.service.shown(&pane).map(|shown| shown.link) == Some(PaneLink::NeedsLogin)
    });
    thread::sleep(Duration::from_millis(1500));
    assert_eq!(rig.world.count("carry"), attempts + 1);

    // Logged in (the master is up): the account dials and the pane is live.
    rig.world.carry_fails_with(None);
    rig.service.retry_account("u@rasp:22");
    until("the pane told it is back", || {
        rig.world.links().last() == Some(&(pane.clone(), PaneLink::Live))
    });
    rig.world.bridge().write(&terminal, b"two");
    until("the bytes go on", || holder.shown.bytes() == b"onetwo");
}

/// The bridge's process says why it ended: a refused login is a wait for the
/// user, a key that is not the trusted one ends the terminal's pane for good.
#[test]
fn a_bridge_that_ends_for_its_login_or_its_key_says_so() {
    let rig = Rig::new("cause");
    let pane = pane_named(&rig.world);
    let holder = Holder::new(&rig.socket(), "b-1");
    let terminal = rig.terminals(1).remove(0);
    rig.world.bridge().write(&terminal, b"one");
    until("one shown", || holder.shown.bytes() == b"one");

    *rig.world.eof_cause.lock().expect("lock") = Cause::LoginNeeded;
    rig.world.break_connection();
    until("it needs a login", || {
        rig.service.shown(&pane).map(|shown| shown.link) == Some(PaneLink::NeedsLogin)
    });
    drop(rig);

    // The key: the pane's terminal is let go of with a line, and a pane that
    // was waiting is not left waiting.
    let rig = Rig::new("key");
    let pane = pane_named(&rig.world);
    let mut holder = Holder::new(&rig.socket(), "b-1");
    let terminal = rig.terminals(1).remove(0);
    rig.world.bridge().write(&terminal, b"one");
    until("one shown", || holder.shown.bytes() == b"one");
    *rig.world.eof_cause.lock().expect("lock") = Cause::Refused;
    rig.world
        .carry_fails_with(Some(io::ErrorKind::ConnectionRefused));
    rig.world.break_connection();
    assert!(matches!(holder.outcome(), Outcome::Exit(_)));
    until("the pane let go of", || rig.service.shown(&pane).is_none());
    let links = rig.world.links();
    assert!(
        links.last().is_none_or(|(_, link)| *link == PaneLink::Live),
        "{links:?}"
    );
}

/// A pane that goes while it waits is told it waits for nothing.
#[test]
fn a_pane_that_goes_while_it_waits_is_not_left_dimmed() {
    let rig = Rig::new("gone");
    let pane = pane_named(&rig.world);
    let holder = Holder::new(&rig.socket(), "b-1");
    let terminal = rig.terminals(1).remove(0);
    rig.world.bridge().write(&terminal, b"one");
    until("one shown", || holder.shown.bytes() == b"one");
    rig.world
        .carry_fails_with(Some(io::ErrorKind::ConnectionRefused));
    rig.world.break_connection();
    until("the pane told", || {
        rig.world.links() == [(pane.clone(), PaneLink::Reconnecting)]
    });
    // The pane's program goes (its terminal hangs up).
    let Holder { handle, typed, .. } = holder;
    drop(typed);
    until("the pane let go of", || rig.service.shown(&pane).is_none());
    until("told it waits for nothing", || {
        rig.world.links().last() == Some(&(pane.clone(), PaneLink::Live))
    });
    if let Some(handle) = handle {
        let _ = handle.join();
    }
}

// ─── what outlives the process ───────────────────────────────────────────

/// A terminal the first bateri made is shown again by the next one, which has nothing but the
/// ledger: no route, no master, no install — only the bridge — and the holder's pane is
/// found by the server's account as it always was.
#[test]
fn a_restarted_bateri_shows_the_terminal_the_one_before_made() {
    let ledger = Arc::new(Ledger::memory());
    let world = Fake::new(Bridge::new(Age::Current));
    let pane = pane_named(&world);
    let first = Rig::kept("again-1", world.clone(), ledger.clone(), END_GRACE);
    let holder = Holder::new(&first.socket(), "b-1");
    let terminal = first.terminals(1).remove(0);
    world.bridge().write(&terminal, b"abc");
    until("shown", || holder.shown.bytes() == b"abc");

    // Written down from the request: the account with the way to reach it, the terminal
    // with the pane.
    let known = ledger
        .known("u@rasp:22")
        .expect("the account is remembered");
    assert_eq!(known.argv, ["ssh", "rasp"]);
    assert_eq!(known.version, "0.2.0");
    assert!(ledger.keeps("u@rasp:22", &terminal));
    let shown = first.service.shown(&pane).expect("the pane shows it");
    assert_eq!(
        shown.argv,
        ["ssh", "rasp"],
        "what a restored pane's pipe carries"
    );

    // bateri goes (the holder is kept by its holder process and waits); another one
    // comes with nothing of the first but the ledger.
    first.service.stop();
    drop(first);
    let before = world.log().len();
    let second = Rig::kept("again-2", world.clone(), ledger.clone(), END_GRACE);
    let mut again = Holder::start(
        &second.socket(),
        "b-1",
        Target::Resume {
            account: "u@rasp:22".into(),
            offset: Some(3),
        },
    );
    world.bridge().write(&terminal, b"def");
    until("shown again, from where it was", || {
        again.shown.bytes() == b"def"
    });
    assert_eq!(
        world.log()[before..],
        ["pane", "carry"],
        "no route, no master, no install: the ledger knew the server"
    );
    assert!(second.service.shown(&pane).is_some());
    world.bridge().exit(&terminal, Exit::Code(0));
    assert_eq!(again.outcome(), Outcome::Exit(0));
    until("forgotten once it ended", || {
        ledger.known("u@rasp:22").is_none()
    });
}

/// An account the ledger remembers at another release of kablo than the one this
/// computer brings is renewed on the server before its bridge is made, and is known at the
/// new release from then on, in the ledger too — whatever the bridge then does.
fn the_old_release(ledger: &Ledger) {
    ledger.remember(
        &host("u@rasp:22"),
        &Installed {
            target: kablo_proto::install::Target::LinuxX86_64,
            version: "0.1.0".into(),
            path: ".local/share/kablo/0.1.0/kablo".into(),
            fresh: false,
        },
    );
}

#[test]
fn a_restarted_bateri_renews_the_kablo_its_ledger_remembers_before_it_dials() {
    let ledger = Arc::new(Ledger::memory());
    // The terminal runs under the relay of the release before.
    let world = Fake::new(Bridge::new(Age::Old));
    let first = Rig::kept("renew-1", world.clone(), ledger.clone(), END_GRACE);
    let holder = Holder::new(&first.socket(), "b-1");
    let terminal = first.terminals(1).remove(0);
    world.bridge().write(&terminal, b"abc");
    until("shown", || holder.shown.bytes() == b"abc");
    assert_eq!(
        world.count("refresh"),
        0,
        "an account made now is of today's release"
    );
    first.service.stop();
    drop(first);
    the_old_release(&ledger);

    let before = world.log().len();
    let second = Rig::kept("renew-2", world.clone(), ledger.clone(), END_GRACE);
    let again = Holder::start(
        &second.socket(),
        "b-1",
        Target::Resume {
            account: "u@rasp:22".into(),
            offset: Some(3),
        },
    );
    world.bridge().write(&terminal, b"def");
    until("shown again, from where it was", || {
        again.shown.bytes() == b"def"
    });
    assert_eq!(
        world.log()[before..],
        ["pane", "refresh", "carry"],
        "the server is renewed first and no bridge exists until then"
    );
    assert_eq!(world.bridge().age(), Age::Current);
    assert_eq!(
        ledger.known("u@rasp:22").expect("known").version,
        "0.2.0",
        "known at the release it is at"
    );

    // And once: a bridge made again does not renew a server that was.
    world.break_connection();
    until("the bridge is made again", || world.count("carry") == 3);
    assert_eq!(world.count("refresh"), 1);
}

/// A renewal that cannot be made now (no connection, a server that will not take the
/// file) does not stand in the bridge's way and is made at the next dial.
#[test]
fn a_renewal_that_cannot_be_made_leaves_the_account_as_it_was_until_the_next_dial() {
    let ledger = Arc::new(Ledger::memory());
    let world = Fake::new(Bridge::new(Age::Old));
    let first = Rig::kept("renew-fail-1", world.clone(), ledger.clone(), END_GRACE);
    let holder = Holder::new(&first.socket(), "b-1");
    let terminal = first.terminals(1).remove(0);
    world.bridge().write(&terminal, b"abc");
    until("shown", || holder.shown.bytes() == b"abc");
    first.service.stop();
    drop(first);
    the_old_release(&ledger);
    world.refresh_fails.store(true, Ordering::SeqCst);

    let before = world.log().len();
    let second = Rig::kept("renew-fail-2", world.clone(), ledger.clone(), END_GRACE);
    let again = Holder::start(
        &second.socket(),
        "b-1",
        Target::Resume {
            account: "u@rasp:22".into(),
            offset: Some(3),
        },
    );
    world.bridge().write(&terminal, b"def");
    until("shown, the relay of the release before serving", || {
        again.shown.bytes() == b"def"
    });
    assert_eq!(world.log()[before..], ["pane", "refresh", "carry"]);
    assert_eq!(world.bridge().age(), Age::Old, "nothing was renewed");
    assert_eq!(
        ledger.known("u@rasp:22").expect("known").version,
        "0.1.0",
        "and nothing is written down as if it was"
    );

    // The connection goes and the server can be reached: the dial that follows
    // renews it first, and the bytes go on with none twice.
    world.refresh_fails.store(false, Ordering::SeqCst);
    world.break_connection();
    until("renewed on the next dial", || world.count("refresh") == 2);
    until("and the bridge made", || world.count("carry") == 3);
    world.bridge().write(&terminal, b"ghi");
    until("the bytes go on", || again.shown.bytes() == b"defghi");
    assert_eq!(world.bridge().age(), Age::Current);
    until("known at the new release", || {
        ledger
            .known("u@rasp:22")
            .is_some_and(|known| known.version == "0.2.0")
    });
}

/// A holder that was shown a terminal bateri has since been told to end is not shown it
/// again, and an account the ledger never heard of is refused as before.
#[test]
fn a_terminal_marked_to_end_is_not_shown_again() {
    let ledger = Arc::new(Ledger::memory());
    ledger.remember(
        &host("u@rasp:22"),
        &Installed {
            target: kablo_proto::install::Target::LinuxX86_64,
            version: "0.2.0".into(),
            path: ".local/share/kablo/0.2.0/kablo".into(),
            fresh: false,
        },
    );
    let terminal = SessionId::new("b-1").expect("an id");
    ledger.shown("u@rasp:22", &terminal, None);
    ledger.end("u@rasp:22", &terminal);
    let rig = Rig::kept(
        "marked",
        Fake::new(Bridge::new(Age::Current)),
        ledger,
        END_GRACE,
    );
    let mut holder = Holder::start(
        &rig.socket(),
        "b-1",
        Target::Resume {
            account: "u@rasp:22".into(),
            offset: Some(0),
        },
    );
    assert_eq!(holder.outcome(), Outcome::Exit(crate::hold::EXIT_REFUSED));
    assert!(
        rig.world.log().iter().all(|entry| entry != "carry"),
        "nothing was dialed for it"
    );
}

/// A quit that ends the terminals: asked, waited for, and gone from the ledger.
#[test]
fn a_quit_that_ends_the_terminals_waits_for_them_to_have_ended() {
    let ledger = Arc::new(Ledger::memory());
    let rig = Rig::kept(
        "quit-ends",
        Fake::new(Bridge::new(Age::Current)),
        ledger.clone(),
        END_GRACE,
    );
    let pane = pane_named(&rig.world);
    let _holder = Holder::new(&rig.socket(), "b-1");
    let terminal = rig.terminals(1).remove(0);
    until("the pane found", || rig.service.shown(&pane).is_some());

    let started = Instant::now();
    rig.service
        .end_all(&[pane.clone()], Duration::from_secs(10));
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "told, not timed out"
    );
    assert_eq!(rig.world.bridge().terminated(&terminal), Some(false));
    assert!(
        ledger.terminals().is_empty(),
        "it ended: nothing to remember"
    );
    assert!(ledger.known("u@rasp:22").is_none());
}

/// A server that does not answer does not hold the quit: the wait is bounded, and the
/// ledger keeps the request for the next connection.
#[test]
fn a_quit_does_not_wait_for_a_server_that_does_not_end_the_terminal() {
    let ledger = Arc::new(Ledger::memory());
    let rig = Rig::kept(
        "quit-stubborn",
        Fake::new(Bridge::new(Age::Current).stubborn()),
        ledger.clone(),
        Duration::from_secs(60),
    );
    let pane = pane_named(&rig.world);
    let _holder = Holder::new(&rig.socket(), "b-1");
    let terminal = rig.terminals(1).remove(0);
    until("the pane found", || rig.service.shown(&pane).is_some());

    let started = Instant::now();
    rig.service.end_all(&[pane], Duration::from_millis(300));
    let waited = started.elapsed();
    assert!(waited >= Duration::from_millis(250), "{waited:?}");
    assert!(waited < Duration::from_secs(5), "{waited:?}");
    assert_eq!(rig.world.bridge().terminated(&terminal), Some(false));
    assert_eq!(ledger.ending("u@rasp:22"), [terminal]);
}

/// A pane that shows nothing, and an account whose connection is down, are not waited for.
#[test]
fn a_quit_has_nothing_to_wait_for_when_nothing_can_be_asked() {
    let ledger = Arc::new(Ledger::memory());
    let rig = Rig::kept(
        "quit-none",
        Fake::new(Bridge::new(Age::Current)),
        ledger.clone(),
        END_GRACE,
    );
    let pane = pane_named(&rig.world);
    let holder = Holder::new(&rig.socket(), "b-1");
    let terminal = rig.terminals(1).remove(0);
    rig.world.bridge().write(&terminal, b"one");
    until("one shown", || holder.shown.bytes() == b"one");
    until("the pane found", || rig.service.shown(&pane).is_some());

    // Another pane: nothing.
    let other = PaneUuid::parse("5a7e9c40-1b2d-4e3f-8a6b-0c9d1e2f3a4b").expect("a uuid");
    let started = Instant::now();
    rig.service.end_all(&[other], Duration::from_secs(10));
    assert!(started.elapsed() < Duration::from_secs(5));
    assert_eq!(rig.world.bridge().terminated(&terminal), None);

    // The connection is down: the request waits in the ledger, and nobody waits for it.
    rig.world
        .carry_fails_with(Some(io::ErrorKind::ConnectionRefused));
    rig.world.break_connection();
    until("the pane told", || {
        rig.service
            .shown(&pane)
            .is_some_and(|shown| shown.link.is_passive())
    });
    let started = Instant::now();
    rig.service.end_all(&[pane], Duration::from_secs(10));
    assert!(started.elapsed() < Duration::from_secs(5), "not waited for");
    assert_eq!(ledger.ending("u@rasp:22"), [terminal.clone()]);

    // The network is back: the first thing the connection does is end the terminal.
    rig.world.carry_fails_with(None);
    rig.service.wake();
    until("the terminal ended at the next connection", || {
        rig.world.bridge().terminated(&terminal) == Some(false)
    });
    until("forgotten once it ended", || ledger.terminals().is_empty());
}

/// A terminal an earlier bateri asked to end on a server it could not reach is ended the
/// first time any pane connects to the account, whichever terminal that pane shows.
#[test]
fn a_terminal_left_to_end_is_ended_by_the_next_connection_to_its_account() {
    let ledger = Arc::new(Ledger::memory());
    let mut bridge = Bridge::new(Age::Current);
    let left = bridge.add(
        "b-left",
        Size {
            cols: 80,
            rows: 24,
            xpixel: 0,
            ypixel: 0,
        },
    );
    ledger.end("u@rasp:22", &left);
    let rig = Rig::kept("left", Fake::new(bridge), ledger.clone(), END_GRACE);
    let _holder = Holder::new(&rig.socket(), "b-1");
    rig.terminals(1);
    until("the left terminal ended", || {
        rig.world.bridge().terminated(&left) == Some(false)
    });
    until("forgotten", || ledger.ending("u@rasp:22").is_empty());
    let kept = SessionId::new("b-1").expect("an id");
    assert_eq!(
        rig.world.bridge().terminated(&kept),
        None,
        "the pane's own terminal is left alone"
    );
    assert!(ledger.keeps("u@rasp:22", &kept));
}

/// The holder of a pane survives the bateri that served it (an update's holder keeps it; so does
/// `"quit"`'s) and finds the next one at the same place: it asks again by itself, from the byte
/// it was at, and the terminal goes on — nothing twice, nothing missing — with nobody having
/// told the new bateri anything but what the ledger kept.
#[test]
fn the_holder_that_waited_through_a_restart_finds_its_terminal_where_it_left_it() {
    let ledger = Arc::new(Ledger::memory());
    let world = Fake::new(Bridge::new(Age::Current));
    let pane = pane_named(&world);
    let first = Rig::kept("wait-1", world.clone(), ledger.clone(), END_GRACE);
    let socket = first.socket();
    let holder = Holder::new(&socket, "b-1");
    let terminal = first.terminals(1).remove(0);
    world.bridge().write(&terminal, b"abc");
    until("shown", || holder.shown.bytes() == b"abc");

    // The bateri goes. The program goes on writing, and the holder waits.
    first.service.stop();
    world.bridge().write(&terminal, b"def");
    thread::sleep(Duration::from_millis(100));
    assert_eq!(holder.shown.bytes(), b"abc");

    // The next bateri binds the same place (the instance's directory is carried over).
    let second = Service::start_with(&first.dir, world.clone(), ledger.clone(), END_GRACE)
        .expect("a second service");
    world.bridge().write(&terminal, b"ghi");
    until("the rest, once", || holder.shown.bytes() == b"abcdefghi");
    until("the pane found again", || second.shown(&pane).is_some());
    assert_eq!(
        second.shown(&pane).expect("shown").account,
        "u@rasp:22",
        "the account came from the ledger"
    );
    second.stop();
}

/// The pane is told its terminal appeared once, when the holder is admitted —
/// before anything is shown and while its link still reads as live — so the
/// add-ons that wait for "this is a server pane" ask again whichever of the
/// process-table probe and the holder came second; and told again when the
/// terminal goes, so what was started on the server for the pane is taken down
/// even when the pane goes on as a plain `ssh` whose process-table target is the
/// same. A lost connection and a return are not changes.
#[test]
fn a_pane_is_told_when_the_terminal_it_shows_appears_and_when_it_goes() {
    let rig = Rig::new("changed");
    let pane = pane_named(&rig.world);
    // The first connection is slow: the terminal is not shown yet.
    rig.world.carry_fails.store(1, Ordering::SeqCst);
    let mut holder = Holder::new(&rig.socket(), "b-1");
    let terminal = rig.terminals(1).remove(0);
    until("the pane told", || rig.world.changed() == [pane.clone()]);
    assert_eq!(
        rig.service.shown(&pane).map(|shown| shown.host),
        Some("rasp".to_owned()),
        "it is shown by the time it is told"
    );
    rig.world.bridge().write(&terminal, b"one");
    until("one shown", || holder.shown.bytes() == b"one");
    rig.world
        .carry_fails_with(Some(io::ErrorKind::ConnectionRefused));
    rig.world.break_connection();
    until("the pane told it is waiting", || {
        rig.world.links() == [(pane.clone(), PaneLink::Reconnecting)]
    });
    rig.world.carry_fails_with(None);
    rig.service.retry(&pane);
    until("it is back", || {
        rig.world.links().last() == Some(&(pane.clone(), PaneLink::Live))
    });
    assert_eq!(rig.world.changed(), [pane.clone()], "once");
    rig.world.bridge().exit(&terminal, Exit::Code(0));
    assert_eq!(holder.outcome(), Outcome::Exit(0));
    until("the pane told it went", || {
        rig.world.changed() == [pane.clone(), pane.clone()]
    });
    assert!(rig.service.shown(&pane).is_none(), "and it is gone by then");
}

/// A terminal serves the remote session of its own server and of no other:
/// the rule every add-on that works on a server asks.
#[test]
fn a_terminal_serves_the_session_to_its_own_server_only() {
    let shown = Shown {
        account: "u@h:22".into(),
        host: "rasp".into(),
        argv: vec!["ssh".into(), "rasp".into()],
        terminal: SessionId::new("b-1").expect("an id"),
        program: None,
        link: PaneLink::Live,
        taken_by: None,
    };
    let target = |host: &str| RemoteTarget {
        host: host.into(),
        port: None,
        kind: bt_core::RemoteKind::Ssh,
        argv: vec!["ssh".into(), host.into()],
        line: format!("ssh {host}"),
    };
    assert!(shown.serves(&target("rasp")));
    assert!(!shown.serves(&target("prod")));
    assert!(!shown.serves(&target("root@rasp")));
}

// ─── what runs in the background ─────────────────────────────────────────

const ACCOUNT: &str = "u@rasp:22";

fn activity(started: u64, front_started: u64, own: bool, output_at: u64) -> Activity {
    Activity {
        started: Some(started),
        front_started: Some(front_started),
        front_is_own: Some(own),
        output_at: Some(output_at),
    }
}

fn background_names(rig: &Rig) -> Vec<String> {
    rig.service
        .background(ACCOUNT)
        .iter()
        .map(|found| found.terminal.as_str().to_owned())
        .collect()
}

/// The terminals an agent started on the server while the Mac was connected are listed once
/// somebody is about to look, the last to write first, with what the server says of them; the
/// one a pane shows is not among them.
#[test]
fn what_runs_on_the_server_and_no_pane_shows_is_in_the_background_the_last_to_write_first() {
    let rig = Rig::new("background");
    let _holder = Holder::new(&rig.socket(), "b-1");
    let shown = rig.terminals(1).remove(0);
    {
        let mut bridge = rig.world.bridge();
        let quiet = bridge.add("agent-quiet", Size::default());
        let busy = bridge.add("agent-busy", Size::default());
        bridge.set_activity(&quiet, activity(100, 100, true, 200));
        bridge.set_activity(&busy, activity(100, 900, false, 1_000));
        bridge.set_state(&busy, saying("claude"));
        bridge.set_label(&busy, "review");
        bridge.set_activity(&shown, activity(50, 50, true, 5_000));
    }
    rig.service.refresh_background();
    until("both listed", || {
        background_names(&rig) == ["agent-busy", "agent-quiet"]
    });

    let found = rig.service.background(ACCOUNT);
    assert_eq!(found[0].host, "rasp");
    assert_eq!(found[0].account, ACCOUNT);
    assert_eq!(
        found[0].argv,
        ["ssh", "rasp"],
        "what a pane that shows it is started with"
    );
    assert_eq!(found[0].program.as_deref(), Some("claude"));
    assert_eq!(found[0].label.as_deref(), Some("review"));
    assert_eq!(
        (
            found[0].started,
            found[0].front_started,
            found[0].front_is_own,
            found[0].output_at
        ),
        (Some(100), Some(900), Some(false), Some(1_000)),
        "the times of kablo 010 are read"
    );
    assert_eq!(found[1].front_is_own, Some(true));
    assert_eq!(
        rig.service.background_all().len(),
        2,
        "every account's, the same two"
    );
    assert!(
        rig.world.background_told().iter().any(|a| a == ACCOUNT),
        "the application is told the list is not what it was"
    );
}

/// A terminal whose program ended while nobody was attached is still on the server's list, and
/// is neither running nor in the background: the application reads it as one that ended.
#[test]
fn a_terminal_that_ended_unseen_is_exited_and_not_in_the_background() {
    let rig = Rig::new("background-exited");
    let _holder = Holder::new(&rig.socket(), "b-1");
    let shown = rig.terminals(1).remove(0);
    let agent = rig.world.bridge().add("agent", Size::default());
    let done = rig.world.bridge().add("done", Size::default());
    rig.world.bridge().exit(&done, Exit::Code(3));
    rig.service.refresh_background();
    until("listed", || background_names(&rig) == ["agent"]);
    assert_eq!(
        rig.service.exited(ACCOUNT),
        [done.clone()],
        "the ended one, and not the one a pane shows"
    );
    let _ = (shown, agent);
    assert_eq!(
        rig.service.exited("another account"),
        Vec::<SessionId>::new()
    );
}

/// A server is read for what was left for the application once per connection, after it has
/// said what it keeps: the listings that follow do not ask again.
#[test]
fn what_a_server_left_for_the_application_is_read_once_per_connection() {
    let rig = Rig::new("reachable");
    let _holder = Holder::new(&rig.socket(), "b-1");
    rig.terminals(1);
    until("told the server can be read", || {
        !rig.world.reached.lock().expect("lock").is_empty()
    });
    rig.service.refresh_background();
    rig.service.refresh_background();
    rig.world.bridge().add("agent", Size::default());
    rig.service.refresh_background();
    until("listed", || background_names(&rig) == ["agent"]);
    assert_eq!(
        *rig.world.reached.lock().expect("lock"),
        [(ACCOUNT.to_owned(), "rasp".to_owned())]
    );
}

/// A terminal a pane lets go of without ending it (the pane's program went; a crash does the
/// same) lives on and is listed; ending it takes it out of the list at once.
#[test]
fn a_terminal_a_pane_lets_go_of_is_in_the_background_until_it_is_ended() {
    let rig = Rig::new("background-let-go");
    let pane = pane_named(&rig.world);
    let holder = Holder::new(&rig.socket(), "b-1");
    let terminal = rig.terminals(1).remove(0);
    until("the pane found", || rig.service.shown(&pane).is_some());
    assert!(
        rig.service.background(ACCOUNT).is_empty(),
        "the terminal on screen is not in the background"
    );

    let Holder { handle, typed, .. } = holder;
    drop(typed);
    until("the pane let go of", || rig.service.shown(&pane).is_none());
    // No refresh asked: the pane going is what makes the service ask.
    until("in the background", || {
        background_names(&rig) == [terminal.as_str().to_owned()]
    });
    if let Some(handle) = handle {
        let _ = handle.join();
    }
    assert!(rig.world.bridge().has(&terminal), "nothing ended it");

    // Shown again, it leaves the background.
    let _again = Holder::start(
        &rig.socket(),
        terminal.as_str(),
        Target::Resume {
            account: ACCOUNT.to_owned(),
            offset: Some(0),
        },
    );
    until("shown again", || rig.service.shown(&pane).is_some());
    until("out of the background", || {
        rig.service.background(ACCOUNT).is_empty()
    });
}

/// The terminal a user closed is on its way out: the list must not show it for the moment
/// between the hang-up and the server's answer.
#[test]
fn a_terminal_asked_to_end_is_not_in_the_background() {
    let rig = Rig::graced(
        "background-ending",
        Fake::new(Bridge::new(Age::Current).stubborn()),
        Duration::from_millis(300),
    );
    let pane = pane_named(&rig.world);
    let holder = Holder::new(&rig.socket(), "b-1");
    let terminal = rig.terminals(1).remove(0);
    until("the pane found", || rig.service.shown(&pane).is_some());

    rig.service.end(&pane);
    let Holder { handle, typed, .. } = holder;
    drop(typed);
    until("the hang-up", || {
        rig.world.bridge().terminated(&terminal) == Some(false)
    });
    let lists = |rig: &Rig| {
        rig.world
            .bridge()
            .received
            .iter()
            .filter(|packet| matches!(packet.message, kablo_proto::v1::Message::List))
            .count()
    };
    let before = lists(&rig);
    rig.service.refresh_background();
    until("the server asked again", || lists(&rig) > before);
    // The stubborn program stays on the server and is listed as running; the list hides it
    // until the kill.
    for _ in 0..20 {
        assert!(
            rig.service.background(ACCOUNT).is_empty(),
            "an ending terminal is not offered"
        );
        thread::sleep(Duration::from_millis(5));
    }
    if let Some(handle) = handle {
        let _ = handle.join();
    }
}

/// The card's End Terminal: a terminal no pane shows is hung up on the server and is out of the
/// list at once, without a pane in between.
#[test]
fn a_terminal_nobody_shows_is_ended_from_the_list() {
    let rig = Rig::new("background-end-terminal");
    let _holder = Holder::new(&rig.socket(), "b-1");
    rig.terminals(1);
    let agent = rig.world.bridge().add("agent", Size::default());
    let other = rig.world.bridge().add("other", Size::default());
    rig.service.refresh_background();
    until("both listed", || background_names(&rig).len() == 2);

    rig.service.end_terminal(ACCOUNT, &agent);
    until("the hang-up", || {
        rig.world.bridge().terminated(&agent).is_some()
    });
    until("out of the list", || {
        background_names(&rig) == [other.as_str().to_owned()]
    });
    assert_eq!(
        rig.world.bridge().terminated(&other),
        None,
        "only the one that was asked"
    );
}

/// What the server lists is not kept for ever: a terminal that ended while nobody watched is
/// gone from the list at the next answer, and one a connection that is down knew of stays as
/// the last answer said.
#[test]
fn the_next_answer_takes_a_terminal_that_ended_out_of_the_list() {
    let rig = Rig::new("background-ended");
    let _holder = Holder::new(&rig.socket(), "b-1");
    rig.terminals(1);
    let agent = rig.world.bridge().add("agent", Size::default());
    rig.service.refresh_background();
    until("listed", || background_names(&rig) == ["agent"]);

    rig.world.bridge().exit(&agent, Exit::Code(0));
    rig.service.refresh_background();
    until("gone", || rig.service.background(ACCOUNT).is_empty());
}

/// A new terminal on a server that keeps others nobody looks at is told so before the terminal
/// writes, once for these terminals: the next one while the same wait is silent, and the first of
/// an account, made before the server was heard from, says nothing.
#[test]
fn a_new_terminal_is_told_of_those_waiting_in_the_background_once() {
    let rig = Rig::new("background-note");
    let first = Holder::new(&rig.socket(), "b-1");
    rig.terminals(1);
    rig.world.bridge().add("agent-busy", Size::default());
    rig.service.refresh_background();
    until("listed", || background_names(&rig) == ["agent-busy"]);

    let line = "bateri: 1 more terminal is running on rasp; click \u{21c4} rasp below to see it";
    let second = Holder::new(&rig.socket(), "b-2");
    rig.terminals(2);
    until("the second is told", || {
        String::from_utf8_lossy(&second.shown.bytes()).contains(line)
    });
    let shown = String::from_utf8_lossy(&second.shown.bytes()).into_owned();
    assert!(
        shown.starts_with("\x1b[0m\x1b[2m"),
        "dim, before any output"
    );

    // The same wait again: said once. A byte from the third terminal is the point after which
    // a note, had one been sent, would have arrived.
    let third = Holder::new(&rig.socket(), "b-3");
    let terminals_after = rig.terminals(3);
    rig.world.bridge().write(&terminals_after[2], b"hello");
    until("the third shows its terminal", || {
        third.shown.bytes().ends_with(b"hello")
    });
    assert!(
        !String::from_utf8_lossy(&third.shown.bytes()).contains("more terminal"),
        "told once for these"
    );
    assert!(
        !String::from_utf8_lossy(&first.shown.bytes()).contains("more terminal"),
        "nothing was known when the first was made"
    );
}

// ─── terminals that move between Macs ────────────────────────────────────

fn elsewhere_names(rig: &Rig) -> Vec<String> {
    rig.service
        .elsewhere(ACCOUNT)
        .iter()
        .map(|found| found.terminal.as_str().to_owned())
        .collect()
}

/// A terminal another Mac has open is *on another Mac*, named by it, and is no longer in the
/// background — which is the terminals nobody has. An agent's `kablo wait` is attached too, with
/// no name, and the terminal is still nobody's.
#[test]
fn a_terminal_another_mac_has_open_is_on_that_mac_and_not_in_the_background() {
    let rig = Rig::new("elsewhere");
    let _holder = Holder::new(&rig.socket(), "b-1");
    rig.terminals(1);
    {
        let mut bridge = rig.world.bridge();
        let open = bridge.add("agent-open", Size::default());
        bridge.set_others(&open, &[Some("iMac"), None]);
        let watched = bridge.add("agent-watched", Size::default());
        bridge.set_others(&watched, &[None]);
        bridge.add("agent-free", Size::default());
    }
    rig.service.refresh_background();
    until("listed", || elsewhere_names(&rig) == ["agent-open"]);
    assert_eq!(
        background_names(&rig).len(),
        2,
        "the one an agent watches and the free one"
    );
    assert!(!background_names(&rig).contains(&"agent-open".to_owned()));
    let found = rig.service.elsewhere(ACCOUNT);
    assert_eq!(found[0].elsewhere(), ["iMac"]);
    assert_eq!(rig.service.elsewhere_all().len(), 1);
    assert!(
        rig.world.background_told().iter().any(|a| a == ACCOUNT),
        "the application is told when either list changes"
    );

    // The Mac lets go of it: the next answer puts it in the background.
    rig.world
        .bridge()
        .set_others(&SessionId::new("agent-open").expect("an id"), &[]);
    rig.service.refresh_background();
    until("in the background", || elsewhere_names(&rig).is_empty());
    assert!(background_names(&rig).contains(&"agent-open".to_owned()));
}

/// A relay from before the names counts its clients and says none: it cannot be told who has the
/// terminal, so nothing is offered to be moved and the terminal stays in the background.
#[test]
fn a_relay_that_names_no_clients_keeps_every_terminal_in_the_background() {
    let rig = Rig::over("elsewhere-old", Fake::new(Bridge::new(Age::Old)));
    let _holder = Holder::new(&rig.socket(), "b-1");
    rig.terminals(1);
    {
        let mut bridge = rig.world.bridge();
        let open = bridge.add("agent-open", Size::default());
        bridge.set_others(&open, &[Some("iMac")]);
    }
    rig.service.refresh_background();
    until("listed", || background_names(&rig) == ["agent-open"]);
    assert!(rig.service.elsewhere(ACCOUNT).is_empty());
}

/// Another Mac takes the terminal a pane shows: the pane keeps its terminal and is told it was
/// taken, and by whom; closing it ends nothing on the server and the ledger does not name the
/// terminal (a quit that ends terminals must not end the other Mac's); ⏎ takes it back.
#[test]
fn a_terminal_another_mac_took_dims_the_pane_and_closing_it_ends_nothing() {
    let ledger = Arc::new(Ledger::memory());
    let rig = Rig::kept(
        "taken",
        Fake::new(Bridge::new(Age::Current)),
        ledger.clone(),
        Duration::from_millis(300),
    );
    let pane = pane_named(&rig.world);
    let _holder = Holder::new(&rig.socket(), "b-1");
    let terminal = rig.terminals(1).remove(0);
    until("the pane found", || rig.service.shown(&pane).is_some());
    rig.world.bridge().set_state(&terminal, saying("npm"));
    until("the program known", || {
        rig.service
            .shown(&pane)
            .is_some_and(|shown| shown.running().is_some())
    });
    assert!(ledger.keeps(ACCOUNT, &terminal));

    rig.world.bridge().take_by(&terminal, "iMac");
    until("the pane told", || {
        rig.world.links().last() == Some(&(pane.clone(), PaneLink::Taken))
    });
    let shown = rig.service.shown(&pane).expect("still shown");
    assert_eq!(shown.link, PaneLink::Taken);
    assert_eq!(shown.taken_by.as_deref(), Some("iMac"));
    assert_eq!(shown.running(), None, "nothing runs here to ask about");
    until("the ledger lets go", || !ledger.keeps(ACCOUNT, &terminal));
    assert!(
        rig.service.background(ACCOUNT).is_empty() && rig.service.elsewhere(ACCOUNT).is_empty(),
        "a pane still names it: it is on screen"
    );

    // Closing the dimmed pane hangs up nothing, now or after the grace.
    rig.service.end(&pane);
    thread::sleep(Duration::from_millis(700));
    assert_eq!(rig.world.bridge().terminated(&terminal), None);
    assert!(rig.world.bridge().has(&terminal));
}

#[test]
fn bringing_a_taken_terminal_back_attaches_taking_and_writes_the_ledger_again() {
    let ledger = Arc::new(Ledger::memory());
    let rig = Rig::kept(
        "taken-back",
        Fake::new(Bridge::new(Age::Current)),
        ledger.clone(),
        END_GRACE,
    );
    let pane = pane_named(&rig.world);
    let _holder = Holder::new(&rig.socket(), "b-1");
    let terminal = rig.terminals(1).remove(0);
    until("the pane found", || rig.service.shown(&pane).is_some());
    // The other Mac can only take what is attached.
    until("attached", || rig.world.bridge().is_attached(&terminal));
    rig.world.bridge().take_by(&terminal, "iMac");
    until("the pane told", || {
        rig.service
            .shown(&pane)
            .is_some_and(|shown| shown.link == PaneLink::Taken)
    });

    rig.service.bring_back(&pane);
    until("the pane live again", || {
        rig.service
            .shown(&pane)
            .is_some_and(|shown| shown.link == PaneLink::Live && shown.taken_by.is_none())
    });
    until("the ledger names it again", || {
        ledger.keeps(ACCOUNT, &terminal)
    });
    let attaches = rig.world.bridge().attaches(&terminal);
    let (_, view) = attaches.last().expect("an attach");
    let view = view.expect("a view");
    assert!(view.take, "taking it back");
    assert!(view.name.is_some(), "under this Mac's name");
    assert_eq!(
        rig.world.links().last(),
        Some(&(pane.clone(), PaneLink::Live))
    );
}

/// A terminal moved here from another Mac: said before the pane opens, the pane's first attach
/// takes it and the other Mac is let go; a terminal opened the usual way takes nothing.
#[test]
fn a_terminal_moved_here_is_taken_and_one_opened_the_usual_way_is_not() {
    let rig = Rig::new("move-here");
    let _first = Holder::new(&rig.socket(), "b-1");
    rig.terminals(1);
    let (open, usual) = {
        let mut bridge = rig.world.bridge();
        let open = bridge.add("agent-open", Size::default());
        bridge.set_others(&open, &[Some("iMac")]);
        let usual = bridge.add("agent-usual", Size::default());
        bridge.set_others(&usual, &[Some("iMac")]);
        (open, usual)
    };
    rig.service.refresh_background();
    until("both are on the other Mac", || {
        elsewhere_names(&rig).len() == 2
    });

    rig.service.take_over(ACCOUNT, &open);
    let resume = || Target::Resume {
        account: ACCOUNT.to_owned(),
        offset: None,
    };
    let _moved = Holder::start(&rig.socket(), open.as_str(), resume());
    until("the move took it", || {
        rig.world.bridge().others_of(&open).is_empty()
    });
    let attaches = rig.world.bridge().attaches(&open);
    assert!(
        attaches
            .last()
            .and_then(|(_, view)| *view)
            .expect("a view")
            .take
    );

    let _usual = Holder::start(&rig.socket(), usual.as_str(), resume());
    until("the other attached", || {
        !rig.world.bridge().attaches(&usual).is_empty()
    });
    assert!(
        !rig.world.bridge().attaches(&usual)[0]
            .1
            .expect("a view")
            .take
    );
    assert_eq!(
        rig.world.bridge().others_of(&usual),
        [Some("iMac".to_owned())],
        "opened beside, taking nothing"
    );
}

// Mail: what a program on the server says to the application

/// A rig whose application takes mail, with one terminal on the server (so the account's
/// bridge is up and has been told the app takes tickets).
fn mail_rig(name: &str) -> (Rig, Holder, SessionId) {
    let world = Fake::new(Bridge::new(Age::Current));
    world.mail.store(true, Ordering::SeqCst);
    let rig = Rig::over(name, world);
    let holder = Holder::new(&rig.socket(), "b-1");
    let terminal = rig.terminals(1).remove(0);
    (rig, holder, terminal)
}

/// How many conversations the app has taken so far, as the bridge saw the answers to tickets.
fn taken(rig: &Rig) -> usize {
    rig.world
        .bridge()
        .received_streams
        .iter()
        .filter(|p| matches!(p.message, stream::Message::OpenTicket { .. }))
        .count()
}

/// A program on the server calls the app: the ticket the bridge gave.
fn program_calls(rig: &Rig, from: Option<&SessionId>) -> u32 {
    let mut ticket = None;
    until("the bridge knows the app takes tickets", || {
        ticket = rig.world.bridge().mail(from, 1000);
        ticket.is_some()
    });
    ticket.expect("a ticket")
}

/// A program on the server opens a conversation and the service takes it at once: the app's
/// stream the program talks into.
fn program_opens(rig: &Rig, from: Option<&SessionId>) -> u32 {
    let before = taken(rig);
    program_calls(rig, from);
    until("the conversation is taken", || taken(rig) > before);
    rig.world
        .bridge()
        .received_streams
        .iter()
        .rev()
        .find(|p| matches!(p.message, stream::Message::OpenTicket { .. }))
        .map(|p| p.stream)
        .expect("a stream")
}

#[test]
fn a_letter_is_handed_over_whole_with_the_terminal_it_is_from_and_the_answer_goes_back() {
    let (rig, _holder, terminal) = mail_rig("letter");
    // The conversation is taken at once, before a word of it: the program waits ten seconds
    // for that, and the application as long as it needs for the answer.
    let stream = program_opens(&rig, Some(&terminal));
    rig.world
        .bridge()
        .sender_data(stream, b"place\nterminal t4\n");
    rig.world.bridge().sender_data(stream, b"side below\n");
    assert!(rig.world.letters.lock().expect("lock").is_empty());
    rig.world.bridge().sender_eof(stream);

    let letter = rig.world.letter();
    assert_eq!(letter.account, "u@rasp:22");
    assert_eq!(letter.from, Some(terminal));
    assert_eq!(letter.bytes, b"place\nterminal t4\nside below\n");

    assert_eq!(
        (letter.host.as_str(), letter.argv.as_slice()),
        ("rasp", ["ssh".to_owned(), "rasp".to_owned()].as_slice()),
        "the server the letter came from, as a pane that shows one of its terminals is started"
    );
    letter.answer(b"ok landed as_asked\n".to_vec());
    until("the answer", || {
        rig.world.bridge().said_to_sender(stream) == b"ok landed as_asked\n"
            && rig.world.bridge().app_finished(stream)
    });
    assert_eq!(rig.world.bridge().breaches(), 0);
}

#[test]
fn a_program_in_no_terminal_is_a_letter_from_none() {
    let (rig, _holder, _terminal) = mail_rig("letter-none");
    let stream = program_opens(&rig, None);
    rig.world.bridge().sender_eof(stream);
    let letter = rig.world.letter();
    assert_eq!(letter.from, None);
    assert!(letter.bytes.is_empty());
    letter.answer(Vec::new());
    until("the end", || rig.world.bridge().app_finished(stream));
}

#[test]
fn a_letter_that_is_dropped_hangs_up_so_that_the_program_is_not_left_waiting() {
    let (rig, _holder, _terminal) = mail_rig("letter-dropped");
    let stream = program_opens(&rig, None);
    rig.world.bridge().sender_data(stream, b"ls\n");
    rig.world.bridge().sender_eof(stream);
    let letter = rig.world.letter();
    drop(letter);
    until("the hang up", || rig.world.bridge().app_closed(stream));
}

#[test]
fn a_letter_can_be_answered_from_another_thread() {
    fn is_send<T: Send>() {}
    is_send::<Letter>();
}

#[test]
fn a_program_that_gives_up_before_it_has_said_all_it_had_hands_nothing_over() {
    let (rig, _holder, _terminal) = mail_rig("letter-gave-up");
    let first = program_opens(&rig, None);
    rig.world.bridge().sender_data(first, b"place\n");
    rig.world.bridge().sender_close(first);
    // A second conversation after it, so that the end of the first is known to have been seen.
    let second = program_opens(&rig, None);
    rig.world.bridge().sender_eof(second);
    let letter = rig.world.letter();
    assert!(letter.bytes.is_empty(), "{:?}", letter.bytes);
    assert!(rig.world.letters.lock().expect("lock").is_empty());
    drop(letter);
}

#[test]
fn a_program_that_says_more_than_a_request_is_ever_hung_up_on() {
    let (rig, _holder, _terminal) = mail_rig("letter-long");
    let stream = program_opens(&rig, None);
    let chunk = vec![b'x'; 16 * 1024];
    let mut sent = 0u32;
    while sent as usize <= LETTER_LIMIT && !rig.world.bridge().app_closed(stream) {
        rig.world.bridge().sender_data(stream, &chunk);
        sent += chunk.len() as u32;
        // The window the app gave is a bounded one: wait for it to be credited back.
        until("the bytes credited", || {
            rig.world.bridge().credited_by_app(stream) >= sent
                || rig.world.bridge().app_closed(stream)
        });
    }
    until("the hang up", || rig.world.bridge().app_closed(stream));
    rig.world.bridge().sender_eof(stream);
    assert!(rig.world.letters.lock().expect("lock").is_empty());
    assert_eq!(rig.world.bridge().breaches(), 0);
}

#[test]
fn an_application_that_takes_no_mail_is_given_none_and_a_ticket_that_comes_anyway_is_declined() {
    // It says nothing of tickets, so the bridge has none to give.
    let rig = Rig::new("no-mail");
    let _holder = Holder::new(&rig.socket(), "b-1");
    rig.terminals(1);
    assert_eq!(rig.world.bridge().mail(None, 100), None);
    drop(rig);

    // It took mail when the account was made and does not now: the ticket is declined, in words.
    let (rig, _holder, _terminal) = mail_rig("mail-off");
    rig.world.mail.store(false, Ordering::SeqCst);
    let ticket = program_calls(&rig, None);
    until("the decline", || !rig.world.bridge().declined().is_empty());
    assert_eq!(
        rig.world.bridge().declined(),
        [(
            ticket,
            "bateri is not taking requests from programs now".to_owned()
        )]
    );
    assert!(rig.world.letters.lock().expect("lock").is_empty());
}

#[test]
fn an_answer_to_a_letter_of_a_connection_that_went_is_not_said_on_the_next() {
    let (rig, _holder, _terminal) = mail_rig("letter-stale");
    let stream = program_opens(&rig, None);
    rig.world.bridge().sender_eof(stream);
    let old = rig.world.letter();

    rig.world.break_connection();
    until("the bridge is made again", || rig.world.count("carry") == 2);
    // The new connection numbers its streams from 1 again, as the old one did.
    let again = program_opens(&rig, None);
    assert_eq!(again, stream);
    old.answer(b"too late\n".to_vec());
    rig.world.bridge().sender_eof(again);
    let new = rig.world.letter();
    new.answer(b"ok\n".to_vec());
    until("the new conversation's answer", || {
        rig.world.bridge().app_finished(again)
    });
    assert_eq!(rig.world.bridge().said_to_sender(again), b"ok\n");
}

// Survey: every terminal of an account as the server has it now, with who opened each

/// The answer to a survey, once it comes.
fn surveyed(rig: &Rig, account: &str) -> Survey {
    let (tx, rx) = mpsc::channel();
    rig.service.survey(account, move |survey| {
        let _ = tx.send(survey);
    });
    rx.recv_timeout(Duration::from_secs(15)).expect("a survey")
}

#[test]
fn a_survey_is_the_servers_list_asked_for_now_with_the_records_read_beside_it() {
    let world = Fake::new(Bridge::new(Age::Current));
    world.records.lock().expect("lock").push((
        crate::agent::Terminal(1),
        crate::agent::notes::Record {
            opener: crate::agent::Anchor::parse("t4"),
            keep_open: true,
        },
    ));
    let rig = Rig::over("survey", world);
    let _holder = Holder::new(&rig.socket(), "b-1");
    let terminals = rig.terminals(1);
    let survey = surveyed(&rig, "u@rasp:22");
    let list = survey.terminals.expect("the server's list");
    assert_eq!(
        list.iter()
            .map(|status| status.id.clone())
            .collect::<Vec<_>>(),
        terminals
    );
    assert_eq!(survey.records.len(), 1);
    assert!(survey.records[0].1.keep_open);
    assert_eq!(rig.world.count("records"), 1);
    // A terminal that came after is in the next one: the list is asked for, not remembered.
    let _second = Holder::new(&rig.socket(), "b-2");
    rig.terminals(2);
    let list = surveyed(&rig, "u@rasp:22").terminals.expect("a list");
    assert_eq!(list.len(), 2);
}

#[test]
fn a_survey_of_an_account_that_is_not_connected_is_answered_at_once_with_nothing() {
    let rig = Rig::new("survey-none");
    let survey = surveyed(&rig, "nobody@nowhere:22");
    assert!(survey.terminals.is_none() && survey.records.is_empty());
    assert_eq!(rig.world.count("records"), 0, "nothing is read for it");
}

#[test]
fn a_survey_whose_records_are_slow_waits_for_them() {
    let world = Fake::new(Bridge::new(Age::Current));
    *world.records_take.lock().expect("lock") = Duration::from_millis(300);
    world.records.lock().expect("lock").push((
        crate::agent::Terminal(1),
        crate::agent::notes::Record {
            opener: None,
            keep_open: false,
        },
    ));
    let rig = Rig::over("survey-slow", world);
    let _holder = Holder::new(&rig.socket(), "b-1");
    rig.terminals(1);
    let started = Instant::now();
    let survey = surveyed(&rig, "u@rasp:22");
    assert!(
        started.elapsed() >= Duration::from_millis(250),
        "the records were waited for"
    );
    assert!(survey.terminals.is_some());
    assert_eq!(survey.records.len(), 1);
}
