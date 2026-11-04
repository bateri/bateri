//! The `bateri` command a window runs on another machine for a program on a server it gave `full`.
//!
//! A program on a server has no way to another machine: the connection it has is the one to the
//! Mac. So the Mac does what the program could not — the `ssh` the command on this Mac runs itself
//! (`bateri split -S prod`), here run by the window, or, for a pane of this Mac, a shell on this Mac
//! — and says how the command ended and what it said ([`Relayed`]). The command is the one the
//! person's own `bateri` is on the other machine: `bateri` there, found where its files are, started by the
//! login shell (so its `PATH` is the person's). The command on the server that asked builds nothing
//! of it. It names the machine and the words, and the window refuses what is not one of
//! [`RELAYED`](super::message::RELAYED) before it gets here.
//!
//! **No byte the program chose is ever read by a shell.** The line the other machine's shells read
//! (the one `ssh` sends reaches the person's login shell, which may be fish, whose single quotes
//! are not `sh`'s) holds fixed text and the words as hex (`x` and the hex of their bytes); the
//! `bateri` there turns them back into words (`--hex`) before it reads a command.
//!
//! **This is the one place a window runs a program** (the contract's "the window writes no bytes to
//! a shell and runs no program" is kept for `own` and `screen`; `full` is a person saying a server
//! may work on their other machines, and one that is taken over can do on this Mac what that person
//! can). What is run is bounded: the commands above, one process at a time per request, no input,
//! [`TIME`] to end and [`OUTPUT`] bytes to say.

use std::io::Read;
use std::os::unix::process::CommandExt as _;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use super::message::Relayed;

/// A word as `bateri --hex` takes it: `x` and the hex of its bytes, so that no shell on the way
/// reads it (`kablo bateri --hex` is the reader).
fn hex_word(word: &str) -> String {
    let mut out = String::with_capacity(1 + word.len() * 2);
    out.push('x');
    for byte in word.bytes() {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// The longest a relayed command may run. A `wait` for a long job is the reason it is long; the
/// limit is for a program that went away, whose request nobody answers any more.
pub const TIME: Duration = Duration::from_secs(60 * 60);

/// The most a relayed command may print: a `capture` of a long history is the reason it is
/// large.
pub const OUTPUT: usize = 4 * 1024 * 1024;

/// The most of stderr that is kept; a reply carries a few lines of it ([`Relayed::of`]). What is
/// said beyond it is read and dropped, so a command that says a lot is not ended by a closed pipe.
const STDERR: usize = 64 * 1024;

/// How long the output of a command that has ended is waited for: a process it left behind may
/// hold the pipe open, and the reply does not wait for that.
const DRAIN: Duration = Duration::from_secs(2);

/// What a machine that has not been set up says when the command is not there.
pub fn not_set_up(server: &str) -> String {
    if server == super::LOCAL_SERVER {
        "the bateri command is not set up on this Mac (it is off in the settings, or no terminal of this Mac has run yet)".to_owned()
    } else {
        format!("bateri is not set up on {server}: open a terminal on it in bateri first")
    }
}

/// `word` in single quotes, as `sh` reads them (a quote in it closes them, is written `\'` and
/// opens them again). Only fixed text goes through it: fish reads a backslash in single quotes
/// differently, and nothing the program chose is in what this quotes.
fn quote(word: &str) -> String {
    format!("'{}'", word.replace('\'', "'\\''"))
}

/// The command line to run on the other machine, which a shell of any kind reads: a `sh` that hands
/// over to the login shell, which runs a `sh` that looks for the command where the files of that
/// machine are (`XDG_DATA_HOME` as the login shell has it, else under the home; `missing` is what it
/// says, on stderr, with status 127, when it is nowhere) and runs it with the `words`, in hex.
pub fn remote_line(missing: &str, words: &[String]) -> String {
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
    let search = format!(
        "for d in \"${{XDG_DATA_HOME:-$HOME/.local/share}}\" \"$HOME/.local/share\"; do [ -x \"$d/bateri/shell/bin/bateri\" ] && exec env BATERI_QUIET_ORPHAN=1 \"$d/bateri/shell/bin/bateri\" --hex \"$@\"; done; echo \"bateri: {missing}\" >&2; exit 127"
    );
    let login = format!("exec sh -c {} sh{hex}", quote(&search));
    format!(
        "exec sh -c {} sh {}",
        quote("exec \"${SHELL:-/bin/sh}\" -lc \"$1\""),
        quote(&login)
    )
}

/// The command that runs `line` on this Mac, in `home`, as the other machines' shells run it.
pub fn local_command(home: &Path, line: &str) -> Command {
    let mut command = Command::new("/bin/sh");
    command
        .arg("-c")
        .arg(line)
        .current_dir(home)
        .env("HOME", home)
        .process_group(0);
    command
}

/// The command that runs `line` on a server: `program` (`ssh`) with the words that reach it
/// (`argv`, the destination last), as the command on this Mac runs it, and as bateri runs every
/// ssh: in a group of its own, which ends with it, and with the `PATH` a login shell has (a
/// `ProxyCommand` from `~/.ssh/config` is found).
pub fn ssh_command(program: &str, argv: &[String], line: &str) -> Command {
    let mut command = crate::ssh_route::command(program);
    command.args(argv).arg(line);
    command
}

/// What a pipe has said so far, and whether it said more than it may.
#[derive(Default)]
struct Tap {
    kept: Mutex<Vec<u8>>,
    over: AtomicBool,
}

/// Reads `from` into a [`Tap`] on a thread of its own, keeping `limit` bytes. Past it, `drain`
/// reads and drops, else the reading stops (the command is ended by the broken pipe, and its
/// output is an error anyway).
fn tap<R: Read + Send + 'static>(
    from: Option<R>,
    limit: usize,
    drain: bool,
) -> (Arc<Tap>, thread::JoinHandle<()>) {
    let tap = Arc::new(Tap::default());
    let into = Arc::clone(&tap);
    let reader = thread::spawn(move || {
        let Some(mut from) = from else { return };
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
                            return;
                        }
                    }
                }
            }
        }
    });
    (tap, reader)
}

