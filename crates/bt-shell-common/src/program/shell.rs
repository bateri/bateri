//! A shell's guide bar: a **root** shell — `root  exit to leave`, in the
//! theme's `error` — and a **nested** one — `nested bash  exit to return`.
//! Neither carries the terminal's integration; the bar says where the keys
//! go.
//!
//! **Root is a name and a user**: a shell running with effective user 0,
//! and — where its command line can be read — an interactive one. On macOS
//! it cannot (`KERN_PROCARGS2` refuses another user's process; `ps` reads
//! it only because it is setuid), so there a root script or `sh -c …` that
//! sudo keeps raw reads as a root shell too; on Linux `cmdline` is anyone's
//! and the script is ruled out.
//!
//! **sudo holds the terminal** ([`ELEVATORS`]): its raw modes are its own,
//! not the program's — the password prompt with `pwfeedback` turns
//! `ICANON` and `ECHO` off together (measured), and with `use_pty` (sudo's
//! default since 1.9.14) sudo opens a terminal of its own for the command
//! and keeps ours raw to relay it for as long as the command runs
//! (measured: `sudo sleep 5` leaves ours raw, the command in another
//! session). So under sudo only the command it runs is looked at, through
//! its monitor and `su` ([`super::find`]), by itself: a bar when it is one
//! the bars know by what can be read of it, else no mark.
//!
//! **Nested is a command line**: a shell whose arguments open an
//! interactive session — no command (`-c`) and no script ([`interactive`]).
//! `bash build.sh` asking for a key, or a wrapper `sh` that did not `exec`
//! the agent it runs, is no nested session.
//!
//! **A tool that opened the shell names the bar** ([`TOOLS`]), found
//! where it can be: in the group, when it holds the terminal itself and
//! runs the shell in a terminal of its own (pexpect's way, which `poetry
//! shell` and `pipenv shell` take), or above the shell, outside the group,
//! when it forked it — an interactive shell takes a group of its own
//! (measured, `sudo -i` without `use_pty`). A tool that `exec`s the shell
//! leaves nothing behind, and the bar is the shell's. None of the five
//! tools is installed where this was written: how each runs its shell is
//! read from its own documentation and code, not measured.

use super::{Family, base, python_script};
use crate::jobs::ProcArgs;

/// A shell the bars know: its name and the options that take the next
/// argument as their value.
struct Shell {
    name: &'static str,
    /// Short options that take a value (`-o vi`); in a cluster the value is
    /// the next argument when the letter ends it (`-eo pipefail`).
    valued: &'static str,
    /// Long options that take a value, unless attached with `=`.
    valued_long: &'static [&'static str],
    /// Long options that run a command and leave.
    command_long: &'static [&'static str],
}

/// The shells, by process name. An option missing from a list that takes a
/// value makes its value look like a script: no bar, never a wrong one.
const SHELLS: [Shell; 7] = [
    Shell {
        name: "bash",
        valued: "oO",
        valued_long: &["--rcfile", "--init-file"],
        command_long: &[],
    },
    Shell {
        name: "zsh",
        valued: "o",
        valued_long: &["--emulate"],
        command_long: &[],
    },
    Shell {
        name: "sh",
        valued: "o",
        valued_long: &[],
        command_long: &[],
    },
    Shell {
        name: "dash",
        valued: "o",
        valued_long: &[],
        command_long: &[],
    },
    Shell {
        name: "fish",
        valued: "Cdo",
        valued_long: &[
            "--init-command",
            "--debug",
            "--debug-output",
            "--features",
            "--profile",
            "--profile-startup",
        ],
        command_long: &["--command"],
    },
    Shell {
        name: "nu",
        valued: "eIm",
        valued_long: &[
            "--execute",
            "--include-path",
            "--table-mode",
            "--config",
            "--env-config",
            "--log-level",
            "--log-target",
            "--plugin-config",
        ],
        command_long: &["--commands"],
    },
    // A Python script: on macOS its process is named after the interpreter
    // and found by its script ([`program_line`]).
    Shell {
        name: "xonsh",
        valued: "D",
        valued_long: &["--rc", "--shell-type"],
        command_long: &[],
    },
];

/// The programs that hold the terminal while they run another as root:
/// sudo (and `su`, the same shape) — whose own raw modes say nothing of the
/// program ([`super::Found::Sudo`]), and under which the command is looked
/// for through them. They run as user 0 (setuid).
pub(super) const ELEVATORS: [&str; 2] = ["sudo", "su"];

