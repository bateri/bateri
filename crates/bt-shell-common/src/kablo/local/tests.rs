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
fn a_real_kablo_is_installed_here_keeps_a_terminal_past_its_bridge_and_is_read_by_the_census() {
    let Some(bundle) = release() else {
        eprintln!("SKIPPED: there is no kablo release to run (../kablo/dist or KABLO_DIST)");
        return;
    };
    let mut ground = Ground::new("real");
    let channel = LocalChannel::new(ground.home.clone());
    let installed = match ensure_here(&channel, &bundle) {
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
    assert!(!ensure_here(&channel, &bundle).expect("found").fresh);

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
        ensure_here(&LocalChannel::new(self.home.clone()), &self.bundle)
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
    fn bell(&self) {}
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
