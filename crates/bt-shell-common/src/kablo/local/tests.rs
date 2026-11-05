//! This Mac's account against a real kablo: the release the sibling repository builds
//! (`../kablo/dist`, or `KABLO_DIST`), installed below a home directory of the test's, its
//! sessions in a socket directory of the test's. Without that release the real tests say
//! `SKIPPED` and pass: a checkout of bateri alone has no kablo to run.

use std::io::{Read, Write};
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use kablo_proto::SessionId;

use super::*;
use crate::kablo::install::{self, Bundle, Failure, ensure_here, prune_versions, unused_versions};
use crate::kablo::ledger::Ledger;
use crate::kablo::service::{Backend, PaneInfo, Peer, Refusal, Service};
use crate::kablo::{Attach, Client, Event, Exit, NewSession, Size};
use crate::settings::TempRoot;

#[test]
fn this_macs_account_is_its_own_and_is_no_servers() {
    assert!(is_account(ACCOUNT));
    // A server's key is `user@host:port`, or the digits of a hash: never this.
    for server in [
        "ada@rasp:22",
        "ada@rasp:22 via bastion:22",
        "00ff00ff00ff00ff",
        "",
    ] {
        assert!(!is_account(server), "{server:?}");
    }
    let host = host();
    assert_eq!(host.key, ACCOUNT);
    assert!(host.argv.is_empty() && host.host.is_empty() && host.port.is_none());
    assert!(!ACCOUNT.contains('@') && !ACCOUNT.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn a_step_here_is_sh_in_the_home_directory() {
    let root = TempRoot::new("local-channel");
    let home = root.0.join("mac");
    std::fs::create_dir_all(&home).expect("home");
    let channel = LocalChannel::new(home.clone());

    let said = channel.run("pwd", &[]).expect("pwd");
    assert_eq!(said.status, Some(0));
    assert_eq!(
        std::fs::canonicalize(said.out.trim()).expect("a directory"),
        std::fs::canonicalize(&home).expect("home")
    );
    // What the step is given on its standard input, it reads; its status is the script's.
    let echoed = channel.run("cat; exit 3", b"bytes").expect("cat");
    assert_eq!((echoed.status, echoed.out.as_str()), (Some(3), "bytes"));
    // A step that takes too long is ended, not waited for.
    let slow = LocalChannel {
        home,
        limit: Duration::from_millis(100),
    };
    let error = slow.run("sleep 20", &[]).expect_err("too slow");
    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
}

#[test]
#[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
fn a_bridge_here_is_the_installed_kablo_run_as_attach_and_ending_it_ends_only_it() {
    let root = TempRoot::new("local-bridge");
    let home = root.0.join("mac");
    let dir = home.join(".local/share/kablo/0.2.0");
    std::fs::create_dir_all(&dir).expect("version");
    // A kablo that is a script, whose `attach` echoes what it is given.
    std::fs::write(
        dir.join("kablo"),
        "#!/bin/sh\ncase $1 in attach) exec cat;; esac\n",
    )
    .expect("kablo");
    std::fs::set_permissions(dir.join("kablo"), std::fs::Permissions::from_mode(0o755))
        .expect("runnable");
    let installed = Installed {
        target: kablo_proto::install::Target::MacosAarch64,
        version: "0.2.0".into(),
        path: ".local/share/kablo/0.2.0/kablo".into(),
        fresh: false,
    };

    let Carried {
        mut reader,
        mut writer,
        close,
        ended,
    } = carry(&home, &installed).expect("a bridge");
    writer.write_all(b"ping\n").expect("written");
    let mut back = [0u8; 5];
    reader.read_exact(&mut back).expect("read");
    assert_eq!(&back, b"ping\n");
    drop(writer);
    // Its stdin closed, `cat` ends: the stream ends, and nothing says more than "dropped".
    let mut rest = Vec::new();
    reader.read_to_end(&mut rest).expect("end");
    assert!(rest.is_empty());
    assert_eq!(ended(), Cause::Dropped);
    close();

    // A home that is not an absolute path names no bridge.
    assert!(carry(Path::new("relative"), &installed).is_err());
    // And a kablo that is not installed is a bridge that cannot be started.
    assert!(carry(&root.0.join("nowhere"), &installed).is_err());
}

// ─── a real kablo ────────────────────────────────────────────────────────

/// kablo's release, if there is one to run.
fn release() -> Option<Bundle> {
    let dist = std::env::var_os("KABLO_DIST")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../kablo/dist"));
    Bundle::open(&dist).ok()
}

/// A private directory with a path short enough for a socket, and what has to be ended when a
/// test that made terminals ends: its own, by name, never anybody else's.
struct Ground {
    root: PathBuf,
    sockets: PathBuf,
    home: PathBuf,
    /// The kablo that was installed, and the terminals made from it.
    kept: Option<PathBuf>,
    made: Vec<SessionId>,
}

impl Ground {
    fn new(name: &str) -> Ground {
        let root = PathBuf::from(format!("/tmp/bt-lk-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&root)
            .expect("a private directory");
        let sockets = root.join("s");
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&sockets)
            .expect("the socket directory");
        let home = root.join("h");
        std::fs::create_dir(&home).expect("home");
        Ground {
            root,
            sockets,
            home,
            kept: None,
            made: Vec::new(),
        }
    }

    /// The bridge, with the environment that keeps every file of it in this ground.
    fn bridge(&self, installed: &Installed, version: Option<&str>) -> Command {
        let mut command = bridge_command(&self.home, installed).expect("a command");
        command
            .env(sockdir_variable(), &self.sockets)
            // An ended terminal is kept for a client that asks for ten minutes: not here.
            .env("KABLO_TEST_END_KEEP_MS", "200");
        if let Some(version) = version {
            command.env("KABLO_TEST_VERSION", version);
        }
        command
    }

    /// kablo's own command line, for what a test asks of it by hand.
    fn run(&self, installed: &Installed, args: &[&str]) -> (Option<i32>, String) {
        let output = Command::new(self.home.join(&installed.path))
            .args(args)
            .env(sockdir_variable(), &self.sockets)
            .stdin(Stdio::null())
            .output()
            .expect("kablo");
        (
            output.status.code(),
            String::from_utf8_lossy(&output.stdout).into_owned(),
        )
    }
}

fn sockdir_variable() -> &'static str {
    "KABLO_DIR"
}

impl Drop for Ground {
    fn drop(&mut self) {
        if let Some(binary) = &self.kept {
            for id in &self.made {
                let _ = Command::new(binary)
                    .args(["end", "--force", id.as_str()])
                    .env(sockdir_variable(), &self.sockets)
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status();
            }
        }
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// A [`Client`] over a bridge it started, driven by the test: the bytes between them, what
/// happened, and what the terminals wrote.
struct Wire {
    client: Client,
    to: Box<dyn Write + Send>,
    from: mpsc::Receiver<Vec<u8>>,
    close: Option<Box<dyn FnOnce() + Send>>,
    seen: Vec<Event>,
    text: Vec<u8>,
}

impl Wire {
    fn open(command: Command) -> Wire {
        let Carried {
            mut reader,
            writer,
            close,
            ..
        } = start(command).expect("a bridge");
        let (tx, from) = mpsc::channel();
        thread::spawn(move || {
            let mut buffer = [0u8; 8192];
            while let Ok(n) = reader.read(&mut buffer) {
                if n == 0 || tx.send(buffer[..n].to_vec()).is_err() {
                    break;
                }
            }
        });
        let mut client = Client::new(Some("a test"), Instant::now());
        client.connected(Instant::now());
        Wire {
            client,
            to: writer,
            from,
            close: Some(close),
            seen: Vec::new(),
            text: Vec::new(),
        }
    }

    /// Carries what the client has to say to the bridge and what the bridge said to the client,
    /// waiting at most `wait` for something to come.
    fn pump(&mut self, wait: Duration) {
        let out = self.client.take_outgoing();
        if !out.is_empty() {
            self.to.write_all(&out).expect("written to the bridge");
            self.to.flush().expect("flushed");
        }
        if let Ok(bytes) = self.from.recv_timeout(wait) {
            self.client.push(&bytes, Instant::now());
            while let Ok(more) = self.from.try_recv() {
                self.client.push(&more, Instant::now());
            }
        }
        self.client.tick(Instant::now());
        while let Some(event) = self.client.next_event() {
            if let Event::Output { data, .. } | Event::Picture { data, .. } = &event {
                self.text.extend_from_slice(data);
            }
            self.seen.push(event);
        }
    }

    fn until(&mut self, what: &str, done: impl Fn(&Wire) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(20);
        while Instant::now() < deadline {
            if done(self) {
                return;
            }
            self.pump(Duration::from_millis(20));
        }
        panic!(
            "{what}: {:?} / {:?}",
            self.seen,
            String::from_utf8_lossy(&self.text)
        );
    }

    fn says(&self, word: &str) -> bool {
        String::from_utf8_lossy(&self.text).contains(word)
    }

    fn has(&self, wanted: impl Fn(&Event) -> bool) -> bool {
        self.seen.iter().any(wanted)
    }

    /// The bridge goes; the terminals it served are not its.
    fn hang_up(mut self) {
        if let Some(close) = self.close.take() {
            close();
        }
    }
}

const SIZE: Size = Size {
    cols: 80,
    rows: 24,
    xpixel: 0,
    ypixel: 0,
};

#[test]
#[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
fn a_real_kablo_is_installed_here_keeps_a_terminal_past_its_bridge_and_is_read_by_the_census() {
    let Some(bundle) = release() else {
        eprintln!("SKIPPED: there is no kablo release to run (../kablo/dist or KABLO_DIST)");
        return;
    };
    let mut ground = Ground::new("real");
    let channel = LocalChannel::new(ground.home.clone());
    let installed = match ensure_here(&channel, &bundle, true) {
        Ok(installed) => installed,
        Err(Failure::Impossible(why)) => {
            eprintln!("SKIPPED: this machine is not one kablo is built for: {why:?}");
            return;
        }
        Err(other) => panic!("kablo was not installed: {other:?}"),
    };
    assert!(installed.fresh);
    ground.kept = Some(ground.home.join(&installed.path));
    // The file is the release's own, under the version's name, in the test's home.
    assert!(ground.home.join(&installed.path).is_file());
    let (status, said) = ground.run(&installed, &["version"]);
    assert_eq!(status, Some(0));
    assert!(
        said.contains(&format!("version={}", installed.version)),
        "{said}"
    );
    assert!(!ensure_here(&channel, &bundle, true).expect("found").fresh);

    // A terminal through the bridge: made, written to, read from.
    let id = SessionId::new("t1").expect("an id");
    ground.made.push(id.clone());
    // The relay and the warden of this terminal call themselves another release, as an older
    // one's would: the census must find the version it names in the listing's own words.
    let mut wire = Wire::open(ground.bridge(&installed, Some("0.0.9-old")));
    wire.until("the link", |w| w.has(|e| matches!(e, Event::Linked)));
    let mut spec = NewSession::default();
    spec.argv = vec![b"/bin/cat".to_vec()];
    wire.client
        .new_terminal(id.clone(), spec, Attach::new(SIZE))
        .expect("a request");
    wire.until("the terminal", |w| {
        w.has(|e| matches!(e, Event::Attached { terminal, .. } if *terminal == id))
    });
    assert!(wire.client.input(&id, b"hello\n"));
    wire.until("the echo", |w| w.says("hello"));

    // The census reads what the real listing says: the version a relay and a warden run from.
    for old in ["0.0.9-old", "0.0.1"] {
        let dir = ground.home.join(format!(".local/share/kablo/{old}"));
        std::fs::create_dir_all(&dir).expect("an old version");
        std::fs::write(dir.join("kablo"), "#!/bin/sh\n").expect("its binary");
        std::fs::set_permissions(dir.join("kablo"), std::fs::Permissions::from_mode(0o755))
            .expect("runnable");
        let status = Command::new("touch")
            .args(["-t", "202001010000"])
            .arg(&dir)
            .status()
            .expect("touch");
        assert!(status.success());
    }
    let channel = LocalChannel {
        limit: install::UPGRADE_LIMIT,
        ..LocalChannel::new(ground.home.clone())
    };
    // The listing is of the test's own socket directory, never the user's.
    let channel = EnvChannel {
        inner: channel,
        sockets: ground.sockets.clone(),
    };
    let census = channel
        .run(&install::census_script(&installed.version), &[])
        .expect("the census");
    assert_eq!(census.status, Some(0), "{}", census.err);
    assert!(
        census.out.contains("\"warden_version\":\"0.0.9-old\""),
        "{}",
        census.out
    );
    assert_eq!(
        unused_versions(&census, &installed.version),
        ["0.0.1"],
        "the version a terminal runs from stays, the one nothing runs from goes"
    );
    assert_eq!(prune_versions(&channel, &installed), ["0.0.1"]);
    assert!(!ground.home.join(".local/share/kablo/0.0.1").exists());
    assert!(ground.home.join(".local/share/kablo/0.0.9-old").exists());
    assert!(ground.home.join(&installed.path).exists());

    // The bridge goes, and the terminal is still there: a bridge made after it finds it, with
    // what it wrote, and the program ends the way a program ends.
    wire.hang_up();
    let (status, listed) = ground.run(&installed, &["list", "--json"]);
    assert_eq!(status, Some(0));
    assert!(listed.contains("\"id\":\"t1\""), "{listed}");
    let mut again = Wire::open(ground.bridge(&installed, None));
    again.until("the link", |w| w.has(|e| matches!(e, Event::Linked)));
    again.client.attach(id.clone(), Attach::new(SIZE).from(0));
    again.until("the screen it kept", |w| w.says("hello"));
    assert!(again.client.input(&id, &[4]));
    again.until("the end", |w| {
        w.has(|e| {
            matches!(
                e,
                Event::Ended {
                    exit: Exit::Code(0),
                    ..
                }
            )
        })
    });
    again.hang_up();
}

/// A [`Channel`] that points the machine's kablo at the test's socket directory.
struct EnvChannel {
    inner: LocalChannel,
    sockets: PathBuf,
}

impl Channel for EnvChannel {
    fn run(&self, script: &str, input: &[u8]) -> io::Result<Reply> {
        let script = format!(
            "{}={}; export {}; {script}",
            sockdir_variable(),
            crate::upload::sq(&self.sockets.to_string_lossy()),
            sockdir_variable()
        );
        self.inner.run(&script, input)
    }
}

// ─── a pane over a real kablo ────────────────────────────────────────────

/// This Mac for the service, with the release the test has, below the test's own home and with
/// the test's own socket directory for everything kablo makes.
struct RealMac {
    bundle: Bundle,
    home: PathBuf,
    sockets: PathBuf,
    streams: std::sync::OnceLock<crate::kablo::service::Streams>,
}

impl Backend for RealMac {
    fn pane(&self, _: &Peer) -> Option<PaneInfo> {
        None
    }

    fn route(&self, _: &[String]) -> Option<KabloHost> {
        None
    }

    fn master(&self, _: &KabloHost) -> Result<Option<Vec<Vec<u8>>>, Refusal> {
        Ok(None)
    }

    fn master_made(&self, _: &KabloHost, _: u8) -> Result<(), Refusal> {
        Ok(())
    }

    fn install(&self, _: &KabloHost, _: &[SessionId]) -> Result<Installed, Refusal> {
        ensure_here(&LocalChannel::new(self.home.clone()), &self.bundle, true)
            .map_err(|_| Refusal::plain())
    }

    fn version(&self) -> Option<String> {
        Some(self.bundle.version().to_owned())
    }

    fn carry(&self, _: &KabloHost, installed: &Installed) -> io::Result<Carried> {
        let mut command = bridge_command(&self.home, installed)?;
        command
            .env(sockdir_variable(), &self.sockets)
            .env("KABLO_TEST_END_KEEP_MS", "200");
        start(command)
    }

    fn serving(&self, streams: crate::kablo::service::Streams) {
        let _ = self.streams.set(streams);
    }

    // This Mac's connection needs no ssh to be there: the account's key is all the service asks.
    fn command(
        &self,
        server: &KabloHost,
        command: kablo_proto::v1::stream::Command,
        _: crate::ssh_route::Ask,
        wait: Duration,
    ) -> Result<crate::kablo::remote::RemoteCommand, String> {
        self.streams
            .get()
            .ok_or_else(|| "the service is not running".to_owned())?
            .command(&server.key, command, wait)
    }
}

/// What a pane's session says to its owner: only the end of the program is kept.
#[derive(Default)]
struct Ends {
    exit: Mutex<Option<Option<i32>>>,
    changed: std::sync::Condvar,
}

impl Ends {
    fn waits(&self) -> Option<Option<i32>> {
        let exit = self.exit.lock().expect("lock");
        let (exit, _) = self
            .changed
            .wait_timeout_while(exit, Duration::from_secs(20), |exit| exit.is_none())
            .expect("lock");
        *exit
    }
}

impl bt_core::Wake for Ends {
    fn wake(&self) {}
    fn child_exit(&self, code: Option<i32>) {
        *self.exit.lock().expect("lock") = Some(code);
        self.changed.notify_all();
    }
    fn copy_to_clipboard(&self, _: String) {}
    fn title_changed(&self) {}
    fn search_changed(&self) {}
    fn command_started(&self) {}
    fn phase_edge(&self) {}
    fn remote_command_edge(&self) {}
    fn mirror_changed(&self) {}
    fn environment_changed(&self) {}
    fn remote_up(&self) {}
    fn remote_typed(&self) {}
    fn link_hover_lost(&self) {}
    fn pointer_changed(&self) {}
    fn blocks_changed(&self) {}
    fn unseen_changed(&self) {}
    fn program_status_changed(&self) {}
    fn notification(&self) {}
}

/// The grid's text, one row to a line.
fn screen(session: &bt_core::Session) -> String {
    let mut rows: Vec<Vec<char>> = Vec::new();
    session.frame(
        |cell| {
            let (row, col) = (usize::from(cell.row), usize::from(cell.col));
            if rows.len() <= row {
                rows.resize(row + 1, Vec::new());
            }
            if rows[row].len() <= col {
                rows[row].resize(col + 1, ' ');
            }
            rows[row][col] = cell.ch.unwrap_or(' ');
        },
        |_| (),
        &mut bt_core::Blocks::default(),
        &mut bt_core::SelectionRuns::default(),
        &mut bt_core::SearchRuns::default(),
        &mut bt_core::TrackMarks::default(),
        &mut bt_core::Clusters::default(),
        &mut bt_core::Pictures::default(),
        bt_core::ScrollGlide::default(),
        bt_core::DockBudget {
            share: 0.5,
            cols: 80,
        },
    );
    rows.iter()
        .map(|row| row.iter().collect::<String>().trim_end().to_owned())
        .collect::<Vec<_>>()
        .join("\n")
}

fn shows(session: &bt_core::Session, needle: &str) -> bool {
    screen(session).contains(needle)
}

fn waits_for(what: &str, mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        if ready() {
            return;
        }
        thread::sleep(Duration::from_millis(10));
    }
    panic!("{what}");
}

#[test]
#[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
fn a_pane_over_a_real_kablo_shows_what_the_program_writes_types_into_it_is_sized_by_it_and_hears_its_end()
 {
    let Some(bundle) = release() else {
        eprintln!("SKIPPED: there is no kablo release to run (../kablo/dist or KABLO_DIST)");
        return;
    };
    let mut ground = Ground::new("pane");
    let service_dir = ground.root.join("svc");
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&service_dir)
        .expect("the service's directory");
    let kablo =
        kablo_proto::install::install_path_in(&ground.home.to_string_lossy(), bundle.version())
            .expect("a path");
    let backend = RealMac {
        bundle,
        home: ground.home.clone(),
        sockets: ground.sockets.clone(),
        streams: Default::default(),
    };
    let ledger = Arc::new(Ledger::memory());
    let service =
        Service::start(&service_dir, Arc::new(backend), Arc::clone(&ledger)).expect("a service");

    // The program: says the identity it was given, then its size, waits for a line, says it
    // again and the line, and ends with 3.
    let script = r#"env | grep -E '^(TERM_PROGRAM|TERM_SESSION_ID|BATERI_TAB_URL|LC_BATERI_TAB_URL|LC_TERMINAL|TERM|COLORTERM|PWD)=' | sort | sed 's/^/ENV:/'
pixels() { command -v python3 >/dev/null && python3 -c "import fcntl,termios,struct;print('PIX:%d,%d'%struct.unpack('HHHH',fcntl.ioctl(0,termios.TIOCGWINSZ,bytes(8)))[2:])" || echo PIX:none; }
printf 'READY\n'; stty size; pixels; read line; printf 'GOT:%s\n' "$line"; stty size; pixels; exit 3"#;
    let options = bt_core::SessionOptions {
        command: Some((
            "/bin/sh".to_owned(),
            vec!["-c".to_owned(), script.to_owned()],
        )),
        working_directory: Some(ground.home.clone()),
        home: None,
        env: std::collections::HashMap::new(),
        cols: 80,
        rows: 24,
        cell_px: (9, 18),
        terminal: bt_core::TerminalOptions {
            scrollback: 100,
            osc52: bt_core::Osc52::Off,
            cursor: bt_core::CaretShape::default(),
            blink: bt_core::CursorBlink::default(),
        },
        theme: bt_core::Theme::BATERI,
        dock: false,
        cluster: true,
        initial_input: None,
        shell_marks: false,
        pane_uuid: bt_core::PaneUuid::parse("5A1E2D3C-4B5A-6978-8796-A5B4C3D2E1F0"),
        hostname: None,
        replay: None,
        views: None,
    };
    let (spec, size) = new_session(&options.child_launch(), 1000);
    assert_eq!(
        (size.cols, size.rows, size.xpixel, size.ypixel),
        (80, 24, 720, 432),
        "the cells and the pixels the program is told"
    );
    let pane = options.pane_uuid.clone().expect("a pane");
    let stream = service
        .open_local(&pane, spec, size, Duration::from_secs(30))
        .expect("a terminal of this Mac");
    // The terminal is the Mac's, in the ledger with the pane, and the program's id is known.
    let id = ledger.terminals_of(ACCOUNT).remove(0);
    ground.kept = Some(PathBuf::from(kablo));
    ground.made.push(id.clone());
    assert!(stream.pid > 1);

    let ends = Arc::new(Ends::default());
    let session =
        bt_core::Session::streamed(options, Arc::clone(&ends) as Arc<dyn bt_core::Wake>, stream)
            .expect("a session over the stream");
    assert!(session.is_streamed());

    // What the program writes is on the grid, and the size it has is the grid's.
    waits_for("the program's first words", || shows(&session, "READY"));
    waits_for("its size", || shows(&session, "24 80"));
    // The pixels of the cells the session has are the terminal's (the program reads them with
    // `TIOCGWINSZ`); a machine without python3 says so and the claim is not made.
    waits_for("its pixels", || shows(&session, "PIX:"));
    let has_python = !shows(&session, "PIX:none");
    if has_python {
        assert!(shows(&session, "PIX:720,432"), "{}", screen(&session));
    }
    // The identity the pane gave the program is what it got: the outward contract's variables
    // are the session's, kablo carries them as they are.
    let screen_now = screen(&session);
    for line in [
        "ENV:BATERI_TAB_URL=bateri://tab/5A1E2D3C-4B5A-6978-8796-A5B4C3D2E1F0",
        "ENV:LC_BATERI_TAB_URL=bateri://tab/5A1E2D3C-4B5A-6978-8796-A5B4C3D2E1F0",
        "ENV:LC_TERMINAL=bateri",
        "ENV:TERM=xterm-256color",
        "ENV:COLORTERM=truecolor",
        "ENV:TERM_PROGRAM=bateri",
        "ENV:TERM_SESSION_ID=5A1E2D3C-4B5A-6978-8796-A5B4C3D2E1F0",
    ] {
        assert!(screen_now.contains(line), "{line}\n{screen_now}");
    }

    // A new size reaches the program: the terminal is sized through the service.
    assert!(session.resize(50, 12, (7, 15)));
    // What the pane types reaches the program.
    session.write(b"typed\r");
    waits_for("the typed line", || shows(&session, "GOT:typed"));
    waits_for("the new size", || shows(&session, "12 50"));
    if has_python {
        waits_for("the new pixels", || shows(&session, "PIX:350,180"));
    }
    // The program ends with 3: the session is told, after the last of its bytes.
    assert_eq!(ends.waits(), Some(Some(3)));
    assert!(shows(&session, "GOT:typed"));
    drop(session);
    service.stop();
}

/// The options of a session whose program is `script` run by `/bin/sh`, the way a pane's shell is
/// started on this machine: behind `login(1)` on macOS (the program is its child, and `login`
/// is root's), the program itself elsewhere.
fn behind_login(
    script: &str,
    home: &std::path::Path,
) -> (bt_core::SessionOptions, crate::jobs::ShellParent) {
    let (command, parent) = if cfg!(target_os = "macos") {
        let user = std::env::var("USER").unwrap_or_else(|_| "nobody".to_owned());
        (
            (
                "/usr/bin/login".to_owned(),
                vec![
                    "-qflp".to_owned(),
                    user,
                    "/bin/sh".to_owned(),
                    "-c".to_owned(),
                    script.to_owned(),
                ],
            ),
            crate::jobs::ShellParent::Login,
        )
    } else {
        (
            (
                "/bin/sh".to_owned(),
                vec!["-c".to_owned(), script.to_owned()],
            ),
            crate::jobs::ShellParent::Direct,
        )
    };
    let options = bt_core::SessionOptions {
        command: Some(command),
        working_directory: Some(home.to_owned()),
        home: None,
        env: std::collections::HashMap::new(),
        cols: 80,
        rows: 24,
        cell_px: (9, 18),
        terminal: bt_core::TerminalOptions {
            scrollback: 100,
            osc52: bt_core::Osc52::Off,
            cursor: bt_core::CaretShape::default(),
            blink: bt_core::CursorBlink::default(),
        },
        theme: bt_core::Theme::BATERI,
        dock: false,
        cluster: true,
        initial_input: None,
        shell_marks: false,
        pane_uuid: bt_core::PaneUuid::parse("5A1E2D3C-4B5A-6978-8796-A5B4C3D2E1F1"),
        hostname: None,
        replay: None,
        views: None,
    };
    (options, parent)
}

#[test]
#[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
fn a_pane_over_a_real_kablo_is_found_and_read_by_its_childs_pid_behind_login_too() {
    let Some(bundle) = release() else {
        eprintln!("SKIPPED: there is no kablo release to run (../kablo/dist or KABLO_DIST)");
        return;
    };
    let mut ground = Ground::new("tty");
    let service_dir = ground.root.join("svc");
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&service_dir)
        .expect("the service's directory");
    let kablo =
        kablo_proto::install::install_path_in(&ground.home.to_string_lossy(), bundle.version())
            .expect("a path");
    let backend = RealMac {
        bundle,
        home: ground.home.clone(),
        sockets: ground.sockets.clone(),
        streams: Default::default(),
    };
    let ledger = Arc::new(Ledger::memory());
    let service =
        Service::start(&service_dir, Arc::new(backend), Arc::clone(&ledger)).expect("a service");

    // A password prompt's modes (canonical, no echo), then a wait.
    let (options, parent) = behind_login(
        "stty icanon -echo; printf 'READY\\n'; exec sleep 60",
        &ground.home,
    );
    let (spec, size) = new_session(&options.child_launch(), 100);
    let pane = options.pane_uuid.clone().expect("a pane");
    let mut stream = service
        .open_local(&pane, spec, size, Duration::from_secs(30))
        .expect("a terminal of this Mac");
    let id = ledger.terminals_of(ACCOUNT).remove(0);
    ground.kept = Some(PathBuf::from(kablo));
    ground.made.push(id);
    // The stream's ops read the terminal by the program's pid, as a pane makes them.
    stream.ops = Arc::new(crate::jobs::StreamTty::new(stream.ops, parent));
    let ends = Arc::new(Ends::default());
    let session = bt_core::Session::streamed(options, ends as Arc<dyn bt_core::Wake>, stream)
        .expect("a session over the stream");
    waits_for("the program's first words", || shows(&session, "READY"));

    let pid = session.child_pid();
    // What kablo says the pid is: the process `login` runs, the one the pane's shell is the
    // child of — or, where there is no login, the program itself.
    if cfg!(target_os = "macos") {
        use crate::jobs::ProcessTable;
        assert_eq!(crate::jobs::SystemTable.name(pid).as_deref(), Some("login"));
    }
    // The terminal is found by the kernel's record of the pane's process, and it is the one the
    // pane's shell is on.
    let device = crate::jobs::pane_terminal(parent, pid).expect("the pane's terminal");
    assert_eq!(session.terminal_device(), Some(device));
    assert_eq!(crate::jobs::session_device(&session), Some(device));
    // A process of the pane is in the pane: the program the shell was made into.
    let shell = {
        use crate::jobs::ProcessTable;
        crate::jobs::SystemTable
            .children(pid)
            .first()
            .copied()
            .unwrap_or(pid)
    };
    assert_eq!(crate::jobs::controlling_terminal(shell), Some(device));
    // The modes the program set are the ones read, and the shell's group is in front.
    let modes = bt_core::TtyModes {
        canonical: true,
        echo: false,
    };
    waits_for("the program's modes from the outside", || {
        session.tty_modes(crate::jobs::tty_modes) == Some(modes)
    });
    let reading = session.tty_reading().expect("a reading");
    assert_eq!(reading.modes, modes);
    {
        use crate::jobs::ProcessTable;
        assert_eq!(
            reading.foreground,
            crate::jobs::SystemTable
                .groups(shell)
                .map(|groups| groups.own)
        );
    }
    assert!(reading.foreground.is_some_and(|group| group > 1));
    // The questions that were always the process table's work the same by the stream's pid:
    // the shell is in front, and the pane's processes are the shell and what it started.
    assert_eq!(
        crate::jobs::foreground(parent, pid, &crate::jobs::SystemTable),
        crate::jobs::Foreground::Idle
    );
    assert!(!crate::jobs::job_in_foreground(
        parent,
        pid,
        &crate::jobs::SystemTable
    ));
    assert_eq!(
        crate::ports::pane_pids(parent, pid, &crate::jobs::SystemTable, &[])[0],
        shell
    );

    // A program that has ended is not asked: its pid may be another process's now.
    service.end_all(std::slice::from_ref(&pane), Duration::from_secs(10));
    waits_for("the session to see the end", || !session.reader_alive());
    assert_eq!(session.tty_reading(), None);
    assert_eq!(session.terminal_device(), None);
    drop(session);
    service.stop();
}

/// The options of a pane over `/bin/sh` running `script`, without the integration's marks.
fn plain_options(script: &str, home: &Path) -> bt_core::SessionOptions {
    bt_core::SessionOptions {
        command: Some((
            "/bin/sh".to_owned(),
            vec!["-c".to_owned(), script.to_owned()],
        )),
        working_directory: Some(home.to_owned()),
        home: None,
        env: std::collections::HashMap::new(),
        cols: 80,
        rows: 24,
        cell_px: (9, 18),
        terminal: bt_core::TerminalOptions {
            scrollback: 100,
            osc52: bt_core::Osc52::Off,
            cursor: bt_core::CaretShape::default(),
            blink: bt_core::CursorBlink::default(),
        },
        theme: bt_core::Theme::BATERI,
        dock: false,
        cluster: true,
        initial_input: None,
        shell_marks: false,
        pane_uuid: bt_core::PaneUuid::parse("6B2F3E4D-5C6B-4A89-9EA0-B6C5D4E3F201"),
        hostname: None,
        replay: None,
        views: None,
    }
}

#[test]
#[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
fn a_pane_over_a_real_kablo_comes_back_to_its_terminal_with_its_screen_and_the_program_is_told_nothing()
 {
    let Some(bundle) = release() else {
        eprintln!("SKIPPED: there is no kablo release to run (../kablo/dist or KABLO_DIST)");
        return;
    };
    let mut ground = Ground::new("back");
    let service_dir = ground.root.join("svc");
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&service_dir)
        .expect("the service's directory");
    let kablo =
        kablo_proto::install::install_path_in(&ground.home.to_string_lossy(), bundle.version())
            .expect("a path");
    let backend = Arc::new(RealMac {
        bundle,
        home: ground.home.clone(),
        sockets: ground.sockets.clone(),
        streams: Default::default(),
    });
    let ledger = Arc::new(Ledger::memory());

    // The program asks the terminal a question and turns on the pixel mouse, then writes down,
    // byte by byte, every line it is typed — the questions' answers included, which come as
    // input.
    let log = ground.root.join("received");
    let script = format!(
        "printf 'FIRST\\n'; printf '\\033[?1016h'; printf '\\033[c'; \
         while IFS= read -r line; do \
         printf '%s' \"$line\" | od -An -c -v | tr -d ' \\n' >> '{}'; echo >> '{}'; \
         printf 'LINE\\n'; done",
        log.display(),
        log.display()
    );

    // The first bateri: the pane's terminal is made, its question is answered once, the pane
    // types a line.
    let service = Service::start(&service_dir, backend.clone(), ledger.clone()).expect("a service");
    let options = plain_options(&script, &ground.home);
    let (spec, size) = new_session(&options.child_launch(), 1000);
    let pane = options.pane_uuid.clone().expect("a pane");
    let stream = service
        .open_local(&pane, spec, size, Duration::from_secs(30))
        .expect("a terminal of this Mac");
    assert!(stream.silence.is_none(), "a terminal just made has no past");
    let id = ledger.terminals_of(ACCOUNT).remove(0);
    ground.kept = Some(PathBuf::from(kablo));
    ground.made.push(id.clone());
    let session = bt_core::Session::streamed(
        options,
        Arc::new(Ends::default()) as Arc<dyn bt_core::Wake>,
        stream,
    )
    .expect("a session over the stream");
    waits_for("the program's first words", || shows(&session, "FIRST"));
    waits_for("the pixel mouse", || session.pixel_mouse());
    session.write(b"a\r");
    waits_for("the first line", || shows(&session, "LINE"));
    waits_for("the program's log", || {
        std::fs::read_to_string(&log).is_ok_and(|text| text.lines().count() == 1)
    });
    // The bateri goes; the terminal does not.
    drop(session);
    service.stop();

    // The next bateri: the pane finds its terminal, not a new one, and is given the whole of
    // what it wrote. The pane was restored with a line ready for its first prompt (a remote
    // pane's `ssh host`): the program is running already, and the line is not typed into it.
    let service = Service::start(&service_dir, backend, ledger.clone()).expect("a second service");
    let mut options = plain_options(&script, &ground.home);
    options.initial_input = Some(bt_core::InitialInput::run("injected"));
    let (spec, size) = new_session(&options.child_launch(), 1000);
    let stream = service
        .open_local(&pane, spec, size, Duration::from_secs(30))
        .expect("its terminal");
    assert!(stream.pid > 1);
    assert!(stream.silence.is_some(), "a terminal that has a past");
    assert_eq!(ledger.terminals_of(ACCOUNT), [id.clone()]);
    let ends = Arc::new(Ends::default());
    let session =
        bt_core::Session::streamed(options, Arc::clone(&ends) as Arc<dyn bt_core::Wake>, stream)
            .expect("a session over the stream");
    assert!(session.is_shown_again());
    waits_for("the screen of the past", || {
        shows(&session, "FIRST") && shows(&session, "LINE")
    });
    waits_for("the pixel mouse across the restart", || {
        session.pixel_mouse()
    });
    waits_for("the past read", || !session.replaying());

    // What the pane types reaches the program, and what the program has been told is the one
    // answer to its question and the two lines: the past was read without answering again.
    session.write(b"x\r");
    waits_for("the second line", || {
        std::fs::read_to_string(&log).is_ok_and(|text| text.lines().count() == 2)
    });
    let received = std::fs::read_to_string(&log).expect("the log");
    let answers = received.matches("033[?6c").count();
    assert_eq!(answers, 1, "one answer to the one question: {received:?}");
    let lines: Vec<&str> = received.lines().collect();
    assert!(lines[0].ends_with('a'), "{received:?}");
    assert_eq!(lines[1], "x", "nothing but the typed line: {received:?}");
    assert_eq!(lines.len(), 2, "{received:?}");

    service.end_all(std::slice::from_ref(&pane), Duration::from_secs(10));
    waits_for("the session to see the end", || !session.reader_alive());
    drop(session);
    service.stop();
}

