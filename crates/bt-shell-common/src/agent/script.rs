//! The script on the server (`assets/shell/remote/bin/bateri`, its counter `next.sh`), run for real.
//!
//! The server is a temporary home. The files get there the way they do in production: the
//! bootstrap a new kablo terminal runs ([`crate::ssh_wrap::kablo_argv`]) is executed with a login
//! shell that does nothing, and writes them. `kablo` is a stand-in program that keeps a record of
//! every call (its arguments, and what `kablo app` was told) and answers from files, so what the
//! script says to kablo and to the Mac is read back and compared with [`message`](super::message)'s
//! own reading and writing — the script and the Mac cannot drift apart without a test saying so.
//!
//! Every test runs under `/bin/sh` and, where there is one, `dash` (the `sh` of the Linux image the
//! gate runs in): the script is POSIX `sh` and nothing else.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::Terminal;
use super::message::{Place, Request};
use super::place::Side;
use crate::settings::TempRoot;

/// The program stood in for kablo: every call is a line of its arguments (the unit separator
/// between them) in `calls`, `kablo app` keeps what it is told in `app-in.PID` and answers with
/// `app-reply` (or the status in `app-exit`; with neither, 69 as with no Mac), `new` refuses a
/// number with a `taken.tN` file beside it, and the commands the script only passes on exit with
/// `exit.COMMAND` when there is one.
const KABLO: &str = r#"#!/bin/sh
for a in "$@"; do printf '%s\037' "$a"; done >> "$STUB_DIR/calls"
printf '\n' >> "$STUB_DIR/calls"
cmd=$1
case $cmd in
  here)
    if [ -n "${STUB_HERE-}" ]; then
      printf '%s\n' "$STUB_HERE"
      exit 0
    fi
    echo "kablo: this is in no session" >&2
    exit 1
    ;;
  new)
    shift
    while [ $# -gt 0 ]; do
      case $1 in
        --id) id=$2; shift ;;
        --) break ;;
      esac
      shift
    done
    if [ -e "$STUB_DIR/taken.$id" ]; then
      echo "kablo: a session called $id exists already" >&2
      exit 1
    fi
    printf '%s\n' "$id"
    ;;
  app)
    cat > "$STUB_DIR/app-in.$$"
    if [ -e "$STUB_DIR/app-exit" ]; then
      echo "kablo: the app did not take it" >&2
      exit "$(cat "$STUB_DIR/app-exit")"
    fi
    if [ -e "$STUB_DIR/app-reply" ]; then
      cat "$STUB_DIR/app-reply"
      exit 0
    fi
    echo "kablo: no app is connected" >&2
    exit 69
    ;;
  list)
    if [ "${2-}" = --json ]; then echo '[]'; else printf 'ID  PROGRAM\nt1  zsh\n'; fi
    ;;
  *)
    if [ -e "$STUB_DIR/exit.$cmd" ]; then exit "$(cat "$STUB_DIR/exit.$cmd")"; fi
    ;;
esac
"#;

/// Where kablo is, relative to home, as the ledger knows it.
const KABLO_PATH: &str = ".local/share/kablo/0.2.0/kablo";

/// The version the terminals say they run.
const VERSION: &str = "0.19.0";

/// The first `name` on `PATH`.
fn which(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|path| {
        std::env::split_paths(&path)
            .map(|dir| dir.join(name))
            .find(|candidate| candidate.is_file())
    })
}

/// The interpreters the script is run by: `/bin/sh`, and `dash` when it is not that.
fn shells() -> Vec<(&'static str, PathBuf)> {
    let mut found = vec![("sh", PathBuf::from("/bin/sh"))];
    if let Some(dash) = which("dash") {
        let same = std::fs::canonicalize(&dash).ok() == std::fs::canonicalize("/bin/sh").ok();
        if !same {
            found.push(("dash", dash));
        }
    }
    found
}

/// What a run said.
struct Said {
    status: i32,
    out: String,
    err: String,
}

/// A server: a home the bootstrap wrote its files to, with the stand-in kablo in it.
struct Server {
    _root: TempRoot,
    home: PathBuf,
    stub: PathBuf,
}

