//! The steps over a stand-in `ssh`: a script that answers `-G` and `-O check`,
//! makes a "master" (a file at the control path) and runs a channel's command
//! for real in a directory that stands for the server's home.

use std::io::{Read, Write};
use std::os::unix::fs::DirBuilderExt;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::*;
use crate::kablo::install::sha256_hex;
use crate::settings::TempRoot;
use crate::ssh_route::SystemSsh;
use kablo_proto::install::Target;

/// kablo, as a script: it says its version, `attach` echoes, `list` says what is in `listing` in the
/// home directory (the terminals of the machine, as [`sessions`] writes them), `upgrade` writes down
/// what it was asked beside the binary, and `bateri --next-number` counts the way kablo's counter does
/// (`~/.local/share/bateri/next`, the number to give next, written before it is given).
const KABLO: &str = "#!/bin/sh\ncase $1 in version) echo \"version=0.2.0 wire_min=1 wire_max=2\";; attach) exec cat;; \
     upgrade) echo \"$*\" >>\"$(dirname \"$0\")/upgrades\";; \
     list) cat \"$HOME/listing\" 2>/dev/null || exit 1;; \
     bateri) [ \"$2\" = --next-number ] || exit 64; s=$HOME/.local/share/bateri; n=1; \
     [ -f \"$s/next\" ] && n=$(cat \"$s/next\"); mkdir -p \"$s\" && echo $((n + 1)) >\"$s/next\" && echo $n;; esac\n";

/// The ids of terminals, as the ledger would hand them over.
fn ids(names: &[&str]) -> Vec<SessionId> {
    names
        .iter()
        .map(|name| SessionId::new(name).expect("an id"))
        .collect()
}

/// Says what the stand-in machine whose home is `home` has running: each terminal, the release its
/// relay and its warden run from, and nobody attached.
fn sessions(home: &Path, runs: &[(&str, &str)]) {
    let objects: Vec<String> = runs
        .iter()
        .map(|(id, version)| {
            format!(
                "{{\"id\":\"{id}\",\"state\":\"running\",\"clients\":0,\
                 \"version\":\"{version}\",\"warden_version\":\"{version}\"}}"
            )
        })
        .collect();
    std::fs::write(
        home.join("listing"),
        format!("[\n{}\n]\n", objects.join(",\n")),
    )
    .expect("a listing");
}

struct World {
    _root: TempRoot,
    dir: PathBuf,
    home: PathBuf,
    log: PathBuf,
    state: PathBuf,
    ssh: PathBuf,
    /// What the backend asked the application to write to the settings.
    saved: Arc<Mutex<Vec<SettingsEdit>>>,
}

/// A stand-in server. `master_exit` is the status the master step ends with.
fn stand_in(name: &str, master_exit: i32) -> World {
    let root = TempRoot::new(name);
    // A short, private directory for the sockets' place.
    let dir = PathBuf::from(format!("/tmp/bt-ssh-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&dir)
        .expect("dir");
    let home = root.0.join("home");
    std::fs::create_dir_all(&home).expect("home");
    let log = root.0.join("log");
    let ssh = root.0.join("ssh");
    crate::child::tests::write_program(
        &ssh,
        &format!(
            r#"#!/bin/sh
printf '%s\n' "$*" >> {log}
mode=channel; cp=; prev=
for a in "$@"; do
  case $a in
    -G) mode=config;;
    check) [ "$prev" = -O ] && mode=check;;
    -M) mode=master;;
    ControlPath=*) cp=${{a#ControlPath=}};;
  esac
  prev=$a
done
for last; do :; done
case $mode in
  config) printf 'user u\nhostname h\nport 22\nrequesttty auto\nsessiontype default\nproxyjump none\n'; exit 0;;
  check) if [ -e "$cp" ]; then echo "Master running (pid=1)" >&2; exit 0; fi; echo "Control socket connect($cp): No such file or directory" >&2; exit 255;;
  master) if [ {master_exit} -eq 0 ]; then : > "$cp"; fi; exit {master_exit};;
  *) cd {home} && HOME={home} SHELL=/bin/zsh exec sh -c "$last";;
esac
"#,
            log = log.display(),
            home = home.display(),
        ),
    );
    World {
        state: root.0.join("hosts"),
        _root: root,
        dir,
        home,
        log,
        ssh,
        saved: Arc::default(),
    }
}

impl World {
    fn backend(&self, bundle: Option<Bundle>) -> SshBackend {
        SshBackend {
            ssh: self.ssh.to_string_lossy().into_owned(),
            bases: vec![self.dir.clone()],
            state_path: self.state.clone(),
            bundle,
            home: None,
            settings: Box::new(Settings::default),
            setup: Box::new(|_, _, _| ()),
            save: {
                let saved = Arc::clone(&self.saved);
                Box::new(move |edit| saved.lock().expect("saved").push(edit))
            },
            panes: Box::new(|_| Some(PaneInfo::default())),
            runner: Box::new(Through(self.ssh.clone())),
            name: Some("MacBook".into()),
            history: Box::new(|| Some(5000)),
            past: Box::new(|| true),
            masters: None,
            linked: Box::new(|_, _| ()),
            changed: Box::new(|_| ()),
            program: Box::new(|_| ()),
            listed: Box::new(|_| ()),
            staged: Box::new(|_| ()),
            mail: None,
            notes: None,
            filed: Default::default(),
            streams: Default::default(),
            watches: None,
        }
    }

    fn package(&self) -> Bundle {
        let dir = self._root.0.join("kablo");
        std::fs::create_dir_all(&dir).expect("package");
        let mut sums = String::new();
        for target in Target::ALL {
            let name = kablo_proto::install::artifact_name("0.2.0", target).expect("name");
            std::fs::write(dir.join(&name), KABLO).expect("binary");
            sums.push_str(&format!("{}  {name}\n", sha256_hex(KABLO.as_bytes())));
        }
        std::fs::write(dir.join("SHA256SUMS"), sums).expect("sums");
        Bundle::open(&dir).expect("package")
    }

    /// What the kablo of a version on the stand-in server was asked to `upgrade`.
    fn upgrades(&self, version: &str) -> Vec<String> {
        std::fs::read_to_string(
            self.home
                .join(format!(".local/share/bateri/kablo/{version}/upgrades")),
        )
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect()
    }

    fn calls(&self) -> Vec<String> {
        std::fs::read_to_string(&self.log)
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect()
    }
}