// ─── a measurement ───────────────────────────────────────────────────────

/// What a pane does with `view_changed`: a flag, taken on the pane's own turn.
#[derive(Default)]
struct PaneFlag {
    pending: Arc<std::sync::atomic::AtomicBool>,
    /// The program's end was told to the pane ([`bt_core::Wake::child_exit`]).
    exited: Arc<std::sync::atomic::AtomicBool>,
}

impl bt_core::Wake for PaneFlag {
    fn wake(&self) {}
    fn child_exit(&self, _: Option<i32>) {
        self.exited
            .store(true, std::sync::atomic::Ordering::Release);
    }
    fn copy_to_clipboard(&self, _: String) {}
    fn title_changed(&self) {}
    fn search_changed(&self) {}
    fn command_started(&self) {}
    fn phase_edge(&self) {}
    fn remote_command_edge(&self) {}
    fn mirror_changed(&self) {}
    fn environment_changed(&self) {}
    fn remote_up(&self) {}
    fn remote_typed(&self) {}
    fn link_hover_lost(&self) {}
    fn pointer_changed(&self) {}
    fn blocks_changed(&self) {}
    fn unseen_changed(&self) {}
    fn program_status_changed(&self) {}
    fn notification(&self) {}
    fn view_changed(&self) {
        self.pending
            .store(true, std::sync::atomic::Ordering::Release);
    }
}

