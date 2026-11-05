//! The `bateri` command a window runs on another machine for a program that asked it to
//! (`bateri split -S prod`, `bateri send p9@prod`).
//!
//! A program has no way to another machine: the connection it has is the one to the bateri app.
//! So the app does what the program could not, over the connection it keeps to that machine
//! ([`Served`]; for this computer's own terminals the one to its local account), and says how the
//! command ended and what it said ([`Relayed`]). The command is the one the person's own `bateri`
//! is on the other machine: `bateri` there, found where its files are, started by the login shell
//! (so its `PATH` is the person's). The program that asked builds nothing of it. It names the
//! machine and the words, and the app refuses what is not one of
//! [`RELAYED`](super::message::RELAYED) before it gets here.
//!
//! **No byte the program chose is ever read by a shell.** The line the login shell reads (which may
//! be fish, whose single quotes are not `sh`'s) holds fixed text and the words as hex (`x` and the
//! hex of its bytes, [`hex_word`]); the `bateri` there turns them back into words (`--hex`, or
//! `--hex-here` where the machine is this computer, which is the app's own and not its server)
//! before it reads a command.
//!
//! **This is the one place the app runs a program for another** (the contract's "the app writes no
//! bytes to a shell and runs no program" is kept for `own` and `screen`; `full` is a person saying
//! a server may work on their other machines, and one that is taken over can do on this computer
//! what that person can). What is run is bounded: the commands above, one program per request, no
//! input, [`OUTPUT`] bytes to say. It lasts as long as the job it waits for, and ends with the
//! conversation: the program that asked hung up ([`Gone`]).

use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use bt_core::CommandLevel;
use kablo_bateri::message::hex_word;
use kablo_bateri::paths;
use kablo_proto::v1::Exit;
use kablo_proto::v1::stream::Command;

use super::LOCAL_SERVER;
use super::level::{self, Why};
use super::message::{Failure, Kind, Relay, Relayed, Reply};
use crate::kablo::SessionId;
use crate::kablo::local;
use crate::kablo::remote::{End, RemoteCommand};
use crate::kablo::served::Served;
use crate::kablo::service::{Backend, Gone};
use crate::ssh_route::Ask;
use crate::ssh_wrap::KabloHost;

/// The most a relayed command may print: a `capture` of a long history is the reason it is
/// large.
pub const OUTPUT: usize = 4 * 1024 * 1024;

/// The most of stderr that is kept; a reply carries a few lines of it ([`Relayed::of`]). What is
/// said beyond it is read and dropped, so a command that says a lot is not held back by it.
const STDERR: usize = 64 * 1024;

/// How long the server may take to start the command, once the connection to it is made.
const OPEN_WAIT: Duration = Duration::from_secs(30);

/// What a machine that has not been set up says when the command is not there.
pub fn not_set_up(server: &str) -> String {
    if server == LOCAL_SERVER {
        "the bateri command is not set up on the computer of the bateri app (@local): it is off in the settings, or no terminal of that computer has run yet".to_owned()
    } else {
        format!("bateri is not set up on {server}: open a terminal on it in bateri first")
    }
}

/// What tells the `bateri` command of a machine that its words are written in hex, and whose app
/// wrote them: the app of another machine, of which a server is the server (`--hex`), or this
/// computer's own, which the command runs for on the computer itself (`--hex-here`): the one
/// starts what has no program with the bootstrap a server is set up with, the other has none.
fn hex_flag(server: &str) -> &'static str {
    if server == LOCAL_SERVER {
        "--hex-here"
    } else {
        "--hex"
    }
}

/// `word` in single quotes, as `sh` reads them (a quote in it closes them, is written `\'` and
/// opens them again). Only fixed text goes through it: fish reads a backslash in single quotes
/// differently, and nothing the program chose is in what this quotes.
fn quote(word: &str) -> String {
    format!("'{}'", word.replace('\'', "'\\''"))
}

