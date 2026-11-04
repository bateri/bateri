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

use super::message::{Place, Relay, Relayed, Reply, Request};
use super::place::Side;
use super::{Anchor, Terminal};
use crate::settings::TempRoot;

/// The program stood in for kablo: every call is a line of its arguments (the unit separator
/// between them) in `calls`, `kablo app` keeps what it is told in `app-in.NNNNNN` (numbered in
/// the order of the conversations) and answers with
/// `app-reply` (or the status in `app-exit`; with neither, 69 as with no Mac), `new` refuses a
/// number with a `taken.pN` file beside it, and the commands the script only passes on exit with
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
    umask > "$STUB_DIR/umask.new"
    env | grep -E '^(LC_BATERI_TAB_URL|BATERI_TAB_URL|TERM_SESSION_ID)=' > "$STUB_DIR/env.new"
    printf '%s\n' "$id"
    ;;
  app)
    # In the order they were told, whatever the process numbers are.
    seq=$(cat "$STUB_DIR/seq" 2>/dev/null || echo 0)
    seq=$((seq + 1))
    echo "$seq" > "$STUB_DIR/seq"
    cat > "$STUB_DIR/app-in.$(printf '%06d' "$seq")"
    # A conversation with an answer of its own (`app-reply.NNNNNN`) gets that one.
    if [ -e "$STUB_DIR/app-reply.$(printf '%06d' "$seq")" ]; then
      cat "$STUB_DIR/app-reply.$(printf '%06d' "$seq")"
      exit 0
    fi
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
    if [ "${2-}" = --json ]; then echo '[]'; else printf 'ID  PROGRAM\np1  zsh\n'; fi
    ;;
  *)
    if [ -e "$STUB_DIR/exit.$cmd" ]; then exit "$(cat "$STUB_DIR/exit.$cmd")"; fi
    ;;
esac
"#;

/// The `ssh` a Mac that reaches a server runs, stood in for: it records its arguments and runs the
/// last one, the command line, as sshd does — the person's shell (`REMOTE_SHELL`, else `sh`) in the
/// server's home with the environment of a login (`HOME`, `SHELL`, and nothing of the caller's),
/// the server's own stand-in kablo recording in its own directory.
const SSH: &str = r#"#!/bin/sh
for a in "$@"; do printf '%s\037' "$a"; done >> "$STUB_DIR/ssh-calls"
printf '\n' >> "$STUB_DIR/ssh-calls"
for last; do :; done
cd "$SERVER_HOME" || exit 255
exec env -i HOME="$SERVER_HOME" SHELL="${REMOTE_SHELL:-/bin/sh}" PATH="$PATH" STUB_DIR="$SERVER_STUB" \
  LC_TERMINAL_VERSION="${LC_TERMINAL_VERSION-}" "${REMOTE_SHELL:-/bin/sh}" -c "$last"
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

/// The login shells a person may have, those that are installed: the two the script is run by and
/// bash, zsh and fish, which read a line of quotes in three more ways.
fn login_shells() -> Vec<(&'static str, PathBuf)> {
    let mut found = shells();
    for name in ["bash", "zsh", "fish"] {
        if let Some(path) = which(name) {
            found.push((name, path));
        }
    }
    found
}

/// The calls a stand-in recorded: each ends with its unit separator and a line break, and a word
/// may hold a line break of its own.
fn records(file: &Path) -> Vec<Vec<String>> {
    let text = std::fs::read_to_string(file).unwrap_or_default();
    text.split("\u{1f}\n")
        .filter(|record| !record.is_empty())
        .map(|record| record.split('\u{1f}').map(str::to_owned).collect())
        .collect()
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

    /// This Mac: a home with the stand-in kablo in it and the two files the application writes
    /// ([`crate::ssh_wrap::files_script_here`]) — no bootstrap, no shells' files.
    fn new_here(name: &str) -> Server {
        let root = TempRoot::new(&format!("agent-script-{name}"));
        let home = std::fs::canonicalize(&root.0).expect("root").join("home");
        let stub = home.join("stub");
        std::fs::create_dir_all(&stub).expect("stub dir");
        let kablo = home.join(KABLO_PATH);
        std::fs::create_dir_all(kablo.parent().expect("a directory")).expect("kablo dir");
        crate::child::tests::write_program(&kablo, KABLO);
        let server = Server {
            _root: root,
            home,
            stub,
        };
        assert!(crate::ssh_wrap::seed_files_here(&server.home, Some(KABLO_PATH), true).success());
        assert!(server.script().is_file());
        server
    }

    /// The bootstrap a new terminal runs, as kablo runs it (an argument vector).
    fn bootstrap(&self) {
        // The files are on the server before the terminal starts (`install::ensure` wrote them).
        crate::ssh_wrap::seed_files(
            &self.home.join("bin/zsh"),
            &self.home,
            &[],
            Some(KABLO_PATH),
        );
        let argv = crate::ssh_wrap::kablo_argv(None, None, "00c0ffee00c0ffee", None);
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
        self.run_file_with(shell, file, here, args, &[])
    }

    /// [`Self::run_file`], with `extra` in the environment the terminal's shell had.
    fn run_file_with(
        &self,
        shell: &Path,
        file: &Path,
        here: Option<&str>,
        args: &[&str],
        extra: &[(&str, &str)],
    ) -> Said {
        self.run_file_input(shell, file, here, args, extra, None)
    }

    /// [`Self::run_file_with`], with `input` on standard input (none: nothing to read).
    fn run_file_input(
        &self,
        shell: &Path,
        file: &Path,
        here: Option<&str>,
        args: &[&str],
        extra: &[(&str, &str)],
        input: Option<&[u8]>,
    ) -> Said {
        // The caller has a umask of its own (the script must not keep its owner-only one for
        // what it starts) and stands in a tab (which a terminal it starts is not in).
        let mut command = Command::new("/bin/sh");
        command
            .arg("-c")
            .arg("umask 0027; exec \"$0\" \"$@\"")
            .arg(shell)
            .arg(file)
            .args(args)
            .env_clear()
            .env("LC_BATERI_TAB_URL", "bateri://tab/caller")
            .env("BATERI_TAB_URL", "bateri://tab/caller")
            .env("TERM_SESSION_ID", "caller")
            .env("HOME", &self.home)
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("STUB_DIR", &self.stub)
            .env("LC_TERMINAL_VERSION", VERSION)
            .envs(extra.iter().copied())
            .current_dir(&self.home)
            .stdin(if input.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            });
        if let Some(here) = here {
            command.env("STUB_HERE", here);
        }
        let out = if let Some(input) = input {
            use std::io::Write;
            command.stdout(Stdio::piped()).stderr(Stdio::piped());
            let mut child = command.spawn().expect("the script");
            child
                .stdin
                .take()
                .expect("its input")
                .write_all(input)
                .expect("the input");
            child.wait_with_output().expect("the script")
        } else {
            command.output().expect("the script")
        };
        Said {
            status: out.status.code().expect("a status, not a signal"),
            out: String::from_utf8_lossy(&out.stdout).into_owned(),
            err: String::from_utf8_lossy(&out.stderr).into_owned(),
        }
    }

    /// What kablo was called with, in order, one call a vector.
    fn calls(&self) -> Vec<Vec<String>> {
        records(&self.stub.join("calls"))
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

    /// The Mac answers `reply` to its `turn`th conversation (from 1), and what it says for all the
    /// others otherwise.
    fn mac_says_in_turn(&self, turn: usize, reply: &str) {
        std::fs::write(self.stub.join(format!("app-reply.{turn:06}")), reply).expect("a reply");
    }

    /// This Mac runs the stand-in `ssh`, which goes to `remote`: the environment to run the script
    /// with.
    fn ssh_to(&self, remote: &Server) -> Vec<(String, String)> {
        let bin = self.home.join("ssh-bin");
        std::fs::create_dir_all(&bin).expect("bin");
        crate::child::tests::write_program(&bin.join("ssh"), SSH);
        let path = format!(
            "{}:{}",
            bin.display(),
            std::env::var("PATH").unwrap_or_default()
        );
        vec![
            ("PATH".to_owned(), path),
            ("SERVER_HOME".to_owned(), remote.home.display().to_string()),
            ("SERVER_STUB".to_owned(), remote.stub.display().to_string()),
        ]
    }

    /// The words `ssh` was run with, one call a vector.
    fn ssh_calls(&self) -> Vec<Vec<String>> {
        records(&self.stub.join("ssh-calls"))
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
        anchor: Some(Anchor::of(Terminal(anchor))),
        side: None,
        ratio: None,
        keep_open: false,
        focus: false,
        fresh: None,
        server: None,
        home: crate::agent::message::Home::default(),
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

const PLACED: &str = "bateri 2\nok placed\nlanded as_asked\nnext-to p4\nsqueezed no\nhidden no\n";

#[test]
#[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
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
#[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
fn a_split_starts_the_terminal_and_tells_the_mac_where_the_pane_goes() {
    for (name, shell) in shells() {
        let server = Server::new(&format!("split-{name}"));
        server.mac_says(PLACED);
        let said = server.run(
            &shell,
            Some("p4"),
            &[
                "split", "--below", "--ratio", "30", "--focus", "--", "npm", "run", "dev",
            ],
        );
        assert_eq!(said.status, 0, "{}: {}", shell.display(), said.err);
        assert_eq!(said.out, "p1\n");
        assert_eq!(said.err, "", "a pane that landed as asked is silent");
        // kablo: who is asking, the terminal (in the caller's directory, with its `PATH`), the
        // conversation.
        let calls = server.calls();
        assert_eq!(calls[0], ["here"]);
        assert_eq!(calls[1], new_call(&server, "p1", &["npm", "run", "dev"]));
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
            std::fs::read_to_string(server.state().join("terminals/p1")).expect("a record"),
            "opener p4\nkeep-open no\n"
        );
        // Nothing waits: the Mac took it.
        assert_eq!(server.note("p1"), None);
    }
}

#[test]
#[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
fn this_mac_gets_the_command_and_the_counter_and_nothing_else_and_off_takes_them_away() {
    let server = Server::new_here("here-files");
    let shell = server.shell_dir();
    // Under the home, whatever `XDG_DATA_HOME` the application had; no login shell asked after.
    assert!(!server.home.join("elsewhere").exists());
    let command = std::fs::read_to_string(shell.join("bin/bateri")).expect("the command");
    let asset = include_str!("../../../../assets/shell/remote/bin/bateri");
    assert_eq!(command, asset.replace("__BATERI_KABLO__", KABLO_PATH));
    let mode = std::os::unix::fs::PermissionsExt::mode(
        &std::fs::metadata(shell.join("bin/bateri"))
            .expect("metadata")
            .permissions(),
    );
    assert_eq!(mode & 0o700, 0o700);
    assert_eq!(
        std::fs::read_to_string(shell.join("next.sh")).expect("the counter"),
        include_str!("../../../../assets/shell/remote/next.sh")
    );
    // Nothing of a server's: no bootstrap, no shell directories.
    let mut found: Vec<String> = Vec::new();
    for entry in std::fs::read_dir(&shell).expect("the directory") {
        found.push(
            entry
                .expect("an entry")
                .file_name()
                .to_string_lossy()
                .into_owned(),
        );
    }
    found.sort();
    assert_eq!(found, ["bin", "next.sh"]);
    // Written again it is the same (no rename on every start), and `off` takes both away.
    assert!(crate::ssh_wrap::seed_files_here(&server.home, Some(KABLO_PATH), true).success());
    assert!(crate::ssh_wrap::seed_files_here(&server.home, Some(KABLO_PATH), false).success());
    assert!(!shell.join("bin/bateri").exists() && !shell.join("next.sh").exists());
}

#[test]
#[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
fn a_split_with_no_program_on_this_mac_starts_nothing_and_asks_for_a_pane_of_its_own() {
    for (name, shell) in shells() {
        let server = Server::new_here(&format!("here-fresh-{name}"));
        server.mac_says(PLACED);
        let said = server.run(&shell, Some("p4"), &["split", "--right", "--cwd", "~"]);
        assert_eq!(said.status, 0, "{}: {}", shell.display(), said.err);
        assert_eq!(said.out, "p1\n");
        // No terminal was started: the Mac opens the pane under the number.
        assert_eq!(server.calls_of("new"), Vec::<Vec<String>>::new());
        let wanted = Place {
            side: Some(Side::Right),
            fresh: Some(server.home.display().to_string()),
            server: None,
            ..place(1, 4)
        };
        assert_eq!(server.told(), [encoded(wanted)]);
        // The record of who opened it is written all the same.
        assert!(server.state().join("terminals/p1").is_file());

        // `--wait` has nothing to wait for; no terminal to go beside is a mistake of its own;
        // both before anything is numbered.
        let said = server.run(&shell, Some("p4"), &["split", "--wait"]);
        assert_eq!(said.status, 64, "{}", said.err);
        let said = server.run(&shell, None, &["split"]);
        assert_eq!(said.status, 1, "{}", said.err);
        assert!(said.err.contains("beside a pane"), "{}", said.err);
        assert_eq!(server.told().len(), 1);

        // The Mac refuses: the number is spent, and nothing is left behind.
        server.mac_says("bateri 2\nerror failed no room\n");
        let said = server.run(&shell, Some("p4"), &["tab"]);
        assert_eq!(said.status, 1, "{}", said.err);
        assert!(!server.state().join("terminals/p2").exists());
    }
}

#[test]
#[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
fn a_program_kept_open_on_this_mac_is_followed_by_the_login_shell_in_its_terminal() {
    for (name, shell) in shells() {
        let here = Server::new_here(&format!("here-keep-{name}"));
        here.mac_says(PLACED);
        let said = here.run(
            &shell,
            Some("p4"),
            &["split", "--keep-open", "--", "make", "test"],
        );
        assert_eq!(said.status, 0, "{}: {}", shell.display(), said.err);
        let calls = here.calls_of("new");
        assert_eq!(calls.len(), 1);
        let call = &calls[0];
        let at = call
            .iter()
            .position(|word| word == "--")
            .expect("the program");
        assert_eq!(
            call[at + 1..],
            [
                "sh",
                "-c",
                "\"$@\"; exec \"${SHELL:-/bin/sh}\" -l",
                "sh",
                "make",
                "test"
            ]
        );
        let wanted = Place {
            keep_open: true,
            ..place(1, 4)
        };
        assert_eq!(here.told(), [encoded(wanted)]);

        // Without `--keep-open`, and on a server (its Mac holds the terminal), the program is
        // started as it was named.
        here.forget_calls();
        here.run(&shell, Some("p4"), &["split", "--", "make", "test"]);
        let plain = &here.calls_of("new")[0];
        assert_eq!(plain[plain.len() - 3..], ["--", "make", "test"]);
        let server = Server::new(&format!("server-keep-{name}"));
        server.mac_says(PLACED);
        server.run(
            &shell,
            Some("p4"),
            &["split", "--keep-open", "--", "make", "test"],
        );
        assert_eq!(
            server.calls_of("new"),
            [new_call(&server, "p1", &["make", "test"])]
        );
    }
}

#[test]
#[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
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
                &["tab", "p7", "--focus"][..],
                Place {
                    side: Some(Side::Tab),
                    focus: true,
                    anchor: Some(Anchor::of(Terminal(7))),
                    ..place(2, 4)
                },
            ),
        ] {
            let said = server.run(&shell, Some("p4"), args);
            assert_eq!(said.status, 0, "{}: {}", shell.display(), said.err);
            let id = format!("p{}", wanted.terminal.0);
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
        let said = server.run(&shell, Some("p4"), &["split", "--json", "--left"]);
        assert_eq!(said.status, 0);
        assert_eq!(
            said.out,
            "{\"pane\":\"p3\",\"tab\":null,\"group\":null,\"window\":null,\"landed\":\"waiting\",\"next_to\":null,\"squeezed\":false,\"hidden\":false}\n"
        );
        // A Mac that is there but does not answer is waited for in the same way.
        server.mac_is_silent();
        let said = server.run(&shell, Some("p4"), &["split"]);
        assert_eq!(said.status, 0);
        assert_eq!(said.out, "p4\n");
        assert!(
            said.err.starts_with("bateri: the Mac did not answer"),
            "{}",
            said.err
        );
        assert!(server.note("p4").is_some());
    }
}