/// The options of a pane whose program is `demopen --files` (the example, built next to the test
/// binary), run by `/bin/sh` with job control as the view tests do.
fn demopen_options(home: &Path) -> bt_core::SessionOptions {
    let program = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent()?.parent().map(Path::to_path_buf))
        .map(|profile| profile.join("examples").join("demopen"))
        .expect("the path of the test binary");
    assert!(program.is_file(), "build it: {}", program.display());
    let mut views = bt_core::pencere::ViewOptions::new("bateri-test", "0.0.0");
    views.native_entry_limit = Some(pencere_sandbox::ModuleLimits::default().component as u64);
    let mut options = plain_options("", home);
    options.command = Some((
        "/bin/sh".to_owned(),
        vec![
            "-c".to_owned(),
            "set -m; \"$1\" --files".to_owned(),
            "sh".to_owned(),
            program.display().to_string(),
        ],
    ));
    options.rows = 10;
    options.views = Some(views);
    options
}

/// Has the program receive `size` bytes of a file the pane gives it, and says how long it took
/// (from the call to its answer), the way the pane's turn drives the session.
#[allow(clippy::too_many_arguments)]
fn receive_time(
    label: &str,
    session: &Arc<bt_core::Session>,
    pending: &std::sync::atomic::AtomicBool,
    exited: &std::sync::atomic::AtomicBool,
    dir: &Path,
    source: &Path,
    content: &[u8],
    remote_to: Option<&str>,
    before: &dyn Fn(),
) -> Duration {
    use bt_core::pencere::{Action, FileGrant};
    use std::sync::atomic::Ordering;

    let files = crate::native_view::ViewFiles::new(
        Arc::new(Arc::downgrade(session)),
        Arc::new(|_: &Path| {}),
    );
    let mut started = false;
    let mut said: Vec<String> = Vec::new();
    let turn = |said: &mut Vec<String>, started: &mut bool| {
        if !pending.swap(false, Ordering::AcqRel) {
            return;
        }
        for action in session.take_view_actions() {
            match action {
                Action::StartView(_) => *started = true,
                Action::ToView(text) => said.push(text),
                Action::ReadFile {
                    read,
                    handle,
                    offset,
                    length,
                } => files.read(read, &handle, offset, length),
                _ => {}
            }
        }
    };
    let begun = Instant::now();
    while !started {
        turn(&mut said, &mut started);
        assert!(begun.elapsed() < Duration::from_secs(30), "no view started");
        thread::sleep(Duration::from_millis(1));
    }
    eprintln!("[{label}] view started after {:?}", begun.elapsed());
    let given = crate::native_view::GivenFile::of(source).expect("a source");
    let handle = session
        .grant_file(&FileGrant { size: given.size })
        .expect("a handle");
    files.keep_given(&handle, given);
    let into = remote_to.map_or_else(|| dir.join(format!("received-{label}")), PathBuf::from);
    let request = format!(
        r#"{{"call":{{"id":1,"method":"receive","params":{{"handle":"{handle}","to":"{}"}}}}}}"#,
        into.display()
    );
    let deadline: u64 = std::env::var("BT_MEASURE_TIMEOUT_S")
        .ok()
        .and_then(|text| text.parse().ok())
        .unwrap_or(600);
    before();
    let since = Instant::now();
    assert!(session.module_message(&request), "the view is open");
    while !said
        .iter()
        .any(|text| text.contains(r#""response":{"id":1,"#))
    {
        turn(&mut said, &mut started);
        // A terminal that ends under the transfer (the server refused its input) is a visible
        // end, not a wait.
        if !session.reader_alive() {
            // What the view is told of the end comes a moment after the reader stopped.
            for _ in 0..200 {
                turn(&mut said, &mut started);
                thread::sleep(Duration::from_millis(5));
            }
            panic!(
                "[{label}] the terminal ended before the file was received, after {:?}; the \
                 pane was told the program ended: {}; the view saw: {said:?}",
                since.elapsed(),
                exited.load(Ordering::Acquire)
            );
        }
        assert!(
            since.elapsed() < Duration::from_secs(deadline),
            "[{label}] not received in time"
        );
        thread::sleep(Duration::from_millis(1));
    }
    let took = since.elapsed();
    assert!(
        remote_to.is_some() || std::fs::read(&into).is_ok_and(|bytes| bytes == content),
        "[{label}] not as given"
    );
    took
}

/// Writes the figures of one run.
fn report(label: &str, took: Duration, bytes: usize) {
    eprintln!(
        "MEASURE {label}: {} MiB in {:.2} s = {:.2} MiB/s",
        bytes >> 20,
        took.as_secs_f64(),
        (bytes as f64 / 1_048_576.0) / took.as_secs_f64()
    );
}

/// The same file, received by the same program, through a PTY of the pane's own and through a
/// terminal of a real kablo. Run in release, with the example built first
/// (`cargo build --release -p bt-core --example demopen`):
/// `cargo test --release -p bt-shell-common --lib measure_file_receive -- --ignored --nocapture`.
/// `BT_MEASURE_MIB` (50), `BT_MEASURE_MODES` (`direct,kablo`) and `BT_MEASURE_TIMEOUT_S` (600) set
/// the size, the runs and the wait. The kablo run does not finish while the input path has no
/// back-pressure: the bytes the pane writes faster than the terminal reads are dropped on the way
/// (`BUSY` from the bridge, `INPUT_FULL` from the relay), and the program waits for them.
#[test]
#[ignore = "measurement: starts real processes and moves tens of megabytes"]
fn measure_file_receive_direct_pty_and_kablo() {
    let Some(bundle) = release() else {
        eprintln!("SKIPPED: there is no kablo release to run (../kablo/dist or KABLO_DIST)");
        return;
    };
    let mib: usize = std::env::var("BT_MEASURE_MIB")
        .ok()
        .and_then(|text| text.parse().ok())
        .unwrap_or(50);
    let size = mib << 20;
    let mut ground = Ground::new("measure");
    let source = ground.root.join("given");
    let content: Vec<u8> = (0..size)
        .map(|at| u8::try_from(at % 251).unwrap_or(0))
        .collect();
    std::fs::write(&source, &content).expect("a file");
    let modes = std::env::var("BT_MEASURE_MODES").unwrap_or_else(|_| "direct,kablo".to_owned());

    if modes.contains("direct") {
        let wake = PaneFlag::default();
        let pending = Arc::clone(&wake.pending);
        let exited = Arc::clone(&wake.exited);
        let session = Arc::new(
            bt_core::Session::spawn(demopen_options(&ground.home), Arc::new(wake))
                .expect("a session"),
        );
        let took = receive_time(
            "direct",
            &session,
            &pending,
            &exited,
            &ground.root,
            &source,
            &content,
            None,
            &|| {},
        );
        report("direct PTY (Session::spawn)", took, size);
        session.shutdown();
        let _ = std::fs::remove_file(ground.root.join("received-direct"));
    }

    if modes.contains("kablo") {
        let service_dir = ground.root.join("svc");
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&service_dir)
            .expect("the service's directory");
        let kablo =
            kablo_proto::install::install_path_in(&ground.home.to_string_lossy(), bundle.version())
                .expect("a path");
        let backend = RealMac {
            bundle,
            home: ground.home.clone(),
            sockets: ground.sockets.clone(),
            streams: Default::default(),
        };
        let ledger = Arc::new(Ledger::memory());
        let service = Service::start(&service_dir, Arc::new(backend), Arc::clone(&ledger))
            .expect("a service");
        let options = demopen_options(&ground.home);
        let (spec, term_size) = new_session(&options.child_launch(), 1000);
        let pane = options.pane_uuid.clone().expect("a pane");
        let stream = service
            .open_local(&pane, spec, term_size, Duration::from_secs(30))
            .expect("a terminal of this Mac");
        let id = ledger.terminals_of(ACCOUNT).remove(0);
        ground.kept = Some(PathBuf::from(kablo));
        ground.made.push(id);
        let wake = PaneFlag::default();
        let pending = Arc::clone(&wake.pending);
        let exited = Arc::clone(&wake.exited);
        let session = Arc::new(
            bt_core::Session::streamed(options, Arc::new(wake), stream)
                .expect("a session over the stream"),
        );
        let took = receive_time(
            "kablo",
            &session,
            &pending,
            &exited,
            &ground.root,
            &source,
            &content,
            None,
            &|| {},
        );
        report("kablo terminal (Session::streamed)", took, size);
        service.end_all(std::slice::from_ref(&pane), Duration::from_secs(10));
        drop(session);
        service.stop();
    }
}

/// What the ssh measurement keeps on the server: one directory below `/tmp`, named, made by the
/// measurement and removed with it. kablo's sockets (`KABLO_DIR`), its home and its binary are
/// all below it; the user's own kablo and its terminals are not looked at.
struct RaspDir {
    host: String,
    dir: String,
    made: Vec<SessionId>,
    /// A file of this measurement outside the directory, removed with it.
    extra: Option<String>,
}

impl RaspDir {
    fn ssh(&self) -> Command {
        let mut command = Command::new("ssh");
        command.args([
            "-T",
            "-o",
            "BatchMode=yes",
            "-o",
            "ControlMaster=no",
            &self.host,
        ]);
        command
    }

    /// Runs a script on the server, with its standard input from `input`.
    fn run(&self, script: &str, input: Stdio) -> (Option<i32>, String) {
        let output = self.ssh().arg(script).stdin(input).output().expect("ssh");
        (
            output.status.code(),
            String::from_utf8_lossy(&output.stdout).into_owned(),
        )
    }

    fn put(&self, local: &Path, name: &str) {
        let file = std::fs::File::open(local).expect("a file to put");
        let script = format!(
            "cat > {dir}/{name} && chmod 755 {dir}/{name}",
            dir = self.dir
        );
        let (status, _) = self.run(&script, Stdio::from(file));
        assert_eq!(status, Some(0), "put {name}");
    }

    /// The environment kablo has on the server, as words of a script.
    fn env(&self) -> String {
        format!(
            "env HOME={dir}/h KABLO_DIR={dir}/s KABLO_TEST_END_KEEP_MS={keep}",
            dir = self.dir,
            keep = if std::env::var_os("BT_SSH_KEEP").is_some() {
                600_000
            } else {
                200
            }
        )
    }
}

impl Drop for RaspDir {
    fn drop(&mut self) {
        if std::env::var_os("BT_SSH_KEEP").is_some() {
            eprintln!("MEASURE kept: {}", self.dir);
            return;
        }
        let (_, err) = self.run(
            &format!("cat {}/attach.err 2>/dev/null | tail -5", self.dir),
            Stdio::null(),
        );
        if !err.trim().is_empty() {
            eprintln!("MEASURE attach.err: {}", err.trim());
        }
        // The terminals this measurement made, by name; then its own directory, and nothing else.
        let kablo = format!("{}/h/.local/share/kablo", self.dir);
        for id in &self.made {
            let script = format!(
                "{} {kablo}/*/kablo end --force {} >/dev/null 2>&1",
                self.env(),
                id.as_str()
            );
            let _ = self.run(&script, Stdio::null());
        }
        // What `end` did not reach, by process id and only if its command line names this directory.
        let leftover = format!(
            "for p in $(pgrep -f '[b]t-ssh-meas-{}/'); do kill $p; done",
            std::process::id()
        );
        let _ = self.run(&leftover, Stdio::null());
        if let Some(file) = &self.extra
            && file.contains("/bt-ssh-meas-")
            && file.ends_with(".recv")
        {
            let _ = self.run(&format!("rm -f {file}"), Stdio::null());
        }
        if self.dir.starts_with("/tmp/bt-ssh-meas") && self.dir.len() > "/tmp/bt-ssh-meas".len() {
            let _ = self.run(&format!("rm -rf {}", self.dir), Stdio::null());
        }
    }
}

/// This Mac's service, with a server for the one account it keeps: the bridge is `ssh`, the
/// kablo is the one put below the measurement's directory, its sockets are that directory's.
struct RaspBackend {
    rasp: Arc<Mutex<RaspDir>>,
    version: String,
}

impl Backend for RaspBackend {
    fn pane(&self, _: &Peer) -> Option<PaneInfo> {
        None
    }
    fn route(&self, _: &[String]) -> Option<KabloHost> {
        None
    }
    fn master(&self, _: &KabloHost) -> Result<Option<Vec<Vec<u8>>>, Refusal> {
        Ok(None)
    }
    fn master_made(&self, _: &KabloHost, _: u8) -> Result<(), Refusal> {
        Ok(())
    }
    fn install(&self, _: &KabloHost, _: &[SessionId]) -> Result<Installed, Refusal> {
        Ok(Installed {
            target: kablo_proto::install::Target::LinuxAarch64,
            version: self.version.clone(),
            path: kablo_proto::install::install_path(&self.version)
                .map_err(|_| Refusal::plain())?,
            fresh: false,
        })
    }
    fn version(&self) -> Option<String> {
        Some(self.version.clone())
    }
    fn carry(&self, _: &KabloHost, installed: &Installed) -> io::Result<Carried> {
        let rasp = self.rasp.lock().unwrap_or_else(|e| e.into_inner());
        let mut command = rasp.ssh();
        command.arg(format!(
            "{} {}/h/{} attach 2>>{}/attach.err",
            rasp.env(),
            rasp.dir,
            installed.path,
            rasp.dir
        ));
        start(command)
    }
}

/// The same file, received by the same program on a server over ssh: `ssh <host>` carries
/// `kablo attach`, the terminal is `demopen --files` on the server, the file is on this Mac and
/// is read by handle. Needs `BT_SSH_DEMOPEN` (demopen built for the server, aarch64 musl) and the
/// server's kablo from the release (`KABLO_DIST`); `BT_SSH_HOST` (`rasp`), `BT_MEASURE_MIB` (50),
/// `BT_MEASURE_TIMEOUT_S` (600). Everything it makes on the server is below `/tmp/bt-ssh-meas-<pid>`.
#[test]
#[ignore = "measurement: starts real processes on a server and moves tens of megabytes over ssh"]
fn measure_file_receive_over_ssh() {
    let Some(bundle) = release() else {
        eprintln!("SKIPPED: there is no kablo release to run");
        return;
    };
    let Some(demopen) = std::env::var_os("BT_SSH_DEMOPEN").map(PathBuf::from) else {
        eprintln!("SKIPPED: BT_SSH_DEMOPEN is not set");
        return;
    };
    let host = std::env::var("BT_SSH_HOST").unwrap_or_else(|_| "rasp".to_owned());
    let mib: usize = std::env::var("BT_MEASURE_MIB")
        .ok()
        .and_then(|text| text.parse().ok())
        .unwrap_or(50);
    let size = mib << 20;
    let mut ground = Ground::new("measure-ssh");
    let source = ground.root.join("given");
    let content: Vec<u8> = (0..size)
        .map(|at| u8::try_from(at % 251).unwrap_or(0))
        .collect();
    std::fs::write(&source, &content).expect("a file");

    let dir = format!("/tmp/bt-ssh-meas-{}", std::process::id());
    let rasp = Arc::new(Mutex::new(RaspDir {
        host,
        dir: dir.clone(),
        made: Vec::new(),
        extra: None,
    }));
    let version = bundle.version().to_owned();
    {
        let rasp = rasp.lock().expect("lock");
        let (status, said) = rasp.run(
            &format!(
                "mkdir -m 700 {dir} && mkdir -m 700 {dir}/s && mkdir -p {dir}/h/.local/share/kablo/{version} && uname -m"
            ),
            Stdio::null(),
        );
        assert_eq!(status, Some(0), "the directory on the server");
        assert_eq!(said.trim(), "aarch64");
        let artifact = bundle
            .artifact(kablo_proto::install::Target::LinuxAarch64)
            .expect("a kablo for the server");
        rasp.put(
            &artifact.path,
            &format!("h/.local/share/kablo/{version}/kablo"),
        );
        rasp.put(&demopen, "demopen");
    }

    if std::env::var_os("BT_SSH_RAW").is_some() {
        // The same bytes through a bare `ssh host cat`, with the handshake taken off.
        let (host, _) = {
            let rasp = rasp.lock().expect("lock");
            (rasp.host.clone(), ())
        };
        let hello = Instant::now();
        let _ = Command::new("ssh")
            .args(["-T", "-o", "BatchMode=yes", &host, "true"])
            .status();
        let hello = hello.elapsed();
        let file = std::fs::File::open(&source).expect("the file");
        let begun = Instant::now();
        let _ = Command::new("ssh")
            .args(["-T", "-o", "BatchMode=yes", &host, "cat >/dev/null"])
            .stdin(Stdio::from(file))
            .status();
        let all = begun.elapsed();
        let net = all.saturating_sub(hello);
        eprintln!(
            "MEASURE raw ssh cat: {mib} MiB in {:.2} s (handshake {:.2} s) = {:.2} MiB/s net, {:.2} MiB/s with the handshake",
            all.as_secs_f64(),
            hello.as_secs_f64(),
            mib as f64 / net.as_secs_f64(),
            mib as f64 / all.as_secs_f64()
        );
    }
    let service_dir = ground.root.join("svc");
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&service_dir)
        .expect("the service's directory");
    let backend = RaspBackend {
        rasp: Arc::clone(&rasp),
        version,
    };
    let ledger = Arc::new(Ledger::memory());
    let service =
        Service::start(&service_dir, Arc::new(backend), Arc::clone(&ledger)).expect("a service");
    let options = demopen_options(&ground.home);
    let (mut spec, term_size) = new_session(&options.child_launch(), 1000);
    spec.argv = [
        "/bin/sh",
        "-c",
        "sleep 5; set -m; \"$1\" --files",
        "sh",
        &format!("{dir}/demopen"),
    ]
    .iter()
    .map(|arg| arg.as_bytes().to_vec())
    .collect();
    spec.env.retain(|(name, _)| name != b"PWD");
    spec.env.push((b"PATH".to_vec(), b"/usr/bin:/bin".to_vec()));
    spec.cwd = Some(dir.clone().into_bytes());
    let pane = options.pane_uuid.clone().expect("a pane");
    let stream = service
        .open_local(&pane, spec, term_size, Duration::from_secs(60))
        .expect("a terminal on the server");
    let id = ledger.terminals_of(ACCOUNT).remove(0);
    rasp.lock().expect("lock").made.push(id);
    let wake = PaneFlag::default();
    let pending = Arc::clone(&wake.pending);
    let exited = Arc::clone(&wake.exited);
    let session = Arc::new(
        bt_core::Session::streamed(options, Arc::new(wake), stream)
            .expect("a session over the stream"),
    );
    // `BT_SSH_RECV=/dev/shm`: the file lands in memory on the server (the disk there is slow).
    let remote_file = match std::env::var("BT_SSH_RECV") {
        Ok(place) => format!("{place}/bt-ssh-meas-{}.recv", std::process::id()),
        Err(_) => format!("{dir}/received"),
    };
    rasp.lock().expect("lock").extra = Some(remote_file.clone());
    let top: Mutex<Option<thread::JoinHandle<io::Result<String>>>> = Mutex::new(None);
    let mac_top: Mutex<Option<thread::JoinHandle<io::Result<String>>>> = Mutex::new(None);
    let begin_top = || {
        if std::env::var_os("BT_MAC_TOP").is_some() {
            *mac_top.lock().expect("lock") = Some(thread::spawn(|| {
                Command::new("sh")
                    .args(["-c", "top -l 6 -s 2 -o cpu -n 7 -stats pid,command,cpu | grep -E 'CPU usage|ssh|cloudflared|bt_shell|PID' | tail -n +4"])
                    .output()
                    .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
            }));
        }
        if std::env::var_os("BT_SSH_TOP").is_none() {
            return;
        }
        let mut command = rasp.lock().expect("lock").ssh();
        command.arg("top -b -n 9 -d 2 -w 200 -o %CPU | awk '/%Cpu/{print} /^ *[0-9]+ /{ if ($9+0>=1.0) print }'");
        *top.lock().expect("lock") = Some(thread::spawn(move || {
            command
                .output()
                .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        }));
    };
    let took = receive_time(
        "ssh",
        &session,
        &pending,
        &exited,
        &ground.root,
        &source,
        &content,
        Some(&remote_file),
        &begin_top,
    );
    report("kablo terminal over ssh (Session::streamed)", took, size);
    if let Some(top) = mac_top.lock().expect("lock").take() {
        eprintln!(
            "MEASURE mac top:\n{}",
            top.join().ok().and_then(Result::ok).unwrap_or_default()
        );
    }
    if let Some(top) = top.lock().expect("lock").take() {
        eprintln!(
            "MEASURE top (2 s samples, the first is since boot):\n{}",
            top.join().ok().and_then(Result::ok).unwrap_or_default()
        );
    }
    // What arrived is what was given: the size and the sum, on the server and here.
    let (_, there) = rasp
        .lock()
        .expect("lock")
        .run(&format!("md5sum < {remote_file}"), Stdio::null());
    let here = Command::new("md5")
        .arg("-q")
        .arg(&source)
        .output()
        .expect("md5");
    eprintln!(
        "MEASURE sums: server {} / here {}",
        there.trim(),
        String::from_utf8_lossy(&here.stdout).trim()
    );
    assert!(
        there.starts_with(String::from_utf8_lossy(&here.stdout).trim()),
        "not as given"
    );
    ground.kept = None;
    service.end_all(std::slice::from_ref(&pane), Duration::from_secs(10));
    drop(session);
    service.stop();
}