/// The line the login shell of the other machine reads, whatever shell that is: a `sh` that looks
/// for the command where the files of that machine are (under the home, [`paths`]; `missing` is
/// what it says, on stderr, with status 127, when it is nowhere) and runs it with `flag` ([`hex_flag`])
/// and the `words`, in hex.
fn login_line(missing: &str, flag: &str, words: &[String]) -> String {
    let hex: String = words
        .iter()
        .map(|word| format!(" {}", hex_word(word)))
        .collect();
    // The text sits between double quotes in the line: only what cannot end them is kept.
    let missing: String = missing
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || " .,:@_-()[]".contains(c) {
                c
            } else {
                '?'
            }
        })
        .collect();
    let command = format!(
        "{}/{}/{}/{}",
        paths::DIR,
        paths::SHELL,
        paths::BIN,
        paths::COMMAND
    );
    let search = format!(
        "[ -x \"$HOME/{command}\" ] && exec \"$HOME/{command}\" {flag} \"$@\"; echo \"bateri: {missing}\" >&2; exit 127"
    );
    format!("exec sh -c {} sh{hex}", quote(&search))
}

/// The program to run on the other machine, as its `argv`: a `sh` that hands over to the login
/// shell, which reads [`login_line`]. The program is not run by a shell that reads it as one line:
/// the words reach the `sh` as they are.
pub fn command(missing: &str, flag: &str, words: &[String]) -> Command {
    let login = login_line(missing, flag, words);
    let argv = [
        "sh",
        "-c",
        "exec \"${SHELL:-/bin/sh}\" -lc \"$1\"",
        "sh",
        login.as_str(),
    ];
    Command::new(argv.iter().map(|word| word.as_bytes().to_vec()).collect())
}

/// What a pipe has said so far, and whether it said more than it may.
#[derive(Default)]
struct Tap {
    kept: Mutex<Vec<u8>>,
    over: AtomicBool,
}

/// Reads `from` into a [`Tap`] on a thread of its own, keeping `limit` bytes. Past it, `drain`
/// reads and drops; else the reading stops and `end` ends the program (its output is an error
/// anyway).
fn tap<R: Read + Send + 'static>(
    mut from: R,
    limit: usize,
    drain: bool,
    end: Option<Arc<End>>,
) -> (Arc<Tap>, thread::JoinHandle<()>) {
    let tap = Arc::new(Tap::default());
    let into = Arc::clone(&tap);
    let reader = thread::spawn(move || {
        let mut chunk = [0u8; 16 * 1024];
        loop {
            match from.read(&mut chunk) {
                Ok(0) => return,
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                Err(_) => return,
                Ok(n) => {
                    let mut kept = into
                        .kept
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    let room = limit.saturating_sub(kept.len());
                    kept.extend_from_slice(&chunk[..n.min(room)]);
                    if n > room {
                        into.over.store(true, Ordering::Release);
                        if !drain {
                            if let Some(end) = &end {
                                end.kill();
                            }
                            return;
                        }
                    }
                }
            }
        }
    });
    (tap, reader)
}

/// A server whose `commands` are off: refused, with the setting that changes it.
pub fn refused_off(server: &KabloHost) -> Failure {
    Failure::new(
        Kind::Refused,
        level::refusal(
            CommandLevel::Off,
            Why::Off,
            (!server.host.is_empty()).then_some(server.host.as_str()),
        ),
    )
}