impl Server {
    fn new(name: &str) -> Server {
        let root = TempRoot::new(&format!("agent-script-{name}"));
        let home = std::fs::canonicalize(&root.0).expect("root").join("home");
        let stub = home.join("stub");
        std::fs::create_dir_all(&stub).expect("stub dir");
        let kablo = home.join(KABLO_PATH);
        std::fs::create_dir_all(kablo.parent().expect("a directory")).expect("kablo dir");
        crate::child::tests::write_program(&kablo, KABLO);
        // The login shell the bootstrap hands over to: one that ends at once.
        let bin = home.join("bin");
        std::fs::create_dir_all(&bin).expect("bin");
        crate::child::tests::write_program(&bin.join("zsh"), "#!/bin/sh\nexit 0\n");
        let server = Server {
            _root: root,
            home,
            stub,
        };
        server.bootstrap();
        server
    }

    /// The bootstrap a new terminal runs, as kablo runs it (an argument vector).
    fn bootstrap(&self) {
        let argv =
            crate::ssh_wrap::kablo_argv(None, None, "00c0ffee00c0ffee", None, Some(KABLO_PATH));
        let words: Vec<String> = argv
            .iter()
            .map(|word| String::from_utf8(word.clone()).expect("text"))
            .collect();
        let out = Command::new(&words[0])
            .args(&words[1..])
            .env_clear()
            .env("HOME", &self.home)
            .env("SHELL", self.home.join("bin/zsh"))
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .stdin(Stdio::null())
            .output()
            .expect("the bootstrap");
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(self.shell_dir().join("bin/bateri").is_file());
    }

    fn state(&self) -> PathBuf {
        self.home.join(".local/share/bateri")
    }

    fn shell_dir(&self) -> PathBuf {
        self.state().join("shell")
    }

    fn script(&self) -> PathBuf {
        self.shell_dir().join("bin/bateri")
    }

    /// The script, run by `shell` as `HERE` says (`None`: outside every terminal).
    fn run(&self, shell: &Path, here: Option<&str>, args: &[&str]) -> Said {
        self.run_file(shell, &self.script(), here, args)
    }

    fn run_file(&self, shell: &Path, file: &Path, here: Option<&str>, args: &[&str]) -> Said {
        let mut command = Command::new(shell);
        command
            .arg(file)
            .args(args)
            .env_clear()
            .env("HOME", &self.home)
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("STUB_DIR", &self.stub)
            .env("LC_TERMINAL_VERSION", VERSION)
            .current_dir(&self.home)
            .stdin(Stdio::null());
        if let Some(here) = here {
            command.env("STUB_HERE", here);
        }
        let out = command.output().expect("the script");
        Said {
            status: out.status.code().expect("a status, not a signal"),
            out: String::from_utf8_lossy(&out.stdout).into_owned(),
            err: String::from_utf8_lossy(&out.stderr).into_owned(),
        }
    }

    /// What kablo was called with, in order, one call a vector.
    fn calls(&self) -> Vec<Vec<String>> {
        let text = std::fs::read_to_string(self.stub.join("calls")).unwrap_or_default();
        text.lines()
            .map(|line| {
                let mut words: Vec<String> = line.split('\u{1f}').map(str::to_owned).collect();
                words.pop();
                words
            })
            .collect()
    }

    /// The calls to kablo with the verb `verb`.
    fn calls_of(&self, verb: &str) -> Vec<Vec<String>> {
        self.calls()
            .into_iter()
            .filter(|call| call.first().map(String::as_str) == Some(verb))
            .collect()
    }

    /// What `kablo app` was told, one text a conversation.
    fn told(&self) -> Vec<String> {
        let mut files = self.told_files();
        files.sort();
        files
            .iter()
            .map(|path| std::fs::read_to_string(path).expect("a message"))
            .collect()
    }

    fn told_files(&self) -> Vec<PathBuf> {
        std::fs::read_dir(&self.stub)
            .expect("stub dir")
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .is_some_and(|name| name.to_string_lossy().starts_with("app-in."))
            })
            .collect()
    }

    fn forget_calls(&self) {
        let _ = std::fs::remove_file(self.stub.join("calls"));
        for file in self.told_files() {
            let _ = std::fs::remove_file(file);
        }
    }

    /// The Mac answers `reply`.
    fn mac_says(&self, reply: &str) {
        std::fs::write(self.stub.join("app-reply"), reply).expect("a reply");
    }

    /// The Mac is not there: kablo's `app` ends with 69 (the default).
    fn no_mac(&self) {
        let _ = std::fs::remove_file(self.stub.join("app-reply"));
        let _ = std::fs::remove_file(self.stub.join("app-exit"));
    }

    /// The conversation fails the way a timeout does: status 1.
    fn mac_is_silent(&self) {
        let _ = std::fs::remove_file(self.stub.join("app-reply"));
        std::fs::write(self.stub.join("app-exit"), "1").expect("a status");
    }

    fn note(&self, terminal: &str) -> Option<String> {
        std::fs::read_to_string(self.state().join("placing").join(terminal)).ok()
    }
}