/// The two contracts the window keeps with the `bateri` command of kablo that two repositories
/// write — the counter (`kablo bateri --next-number`, which numbers the terminals the window
/// opens) and the words of a relayed command (`--hex`, in the line the window builds) — against
/// the real kablo, not a stand-in: what kablo's own tests prove of its half, this proves of the
/// window's.
#[test]
#[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
fn a_real_kablo_counts_for_the_window_and_reads_the_words_of_a_relayed_command_whole() {
    let Some(bundle) = release() else {
        eprintln!("SKIPPED: there is no kablo release to run (../kablo/dist or KABLO_DIST)");
        return;
    };
    // The `bateri` command is kablo's from 0.6 on: an earlier release has none to hold to.
    let mut parts = bundle.version().split(['.', '-', '+']);
    let at_least_0_6 = match (
        parts.next().and_then(|part| part.parse::<u32>().ok()),
        parts.next().and_then(|part| part.parse::<u32>().ok()),
    ) {
        (Some(major), Some(minor)) => (major, minor) >= (0, 6),
        _ => false,
    };
    if !at_least_0_6 {
        eprintln!(
            "SKIPPED: kablo {} has no `bateri` command (KABLO_DIST names a 0.6 release)",
            bundle.version()
        );
        return;
    }
    let ground = Ground::new("contract");
    let channel = LocalChannel::new(ground.home.clone());
    let installed = match ensure_here(&channel, &bundle, true) {
        Ok(installed) => installed,
        Err(Failure::Impossible(why)) => {
            eprintln!("SKIPPED: this machine is not one kablo is built for: {why:?}");
            return;
        }
        Err(other) => panic!("kablo was not installed: {other:?}"),
    };

    // The window's counter is kablo's, run as the install put it: a number, once.
    let counting = LocalChannel {
        home: ground.home.clone(),
        limit: Duration::from_secs(15),
    };
    assert_eq!(install::next_number(&counting, &installed.path), Some(1));
    assert_eq!(install::next_number(&counting, &installed.path), Some(2));
    assert_eq!(
        std::fs::read_to_string(ground.home.join(".local/share/bateri/next")).expect("the counter"),
        "3\n"
    );

    // The line the window sends another machine reaches `bin/bateri` there, which starts that
    // machine's kablo with the words as they were, whatever the characters in them: kablo reads
    // them (hex) and says which one it does not know, in the words it was given.
    let word = "a b'c\"d$HOME`x`\\z";
    let line = crate::agent::relay::remote_line(
        &crate::agent::relay::not_set_up("there"),
        &[word.to_owned()],
    );
    let mut command = crate::agent::relay::local_command(&ground.home, &line);
    command
        .env_clear()
        .env("HOME", &ground.home)
        .env("SHELL", "/bin/sh")
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env(sockdir_variable(), &ground.sockets);
    let said = crate::agent::relay::execute(command, Duration::from_secs(30), 1 << 16)
        .expect("the line ran");
    assert_eq!(said.status, 64, "{said:?}");
    assert_eq!(
        said.stderr.first().map(String::as_str),
        Some(&*format!("bateri: no command '{word}'"))
    );
}