impl Drop for World {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// The real runner with the program named `ssh` replaced by the stand-in: the
/// questions the decision asks (`ssh -G`) never reach a real ssh.
struct Through(PathBuf);

impl SshRunner for Through {
    fn run(&self, argv: &[String]) -> io::Result<(Option<i32>, String, String)> {
        let mut argv = argv.to_vec();
        argv[0] = self.0.to_string_lossy().into_owned();
        SystemSsh.run(&argv)
    }
}

fn run_argv(argv: &[Vec<u8>]) -> u8 {
    let argv: Vec<String> = argv
        .iter()
        .map(|word| String::from_utf8(word.clone()).expect("text"))
        .collect();
    let status = Command::new(&argv[0])
        .args(&argv[1..])
        .status()
        .expect("ran");
    u8::try_from(status.code().expect("a code")).expect("a byte")
}

#[test]
#[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
fn a_kablo_call_is_the_decisions_and_needs_the_package() {
    let world = stand_in("route", 0);
    let args = vec!["rasp".to_owned()];
    let backend = world.backend(Some(world.package()));
    let host = backend.route(&args).expect("a kablo call");
    assert_eq!((host.host.as_str(), host.key.as_str()), ("rasp", "u@h:22"));
    // No package, no kablo call: a development build runs the plain ssh.
    assert!(world.backend(None).route(&args).is_none());
    // A server kablo cannot be kept on is a plain one.
    let mut state = ssh_wrap::HostState::default();
    state.note(Fact::NoKablo, "u@h:22", 1);
    std::fs::write(&world.state, state.render()).expect("state");
    assert!(backend.route(&args).is_none());
    // A call that runs a command is not asked about at all.
    assert!(
        world
            .backend(Some(world.package()))
            .route(&["rasp".to_owned(), "uptime".to_owned()])
            .is_none()
    );
}

#[test]
#[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
fn the_master_is_made_on_the_pane_where_the_files_jobs_look_for_it() {
    let world = stand_in("master", 0);
    let backend = world.backend(Some(world.package()));
    let host = backend
        .route(&["-p".into(), "2222".into(), "rasp".into()])
        .expect("kablo");

    let argv = backend
        .master(&host)
        .expect("a plan")
        .expect("a step on the pane");
    let words: Vec<String> = argv
        .iter()
        .map(|word| String::from_utf8(word.clone()).expect("text"))
        .collect();
    assert_eq!(&words[1..4], ["-M", "-N", "-f"]);
    assert_eq!(words[0], world.ssh.to_string_lossy());
    // At the socket ssh_route names for the account, in this instance's directory.
    let control = words
        .iter()
        .find_map(|word| word.strip_prefix("ControlPath="))
        .expect("a control path");
    assert!(
        control.starts_with(&*world.dir.to_string_lossy()),
        "{control}"
    );
    assert!(words.contains(&"BatchMode=no".to_owned()));
    assert!(
        !words
            .iter()
            .any(|word| word.starts_with("StrictHostKeyChecking"))
    );
    assert!(
        words.windows(2).any(|pair| pair == ["-p", "2222"]),
        "{words:?}"
    );

    // The step ends well: there is a master now, and nothing more to ask.
    assert_eq!(run_argv(&argv), 0);
    backend.master_made(&host, 0).expect("made");
    assert_eq!(backend.master(&host).expect("a plan"), None);

    // How the step ended decides what the pane does: ssh's own failure and a
    // signal end the command (what the user saw is the answer), anything else
    // is the plain ssh's to try.
    for status in [255, 130] {
        assert_eq!(
            backend.master_made(&host, status),
            Err(Refusal::ended("")),
            "{status}"
        );
    }
    assert_eq!(backend.master_made(&host, 1), Err(Refusal::plain()));
    // A step that ended well but made no master we can find has nothing to ride on.
    let lonely = world_without_socket();
    let backend = lonely.backend(Some(lonely.package()));
    let host = backend.route(&["rasp".into()]).expect("kablo");
    assert_eq!(backend.master_made(&host, 0), Err(Refusal::plain()));
}

fn world_without_socket() -> World {
    stand_in("lonely", 0)
}

#[test]
#[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
fn the_install_and_the_bridge_ride_the_master_and_ask_nothing() {
    let world = stand_in("install", 0);
    let backend = world.backend(Some(world.package()));
    let host = backend.route(&["rasp".into()]).expect("kablo");
    let argv = backend.master(&host).expect("plan").expect("step");
    assert_eq!(run_argv(&argv), 0);

    sessions(&world.home, &[("t1", "0.1.0"), ("t2", "0.1.0")]);
    let installed = backend
        .install(&host, &ids(&["t1", "t2"]))
        .expect("installed");
    assert!(installed.fresh);
    assert_eq!(installed.path, ".local/share/bateri/kablo/0.2.0/kablo");
    assert!(world.home.join(&installed.path).exists());
    // Remembered as a server with bateri's files.
    let state = ssh_wrap::load(&world.state);
    assert!(state.knows(Fact::Touched, "u@h:22"));
    assert!(!state.knows(Fact::NoKablo, "u@h:22"));
    // The terminals the ledger has were renewed with the file just written, one by one and no
    // others (never `--all`), from the directory the commands of an ssh start in.
    assert_eq!(
        world.upgrades("0.2.0"),
        [
            "upgrade t1 .local/share/bateri/kablo/0.2.0/kablo",
            "upgrade t2 .local/share/bateri/kablo/0.2.0/kablo"
        ]
    );
    // The second time finds it, and renews nothing: the terminals run it now.
    sessions(&world.home, &[("t1", "0.2.0"), ("t2", "0.2.0")]);
    let channel_calls = |world: &World| {
        world
            .calls()
            .iter()
            .filter(|call| call.contains(" sh -c "))
            .count()
    };
    let before = channel_calls(&world);
    assert!(
        !backend
            .install(&host, &ids(&["t1", "t2"]))
            .expect("found")
            .fresh
    );
    assert_eq!(world.upgrades("0.2.0").len(), 2);
    // A connection that finds nothing to do makes the two calls the look has always made (what
    // the server is, and the one that finds kablo and lists the machine), and no more.
    assert_eq!(channel_calls(&world), before + 2, "{:?}", world.calls());

    // Every command went over the master and could not ask or become one.
    let channels: Vec<String> = world
        .calls()
        .into_iter()
        .filter(|call| call.contains("sh -c"))
        .collect();
    assert!(!channels.is_empty());
    for call in &channels {
        assert!(
            call.starts_with("-T -o BatchMode=yes -o ControlMaster=no"),
            "{call}"
        );
        assert!(call.contains("ControlPath=/tmp/bt-ssh-install"), "{call}");
    }

    // The bridge: the bytes written to it come back from the installed kablo.
    let mut carried = backend.carry(&host, &installed).expect("a bridge");
    carried.writer.write_all(b"hello bridge").expect("written");
    carried.writer.flush().expect("flushed");
    let mut back = vec![0u8; 12];
    carried.reader.read_exact(&mut back).expect("echoed");
    assert_eq!(back, b"hello bridge");
    let last = world.calls().into_iter().last().expect("a call");
    assert!(
        last.ends_with(".local/share/bateri/kablo/0.2.0/kablo attach"),
        "{last}"
    );
    // Letting go ends it. The stand-in `ssh` is a shell with a `cat` under it, so the
    // bridge's input is closed too, as the writer thread does when the bridge is
    // dropped: what is left reads to its end.
    let Carried {
        mut reader,
        writer,
        close,
        ended,
    } = carried;
    close();
    drop(writer);
    let mut rest = Vec::new();
    reader.read_to_end(&mut rest).expect("ended");
    // A bridge that was let go of said nothing: the network, if anything.
    assert_eq!(ended(), Cause::Dropped);
}

/// A bridge that never opened is ended at every attempt, and what its ssh started goes with
/// it: a `ProxyCommand` left behind (a `cloudflared` waiting for a login) was one more process
/// at every attempt. The stand-in's `attach` starts a process of its own in the background, as a
/// proxy does.
#[test]
#[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
fn ending_a_bridge_ends_what_its_ssh_started() {
    let world = stand_in("bridge-group", 0);
    let backend = world.backend(Some(world.package()));
    let host = backend.route(&["rasp".into()]).expect("kablo");
    let argv = backend.master(&host).expect("plan").expect("step");
    assert_eq!(run_argv(&argv), 0);
    let installed = backend.install(&host, &[]).expect("installed");
    let started = world.home.join("started");
    crate::child::tests::write_program(
        &world.home.join(&installed.path),
        &format!(
            "#!/bin/sh\ncase $1 in attach) sleep 60 & echo $! > '{}'; exec cat;; esac\n",
            started.display()
        ),
    );

    let carried = backend.carry(&host, &installed).expect("a bridge");
    let deadline = Instant::now() + Duration::from_secs(10);
    let pid = loop {
        if let Some(pid) = std::fs::read_to_string(&started)
            .ok()
            .and_then(|text| text.trim().parse::<libc::pid_t>().ok())
        {
            break pid;
        }
        assert!(
            Instant::now() < deadline,
            "the bridge's ssh started nothing"
        );
        std::thread::sleep(Duration::from_millis(20));
    };
    (carried.close)();
    // Gone once reaped; its parent went with it, so launchd reaps it.
    let deadline = Instant::now() + Duration::from_secs(5);
    // SAFETY: signal 0 only asks whether the process exists.
    while unsafe { libc::kill(pid, 0) } == 0 {
        assert!(
            Instant::now() < deadline,
            "what the bridge's ssh started outlived the bridge"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// A bridge whose ssh ends by itself leaves nothing of what the ssh started either: the status is
/// read from the ssh, which is reaped for it, and the group goes in the same breath. (The stand-in
/// `attach` starts a process of its own in the background, as a proxy does, and ends with 3.)
#[test]
#[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
fn a_bridge_whose_ssh_ended_by_itself_takes_what_it_started_with_it() {
    let world = stand_in("bridge-self-end", 0);
    let backend = world.backend(Some(world.package()));
    let host = backend.route(&["rasp".into()]).expect("kablo");
    let argv = backend.master(&host).expect("plan").expect("step");
    assert_eq!(run_argv(&argv), 0);
    let installed = backend.install(&host, &[]).expect("installed");
    let started = world.home.join("started");
    crate::child::tests::write_program(
        &world.home.join(&installed.path),
        &format!(
            "#!/bin/sh\ncase $1 in attach) sleep 60 >/dev/null </dev/null & echo $! > '{}'; exit 3;; esac\n",
            started.display()
        ),
    );

    let Carried {
        mut reader,
        writer,
        close,
        ended,
    } = backend.carry(&host, &installed).expect("a bridge");
    let mut rest = Vec::new();
    reader.read_to_end(&mut rest).expect("the stream ends");
    // 3 is neither a missing kablo nor a login: the network's, tried again. What the ssh left
    // holds its stderr, which is not waited for before the leftover is ended.
    let asked_at = Instant::now();
    assert_eq!(ended(), Cause::Dropped);
    assert!(
        asked_at.elapsed() < Duration::from_secs(10),
        "{:?}",
        asked_at.elapsed()
    );
    let pid = std::fs::read_to_string(&started)
        .expect("the bridge started it")
        .trim()
        .parse::<libc::pid_t>()
        .expect("a pid");
    let deadline = Instant::now() + Duration::from_secs(5);
    // SAFETY: signal 0 only asks whether the process exists.
    while unsafe { libc::kill(pid, 0) } == 0 {
        assert!(
            Instant::now() < deadline,
            "what the bridge's ssh started outlived it"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    close();
    drop(writer);
}

/// One login serves the whole first connection: the master the preparation opens first is the only
/// `ssh` that makes a connection (it rides a key as it rides a password); the install's steps, the
/// number and the bridge are multiplexed over it, and none of them can open one of its own or become
/// a master. What a server sees is one login, however many steps the set-up takes.
#[test]
#[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
fn the_first_connection_is_one_login_for_every_step() {
    let world = stand_in("onelogin", 0);
    let backend = world.backend(Some(world.package()));
    let host = backend.route(&["rasp".into()]).expect("kablo");
    let argv = backend.master(&host).expect("plan").expect("step");
    assert_eq!(run_argv(&argv), 0);
    backend.master_made(&host, 0).expect("a master to ride");

    let installed = backend.install(&host, &[]).expect("installed");
    assert!(installed.fresh);
    assert_eq!(backend.number(&host), Some(1));
    let carried = backend.carry(&host, &installed).expect("a bridge");
    let Carried { close, writer, .. } = carried;
    close();
    drop(writer);

    // What asks nothing of the server (`-G`, `-O check`) is not a connection.
    let calls = world.calls();
    let connecting: Vec<&String> = calls
        .iter()
        .filter(|call| {
            !call.split_whitespace().any(|word| word == "-G") && !call.contains("-O check")
        })
        .collect();
    let masters = connecting
        .iter()
        .filter(|call| call.split_whitespace().any(|word| word == "-M"))
        .count();
    assert_eq!(masters, 1, "{calls:?}");
    // The master, and what is left: at least the install's steps, the number and the bridge.
    assert!(connecting.len() >= 5, "{calls:?}");
    for call in connecting
        .iter()
        .filter(|call| !call.split_whitespace().any(|word| word == "-M"))
    {
        assert!(call.contains("-o ControlMaster=no"), "{call}");
        assert!(call.contains("ControlPath=/tmp/bt-ssh-onelogin"), "{call}");
    }
}

/// A person who stops the install as soon as it has begun to write, and notes what the server
/// had of bateri's by then.
struct StopAtOnce {
    folder: PathBuf,
    written: std::sync::atomic::AtomicBool,
}

impl install::Watch for StopAtOnce {
    fn sent(&self, _: u64, _: u64) {}

    fn stopped(&self) -> bool {
        self.written
            .store(self.folder.exists(), std::sync::atomic::Ordering::SeqCst);
        true
    }
}

fn stopper(world: &World) -> StopAtOnce {
    StopAtOnce {
        folder: world.home.join(".local/share/bateri"),
        written: Default::default(),
    }
}

/// *Cancel* while bateri goes up takes back what the install had written (the shells' files
/// ride its look) with the script of *Remove bateri from*, and the record that bateri was there:
/// the next connection asks again, and the server has nothing of bateri's.
#[test]
#[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
fn cancelling_the_set_up_takes_back_what_was_written_and_the_record() {
    let world = stand_in("cancel", 0);
    let backend = world.backend(Some(world.package()));
    let host = backend.route(&["rasp".into()]).expect("kablo");
    let argv = backend.master(&host).expect("plan").expect("step");
    assert_eq!(run_argv(&argv), 0);

    let stop = stopper(&world);
    assert!(backend.install_watched(&host, &[], &stop).is_err());
    // Something of bateri's was on the server when the person stopped it ...
    assert!(stop.written.load(std::sync::atomic::Ordering::SeqCst));
    // ... and none of it is now, nor is the record: the next connection asks again.
    assert!(!world.home.join(".local/share/bateri").exists());
    assert!(!ssh_wrap::load(&world.state).knows(Fact::Touched, "u@h:22"));
    assert_eq!(backend.consent(&host), Consent::Ask);
    assert!(
        world.calls().iter().any(|call| call.contains("rm -rf")),
        "the removal's script ran: {:?}",
        world.calls()
    );
    // It ended no terminal and installed nothing.
    assert!(world.upgrades("0.2.0").is_empty());
}

/// A machine that has a kablo on it, or terminals, may be another computer's: *Cancel* leaves its
/// folder (and ends nothing), and still takes the record back, so that the next connection asks.
#[test]
#[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
fn cancelling_the_set_up_leaves_the_folder_of_a_machine_that_has_terminals_of_someone_else() {
    let world = stand_in("cancelkept", 0);
    let backend = world.backend(Some(world.package()));
    let host = backend.route(&["rasp".into()]).expect("kablo");
    let argv = backend.master(&host).expect("plan").expect("step");
    assert_eq!(run_argv(&argv), 0);
    // Another computer's bateri is there, with a terminal that runs.
    let theirs = world.home.join(".local/share/bateri/kablo/0.1.0/kablo");
    std::fs::create_dir_all(theirs.parent().expect("a directory")).expect("their place");
    crate::child::tests::write_program(&theirs, KABLO);
    sessions(&world.home, &[("t1", "0.1.0")]);

    let stop = stopper(&world);
    assert!(backend.install_watched(&host, &[], &stop).is_err());
    assert!(theirs.exists(), "their kablo stays");
    assert!(!ssh_wrap::load(&world.state).knows(Fact::Touched, "u@h:22"));
    assert!(
        !world.calls().iter().any(|call| call.contains("rm -rf")),
        "no removal script: {:?}",
        world.calls()
    );
}

/// A server this Mac had put bateri on already keeps its record when the person stops an upgrade:
/// only the install was stopped.
#[test]
#[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
fn cancelling_an_upgrade_keeps_the_record_of_a_server_bateri_was_on() {
    let world = stand_in("cancelold", 0);
    let backend = world.backend(Some(world.package()));
    let host = backend.route(&["rasp".into()]).expect("kablo");
    let argv = backend.master(&host).expect("plan").expect("step");
    assert_eq!(run_argv(&argv), 0);
    ssh_wrap::record(
        &world.state,
        Fact::Touched,
        "u@h:22",
        ssh_wrap::unix_now(),
        ssh_wrap::LOCK_PATIENCE,
    )
    .expect("recorded");

    let stop = stopper(&world);
    assert!(backend.install_watched(&host, &[], &stop).is_err());
    assert!(ssh_wrap::load(&world.state).knows(Fact::Touched, "u@h:22"));
}

/// A server the setting says never to write to is not written to by a renewal: no step is made,
/// and the terminals there go on with the kablo they have.
#[test]
#[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
fn a_renewal_writes_nothing_to_a_server_the_setting_refuses() {
    let world = stand_in("refreshoff", 0);
    let mut backend = world.backend(Some(world.package()));
    let host = backend.route(&["rasp".into()]).expect("kablo");
    let argv = backend.master(&host).expect("plan").expect("step");
    assert_eq!(run_argv(&argv), 0);
    backend.settings = Box::new(|| {
        bt_core::Settings::parse("[remote]\nintegration = \"off\"\n")
            .expect("parseable text")
            .settings
    });
    let before = world.calls().len();
    assert!(backend.refresh(&host, &[]).is_none());
    assert_eq!(world.calls().len(), before, "no call was made");
    assert!(!world.home.join(".local/share/bateri").exists());
}

/// A look that does not come back whole says nothing of the machine: *Cancel* then leaves the
/// folder and the record, since what the install wrote may still be there.
#[test]
#[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
fn cancelling_the_set_up_keeps_the_record_when_the_machine_cannot_be_looked_at() {
    let world = stand_in("cancelblind", 0);
    let backend = world.backend(Some(world.package()));
    let host = backend.route(&["rasp".into()]).expect("kablo");
    let argv = backend.master(&host).expect("plan").expect("step");
    assert_eq!(run_argv(&argv), 0);
    let theirs = world.home.join(".local/share/bateri/kablo/0.1.0/kablo");
    std::fs::create_dir_all(theirs.parent().expect("a directory")).expect("their place");
    crate::child::tests::write_program(&theirs, KABLO);
    std::fs::write(world.home.join("listing"), "not a listing").expect("a listing");

    let stop = stopper(&world);
    assert!(backend.install_watched(&host, &[], &stop).is_err());
    assert!(ssh_wrap::load(&world.state).knows(Fact::Touched, "u@h:22"));
    assert!(theirs.exists());
}

/// The server's counter is run over the connection the user made, as a step of its own: kablo's
/// own (`kablo bateri --next-number`, which the agent's command there runs too), in the home the
/// server says (a stand-in that sets it, as an ssh login does).
#[test]
#[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
fn the_counter_is_asked_over_the_master_and_a_build_without_the_package_has_none() {
    let world = stand_in("number", 0);
    let backend = world.backend(Some(world.package()));
    let host = backend.route(&["rasp".into()]).expect("kablo");
    let argv = backend.master(&host).expect("plan").expect("step");
    assert_eq!(run_argv(&argv), 0);
    // kablo is on the server before any terminal there is numbered (the install put it).
    let kablo = world.home.join(".local/share/bateri/kablo/0.2.0/kablo");
    std::fs::create_dir_all(kablo.parent().expect("a directory")).expect("kablo's place");
    crate::child::tests::write_program(&kablo, KABLO);
    assert_eq!(backend.number(&host), Some(1));
    assert_eq!(backend.number(&host), Some(2));
    assert_eq!(
        std::fs::read_to_string(world.home.join(".local/share/bateri/next")).expect("a counter"),
        "3\n"
    );
    let channels: Vec<String> = world
        .calls()
        .into_iter()
        .filter(|call| call.contains("sh -c") && call.contains("ControlMaster=no"))
        .collect();
    assert_eq!(channels.len(), 2);
    for call in &channels {
        assert!(
            call.starts_with("-T -o BatchMode=yes -o ControlMaster=no"),
            "{call}"
        );
    }
    // Nothing of bateri's is put on a server that has no package to put kablo with.
    let asked = world.calls().len();
    let bare = world.backend(None);
    assert_eq!(bare.number(&host), None);
    assert_eq!(world.calls().len(), asked, "no call was made");
}

/// A server whose connection is up is asked over it: the counter, the shells' files of an account
/// the ledger brought back, what the script left and who opened what are programs the bridge
/// starts, and no `ssh` is run for any of them — not even the ones that ask where the master
/// is. A new login for each (seconds, on a server behind a proxy) is what the pane that waits
/// for its number would wait out.
#[test]
#[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
fn the_steps_of_a_server_whose_connection_is_up_run_over_it_and_start_no_ssh() {
    let world = stand_in("kept-steps", 0);
    let mut backend = world.backend(Some(world.package()));
    let host = backend.route(&["rasp".into()]).expect("kablo");
    let kablo = world.home.join(".local/share/bateri/kablo/0.2.0/kablo");
    std::fs::create_dir_all(kablo.parent().expect("a directory")).expect("kablo's place");
    crate::child::tests::write_program(&kablo, KABLO);
    let dir = world.home.join(".local/share/bateri/terminals");
    std::fs::create_dir_all(&dir).expect("dir");
    std::fs::write(dir.join("p3"), "opener p4\nkeep-open yes\n").expect("a record");
    let placing = world.home.join(".local/share/bateri/placing");
    std::fs::create_dir_all(&placing).expect("dir");
    std::fs::write(
        placing.join("p7"),
        "bateri 2\nplace\nterminal p7\nanchor p4\nside below\n",
    )
    .expect("a note");
    let got = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&got);
    backend.notes = Some(Box::new(move |server, notes| {
        sink.lock().expect("lock").push((server, notes));
    }));

    let ran = crate::kablo::service::fake_streams::Ran::default();
    backend.serving(Streams::at_home(&world.home, Arc::clone(&ran)));
    let asked = world.calls().len();

    assert_eq!(backend.number(&host), Some(1));
    assert_eq!(backend.number(&host), Some(2));
    let installed = Installed {
        target: Target::LinuxX86_64,
        version: "0.2.0".to_owned(),
        path: ".local/share/bateri/kablo/0.2.0/kablo".to_owned(),
        fresh: false,
        census: None,
    };
    backend.files(&host, &installed);
    assert!(
        world
            .home
            .join(".local/share/bateri/shell/bash/dock.bash")
            .is_file(),
        "the shells' files were written"
    );
    backend.reachable(&host.key, &host);
    assert_eq!(got.lock().expect("lock").len(), 1);
    assert!(!placing.join("p7").exists(), "read once");
    assert_eq!(backend.records(&host).len(), 1);

    assert_eq!(world.calls().len(), asked, "{:?}", world.calls());
    let ran = ran.lock().expect("lock");
    assert_eq!(ran.len(), 5);
    for words in ran.iter() {
        assert_eq!(words[..2], ["sh", "-c"], "the server's sh reads the step");
    }
    assert!(ran[0][2].ends_with("bateri --next-number"), "{:?}", ran[0]);
}

/// What the server's script left while the Mac was away is read over the master as a step of its
/// own, handed over with the server it came from and removed on the server; a build without the
/// package, or one that places nothing, asks nothing.
#[test]
#[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
fn what_the_script_left_for_the_application_is_read_over_the_master_and_removed() {
    let world = stand_in("notes", 0);
    let host = world
        .backend(Some(world.package()))
        .route(&["rasp".into()])
        .expect("kablo");
    let placing = world.home.join(".local/share/bateri/placing");
    std::fs::create_dir_all(&placing).expect("dir");
    std::fs::write(
        placing.join("p7"),
        "bateri 2\nplace\nterminal p7\nanchor p4\nside below\n",
    )
    .expect("a note");

    let got = Arc::new(Mutex::new(Vec::new()));
    let backend_got = Arc::clone(&got);
    let got_len = |got: &Arc<Mutex<Vec<_>>>| got.lock().expect("lock").len();
    let mut backend = world.backend(Some(world.package()));
    let sink = Arc::clone(&got);
    backend.notes = Some(Box::new(move |server, notes| {
        sink.lock().expect("lock").push((server, notes));
    }));
    backend.reachable("u@rasp:22", &host);
    {
        let got = got.lock().expect("lock");
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].0.account, "u@rasp:22");
        assert_eq!(got[0].0.host, "rasp");
        assert_eq!(got[0].1.len(), 1);
        assert_eq!(got[0].1[0].terminal, crate::agent::Terminal(7));
    }
    assert!(!placing.join("p7").exists(), "read once");

    // Nothing left: one more step on the server, and the application is not told.
    let steps = |world: &World| {
        world
            .calls()
            .into_iter()
            .filter(|call| call.contains("sh -c"))
            .count()
    };
    let before = steps(&world);
    backend.reachable("u@rasp:22", &host);
    assert_eq!(steps(&world), before + 1);
    assert_eq!(got_len(&backend_got), 1);
    // No package: no call at all.
    let bare = world.backend(None);
    let before = steps(&world);
    bare.reachable("u@rasp:22", &host);
    assert_eq!(steps(&world), before);
}

/// Who opened which terminal is read over the master the same way, and left where it is; a build
/// without the package asks nothing.
#[test]
#[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
fn who_opened_what_is_read_over_the_master_and_left_on_the_server() {
    let world = stand_in("records", 0);
    let host = world
        .backend(Some(world.package()))
        .route(&["rasp".into()])
        .expect("kablo");
    let dir = world.home.join(".local/share/bateri/terminals");
    std::fs::create_dir_all(&dir).expect("dir");
    std::fs::write(dir.join("p3"), "opener p4\nkeep-open yes\n").expect("a record");

    let backend = world.backend(Some(world.package()));
    let records = backend.records(&host);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].0, crate::agent::Terminal(3));
    assert_eq!(records[0].1.opener, crate::agent::Anchor::parse("p4"));
    assert!(records[0].1.keep_open);
    assert!(dir.join("p3").exists(), "read, not taken");

    let bare = world.backend(None);
    let before = world.calls().len();
    assert!(bare.records(&host).is_empty());
    assert_eq!(world.calls().len(), before, "no call was made");
}

#[test]
fn what_ssh_says_when_the_bridge_ends_tells_a_login_from_a_network() {
    // A password the server would have taken: the user's to give.
    for said in [
        "u@rasp: Permission denied (publickey,password).\n",
        "Permission denied (publickey,keyboard-interactive).",
    ] {
        assert_eq!(why_it_ended(said, Some(255)), Cause::LoginNeeded, "{said}");
    }
    // A key's refusal is not a password's business, and a network is a network.
    for said in [
        "u@rasp: Permission denied (publickey).\n",
        "ssh: Could not resolve hostname rasp: nodename nor servname provided\n",
        "ssh: connect to host rasp port 22: Operation timed out\n",
        "ssh: connect to host rasp port 22: Connection refused\n",
        "Connection closed by 10.0.0.2 port 22\n",
        "",
    ] {
        assert_eq!(why_it_ended(said, Some(255)), Cause::Dropped, "{said:?}");
        assert_eq!(why_it_ended(said, None), Cause::Dropped, "{said:?}");
    }
    // A server whose key is not the trusted one is not asked about anything.
    for said in [
        "Host key verification failed.\n",
        "@@@ WARNING: REMOTE HOST IDENTIFICATION HAS CHANGED! @@@\n",
    ] {
        assert_eq!(why_it_ended(said, Some(255)), Cause::Refused, "{said}");
    }
}

/// The far shell that could not run kablo says so by its status, in its own words (or in none): a
/// kablo that is not there is mended, not waited for like a network. The words are not read.
#[test]
fn a_status_of_the_far_shell_that_could_not_run_kablo_is_a_kablo_that_is_missing() {
    for said in [
        "",
        "bash: line 1: .local/share/bateri/kablo/0.2.0/kablo: No such file or directory\n",
        "zsh:1: no such file or directory: .local/share/bateri/kablo/0.2.0/kablo\n",
        "fish: Unknown command: .local/share/bateri/kablo/0.2.0/kablo\n",
    ] {
        assert_eq!(why_it_ended(said, Some(127)), Cause::Missing, "{said:?}");
    }
    assert_eq!(
        why_it_ended("", Some(126)),
        Cause::Missing,
        "not executable"
    );
    // Anything else the command ended with is the network's or the program's.
    for status in [Some(0), Some(1), Some(69), Some(255), None] {
        assert_eq!(why_it_ended("", status), Cause::Dropped, "{status:?}");
    }
    // What ssh said about the key or the login comes first.
    assert_eq!(
        why_it_ended("Host key verification failed.\n", Some(127)),
        Cause::Refused
    );
    assert_eq!(
        why_it_ended(
            "u@rasp: Permission denied (publickey,password).\n",
            Some(127)
        ),
        Cause::LoginNeeded
    );
}

/// A bridge whose kablo was removed on the server ends with the far shell's 127, which the ssh
/// carries out; the ssh is waited for, and what ends the bridge afterwards finds nothing to end.
#[test]
#[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
fn a_bridge_whose_kablo_is_gone_from_the_server_ends_as_missing() {
    let world = stand_in("bridge-missing", 0);
    let backend = world.backend(Some(world.package()));
    let host = backend.route(&["rasp".into()]).expect("kablo");
    let argv = backend.master(&host).expect("plan").expect("step");
    assert_eq!(run_argv(&argv), 0);
    let installed = backend.install(&host, &[]).expect("installed");
    std::fs::remove_dir_all(world.home.join(".local/share/bateri/kablo"))
        .expect("removed on the server");

    let Carried {
        mut reader,
        writer,
        close,
        ended,
    } = backend.carry(&host, &installed).expect("a bridge");
    let mut rest = Vec::new();
    reader.read_to_end(&mut rest).expect("the stream ends");
    assert_eq!(ended(), Cause::Missing);
    // The bridge is let go of afterwards, as the service does; the ssh was reaped by `ended` and
    // `close` has nothing to signal.
    close();
    drop(writer);
}

#[test]
#[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
fn a_server_kablo_cannot_be_kept_on_is_written_down_and_a_bad_moment_is_not() {
    // A home that cannot be written to.
    let world = stand_in("impossible", 0);
    std::fs::write(world.home.join(".local"), "in the way").expect("file");
    let backend = world.backend(Some(world.package()));
    let host = backend.route(&["rasp".into()]).expect("kablo");
    let argv = backend.master(&host).expect("plan").expect("step");
    assert_eq!(run_argv(&argv), 0);
    assert_eq!(backend.install(&host, &[]), Err(Refusal::plain()));
    assert!(ssh_wrap::load(&world.state).knows(Fact::NoKablo, "u@h:22"));
    assert!(
        backend.route(&["rasp".into()]).is_none(),
        "and it is a plain ssh from then on"
    );
    drop(world);

    // A connection that fails now is not a verdict.
    let world = stand_in("moment", 0);
    let backend = world.backend(Some(world.package()));
    let host = backend.route(&["rasp".into()]).expect("kablo");
    // The server's home is gone from under the connection: the channel fails.
    std::fs::remove_dir_all(&world.home).expect("gone");
    assert_eq!(backend.install(&host, &[]), Err(Refusal::plain()));
    assert!(!ssh_wrap::load(&world.state).knows(Fact::NoKablo, "u@h:22"));
    // The server is recorded before anything is written, so one the install fell over on still
    // is: whatever removes bateri's files from a server finds it by this, and a person who said
    // yes is not asked again.
    assert!(ssh_wrap::load(&world.state).knows(Fact::Touched, "u@h:22"));
}

/// A server as the route would give it, for the steps that do not run `ssh`.
fn rasp(host: &str) -> KabloHost {
    KabloHost {
        host: host.into(),
        port: None,
        argv: vec!["ssh".into(), host.into()],
        config: String::new(),
        key: "u@h:22".into(),
    }
}

fn with_setting(backend: &mut SshBackend, mode: RemoteIntegration, hosts: Vec<bt_core::HostRule>) {
    backend.settings = Box::new(move || Settings {
        remote_integration: mode,
        remote_hosts: hosts.clone(),
        ..Settings::default()
    });
}

fn entry(pattern: &str, integration: bool) -> bt_core::HostRule {
    bt_core::HostRule {
        pattern: pattern.to_owned(),
        mark: None,
        integration: Some(integration),
        commands: None,
        profiles: None,
    }
}

#[test]
fn a_server_bateri_has_not_been_to_is_asked_about_before_anything_is_done() {
    let world = stand_in("ask", 0);
    let backend = world.backend(None);
    // The default asks: in the pane's dock, once the login is made.
    let asked = backend.consent(&rasp("deploy@rasp"));
    assert_eq!(asked, Consent::Ask);
    // Nothing is written to the settings, to the server's record or to the server.
    assert!(world.saved.lock().expect("saved").is_empty());
    assert!(!world.state.exists());
    assert!(!std::fs::read_to_string(&world.log).is_ok_and(|log| log.contains("rasp")));
}

#[test]
fn a_server_bateri_has_been_to_the_setting_and_the_entry_settle_without_asking() {
    use RemoteIntegration::{Ask, Auto, Off};
    let world = stand_in("consent", 0);
    let mut backend = world.backend(None);
    let server = rasp("deploy@rasp");

    // A server bateri has put its files on is not asked about again, however it got there.
    let mut state = ssh_wrap::HostState::default();
    state.note(Fact::Touched, "u@h:22", 1);
    std::fs::write(&world.state, state.render()).expect("state");
    assert_eq!(backend.consent(&server), Consent::Granted);
    // …but a setting of `off` and an entry that says `false` still win: nothing is written.
    with_setting(&mut backend, Off, vec![]);
    assert_eq!(backend.consent(&server), Consent::Refused);
    with_setting(&mut backend, Ask, vec![entry("rasp", false)]);
    assert_eq!(backend.consent(&server), Consent::Refused);
    std::fs::remove_file(&world.state).expect("state gone");

    // Not been to: `auto` and the host's own `true` go ahead without a question, `off` and
    // `false` refuse, and only `ask` asks.
    with_setting(&mut backend, Auto, vec![]);
    assert_eq!(backend.consent(&server), Consent::Granted);
    with_setting(&mut backend, Ask, vec![entry("rasp", true)]);
    assert_eq!(backend.consent(&server), Consent::Granted);
    with_setting(&mut backend, Off, vec![]);
    assert_eq!(backend.consent(&server), Consent::Refused);
    with_setting(&mut backend, Off, vec![entry("rasp", true)]);
    assert_eq!(backend.consent(&server), Consent::Granted);
    with_setting(&mut backend, Auto, vec![entry("rasp", false)]);
    assert_eq!(backend.consent(&server), Consent::Refused);
    // A production host is colored and decides nothing: it is asked about like the rest.
    backend.settings = Box::new(|| Settings {
        remote_hosts: vec![bt_core::HostRule {
            pattern: "rasp".to_owned(),
            mark: Some(bt_core::HostMark::Production),
            integration: None,
            commands: None,
            profiles: None,
        }],
        ..Settings::default()
    });
    assert_eq!(backend.consent(&server), Consent::Ask);
}

#[test]
fn this_mac_is_never_asked_about() {
    let world = stand_in("mac", 0);
    let backend = world.backend(None);
    let here = local::host();
    assert_eq!(backend.consent(&here), Consent::Granted);
}

#[test]
fn the_answers_are_written_where_the_menu_writes_them() {
    let world = stand_in("answers", 0);
    let backend = world.backend(None);
    let server = rasp("deploy@rasp");
    // Set Up is kept by the install that follows (it records the server first), not here; a
    // Cancel and a row nobody could be asked in keep nothing.
    for answer in [Answer::SetUp, Answer::Cancel, Answer::Unshown] {
        backend.answered(&server, answer);
    }
    assert!(world.saved.lock().expect("saved").is_empty());
    // Plain ssh is this host's own entry, the line Shell ▸ Set Up bateri on “{host}” writes (the other way).
    backend.answered(&server, Answer::Plain);
    assert_eq!(
        *world.saved.lock().expect("saved"),
        [SettingsEdit::RemoteHostIntegration {
            host: "deploy@rasp".into(),
            on: false,
        }]
    );
}

#[test]
fn a_build_without_the_package_has_nothing_to_install() {
    let world = stand_in("package", 0);
    let backend = world.backend(None);
    let host = KabloHost {
        host: "rasp".into(),
        port: None,
        argv: vec!["ssh".into(), "rasp".into()],
        config: String::new(),
        key: "u@h:22".into(),
    };
    assert_eq!(backend.install(&host, &[]), Err(Refusal::plain()));
}

/// The shells' files go to a server whose kablo is where the ledger says without a look at it:
/// the same script, over the connection the bridge takes.
#[test]
#[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
fn the_shells_files_are_written_to_a_server_the_ledger_knew() {
    let world = stand_in("files", 0);
    let mut backend = world.backend(Some(world.package()));
    let host = KabloHost {
        host: "rasp".into(),
        port: None,
        argv: vec!["ssh".into(), "rasp".into()],
        config: String::new(),
        key: "u@h:22".into(),
    };
    let installed = Installed {
        target: Target::LinuxX86_64,
        version: "0.2.0".into(),
        path: ".local/share/bateri/kablo/0.2.0/kablo".into(),
        fresh: false,
        census: None,
    };
    backend.files(&host, &installed);
    let shell = world.home.join(".local/share/bateri/shell");
    assert!(shell.join("bash/dock.bash").is_file());
    assert!(
        std::fs::read_to_string(shell.join("bin/bateri"))
            .expect("the agent's command")
            .contains(".local/share/bateri/kablo/0.2.0/kablo"),
        "kablo's place is where the ledger says"
    );
    // An agent's hook comes with the setting and goes with it, on the next call.
    let hook = shell.join("agents/claude.json");
    assert_eq!(
        std::fs::read_to_string(&hook).expect("the setting is on by default"),
        crate::program::reply::REPLIERS[0].hook
    );
    backend.settings = Box::new(|| Settings {
        agents_claude_integration: false,
        ..Settings::default()
    });
    backend.files(&host, &installed);
    assert!(!hook.exists(), "the hook stayed after the setting went off");
    backend.settings = Box::new(Settings::default);
    backend.files(&host, &installed);
    assert_eq!(
        std::fs::read_to_string(&hook).expect("the hook"),
        crate::program::reply::REPLIERS[0].hook
    );
    // What a reach believes is done for other wants goes when the files are written under these:
    // turned off again, the reach writes them instead of finding them done. The belief that holds
    // stays.
    use crate::ssh_wrap::Wants;
    let off = Wants::of(
        &Settings {
            agents_claude_integration: false,
            ..Settings::default()
        },
        true,
    );
    let on = Wants::of(&Settings::default(), true);
    let (before, now) = (format!("u@h:22#{off:?}"), format!("u@h:22#{on:?}"));
    let other = "u@elsewhere:22#anything".to_owned();
    for belief in [&before, &now, &other] {
        backend.filed.lock().expect("lock").insert(belief.clone());
    }
    backend.files(&host, &installed);
    let filed = backend.filed.lock().expect("lock");
    assert!(
        !filed.contains(&before),
        "a belief about other wants stayed"
    );
    assert!(filed.contains(&now) && filed.contains(&other));
}

/// The dock's tier a server's new terminal gets is the settings' `[shell] integration`, the
/// answer the Mac's own shells get: `"auto"` draws the dock, `"blocks"` and `"off"` do not.
#[test]
#[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
fn the_dock_tier_of_a_new_terminal_is_the_settings() {
    let world = stand_in("tier", 0);
    let mut backend = world.backend(None);
    assert!(backend.dock());
    for (setting, dock) in [
        (bt_core::ShellIntegration::Auto, true),
        (bt_core::ShellIntegration::Blocks, false),
        (bt_core::ShellIntegration::Off, false),
    ] {
        backend.settings = Box::new(move || Settings {
            shell_integration: setting,
            ..Settings::default()
        });
        assert_eq!(backend.dock(), dock, "{setting:?}");
    }
}

#[test]
#[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
fn a_kablo_of_an_older_release_is_replaced_under_the_terminals_it_runs() {
    let world = stand_in("refresh", 0);
    let backend = world.backend(Some(world.package()));
    assert_eq!(backend.version().as_deref(), Some("0.2.0"));
    assert_eq!(world.backend(None).version(), None);
    let host = backend.route(&["rasp".into()]).expect("kablo");
    let argv = backend.master(&host).expect("plan").expect("step");
    assert_eq!(run_argv(&argv), 0);
    // The server has the release the ledger remembers, and its terminals.
    let old = world.home.join(".local/share/bateri/kablo/0.1.0/kablo");
    std::fs::create_dir_all(old.parent().expect("dir")).expect("dir");
    std::fs::write(&old, "#!/bin/sh\n").expect("old kablo");

    sessions(&world.home, &[("t1", "0.1.0")]);
    let installed = backend.refresh(&host, &ids(&["t1"])).expect("renewed");
    assert_eq!(installed.version, "0.2.0");
    assert_eq!(installed.path, ".local/share/bateri/kablo/0.2.0/kablo");
    assert!(installed.fresh, "it was written now");
    assert_eq!(
        world.upgrades("0.2.0"),
        ["upgrade t1 .local/share/bateri/kablo/0.2.0/kablo"]
    );
    assert_eq!(
        std::fs::read(&old).expect("the old one"),
        b"#!/bin/sh\n",
        "the release that terminals run from is left where it is"
    );
    // Every step rode the connection, could not ask, and was not let wait for a
    // network that is not there.
    let channels: Vec<String> = world
        .calls()
        .into_iter()
        .filter(|call| call.contains("sh -c"))
        .collect();
    assert!(channels.len() >= 3, "{channels:?}");
    for call in &channels {
        assert!(
            call.starts_with("-o ConnectTimeout=15 -T -o BatchMode=yes -o ControlMaster=no"),
            "{call}"
        );
    }

    // A renewal asked for again (the first one's answer never arrived) finds the file
    // and renews again: the account is known at the new release only once it was.
    let again = backend
        .refresh(&host, &ids(&["t1"]))
        .expect("renewed again");
    assert!(!again.fresh);
    assert_eq!(world.upgrades("0.2.0").len(), 2);
}

#[test]
#[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
fn a_renewal_that_cannot_be_done_now_says_so_and_writes_nothing_down() {
    // No package: nothing to bring.
    let world = stand_in("refresh-none", 0);
    let host = world
        .backend(Some(world.package()))
        .route(&["rasp".into()])
        .expect("kablo");
    assert!(world.backend(None).refresh(&host, &[]).is_none());

    // A home that cannot be written to now is not a server kablo cannot be kept on:
    // terminals are running there, and the next `ssh` stays a kablo one.
    std::fs::write(world.home.join(".local"), "in the way").expect("file");
    let backend = world.backend(Some(world.package()));
    assert!(backend.refresh(&host, &ids(&["t1"])).is_none());
    assert!(!ssh_wrap::load(&world.state).knows(Fact::NoKablo, "u@h:22"));
    assert!(world.upgrades("0.2.0").is_empty());
}

#[test]
#[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
fn the_application_takes_mail_when_it_brought_a_handler_for_it() {
    let world = stand_in("mail", 0);
    let mut backend = world.backend(None);
    assert!(!backend.takes_mail());
    backend.mail = Some(Box::new(|_| ()));
    assert!(backend.takes_mail());
}

#[test]
#[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
fn this_macs_account_takes_the_same_steps_with_no_server_and_no_connection() {
    let world = stand_in("here", 0);
    let mac = world._root.0.join("mac");
    std::fs::create_dir_all(&mac).expect("this Mac's home");
    let mut backend = world.backend(Some(world.package()));
    backend.home = Some(mac.clone());
    let here = local::host();

    // No connection is made for it, and none is checked.
    assert_eq!(backend.master(&here), Ok(None));
    assert_eq!(backend.master_made(&here, 255), Ok(()));

    // kablo is put below this Mac's home, and the terminals of the ledger are renewed with it
    // (by the file that was just written, from that home).
    sessions(&mac, &[("t1", "0.1.0"), ("t2", "0.1.0")]);
    let installed = backend
        .install(&here, &ids(&["t1", "t2"]))
        .expect("installed");
    assert!(installed.fresh);
    assert_eq!(installed.path, ".local/share/bateri/kablo/0.2.0/kablo");
    assert!(mac.join(&installed.path).is_file());
    let asked = std::fs::read_to_string(mac.join(".local/share/bateri/kablo/0.2.0/upgrades"))
        .expect("the renewal");
    assert_eq!(
        asked.lines().collect::<Vec<_>>(),
        [
            "upgrade t1 .local/share/bateri/kablo/0.2.0/kablo",
            "upgrade t2 .local/share/bateri/kablo/0.2.0/kablo"
        ]
    );
    // Nothing of the stand-in server's was touched, and no ssh ran.
    assert!(!world.home.join(".local/share/bateri/kablo").exists());
    assert!(world.calls().is_empty(), "{:?}", world.calls());
    // The second time finds it, and renews nothing: the terminals run it now.
    sessions(&mac, &[("t1", "0.2.0"), ("t2", "0.2.0")]);
    assert!(!backend.install(&here, &ids(&["t1"])).expect("found").fresh);
    assert_eq!(
        std::fs::read_to_string(mac.join(".local/share/bateri/kablo/0.2.0/upgrades"))
            .expect("the renewal")
            .lines()
            .count(),
        2
    );

    // The `bateri` command is written to it, and nothing else of a server's files: its shells
    // start from the application's own.
    let shell = mac.join(".local/share/bateri/shell");
    assert!(shell.join("bin/bateri").is_file());
    assert!(!shell.join("boot.sh").exists() && !shell.join("zsh").exists());
    // Its terminals are numbered by the counter the command runs, under this Mac's home.
    assert_eq!(backend.number(&here), Some(1));
    assert_eq!(backend.number(&here), Some(2));
    assert_eq!(
        std::fs::read_to_string(mac.join(".local/share/bateri/next")).expect("a counter"),
        "3\n"
    );
    // What only a server has is not taken for it.
    std::fs::remove_file(shell.join("bin/bateri")).expect("removed");
    backend.files(&here, &installed);
    assert!(
        shell.join("bin/bateri").is_file(),
        "the files are written again"
    );
    assert!(backend.records(&here).is_empty());
    assert!(world.calls().is_empty(), "{:?}", world.calls());
    assert!(!world.home.join(".local/share/bateri").exists());

    // The bridge is the file's `attach`, here (the stand-in echoes what it is given).
    let Carried {
        mut reader,
        mut writer,
        close,
        ..
    } = backend.carry(&here, &installed).expect("a bridge");
    writer.write_all(b"ping\n").expect("written");
    let mut back = [0u8; 5];
    reader.read_exact(&mut back).expect("read");
    assert_eq!(&back, b"ping\n");
    close();

    // A release older than the package's is renewed under the terminals it runs, and kept where
    // it is for the relays that run from it.
    let old = mac.join(".local/share/bateri/kablo/0.1.0/kablo");
    std::fs::create_dir_all(old.parent().expect("dir")).expect("dir");
    std::fs::write(&old, "#!/bin/sh\n").expect("an old kablo");
    sessions(&mac, &[("t1", "0.1.0")]);
    let renewed = backend.refresh(&here, &ids(&["t1"])).expect("renewed");
    assert_eq!(renewed.version, "0.2.0");
    assert!(old.exists());
    assert_eq!(
        std::fs::read_to_string(mac.join(".local/share/bateri/kablo/0.2.0/upgrades"))
            .expect("the renewal")
            .lines()
            .count(),
        3
    );
}

/// `[commands] level = "off"` on this Mac: the command is not written, is taken away when an
/// earlier start wrote it, and no terminal is numbered.
#[test]
#[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
fn this_mac_with_its_commands_off_has_no_command_and_no_numbers() {
    let world = stand_in("here-off", 0);
    let mac = world._root.0.join("mac");
    std::fs::create_dir_all(&mac).expect("this Mac's home");
    let here = local::host();
    let level = Arc::new(Mutex::new(CommandLevel::Own));
    let mut backend = world.backend(Some(world.package()));
    backend.home = Some(mac.clone());
    let shared = Arc::clone(&level);
    backend.settings = Box::new(move || Settings {
        commands_level: *shared.lock().expect("lock"),
        ..Settings::default()
    });
    let installed = backend.install(&here, &[]).expect("installed");
    let shell = mac.join(".local/share/bateri/shell");
    assert!(shell.join("bin/bateri").is_file());

    // The level is turned off and the account comes up again (a start of the application, or a
    // renewal): the command goes.
    *level.lock().expect("lock") = CommandLevel::Off;
    backend.install(&here, &[]).expect("found");
    assert!(!shell.join("bin/bateri").exists());
    assert_eq!(backend.number(&here), None);
    assert!(
        !mac.join(".local/share/bateri/next").exists(),
        "no number was given"
    );
    backend.files(&here, &installed);
    assert!(!shell.join("bin/bateri").exists());
    // And back on, the command is written again.
    *level.lock().expect("lock") = CommandLevel::Own;
    backend.files(&here, &installed);
    assert!(shell.join("bin/bateri").is_file());
    assert_eq!(backend.number(&here), Some(1));
}

#[test]
#[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
fn this_macs_account_needs_the_package_and_a_home_to_keep_kablo_in() {
    let world = stand_in("here-bare", 0);
    let here = local::host();
    // A package and no home: no account, and the pane is an ordinary terminal.
    let homeless = world.backend(Some(world.package()));
    assert_eq!(homeless.install(&here, &[]), Err(Refusal::plain()));
    assert!(homeless.refresh(&here, &[]).is_none());
    // A home and no package (a build run from cargo).
    let mut unpacked = world.backend(None);
    unpacked.home = Some(world._root.0.join("mac"));
    assert_eq!(unpacked.install(&here, &[]), Err(Refusal::plain()));
    assert!(unpacked.refresh(&here, &[]).is_none());
    // Neither a failure nor the lack of a package is remembered against the Mac, which is not a
    // server kablo cannot be kept on.
    assert!(!ssh_wrap::load(&world.state).knows(Fact::NoKablo, ""));
}

/// `bateri split -S rasp` right after an update, with no terminal opened on rasp yet: what runs
/// there is this build's kablo and command, not what an earlier build left, and the terminals
/// that run from an earlier kablo are renewed.
#[test]
#[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
fn reaching_a_server_sets_it_up_with_this_builds_kablo_and_command_once_a_run() {
    let world = stand_in("reach-files", 0);
    let backend = world.backend(Some(world.package()));
    let args = vec!["rasp".to_owned()];
    let mut state = ssh_wrap::HostState::default();
    state.note(Fact::Touched, "u@h:22", 1);
    std::fs::write(&world.state, state.render()).expect("state");
    let script = world.home.join(".local/share/bateri/shell/bin/bateri");
    std::fs::create_dir_all(script.parent().expect("a directory")).expect("bin");
    std::fs::write(&script, "an earlier build's").expect("the old command");

    sessions(&world.home, &[("t1", "0.1.0"), ("t2", "0.1.0")]);
    backend
        .reach(&args, Ask::Never, &|_| ids(&["t1", "t2"]))
        .expect("reached");
    assert!(
        std::fs::read_to_string(&script)
            .expect("the command")
            .contains(".local/share/bateri/kablo/0.2.0/kablo"),
        "this build's, with the package's kablo"
    );
    // kablo itself was written (the server had none of this package's), and what ran from the
    // earlier one is renewed with it.
    let kablo = world.home.join(".local/share/bateri/kablo/0.2.0");
    assert!(kablo.join("kablo").is_file(), "no kablo was written");
    let asked = std::fs::read_to_string(kablo.join("upgrades")).expect("the renewal");
    assert_eq!(asked.lines().count(), 2, "{asked}");
    assert!(
        asked.contains("upgrade t1") && asked.contains("upgrade t2"),
        "{asked}"
    );

    // `send` and `capture` reach too: they pay for no second write.
    std::fs::write(&script, "left alone").expect("a marker");
    backend
        .reach(&args, Ask::Never, &|_| ids(&["t1", "t2"]))
        .expect("reached");
    assert_eq!(
        std::fs::read_to_string(&script).expect("the command"),
        "left alone"
    );
    assert_eq!(
        std::fs::read_to_string(kablo.join("upgrades"))
            .expect("the renewal")
            .lines()
            .count(),
        2,
        "nothing was renewed a second time"
    );
}

#[test]
fn a_program_or_a_connection_is_asked_for_only_on_a_server_bateri_has_been() {
    let world = stand_in("streams-gate", 0);
    let backend = world.backend(None);
    let server = KabloHost {
        host: "rasp".to_owned(),
        port: None,
        argv: vec!["ssh".to_owned(), "rasp".to_owned()],
        config: String::new(),
        key: "u@h:22".to_owned(),
    };
    let wait = Duration::from_secs(1);
    let program = || kablo_proto::v1::stream::Command::new(vec![b"true".to_vec()]);

    // A server bateri never opened: told to open a terminal on it, and nothing is asked of it.
    let error = backend
        .command(&server, program(), Ask::Never, wait)
        .err()
        .expect("not set up");
    assert!(
        error.contains("has not set up rasp") && error.contains("ssh rasp"),
        "{error}"
    );
    let error = backend
        .connection(&server, "db", 5432, Ask::Never, wait)
        .err()
        .expect("not set up");
    assert!(error.contains("has not set up rasp"), "{error}");

    // Opened once, the request goes on to the service; one that is not running says so.
    let mut state = ssh_wrap::HostState::default();
    state.note(Fact::Touched, "u@h:22", 1);
    std::fs::write(&world.state, state.render()).expect("state");
    let error = backend
        .command(&server, program(), Ask::Never, wait)
        .err()
        .expect("no service");
    assert_eq!(error, "bateri's connections are not running");
    let error = backend
        .connection(&server, "db", 5432, Ask::Never, wait)
        .err()
        .expect("no service");
    assert_eq!(error, "bateri's connections are not running");
}

#[test]
#[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
fn a_program_reaches_a_server_through_the_gate_of_its_own_pane_and_only_where_bateri_has_been() {
    let world = stand_in("reach", 0);
    let backend = world.backend(Some(world.package()));
    let args = vec!["rasp".to_owned()];

    // A server bateri never opened has no `bateri` to start a terminal with: told to open one.
    // Under the default, which asks, that is also the answer for a program's `-S`: the question is
    // asked in a pane, never by a program, so nothing is put on the server for one.
    let error = backend
        .reach(&args, Ask::Never, &|_| Vec::new())
        .expect_err("not set up");
    assert!(
        error.contains("has not set up rasp") && error.contains("ssh rasp"),
        "{error}"
    );
    assert!(
        !world.home.join(".local").exists(),
        "nothing was put on the server"
    );

    // Opened once, the server is reached: no terminal, no password, no second master.
    let mut state = ssh_wrap::HostState::default();
    state.note(Fact::Touched, "u@h:22", 1);
    std::fs::write(&world.state, state.render()).expect("state");
    let reached = backend
        .reach(&args, Ask::Never, &|_| Vec::new())
        .expect("reached");
    assert_eq!(reached.server.key, "u@h:22");
    assert_eq!(reached.level, CommandLevel::Own);

    // The gate of the server's own pane: a server kablo cannot be kept on, a call that is not a
    // plain interactive one, a package that is not there.
    let mut state = ssh_wrap::HostState::default();
    state.note(Fact::Touched, "u@h:22", 1);
    state.note(Fact::NoKablo, "u@h:22", 1);
    std::fs::write(&world.state, state.render()).expect("state");
    let error = backend
        .reach(&args, Ask::Never, &|_| Vec::new())
        .expect_err("plain");
    assert!(
        error.contains("rasp is not a server bateri keeps terminals on"),
        "{error}"
    );
    let error = world
        .backend(None)
        .reach(&args, Ask::Never, &|_| Vec::new())
        .expect_err("no package");
    assert!(error.contains("not a server bateri keeps"), "{error}");
    let error = backend
        .reach(
            &["rasp".to_owned(), "uptime".to_owned()],
            Ask::Never,
            &|_| Vec::new(),
        )
        .expect_err("a command");
    assert!(error.contains("not a server bateri keeps"), "{error}");

    // A server whose commands are off is reached by nobody: the level says so and no connection is
    // made for it.
    let mut state = ssh_wrap::HostState::default();
    state.note(Fact::Touched, "u@h:22", 1);
    std::fs::write(&world.state, state.render()).expect("state");
    let mut off = world.backend(Some(world.package()));
    off.settings = Box::new(|| Settings {
        remote_hosts: vec![bt_core::HostRule {
            pattern: "rasp".to_owned(),
            mark: None,
            integration: None,
            commands: Some(CommandLevel::Off),
            profiles: None,
        }],
        ..Settings::default()
    });
    let calls = world.calls().len();
    let reached = off.reach(&args, Ask::Never, &|_| Vec::new()).expect("said");
    assert_eq!(reached.level, CommandLevel::Off);
    assert_eq!(
        world.calls().len(),
        calls + 1,
        "only `ssh -G` ran: {:?}",
        world.calls()
    );
}

/// A server has its programs watched when kablo is kept on it, its `bateri` command is wanted and
/// its shells get the integration (which puts the programs' folder on `PATH`); this Mac needs no
/// integration of a server's kind, only the command. A build without watches (the timed run)
/// starts none.
#[test]
fn a_server_has_its_programs_watched_where_the_command_and_the_integration_are_wanted() {
    let world = stand_in("apps-watch", 0);
    let mut backend = world.backend(Some(world.package()));
    let host = backend.route(&["rasp".into()]).expect("kablo");
    assert!(backend.watch_wanted(&host));

    // A server whose shells get no integration has the programs nowhere on its `PATH`.
    backend.settings = Box::new(|| Settings {
        remote_integration: bt_core::RemoteIntegration::Off,
        ..Settings::default()
    });
    assert!(!backend.watch_wanted(&host));

    // Nor one whose `bateri` command is off: nothing of ours is put there.
    backend.settings = Box::new(|| Settings {
        remote_hosts: vec![bt_core::HostRule {
            pattern: "rasp".to_owned(),
            mark: None,
            integration: None,
            commands: Some(CommandLevel::Off),
            profiles: None,
        }],
        ..Settings::default()
    });
    assert!(!backend.watch_wanted(&host));

    // Where kablo is not kept there is nothing to watch with.
    backend.settings = Box::new(Settings::default);
    let mut without = world.backend(None);
    without.settings = Box::new(Settings::default);
    assert!(!without.watch_wanted(&host));
}