/// A tool that opens a shell: its process (or Python script) name, the
/// subcommand that does it, its options that take a separate value (before
/// the subcommand, a value would read as one), the options that run a
/// command instead, and the bar's name for it.
struct Tool {
    name: &'static str,
    subcommand: Option<&'static str>,
    valued: &'static [&'static str],
    commands: &'static [&'static str],
    title: &'static str,
}

const TOOLS: [Tool; 5] = [
    Tool {
        name: "nix-shell",
        subcommand: None,
        valued: &[],
        commands: &["--run", "--command"],
        title: "nix-shell",
    },
    Tool {
        name: "nix",
        subcommand: Some("develop"),
        valued: &[
            "--extra-experimental-features",
            "--experimental-features",
            "-I",
            "--include",
            "--log-format",
            "--store",
        ],
        commands: &["-c", "--command"],
        title: "nix develop",
    },
    Tool {
        name: "poetry",
        subcommand: Some("shell"),
        valued: &["-C", "--directory", "-P", "--project"],
        commands: &[],
        title: "poetry shell",
    },
    Tool {
        name: "pipenv",
        subcommand: Some("shell"),
        valued: &["--python", "--pypi-mirror"],
        commands: &[],
        title: "pipenv shell",
    },
    Tool {
        name: "devbox",
        subcommand: Some("shell"),
        valued: &["-c", "--config"],
        commands: &[],
        title: "devbox shell",
    },
];

/// The bar's word before a nested shell's name — a UI string.
pub(super) const NESTED: &str = "nested";

/// A process name worth a record for this module: a shell, a tool or an
/// elevator (a Python script's name is the interpreter's, already a
/// candidate).
pub(super) fn is_candidate(name: &str) -> bool {
    is_shell(name) || ELEVATORS.contains(&name) || TOOLS.iter().any(|tool| tool.name == name)
}

/// Whether `name` is a shell's.
pub(super) fn is_shell(name: &str) -> bool {
    SHELLS.iter().any(|shell| shell.name == name)
}

/// A root shell: a shell's name and effective user 0, and — when its
/// record could be read (Linux) — an interactive command line
/// ([`nested`]); on macOS the name and the user are all that is known.
pub(super) fn is_root_shell(name: &str, uid: Option<u32>, record: Option<&ProcArgs>) -> bool {
    uid == Some(0) && is_shell(name) && record.is_none_or(|record| nested(name, record).is_some())
}

/// What a process opened, as the nested bar names it: a tool's title, or
/// an interactive shell's name; `None` for anything else.
pub(super) fn nested(name: &str, record: &ProcArgs) -> Option<&'static str> {
    let (program, args) = program_line(name, record)?;
    if let Some(title) = tool_line(program, args) {
        return Some(title);
    }
    let shell = SHELLS.iter().find(|shell| shell.name == program)?;
    interactive(shell, args).then_some(shell.name)
}

/// A tool's title when `name`'s process is one that opens a shell.
pub(super) fn tool(name: &str, record: &ProcArgs) -> Option<&'static str> {
    let (program, args) = program_line(name, record)?;
    tool_line(program, args)
}

/// The program a process runs and its arguments: a Python script's name
/// and its own arguments (`python /usr/bin/poetry shell` — on Linux the
/// process is named after the script, on macOS after the interpreter, and
/// the argv is the interpreter's on both), else the process's name and
/// argv without argv[0].
fn program_line<'a>(name: &'a str, record: &'a ProcArgs) -> Option<(&'a str, &'a [String])> {
    let args = record.args.get(1..).unwrap_or_default();
    let python = |name: &str| Family::of_name(name) == Some(Family::Python);
    let interpreted = python(name) || record.args.first().is_some_and(|zero| python(base(zero)));
    if interpreted {
        let (script, rest) = python_script(args)?;
        return Some((base(script), rest));
    }
    Some((name, args))
}

/// The tool `program` with `args` is, when it opens a shell: its
/// subcommand is the first argument that is neither an option nor the value
/// of one ([`Tool::valued`]; an unlisted one's value reads as the
/// subcommand — no bar), and none of its command options is given
/// (`nix-shell --run make` runs make).
fn tool_line(program: &str, args: &[String]) -> Option<&'static str> {
    let tool = TOOLS.iter().find(|tool| tool.name == program)?;
    let mut rest = args.iter();
    let mut subcommand = None;
    while let Some(arg) = rest.next() {
        if !arg.starts_with('-') {
            subcommand = Some(arg);
            break;
        }
        if tool.valued.contains(&arg.as_str()) {
            rest.next();
        }
    }
    let opens = tool
        .subcommand
        .is_none_or(|wanted| subcommand.is_some_and(|given| given == wanted));
    let runs = args.iter().any(|arg| {
        let option = arg
            .split_once('=')
            .map_or(arg.as_str(), |(option, _)| option);
        tool.commands.contains(&option)
    });
    (opens && !runs).then_some(tool.title)
}