#[test]
#[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
fn a_pane_that_landed_says_its_tab_group_and_window() {
    let server = Server::new("standing");
    let shell = Path::new("/bin/sh");
    // The Mac says where the pane stands, and the object has the four names always.
    server.mac_says(
        "bateri 2\nok placed\nlanded as_asked\nnext-to p4\nsqueezed no\nhidden no\ntab t3\ngroup g1\nwindow w1\n",
    );
    let said = server.run(shell, Some("p4"), &["split", "--json"]);
    assert_eq!(said.status, 0);
    assert_eq!(
        said.out,
        "{\"pane\":\"p1\",\"tab\":\"t3\",\"group\":\"g1\",\"window\":\"w1\",\"landed\":\"as_asked\",\"next_to\":\"p4\",\"squeezed\":false,\"hidden\":false}\n"
    );
    // A tab in no group: `null` for the group, the other two as given.
    server.mac_says(
        "bateri 2\nok placed\nlanded as_asked\nnext-to p4\nsqueezed no\nhidden no\ntab t8\nwindow w2\n",
    );
    let said = server.run(shell, Some("p4"), &["split", "--json"]);
    assert_eq!(
        said.out,
        "{\"pane\":\"p2\",\"tab\":\"t8\",\"group\":null,\"window\":\"w2\",\"landed\":\"as_asked\",\"next_to\":\"p4\",\"squeezed\":false,\"hidden\":false}\n"
    );
    // A name of another kind, or written another way, is not taken from the Mac.
    server.mac_says(
        "bateri 2\nok placed\nlanded as_asked\nnext-to p4\nsqueezed no\nhidden no\ntab g1\ngroup t03\nwindow w1;rm\n",
    );
    let said = server.run(shell, Some("p4"), &["split", "--json"]);
    assert_eq!(
        said.out,
        "{\"pane\":\"p3\",\"tab\":null,\"group\":null,\"window\":null,\"landed\":\"as_asked\",\"next_to\":\"p4\",\"squeezed\":false,\"hidden\":false}\n"
    );
    // Without --json the number is all there is.
    let said = server.run(shell, Some("p4"), &["split"]);
    assert_eq!(said.out, "p4\n");
}

#[test]
#[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
fn a_mac_that_speaks_the_version_before_is_not_read_and_the_pane_waits() {
    let server = Server::new("oldmac");
    let shell = Path::new("/bin/sh");
    // The reply is of version 1, which nothing here reads: it is told, nothing else is taken
    // from it, and the pane is left to be placed when a Mac that reads this connects.
    server.mac_says("bateri 1\nok placed\nlanded as_asked\nnext-to p4\nsqueezed no\nhidden no\n");
    let said = server.run(shell, Some("p4"), &["split", "--json"]);
    assert_eq!(said.status, 0, "{}", said.err);
    assert!(
        said.err
            .contains("answered with something this does not read"),
        "{}",
        said.err
    );
    assert_eq!(
        said.out,
        "{\"pane\":\"p1\",\"tab\":null,\"group\":null,\"window\":null,\"landed\":\"waiting\",\"next_to\":null,\"squeezed\":false,\"hidden\":false}\n"
    );
}

#[test]
#[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
fn a_pane_that_did_not_land_as_asked_is_told_on_one_line() {
    let server = Server::new("landing");
    let shell = Path::new("/bin/sh");
    server.mac_says("bateri 2\nok placed\nlanded new_tab\nnext-to p4\nsqueezed yes\nhidden no\n");
    let said = server.run(shell, Some("p4"), &["split", "--json", "--below"]);
    assert_eq!(said.status, 0);
    assert_eq!(
        said.out,
        "{\"pane\":\"p1\",\"tab\":null,\"group\":null,\"window\":null,\"landed\":\"new_tab\",\"next_to\":\"p4\",\"squeezed\":true,\"hidden\":false}\n"
    );
    assert_eq!(
        said.err,
        "bateri: p1 is in a tab of its own: there was no room beside p4\n"
    );
    // The Mac has it, waiting for the anchor to be on screen: that is not "when a Mac connects".
    server.mac_says("bateri 2\nok placed\nlanded waiting\nnext-to p4\nsqueezed no\nhidden no\n");
    let said = server.run(shell, Some("p4"), &["split", "--json"]);
    assert_eq!(said.status, 0);
    assert_eq!(
        said.out,
        "{\"pane\":\"p2\",\"tab\":null,\"group\":null,\"window\":null,\"landed\":\"waiting\",\"next_to\":\"p4\",\"squeezed\":false,\"hidden\":false}\n"
    );
    assert_eq!(
        said.err,
        "bateri: p2 waits in the Mac's list of terminals running in the background, until p4 is on screen\n"
    );
    assert_eq!(
        server.note("p2"),
        None,
        "the Mac has it; nothing waits on the server"
    );
    // The Mac could not make the pane just now and kept nothing to place it by: it does not say
    // the pane will come when the anchor is on screen.
    server.mac_says("bateri 2\nok placed\nlanded waiting\nsqueezed no\nhidden no\n");
    let said = server.run(shell, Some("p4"), &["split"]);
    assert_eq!(said.out, "p3\n");
    assert_eq!(
        said.err,
        "bateri: p3 waits in the Mac's list of terminals running in the background: the Mac could not make its pane now\n"
    );
    server.mac_says("bateri 2\nok placed\nlanded as_asked\nnext-to p4\nsqueezed no\nhidden yes\n");
    let said = server.run(shell, Some("p4"), &["split"]);
    assert_eq!(said.out, "p4\n");
    assert_eq!(said.err, "bateri: p4 is behind a pane that fills its tab\n");
}

/// A terminal the Mac opened before the numbers existed, and one a person made with kablo.
const MADE: &str = "b-0123456789abcdef0123456789abcdef";

#[test]
#[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
fn the_callers_own_terminal_is_the_anchor_whatever_it_is_called() {
    for (name, shell) in shells() {
        let server = Server::new(&format!("anchor-{name}"));
        for here in [MADE, "mine", "build.2", "t9"] {
            server.forget_calls();
            server.mac_says(&format!(
                "bateri 2\nok placed\nlanded as_asked\nnext-to {here}\nsqueezed no\nhidden no\n"
            ));
            let said = server.run(&shell, Some(here), &["split", "--below", "--json"]);
            assert_eq!(said.status, 0, "{here}: {}", said.err);
            let id = server
                .calls_of("new")
                .first()
                .map(|call| call[2].clone())
                .expect("a terminal");
            // The Mac is told the anchor by the name kablo gave the terminal, in a message it
            // reads, and says the same name back.
            let told = server.told();
            let wanted = Place {
                terminal: Terminal::parse(&id).expect("a number"),
                anchor: Anchor::parse(here),
                side: Some(Side::Below),
                ..place(1, 1)
            };
            assert_eq!(told, [encoded(wanted.clone())], "{here}");
            assert_eq!(Request::parse(&told[0]), Ok(Request::Place(wanted)));
            assert_eq!(
                said.out,
                format!(
                    "{{\"pane\":\"{id}\",\"tab\":null,\"group\":null,\"window\":null,\"landed\":\"as_asked\",\"next_to\":\"{here}\",\"squeezed\":false,\"hidden\":false}}\n"
                ),
                "{here}"
            );
            assert_eq!(said.err, "", "{here}");
            // Who opened it, so that it can close what it opened.
            assert_eq!(
                std::fs::read_to_string(server.state().join("terminals").join(&id))
                    .expect("a record"),
                format!("opener {here}\nkeep-open no\n")
            );
            let closed = server.run(&shell, Some(here), &["close", "pane", &id]);
            assert_eq!(closed.status, 0, "{here}: {}", closed.err);
            assert_eq!(
                server.calls_of("end").last(),
                Some(&vec!["end".to_owned(), id])
            );
        }
        // A `tab` from such a terminal has the same anchor; one named is still only a number.
        server.mac_says(PLACED);
        let said = server.run(&shell, Some(MADE), &["tab"]);
        assert_eq!(said.status, 0, "{}", said.err);
        let said = server.run(&shell, Some(MADE), &["split", MADE]);
        assert_eq!(said.status, 64, "an anchor the agent writes is a number");
        assert!(said.err.contains("is not a pane"), "{}", said.err);
    }
}

#[test]
#[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
fn a_pane_that_waits_for_the_terminal_that_asked_says_so_without_its_long_name() {
    let server = Server::new("anchor-waiting");
    let shell = Path::new("/bin/sh");
    // The Mac has it, waiting for the anchor to be on screen.
    server.mac_says(&format!(
        "bateri 2\nok placed\nlanded waiting\nnext-to {MADE}\nsqueezed no\nhidden no\n"
    ));
    let said = server.run(shell, Some(MADE), &["split"]);
    assert_eq!(
        said.err,
        "bateri: p1 waits in the Mac's list of terminals running in the background, until this terminal is on screen\n"
    );
    // No Mac: the note is the message, anchor and all, and is read as one.
    server.no_mac();
    let said = server.run(shell, Some(MADE), &["split", "--right", "--keep-open"]);
    assert_eq!(
        said.err,
        "bateri: p2 will take its place when a Mac connects\n"
    );
    let wanted = Place {
        terminal: Terminal(2),
        anchor: Anchor::parse(MADE),
        side: Some(Side::Right),
        keep_open: true,
        ..place(2, 1)
    };
    let note = server.note("p2").expect("a note");
    assert_eq!(note, encoded(wanted.clone()));
    assert_eq!(Request::parse(&note), Ok(Request::Place(wanted)));
    // A terminal that has a number is called by it.
    server.mac_says("bateri 2\nok placed\nlanded new_tab\nnext-to p4\nsqueezed no\nhidden no\n");
    let said = server.run(shell, Some("p4"), &["tab"]);
    assert_eq!(
        said.err,
        "bateri: p3 is in a tab of its own: there was no room beside p4\n"
    );
    let said = server.run(shell, Some(MADE), &["tab"]);
    assert_eq!(
        said.err,
        "bateri: p4 is in a tab of its own: there was no room beside this terminal\n"
    );
}

#[test]
#[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
fn run_in_no_terminal_a_pane_with_no_anchor_leaves_the_terminal_in_the_list() {
    for (name, shell) in shells() {
        let server = Server::new(&format!("cron-{name}"));
        server.mac_says(PLACED);
        for (mode, number) in [("split", 1), ("tab", 2)] {
            let said = server.run(&shell, None, &[mode, "--keep-open", "--", "sleep", "60"]);
            assert_eq!(said.status, 0, "{mode}: {}", said.err);
            assert_eq!(said.out, format!("p{number}\n"));
            assert_eq!(
                said.err,
                format!(
                    "bateri: p{number} waits in the Mac's list of terminals running in the background: this ran in no terminal, so it has no pane to go beside\n"
                )
            );
            // The terminal runs, nobody is asked where to put a pane and no note waits.
            assert_eq!(
                server.calls_of("new").last(),
                Some(&new_call(&server, &format!("p{number}"), &["sleep", "60"]))
            );
            assert_eq!(server.note(&format!("p{number}")), None);
            assert_eq!(
                std::fs::read_to_string(server.state().join(format!("terminals/p{number}")))
                    .expect("a record"),
                "opener -\nkeep-open yes\n"
            );
        }
        assert!(server.calls_of("app").is_empty());
        assert!(server.told().is_empty());
        // The answer as an object says it waits.
        let said = server.run(&shell, None, &["split", "--json", "--below"]);
        assert_eq!(said.status, 0);
        assert_eq!(
            said.out,
            "{\"pane\":\"p3\",\"tab\":null,\"group\":null,\"window\":null,\"landed\":\"waiting\",\"next_to\":null,\"squeezed\":false,\"hidden\":false}\n"
        );
        // A name the agent wrote is an anchor even from outside every terminal.
        let said = server.run(&shell, None, &["split", "p4"]);
        assert_eq!(said.status, 0, "{}", said.err);
        assert_eq!(
            server.told(),
            [encoded(Place {
                terminal: Terminal(4),
                ..place(4, 4)
            })]
        );
        // A name that is not a name kablo allows is no terminal either.
        let said = server.run(&shell, Some("a b/c"), &["split"]);
        assert_eq!(said.status, 0, "{}", said.err);
        assert!(said.err.contains("ran in no terminal"), "{}", said.err);
    }
}