fn place(terminal: u64, anchor: u64) -> Place {
    Place {
        terminal: Terminal(terminal),
        anchor: Some(Terminal(anchor)),
        side: None,
        ratio: None,
        keep_open: false,
        focus: false,
    }
}

fn encoded(place: Place) -> String {
    Request::Place(place).encode().expect("a message")
}

/// The words of a call as the script was asked to make it: the cwd and `PATH` the new terminal
/// gets are the caller's.
fn new_call(server: &Server, id: &str, program: &[&str]) -> Vec<String> {
    let mut call: Vec<String> = [
        "new",
        "--id",
        id,
        "--cwd",
        &server.home.display().to_string(),
        "--env",
        &format!("PATH={}", std::env::var("PATH").unwrap_or_default()),
        "--",
    ]
    .iter()
    .map(|word| (*word).to_owned())
    .collect();
    call.extend(program.iter().map(|word| (*word).to_owned()));
    call
}

const PLACED: &str = "bateri 1\nok placed\nlanded as_asked\nnext-to t4\nsqueezed no\nhidden no\n";

#[test]
fn the_bootstrap_writes_the_command_and_the_counter_and_the_bootstrap_itself() {
    let server = Server::new("files");
    let shell = server.shell_dir();
    // The command: kablo's place filled in, once; executable.
    let command = std::fs::read_to_string(shell.join("bin/bateri")).expect("the command");
    let asset = include_str!("../../../../assets/shell/remote/bin/bateri");
    assert_eq!(command, asset.replace("__BATERI_KABLO__", KABLO_PATH));
    assert_eq!(
        asset.matches("__BATERI_KABLO__").count(),
        1,
        "one place to fill"
    );
    assert!(!command.contains("__BATERI_KABLO__"));
    let mode = std::os::unix::fs::PermissionsExt::mode(
        &std::fs::metadata(shell.join("bin/bateri"))
            .expect("metadata")
            .permissions(),
    );
    assert_eq!(mode & 0o700, 0o700);
    // The counter and the bootstrap are the assets, byte for byte.
    assert_eq!(
        std::fs::read_to_string(shell.join("next.sh")).expect("the counter"),
        include_str!("../../../../assets/shell/remote/next.sh")
    );
    assert_eq!(
        std::fs::read_to_string(shell.join("boot.sh")).expect("the bootstrap"),
        include_str!("../../../../assets/shell/remote/boot.sh")
    );
}

#[test]
fn a_split_starts_the_terminal_and_tells_the_mac_where_the_pane_goes() {
    for (name, shell) in shells() {
        let server = Server::new(&format!("split-{name}"));
        server.mac_says(PLACED);
        let said = server.run(
            &shell,
            Some("t4"),
            &[
                "split", "--below", "--ratio", "30", "--focus", "--", "npm", "run", "dev",
            ],
        );
        assert_eq!(said.status, 0, "{}: {}", shell.display(), said.err);
        assert_eq!(said.out, "t1\n");
        assert_eq!(said.err, "", "a pane that landed as asked is silent");
        // kablo: who is asking, the terminal (in the caller's directory, with its `PATH`), the
        // conversation.
        let calls = server.calls();
        assert_eq!(calls[0], ["here"]);
        assert_eq!(calls[1], new_call(&server, "t1", &["npm", "run", "dev"]));
        assert_eq!(calls[2], ["app"]);
        // The message is the one the Mac reads, and the one it writes.
        let told = server.told();
        let wanted = Place {
            side: Some(Side::Below),
            ratio: Some(30),
            focus: true,
            ..place(1, 4)
        };
        assert_eq!(told, [encoded(wanted.clone())]);
        assert_eq!(Request::parse(&told[0]), Ok(Request::Place(wanted)));
        // Who opened it, for `close`.
        assert_eq!(
            std::fs::read_to_string(server.state().join("terminals/t1")).expect("a record"),
            "opener t4\nkeep-open no\n"
        );
        // Nothing waits: the Mac took it.
        assert_eq!(server.note("t1"), None);
    }
}

