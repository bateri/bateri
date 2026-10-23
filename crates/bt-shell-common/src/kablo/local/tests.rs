//! This Mac's account against a real kablo: the release the sibling repository builds
//! (`../kablo/dist`, or `KABLO_DIST`), installed below a home directory of the test's, its
//! sessions in a socket directory of the test's. Without that release the real tests say
//! `SKIPPED` and pass: a checkout of bateri alone has no kablo to run.

use std::io::{Read, Write};
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use kablo_proto::SessionId;

use super::*;
use crate::kablo::install::{self, Bundle, Failure, ensure_here, prune_versions, unused_versions};
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