/// Whether a shell's arguments (argv without argv[0]) open an interactive
/// session: no command — a `c` in a cluster of short options (`-c`,
/// `-lc`), the long ones of [`Shell::command_long`] — and no operand, a
/// script; `-i` changes neither (`bash -i build.sh` runs the script and
/// leaves). `--` and a lone `-` end the options; `+o` sets them like `-o`.
fn interactive(shell: &Shell, args: &[String]) -> bool {
    let mut index = 0;
    while let Some(arg) = args.get(index) {
        index += 1;
        if arg == "--" || arg == "-" {
            return index >= args.len();
        }
        if let Some(long) = arg.strip_prefix("--") {
            let (option, attached) = match long.split_once('=') {
                Some((option, _)) => (option, true),
                None => (long, false),
            };
            let option = format!("--{option}");
            if shell.command_long.contains(&option.as_str()) {
                return false;
            }
            if !attached && shell.valued_long.contains(&option.as_str()) {
                index += 1;
            }
            continue;
        }
        let Some(cluster) = arg.strip_prefix('-').or_else(|| arg.strip_prefix('+')) else {
            // An operand: a script, or a value an option above did not
            // claim.
            return false;
        };
        for (at, letter) in cluster.char_indices() {
            if letter == 'c' && arg.starts_with('-') {
                return false;
            }
            if shell.valued.contains(letter) {
                // The value is the cluster's rest, or the next argument.
                index += usize::from(at + letter.len_utf8() == cluster.len());
                break;
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(argv: &[&str]) -> ProcArgs {
        ProcArgs {
            exec: String::new(),
            args: argv.iter().map(|&arg| arg.to_owned()).collect(),
            env: Vec::new(),
        }
    }

    fn opens(name: &str, argv: &[&str]) -> Option<&'static str> {
        nested(name, &record(argv))
    }

    #[test]
    fn a_shell_with_no_command_and_no_script_is_a_session() {
        for (name, argv) in [
            ("bash", &["bash"][..]),
            ("bash", &["-bash"]),
            ("bash", &["bash", "-l"]),
            ("bash", &["bash", "--login", "-i"]),
            ("bash", &["bash", "-o", "vi"]),
            ("bash", &["bash", "+o", "history"]),
            ("bash", &["bash", "--rcfile", "/tmp/nix-shell-1-0/rc"]),
            ("bash", &["bash", "--rcfile=/tmp/rc", "--noprofile"]),
            ("bash", &["bash", "-eo", "pipefail"]),
            ("zsh", &["zsh", "-o", "vi", "-l"]),
            ("zsh", &["zsh", "--emulate", "sh"]),
            ("sh", &["sh"]),
            ("dash", &["dash", "-i", "--"]),
            ("fish", &["fish", "-C", "echo hi", "--private"]),
            ("fish", &["fish", "--init-command=set x 1"]),
            ("nu", &["nu", "-e", "ls", "--config", "/x.nu"]),
        ] {
            assert_eq!(opens(name, argv), Some(name), "{argv:?}");
        }
    }

    #[test]
    fn a_shell_given_a_command_or_a_script_is_none() {
        for (name, argv) in [
            ("bash", &["bash", "build.sh"][..]),
            ("bash", &["bash", "-c", "read -sn1"]),
            ("bash", &["bash", "-lc", "codex"]),
            ("bash", &["bash", "-i", "build.sh"]),
            ("bash", &["bash", "--", "build.sh"]),
            ("bash", &["/bin/bash", "-e", "/usr/local/bin/sbt"]),
            ("sh", &["sh", "-c", "exec node codex.js"]),
            ("sh", &["sh", "-", "arg"]),
            ("zsh", &["zsh", "-o", "vi", "script.zsh"]),
            ("fish", &["fish", "-c", "ls"]),
            ("fish", &["fish", "--command=ls"]),
            ("nu", &["nu", "--commands", "ls"]),
            ("nu", &["nu", "script.nu"]),
            // An option the table does not know that takes a value: its
            // value reads as a script — no bar.
            ("bash", &["bash", "--unknown-valued", "x"]),
        ] {
            assert_eq!(opens(name, argv), None, "{argv:?}");
        }
        // Not a shell's name at all.
        assert_eq!(opens("bashful", &["bashful"]), None);
    }

    #[test]
    fn a_tool_that_opens_a_shell_is_named_by_its_title() {
        let python = "/opt/homebrew/Cellar/python@3.13/Python";
        for (name, argv, title) in [
            // macOS: the process is the interpreter, the argv its.
            (
                "Python",
                &[python, "/Users/me/.local/bin/poetry", "shell"][..],
                "poetry shell",
            ),
            // Linux: named after the script, the argv the interpreter's.
            (
                "poetry",
                &["/usr/bin/python3", "/usr/bin/poetry", "shell"],
                "poetry shell",
            ),
            (
                "python3.12",
                &["python3", "-I", "/usr/bin/pipenv", "shell"],
                "pipenv shell",
            ),
            ("devbox", &["devbox", "shell"], "devbox shell"),
            ("nix-shell", &["nix-shell", "-p", "hello"], "nix-shell"),
            ("nix", &["nix", "develop", ".#dev"], "nix develop"),
            // A global option's value is no subcommand.
            (
                "poetry",
                &["/usr/bin/python3", "/usr/bin/poetry", "-C", "/w", "shell"],
                "poetry shell",
            ),
            (
                "pipenv",
                &[
                    "/usr/bin/python3",
                    "/usr/bin/pipenv",
                    "--python",
                    "3.12",
                    "shell",
                ],
                "pipenv shell",
            ),
            (
                "nix",
                &[
                    "nix",
                    "--extra-experimental-features",
                    "nix-command flakes",
                    "develop",
                ],
                "nix develop",
            ),
        ] {
            assert_eq!(opens(name, argv), Some(title), "{argv:?}");
            assert_eq!(tool(name, &record(argv)), Some(title), "{argv:?}");
        }
        for (name, argv) in [
            (
                "Python",
                &[python, "/Users/me/.local/bin/poetry", "install"][..],
            ),
            ("devbox", &["devbox", "run", "test"]),
            ("devbox", &["devbox", "run", "shell"]),
            // An unlisted option's value reads as the subcommand.
            (
                "poetry",
                &[
                    "/usr/bin/python3",
                    "/usr/bin/poetry",
                    "--unlisted",
                    "x",
                    "shell",
                ],
            ),
            ("nix-shell", &["nix-shell", "-p", "hello", "--run", "hello"]),
            ("nix-shell", &["nix-shell", "--command=make"]),
            ("nix", &["nix", "develop", "-c", "make"]),
            ("nix", &["nix", "build"]),
            ("Python", &[python, "-c", "import poetry"]),
        ] {
            assert_eq!(opens(name, argv), None, "{argv:?}");
            assert_eq!(tool(name, &record(argv)), None, "{argv:?}");
        }
        // A shell is no tool.
        assert_eq!(tool("bash", &record(&["bash"])), None);
    }

    #[test]
    fn xonsh_is_found_by_its_script() {
        let python = "/opt/homebrew/Cellar/python@3.13/Python";
        assert_eq!(
            opens("Python", &[python, "/opt/homebrew/bin/xonsh"]),
            Some("xonsh")
        );
        assert_eq!(
            opens("xonsh", &["/usr/bin/python3", "/usr/bin/xonsh", "--no-rc"]),
            Some("xonsh")
        );
        assert_eq!(
            opens("Python", &[python, "/opt/homebrew/bin/xonsh", "-c", "ls"]),
            None
        );
        assert_eq!(
            opens("Python", &[python, "/opt/homebrew/bin/xonsh", "x.xsh"]),
            None
        );
    }

    #[test]
    fn root_is_a_shells_name_and_user_zero() {
        // macOS: root's record does not read — the name and the user.
        assert!(is_root_shell("sh", Some(0), None));
        assert!(is_root_shell("zsh", Some(0), None));
        assert!(!is_root_shell("sh", Some(501), None));
        assert!(!is_root_shell("sh", None, None));
        assert!(!is_root_shell("python3", Some(0), None));
        assert!(!is_root_shell("sudo", Some(0), None));
        // Linux: its `cmdline` reads, and a script or a command is no
        // session (`sudo ./install.sh`, `sudo sh -c …`).
        assert!(is_root_shell("bash", Some(0), Some(&record(&["-bash"]))));
        assert!(!is_root_shell(
            "bash",
            Some(0),
            Some(&record(&["/bin/bash", "./install.sh"]))
        ));
        assert!(!is_root_shell(
            "sh",
            Some(0),
            Some(&record(&["sh", "-c", "make install"]))
        ));
    }

    #[test]
    fn the_bars_words_are_ascii() {
        assert!(NESTED.is_ascii());
        for shell in &SHELLS {
            assert!(shell.name.is_ascii(), "{}", shell.name);
        }
        for tool in &TOOLS {
            assert!(tool.title.is_ascii(), "{}", tool.title);
        }
    }
}
