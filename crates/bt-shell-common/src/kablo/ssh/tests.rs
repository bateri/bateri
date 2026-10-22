//! The steps over a stand-in `ssh`: a script that answers `-G` and `-O check`,
//! makes a "master" (a file at the control path) and runs a channel's command
//! for real in a directory that stands for the server's home.

use std::io::{Read, Write};
use std::os::unix::fs::DirBuilderExt;
use std::process::Command;
use std::time::{Duration, Instant};

use super::*;
use crate::kablo::install::sha256_hex;
use crate::settings::TempRoot;
use crate::ssh_route::SystemSsh;
use kablo_proto::install::Target;

/// kablo, as a script: it says its version, `attach` echoes and `upgrade` writes down
/// what it was asked beside the binary.
const KABLO: &str = "#!/bin/sh\ncase $1 in version) echo \"version=0.2.0 wire_min=1 wire_max=2\";; attach) exec cat;; \
     upgrade) echo \"$*\" >>\"$(dirname \"$0\")/upgrades\";; esac\n";

struct World {
    _root: TempRoot,
    dir: PathBuf,
    home: PathBuf,
    log: PathBuf,
    state: PathBuf,
    ssh: PathBuf,
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
  *) cd {home} && HOME={home} SHELL=/bin/zsh XDG_DATA_HOME= exec sh -c "$last";;
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
    }
}