#[test]
fn without_a_mac_the_message_waits_on_the_server_and_the_command_still_answers() {
    for (name, shell) in shells() {
        let server = Server::new(&format!("waiting-{name}"));
        for (args, wanted) in [
            (
                &["split", "--keep-open"][..],
                Place {
                    keep_open: true,
                    ..place(1, 4)
                },
            ),
            (
                &["tab", "t7", "--focus"][..],
                Place {
                    side: Some(Side::Tab),
                    focus: true,
                    anchor: Some(Terminal(7)),
                    ..place(2, 4)
                },
            ),
        ] {
            let said = server.run(&shell, Some("t4"), args);
            assert_eq!(said.status, 0, "{}: {}", shell.display(), said.err);
            let id = format!("t{}", wanted.terminal.0);
            assert_eq!(said.out, format!("{id}\n"));
            assert_eq!(
                said.err,
                format!("bateri: {id} will take its place when a Mac connects\n")
            );
            // The note is the message, whole — the anchor included, since nobody names its
            // sender when it is read — and is read as one.
            let note = server.note(&id).expect("a note");
            assert_eq!(note, encoded(wanted.clone()));
            assert_eq!(Request::parse(&note), Ok(Request::Place(wanted)));
        }
        // The answer as an object.
        let said = server.run(&shell, Some("t4"), &["split", "--json", "--left"]);
        assert_eq!(said.status, 0);
        assert_eq!(
            said.out,
            "{\"terminal\":\"t3\",\"landed\":\"waiting\",\"next_to\":null,\"squeezed\":false,\"hidden\":false}\n"
        );
        // A Mac that is there but does not answer is waited for in the same way.
        server.mac_is_silent();
        let said = server.run(&shell, Some("t4"), &["split"]);
        assert_eq!(said.status, 0);
        assert_eq!(said.out, "t4\n");
        assert!(
            said.err.starts_with("bateri: the Mac did not answer"),
            "{}",
            said.err
        );
        assert!(server.note("t4").is_some());
    }
}

#[test]
fn a_pane_that_did_not_land_as_asked_is_told_on_one_line() {
    let server = Server::new("landing");
    let shell = Path::new("/bin/sh");
    server.mac_says("bateri 1\nok placed\nlanded new_tab\nnext-to t4\nsqueezed yes\nhidden no\n");
    let said = server.run(shell, Some("t4"), &["split", "--json", "--below"]);
    assert_eq!(said.status, 0);
    assert_eq!(
        said.out,
        "{\"terminal\":\"t1\",\"landed\":\"new_tab\",\"next_to\":\"t4\",\"squeezed\":true,\"hidden\":false}\n"
    );
    assert_eq!(
        said.err,
        "bateri: t1 is in a tab of its own: there was no room beside t4\n"
    );
    // The Mac has it, waiting for the anchor to be on screen: that is not "when a Mac connects".
    server.mac_says("bateri 1\nok placed\nlanded waiting\nsqueezed no\nhidden no\n");
    let said = server.run(shell, Some("t4"), &["split", "--json"]);
    assert_eq!(said.status, 0);
    assert_eq!(
        said.out,
        "{\"terminal\":\"t2\",\"landed\":\"waiting\",\"next_to\":null,\"squeezed\":false,\"hidden\":false}\n"
    );
    assert_eq!(
        said.err,
        "bateri: t2 waits in the Mac's list of terminals running in the background, until t4 is on screen\n"
    );
    assert_eq!(
        server.note("t2"),
        None,
        "the Mac has it; nothing waits on the server"
    );
    server.mac_says("bateri 1\nok placed\nlanded as_asked\nnext-to t4\nsqueezed no\nhidden yes\n");
    let said = server.run(shell, Some("t4"), &["split"]);
    assert_eq!(said.out, "t3\n");
    assert_eq!(said.err, "bateri: t3 is behind a pane that fills its tab\n");
}

#[test]
fn a_request_the_mac_refuses_leaves_no_terminal_running() {
    let server = Server::new("refused");
    server.mac_says("bateri 1\nerror refused the user said no\n");
    let said = server.run(Path::new("/bin/sh"), Some("t4"), &["split"]);
    assert_eq!(said.status, 77);
    assert_eq!(said.out, "");
    assert_eq!(said.err, "bateri: the user said no\n");
    assert_eq!(server.calls_of("end"), [["end", "t1", "--force"]]);
    assert!(!server.state().join("terminals/t1").exists());
    assert_eq!(server.note("t1"), None);
    for (kind, status) in [("usage", 64), ("unknown", 1), ("failed", 1), ("version", 1)] {
        server.mac_says(&format!("bateri 1\nerror {kind} it went wrong\n"));
        let said = server.run(Path::new("/bin/sh"), Some("t4"), &["tab"]);
        assert_eq!(said.status, status, "{kind}");
    }
}