// ─── programs and connections over a real kablo ──────────────────────────

/// This Mac's connection to a real kablo with one terminal on it, so that the service has the
/// connection up for the programs and connections tests ask it for.
struct Here {
    ground: Ground,
    service: Service,
    _terminal: bt_core::Stream,
}

impl Here {
    fn new(name: &str) -> Option<Here> {
        let Some(bundle) = release() else {
            eprintln!("SKIPPED: there is no kablo release to run (../kablo/dist or KABLO_DIST)");
            return None;
        };
        let mut ground = Ground::new(name);
        let service_dir = ground.root.join("svc");
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&service_dir)
            .expect("the service's directory");
        let kablo =
            kablo_proto::install::install_path_in(&ground.home.to_string_lossy(), bundle.version())
                .expect("a path");
        let backend = RealMac {
            bundle,
            home: ground.home.clone(),
            sockets: ground.sockets.clone(),
            streams: Default::default(),
        };
        let ledger = Arc::new(Ledger::memory());
        let service = Service::start(&service_dir, Arc::new(backend), Arc::clone(&ledger))
            .expect("a service");
        let options = plain_options("sleep 600", &ground.home);
        let (spec, size) = new_session(&options.child_launch(), 1000);
        let pane = options.pane_uuid.clone().expect("a pane");
        let terminal = service
            .open_local(&pane, spec, size, Duration::from_secs(30))
            .expect("a terminal of this Mac");
        let id = ledger.terminals_of(ACCOUNT).remove(0);
        ground.kept = Some(PathBuf::from(kablo));
        ground.made.push(id);
        Some(Here {
            ground,
            service,
            _terminal: terminal,
        })
    }

    fn run(&self, words: &[&str]) -> Result<crate::kablo::remote::RemoteCommand, String> {
        let command = kablo_proto::v1::stream::Command::new(
            words.iter().map(|word| word.as_bytes().to_vec()).collect(),
        );
        self.service
            .streams()
            .command(ACCOUNT, command, Duration::from_secs(30))
    }
}