#[test]
#[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
fn a_request_the_mac_refuses_leaves_no_terminal_running() {
    let server = Server::new("refused");
    server.mac_says("bateri 2\nerror refused the user said no\n");
    let said = server.run(Path::new("/bin/sh"), Some("p4"), &["split"]);
    assert_eq!(said.status, 77);
    assert_eq!(said.out, "");
    assert_eq!(said.err, "bateri: the user said no\n");
    assert_eq!(server.calls_of("end"), [["end", "p1", "--force"]]);
    assert!(!server.state().join("terminals/p1").exists());
    assert_eq!(server.note("p1"), None);
    for (kind, status) in [("usage", 64), ("unknown", 1), ("failed", 1), ("version", 1)] {
        server.mac_says(&format!("bateri 2\nerror {kind} it went wrong\n"));
        let said = server.run(Path::new("/bin/sh"), Some("p4"), &["tab"]);
        assert_eq!(said.status, status, "{kind}");
    }
}

#[test]
#[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
fn the_terminal_it_starts_has_the_callers_umask_and_none_of_the_callers_tab() {
    use std::os::unix::fs::PermissionsExt;
    for (name, shell) in shells() {
        let server = Server::new(&format!("umask-{name}"));
        server.mac_says(PLACED);
        let said = server.run(&shell, Some("p4"), &["split", "--", "make"]);
        assert_eq!(said.status, 0, "{}", said.err);
        // What kablo started it with is the caller's, not the owner-only one the state is written with.
        assert_eq!(
            std::fs::read_to_string(server.stub.join("umask.new")).expect("umask"),
            "0027\n"
        );
        // And not the tab the caller stands in: that tab's pane is not this terminal's.
        assert_eq!(
            std::fs::read_to_string(server.stub.join("env.new")).expect("env"),
            ""
        );
        let record = std::fs::metadata(server.state().join("terminals/p1")).expect("a record");
        assert_eq!(record.permissions().mode() & 0o777, 0o600);
    }
}

/// The dock's tier travels: a terminal the command starts gets `BATERI_DOCK=off` where the
/// shell it runs in has it (the Mac's `"blocks"`), and nothing of the kind where it has not.
#[test]
#[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
fn the_terminal_it_starts_gets_the_docks_tier_of_the_one_that_asked() {
    for (name, shell) in shells() {
        let server = Server::new(&format!("dock-{name}"));
        server.mac_says(PLACED);
        let said = server.run_file_with(
            &shell,
            &server.script(),
            Some("p4"),
            &["split", "--", "make"],
            &[("BATERI_DOCK", "off")],
        );
        assert_eq!(said.status, 0, "{name}: {}", said.err);
        let mut call = new_call(&server, "p1", &["make"]);
        call.insert(7, "--env".to_owned());
        call.insert(8, "BATERI_DOCK=off".to_owned());
        assert_eq!(server.calls_of("new"), [call], "{name}");

        server.forget_calls();
        server.mac_says(PLACED);
        let said = server.run(&shell, Some("p4"), &["split", "--", "make"]);
        assert_eq!(said.status, 0, "{name}: {}", said.err);
        assert!(
            server
                .calls_of("new")
                .iter()
                .all(|call| !call.iter().any(|word| word.contains("BATERI_DOCK"))),
            "{name}: the tier came from nowhere"
        );
    }
}

#[test]
#[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
fn only_a_plain_terminal_number_is_a_target() {
    for (name, shell) in shells() {
        let server = Server::new(&format!("targets-{name}"));
        server.mac_says(PLACED);
        // Labels, bare numbers, a leading zero, a made-up id, and the `t7` the numbers were called
        // before panes: none reaches kablo, all are 64.
        for target in ["build", "7", "p07", "P7", "p", "b-0123", "p7x", "t7", "t07"] {
            for args in [
                vec!["split", target],
                vec!["close", "pane", target],
                vec!["wait", target],
                vec!["capture", target],
                vec!["send", target, "text"],
                vec!["send-key", target, "enter"],
            ] {
                // `close pane t7` names a tab: it says which command that is.
                if args[0] == "close" && target == "t7" {
                    continue;
                }
                let said = server.run(&shell, Some("p4"), &args);
                assert_eq!(said.status, 64, "{args:?}: {}", said.err);
                assert!(said.err.contains("is not a pane"), "{args:?}: {}", said.err);
                assert!(said.err.contains("p1  zsh"), "ls is shown: {}", said.err);
            }
        }
        let said = server.run(&shell, Some("p4"), &["close", "pane", "t7"]);
        assert_eq!(said.status, 64);
        assert!(
            said.err
                .contains("t7 is a tab, not a pane. Did you mean: bateri close tab t7"),
            "{}",
            said.err
        );
        assert!(server.calls_of("new").is_empty());
        assert!(server.calls_of("end").is_empty());
        assert!(server.calls_of("wait").is_empty());
        assert!(server.calls_of("send").is_empty());
        // The words that are not targets pass: the text of a one-word `send`, a key.
        assert_eq!(server.run(&shell, Some("p4"), &["send", "build"]).status, 0);
        assert_eq!(
            server
                .run(&shell, Some("p4"), &["send-key", "enter"])
                .status,
            0
        );
        assert_eq!(
            server
                .run(&shell, Some("p4"), &["send-key", "Ctrl+C", "enter"])
                .status,
            0
        );
    }
}

#[test]
#[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
fn the_commands_that_are_kablos_own_pass_on_with_their_words_and_their_status() {
    for (name, shell) in shells() {
        let server = Server::new(&format!("passing-{name}"));
        let run = |args: &[&str]| server.run(&shell, Some("p4"), args);
        assert_eq!(run(&["send", "p3", "cargo test"]).status, 0);
        assert_eq!(run(&["send-key", "p3", "ctrl+c", "enter"]).status, 0);
        assert_eq!(
            run(&["capture", "p3", "--history", "200", "--trim"]).status,
            0
        );
        assert_eq!(run(&["capture", "--trim"]).status, 0);
        std::fs::write(server.stub.join("exit.wait"), "124").expect("a status");
        let said = run(&["wait", "--for", "10m", "p3"]);
        assert_eq!(
            said.status, 124,
            "the program's own status, not the script's"
        );
        std::fs::write(server.stub.join("exit.wait"), "3").expect("a status");
        assert_eq!(run(&["wait", "p3"]).status, 3);
        let kablo: Vec<Vec<String>> = server
            .calls()
            .into_iter()
            .filter(|call| call[0] != "here")
            .collect();
        assert_eq!(
            kablo,
            [
                vec!["send", "p3", "cargo test"],
                vec!["send-key", "p3", "ctrl+c", "enter"],
                vec!["capture", "p3", "--history", "200", "--trim"],
                vec!["capture", "--trim"],
                vec!["wait", "--for", "10m", "p3"],
                vec!["wait", "p3"],
            ]
        );
        // A `wait` with no terminal is a mistake, not a wait for itself.
        assert_eq!(run(&["wait"]).status, 64);
    }
}

#[test]
#[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
fn a_mistake_in_the_command_is_64_before_anything_is_started() {
    for (name, shell) in shells() {
        let server = Server::new(&format!("mistakes-{name}"));
        server.mac_says(PLACED);
        let tries: [(&[&str], Option<&str>); 14] = [
            (&["split", "--ratio", "0"], Some("p4")),
            (&["split", "--ratio", "100"], Some("p4")),
            (&["split", "--ratio", "x"], Some("p4")),
            (&["split", "--ratio"], Some("p4")),
            (&["split", "--left", "--below"], Some("p4")),
            (&["tab", "--left"], Some("p4")),
            (&["tab", "--ratio", "30"], Some("p4")),
            (&["new", "--keep-open"], Some("p4")),
            (&["new", "p4"], Some("p4")),
            (&["split", "p4", "p5"], Some("p4")),
            (&["split", "--bogus"], Some("p4")),
            (&["notify"], Some("p4")),
            (&["frobnicate"], Some("p4")),
            (&[], Some("p4")),
        ];
        for (args, here) in tries {
            let said = server.run(&shell, here, args);
            assert_eq!(said.status, 64, "{args:?} from {here:?}: {}", said.err);
        }
        assert!(server.calls_of("new").is_empty());
        // `new` is no command: the mistake is the usage, which does not offer it.
        let said = server.run(&shell, None, &["new", "--", "true"]);
        assert_eq!(said.status, 64, "{}", said.err);
        assert!(said.err.starts_with("usage: bateri split"), "{}", said.err);
        assert!(!said.err.contains("bateri new"), "{}", said.err);
        assert!(server.calls_of("new").is_empty());
        // From outside every terminal a `split` needs no anchor and the number is still given.
        let said = server.run(&shell, None, &["split", "--", "true"]);
        assert_eq!(said.status, 0, "{}", said.err);
        assert_eq!(said.out, "p1\n");
        assert_eq!(
            std::fs::read_to_string(server.state().join("terminals/p1")).expect("a record"),
            "opener -\nkeep-open no\n"
        );
        assert!(
            server.told().is_empty(),
            "no pane to place, so the Mac is told nothing"
        );
    }
}

#[test]
#[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
fn a_number_is_never_given_twice_and_one_a_hand_made_terminal_holds_is_skipped() {
    for (name, shell) in shells() {
        let server = Server::new(&format!("numbers-{name}"));
        assert_eq!(
            server.run(&shell, None, &["split", "--", "true"]).out,
            "p1\n"
        );
        assert_eq!(
            server.run(&shell, None, &["split", "--", "true"]).out,
            "p2\n"
        );
        // p1 is closed and gone; the next is not p1.
        let _ = std::fs::remove_file(server.state().join("terminals/p1"));
        assert_eq!(
            server.run(&shell, None, &["split", "--", "true"]).out,
            "p3\n"
        );
        // A session somebody made with `kablo new --id p4`: kablo refuses the id, the next is tried.
        std::fs::write(server.stub.join("taken.p4"), "").expect("taken");
        std::fs::write(server.stub.join("taken.p5"), "").expect("taken");
        let said = server.run(&shell, None, &["split", "--", "true"]);
        assert_eq!(said.out, "p6\n", "{}", said.err);
        assert_eq!(
            std::fs::read_to_string(server.state().join("next")).expect("the counter"),
            "7\n"
        );
    }
}

/// The server as the Mac reaches it: `sh -c` in a shell that has the home and nothing else.
struct MacChannel(PathBuf);

impl crate::kablo::install::Channel for MacChannel {
    fn run(&self, script: &str, input: &[u8]) -> std::io::Result<crate::kablo::install::Reply> {
        crate::kablo::install::run_command(
            &[
                "env".into(),
                "-i".into(),
                format!("HOME={}", self.0.display()),
                format!("PATH={}", std::env::var("PATH").unwrap_or_default()),
                "sh".into(),
                "-c".into(),
                script.into(),
            ],
            input,
            std::time::Duration::from_secs(20),
        )
    }
}

/// The terminal the Mac opens itself and the ones an agent opens are numbered by one counter: the
/// Mac runs the very bytes the command runs, over its connection, on the same file.
#[test]
#[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
fn the_mac_and_the_agents_command_never_give_the_same_number() {
    for (name, shell) in shells() {
        let server = Server::new(&format!("mac-counter-{name}"));
        let mac = MacChannel(server.home.clone());
        let mut seen = Vec::new();
        for _ in 0..3 {
            let said = server.run(&shell, None, &["split", "--", "true"]);
            assert_eq!(said.status, 0, "{}", said.err);
            seen.push(said.out.trim().to_owned());
            seen.push(format!(
                "p{}",
                crate::kablo::install::next_number(&mac).expect("a number")
            ));
        }
        assert_eq!(seen, ["p1", "p2", "p3", "p4", "p5", "p6"], "{name}");
        // The Mac's numbers are no terminal the script knows of: it never recorded them, and the
        // server's counter does not forget them.
        assert!(!server.state().join("terminals/p2").exists());
    }
}

#[test]
#[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
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

/// The counter's lock names its holder: one that is gone loses it at once, one that is alive
/// keeps it however long it takes.
#[test]
#[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
fn a_lock_held_by_a_live_process_is_waited_for_and_one_held_by_a_dead_one_is_not() {
    let server = Server::new("counter-holder");
    let counter = server.shell_dir().join("next.sh");
    let lock = server.state().join("next.lock");
    let run = || {
        Command::new("/bin/sh")
            .arg(&counter)
            .env_clear()
            .env("HOME", &server.home)
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .stdout(Stdio::piped())
            .spawn()
            .expect("the counter")
    };
    // A process that has ended: its number is nobody's now.
    let mut gone = Command::new("true").spawn().expect("true");
    let dead = gone.id();
    gone.wait().expect("ended");
    std::fs::create_dir(&lock).expect("a lock");
    std::fs::write(lock.join("pid"), format!("{dead}\n")).expect("its holder");
    let started = std::time::Instant::now();
    let out = run().wait_with_output().expect("the counter's end");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "1\n");
    assert!(
        started.elapsed() < std::time::Duration::from_secs(4),
        "taken over at once, not after the wait for a holder that wrote nothing"
    );
    assert!(!lock.exists());

    // This process is alive: a lock it holds is waited for past the five seconds a lock that
    // names nobody gets.
    std::fs::create_dir(&lock).expect("a lock");
    std::fs::write(lock.join("pid"), format!("{}\n", std::process::id())).expect("its holder");
    let mut waiting = run();
    std::thread::sleep(std::time::Duration::from_secs(7));
    assert!(
        waiting.try_wait().expect("a state").is_none(),
        "a live holder's lock is not taken"
    );
    std::fs::remove_dir_all(&lock).expect("the holder lets go");
    let out = waiting.wait_with_output().expect("the counter's end");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "2\n");
}