#[test]
fn only_a_plain_terminal_number_is_a_target() {
    for (name, shell) in shells() {
        let server = Server::new(&format!("targets-{name}"));
        server.mac_says(PLACED);
        // Labels, bare numbers, a leading zero, a made-up id: none reaches kablo, all are 64.
        for target in ["build", "7", "t07", "T7", "t", "b-0123", "t7x"] {
            for args in [
                vec!["split", target],
                vec!["close", target],
                vec!["wait", target],
                vec!["capture", target],
                vec!["send", target, "text"],
                vec!["send-key", target, "enter"],
            ] {
                let said = server.run(&shell, Some("t4"), &args);
                assert_eq!(said.status, 64, "{args:?}: {}", said.err);
                assert!(
                    said.err.contains("is not a terminal"),
                    "{args:?}: {}",
                    said.err
                );
                assert!(said.err.contains("t1  zsh"), "ls is shown: {}", said.err);
            }
        }
        assert!(server.calls_of("new").is_empty());
        assert!(server.calls_of("end").is_empty());
        assert!(server.calls_of("wait").is_empty());
        assert!(server.calls_of("send").is_empty());
        // The words that are not targets pass: the text of a one-word `send`, a key.
        assert_eq!(server.run(&shell, Some("t4"), &["send", "build"]).status, 0);
        assert_eq!(
            server
                .run(&shell, Some("t4"), &["send-key", "enter"])
                .status,
            0
        );
        assert_eq!(
            server
                .run(&shell, Some("t4"), &["send-key", "Ctrl+C", "enter"])
                .status,
            0
        );
    }
}

#[test]
fn the_commands_that_are_kablos_own_pass_on_with_their_words_and_their_status() {
    for (name, shell) in shells() {
        let server = Server::new(&format!("passing-{name}"));
        let run = |args: &[&str]| server.run(&shell, Some("t4"), args);
        assert_eq!(run(&["send", "t3", "cargo test"]).status, 0);
        assert_eq!(run(&["send-key", "t3", "ctrl+c", "enter"]).status, 0);
        assert_eq!(
            run(&["capture", "t3", "--history", "200", "--trim"]).status,
            0
        );
        assert_eq!(run(&["capture", "--trim"]).status, 0);
        std::fs::write(server.stub.join("exit.wait"), "124").expect("a status");
        let said = run(&["wait", "--for", "10m", "t3"]);
        assert_eq!(
            said.status, 124,
            "the program's own status, not the script's"
        );
        std::fs::write(server.stub.join("exit.wait"), "3").expect("a status");
        assert_eq!(run(&["wait", "t3"]).status, 3);
        let kablo: Vec<Vec<String>> = server
            .calls()
            .into_iter()
            .filter(|call| call[0] != "here")
            .collect();
        assert_eq!(
            kablo,
            [
                vec!["send", "t3", "cargo test"],
                vec!["send-key", "t3", "ctrl+c", "enter"],
                vec!["capture", "t3", "--history", "200", "--trim"],
                vec!["capture", "--trim"],
                vec!["wait", "--for", "10m", "t3"],
                vec!["wait", "t3"],
            ]
        );
        // A `wait` with no terminal is a mistake, not a wait for itself.
        assert_eq!(run(&["wait"]).status, 64);
    }
}

#[test]
fn a_mistake_in_the_command_is_64_before_anything_is_started() {
    for (name, shell) in shells() {
        let server = Server::new(&format!("mistakes-{name}"));
        server.mac_says(PLACED);
        let tries: [(&[&str], Option<&str>); 17] = [
            (&["split", "--ratio", "0"], Some("t4")),
            (&["split", "--ratio", "100"], Some("t4")),
            (&["split", "--ratio", "x"], Some("t4")),
            (&["split", "--ratio"], Some("t4")),
            (&["split", "--left", "--below"], Some("t4")),
            (&["tab", "--left"], Some("t4")),
            (&["tab", "--ratio", "30"], Some("t4")),
            (&["new", "--keep-open"], Some("t4")),
            (&["new", "t4"], Some("t4")),
            (&["split", "t4", "t5"], Some("t4")),
            (&["split", "--bogus"], Some("t4")),
            (&["split"], None),
            (&["tab"], None),
            (&["split"], Some("b-00ff")),
            (&["notify"], Some("t4")),
            (&["frobnicate"], Some("t4")),
            (&[], Some("t4")),
        ];
        for (args, here) in tries {
            let said = server.run(&shell, here, args);
            assert_eq!(said.status, 64, "{args:?} from {here:?}: {}", said.err);
        }
        assert!(server.calls_of("new").is_empty());
        // From outside every terminal, `new` needs no anchor and the number is still given.
        let said = server.run(&shell, None, &["new", "--", "true"]);
        assert_eq!(said.status, 0, "{}", said.err);
        assert_eq!(said.out, "t1\n");
        assert_eq!(
            std::fs::read_to_string(server.state().join("terminals/t1")).expect("a record"),
            "opener -\nkeep-open no\n"
        );
        assert!(server.told().is_empty(), "`new` tells the Mac nothing");
    }
}