/// Whether the process `pid` is there.
fn alive(pid: &str) -> bool {
    Command::new("kill")
        .args(["-0", pid])
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

#[test]
#[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
fn a_program_over_a_real_kablo_is_given_its_input_and_gives_its_output_errors_and_how_it_ended() {
    let Some(here) = Here::new("run") else {
        return;
    };
    let crate::kablo::remote::RemoteCommand {
        stdin,
        mut stdout,
        mut stderr,
        end,
    } = here
        .run(&[
            "sh",
            "-c",
            "echo out-first; echo err-first >&2; cat; echo out-last; echo err-last >&2; exit 5",
        ])
        .expect("a program");
    let mut stdin = stdin.expect("an input");
    stdin.write_all(b"typed\n").expect("written");
    drop(stdin);
    // The two are read apart: neither waits for the other.
    let errors = thread::spawn(move || {
        let mut err = String::new();
        stderr.read_to_string(&mut err).expect("errors");
        err
    });
    let mut out = String::new();
    stdout.read_to_string(&mut out).expect("output");
    assert_eq!(out, "out-first\ntyped\nout-last\n");
    assert_eq!(errors.join().expect("errors"), "err-first\nerr-last\n");
    assert_eq!(end.wait(), Ok(Exit::Code(5)));
}

#[test]
#[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
fn a_program_the_server_cannot_start_is_refused_in_the_servers_words() {
    let Some(here) = Here::new("refused") else {
        return;
    };
    let error = here
        .run(&["/nonexistent-dir/no-such-program"])
        .err()
        .expect("refused");
    assert_eq!(error, "No such file or directory");
}

#[test]
#[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
fn a_program_whose_handles_are_dropped_is_ended_on_the_server() {
    let Some(here) = Here::new("hangup") else {
        return;
    };
    let pidfile = here.ground.home.join("pid");
    let command = here
        .run(&[
            "sh",
            "-c",
            &format!("echo $$ > '{}'; exec sleep 600", pidfile.display()),
        ])
        .expect("a program");
    let started = Instant::now();
    let pid = loop {
        if let Some(pid) = std::fs::read_to_string(&pidfile)
            .ok()
            .map(|text| text.trim().to_owned())
            .filter(|text| !text.is_empty())
        {
            break pid;
        }
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "the program never started"
        );
        thread::sleep(Duration::from_millis(20));
    };
    assert!(alive(&pid), "the program runs while it is held");
    drop(command);
    let dropped = Instant::now();
    while alive(&pid) && dropped.elapsed() < Duration::from_secs(20) {
        thread::sleep(Duration::from_millis(50));
    }
    let ended = !alive(&pid);
    if !ended {
        // What the test started is not left behind.
        let _ = Command::new("kill").args(["-9", &pid]).status();
    }
    assert!(
        ended,
        "the program was still there 20 s after its handles were dropped"
    );
}