#[test]
#[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
fn a_terminal_without_a_program_runs_the_bootstrap_from_its_file() {
    for (name, shell) in shells() {
        let server = Server::new(&format!("shell-{name}"));
        server.mac_says(PLACED);
        let said = server.run(&shell, Some("p4"), &["split"]);
        assert_eq!(said.status, 0, "{}", said.err);
        let boot = server.shell_dir().join("boot.sh");
        // The shell is the integrated one, started the way the one-liner starts it: no parent
        // block, no nonce, no tab.
        let program = ["sh", &boot.display().to_string(), "", "", VERSION, "-"];
        assert_eq!(server.calls_of("new"), [new_call(&server, "p1", &program)]);

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
#[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
fn close_goes_to_the_mac_and_without_one_ends_any_terminal_of_the_machine() {
    for (name, shell) in shells() {
        let server = Server::new(&format!("close-{name}"));
        server.mac_says(PLACED);
        assert_eq!(server.run(&shell, Some("p4"), &["split"]).out, "p1\n");
        server.no_mac();
        assert_eq!(server.run(&shell, Some("p9"), &["split"]).out, "p2\n");
        assert!(server.note("p2").is_some());
        server.forget_calls();

        // No Mac: kablo ends it, nobody is asked, the records go.
        let said = server.run(&shell, Some("p4"), &["close", "pane", "p1"]);
        assert_eq!(said.status, 0, "{}", said.err);
        assert_eq!(server.calls_of("end"), [["end", "p1"]]);
        assert!(!server.state().join("terminals/p1").exists());
        // The Mac was asked first, and nobody was there.
        assert_eq!(server.told(), ["bateri 2\nclose\nterminal p1\n"]);
        // With the words kablo takes. Named nothing, it is the caller's own terminal.
        server.forget_calls();
        let said = server.run(&shell, Some("p9"), &["close", "pane", "--force"]);
        assert_eq!(said.status, 0, "{}", said.err);
        assert_eq!(server.calls_of("end"), [["end", "p9", "--force"]]);
        server.forget_calls();
        let said = server.run(&shell, Some("p9"), &["close", "pane", "p2", "--force"]);
        assert_eq!(said.status, 0);
        assert_eq!(server.calls_of("end"), [["end", "p2", "--force"]]);
        assert!(
            server.note("p2").is_none(),
            "a note of a closed terminal goes with it"
        );

        // Somebody else's, with no Mac: it is this machine's own, at every level that has the
        // command, so kablo ends it. Who opened it is not asked.
        server.forget_calls();
        assert_eq!(server.run(&shell, Some("p4"), &["split"]).out, "p3\n");
        let said = server.run(&shell, Some("p9"), &["close", "pane", "p3"]);
        assert_eq!(said.status, 0, "{}", said.err);
        assert_eq!(said.err, "");
        assert_eq!(server.calls_of("end"), [["end", "p3"]]);
        assert!(!server.state().join("terminals/p3").exists());
        assert_eq!(server.run(&shell, Some("p4"), &["split"]).out, "p4\n");

        // With a Mac the question is its, whoever opened it: it closes the pane and the terminal,
        // and the script ends nothing itself unless it was told to kill.
        server.forget_calls();
        server.mac_says("bateri 2\nok\n");
        assert_eq!(
            server
                .run(&shell, Some("p9"), &["close", "pane", "p3"])
                .status,
            0
        );
        assert_eq!(server.told(), ["bateri 2\nclose\nterminal p3\n"]);
        assert_eq!(
            Request::parse(&server.told()[0]),
            Ok(Request::Close(Anchor::of(Terminal(3))))
        );
        assert!(server.calls_of("end").is_empty());
        assert!(
            !server.state().join("terminals/p3").exists(),
            "the record goes with the terminal"
        );
        let said = server.run(&shell, Some("p9"), &["close", "pane", "p3", "--force"]);
        assert_eq!(said.status, 0);
        assert_eq!(server.calls_of("end"), [["end", "p3", "--force"]]);
        // The Mac refuses by the level it has for this machine, and says which and how to change
        // it; it does not know the pane; it could not tell.
        server.forget_calls();
        server.mac_says(
            "bateri 2\nerror refused this holds a pane of another machine, which \"own\" does not change on rasp (level \"own\"); set `commands` in the [remote] hosts entry for rasp to \"screen\" in settings.toml to allow it\n",
        );
        let said = server.run(&shell, Some("p9"), &["close", "pane", "p3"]);
        assert_eq!(said.status, 77);
        assert!(said.err.contains("\"screen\""), "{}", said.err);
        assert!(server.calls_of("end").is_empty());
        server.mac_says("bateri 2\nerror unknown p3 is no pane on this server\n");
        let said = server.run(&shell, Some("p9"), &["close", "pane", "p3"]);
        assert_eq!(
            (said.status, said.err.as_str()),
            (1, "bateri: p3 is no pane on this server\n")
        );
        assert!(server.calls_of("end").is_empty());
        // The Mac could not tell: the terminal is this machine's own all the same.
        server.mac_says("bateri 2\nerror failed the server did not answer\n");
        let said = server.run(&shell, Some("p9"), &["close", "pane", "p3"]);
        assert_eq!(said.status, 0, "{}", said.err);
        assert_eq!(server.calls_of("end"), [["end", "p3"]]);
        server.forget_calls();
        let said = server.run(&shell, Some("p9"), &["close", "pane"]);
        assert_eq!(said.status, 0, "{}", said.err);
        assert_eq!(server.calls_of("end"), [["end", "p9"]]);

        // Outside every terminal a named pane is closed too; named nothing there is none to close.
        server.no_mac();
        server.forget_calls();
        assert_eq!(server.run(&shell, None, &["close", "pane", "p3"]).status, 0);
        assert_eq!(server.calls_of("end"), [["end", "p3"]]);
        assert_eq!(server.run(&shell, None, &["close", "pane"]).status, 64);
    }
}

#[test]
#[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
fn a_terminal_named_nothing_is_the_callers_own_whatever_it_is_called() {
    let made = format!("b-{}", "0123456789abcdef".repeat(2));
    for (name, shell) in shells() {
        let server = Server::new(&format!("close-own-{name}"));
        // No Mac: kablo ends it by the name it has.
        let said = server.run(&shell, Some(&made), &["close", "pane"]);
        assert_eq!(said.status, 0, "{}", said.err);
        assert_eq!(server.calls_of("end"), [["end".to_owned(), made.clone()]]);
        // With one the Mac is told which terminal, so that it closes the pane that shows it.
        server.forget_calls();
        server.mac_says("bateri 2\nok\n");
        let said = server.run(&shell, Some(&made), &["close", "pane"]);
        assert_eq!(said.status, 0, "{}", said.err);
        assert_eq!(
            Request::parse(&server.told()[0]),
            Ok(Request::Close(Anchor::parse(&made).expect("a name")))
        );
        assert!(server.calls_of("end").is_empty());
        // The rule that a target is a number is for what is written: this one is not a number.
        server.forget_calls();
        let said = server.run(&shell, Some("p4"), &["close", "pane", &made]);
        assert_eq!(said.status, 64);
        assert!(server.told().is_empty());
    }
}

#[test]
#[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
fn ls_prints_what_the_mac_says_and_without_one_what_the_server_has() {
    for (name, shell) in shells() {
        let server = Server::new(&format!("ls-{name}"));
        // The Mac's text, byte for byte but for the bytes a terminal would act on.
        server.mac_says("bateri 2\nok text\nWindow 1\n  Tab 1.1\n\u{1b}[31mred\u{7}\n\n");
        let said = server.run(&shell, Some("p4"), &["ls"]);
        assert_eq!(said.status, 0);
        assert_eq!(said.out, "Window 1\n  Tab 1.1\n[31mred\n\n");
        assert_eq!(server.told(), ["bateri 2\nls\n"]);
        server.forget_calls();
        server.mac_says("bateri 2\nok text\n[]\n");
        let said = server.run(&shell, Some("p4"), &["ls", "--json"]);
        assert_eq!(said.out, "[]\n");
        assert_eq!(server.told(), ["bateri 2\nls\nformat json\n"]);

        // No Mac: kablo's own list and what waits to be placed.
        server.no_mac();
        server.run(&shell, Some("p4"), &["split", "--below", "--ratio", "30"]);
        let said = server.run(&shell, Some("p4"), &["ls"]);
        assert_eq!(said.status, 0);
        assert_eq!(
            said.out,
            "ID  PROGRAM\np1  zsh\n\nWaiting for a Mac to place:\n  p1  below p4 (30%)\n"
        );
        assert!(said.err.starts_with("bateri: no Mac is connected"));
        let said = server.run(&shell, Some("p4"), &["ls", "--json"]);
        assert_eq!(said.out, "[]\n");
        // The Mac's mistakes are the status, as for any request...
        server.mac_says("bateri 2\nerror usage no such format\n");
        assert_eq!(server.run(&shell, Some("p4"), &["ls"]).status, 64);
        // ...but one that is only that it could not tell now leaves the server's own list.
        server.mac_says("bateri 2\nerror failed the server did not give its terminals in time\n");
        let said = server.run(&shell, Some("p4"), &["ls"]);
        assert_eq!(said.status, 0);
        assert!(
            said.out.starts_with("ID  PROGRAM\np1  zsh\n"),
            "{}",
            said.out
        );
        assert!(
            said.err
                .starts_with("bateri: the Mac could not tell what is on screen:"),
            "{}",
            said.err
        );
    }
}

#[test]
#[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
fn notify_needs_a_mac_and_sends_one_line() {
    for (name, shell) in shells() {
        let server = Server::new(&format!("notify-{name}"));
        let said = server.run(&shell, Some("p4"), &["notify", "the build is done"]);
        assert_eq!(said.status, 69);
        assert_eq!(said.err, "bateri: no Mac is connected to show it\n");
        server.forget_calls();
        server.mac_says("bateri 2\nok\n");
        let said = server.run(
            &shell,
            Some("p4"),
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
            server.run(&shell, Some("p4"), &["notify", &long]).status,
            64
        );
    }
}

#[test]
#[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
fn the_layout_verbs_are_the_macs_alone_and_say_what_they_asked() {
    use crate::agent::message::{Arrange, Subject};
    use crate::ids::{Id, Kind as IdKind};
    let pane = |number: u64| Anchor::of(Terminal(number));
    let tab = |number: u64| Id::new(IdKind::Tab, number);
    for (name, shell) in shells() {
        let server = Server::new(&format!("layout-{name}"));
        // With no Mac they are 69, and the words say what is missing.
        for args in [
            &["focus", "tab", "t3"][..],
            &["zoom", "p9"],
            &["swap", "p9", "p3"],
            &["move", "pane", "p9", "--beside", "p3", "--right"],
            &["resize", "p9", "--left"],
        ] {
            let said = server.run(&shell, Some("p4"), args);
            assert_eq!(said.status, 69, "{args:?}: {}", said.err);
            assert_eq!(said.err, "bateri: no Mac is connected to do it\n");
        }
        server.forget_calls();
        server.mac_says("bateri 2\nok\n");
        let tries: Vec<(Vec<&str>, Arrange)> = vec![
            (
                vec!["focus", "tab", "t3"],
                Arrange::Focus(Subject::Tab(tab(3))),
            ),
            (vec!["focus", "tab"], Arrange::Focus(Subject::Tab(None))),
            (
                vec!["focus", "window", "w2"],
                Arrange::Focus(Subject::Window(Id::new(IdKind::Window, 2))),
            ),
            (vec!["focus", "group"], Arrange::Focus(Subject::Group(None))),
            (
                vec!["focus", "pane", "p9"],
                Arrange::Focus(Subject::Pane(Some(pane(9)))),
            ),
            (
                vec!["focus", "pane"],
                Arrange::Focus(Subject::Pane(Some(pane(4)))),
            ),
            (vec!["zoom", "p9"], Arrange::Zoom(Some(pane(9)))),
            (vec!["zoom"], Arrange::Zoom(None)),
            (
                vec!["swap", "p9", "p3"],
                Arrange::Swap {
                    pane: pane(9),
                    other: pane(3),
                },
            ),
            (
                vec!["move", "pane", "p9", "--beside", "p3", "--right"],
                Arrange::Move {
                    pane: pane(9),
                    beside: pane(3),
                    side: Side::Right,
                },
            ),
            (
                vec!["resize", "p9", "--left", "--by", "3"],
                Arrange::Resize {
                    pane: Some(pane(9)),
                    side: Side::Left,
                    steps: 3,
                },
            ),
            (
                vec!["resize", "--below"],
                Arrange::Resize {
                    pane: None,
                    side: Side::Below,
                    steps: 1,
                },
            ),
        ];
        for (args, arrange) in &tries {
            let said = server.run(&shell, Some("p4"), args);
            assert_eq!(said.status, 0, "{args:?}: {}", said.err);
            assert_eq!(said.out, "");
            let told = server.told();
            assert_eq!(
                Request::parse(told.last().expect("told")),
                Ok(Request::Arrange(arrange.clone())),
                "{args:?}"
            );
        }
        // Run in no terminal the caller's own is nothing; a named one is still fine.
        let said = server.run(&shell, None, &["focus", "tab"]);
        assert_eq!(said.status, 64, "{}", said.err);
        assert!(said.err.contains("run in no terminal"), "{}", said.err);
        let said = server.run(&shell, None, &["focus", "tab", "t3"]);
        assert_eq!(said.status, 0, "{}", said.err);
        // The Mac refuses what would take the keyboard: 77, and its words.
        server.mac_says("bateri 2\nerror refused the keyboard is in another pane\n");
        let said = server.run(&shell, Some("p4"), &["focus", "pane", "p9"]);
        assert_eq!(said.status, 77, "{}", said.err);
        assert_eq!(said.err, "bateri: the keyboard is in another pane\n");
        server.mac_says("bateri 2\nerror unknown t9 is no tab\n");
        let said = server.run(&shell, Some("p4"), &["focus", "tab", "t9"]);
        assert_eq!(said.status, 1, "{}", said.err);
    }
}

#[test]
#[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
fn a_word_and_an_id_of_another_kind_is_64_and_says_what_to_write() {
    for (name, shell) in shells() {
        let server = Server::new(&format!("kinds-{name}"));
        server.mac_says("bateri 2\nok\n");
        let said = server.run(&shell, Some("p4"), &["focus", "group", "t3"]);
        assert_eq!(said.status, 64, "{}", said.err);
        assert_eq!(
            said.err,
            "bateri: t3 is a tab, not a group. Did you mean: bateri focus tab t3\n"
        );
        let said = server.run(&shell, Some("p4"), &["focus", "tab", "w1"]);
        assert!(
            said.err.contains("w1 is a window, not a tab"),
            "{}",
            said.err
        );
        let said = server.run(&shell, Some("p4"), &["zoom", "t3"]);
        assert_eq!(said.status, 64, "{}", said.err);
        assert_eq!(
            said.err,
            "bateri: t3 is a tab, not a pane: zoom takes a pane\n"
        );
        for args in [
            &["focus"][..],
            &["focus", "frame", "t3"],
            &["focus", "tab", "t3", "t4"],
            &["focus", "tab", "t03"],
            &["focus", "tab", "web"],
            &["focus", "tab", "--json"],
            &["zoom", "p4", "p5"],
            &["swap", "p4"],
            &["swap", "p4", "p4"],
            &["swap", "p4", "t3"],
            &["move"],
            &["move", "tab", "t3", "--beside", "p3", "--right"],
            &["move", "pane", "p9", "--right"],
            &["move", "pane", "p9", "--beside", "p3"],
            &["move", "pane", "p9", "--beside", "p9", "--right"],
            &["move", "pane", "p9", "--beside", "p3", "--right", "--left"],
            &["move", "pane", "p9", "p8", "--beside", "p3", "--right"],
            &["resize", "p9"],
            &["resize", "p9", "--left", "--right"],
            &["resize", "p9", "--left", "--by", "0"],
            &["resize", "p9", "--left", "--by", "51"],
            &["resize", "p9", "--left", "--by", "x"],
            &["resize", "p9", "p8", "--left"],
            &["resize", "--bogus", "--left"],
        ] {
            let said = server.run(&shell, Some("p4"), args);
            assert_eq!(said.status, 64, "{args:?}: {}", said.err);
        }
        assert!(server.told().is_empty(), "nothing reached the Mac");
    }
}

#[test]
#[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
fn a_pane_that_did_not_take_the_keyboard_says_so_on_one_line() {
    let server = Server::new("keyboard-stayed");
    let shell = Path::new("/bin/sh");
    server.mac_says(&format!("{PLACED}keyboard stayed\n"));
    let said = server.run(shell, Some("p4"), &["split", "--focus"]);
    assert_eq!(said.status, 0, "{}", said.err);
    assert_eq!(said.out, "p1\n");
    assert_eq!(
        said.err,
        "bateri: p1 did not take the keyboard: the cursor is in another pane of that window; bateri notify asks the user to look\n"
    );
    // The keyboard went where it was asked: no word.
    server.mac_says(PLACED);
    let said = server.run(shell, Some("p4"), &["split", "--focus"]);
    assert_eq!(said.err, "");
}

#[test]
#[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
fn a_split_with_no_pane_to_go_beside_prints_the_number_and_waits_with_the_programs_status() {
    for (name, shell) in shells() {
        let server = Server::new(&format!("orphan-{name}"));
        let said = server.run(
            &shell,
            None,
            &["split", "--json", "--cwd", "~", "--", "make"],
        );
        assert_eq!(said.status, 0, "{}", said.err);
        assert_eq!(
            said.out,
            "{\"pane\":\"p1\",\"tab\":null,\"group\":null,\"window\":null,\"landed\":\"waiting\",\"next_to\":null,\"squeezed\":false,\"hidden\":false}\n"
        );
        assert_eq!(server.calls_of("new"), [new_call(&server, "p1", &["make"])]);
        std::fs::write(server.stub.join("exit.wait"), "7").expect("a status");
        let said = server.run(&shell, None, &["split", "--wait", "--", "make"]);
        assert_eq!((said.status, said.out.as_str()), (7, "p2\n"));
        assert_eq!(server.calls_of("wait"), [["wait", "p2"]]);
        // A directory that is not there is said, and nothing is started.
        let said = server.run(&shell, None, &["split", "--cwd", "/nowhere/at/all"]);
        assert_eq!(said.status, 1);
        assert_eq!(server.calls_of("new").len(), 2);
    }
}

#[test]
#[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
fn close_names_its_kind_and_a_tab_a_group_or_a_window_is_the_macs_alone() {
    use crate::agent::message::{Arrange, Subject};
    use crate::ids::{Id, Kind as IdKind};
    for (name, shell) in shells() {
        let server = Server::new(&format!("close-kinds-{name}"));
        // No kind, or an id where the kind goes: 64, and what to write. Nothing is asked of kablo
        // or of the Mac.
        for (args, hint) in [
            (&["close"][..], "Did you mean: bateri close pane"),
            (&["close", "p9"], "Did you mean: bateri close pane p9"),
            (&["close", "t3"], "Did you mean: bateri close tab t3"),
            (&["close", "g1"], "Did you mean: bateri close group g1"),
            (&["close", "w2"], "Did you mean: bateri close window w2"),
            (
                &["close", "--force"],
                "close takes pane, tab, group or window",
            ),
            (
                &["close", "frame", "t3"],
                "close takes pane, tab, group or window",
            ),
            (&["close", "tab", "g1"], "g1 is a group, not a tab"),
            (&["close", "tab", "t3", "t4"], "close tab names one id"),
            (&["close", "tab", "t03"], "is not a tab"),
            (&["close", "tab", "--force"], "close tab takes no option"),
            (&["close", "window", "web"], "is not a window"),
            (
                &["close", "pane", "t3"],
                "Did you mean: bateri close tab t3",
            ),
        ] {
            let said = server.run(&shell, Some("p4"), args);
            assert_eq!(said.status, 64, "{args:?}: {}", said.err);
            assert!(said.err.contains(hint), "{args:?}: {}", said.err);
        }
        assert!(server.calls_of("end").is_empty());
        assert!(server.told().is_empty());

        // No Mac: a tab, a group and a window belong to its windows, so they are 69.
        for args in [
            &["close", "tab", "t3"][..],
            &["close", "group", "g1"],
            &["close", "window", "w1"],
            &["close", "tab"],
        ] {
            let said = server.run(&shell, Some("p4"), args);
            assert_eq!(said.status, 69, "{args:?}: {}", said.err);
        }
        assert!(
            server.calls_of("end").is_empty(),
            "kablo ends no terminal for a layer"
        );

        // With one the Mac is told which layer, or none: the caller's own.
        server.forget_calls();
        server.mac_says("bateri 2\nok\n");
        let tab = Id::new(IdKind::Tab, 3);
        let tries: Vec<(Vec<&str>, Arrange)> = vec![
            (
                vec!["close", "tab", "t3"],
                Arrange::Close(Subject::Tab(tab)),
            ),
            (vec!["close", "tab"], Arrange::Close(Subject::Tab(None))),
            (
                vec!["close", "group", "g1"],
                Arrange::Close(Subject::Group(Id::new(IdKind::Group, 1))),
            ),
            (
                vec!["close", "window"],
                Arrange::Close(Subject::Window(None)),
            ),
        ];
        for (args, arrange) in &tries {
            server.forget_calls();
            let said = server.run(&shell, Some("p4"), args);
            assert_eq!(said.status, 0, "{args:?}: {}", said.err);
            assert_eq!(said.out, "");
            let told = server.told();
            assert_eq!(told.len(), 1, "{args:?}");
            assert_eq!(
                Request::parse(&told[0]),
                Ok(Request::Arrange(arrange.clone())),
                "{args:?}"
            );
            if args == &["close", "tab", "t3"] {
                assert_eq!(
                    told[0], "bateri 2\nclose\nsubject tab\nid t3\n",
                    "the words are frozen"
                );
            }
        }
        // Run in no terminal the caller's own is nothing.
        let said = server.run(&shell, None, &["close", "tab"]);
        assert_eq!(said.status, 64, "{}", said.err);
        // The level for this machine refuses (77); a tab that is not there is 1.
        server.mac_says("bateri 2\nerror refused this holds a pane of another machine\n");
        let said = server.run(&shell, Some("p4"), &["close", "tab", "t3"]);
        assert_eq!(said.status, 77, "{}", said.err);
        server.mac_says("bateri 2\nerror unknown there is no tab t9\n");
        let said = server.run(&shell, Some("p4"), &["close", "tab", "t9"]);
        assert_eq!(
            (said.status, said.err.as_str()),
            (1, "bateri: there is no tab t9\n")
        );
    }
}

#[test]
#[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
fn rename_gives_a_tab_or_a_group_a_label_through_the_mac_and_a_pane_one_through_kablo() {
    use crate::agent::message::{Arrange, Subject};
    use crate::ids::{Id, Kind as IdKind};
    for (name, shell) in shells() {
        let server = Server::new(&format!("rename-{name}"));
        // A pane's label is kablo's: the Mac is not asked, an empty one clears.
        let said = server.run(&shell, Some("p4"), &["rename", "pane", "p9", "server"]);
        assert_eq!(said.status, 0, "{}", said.err);
        assert_eq!(server.calls_of("label"), [["label", "p9", "server"]]);
        assert!(server.told().is_empty());
        server.forget_calls();
        let said = server.run(&shell, Some("p4"), &["rename", "pane", "p9", ""]);
        assert_eq!(said.status, 0, "{}", said.err);
        assert_eq!(server.calls_of("label"), [["label", "p9", ""]]);
        // Words after the id are the label, as `notify`'s are.
        server.forget_calls();
        server.run(
            &shell,
            Some("p4"),
            &["rename", "pane", "p9", "unit", "tests"],
        );
        assert_eq!(server.calls_of("label"), [["label", "p9", "unit tests"]]);

        // What is wrong is 64 before anything is asked.
        server.forget_calls();
        let long = "x".repeat(65);
        for (args, hint) in [
            (&["rename"][..], "rename names what gets a label"),
            (&["rename", "window", "w1", "x"], "a window has no label"),
            (
                &["rename", "frame", "t3", "x"],
                "rename takes pane, tab or group",
            ),
            (&["rename", "tab"], "rename names the tab to label"),
            (&["rename", "tab", "web"], "is not a tab"),
            (&["rename", "tab", "t3"], "rename needs a label"),
            (
                &["rename", "tab", "g1", "web"],
                "Did you mean: bateri rename group g1",
            ),
            (&["rename", "group", "g1", ""], "a group always has a label"),
            (&["rename", "tab", "t3", &long], "1 to 64 bytes"),
            (
                &["rename", "tab", "t3", "a\nwindow w1"],
                "one line of plain text",
            ),
            (&["rename", "tab", "t3", "   "], "only spaces"),
            (
                &["rename", "pane", "t3", "x"],
                "Did you mean: bateri rename tab t3",
            ),
        ] {
            let said = server.run(&shell, Some("p4"), args);
            assert_eq!(said.status, 64, "{args:?}: {}", said.err);
            assert!(said.err.contains(hint), "{args:?}: {}", said.err);
        }
        assert!(server.calls_of("label").is_empty());
        assert!(server.told().is_empty());

        // A tab's and a group's are the Mac's: 69 with none.
        let said = server.run(&shell, Some("p4"), &["rename", "tab", "t3", "web"]);
        assert_eq!(said.status, 69, "{}", said.err);
        server.mac_says("bateri 2\nok\n");
        let tries: Vec<(Vec<&str>, Arrange)> = vec![
            (
                vec!["rename", "tab", "t3", "web"],
                Arrange::Rename {
                    subject: Subject::Tab(Id::new(IdKind::Tab, 3)),
                    label: Some("web".to_owned()),
                },
            ),
            (
                vec!["rename", "tab", "t3", ""],
                Arrange::Rename {
                    subject: Subject::Tab(Id::new(IdKind::Tab, 3)),
                    label: None,
                },
            ),
            (
                vec!["rename", "group", "g1", "api", "v2"],
                Arrange::Rename {
                    subject: Subject::Group(Id::new(IdKind::Group, 1)),
                    label: Some("api v2".to_owned()),
                },
            ),
        ];
        for (args, arrange) in &tries {
            server.forget_calls();
            let said = server.run(&shell, Some("p4"), args);
            assert_eq!(said.status, 0, "{args:?}: {}", said.err);
            let told = server.told();
            assert_eq!(told.len(), 1, "{args:?}");
            assert_eq!(
                Request::parse(&told[0]),
                Ok(Request::Arrange(arrange.clone())),
                "{args:?}"
            );
            if args == &["rename", "tab", "t3", "web"] {
                assert_eq!(told[0], "bateri 2\nrename\nsubject tab\nid t3\nlabel web\n");
            }
        }
    }
}

#[test]
#[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
fn move_tab_goes_into_a_group_or_a_window_and_nothing_else() {
    use crate::agent::message::{Arrange, TabHome};
    use crate::ids::{Id, Kind as IdKind};
    for (name, shell) in shells() {
        let server = Server::new(&format!("move-tab-{name}"));
        for (args, hint) in [
            (&["move"][..], "move names what moves"),
            (&["move", "group", "g1"], "move takes a pane or a tab"),
            (&["move", "tab"], "move names the tab to move"),
            (
                &["move", "tab", "--group", "g1"],
                "move names the tab to move",
            ),
            (&["move", "tab", "t3"], "move tab needs a place"),
            (
                &["move", "tab", "t3", "--group", "g1", "--window", "w1"],
                "a group or into a window, not both",
            ),
            (
                &["move", "tab", "t3", "--group", "t4"],
                "t4 is a tab, not a group",
            ),
            (
                &["move", "tab", "t3", "--window", "g1"],
                "g1 is a group, not a window",
            ),
            (
                &["move", "tab", "p9", "--group", "g1"],
                "Did you mean: bateri move pane p9",
            ),
            (
                &["move", "tab", "t3", "t4", "--group", "g1"],
                "move names one tab",
            ),
            (&["move", "tab", "t3", "--group"], "--group takes a group"),
            (
                &["move", "tab", "t3", "--right"],
                "move tab takes no option",
            ),
            (
                &["move", "tab", "t3", "--group", "g1", "--group", "g2"],
                "name one group",
            ),
        ] {
            let said = server.run(&shell, Some("p4"), args);
            assert_eq!(said.status, 64, "{args:?}: {}", said.err);
            assert!(said.err.contains(hint), "{args:?}: {}", said.err);
        }
        assert!(server.told().is_empty());
        let said = server.run(&shell, Some("p4"), &["move", "tab", "t3", "--group", "g1"]);
        assert_eq!(said.status, 69, "{}", said.err);

        server.mac_says("bateri 2\nok\n");
        let tab = Id::new(IdKind::Tab, 3).expect("an id");
        let tries: Vec<(Vec<&str>, Arrange)> = vec![
            (
                vec!["move", "tab", "t3", "--group", "g1"],
                Arrange::MoveTab {
                    tab,
                    to: TabHome::Group(Id::new(IdKind::Group, 1).expect("an id")),
                },
            ),
            (
                vec!["move", "tab", "t3", "--window=w2"],
                Arrange::MoveTab {
                    tab,
                    to: TabHome::Window(Id::new(IdKind::Window, 2).expect("an id")),
                },
            ),
        ];
        for (args, arrange) in &tries {
            server.forget_calls();
            let said = server.run(&shell, Some("p4"), args);
            assert_eq!(said.status, 0, "{args:?}: {}", said.err);
            let told = server.told();
            assert_eq!(told.len(), 1, "{args:?}");
            assert_eq!(
                Request::parse(&told[0]),
                Ok(Request::Arrange(arrange.clone())),
                "{args:?}"
            );
            if args.contains(&"--group") {
                assert_eq!(told[0], "bateri 2\nmove\nsubject tab\nid t3\ngroup g1\n");
            }
        }
    }
}

#[test]
#[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
fn a_pane_or_a_tab_opens_where_it_is_told_and_what_contradicts_is_64() {
    use crate::agent::message::{Home, WindowHome};
    use crate::ids::{Id, Kind as IdKind};
    let id = |kind: IdKind, number: u64| Id::new(kind, number);
    for (name, shell) in shells() {
        let server = Server::new(&format!("homes-{name}"));
        server.mac_says(PLACED);
        let mut tries: Vec<(Vec<&str>, Option<Side>, Home)> = vec![
            (
                vec!["split", "--tab", "t3"],
                None,
                Home {
                    tab: id(IdKind::Tab, 3),
                    ..Home::default()
                },
            ),
            (
                vec!["split", "--tab=t3", "--right"],
                Some(Side::Right),
                Home {
                    tab: id(IdKind::Tab, 3),
                    ..Home::default()
                },
            ),
            (
                vec!["tab", "--group", "g1"],
                Some(Side::Tab),
                Home {
                    group: id(IdKind::Group, 1),
                    ..Home::default()
                },
            ),
            (
                vec!["tab", "--window", "w2"],
                Some(Side::Tab),
                Home {
                    window: Some(WindowHome::Existing(id(IdKind::Window, 2).expect("an id"))),
                    ..Home::default()
                },
            ),
            (
                vec![
                    "tab",
                    "--new-group",
                    "api",
                    "--window",
                    "w2",
                    "--label",
                    "web",
                ],
                Some(Side::Tab),
                Home {
                    new_group: Some("api".to_owned()),
                    window: Some(WindowHome::Existing(id(IdKind::Window, 2).expect("an id"))),
                    label: Some("web".to_owned()),
                    ..Home::default()
                },
            ),
            (
                vec!["tab", "--new-group", "api v2"],
                Some(Side::Tab),
                Home {
                    new_group: Some("api v2".to_owned()),
                    ..Home::default()
                },
            ),
            (
                vec!["window"],
                Some(Side::Tab),
                Home {
                    window: Some(WindowHome::New),
                    ..Home::default()
                },
            ),
            (
                vec!["window", "--label", "logs", "--focus"],
                Some(Side::Tab),
                Home {
                    window: Some(WindowHome::New),
                    label: Some("logs".to_owned()),
                    ..Home::default()
                },
            ),
        ];
        for (n, (args, side, home)) in tries.drain(..).enumerate() {
            server.forget_calls();
            let said = server.run(&shell, Some("p4"), &args);
            assert_eq!(said.status, 0, "{args:?}: {}", said.err);
            assert_eq!(said.out, format!("p{}\n", n + 1), "{args:?}");
            let told = server.told();
            assert_eq!(told.len(), 1, "{args:?}");
            let Ok(Request::Place(placed)) = Request::parse(&told[0]) else {
                panic!("{args:?}: {:?}", told[0]);
            };
            assert_eq!(placed.side, side, "{args:?}");
            assert_eq!(placed.home, home, "{args:?}");
            assert_eq!(placed.anchor, Some(Anchor::of(Terminal(4))), "{args:?}");
            // The words are frozen.
            if n == 4 {
                assert_eq!(
                    told[0],
                    "bateri 2\nplace\nterminal p5\nanchor p4\nside tab\nnew-group api\nwindow w2\nlabel web\n"
                );
            }
            if n == 6 {
                assert_eq!(
                    told[0],
                    "bateri 2\nplace\nterminal p7\nanchor p4\nside tab\nwindow new\n"
                );
            }
        }

        // What contradicts, and what is not for the command, is 64 before a terminal starts.
        let started = server.calls_of("new").len();
        for (args, hint) in [
            (&["split", "p9", "--tab", "t3"][..], "a pane or into a tab"),
            (&["split", "--tab", "g1"], "g1 is a group, not a tab"),
            (&["split", "--tab"], "--tab takes a tab"),
            (&["split", "--tab", "t3", "--tab", "t4"], "name one tab"),
            (&["split", "--group", "g1"], "split takes no --group"),
            (&["split", "--window", "w1"], "split takes no --window"),
            (
                &["split", "--new-group", "api"],
                "split takes no --new-group",
            ),
            (&["tab", "--tab", "t3"], "tab takes no --tab"),
            (
                &["tab", "--group", "g1", "--window", "w1"],
                "it goes with no --window",
            ),
            (
                &["tab", "--group", "g1", "--new-group", "api"],
                "it goes with no --window",
            ),
            (&["tab", "--group", "t3"], "t3 is a tab, not a group"),
            (&["tab", "--window", "g1"], "g1 is a group, not a window"),
            (&["tab", "--new-group", ""], "--new-group is 1 to 64 bytes"),
            (&["tab", "--new-group", "   "], "only spaces"),
            (
                &["tab", "--label", "a\nwindow w1"],
                "one line of plain text",
            ),
            (&["tab", "--label", "x", "--label", "y"], "name one label"),
            (&["window", "--tab", "t3"], "window takes no --tab"),
            (&["window", "--group", "g1"], "window takes no --group"),
            (&["window", "--window", "w1"], "window takes no --window"),
            (&["window", "--left"], "window takes no side"),
            (&["window", "--ratio", "30"], "window takes no ratio"),
        ] {
            let said = server.run(&shell, Some("p4"), args);
            assert_eq!(said.status, 64, "{args:?}: {}", said.err);
            assert!(said.err.contains(hint), "{args:?}: {}", said.err);
        }
        assert_eq!(server.calls_of("new").len(), started);
    }
}

#[test]
#[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
fn a_label_is_kablos_for_a_pane_and_the_macs_for_a_tab() {
    for (name, shell) in shells() {
        let server = Server::new(&format!("labels-{name}"));
        server.mac_says(PLACED);
        // A pane's label rides on `kablo new` and is not in the message.
        let said = server.run(&shell, Some("p4"), &["split", "--label", "dev server"]);
        assert_eq!(said.status, 0, "{}", said.err);
        let call = &server.calls_of("new")[0];
        let at = call
            .iter()
            .position(|word| word == "--label")
            .expect("label");
        assert_eq!(call[at + 1], "dev server");
        assert_eq!(call[2], "p1", "the number is still given first");
        assert_eq!(server.told(), ["bateri 2\nplace\nterminal p1\nanchor p4\n"]);
        // A tab's is in the message and not in kablo's call.
        server.forget_calls();
        let said = server.run(&shell, Some("p4"), &["tab", "--label=web"]);
        assert_eq!(said.status, 0, "{}", said.err);
        assert!(
            !server.calls_of("new")[0]
                .iter()
                .any(|word| word == "--label")
        );
        assert_eq!(
            server.told(),
            ["bateri 2\nplace\nterminal p2\nanchor p4\nside tab\nlabel web\n"]
        );
        // Without a label the call has none.
        server.forget_calls();
        server.run(&shell, Some("p4"), &["split"]);
        assert!(
            !server.calls_of("new")[0]
                .iter()
                .any(|word| word == "--label")
        );
    }
}

#[test]
#[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
fn a_window_waits_for_a_mac_as_a_tab_does_and_a_place_the_mac_refuses_ends_the_terminal() {
    use crate::agent::message::{Home, WindowHome};
    for (name, shell) in shells() {
        let server = Server::new(&format!("window-waits-{name}"));
        // No Mac: the number is given, the message waits on the server with its fields, and the
        // command says so on one line.
        let said = server.run(&shell, Some("p4"), &["window", "--label", "logs", "--json"]);
        assert_eq!(said.status, 0, "{}", said.err);
        assert!(said.out.starts_with("{\"pane\":\"p1\""), "{}", said.out);
        assert!(said.out.contains("\"window\":null"), "{}", said.out);
        assert_eq!(
            said.err,
            "bateri: p1 will take its place when a Mac connects\n"
        );
        let note = server.note("p1").expect("a note");
        let Ok(Request::Place(waiting)) = Request::parse(&note) else {
            panic!("{note:?}");
        };
        assert_eq!(waiting.side, Some(Side::Tab));
        assert_eq!(
            waiting.home,
            Home {
                window: Some(WindowHome::New),
                label: Some("logs".to_owned()),
                ..Home::default()
            }
        );
        // The Mac that does not have the place the pane asked for refuses it: the terminal the
        // script started ends and no note is left, with the code the Mac's kind says.
        server.forget_calls();
        server.mac_says("bateri 2\nerror unknown there is no tab t9\n");
        let said = server.run(&shell, Some("p4"), &["split", "--tab", "t9"]);
        assert_eq!(
            (said.status, said.err.as_str()),
            (1, "bateri: there is no tab t9\n")
        );
        assert_eq!(server.calls_of("end"), [["end", "p2", "--force"]]);
        assert_eq!(server.note("p2"), None);
        server.mac_says("bateri 2\nerror refused this holds a pane of another machine\n");
        let said = server.run(&shell, Some("p4"), &["tab", "--group", "g1"]);
        assert_eq!(said.status, 77, "{}", said.err);
        assert_eq!(
            server.calls_of("end").last(),
            Some(&vec![
                "end".to_owned(),
                "p3".to_owned(),
                "--force".to_owned()
            ])
        );
    }
}

/// The Mac's answer to the question how to reach a server: the words of an `ssh` command.
fn ssh_words() -> String {
    Reply::Text("arg -T\narg -o\narg BatchMode=yes\narg prod\n".to_owned()).encode()
}

/// A Mac and a server `prod` it reaches, and the environment the Mac's script runs with.
fn mac_and_prod(name: &str) -> (Server, Server, Vec<(String, String)>) {
    let mac = Server::new_here(&format!("{name}-mac"));
    let prod = Server::new(&format!("{name}-prod"));
    let env = mac.ssh_to(&prod);
    (mac, prod, env)
}

fn borrowed(env: &[(String, String)]) -> Vec<(&str, &str)> {
    env.iter()
        .map(|(key, value)| (key.as_str(), value.as_str()))
        .collect()
}

/// What a `kablo new` call was asked: the name, the directory and the program (its `PATH` is the
/// login shell's of the machine, which is not what the test is about).
fn new_parts(call: &[String]) -> (String, String, Vec<String>) {
    let after = |flag: &str| {
        let at = call.iter().position(|word| word == flag).expect(flag);
        call[at + 1].clone()
    };
    let program = call
        .iter()
        .position(|word| word == "--")
        .expect("a program");
    (after("--id"), after("--cwd"), call[program + 1..].to_vec())
}

#[test]
#[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
fn a_split_on_a_server_asks_the_mac_how_to_reach_it_starts_the_terminal_there_and_asks_for_the_pane()
 {
    for (name, shell) in shells() {
        let (mac, prod, env) = mac_and_prod(&format!("on-{name}"));
        mac.mac_says_in_turn(1, &ssh_words());
        mac.mac_says_in_turn(2, PLACED);
        let said = mac.run_file_with(
            &shell,
            &mac.script(),
            Some("p4"),
            &["split", "--below", "-S", "prod", "--", "make", "it's", "ok"],
            &borrowed(&env),
        );
        assert_eq!(said.status, 0, "{}: {}", shell.display(), said.err);
        // The terminal is the server's `p1`, written by its machine.
        assert_eq!(said.out, "p1@prod\n", "{}", said.err);
        assert_eq!(said.err, "", "a pane that landed as asked is silent");

        // The Mac was asked how, then for the pane: a terminal of the server, beside this Mac's.
        let told = mac.told();
        assert_eq!(told.len(), 2, "{told:?}");
        assert_eq!(
            Request::parse(&told[0]),
            Ok(Request::Ssh("prod".to_owned())),
            "{told:?}"
        );
        let wanted = Place {
            side: Some(Side::Below),
            server: Some("prod".to_owned()),
            ..place(1, 4)
        };
        assert_eq!(told[1], encoded(wanted));

        // ssh got the Mac's words and one command line.
        let ssh = mac.ssh_calls();
        assert_eq!(ssh.len(), 1, "{ssh:?}");
        assert_eq!(ssh[0][..4], ["-T", "-o", "BatchMode=yes", "prod"]);
        assert_eq!(ssh[0].len(), 5, "{ssh:?}");

        // The server started the terminal with its own command, in no terminal, in its home, the
        // program's words as they were written; nothing was started or recorded on the Mac.
        let calls = prod.calls_of("new");
        assert_eq!(calls.len(), 1, "{calls:?}");
        assert_eq!(
            new_parts(&calls[0]),
            (
                "p1".to_owned(),
                prod.home.display().to_string(),
                vec!["make".to_owned(), "it's".to_owned(), "ok".to_owned()]
            )
        );
        assert_eq!(mac.calls_of("new"), Vec::<Vec<String>>::new());
        assert!(!mac.state().join("terminals/p1").exists());
        assert!(prod.state().join("terminals/p1").is_file());
        // The server was not asked to place anything: it ran in no terminal.
        assert!(prod.told().is_empty(), "{:?}", prod.told());
    }
}

#[test]
#[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
fn a_server_that_cannot_be_reached_is_said_so_before_or_after_anything_is_started() {
    for (name, shell) in shells() {
        let (mac, prod, env) = mac_and_prod(&format!("refused-{name}"));
        let env = borrowed(&env);
        let run = |args: &[&str]| mac.run_file_with(&shell, &mac.script(), Some("p4"), args, &env);

        // A name that is not a name is a mistake before the Mac is asked.
        for bad in ["-oProxyCommand=x", "a b", "a;b", "", "p9@"] {
            let said = run(&["split", "-S", bad]);
            assert_eq!(said.status, 64, "{bad:?}: {}", said.err);
        }
        let said = run(&["split", "-S"]);
        assert_eq!(said.status, 64, "{}", said.err);
        let said = run(&["split", "-S", "prod", "-S", "web"]);
        assert_eq!(said.status, 64, "{}", said.err);
        assert!(mac.told().is_empty());

        // The level, the server's own `commands` and the gate of `ssh prod` are the Mac's: its
        // words are the script's, and nothing is started.
        mac.mac_says(
            "bateri 2\nerror refused this reaches another machine, which only \"full\" does\n",
        );
        let said = run(&["split", "-S", "prod", "--", "make"]);
        assert_eq!(said.status, 77, "{}", said.err);
        assert!(said.err.contains("only \"full\""), "{}", said.err);
        assert_eq!(said.out, "");
        mac.mac_says("bateri 2\nerror failed prod is not a server bateri keeps terminals on\n");
        let said = run(&["split", "-S", "prod", "--", "make"]);
        assert_eq!(said.status, 1, "{}", said.err);
        assert!(said.err.contains("keeps terminals on"), "{}", said.err);
        assert_eq!(mac.ssh_calls().len(), 0);
        assert_eq!(prod.calls_of("new"), Vec::<Vec<String>>::new());

        // No Mac, or one that says nothing.
        mac.no_mac();
        let said = run(&["split", "-S", "prod"]);
        assert_eq!(said.status, 69, "{}", said.err);
        mac.mac_is_silent();
        let said = run(&["tab", "-S", "prod"]);
        assert_eq!(said.status, 1, "{}", said.err);
        assert_eq!(mac.ssh_calls().len(), 0);

        // A server bateri never opened has no command: it is told to be, with nothing installed.
        mac.no_mac();
        mac.forget_calls();
        mac.mac_says(&ssh_words());
        std::fs::remove_file(prod.script()).expect("the command");
        let said = run(&["split", "-S", "prod", "--", "make"]);
        assert_eq!(said.status, 1, "{}", said.err);
        assert!(
            said.err.contains("not set up on prod") && said.err.contains("first"),
            "{}",
            said.err
        );
        assert!(!prod.script().exists());
        assert_eq!(prod.calls_of("new"), Vec::<Vec<String>>::new());
    }
}

#[test]
#[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
fn a_pane_the_mac_refuses_ends_the_terminal_the_server_started() {
    for (name, shell) in shells() {
        let (mac, prod, env) = mac_and_prod(&format!("ends-{name}"));
        mac.mac_says_in_turn(1, &ssh_words());
        mac.mac_says_in_turn(2, "bateri 2\nerror unknown there is no tab t9\n");
        let said = mac.run_file_with(
            &shell,
            &mac.script(),
            Some("p4"),
            &["split", "--tab", "t9", "-S", "prod", "--", "make"],
            &borrowed(&env),
        );
        assert_eq!(
            (said.status, said.err.as_str()),
            (1, "bateri: there is no tab t9\n"),
            "{}",
            shell.display()
        );
        assert_eq!(said.out, "");
        // The server's own command ended it, with no Mac to ask there.
        assert_eq!(prod.calls_of("end"), [["end", "p1", "--force"]]);
        assert!(mac.calls_of("end").is_empty());
    }
}

#[test]
#[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
fn send_capture_and_wait_for_a_pane_of_a_server_run_there_and_keep_their_status() {
    for (name, shell) in shells() {
        let (mac, prod, env) = mac_and_prod(&format!("carried-{name}"));
        mac.mac_says(&ssh_words());
        let env = borrowed(&env);
        let run = |args: &[&str]| mac.run_file_with(&shell, &mac.script(), Some("p4"), args, &env);

        let said = run(&["send", "p9@prod", "echo 'hi' $HOME; ls"]);
        assert_eq!(said.status, 0, "{}", said.err);
        // The text arrives as it was written: nothing of it is read by any shell on the way.
        assert_eq!(
            prod.calls_of("send"),
            [["send", "p9", "echo 'hi' $HOME; ls"]]
        );
        let said = run(&["send-key", "p9@prod", "Enter"]);
        assert_eq!(said.status, 0, "{}", said.err);
        assert_eq!(prod.calls_of("send-key"), [["send-key", "p9", "Enter"]]);
        // A key first is the caller's own terminal's: no machine is named, nobody is asked.
        let before = mac.ssh_calls().len();
        run(&["send-key", "Enter"]);
        assert_eq!(mac.ssh_calls().len(), before);
        assert_eq!(mac.calls_of("send-key"), [["send-key", "Enter"]]);

        let said = run(&["capture", "--history", "20", "p9@prod", "--trim"]);
        assert_eq!(said.status, 0, "{}", said.err);
        assert_eq!(
            prod.calls_of("capture"),
            [["capture", "--history", "20", "p9", "--trim"]]
        );

        // The program's own status comes back through ssh.
        std::fs::write(prod.stub.join("exit.wait"), "7").expect("a status");
        let said = run(&["wait", "p9@prod", "--for", "5s"]);
        assert_eq!(said.status, 7, "{}", said.err);
        assert_eq!(prod.calls_of("wait"), [["wait", "p9", "--for", "5s"]]);

        // Each went to the Mac for the connection first.
        assert!(
            mac.told()
                .iter()
                .all(|text| text.starts_with("bateri 2\nssh\n"))
        );
        // And nothing of it was kablo's here.
        assert!(mac.calls_of("send").is_empty() && mac.calls_of("capture").is_empty());

        // This Mac's own pane written out is this Mac's: no ssh.
        let before = mac.ssh_calls().len();
        let said = run(&["capture", "p4@local"]);
        assert_eq!(said.status, 0, "{}", said.err);
        assert_eq!(mac.calls_of("capture"), [["capture", "p4"]]);
        assert_eq!(mac.ssh_calls().len(), before);

        // A malformed address is no pane, and an unreachable server is 255 or the Mac's word.
        let said = run(&["capture", "p9@pr od"]);
        assert_eq!(said.status, 64, "{}", said.err);
        mac.mac_says(
            "bateri 2\nerror refused this reaches another machine, which only \"full\" does\n",
        );
        let said = run(&["wait", "p9@prod"]);
        assert_eq!(said.status, 77, "{}", said.err);
    }
}

#[test]
#[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
fn focus_close_and_the_layout_verbs_hand_the_address_of_a_pane_of_another_machine_to_the_mac() {
    use crate::agent::message::{Arrange, Subject};
    for (name, shell) in shells() {
        let (mac, _prod, env) = mac_and_prod(&format!("addresses-{name}"));
        let env = borrowed(&env);
        mac.mac_says("bateri 2\nok\n");
        let run = |args: &[&str]| mac.run_file_with(&shell, &mac.script(), Some("p4"), args, &env);
        let address = |text: &str| Anchor::address(text).expect("an address");

        assert_eq!(run(&["focus", "pane", "p9@prod"]).status, 0);
        assert_eq!(run(&["zoom", "p9@prod"]).status, 0);
        assert_eq!(run(&["swap", "p9@prod", "p3"]).status, 0);
        assert_eq!(
            run(&["move", "pane", "p9@prod", "--beside", "p3@local", "--right"]).status,
            0
        );
        assert_eq!(run(&["resize", "p9@prod", "--left"]).status, 0);
        assert_eq!(run(&["close", "pane", "p9@prod"]).status, 0);
        let told: Vec<_> = mac
            .told()
            .iter()
            .map(|text| Request::parse(text).expect("a request"))
            .collect();
        assert_eq!(
            told,
            [
                Request::Arrange(Arrange::Focus(Subject::Pane(Some(address("p9@prod"))))),
                Request::Arrange(Arrange::Zoom(Some(address("p9@prod")))),
                Request::Arrange(Arrange::Swap {
                    pane: address("p9@prod"),
                    other: address("p3"),
                }),
                Request::Arrange(Arrange::Move {
                    pane: address("p9@prod"),
                    beside: address("p3@local"),
                    side: Side::Right,
                }),
                Request::Arrange(Arrange::Resize {
                    pane: Some(address("p9@prod")),
                    side: Side::Left,
                    steps: 1,
                }),
                Request::Close(address("p9@prod")),
            ]
        );
        // None of it needed a connection, and none was made.
        assert!(mac.ssh_calls().is_empty());

        // A pane of another machine is closed by the Mac or by nobody: with none, this machine's
        // own `kablo end` would end another terminal of the same number.
        mac.no_mac();
        let said = run(&["close", "pane", "p9@prod"]);
        assert_eq!(said.status, 69, "{}", said.err);
        assert!(mac.calls_of("end").is_empty());
        // A label is kablo's own where the pane is; and an address is no anchor.
        assert_eq!(run(&["rename", "pane", "p9@prod", "x"]).status, 64);
        assert_eq!(run(&["split", "p9@prod"]).status, 64);
        assert_eq!(run(&["focus", "pane", "p9@"]).status, 64);
    }
}

// ─── from a server that has `full`: the Mac relays ───────────────────────

/// The Mac's answer to the question how to reach a server, when a program on a server asks: the
/// word `relay`.
fn relay_word() -> String {
    Reply::Text("relay\n".to_owned()).encode()
}

/// The Mac's answer to a relay: how the command ended and what it said.
fn relayed(status: u8, stderr: &str, stdout: &str) -> String {
    Reply::Relayed(Relayed::of(status, stderr, stdout)).encode()
}

/// The relay a program is to send for `words` on `server`.
fn relay_text(server: &str, words: &[&str]) -> String {
    Request::Relay(Relay {
        server: server.to_owned(),
        words: words.iter().map(|word| (*word).to_owned()).collect(),
    })
    .encode()
    .expect("a relay")
}

fn ssh_text(server: &str) -> String {
    Request::Ssh(server.to_owned()).encode().expect("an ssh")
}

/// The Mac's answers, one a conversation, in the order the script has them (after the ones it has
/// had already).
struct Turns {
    next: usize,
}

impl Turns {
    fn new() -> Turns {
        Turns { next: 1 }
    }

    fn say(&mut self, server: &Server, replies: &[String]) {
        for reply in replies {
            server.mac_says_in_turn(self.next, reply);
            self.next += 1;
        }
    }
}

#[test]
#[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
fn a_server_given_full_hands_the_commands_for_another_machine_to_the_mac_and_prints_what_comes_back()
 {
    for (name, shell) in shells() {
        let a = Server::new(&format!("relayed-{name}"));
        let mut turns = Turns::new();
        let run = |args: &[&str]| a.run(&shell, Some("p4"), args);

        // capture: the Mac is asked how (`relay`), then to run it; stdout comes back as it was.
        turns.say(&a, &[relay_word(), relayed(0, "", "first\n\ntrail")]);
        let said = run(&["capture", "--history", "20", "p9@rasp", "--trim"]);
        assert_eq!(said.status, 0, "{}: {}", shell.display(), said.err);
        assert_eq!(said.out, "first\n\ntrail");
        assert_eq!(said.err, "");
        let told = a.told();
        assert_eq!(told.len(), 2, "{told:?}");
        assert_eq!(told[0], ssh_text("rasp"));
        assert_eq!(
            told[1],
            relay_text("rasp", &["capture", "--history", "20", "p9", "--trim"])
        );
        // Nothing of it was kablo's here: it runs where the pane is.
        assert!(a.calls_of("capture").is_empty());
        assert!(a.ssh_calls().is_empty());

        // send: the text with a line break and a quote arrives as one word.
        a.forget_calls();
        turns.say(&a, &[relay_word(), relayed(0, "", "")]);
        let said = run(&["send", "p9@rasp", "it's\nme"]);
        assert_eq!(said.status, 0, "{}", said.err);
        assert_eq!(a.told()[1], relay_text("rasp", &["send", "p9", "it's\nme"]));
        assert!(a.calls_of("send").is_empty());

        // `send ID -` reads its text here, since standard input does not cross.
        a.forget_calls();
        turns.say(&a, &[relay_word(), relayed(0, "", "")]);
        let said = a.run_file_input(
            &shell,
            &a.script(),
            Some("p4"),
            &["send", "p9@rasp", "-"],
            &[],
            Some(b"from\nstdin\n"),
        );
        assert_eq!(said.status, 0, "{}", said.err);
        assert_eq!(
            a.told()[1],
            relay_text("rasp", &["send", "p9", "from\nstdin\n"])
        );

        // send-key, and the status and stderr of `wait` come back.
        a.forget_calls();
        turns.say(&a, &[relay_word(), relayed(0, "", "")]);
        let said = run(&["send-key", "p9@rasp", "Enter"]);
        assert_eq!(said.status, 0, "{}", said.err);
        assert_eq!(
            a.told()[1],
            relay_text("rasp", &["send-key", "p9", "Enter"])
        );
        a.forget_calls();
        turns.say(
            &a,
            &[relay_word(), relayed(7, "kablo: it ended\nbadly\n", "")],
        );
        let said = run(&["wait", "p9@rasp", "--for", "5s"]);
        assert_eq!(said.status, 7, "{}", said.err);
        assert_eq!(said.err, "kablo: it ended\nbadly\n");
        assert_eq!(said.out, "");
        assert_eq!(
            a.told()[1],
            relay_text("rasp", &["wait", "p9", "--for", "5s"])
        );

        // This Mac is `@local` to a server, and asked for under that word.
        a.forget_calls();
        turns.say(&a, &[relay_word(), relayed(0, "", "")]);
        let said = run(&["send", "p3@local", "hi"]);
        assert_eq!(said.status, 0, "{}", said.err);
        let told = a.told();
        assert_eq!(told[0], ssh_text("@local"));
        assert_eq!(told[1], relay_text("@local", &["send", "p3", "hi"]));

        // A key first, or a pane with no machine, is this machine's own: nobody is asked.
        a.forget_calls();
        run(&["send-key", "Enter"]);
        assert!(a.told().is_empty());
        assert_eq!(a.calls_of("send-key"), [["send-key", "Enter"]]);
        // A malformed address is no pane.
        let said = run(&["capture", "p9@pr od"]);
        assert_eq!(said.status, 64, "{}", said.err);
        assert!(a.told().is_empty());
    }
}

#[test]
#[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
fn a_server_that_is_not_given_full_is_told_77_and_a_mac_that_is_gone_or_odd_is_said_so() {
    for (name, shell) in shells() {
        let a = Server::new(&format!("relay-refused-{name}"));
        let mut turns = Turns::new();
        let run = |args: &[&str]| a.run(&shell, Some("p4"), args);
        let refused =
            "bateri 2\nerror refused this reaches another machine, which only \"full\" does\n";

        // The first question is the Mac's to refuse: nothing more is asked, nothing is run here.
        turns.say(&a, &[refused.to_owned()]);
        let said = run(&["send", "p9@rasp", "x"]);
        assert_eq!(said.status, 77, "{}", said.err);
        assert!(said.err.contains("only \"full\""), "{}", said.err);
        assert_eq!(a.told().len(), 1);
        assert!(a.calls_of("send").is_empty());

        // So is the relay itself.
        a.forget_calls();
        turns.say(&a, &[relay_word(), refused.to_owned()]);
        let said = run(&["capture", "p9@rasp"]);
        assert_eq!(said.status, 77, "{}", said.err);
        assert_eq!(said.out, "");

        // A server whose `commands` are off, or that bateri does not keep terminals on.
        a.forget_calls();
        turns.say(
            &a,
            &[
                "bateri 2\nerror failed rasp is not a server bateri keeps terminals on\n"
                    .to_owned(),
            ],
        );
        let said = run(&["wait", "p9@rasp"]);
        assert_eq!(said.status, 1, "{}", said.err);
        assert!(said.err.contains("keeps terminals on"), "{}", said.err);

        // A command the Mac could not run in time, an answer this does not read, no Mac.
        a.forget_calls();
        turns.say(
            &a,
            &[
                relay_word(),
                "bateri 2\nerror failed the command ran longer than 3600 seconds and was ended\n"
                    .to_owned(),
            ],
        );
        let said = run(&["wait", "p9@rasp"]);
        assert_eq!(said.status, 1, "{}", said.err);
        assert!(said.err.contains("longer than"), "{}", said.err);
        a.forget_calls();
        turns.say(
            &a,
            &[relay_word(), "bateri 2\nok text\nnot this\n".to_owned()],
        );
        let said = run(&["wait", "p9@rasp"]);
        assert_eq!(said.status, 1, "{}", said.err);
        assert_eq!(said.out, "");
        a.forget_calls();
        a.no_mac();
        let said = run(&["send", "p9@rasp", "x"]);
        assert_eq!(said.status, 69, "{}", said.err);
    }
}

#[test]
#[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
fn a_server_given_full_starts_a_terminal_on_another_machine_through_the_mac_and_asks_for_its_pane()
{
    for (name, shell) in shells() {
        let a = Server::new(&format!("relay-split-{name}"));
        let mut turns = Turns::new();
        let run = |args: &[&str]| a.run(&shell, Some("p4"), args);

        // The terminal is started there by the command the Mac runs; the pane is asked for beside
        // this machine's own, with the server's name.
        turns.say(
            &a,
            &[relay_word(), relayed(0, "", "p7\n"), PLACED.to_owned()],
        );
        let said = run(&[
            "split",
            "--below",
            "-S",
            "rasp",
            "--label",
            "work",
            "--keep-open",
            "--",
            "make",
            "it's ok",
        ]);
        assert_eq!(said.status, 0, "{}: {}", shell.display(), said.err);
        assert_eq!(said.out, "p7@rasp\n");
        assert_eq!(said.err, "");
        let told = a.told();
        assert_eq!(told.len(), 3, "{told:?}");
        assert_eq!(told[0], ssh_text("rasp"));
        assert_eq!(
            told[1],
            relay_text(
                "rasp",
                &[
                    "split",
                    "--label",
                    "work",
                    "--keep-open",
                    "--",
                    "make",
                    "it's ok"
                ]
            )
        );
        let wanted = Place {
            side: Some(Side::Below),
            keep_open: true,
            server: Some("rasp".to_owned()),
            ..place(7, 4)
        };
        assert_eq!(told[2], encoded(wanted));
        // Nothing was started or recorded here, and no ssh was run.
        assert!(a.calls_of("new").is_empty());
        assert!(!a.state().join("terminals/p7").exists());
        assert!(a.ssh_calls().is_empty());

        // This Mac is `@local`.
        a.forget_calls();
        turns.say(
            &a,
            &[relay_word(), relayed(0, "", "p2\n"), PLACED.to_owned()],
        );
        let said = run(&["split", "-S", "@local", "--", "make"]);
        assert_eq!(said.status, 0, "{}", said.err);
        assert_eq!(said.out, "p2@local\n");
        let told = a.told();
        assert_eq!(told[0], ssh_text("@local"));
        assert_eq!(told[1], relay_text("@local", &["split", "--", "make"]));
        let wanted = Place {
            server: Some("@local".to_owned()),
            ..place(2, 4)
        };
        assert_eq!(told[2], encoded(wanted));

        // `--wait` waits there, through the Mac, and ends with the program's status.
        a.forget_calls();
        turns.say(
            &a,
            &[
                relay_word(),
                relayed(0, "", "p8\n"),
                PLACED.to_owned(),
                relayed(5, "", ""),
            ],
        );
        let said = run(&["split", "-S", "rasp", "--wait", "--", "make"]);
        assert_eq!(said.status, 5, "{}", said.err);
        assert_eq!(a.told()[3], relay_text("rasp", &["wait", "p8"]));

        // A pane the Mac refuses ends the terminal that was started there, through the Mac.
        a.forget_calls();
        turns.say(
            &a,
            &[
                relay_word(),
                relayed(0, "", "p9\n"),
                "bateri 2\nerror unknown there is no tab t9\n".to_owned(),
                relayed(0, "", ""),
            ],
        );
        let said = run(&["split", "--tab", "t9", "-S", "rasp", "--", "make"]);
        assert_eq!(
            (said.status, said.err.as_str()),
            (1, "bateri: there is no tab t9\n"),
            "{}",
            shell.display()
        );
        assert_eq!(
            a.told()[3],
            relay_text("rasp", &["close", "pane", "p9", "--force"])
        );

        // A command that cannot start the terminal is its failure: the status, what it said, and
        // no pane is asked for.
        a.forget_calls();
        turns.say(
            &a,
            &[
                relay_word(),
                relayed(
                    127,
                    "bateri: bateri is not set up on rasp: open a terminal on it in bateri first\n",
                    "",
                ),
            ],
        );
        let said = run(&["split", "-S", "rasp", "--", "make"]);
        assert_eq!(said.status, 1, "{}", said.err);
        assert!(said.err.contains("not set up on rasp"), "{}", said.err);
        assert_eq!(a.told().len(), 2);
    }
}

#[test]
#[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
fn on_this_mac_at_local_is_the_plain_terminal_and_pane_and_nobody_is_asked_for_a_relay() {
    for (name, shell) in shells() {
        let mac = Server::new_here(&format!("relay-mac-{name}"));
        mac.mac_says(PLACED);
        let said = mac.run(&shell, Some("p4"), &["split", "-S", "@local", "--", "make"]);
        assert_eq!(said.status, 0, "{}: {}", shell.display(), said.err);
        // The terminal is this Mac's own: started here, written without a machine, and the Mac
        // is asked for a plain pane.
        assert_eq!(said.out, "p1\n", "{}", said.err);
        assert_eq!(mac.calls_of("new").len(), 1);
        let told = mac.told();
        assert_eq!(told.len(), 1, "{told:?}");
        assert_eq!(told[0], encoded(place(1, 4)));
        let said = mac.run(&shell, Some("p4"), &["split", "-S", "@local", "-S", "prod"]);
        assert_eq!(said.status, 64, "{}", said.err);
        let said = mac.run(&shell, Some("p4"), &["send", "p3@local", "hi"]);
        assert_eq!(said.status, 0, "{}", said.err);
        assert_eq!(mac.calls_of("send"), [["send", "p3", "hi"]]);
        assert_eq!(mac.told().len(), 1);
    }
}

#[test]
#[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
fn the_command_the_window_runs_for_a_relay_reaches_the_script_of_the_other_machine_with_its_words_whole()
 {
    use super::relay;
    for (name, shell) in login_shells() {
        let rasp = Server::new(&format!("relay-run-{name}"));
        // The command the window builds, run as it is on this Mac (a shell in the home of the
        // machine, which is the login shell of the person: sh, dash, bash, zsh and fish here).
        let run = |words: &[&str]| {
            let words: Vec<String> = words.iter().map(|word| (*word).to_owned()).collect();
            let line = relay::remote_line(&relay::not_set_up("rasp"), &words);
            let mut command = relay::local_command(&rasp.home, &line);
            command
                .env_clear()
                .env("HOME", &rasp.home)
                .env("SHELL", &shell)
                .env("PATH", std::env::var_os("PATH").unwrap_or_default())
                .env("STUB_DIR", &rasp.stub)
                .env("LC_TERMINAL_VERSION", VERSION);
            relay::execute(command, std::time::Duration::from_secs(30), relay::OUTPUT)
                .expect("the command ran")
        };

        // The words arrive whole at the script, which hands them to kablo as they are: whatever
        // the login shell, a backslash, a quote, a dollar, a line break and a trailing backslash
        // are only text.
        let odd = [
            "echo 'hi' $HOME; `x` \"y\" \\z",
            "C:\\",
            "\\';echo INJECTED;#",
            "two\nlines\n",
            "$(touch /tmp/never-relay)",
            "",
            "-",
            "ünï ✓",
        ];
        for word in odd {
            rasp.forget_calls();
            let done = run(&["send", "p9", word]);
            assert_eq!(done.status, 0, "{name} {word:?}: {done:?}");
            assert_eq!(done.stdout, "", "{name} {word:?}");
            assert_eq!(rasp.calls_of("send"), [["send", "p9", word]], "{name}");
        }
        assert!(!Path::new("/tmp/never-relay").exists());

        // Its status and what it says come back.
        std::fs::write(rasp.stub.join("exit.wait"), "7").expect("a status");
        let done = run(&["wait", "p9", "--for", "5s"]);
        assert_eq!(done.status, 7, "{done:?}");
        let done = run(&["send", "p 9", "x"]);
        assert_eq!(done.status, 64, "{done:?}");
        assert!(!done.stderr.is_empty());

        // A terminal started there prints its number and no note about having no pane.
        let done = run(&["split", "--label", "work", "--", "make", "it's ok"]);
        assert_eq!(done.status, 0, "{done:?}");
        assert_eq!(done.stdout, "p1\n");
        assert!(done.stderr.is_empty(), "{done:?}");
        let calls = rasp.calls_of("new");
        assert_eq!(calls.len(), 1, "{calls:?}");
        assert_eq!(
            new_parts(&calls[0]),
            (
                "p1".to_owned(),
                rasp.home.display().to_string(),
                vec!["make".to_owned(), "it's ok".to_owned()]
            )
        );

        // A machine that was never opened from bateri is told to be.
        std::fs::remove_file(rasp.script()).expect("the command");
        let done = run(&["capture", "p9"]);
        assert_eq!(done.status, 127, "{done:?}");
        assert_eq!(
            done.stderr,
            vec!["bateri: bateri is not set up on rasp: open a terminal on it in bateri first"]
        );
    }
}

#[test]
#[ignore = "heavy: starts real processes or waits on a clock; make test-heavy"]
fn the_line_a_mac_sends_a_server_is_read_the_same_by_whatever_shell_the_server_gives_a_person() {
    for (name, shell) in login_shells() {
        let (mac, prod, env) = mac_and_prod(&format!("shells-{name}"));
        mac.mac_says(&ssh_words());
        let mut env = env;
        env.push(("REMOTE_SHELL".to_owned(), shell.display().to_string()));
        let env = borrowed(&env);
        for word in ["C:\\", "\\';echo INJECTED;#", "it's $HOME `x` \"y\"", "ünï"] {
            prod.forget_calls();
            let said = mac.run_file_with(
                Path::new("/bin/sh"),
                &mac.script(),
                Some("p4"),
                &["send", "p9@prod", word],
                &env,
            );
            assert_eq!(said.status, 0, "{name} {word:?}: {}", said.err);
            assert_eq!(said.out, "", "{name} {word:?}");
            assert_eq!(prod.calls_of("send"), [["send", "p9", word]], "{name}");
        }
    }
}