/// The reply to a relay: the command run on the machine it names, or why it was not. The machine
/// is found again here, as the one bateri keeps terminals on (`backend`; `ask` is how a password
/// is asked for when the connection to it must be made, and `live` gives the ledger's terminals
/// of an account, [`Backend::reach`]), or is this computer, as `local_level` lets it be. Nothing
/// of what the program said is run but its words ([`run`]).
pub fn serve(
    backend: Option<Arc<dyn Backend>>,
    ask: Ask,
    local_level: CommandLevel,
    live: &dyn Fn(&str) -> Vec<SessionId>,
    relay: &Relay,
    gone: &Gone,
) -> Reply {
    let Some(backend) = backend else {
        return Reply::Failed(Failure::new(
            Kind::Failed,
            "this bateri keeps no connection to servers",
        ));
    };
    let server = if relay.server == LOCAL_SERVER {
        if local_level == CommandLevel::Off {
            return Reply::Failed(Failure::new(
                Kind::Refused,
                level::refusal(CommandLevel::Off, Why::Off, None),
            ));
        }
        local::host()
    } else {
        match backend.reach(std::slice::from_ref(&relay.server), ask, live) {
            Err(text) => return Reply::Failed(Failure::new(Kind::Failed, text)),
            Ok(reached) if reached.level == CommandLevel::Off => {
                return Reply::Failed(refused_off(&reached.server));
            }
            Ok(reached) => reached.server,
        }
    };
    match run(
        &Served::new(backend, server),
        &relay.server,
        &relay.words,
        gone,
    ) {
        Ok(relayed) => Reply::Relayed(relayed),
        Err(text) => Reply::Failed(Failure::new(Kind::Failed, text)),
    }
}