impl World {
    fn backend(&self, bundle: Option<Bundle>) -> SshBackend {
        SshBackend {
            ssh: self.ssh.to_string_lossy().into_owned(),
            bases: vec![self.dir.clone()],
            state_path: self.state.clone(),
            bundle,
            settings: Box::new(Settings::default),
            panes: Box::new(|_| Some(PaneInfo::default())),
            runner: Box::new(Through(self.ssh.clone())),
            name: Some("MacBook".into()),
            history: Box::new(|| Some(5000)),
            masters: None,
            linked: Box::new(|_, _| ()),
            changed: Box::new(|_| ()),
            program: Box::new(|_| ()),
            listed: Box::new(|_| ()),
            mail: None,
            notes: None,
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
                .join(format!(".local/share/kablo/{version}/upgrades")),
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
fn the_install_and_the_bridge_ride_the_master_and_ask_nothing() {
    let world = stand_in("install", 0);
    let backend = world.backend(Some(world.package()));
    let host = backend.route(&["rasp".into()]).expect("kablo");
    let argv = backend.master(&host).expect("plan").expect("step");
    assert_eq!(run_argv(&argv), 0);

    let installed = backend.install(&host).expect("installed");
    assert!(installed.fresh);
    assert_eq!(installed.path, ".local/share/kablo/0.2.0/kablo");
    assert!(world.home.join(&installed.path).exists());
    // Remembered as a server with bateri's files.
    let state = ssh_wrap::load(&world.state);
    assert!(state.knows(Fact::Touched, "u@h:22"));
    assert!(!state.knows(Fact::NoKablo, "u@h:22"));
    // The terminals the server runs were renewed with the file just written, from the
    // directory the commands of an ssh start in.
    assert_eq!(
        world.upgrades("0.2.0"),
        ["upgrade --all .local/share/kablo/0.2.0/kablo"]
    );
    // The second time finds it, and renews nothing.
    assert!(!backend.install(&host).expect("found").fresh);
    assert_eq!(world.upgrades("0.2.0").len(), 1);

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
        last.ends_with(".local/share/kablo/0.2.0/kablo attach"),
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
fn ending_a_bridge_ends_what_its_ssh_started() {
    let world = stand_in("bridge-group", 0);
    let backend = world.backend(Some(world.package()));
    let host = backend.route(&["rasp".into()]).expect("kablo");
    let argv = backend.master(&host).expect("plan").expect("step");
    assert_eq!(run_argv(&argv), 0);
    let installed = backend.install(&host).expect("installed");
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

/// The server's counter is run over the connection the user made, as a step of its own: the same
/// bytes the agent's command there runs, the home the server says (a stand-in that sets it, as an
/// ssh login does).
#[test]
fn the_counter_is_asked_over_the_master_and_a_build_without_the_package_has_none() {
    let world = stand_in("number", 0);
    let backend = world.backend(Some(world.package()));
    let host = backend.route(&["rasp".into()]).expect("kablo");
    let argv = backend.master(&host).expect("plan").expect("step");
    assert_eq!(run_argv(&argv), 0);
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

/// What the server's script left while the Mac was away is read over the master as a step of its
/// own, handed over with the server it came from and removed on the server; a build without the
/// package, or one that places nothing, asks nothing.
#[test]
fn what_the_script_left_for_the_application_is_read_over_the_master_and_removed() {
    let world = stand_in("notes", 0);
    let host = world
        .backend(Some(world.package()))
        .route(&["rasp".into()])
        .expect("kablo");
    let placing = world.home.join(".local/share/bateri/placing");
    std::fs::create_dir_all(&placing).expect("dir");
    std::fs::write(
        placing.join("t7"),
        "bateri 1\nplace\nterminal t7\nanchor t4\nside below\n",
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
    assert!(!placing.join("t7").exists(), "read once");

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
fn who_opened_what_is_read_over_the_master_and_left_on_the_server() {
    let world = stand_in("records", 0);
    let host = world
        .backend(Some(world.package()))
        .route(&["rasp".into()])
        .expect("kablo");
    let dir = world.home.join(".local/share/bateri/terminals");
    std::fs::create_dir_all(&dir).expect("dir");
    std::fs::write(dir.join("t3"), "opener t4\nkeep-open yes\n").expect("a record");

    let backend = world.backend(Some(world.package()));
    let records = backend.records(&host);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].0, crate::agent::Terminal(3));
    assert_eq!(records[0].1.opener, crate::agent::Anchor::parse("t4"));
    assert!(records[0].1.keep_open);
    assert!(dir.join("t3").exists(), "read, not taken");

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
        assert_eq!(why_it_ended(said), Cause::LoginNeeded, "{said}");
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
        assert_eq!(why_it_ended(said), Cause::Dropped, "{said:?}");
    }
    // A server whose key is not the trusted one is not asked about anything.
    for said in [
        "Host key verification failed.\n",
        "@@@ WARNING: REMOTE HOST IDENTIFICATION HAS CHANGED! @@@\n",
    ] {
        assert_eq!(why_it_ended(said), Cause::Refused, "{said}");
    }
}

#[test]
fn a_server_kablo_cannot_be_kept_on_is_written_down_and_a_bad_moment_is_not() {
    // A home that cannot be written to.
    let world = stand_in("impossible", 0);
    std::fs::write(world.home.join(".local"), "in the way").expect("file");
    let backend = world.backend(Some(world.package()));
    let host = backend.route(&["rasp".into()]).expect("kablo");
    let argv = backend.master(&host).expect("plan").expect("step");
    assert_eq!(run_argv(&argv), 0);
    assert_eq!(backend.install(&host), Err(Refusal::plain()));
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
    assert_eq!(backend.install(&host), Err(Refusal::plain()));
    assert!(!ssh_wrap::load(&world.state).knows(Fact::NoKablo, "u@h:22"));
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
    assert_eq!(backend.install(&host), Err(Refusal::plain()));
}

/// The shells' files go to a server whose kablo is where the ledger says without a look at it:
/// the same script, over the connection the bridge takes.
#[test]
fn the_shells_files_are_written_to_a_server_the_ledger_knew() {
    let world = stand_in("files", 0);
    let backend = world.backend(Some(world.package()));
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
        path: ".local/share/kablo/0.2.0/kablo".into(),
        fresh: false,
    };
    backend.files(&host, &installed);
    let shell = world.home.join(".local/share/bateri/shell");
    assert!(shell.join("bash/dock.bash").is_file());
    assert!(
        std::fs::read_to_string(shell.join("bin/bateri"))
            .expect("the agent's command")
            .contains(".local/share/kablo/0.2.0/kablo"),
        "kablo's place is where the ledger says"
    );
}

/// The dock's tier a server's new terminal gets is the settings' `[shell] integration`, the
/// answer the Mac's own shells get: `"auto"` draws the dock, `"blocks"` and `"off"` do not.
#[test]
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
fn a_kablo_of_an_older_release_is_replaced_under_the_terminals_it_runs() {
    let world = stand_in("refresh", 0);
    let backend = world.backend(Some(world.package()));
    assert_eq!(backend.version().as_deref(), Some("0.2.0"));
    assert_eq!(world.backend(None).version(), None);
    let host = backend.route(&["rasp".into()]).expect("kablo");
    let argv = backend.master(&host).expect("plan").expect("step");
    assert_eq!(run_argv(&argv), 0);
    // The server has the release the ledger remembers, and its terminals.
    let old = world.home.join(".local/share/kablo/0.1.0/kablo");
    std::fs::create_dir_all(old.parent().expect("dir")).expect("dir");
    std::fs::write(&old, "#!/bin/sh\n").expect("old kablo");

    let installed = backend.refresh(&host).expect("renewed");
    assert_eq!(installed.version, "0.2.0");
    assert_eq!(installed.path, ".local/share/kablo/0.2.0/kablo");
    assert!(installed.fresh, "it was written now");
    assert_eq!(
        world.upgrades("0.2.0"),
        ["upgrade --all .local/share/kablo/0.2.0/kablo"]
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
    let again = backend.refresh(&host).expect("renewed again");
    assert!(!again.fresh);
    assert_eq!(world.upgrades("0.2.0").len(), 2);
}

#[test]
fn a_renewal_that_cannot_be_done_now_says_so_and_writes_nothing_down() {
    // No package: nothing to bring.
    let world = stand_in("refresh-none", 0);
    let host = world
        .backend(Some(world.package()))
        .route(&["rasp".into()])
        .expect("kablo");
    assert!(world.backend(None).refresh(&host).is_none());

    // A home that cannot be written to now is not a server kablo cannot be kept on:
    // terminals are running there, and the next `ssh` stays a kablo one.
    std::fs::write(world.home.join(".local"), "in the way").expect("file");
    let backend = world.backend(Some(world.package()));
    assert!(backend.refresh(&host).is_none());
    assert!(!ssh_wrap::load(&world.state).knows(Fact::NoKablo, "u@h:22"));
    assert!(world.upgrades("0.2.0").is_empty());
}

#[test]
fn the_application_takes_mail_when_it_brought_a_handler_for_it() {
    let world = stand_in("mail", 0);
    let mut backend = world.backend(None);
    assert!(!backend.takes_mail());
    backend.mail = Some(Box::new(|_| ()));
    assert!(backend.takes_mail());
}
