//! The helper's life, with a helper that is a thread of the test: it speaks `browser_ipc`'s own
//! end of the conversation (`Client`), started from the command line the manager gives, so the
//! start, the hello, the key, the proxy's port and the stop are the real ones.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver as Inbox, Sender as Outbox, channel};

use browser_ipc::Client;
use browser_ipc::message::{DownloadState, Event};

use super::*;
use crate::browser::layout::APP;
use crate::browser::version::Version;

/// What the stand-in helper does.
#[derive(Clone, Default)]
pub(crate) struct Behaviour {
    /// Says hello with a key that is not the one it was started with.
    pub(crate) wrong_key: bool,
    /// Does not connect at all (and ends).
    pub(crate) silent: bool,
}

struct Fake {
    behaviour: Behaviour,
    spawned: Arc<AtomicUsize>,
    /// Every command a helper got, and the arguments it was started with.
    commands: Outbox<Command>,
    args: Outbox<Vec<String>>,
    /// The helpers' event writers, to break one.
    events: Arc<Mutex<Vec<Writer<Event>>>>,
}

struct FakeProcess {
    id: u32,
    over: Arc<AtomicBool>,
    events: Arc<Mutex<Option<Writer<Event>>>>,
}

impl Process for FakeProcess {
    fn id(&self) -> u32 {
        self.id
    }

    fn exited(&mut self) -> io::Result<Option<String>> {
        Ok(self
            .over
            .load(Ordering::Acquire)
            .then(|| "exit status: 0".to_owned()))
    }

    fn kill(&mut self) {
        self.over.store(true, Ordering::Release);
        if let Some(events) = self.events.lock().unwrap().as_ref() {
            events.shutdown();
        }
    }
}

impl Spawn for Fake {
    fn spawn(&self, _executable: &Path, args: &[String]) -> io::Result<Box<dyn Process>> {
        self.spawned.fetch_add(1, Ordering::SeqCst);
        self.args.send(args.to_vec()).unwrap();
        let value = |name: &str| {
            args.iter()
                .find_map(|arg| arg.strip_prefix(&format!("--{name}=")))
                .unwrap()
                .to_owned()
        };
        let (socket, key) = (value("socket"), value("key"));
        let over = Arc::new(AtomicBool::new(false));
        let writer = Arc::new(Mutex::new(None));
        let process = FakeProcess {
            // The helper is this very process: `accept` checks the hello's pid against it.
            id: std::process::id(),
            over: over.clone(),
            events: writer.clone(),
        };
        if self.behaviour.silent {
            over.store(true, Ordering::Release);
            return Ok(Box::new(process));
        }
        let key = if self.behaviour.wrong_key {
            "not the key".to_owned()
        } else {
            key
        };
        let (commands, events_log) = (self.commands.clone(), self.events.clone());
        std::thread::spawn(move || {
            let Ok(mut client) = Client::connect(Path::new(&socket), &key, "0.1.0", "156.0.1")
            else {
                over.store(true, Ordering::Release);
                return;
            };
            *writer.lock().unwrap() = Some(client.events.clone());
            events_log.lock().unwrap().push(client.events.clone());
            while let Ok(Some(command)) = client.commands.recv() {
                let _ = commands.send(command.clone());
                match command {
                    Command::Open { browser, .. } => {
                        let _ = client.events.send(&Event::Opened { browser });
                    }
                    Command::Close { browser } => {
                        let _ = client.events.send(&Event::Closed { browser });
                    }
                    Command::Quit => break,
                    _ => {}
                }
            }
            over.store(true, Ordering::Release);
        });
        Ok(Box::new(process))
    }
}

/// A browser's sink that tells the test what it got.
struct Log {
    events: Outbox<Event>,
    lost: Outbox<String>,
}

impl Sink for Log {
    fn event(&self, event: Event) {
        let _ = self.events.send(event);
    }

    fn frame(&self, _frame: Frame, _surface: Surface) {}

    fn lost(&self, why: String) {
        let _ = self.lost.send(why);
    }
}