/// Runs `command` with no input, until it ends: how it ended and what it said. A command that runs
/// past `time`, or says more than `output` bytes, is ended and is the error.
pub fn execute(mut command: Command, time: Duration, output: usize) -> Result<Relayed, String> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|error| format!("cannot run the command: {error}"))?;
    let (out, out_reader) = tap(child.stdout.take(), output, false);
    let (err, err_reader) = tap(child.stderr.take(), STDERR, true);
    let deadline = Instant::now() + time;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(error) => {
                crate::program::end_group(&mut child);
                return Err(format!("cannot wait for the command: {error}"));
            }
        }
        if Instant::now() >= deadline {
            crate::program::end_group(&mut child);
            return Err(format!(
                "the command ran longer than {} seconds and was ended",
                time.as_secs()
            ));
        }
        thread::sleep(Duration::from_millis(20));
    };
    // What the command said is read to its end, unless something it left behind keeps the pipe
    // open: then what came is what there is.
    let until = Instant::now() + DRAIN;
    while !(out_reader.is_finished() && err_reader.is_finished()) && Instant::now() < until {
        thread::sleep(Duration::from_millis(5));
    }
    if out.over.load(Ordering::Acquire) {
        return Err(format!("the command said more than {output} bytes"));
    }
    let said = |tap: &Tap| {
        String::from_utf8_lossy(
            &tap.kept
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        )
        .into_owned()
    };
    Ok(Relayed::of(
        // Ended by a signal: not an exit status of the command; 255 is what ssh says too.
        status
            .code()
            .and_then(|code| u8::try_from(code).ok())
            .unwrap_or(255),
        &said(&err),
        &said(&out),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::TempRoot;

    fn words(list: &[&str]) -> Vec<String> {
        list.iter().map(|word| (*word).to_owned()).collect()
    }

    fn run_local(home: &Path, line: &str) -> Result<Relayed, String> {
        let mut command = local_command(home, line);
        command.env("SHELL", "/bin/sh");
        execute(command, Duration::from_secs(20), OUTPUT)
    }

    /// A home with a `bateri` where the files of a machine are, and an executable `body`.
    fn home_with_bateri(name: &str, under: &str, body: &str) -> (TempRoot, std::path::PathBuf) {
        let root = TempRoot::new(&format!("relay-{name}"));
        let home = root.0.join("home");
        let bin = home.join(under).join("bateri/shell/bin");
        std::fs::create_dir_all(&bin).expect("a bin");
        let script = bin.join("bateri");
        std::fs::write(&script, format!("#!/bin/sh\n{body}\n")).expect("a script");
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).expect("exec");
        (root, home)
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
        let line = remote_line("not here", &odd);
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
            remote_line("not here", &words(&["a"])).replace(" x61", ""),
            remote_line("not here", &words(&[])),
        );
    }

    #[test]
    fn the_script_is_looked_for_where_a_machines_files_are_and_the_hex_reaches_it() {
        let hex = |list: &[&str]| -> String {
            list.iter()
                .map(|word| format!(" {}", hex_word(word)))
                .collect()
        };
        // Under the home, the default.
        let (_root, home) = home_with_bateri("under-home", ".local/share", "echo \"[$*]\"");
        let relayed = run_local(&home, &remote_line("n", &words(&["capture", "p9"]))).expect("ran");
        assert_eq!(relayed.status, 0, "{relayed:?}");
        assert_eq!(
            relayed.stdout,
            format!("[--hex{}]\n", hex(&["capture", "p9"]))
        );
        // Where `XDG_DATA_HOME` says, which the login shell has.
        let (root, home) = home_with_bateri("xdg", "xdg-data", "echo xdg \"$#\"");
        let mut command = local_command(&home, &remote_line("n", &words(&["capture", "p9"])));
        command
            .env("SHELL", "/bin/sh")
            .env("XDG_DATA_HOME", home.join("xdg-data"));
        let relayed = execute(command, Duration::from_secs(20), OUTPUT).expect("ran");
        assert_eq!(relayed.stdout, "xdg 3\n", "{relayed:?}");
        drop(root);
    }

    #[test]
    fn a_machine_without_the_script_says_so_and_ends_with_127() {
        let root = TempRoot::new("relay-none");
        let home = root.0.join("home");
        std::fs::create_dir_all(&home).expect("a home");
        let line = remote_line(&not_set_up("prod"), &words(&["send", "p9", "q"]));
        let relayed = run_local(&home, &line).expect("ran");
        assert_eq!(relayed.status, 127);
        assert_eq!(
            relayed.stderr,
            vec!["bateri: bateri is not set up on prod: open a terminal on it in bateri first"]
        );
        assert_eq!(relayed.stdout, "");
        assert!(not_set_up("@local").contains("this Mac"));
    }

    #[test]
    fn what_the_text_of_the_gate_says_cannot_end_the_quotes_it_sits_in() {
        let root = TempRoot::new("relay-gate");
        let home = root.0.join("home");
        std::fs::create_dir_all(&home).expect("a home");
        let marker = root.0.join("ran");
        let line = remote_line(
            &format!("\"; touch {}; echo \"", marker.display()),
            &words(&["send", "p9"]),
        );
        let relayed = run_local(&home, &line).expect("ran");
        assert_eq!(relayed.status, 127);
        assert!(!marker.exists());
    }

    #[test]
    fn a_server_is_reached_with_the_words_that_reach_it_and_the_line_last() {
        let (root, home) = home_with_bateri("server", ".local/share", "printf 'on %s\\n' \"$*\"");
        let stub = root.0.join("ssh");
        std::fs::write(
            &stub,
            format!(
                "#!/bin/sh\nfor a in \"$@\"; do printf '%s\\037' \"$a\"; done > {}/args\nfor last; do :; done\ncd {} && exec env -i HOME={} SHELL=/bin/sh PATH=\"$PATH\" /bin/sh -c \"$last\"\n",
                root.0.display(),
                home.display(),
                home.display()
            ),
        )
        .expect("a stub");
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).expect("exec");
        let line = remote_line(&not_set_up("prod"), &words(&["capture", "p9"]));
        let argv = words(&["-T", "-o", "BatchMode=yes", "prod"]);
        let relayed = execute(
            ssh_command(stub.to_str().expect("a path"), &argv, &line),
            Duration::from_secs(20),
            OUTPUT,
        )
        .expect("ran");
        assert_eq!(
            (relayed.status, relayed.stdout.as_str()),
            (0, "on --hex x63617074757265 x7039\n")
        );
        let args = std::fs::read_to_string(root.0.join("args")).expect("args");
        let args: Vec<&str> = args.trim_end_matches('\u{1f}').split('\u{1f}').collect();
        assert_eq!(&args[..4], ["-T", "-o", "BatchMode=yes", "prod"]);
        assert_eq!(args[4], line);
        assert_eq!(args.len(), 5);
    }

    #[test]
    fn a_command_that_runs_too_long_or_says_too_much_is_ended() {
        let mut slow = Command::new("/bin/sh");
        slow.arg("-c").arg("sleep 30").process_group(0);
        let started = Instant::now();
        let error = execute(slow, Duration::from_millis(200), OUTPUT).expect_err("too long");
        assert!(error.contains("longer than"), "{error}");
        assert!(started.elapsed() < Duration::from_secs(10));

        let mut loud = Command::new("/bin/sh");
        loud.arg("-c").arg("yes x");
        let error = execute(loud, Duration::from_secs(20), 10_000).expect_err("too much");
        assert!(error.contains("more than 10000 bytes"), "{error}");

        let mut missing = Command::new("/nonexistent/ssh");
        missing.arg("x");
        assert!(execute(missing, Duration::from_secs(1), OUTPUT).is_err());
    }

    #[test]
    fn a_process_the_command_left_holding_the_pipe_does_not_hold_the_reply() {
        // The command ends at once and leaves a process that keeps its output open for long.
        let mut leaver = Command::new("/bin/sh");
        leaver
            .arg("-c")
            .arg("echo said; sleep 30 &")
            .process_group(0);
        let started = Instant::now();
        let relayed = execute(leaver, Duration::from_secs(20), OUTPUT).expect("ran");
        assert_eq!((relayed.status, relayed.stdout.as_str()), (0, "said\n"));
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    #[test]
    fn a_command_that_says_a_lot_on_stderr_is_not_ended_by_it() {
        let mut noisy = Command::new("/bin/sh");
        noisy
            .arg("-c")
            .arg("i=0; while [ $i -lt 3000 ]; do echo 'a line of some length, again and again, so that the pipe has a lot to hold' >&2; i=$((i+1)); done; echo done");
        let relayed = execute(noisy, Duration::from_secs(20), OUTPUT).expect("ran");
        assert_eq!((relayed.status, relayed.stdout.as_str()), (0, "done\n"));
        assert_eq!(relayed.stderr.len(), 20);
    }

    #[test]
    fn a_command_killed_by_a_signal_ends_with_255_and_stdout_is_kept_whole() {
        let mut killed = Command::new("/bin/sh");
        killed.arg("-c").arg("printf 'a\\nb'; kill -9 $$");
        let relayed = execute(killed, Duration::from_secs(20), OUTPUT).expect("ran");
        assert_eq!((relayed.status, relayed.stdout.as_str()), (255, "a\nb"));
    }
}
