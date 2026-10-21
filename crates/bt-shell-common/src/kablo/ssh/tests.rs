//! The steps over a stand-in `ssh`: a script that answers `-G` and `-O check`,
//! makes a "master" (a file at the control path) and runs a channel's command
//! for real in a directory that stands for the server's home.

use std::io::{Read, Write};
use std::os::unix::fs::DirBuilderExt;

use super::*;
use crate::kablo::install::sha256_hex;
use crate::settings::TempRoot;
use crate::ssh_route::SystemSsh;
use kablo_proto::install::Target;

/// kablo, as a script: it says its version and `attach` echoes.
const KABLO: &str = "#!/bin/sh\ncase $1 in version) echo \"version=0.2.0 wire_min=1 wire_max=2\";; attach) exec cat;; esac\n";

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
  *) cd {home} && exec sh -c "$last";;
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
    // The second time finds it.
    assert!(!backend.install(&host).expect("found").fresh);

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
    } = carried;
    close();
    drop(writer);
    let mut rest = Vec::new();
    reader.read_to_end(&mut rest).expect("ended");
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