/// Runs the `bateri` command with the `words` on `server`, over the connection `served` is, until
/// it ends: how it ended and what it said. When the conversation of the program that asked is over
/// (`gone`) the command is ended. A command that says more than [`OUTPUT`] bytes is ended, and is
/// the error; so is a connection that went while it ran. The `Err` is a line for the person.
pub fn run(
    served: &Served,
    server: &str,
    words: &[String],
    gone: &Gone,
) -> Result<Relayed, String> {
    let job = words.first().map_or("a command", String::as_str);
    let started = served.run(
        command(&not_set_up(server), hex_flag(server), words),
        // The connection is made (and asked for) by whoever found the server.
        Ask::Never,
        OPEN_WAIT,
    )?;
    let RemoteCommand {
        stdin,
        stdout,
        stderr,
        end,
    } = started;
    // Nothing crosses on standard input: a `send -` is read by the program that asked.
    drop(stdin);
    {
        let end = Arc::clone(&end);
        gone.when_over(move || end.kill());
    }
    let (out, out_reader) = tap(stdout, OUTPUT, false, Some(Arc::clone(&end)));
    let (err, err_reader) = tap(stderr, STDERR, true, None);
    let exit = end.wait();
    // What the command said came before how it ended: the readers have it all, or the stream
    // is over and they stop with it.
    let _ = out_reader.join();
    let _ = err_reader.join();
    if out.over.load(Ordering::Acquire) {
        return Err(format!(
            "bateri {job} on {server} printed more than {} MiB and was ended",
            OUTPUT / (1024 * 1024)
        ));
    }
    let status = match exit {
        Ok(Exit::Code(code)) => u8::try_from(code & 0xff).unwrap_or(255),
        // Ended by a signal: not an exit status of the command ([`Relayed::status`]).
        Ok(Exit::Signal(_)) => 255,
        Err(why) => {
            return Err(format!(
                "the connection to {server} ended before bateri {job} did: {why}"
            ));
        }
    };
    let said = |tap: &Tap| {
        String::from_utf8_lossy(
            &tap.kept
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        )
        .into_owned()
    };
    Ok(Relayed::of(status, &said(&err), &said(&out)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::TempRoot;
    use std::os::unix::fs::PermissionsExt as _;
    use std::path::{Path, PathBuf};
    use std::time::Instant;

    fn words(list: &[&str]) -> Vec<String> {
        list.iter().map(|word| (*word).to_owned()).collect()
    }

    /// An executable file with `body` (a `sh` script).
    fn script(at: &Path, body: &str) {
        std::fs::create_dir_all(at.parent().expect("a directory")).expect("a directory");
        std::fs::write(at, format!("#!/bin/sh\n{body}\n")).expect("a script");
        std::fs::set_permissions(at, std::fs::Permissions::from_mode(0o755)).expect("exec");
    }

    /// A home with a `bateri` where the files of a machine are, and `body` for it to run.
    fn home_with_bateri(name: &str, under: &str, body: &str) -> (TempRoot, PathBuf) {
        let root = TempRoot::new(&format!("relay-{name}"));
        let home = root.0.join("home");
        script(&home.join(under).join("bateri/shell/bin/bateri"), body);
        (root, home)
    }

    /// The server `home` is, with `/bin/sh` for the person's login shell.
    fn at(home: &Path) -> Served {
        Served::at_home(home, Path::new("/bin/sh"))
    }

    fn relay(served: &Served, list: &[&str]) -> Result<Relayed, String> {
        run(served, "prod", &words(list), &Gone::new())
    }

    fn hex(list: &[&str]) -> String {
        list.iter()
            .map(|word| format!(" {}", hex_word(word)))
            .collect()
    }

    #[test]
    fn no_byte_the_program_chose_is_in_the_line_a_shell_reads() {
        let odd = words(&[
            "send",
            "p9",
            "",
            "it's $HOME `x` \"y\" \\z \\",
            "two\nlines\n",
            "; touch /tmp/never",
            "-",
            "ünï",
        ]);
        let line = login_line("not here", "--hex", &odd);
        // A backslash or a quote of the words would be read differently by fish and by sh: the
        // line has the quotes of the fixed text and nothing else of the kind.
        assert_eq!(line.matches('\\').count(), line.matches("'\\''").count());
        for word in odd.iter().filter(|word| word.len() > 2) {
            assert!(!line.contains(word.as_str()), "{word:?} is in {line}");
        }
        assert!(!line.contains('\n'), "{line}");
        assert!(line.contains("--hex"));
        assert!(line.contains(" x73656e64 x7039 x "), "{line}");
        // The line is the same fixed text for any words.
        assert_eq!(
            login_line("not here", "--hex", &words(&["a"])).replace(" x61", ""),
            login_line("not here", "--hex", &words(&[])),
        );
    }

    #[test]
    fn the_words_for_this_computer_are_marked_as_its_own_apps_and_a_servers_are_not() {
        let (_root, home) = home_with_bateri("which-app", ".local/share", "echo \"[$*]\"");
        let list = ["split", "--", "zsh"];
        for (server, flag) in [("prod", "--hex"), ("@local", "--hex-here")] {
            let relayed = run(&at(&home), server, &words(&list), &Gone::new()).expect("ran");
            assert_eq!(
                relayed.stdout,
                format!("[{flag}{}]\n", hex(&list)),
                "{server}"
            );
        }
        // Fixed text: the words are in hex after it, whatever they are.
        assert_eq!(
            login_line("x", hex_flag("@local"), &words(&["--hex"])),
            login_line("x", "--hex-here", &words(&["--hex"]))
        );
    }

    #[test]
    fn the_program_is_five_words_and_the_login_shell_reads_the_last() {
        let argv: Vec<String> = command("none", "--hex", &words(&["capture", "p9"]))
            .argv
            .iter()
            .map(|word| String::from_utf8_lossy(word).into_owned())
            .collect();
        assert_eq!(argv.len(), 5, "{argv:?}");
        assert_eq!(
            argv[..4],
            ["sh", "-c", "exec \"${SHELL:-/bin/sh}\" -lc \"$1\"", "sh"]
        );
        assert_eq!(
            argv[4],
            login_line("none", "--hex", &words(&["capture", "p9"]))
        );
    }

    #[test]
    fn the_command_is_looked_for_where_a_machines_files_are_and_the_hex_reaches_it() {
        // Under the home.
        let (_root, home) = home_with_bateri("under-home", ".local/share", "echo \"[$*]\"");
        let relayed = relay(&at(&home), &["capture", "p9"]).expect("ran");
        assert_eq!(relayed.status, 0, "{relayed:?}");
        assert_eq!(
            relayed.stdout,
            format!("[--hex{}]\n", hex(&["capture", "p9"]))
        );
        // Not where `XDG_DATA_HOME` would say: the files of a machine are in one place.
        let (_root, home) = home_with_bateri("xdg", "xdg-data", "echo found");
        let relayed = relay(&at(&home), &["capture", "p9"]).expect("ran");
        assert_eq!((relayed.status, relayed.stdout.as_str()), (127, ""));
    }

    #[test]
    fn a_machine_without_the_command_says_so_and_ends_with_127() {
        let root = TempRoot::new("relay-none");
        let home = root.0.join("home");
        std::fs::create_dir_all(&home).expect("a home");
        let relayed = relay(&at(&home), &["send", "p9", "q"]).expect("ran");
        assert_eq!(relayed.status, 127);
        assert_eq!(
            relayed.stderr,
            vec!["bateri: bateri is not set up on prod: open a terminal on it in bateri first"]
        );
        assert_eq!(relayed.stdout, "");
        // Shown to a program on a server: "this computer" would be read as that server.
        let this = not_set_up("@local");
        assert!(
            this.contains("the computer of the bateri app (@local)"),
            "{this}"
        );
        assert!(
            !this.contains("this computer") && !this.contains("Mac"),
            "{this}"
        );
        // The line the login shell reads keeps all of it (nothing in it is a quote or a `$`).
        let line = login_line(&this, "--hex-here", &words(&[]));
        assert!(line.contains(&this), "{line}");
    }

    #[test]
    fn what_the_text_of_the_gate_says_cannot_end_the_quotes_it_sits_in() {
        let root = TempRoot::new("relay-gate");
        let home = root.0.join("home");
        std::fs::create_dir_all(&home).expect("a home");
        let marker = root.0.join("ran");
        let served = at(&home);
        let started = served
            .run(
                command(
                    &format!("\"; touch {}; echo \"", marker.display()),
                    "--hex",
                    &words(&["send", "p9"]),
                ),
                Ask::Never,
                OPEN_WAIT,
            )
            .expect("started");
        assert_eq!(started.wait(), Ok(Exit::Code(127)));
        assert!(!marker.exists());
    }

    #[test]
    fn the_words_reach_the_command_whole_through_the_login_shell() {
        let odd = [
            "send",
            "p9",
            "it's $HOME `x` \"y\" \\z \\",
            "two\nlines\n",
            "$(touch never)",
            "",
        ];
        let (root, home) = home_with_bateri(
            "login",
            ".local/share",
            "for a in \"$@\"; do printf '%s\\n' \"$a\"; done",
        );
        // A login shell that records how it was started: `-l` and `-c`, and the line.
        let shell = root.0.join("login-shell");
        script(
            &shell,
            &format!(
                "printf '%s\\n' \"$1\" > {}/flag; shift; printf '%s' \"$1\" > {}/line\nexec /bin/sh -c \"$1\"",
                root.0.display(),
                root.0.display()
            ),
        );
        let served = Served::at_home(&home, &shell);
        let relayed = relay(&served, &odd).expect("ran");
        assert_eq!(relayed.status, 0, "{relayed:?}");
        let mut expected = String::from("--hex\n");
        for word in odd {
            expected.push_str(&hex_word(word));
            expected.push('\n');
        }
        assert_eq!(relayed.stdout, expected);
        assert_eq!(
            std::fs::read_to_string(root.0.join("flag")).expect("flag"),
            "-lc\n"
        );
        let line = std::fs::read_to_string(root.0.join("line")).expect("line");
        assert_eq!(line, login_line(&not_set_up("prod"), "--hex", &words(&odd)));
        assert!(!root.0.join("never").exists());
        assert!(!home.join("never").exists());
    }

    #[test]
    fn the_status_of_the_command_is_its_own_and_a_signal_is_255() {
        let (_root, home) =
            home_with_bateri("status", ".local/share", "echo a; echo b >&2; exit 7");
        let relayed = relay(&at(&home), &["capture", "p9"]).expect("ran");
        assert_eq!(
            (relayed.status, relayed.stdout.as_str(), relayed.stderr),
            (7, "a\n", vec!["b".to_owned()])
        );
        let (_root, home) =
            home_with_bateri("signal", ".local/share", "printf 'a\\nb'; kill -9 $$");
        let relayed = relay(&at(&home), &["capture", "p9"]).expect("ran");
        assert_eq!((relayed.status, relayed.stdout.as_str()), (255, "a\nb"));
    }

    #[test]
    fn a_command_that_says_too_much_is_ended_and_the_person_is_told() {
        let (_root, home) = home_with_bateri("loud", ".local/share", "yes x");
        let started = Instant::now();
        let error = relay(&at(&home), &["capture", "p9"]).expect_err("too much");
        assert_eq!(
            error,
            "bateri capture on prod printed more than 4 MiB and was ended"
        );
        assert!(started.elapsed() < Duration::from_secs(20));
    }

    #[test]
    fn a_command_that_says_a_lot_on_stderr_is_not_held_back_by_it() {
        let (_root, home) = home_with_bateri(
            "noisy",
            ".local/share",
            "i=0; while [ $i -lt 3000 ]; do echo 'a line of some length, again and again, so that the pipe has a lot to hold' >&2; i=$((i+1)); done; echo done",
        );
        let relayed = relay(&at(&home), &["capture", "p9"]).expect("ran");
        assert_eq!((relayed.status, relayed.stdout.as_str()), (0, "done\n"));
        assert_eq!(relayed.stderr.len(), 20);
    }

    #[test]
    fn a_conversation_that_is_over_ends_the_command_and_nothing_is_left_running() {
        let (root, home) = home_with_bateri(
            "gone",
            ".local/share",
            "echo $$ > \"$HOME/../pid\"; exec sleep 600",
        );
        let served = at(&home);
        let gone = Gone::new();
        let waiting = {
            let gone = gone.clone();
            thread::spawn(move || run(&served, "prod", &words(&["wait", "p9"]), &gone))
        };
        let pid_file = root.0.join("pid");
        let until = Instant::now() + Duration::from_secs(20);
        while !pid_file.exists() && Instant::now() < until {
            thread::sleep(Duration::from_millis(20));
        }
        let pid: i32 = std::fs::read_to_string(&pid_file)
            .expect("the command started")
            .trim()
            .parse()
            .expect("a pid");
        // SAFETY: signal 0 only asks whether the process is there.
        let alive = || unsafe { libc::kill(pid, 0) == 0 };
        assert!(alive());
        gone.end();
        let reply = waiting.join().expect("the relay ended");
        // The program that asked is gone: what the reply is does not matter, the program must be.
        drop(reply);
        let until = Instant::now() + Duration::from_secs(10);
        while alive() && Instant::now() < until {
            thread::sleep(Duration::from_millis(20));
        }
        assert!(!alive(), "the command is still running");
    }

    #[test]
    fn a_conversation_that_was_over_before_the_command_started_ends_it_at_once() {
        let (_root, home) = home_with_bateri("late", ".local/share", "exec sleep 600");
        let gone = Gone::new();
        gone.end();
        let started = Instant::now();
        let _ = run(&at(&home), "prod", &words(&["wait", "p9"]), &gone);
        assert!(started.elapsed() < Duration::from_secs(20));
    }

    #[test]
    fn a_server_that_cannot_be_asked_is_told_in_its_own_words() {
        let served = Served::refusing("prod wants a login that bateri cannot give");
        let error = relay(&served, &["capture", "p9"]).expect_err("refused");
        assert_eq!(error, "prod wants a login that bateri cannot give");
        let error = relay(&Served::none(), &["capture", "p9"]).expect_err("none");
        assert_eq!(error, "bateri's connections are not running");
    }
}