#[test]
#[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
fn a_connection_over_a_real_kablo_reaches_a_server_of_the_far_machine_and_its_ends_end_apart() {
    let Some(here) = Here::new("tcp") else {
        return;
    };
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("a port");
    let port = listener.local_addr().expect("an address").port();
    // The far end answers each line with its length and, when the other end has said all it has,
    // says "bye" and closes.
    let far = thread::spawn(move || {
        let (mut peer, _) = listener.accept().expect("a connection");
        let mut said = Vec::new();
        let mut buf = [0u8; 1024];
        loop {
            let n = peer.read(&mut buf).expect("read");
            if n == 0 {
                break;
            }
            said.extend_from_slice(buf.get(..n).unwrap_or_default());
            peer.write_all(format!("{}\n", said.len()).as_bytes())
                .expect("answered");
        }
        peer.write_all(b"bye").expect("said bye");
        said
    });
    let mut remote = here
        .service
        .streams()
        .connection(ACCOUNT, "127.0.0.1", port, Duration::from_secs(30))
        .expect("a connection");
    let mut writer = remote.writer.take().expect("a writer");
    writer.write_all(b"hello").expect("written");
    let mut first = [0u8; 2];
    remote.reader.read_exact(&mut first).expect("the answer");
    assert_eq!(&first, b"5\n");
    drop(writer);
    let mut rest = String::new();
    remote.reader.read_to_string(&mut rest).expect("the rest");
    assert_eq!(rest, "bye");
    assert_eq!(far.join().expect("the far end"), b"hello");
}

#[test]
#[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
fn thirty_two_megabytes_go_through_a_program_and_come_back_whole_while_both_are_read_and_written() {
    let Some(here) = Here::new("big") else {
        return;
    };
    const BYTES: usize = 32 << 20;
    let crate::kablo::remote::RemoteCommand {
        stdin,
        mut stdout,
        stderr: _stderr,
        end,
    } = here.run(&["cat"]).expect("a program");
    let mut stdin = stdin.expect("an input");
    let started = Instant::now();
    let writer = thread::spawn(move || {
        let mut chunk = vec![0u8; 61_440];
        let mut sent = 0usize;
        while sent < BYTES {
            let n = chunk.len().min(BYTES - sent);
            for (offset, byte) in chunk.iter_mut().take(n).enumerate() {
                *byte = ((sent + offset) % 251) as u8;
            }
            stdin
                .write_all(chunk.get(..n).unwrap_or_default())
                .expect("written");
            sent += n;
        }
    });
    let (done, heard) = mpsc::channel();
    let reader = thread::spawn(move || {
        let mut buf = vec![0u8; 100_000];
        let mut got = 0usize;
        let mut wrong = None;
        loop {
            let n = stdout.read(&mut buf).expect("read");
            if n == 0 {
                break;
            }
            for (offset, byte) in buf.iter().take(n).enumerate() {
                if *byte != ((got + offset) % 251) as u8 && wrong.is_none() {
                    wrong = Some(got + offset);
                }
            }
            got += n;
        }
        let _ = done.send((got, wrong));
    });
    let (got, wrong) = heard
        .recv_timeout(Duration::from_secs(180))
        .expect("the 32 MiB came back in time");
    assert_eq!(wrong, None, "the first wrong byte");
    assert_eq!(got, BYTES);
    writer.join().expect("the writer");
    reader.join().expect("the reader");
    assert_eq!(end.wait(), Ok(Exit::Code(0)));
    report(
        "32 MiB through a program, both ways",
        started.elapsed(),
        BYTES * 2,
    );
}