#[test]
fn a_number_is_never_given_twice_and_one_a_hand_made_terminal_holds_is_skipped() {
    for (name, shell) in shells() {
        let server = Server::new(&format!("numbers-{name}"));
        assert_eq!(server.run(&shell, None, &["new", "--", "true"]).out, "t1\n");
        assert_eq!(server.run(&shell, None, &["new", "--", "true"]).out, "t2\n");
        // t1 is closed and gone; the next is not t1.
        let _ = std::fs::remove_file(server.state().join("terminals/t1"));
        assert_eq!(server.run(&shell, None, &["new", "--", "true"]).out, "t3\n");
        // A session somebody made with `kablo new --id t4`: kablo refuses the id, the next is tried.
        std::fs::write(server.stub.join("taken.t4"), "").expect("taken");
        std::fs::write(server.stub.join("taken.t5"), "").expect("taken");
        let said = server.run(&shell, None, &["new", "--", "true"]);
        assert_eq!(said.out, "t6\n", "{}", said.err);
        assert_eq!(
            std::fs::read_to_string(server.state().join("next")).expect("the counter"),
            "7\n"
        );
    }
}

#[test]
fn the_counter_gives_each_caller_its_own_number_and_survives_a_dead_lock() {
    for (name, shell) in shells() {
        let server = Server::new(&format!("counter-{name}"));
        let counter = server.shell_dir().join("next.sh");
        let mut children = Vec::new();
        for _ in 0..6 {
            children.push(
                Command::new(&shell)
                    .arg("-c")
                    .arg(format!(
                        "for i in 1 2 3 4 5 6 7 8; do '{}' '{}'; done",
                        shell.display(),
                        counter.display()
                    ))
                    .env_clear()
                    .env("HOME", &server.home)
                    .env("PATH", std::env::var_os("PATH").unwrap_or_default())
                    .stdout(Stdio::piped())
                    .spawn()
                    .expect("a caller"),
            );
        }
        let mut numbers: Vec<u64> = Vec::new();
        for child in children {
            let out = child.wait_with_output().expect("a caller's end");
            assert!(out.status.success());
            numbers.extend(
                String::from_utf8_lossy(&out.stdout)
                    .lines()
                    .map(|line| line.parse::<u64>().expect("a number")),
            );
        }
        numbers.sort_unstable();
        assert_eq!(numbers, (1..=48).collect::<Vec<u64>>(), "every number once");
        assert!(
            !server.state().join("next.lock").exists(),
            "the lock is let go"
        );

        // The counter run on its own: the Mac runs these same bytes over its connection.
        let ask = || {
            Command::new(&shell)
                .arg(&counter)
                .env_clear()
                .env("HOME", &server.home)
                .env("PATH", std::env::var_os("PATH").unwrap_or_default())
                .output()
                .expect("the counter")
        };
        assert_eq!(String::from_utf8_lossy(&ask().stdout), "49\n");

        // A caller that died holding the lock: it is taken over, not waited for ever.
        std::fs::create_dir(server.state().join("next.lock")).expect("a dead lock");
        let started = std::time::Instant::now();
        assert_eq!(String::from_utf8_lossy(&ask().stdout), "50\n");
        assert!(started.elapsed() < std::time::Duration::from_secs(30));

        // A counter that is no number is not guessed at; no home is no counter.
        std::fs::write(server.state().join("next"), "abc\n").expect("a damaged counter");
        let out = ask();
        assert_eq!((out.status.code(), out.stdout.len()), (Some(1), 0));
        assert!(!server.state().join("next.lock").exists());
        let out = Command::new(&shell)
            .arg(&counter)
            .env_clear()
            .output()
            .expect("the counter");
        assert_eq!((out.status.code(), out.stdout.len()), (Some(1), 0));
    }
}