pub(crate) struct World {
    pub(crate) manager: Arc<Manager>,
    pub(crate) root: PathBuf,
    pub(crate) spawned: Arc<AtomicUsize>,
    pub(crate) commands: Inbox<Command>,
    pub(crate) args: Inbox<Vec<String>>,
    pub(crate) events: Arc<Mutex<Vec<Writer<Event>>>>,
}

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("bateri-helper-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

pub(crate) fn world(
    name: &str,
    behaviour: Behaviour,
    install: Option<&str>,
    linger: Duration,
) -> World {
    let root = temp(name);
    let layout = Layout::new(root.join("helper"));
    if let Some(version) = install {
        let staging = layout.staging().unwrap();
        std::fs::create_dir_all(staging.path().join(APP)).unwrap();
        let version = Version::parse(version).unwrap();
        layout.commit(&staging, &version, &Version::zero()).unwrap();
    }
    let (commands_in, commands) = channel();
    let (args_in, args) = channel();
    let spawned = Arc::new(AtomicUsize::new(0));
    let events = Arc::new(Mutex::new(Vec::new()));
    let fake = Fake {
        behaviour,
        spawned: spawned.clone(),
        commands: commands_in,
        args: args_in,
        events: events.clone(),
    };
    let config = Config {
        source: Source::Installed(layout),
        profiles: root.join("profiles"),
        run: root.join("run"),
        mock_keychain: true,
        linger,
        start_timeout: Duration::from_secs(5),
        trust: Arc::new(|_| Ok(())),
    };
    World {
        manager: Manager::new(config, Arc::new(fake)),
        root,
        spawned,
        commands,
        args,
        events,
    }
}

fn log() -> (Arc<Log>, Inbox<Event>, Inbox<String>) {
    let (events, events_out) = channel();
    let (lost, lost_out) = channel();
    (Arc::new(Log { events, lost }), events_out, lost_out)
}

fn key(name: Option<&str>) -> ProfileKey {
    ProfileKey::new("mac", "browserpen", name).unwrap()
}

const SIZE: (u32, u32) = (800, 600);
const WAIT: Duration = Duration::from_secs(5);

fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + WAIT;
    while !done() {
        assert!(Instant::now() < deadline, "timed out: {what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn opens(world: &World, count: usize) -> Vec<Command> {
    (0..count)
        .map(|_| {
            loop {
                let command = world.commands.recv_timeout(WAIT).expect("a command");
                if matches!(command, Command::Open { .. }) {
                    break command;
                }
            }
        })
        .collect()
}

#[test]
fn the_first_browser_starts_the_helper_and_the_second_shares_it() {
    let w = world(
        "share",
        Behaviour::default(),
        Some("0.1.0"),
        Duration::from_secs(60),
    );
    assert!(!w.manager.status().running);
    let (sink, events, _lost) = log();
    let first = w.manager.open(&key(None), SIZE, 2.0, sink.clone()).unwrap();
    let second = w.manager.open(&key(None), SIZE, 2.0, sink).unwrap();
    assert_eq!(
        w.spawned.load(Ordering::SeqCst),
        1,
        "one helper for two browsers"
    );
    let status = w.manager.status();
    assert_eq!(
        (
            status.running,
            status.browsers,
            status.version.as_deref(),
            status.chromium.as_deref()
        ),
        (true, 2, Some("0.1.0"), Some("156.0.1"))
    );
    let mut opened = vec![
        events.recv_timeout(WAIT).unwrap(),
        events.recv_timeout(WAIT).unwrap(),
    ];
    opened.sort_by_key(|event| event.browser());
    assert_eq!(
        opened,
        vec![
            Event::Opened {
                browser: first.id()
            },
            Event::Opened {
                browser: second.id()
            }
        ]
    );
    std::fs::remove_dir_all(&w.root).unwrap();
}

#[test]
fn the_helper_is_started_the_way_the_contract_says() {
    let w = world(
        "contract",
        Behaviour::default(),
        Some("0.1.0"),
        Duration::from_secs(60),
    );
    let (sink, _events, _lost) = log();
    let _browser = w.manager.open(&key(None), SIZE, 1.0, sink).unwrap();
    let args = w.args.recv_timeout(WAIT).unwrap();
    let names: Vec<&str> = args.iter().filter_map(|a| a.split('=').next()).collect();
    assert_eq!(
        names,
        [
            "--key",
            "--socket",
            "--mach",
            "--root",
            "--proxy",
            "--mock-keychain"
        ]
    );
    let proxy = args
        .iter()
        .find_map(|a| a.strip_prefix("--proxy="))
        .unwrap();
    assert!(proxy.starts_with("http://127.0.0.1:"), "{proxy}");
    let root = args.iter().find_map(|a| a.strip_prefix("--root=")).unwrap();
    assert_eq!(Path::new(root), w.root.join("profiles"));
    std::fs::remove_dir_all(&w.root).unwrap();
}

#[test]
fn the_last_close_ends_the_helper_after_the_linger_and_a_new_browser_starts_another() {
    let w = world(
        "linger",
        Behaviour::default(),
        Some("0.1.0"),
        Duration::from_millis(150),
    );
    let (sink, _events, _lost) = log();
    let first = w.manager.open(&key(None), SIZE, 1.0, sink.clone()).unwrap();
    let second = w.manager.open(&key(None), SIZE, 1.0, sink.clone()).unwrap();
    drop(first);
    std::thread::sleep(Duration::from_millis(400));
    assert!(w.manager.status().running, "one browser is still open");
    drop(second);
    wait_until("the helper is told to quit", || !w.manager.status().running);
    let seen: Vec<Command> = w.commands.try_iter().collect();
    assert!(seen.contains(&Command::Quit), "{seen:?}");
    let _third = w.manager.open(&key(None), SIZE, 1.0, sink).unwrap();
    assert_eq!(w.spawned.load(Ordering::SeqCst), 2);
    std::fs::remove_dir_all(&w.root).unwrap();
}

#[test]
fn a_browser_opened_within_the_linger_keeps_the_helper() {
    let w = world(
        "keep",
        Behaviour::default(),
        Some("0.1.0"),
        Duration::from_millis(300),
    );
    let (sink, _events, _lost) = log();
    let first = w.manager.open(&key(None), SIZE, 1.0, sink.clone()).unwrap();
    drop(first);
    std::thread::sleep(Duration::from_millis(100));
    let _second = w.manager.open(&key(None), SIZE, 1.0, sink).unwrap();
    std::thread::sleep(Duration::from_millis(600));
    assert!(w.manager.status().running);
    assert_eq!(w.spawned.load(Ordering::SeqCst), 1);
    std::fs::remove_dir_all(&w.root).unwrap();
}

#[test]
fn a_dead_helper_tells_its_browsers_and_the_next_one_starts_another() {
    let w = world(
        "crash",
        Behaviour::default(),
        Some("0.1.0"),
        Duration::from_secs(60),
    );
    let (sink, _events, lost) = log();
    let first = w.manager.open(&key(None), SIZE, 1.0, sink.clone()).unwrap();
    let _second = w.manager.open(&key(None), SIZE, 1.0, sink.clone()).unwrap();
    wait_until("the helper is connected", || {
        !w.events.lock().unwrap().is_empty()
    });
    w.events.lock().unwrap()[0].shutdown();
    let (a, b) = (
        lost.recv_timeout(WAIT).unwrap(),
        lost.recv_timeout(WAIT).unwrap(),
    );
    assert_eq!(a, b);
    assert!(!first.alive());
    assert!(
        lost.recv_timeout(Duration::from_millis(200)).is_err(),
        "said once to each"
    );
    assert!(!w.manager.status().running);
    // Closing what a dead helper had is no error and starts nothing.
    drop(first);
    let again = w.manager.open(&key(None), SIZE, 1.0, sink).unwrap();
    assert!(again.alive());
    assert_eq!(w.spawned.load(Ordering::SeqCst), 2);
    std::fs::remove_dir_all(&w.root).unwrap();
}

#[test]
fn a_helper_with_the_wrong_key_is_refused_and_ended() {
    let w = world(
        "key",
        Behaviour {
            wrong_key: true,
            ..Default::default()
        },
        Some("0.1.0"),
        LINGER,
    );
    let (sink, _events, _lost) = log();
    let error = w
        .manager
        .open(&key(None), SIZE, 1.0, sink)
        .err()
        .expect("a refusal");
    assert!(matches!(error, StartError::Io(_)), "{error}");
    assert!(!w.manager.status().running);
    std::fs::remove_dir_all(&w.root).unwrap();
}

#[test]
fn a_helper_that_does_not_connect_is_a_failed_start_not_a_hang() {
    let w = world(
        "silent",
        Behaviour {
            silent: true,
            ..Default::default()
        },
        Some("0.1.0"),
        LINGER,
    );
    let (sink, _events, _lost) = log();
    let started = Instant::now();
    let error = w
        .manager
        .open(&key(None), SIZE, 1.0, sink)
        .err()
        .expect("a failure");
    assert!(matches!(error, StartError::Io(_)), "{error}");
    assert!(started.elapsed() < Duration::from_secs(15));
    std::fs::remove_dir_all(&w.root).unwrap();
}

#[test]
fn nothing_installed_is_a_state_not_a_failure_and_nothing_starts() {
    let w = world("none", Behaviour::default(), None, LINGER);
    assert!(!w.manager.installed());
    let (sink, _events, _lost) = log();
    let error = w
        .manager
        .open(&key(None), SIZE, 1.0, sink)
        .err()
        .expect("not installed");
    assert!(matches!(error, StartError::NotInstalled), "{error}");
    assert_eq!(w.spawned.load(Ordering::SeqCst), 0);
    std::fs::remove_dir_all(&w.root).unwrap();
}

#[test]
fn an_app_that_is_not_trusted_is_never_run() {
    let w = world("trust", Behaviour::default(), Some("0.1.0"), LINGER);
    let config = Config {
        trust: Arc::new(|app| Err(format!("{} is not signed by us", app.display()))),
        ..w.manager.config.clone()
    };
    let manager = Manager::new(config, w.manager.spawn.clone());
    let (sink, _events, _lost) = log();
    let error = manager
        .open(&key(None), SIZE, 1.0, sink)
        .err()
        .expect("untrusted");
    assert!(matches!(error, StartError::Untrusted(_)), "{error}");
    assert_eq!(w.spawned.load(Ordering::SeqCst), 0);
    std::fs::remove_dir_all(&w.root).unwrap();
}

#[test]
fn the_newest_installed_version_runs_and_the_older_one_is_deleted() {
    let w = world("newest", Behaviour::default(), Some("0.1.0"), LINGER);
    let layout = Layout::new(w.root.join("helper"));
    let staging = layout.staging().unwrap();
    std::fs::create_dir_all(staging.path().join(APP)).unwrap();
    layout
        .commit(
            &staging,
            &Version::parse("0.2.0").unwrap(),
            &Version::zero(),
        )
        .unwrap();
    let (sink, _events, _lost) = log();
    let _browser = w.manager.open(&key(None), SIZE, 1.0, sink).unwrap();
    assert_eq!(layout.installed(), vec![Version::parse("0.2.0").unwrap()]);
    std::fs::remove_dir_all(&w.root).unwrap();
}

#[test]
fn a_profile_with_a_name_is_one_for_every_browser_of_its_key_and_an_unnamed_one_is_its_own() {
    let w = world(
        "profiles",
        Behaviour::default(),
        Some("0.1.0"),
        Duration::from_secs(60),
    );
    let (sink, _events, _lost) = log();
    let named = key(Some("work"));
    let _a = w.manager.open(&named, SIZE, 1.0, sink.clone()).unwrap();
    let _b = w.manager.open(&named, SIZE, 1.0, sink.clone()).unwrap();
    let _c = w.manager.open(&key(None), SIZE, 1.0, sink.clone()).unwrap();
    let _d = w.manager.open(&key(None), SIZE, 1.0, sink).unwrap();
    let profiles: Vec<Profile> = opens(&w, 4)
        .into_iter()
        .map(|command| match command {
            Command::Open { profile, .. } => profile,
            _ => unreachable!(),
        })
        .collect();
    assert_eq!(
        profiles[0], profiles[1],
        "the same key is the same context, folder and credential"
    );
    assert_eq!(profiles[0].dir, named.dir());
    assert_eq!(profiles[0].context, profiles[0].dir.clone().unwrap());
    assert_eq!(profiles[2].dir, None);
    assert_ne!(profiles[2].context, profiles[3].context);
    assert_ne!(profiles[2].pass, profiles[3].pass);
    assert_ne!(profiles[0].pass, profiles[2].pass);
    std::fs::remove_dir_all(&w.root).unwrap();
}

#[test]
fn the_credentials_a_profile_is_given_are_the_ones_the_proxy_knows() {
    let w = world(
        "proxy",
        Behaviour::default(),
        Some("0.1.0"),
        Duration::from_secs(60),
    );
    let (sink, _events, _lost) = log();
    let _browser = w.manager.open(&key(Some("work")), SIZE, 1.0, sink).unwrap();
    let Command::Open { profile, .. } = opens(&w, 1).remove(0) else {
        unreachable!()
    };
    let args = w.args.recv_timeout(WAIT).unwrap();
    let port: u16 = args
        .iter()
        .find_map(|a| a.strip_prefix("--proxy=http://127.0.0.1:"))
        .unwrap()
        .parse()
        .unwrap();
    let ask = |authorization: Option<String>| {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream.set_read_timeout(Some(WAIT)).unwrap();
        let header = authorization.map_or_else(String::new, |value| {
            format!("Proxy-Authorization: Basic {value}\r\n")
        });
        stream
            .write_all(format!("CONNECT localhost:3000 HTTP/1.1\r\n{header}\r\n").as_bytes())
            .unwrap();
        let mut answer = String::new();
        stream.read_to_string(&mut answer).unwrap();
        answer
    };
    assert!(ask(None).starts_with("HTTP/1.1 407"));
    let encoded = {
        // The proxy's own encoder is a test's; the credentials are plain ASCII.
        const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let text = format!("{}:{}", profile.user, profile.pass);
        let mut out = String::new();
        for chunk in text.as_bytes().chunks(3) {
            let n =
                chunk.iter().fold(0u32, |n, b| (n << 8) | u32::from(*b)) << (8 * (3 - chunk.len()));
            for i in 0..=chunk.len() {
                out.push(ALPHABET[((n >> (18 - 6 * i)) & 63) as usize] as char);
            }
            out.push_str(&"=".repeat(3 - chunk.len()));
        }
        out
    };
    assert!(ask(Some(encoded)).starts_with("HTTP/1.1 502"));
    std::fs::remove_dir_all(&w.root).unwrap();
}

#[test]
fn a_download_progress_reaches_the_browser_that_asked() {
    let w = world(
        "download",
        Behaviour::default(),
        Some("0.1.0"),
        Duration::from_secs(60),
    );
    let (sink_a, events_a, _) = log();
    let (sink_b, events_b, _) = log();
    let a = w.manager.open(&key(None), SIZE, 1.0, sink_a).unwrap();
    let b = w.manager.open(&key(None), SIZE, 1.0, sink_b).unwrap();
    for events in [&events_a, &events_b] {
        assert!(matches!(
            events.recv_timeout(WAIT).unwrap(),
            Event::Opened { .. }
        ));
    }
    wait_until("the helper is connected", || {
        !w.events.lock().unwrap().is_empty()
    });
    let helper = w.events.lock().unwrap()[0].clone();
    let download = |browser, req| Event::Download {
        browser,
        req,
        url: "http://localhost/f".into(),
        name: "f".into(),
        mime: "text/plain".into(),
        size: Some(3),
    };
    let progress = |req, state| Event::DownloadProgress {
        req,
        state,
        received: 1,
        total: Some(3),
    };
    helper.send(&download(b.id(), 7)).unwrap();
    helper.send(&progress(7, DownloadState::Running)).unwrap();
    helper.send(&progress(7, DownloadState::Done)).unwrap();
    for expected in [
        download(b.id(), 7),
        progress(7, DownloadState::Running),
        progress(7, DownloadState::Done),
    ] {
        assert_eq!(events_b.recv_timeout(WAIT).unwrap(), expected);
    }
    assert!(
        events_a.recv_timeout(Duration::from_millis(200)).is_err(),
        "not the other browser's"
    );
    // A progress of a download that is over has nobody to go to.
    helper.send(&progress(7, DownloadState::Running)).unwrap();
    assert!(events_b.recv_timeout(Duration::from_millis(200)).is_err());
    drop((a, b));
    std::fs::remove_dir_all(&w.root).unwrap();
}

#[test]
fn a_browser_may_only_command_itself() {
    let w = world(
        "own",
        Behaviour::default(),
        Some("0.1.0"),
        Duration::from_secs(60),
    );
    let (sink, _events, _lost) = log();
    let a = w.manager.open(&key(None), SIZE, 1.0, sink.clone()).unwrap();
    let b = w.manager.open(&key(None), SIZE, 1.0, sink).unwrap();
    assert!(a.send(Command::Back { browser: a.id() }).is_ok());
    let error = a.send(Command::Back { browser: b.id() }).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    assert!(
        a.send(Command::MenuResult {
            req: 1,
            command: None
        })
        .is_ok()
    );
    std::fs::remove_dir_all(&w.root).unwrap();
}

#[test]
fn a_fixed_app_is_run_where_it_is_and_nothing_is_installed_around_it() {
    let root = temp("fixed");
    let app = root.join("dev").join(APP);
    std::fs::create_dir_all(&app).unwrap();
    let w = world("fixed-fake", Behaviour::default(), None, LINGER);
    let config = Config {
        source: Source::Fixed(app),
        profiles: root.join("profiles"),
        run: root.join("run"),
        ..w.manager.config.clone()
    };
    let manager = Manager::new(config, w.manager.spawn.clone());
    assert!(manager.installed());
    let (sink, _events, _lost) = log();
    let _browser = manager.open(&key(None), SIZE, 1.0, sink).unwrap();
    assert_eq!(w.spawned.load(Ordering::SeqCst), 1);
    std::fs::remove_dir_all(&root).unwrap();
    std::fs::remove_dir_all(&w.root).unwrap();
}