/// The pids of the processes on this machine that run the helper's script.
fn helper_shells() -> std::collections::HashSet<String> {
    let listing = Command::new("ps")
        .args(["-axww", "-o", "pid=,command="])
        .output()
        .expect("ps");
    String::from_utf8_lossy(&listing.stdout)
        .lines()
        .filter(|line| line.contains("IFS= read -r bt_line"))
        .filter_map(|line| line.split_whitespace().next().map(str::to_owned))
        .collect()
}

#[test]
#[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
fn the_helper_over_a_real_kablo_answers_what_exists_the_ports_and_the_load_and_leaves_no_sh_idle() {
    use crate::kablo::served::Served;
    use crate::remote_helper::{Answer, Dial, LoadReply, Query, RemoteHelper, Reply, Request};
    let Some(here) = Here::new("helper") else {
        return;
    };
    let before = helper_shells();
    let file = here.ground.home.join("a file.txt");
    std::fs::write(&file, b"twelve bytes").expect("a file");
    let backend = here.service.backend();
    let mut helper = RemoteHelper::idling(Duration::from_millis(500));
    let mut ask = |query: Query| {
        let (tx, answers) = mpsc::channel();
        let backend = Arc::clone(&backend);
        let reply: Reply = Box::new(move |answer| {
            let _ = tx.send(answer);
        });
        helper.ask(Request {
            command: 1,
            dial: Dial::new(
                Served::new(Arc::clone(&backend), super::host()),
                crate::ssh_route::Ask::Never,
            ),
            host: "this computer".to_owned(),
            query,
            reply,
        });
        answers
            .recv_timeout(Duration::from_secs(60))
            .expect("the worker replies")
    };

    // What exists, and what a folder holds.
    let path = file.to_string_lossy().into_owned();
    let answer = ask(Query::Verify {
        candidates: vec!["nowhere-at-all".to_owned(), path.clone()],
        cwd: here.ground.home.to_string_lossy().into_owned(),
    });
    match answer {
        Ok(Answer::Verified(Some((1, found, entry)))) => {
            assert_eq!(found, path);
            assert!(
                matches!(
                    entry,
                    crate::remote_files::RemoteEntry::File { size: Some(12), .. }
                ),
                "{entry:?}"
            );
        }
        other => panic!("expected the second candidate, got {other:?}"),
    }
    let home = here.ground.home.to_string_lossy().into_owned();
    assert!(matches!(
        ask(Query::Count(home)),
        Ok(Answer::Counted(Some(
            crate::remote_files::RemoteEntry::Dir(_)
        )))
    ));
    // The load line and the port list: a Linux server reads them, any other says it cannot.
    assert!(matches!(
        ask(Query::Load { detail: false }),
        Ok(Answer::Load(LoadReply::Sample(_) | LoadReply::NoProc))
    ));
    assert!(matches!(
        ask(Query::Ports {
            shell: std::process::id()
        }),
        Ok(Answer::Ports(_))
    ));

    // One `sh` runs on the server for all of it, and it is gone once the helper has idled.
    let ours: Vec<String> = helper_shells().difference(&before).cloned().collect();
    assert_eq!(ours.len(), 1, "one helper shell: {ours:?}");
    let idled = Instant::now();
    while idled.elapsed() < Duration::from_secs(20) && ours.iter().any(|pid| alive(pid)) {
        thread::sleep(Duration::from_millis(50));
    }
    let left: Vec<&String> = ours.iter().filter(|pid| alive(pid)).collect();
    for pid in &left {
        // What the test started is not left behind.
        let _ = Command::new("kill").args(["-9", pid]).status();
    }
    assert!(left.is_empty(), "the helper's sh stayed: {left:?}");
}

/// The pids of the processes on this machine that run a program with these words.
fn running(words: &str) -> std::collections::HashSet<String> {
    let listing = Command::new("ps")
        .args(["-axww", "-o", "pid=,command="])
        .output()
        .expect("ps");
    String::from_utf8_lossy(&listing.stdout)
        .lines()
        .filter(|line| line.contains(words))
        .filter_map(|line| line.split_whitespace().next().map(str::to_owned))
        .collect()
}

#[test]
#[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
fn a_folder_goes_up_and_comes_down_over_a_real_kablo_with_the_same_contents() {
    use crate::download::{self, Conflict};
    use crate::kablo::served::Served;
    use crate::ssh_route::Ask;
    use crate::upload::{self, Outcome, Shared};
    let Some(here) = Here::new("updown") else {
        return;
    };
    let served = Served::new(here.service.backend(), super::host());
    let home = here.ground.home.clone();
    let site = home.join("src/site");
    std::fs::create_dir_all(site.join("css")).expect("a tree");
    std::fs::write(site.join("css/site.css"), b"body{}").expect("a file");
    std::fs::write(site.join("a name with spaces.js"), vec![b'j'; 70_000]).expect("a file");
    let server = home.join("server");
    std::fs::create_dir_all(&server).expect("the server's folder");

    // Up: the probe finds the folder, the stream puts the tree in it.
    let (items, reply) = upload::probe(
        &served,
        Ask::Never,
        "this computer",
        Some(&server.to_string_lossy()),
        &[site.to_string_lossy().into_owned()],
    )
    .expect("a probe");
    assert!(reply.existing.is_empty());
    let shared = Arc::new(Shared::default());
    assert_eq!(
        upload::transfer(&served, &items[0], &reply.dir, &shared, || {}),
        Outcome::Done
    );
    assert_eq!(shared.progress().1, 2, "two files went");
    assert_eq!(
        std::fs::read(server.join("site/css/site.css")).expect("css"),
        b"body{}"
    );
    assert_eq!(
        std::fs::read(server.join("site/a name with spaces.js"))
            .expect("js")
            .len(),
        70_000
    );

    // Down: the same tree lands whole in the folder it is given.
    let landing = home.join("Downloads");
    std::fs::create_dir_all(&landing).expect("a landing folder");
    let (outcome, landed) = download::transfer(
        &served,
        &server.join("site").to_string_lossy(),
        &landing.join("site"),
        Conflict::KeepBoth,
        &Arc::new(Shared::default()),
        || {},
        |_| {},
    );
    assert_eq!(outcome, Outcome::Done);
    assert_eq!(landed, Some(landing.join("site")));
    assert_eq!(
        std::fs::read(landing.join("site/css/site.css")).expect("css"),
        b"body{}"
    );
    assert_eq!(
        std::fs::read(landing.join("site/a name with spaces.js"))
            .expect("js")
            .len(),
        70_000
    );
}

#[test]
#[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
fn a_cancelled_upload_ends_its_tar_on_the_server_and_removes_the_half_written_file() {
    use crate::kablo::served::Served;
    use crate::upload::{self, Outcome, Shared};
    let Some(here) = Here::new("cancel") else {
        return;
    };
    let served = Served::new(here.service.backend(), super::host());
    let home = here.ground.home.clone();
    let server = home.join("server");
    std::fs::create_dir_all(&server).expect("the server's folder");
    let big = home.join("big.bin");
    {
        let mut file = std::fs::File::create(&big).expect("a big file");
        let block = vec![b'z'; 8 << 20];
        for _ in 0..48 {
            file.write_all(&block).expect("a block");
        }
    }
    let before = running("tar -x -v -p -o -f -");
    let item = upload::measure(&big).expect("measured");
    let dir = server.to_string_lossy().into_owned();
    let shared = Arc::new(Shared::default());
    let (sender, ended) = mpsc::channel();
    thread::spawn({
        let shared = Arc::clone(&shared);
        let served = served.clone();
        move || {
            let _ = sender.send(upload::transfer(&served, &item, &dir, &shared, || {}));
        }
    });
    // The server's tar has begun the file when it is there; it has said so a moment later.
    let began = Instant::now();
    while !server.join("big.bin").exists() && began.elapsed() < Duration::from_secs(30) {
        thread::sleep(Duration::from_millis(2));
    }
    assert!(
        server.join("big.bin").exists(),
        "the stream never got going"
    );
    thread::sleep(Duration::from_millis(100));
    shared.cancel();
    let outcome = ended
        .recv_timeout(Duration::from_secs(30))
        .expect("the stream thread ended");
    assert_eq!(outcome, Outcome::Cancelled);

    // The server's tar is gone with the stream, and the file it had begun is removed.
    let waited = Instant::now();
    while running("tar -x -v -p -o -f -")
        .difference(&before)
        .next()
        .is_some()
        && waited.elapsed() < Duration::from_secs(20)
    {
        thread::sleep(Duration::from_millis(50));
    }
    let left: Vec<String> = running("tar -x -v -p -o -f -")
        .difference(&before)
        .cloned()
        .collect();
    for pid in &left {
        // What the test started is not left behind.
        let _ = Command::new("kill").args(["-9", pid]).status();
    }
    assert!(left.is_empty(), "the server's tar stayed: {left:?}");
    assert!(
        !server.join("big.bin").exists(),
        "the half-written file stayed"
    );
}

#[test]
#[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
fn a_write_to_a_program_that_reads_nothing_waits_on_its_window_until_the_program_is_ended() {
    use std::sync::atomic::AtomicBool;
    let Some(here) = Here::new("blocked") else {
        return;
    };
    let mut command = here.run(&["sleep", "600"]).expect("a program");
    let mut input = command.stdin.take().expect("its input");
    let end = Arc::clone(&command.end);
    let (sender, result) = mpsc::channel();
    thread::spawn(move || {
        let stop = AtomicBool::new(false);
        let bytes = vec![b'q'; 16 << 20];
        let _ = sender.send(input.write_all_within(
            &bytes,
            Duration::from_secs(120),
            Duration::from_millis(10),
            &stop,
        ));
    });
    assert!(
        result.recv_timeout(Duration::from_millis(1500)).is_err(),
        "sixteen megabytes were taken by a program that reads nothing"
    );
    end.kill();
    let written = result
        .recv_timeout(Duration::from_secs(20))
        .expect("the write ended with the program");
    assert_eq!(
        written.expect_err("the stream is over").kind(),
        std::io::ErrorKind::BrokenPipe
    );
}