#[test]
fn a_terminal_without_a_program_runs_the_bootstrap_from_its_file() {
    for (name, shell) in shells() {
        let server = Server::new(&format!("shell-{name}"));
        server.mac_says(PLACED);
        let said = server.run(&shell, Some("t4"), &["split"]);
        assert_eq!(said.status, 0, "{}", said.err);
        let boot = server.shell_dir().join("boot.sh");
        // The shell is the integrated one, started the way the one-liner starts it: no parent
        // block, no nonce, no tab.
        let program = ["sh", &boot.display().to_string(), "", "", VERSION, "-"];
        assert_eq!(server.calls_of("new"), [new_call(&server, "t1", &program)]);

        // And run so, the file writes nothing, prints no motd, and hands over to the login shell
        // with the integration's directory (it is where the file is, whatever the environment).
        let stand_in = server.home.join("bin/zsh");
        crate::child::tests::write_program(
            &stand_in,
            "#!/bin/sh\nprintf 'ZDOTDIR=%s LC=%s/%s TAB=%s' \"${ZDOTDIR-}\" \"${LC_TERMINAL-}\" \
             \"${LC_TERMINAL_VERSION-}\" \"${LC_BATERI_TAB_URL-unset}\"\n",
        );
        std::fs::write(server.shell_dir().join("bin/bateri"), "changed").expect("a change");
        let out = Command::new(&shell)
            .arg(&boot)
            .args(["", "", VERSION, "-"])
            .env_clear()
            .env("HOME", &server.home)
            .env("SHELL", &stand_in)
            .env("XDG_DATA_HOME", server.home.join("elsewhere"))
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .stdin(Stdio::null())
            .output()
            .expect("the bootstrap");
        assert!(out.status.success());
        assert_eq!(
            String::from_utf8_lossy(&out.stdout),
            format!(
                "ZDOTDIR={} LC=bateri/{VERSION} TAB=unset",
                server.shell_dir().join("zsh").display()
            ),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        // Nothing was written: the changed file is as it was left, and no other directory exists.
        assert_eq!(
            std::fs::read_to_string(server.shell_dir().join("bin/bateri")).expect("a file"),
            "changed"
        );
        assert!(!server.home.join("elsewhere").exists());
    }
}

#[test]
fn close_ends_what_the_caller_opened_and_asks_the_mac_about_the_rest() {
    for (name, shell) in shells() {
        let server = Server::new(&format!("close-{name}"));
        server.mac_says(PLACED);
        assert_eq!(server.run(&shell, Some("t4"), &["split"]).out, "t1\n");
        server.no_mac();
        assert_eq!(server.run(&shell, Some("t9"), &["split"]).out, "t2\n");
        assert!(server.note("t2").is_some());
        server.forget_calls();

        // Its opener closes it: kablo ends it, nobody is asked, the records go.
        let said = server.run(&shell, Some("t4"), &["close", "t1"]);
        assert_eq!(said.status, 0, "{}", said.err);
        assert_eq!(server.calls_of("end"), [["end", "t1"]]);
        assert!(server.told().is_empty());
        assert!(!server.state().join("terminals/t1").exists());
        // With the words kablo takes.
        server.forget_calls();
        assert_eq!(
            server.run(&shell, Some("t9"), &["close", "--force"]).status,
            77
        );
        let said = server.run(&shell, Some("t9"), &["close", "t2", "--force"]);
        assert_eq!(said.status, 0);
        assert_eq!(server.calls_of("end"), [["end", "t2", "--force"]]);
        assert!(
            server.note("t2").is_none(),
            "a note of a closed terminal goes with it"
        );

        // Somebody else's, with no Mac: refused.
        server.forget_calls();
        assert_eq!(server.run(&shell, Some("t4"), &["split"]).out, "t3\n");
        let said = server.run(&shell, Some("t9"), &["close", "t3"]);
        assert_eq!(said.status, 77);
        assert_eq!(
            said.err,
            "bateri: t3 was not opened by this terminal, and no Mac is connected to ask\n"
        );
        assert!(server.calls_of("end").is_empty());

        // The Mac is asked, and its answer is the status.
        server.forget_calls();
        server.mac_says("bateri 1\nok\n");
        assert_eq!(server.run(&shell, Some("t9"), &["close", "t3"]).status, 0);
        assert_eq!(server.told(), ["bateri 1\nclose\nterminal t3\n"]);
        assert_eq!(
            Request::parse(&server.told()[0]),
            Ok(Request::Close(Terminal(3)))
        );
        server.mac_says("bateri 1\nerror refused the user said no\n");
        let said = server.run(&shell, Some("t9"), &["close", "t3"]);
        assert_eq!(
            (said.status, said.err.as_str()),
            (77, "bateri: the user said no\n")
        );
        // Outside every terminal nothing is the caller's own.
        server.no_mac();
        assert_eq!(server.run(&shell, None, &["close", "t3"]).status, 77);
        assert_eq!(server.run(&shell, None, &["close"]).status, 64);
    }
}

#[test]
fn ls_prints_what_the_mac_says_and_without_one_what_the_server_has() {
    for (name, shell) in shells() {
        let server = Server::new(&format!("ls-{name}"));
        // The Mac's text, byte for byte but for the bytes a terminal would act on.
        server.mac_says("bateri 1\nok text\nWindow 1\n  Tab 1.1\n\u{1b}[31mred\u{7}\n\n");
        let said = server.run(&shell, Some("t4"), &["ls"]);
        assert_eq!(said.status, 0);
        assert_eq!(said.out, "Window 1\n  Tab 1.1\n[31mred\n\n");
        assert_eq!(server.told(), ["bateri 1\nls\n"]);
        server.forget_calls();
        server.mac_says("bateri 1\nok text\n[]\n");
        let said = server.run(&shell, Some("t4"), &["ls", "--json"]);
        assert_eq!(said.out, "[]\n");
        assert_eq!(server.told(), ["bateri 1\nls\nformat json\n"]);

        // No Mac: kablo's own list and what waits to be placed.
        server.no_mac();
        server.run(&shell, Some("t4"), &["split", "--below", "--ratio", "30"]);
        let said = server.run(&shell, Some("t4"), &["ls"]);
        assert_eq!(said.status, 0);
        assert_eq!(
            said.out,
            "ID  PROGRAM\nt1  zsh\n\nWaiting for a Mac to place:\n  t1  below t4 (30%)\n"
        );
        assert!(said.err.starts_with("bateri: no Mac is connected"));
        let said = server.run(&shell, Some("t4"), &["ls", "--json"]);
        assert_eq!(said.out, "[]\n");
        // The Mac's mistakes are the status, as for any request.
        server.mac_says("bateri 1\nerror failed no window\n");
        assert_eq!(server.run(&shell, Some("t4"), &["ls"]).status, 1);
    }
}

#[test]
fn notify_needs_a_mac_and_sends_one_line() {
    for (name, shell) in shells() {
        let server = Server::new(&format!("notify-{name}"));
        let said = server.run(&shell, Some("t4"), &["notify", "the build is done"]);
        assert_eq!(said.status, 69);
        assert_eq!(said.err, "bateri: no Mac is connected to show it\n");
        server.forget_calls();
        server.mac_says("bateri 1\nok\n");
        let said = server.run(
            &shell,
            Some("t4"),
            &["notify", "3 tests\nfailed:", "\u{1b}[31mx\ty"],
        );
        assert_eq!(said.status, 0, "{}", said.err);
        let told = server.told();
        assert_eq!(
            Request::parse(&told[0]),
            Ok(Request::Notify("3 tests failed: [31mx y".to_owned()))
        );
        let long = "x".repeat(1001);
        assert_eq!(
            server.run(&shell, Some("t4"), &["notify", &long]).status,
            64
        );
    }
}

#[test]
fn new_prints_the_number_and_waits_with_the_programs_status() {
    for (name, shell) in shells() {
        let server = Server::new(&format!("new-{name}"));
        let said = server.run(
            &shell,
            Some("t4"),
            &["new", "--json", "--cwd", "~", "--", "make"],
        );
        assert_eq!(said.status, 0, "{}", said.err);
        assert_eq!(said.out, "{\"terminal\":\"t1\"}\n");
        assert_eq!(server.calls_of("new"), [new_call(&server, "t1", &["make"])]);
        std::fs::write(server.stub.join("exit.wait"), "7").expect("a status");
        let said = server.run(&shell, Some("t4"), &["new", "--wait", "--", "make"]);
        assert_eq!((said.status, said.out.as_str()), (7, "t2\n"));
        assert_eq!(server.calls_of("wait"), [["wait", "t2"]]);
        // A directory that is not there is said, and nothing is started.
        let said = server.run(&shell, Some("t4"), &["new", "--cwd", "/nowhere/at/all"]);
        assert_eq!(said.status, 1);
        assert_eq!(server.calls_of("new").len(), 2);
    }
}
