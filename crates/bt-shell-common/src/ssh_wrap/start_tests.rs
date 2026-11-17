//! `start` and `wrap/claude`, the two files a program that was asked for is started through,
//! run for real with `sh` and stand-in programs: what they drop, what they wrap, what they run
//! and what is left in the terminal after them.

use std::path::PathBuf;
use std::process::{Command, Stdio};

use crate::child::tests::write_program;
use crate::settings::TempRoot;

use super::{CLAUDE_WRAPPER, START_SCRIPT};

/// The names an agent puts in the environment of what it starts that `start` drops when the one
/// who asked is Claude Code (what the `drop` line of `start` says, one by one).
const CLAUDE_NAMES: [&str; 14] = [
    "CLAUDECODE",
    "CLAUDE_CODE_CHILD_SESSION",
    "CLAUDE_CODE_ENTRYPOINT",
    "CLAUDE_CODE_EXECPATH",
    "CLAUDE_CODE_MESSAGING_SOCKET",
    "CLAUDE_CODE_MESSAGING_TOKEN",
    "CLAUDE_CODE_SESSION_ATTENDED",
    "CLAUDE_CODE_SESSION_ID",
    "CLAUDE_EFFORT",
    "CLAUDE_PID",
    "AI_AGENT",
    "GIT_EDITOR",
    "COREPACK_ENABLE_AUTO_PIN",
    "NoDefaultCurrentDirectoryInExePath",
];

/// A machine's folder of bateri's files, with `start` and `wrap/claude` written as the files
/// script writes them, and stand-in programs on a `PATH` of its own.
struct Machine {
    _root: TempRoot,
    home: PathBuf,
    shell: PathBuf,
    bin: PathBuf,
}

impl Machine {
    fn new(name: &str) -> Machine {
        let root = TempRoot::new(name);
        let home = std::fs::canonicalize(&root.0).expect("root").join("home");
        let shell = home.join(".local/share/bateri/shell");
        let bin = home.join("fakebin");
        for dir in [&shell.join("wrap"), &shell.join("agents"), &bin] {
            std::fs::create_dir_all(dir).expect("dir");
        }
        std::fs::write(shell.join("start"), START_SCRIPT).expect("start");
        std::fs::write(shell.join("wrap/claude"), CLAUDE_WRAPPER).expect("wrap/claude");
        Machine {
            _root: root,
            home,
            shell,
            bin,
        }
    }

    /// A stand-in program on the `PATH`.
    fn program(&self, name: &str, body: &str) -> PathBuf {
        let path = self.bin.join(name);
        write_program(&path, &format!("#!/bin/sh\n{body}"));
        path
    }

    /// `sh start KEEP WORDS...`, run in a clean environment with `vars` added: what it said, and
    /// the process it was (the stand-ins say theirs).
    fn start(&self, keep: &str, words: &[&str], vars: &[(&str, &str)]) -> (Vec<String>, u32) {
        let mut command = Command::new("sh");
        command
            .arg(self.shell.join("start"))
            .arg(keep)
            .args(words)
            .env_clear()
            .env("HOME", &self.home)
            .env("PATH", format!("{}:/usr/bin:/bin", self.bin.display()))
            .current_dir(&self.home)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (name, value) in vars {
            command.env(name, value);
        }
        let child = command.spawn().expect("sh");
        let pid = child.id();
        let output = child.wait_with_output().expect("start ended");
        let mut lines: Vec<String> = String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(str::to_owned)
            .collect();
        lines.extend(
            String::from_utf8_lossy(&output.stderr)
                .lines()
                .map(|line| format!("stderr: {line}")),
        );
        (lines, pid)
    }

    /// A program that says each name of `names` it is given, or that it is not there.
    fn says(&self, name: &str, names: &[&str]) -> PathBuf {
        let mut body = String::new();
        for var in names {
            body.push_str(&format!("printf '{var}=%s\\n' \"${{{var}-unset}}\"\n"));
        }
        self.program(name, &body)
    }
}

fn marks_set(value: &str) -> Vec<(&'static str, &str)> {
    CLAUDE_NAMES.iter().map(|name| (*name, value)).collect()
}

#[test]
fn what_an_agent_puts_in_the_environment_is_dropped_only_while_its_marker_is_set() {
    let machine = Machine::new("start-drops");
    let mut names = CLAUDE_NAMES.to_vec();
    names.extend(["CLAUDE_MINE", "AWS_PROFILE", "PATH_EXTRA"]);
    machine.says("probe", &names);
    let mut vars = marks_set("x");
    vars.extend([
        ("CLAUDE_MINE", "kept"),
        ("AWS_PROFILE", "work"),
        ("PATH_EXTRA", "kept"),
    ]);
    // Claude Code asked: its fourteen names go, and the person's own stay — a `CLAUDE_*` of
    // theirs, a profile, the rest of the environment.
    let (lines, _) = machine.start("0", &["probe"], &vars);
    let mut wanted: Vec<String> = CLAUDE_NAMES.iter().map(|n| format!("{n}=unset")).collect();
    wanted.extend([
        "CLAUDE_MINE=kept".to_owned(),
        "AWS_PROFILE=work".to_owned(),
        "PATH_EXTRA=kept".to_owned(),
    ]);
    assert_eq!(lines, wanted);
    // Somebody else asked (no `CLAUDECODE`): the program gets the environment it was asked in,
    // even a `GIT_EDITOR` of the person's own.
    let vars: Vec<_> = marks_set("x")
        .into_iter()
        .filter(|(name, _)| *name != "CLAUDECODE")
        .collect();
    let (lines, _) = machine.start("0", &["probe"], &vars);
    assert_eq!(lines[0], "CLAUDECODE=unset");
    assert_eq!(lines[11], "GIT_EDITOR=x");
    assert_eq!(lines[7], "CLAUDE_CODE_SESSION_ID=x");
    // A marker with nothing in it says nothing.
    let (lines, _) = machine.start("0", &["probe"], &[("CLAUDECODE", ""), ("GIT_EDITOR", "x")]);
    assert_eq!(lines[11], "GIT_EDITOR=x");
}

#[test]
fn a_name_or_a_program_of_bateris_folder_is_started_by_its_wrapper_and_no_other_word_is() {
    let machine = Machine::new("start-wraps");
    machine.program("tool", "printf 'ran:%s\\n' \"$*\"\n");
    let apps = machine.home.join(".local/share/bateri/apps/bin");
    std::fs::create_dir_all(apps.join("deeper")).expect("apps");
    for path in [apps.join("tool"), apps.join("deeper/tool")] {
        write_program(&path, "#!/bin/sh\nprintf 'ran:%s\\n' \"$*\"\n");
    }
    // The wrapper is given the word to run, whole, and ends by running it.
    std::fs::write(
        machine.shell.join("wrap/tool"),
        "printf 'wrapped:%s\\n' \"$*\"\nexec \"$@\"\n",
    )
    .expect("wrapper");
    // A `./tool` in the directory the program starts in (the home here).
    write_program(
        &machine.home.join("tool"),
        "#!/bin/sh\nprintf 'ran:%s\\n' \"$*\"\n",
    );
    let apps_tool = apps.join("tool").display().to_string();
    let deeper = apps.join("deeper/tool").display().to_string();
    let here = machine.bin.join("tool").display().to_string();
    let cases: Vec<(Vec<&str>, Vec<String>)> = vec![
        (
            vec!["tool", "a b"],
            vec!["wrapped:tool a b".into(), "ran:a b".into()],
        ),
        (
            vec![apps_tool.as_str(), "x"],
            vec![format!("wrapped:{apps_tool} x"), "ran:x".into()],
        ),
        // A path that is not in the folder of the programs is no program of that name.
        (vec!["./tool", "w"], vec!["ran:w".into()]),
        (vec![here.as_str(), "y"], vec!["ran:y".into()]),
        (vec![deeper.as_str()], vec!["ran:".into()]),
        // The line a server's click starts a program with is `sh`, which has no wrapper.
        (
            vec!["sh", "-c", "exec \"$0\" \"$@\"", here.as_str(), "z"],
            vec!["ran:z".into()],
        ),
    ];
    for (words, wanted) in cases {
        let (lines, _) = machine.start("0", &words, &[]);
        assert_eq!(lines, wanted, "{words:?}");
    }
    // A wrapper that is not there, or is a folder, wraps nothing.
    std::fs::remove_file(machine.shell.join("wrap/tool")).expect("removed");
    let (lines, _) = machine.start("0", &["tool", "q"], &[]);
    assert_eq!(lines, ["ran:q"]);
    std::fs::create_dir(machine.shell.join("wrap/tool")).expect("a folder");
    let (lines, _) = machine.start("0", &["tool", "q"], &[]);
    assert_eq!(lines, ["ran:q"]);
}

#[test]
fn a_terminal_kept_open_goes_on_with_the_login_shell_that_has_no_marks() {
    let machine = Machine::new("start-keeps");
    machine.says("probe", &["CLAUDECODE", "GIT_EDITOR", "CLAUDE_MINE"]);
    let login = machine.program(
        "login",
        "printf 'login:%s\\n' \"$*\"\nprintf 'CLAUDECODE=%s\\n' \"${CLAUDECODE-unset}\"\nprintf 'GIT_EDITOR=%s\\n' \"${GIT_EDITOR-unset}\"\n",
    );
    let vars = [
        ("CLAUDECODE", "1"),
        ("GIT_EDITOR", "true"),
        ("CLAUDE_MINE", "kept"),
        ("SHELL", login.to_str().expect("path")),
    ];
    // Kept open: the program, then the login shell, told it is one, with no marks.
    let (lines, _) = machine.start("1", &["probe"], &vars);
    assert_eq!(
        lines,
        [
            "CLAUDECODE=unset",
            "GIT_EDITOR=unset",
            "CLAUDE_MINE=kept",
            "login:-l",
            "CLAUDECODE=unset",
            "GIT_EDITOR=unset",
        ]
    );
    // Not kept open: the program, and that is all.
    let (lines, _) = machine.start("0", &["probe"], &vars);
    assert_eq!(
        lines,
        ["CLAUDECODE=unset", "GIT_EDITOR=unset", "CLAUDE_MINE=kept"]
    );
    // The same with a wrapper in between.
    std::fs::write(
        machine.shell.join("wrap/probe"),
        "echo wrapped\nexec \"$@\"\n",
    )
    .expect("wrapper");
    let (lines, _) = machine.start("1", &["probe"], &vars);
    assert_eq!(lines[0], "wrapped");
    assert_eq!(lines.last().map(String::as_str), Some("GIT_EDITOR=unset"));
    assert!(lines.contains(&"login:-l".to_owned()), "{lines:?}");
}

#[test]
fn a_kept_terminal_survives_the_ctrl_c_that_ends_its_program_and_runs_no_builtin_for_it() {
    use std::os::unix::process::CommandExt as _;
    let machine = Machine::new("start-keeps-ctrl-c");
    let login = machine.program("login", "echo login-shell\n");
    // The program is in the group of the terminal's foreground, as a program in a pane is: the
    // Ctrl-C of the terminal is a signal to every process of it, this one and `start` too.
    machine.program(
        "interrupted",
        "echo before\nkill -INT 0\nsleep 5\necho not-reached\n",
    );
    let run = |words: &[&str]| -> String {
        let output = Command::new("sh")
            .arg(machine.shell.join("start"))
            .arg("1")
            .args(words)
            .process_group(0)
            .env_clear()
            .env("HOME", &machine.home)
            .env("PATH", format!("{}:/usr/bin:/bin", machine.bin.display()))
            .env("SHELL", &login)
            .stdin(Stdio::null())
            .output()
            .expect("sh");
        String::from_utf8_lossy(&output.stdout).into_owned()
    };
    let said = run(&["interrupted"]);
    assert_eq!(
        said, "before\nlogin-shell\n",
        "the program was not left to end alone"
    );
    // A word that is a builtin of `sh` is the program of that name, not the builtin: `exit` here
    // would end `start` and skip the login shell.
    machine.program("exit", "echo the-program-exit\n");
    assert_eq!(run(&["exit", "3"]), "the-program-exit\nlogin-shell\n");
}

/// A `claude` that says what it was started with and the process it is.
fn claude_stand_in(machine: &Machine) {
    machine.program(
        "claude",
        "[ \"$1\" = --help ] && { echo asked >> \"$HOME/help-asked\"; cat \"$HOME/help.txt\" 2>/dev/null; exit 0; }\nprintf 'PID=%s\\n' \"$$\"\nprintf 'N=%s\\n' \"$#\"\nfor a in \"$@\"; do printf '<%s>\\n' \"$a\"; done\nprintf 'CLAUDECODE=%s\\n' \"${CLAUDECODE-unset}\"\n",
    );
}

#[test]
fn claude_asked_for_by_a_program_start_is_the_process_start_was_and_has_no_marks() {
    let machine = Machine::new("start-claude");
    claude_stand_in(&machine);
    let hook = "{\"hooks\":{}}";
    std::fs::write(machine.shell.join("agents/claude.json"), hook).expect("hook");
    // `bateri split -- claude` from a Claude Code: its marker goes, the hook is added, and the
    // chain from `start` through the wrapper to the program is one process.
    let (lines, pid) = machine.start(
        "0",
        &["claude", "one", "two words"],
        &[("CLAUDECODE", "1"), ("CLAUDE_CODE_CHILD_SESSION", "1")],
    );
    assert_eq!(
        lines,
        [
            format!("PID={pid}"),
            "N=4".to_owned(),
            "<--settings>".to_owned(),
            format!("<{hook}>"),
            "<one>".to_owned(),
            "<two words>".to_owned(),
            "CLAUDECODE=unset".to_owned(),
        ]
    );
    // `./claude` is not Claude: no hook.
    machine.program("claude", "printf 'N=%s\\n' \"$#\"\n");
    let (lines, _) = machine.start("0", &["./fakebin/claude", "one"], &[]);
    assert_eq!(lines, ["N=1"]);
    // No hook file: the program as it was asked for.
    claude_stand_in(&machine);
    std::fs::remove_file(machine.shell.join("agents/claude.json")).expect("removed");
    let (lines, pid) = machine.start("0", &["claude", "one"], &[]);
    assert_eq!(
        lines,
        [
            format!("PID={pid}"),
            "N=1".to_owned(),
            "<one>".to_owned(),
            "CLAUDECODE=unset".to_owned()
        ]
    );
}

#[test]
fn the_wrapper_of_claude_adds_the_hook_unless_the_user_said_their_own_or_claude_code_started_it() {
    let machine = Machine::new("wrap-claude");
    claude_stand_in(&machine);
    let hook = "{\"hooks\":{}}";
    std::fs::write(machine.shell.join("agents/claude.json"), hook).expect("hook");
    let wrapper = machine.shell.join("wrap/claude");
    let run = |words: &[&str], claudecode: bool| -> Vec<String> {
        let mut command = Command::new("sh");
        command
            .arg(&wrapper)
            .args(words)
            .env_clear()
            .env("HOME", &machine.home)
            .env("PATH", format!("{}:/usr/bin:/bin", machine.bin.display()));
        if claudecode {
            command.env("CLAUDECODE", "1");
        }
        let output = command.output().expect("sh");
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .skip(1)
            .map(str::to_owned)
            .collect()
    };
    let hooked = |rest: &[&str]| -> Vec<String> {
        let mut lines = vec![
            format!("N={}", rest.len() + 2),
            "<--settings>".to_owned(),
            format!("<{hook}>"),
        ];
        lines.extend(rest.iter().map(|word| format!("<{word}>")));
        lines.push("CLAUDECODE=unset".to_owned());
        lines
    };
    assert_eq!(run(&["claude", "one"], false), hooked(&["one"]));
    // The user's own `--settings`, either way of writing it: theirs alone.
    assert_eq!(
        run(&["claude", "--settings", "mine", "one"], false),
        ["N=3", "<--settings>", "<mine>", "<one>", "CLAUDECODE=unset"]
    );
    assert_eq!(
        run(&["claude", "--settings=mine"], false),
        ["N=1", "<--settings=mine>", "CLAUDECODE=unset"]
    );
    // After `--` it is no flag of Claude Code's.
    assert_eq!(
        run(&["claude", "--", "--settings", "x"], false),
        hooked(&["--", "--settings", "x"])
    );
    // Started by Claude Code itself: as it was typed.
    assert_eq!(
        run(&["claude", "-p", "hi"], true),
        ["N=2", "<-p>", "<hi>", "CLAUDECODE=1"]
    );
    // No hook file: as it was typed.
    std::fs::remove_file(machine.shell.join("agents/claude.json")).expect("removed");
    assert_eq!(
        run(&["claude", "one"], false),
        ["N=1", "<one>", "CLAUDECODE=unset"]
    );
}

#[test]
fn the_files_are_text_that_sh_reads_and_nothing_in_them_writes_a_startup_file() {
    for (name, text) in [("start", START_SCRIPT), ("wrap/claude", CLAUDE_WRAPPER)] {
        let checked = Command::new("sh")
            .arg("-n")
            .arg("-c")
            .arg(text)
            .status()
            .expect("sh");
        assert!(checked.success(), "{name} is not valid sh");
        for rc in [".zshrc", ".bashrc", ".profile", "config.fish"] {
            assert!(!text.contains(rc), "{name} names {rc}");
        }
    }
}

const HELP_WITH_THE_FLAG: &str = "Usage: claude [options]\n\nOptions:\n  --settings <file>  Load settings. Not used with --plugin-dir here.\n  --plugin-dir <path>  Load a plugin from a directory\n";
/// A help that only names the flag: in another option's line, and in the line that goes on with one
/// (indented far in, as the real help does it).
const HELP_WITHOUT_THE_FLAG: &str = "Usage: claude [options]\n\nOptions:\n  --settings <file>  Load settings, like --plugin-dir does not.\n  --add-dir <path>  A folder (a plugin has its own:\n                                        --plugin-dir A --plugin-dir B.zip)\n  --model <name>  The model\n";

#[test]
fn the_wrapper_of_claude_gives_the_plugin_only_where_the_skill_is_and_claude_knows_the_flag() {
    let machine = Machine::new("wrap-claude-plugin");
    claude_stand_in(&machine);
    let hook = "{\"hooks\":{}}";
    std::fs::write(machine.shell.join("agents/claude.json"), hook).expect("the hook");
    let skill = machine
        .shell
        .join("agents/claude-plugin/skills/bateri/SKILL.md");
    let help = machine.home.join("help.txt");
    let folder = format!("{}/wrap/../agents/claude-plugin", machine.shell.display());
    let wrapper = machine.shell.join("wrap/claude");
    let run = |words: &[&str], claudecode: bool| -> Vec<String> {
        let mut command = Command::new("sh");
        command
            .arg(&wrapper)
            .args(words)
            .env_clear()
            .env("HOME", &machine.home)
            .env("PATH", format!("{}:/usr/bin:/bin", machine.bin.display()));
        if claudecode {
            command.env("CLAUDECODE", "1");
        }
        let output = command.output().expect("sh");
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .skip(1)
            .map(str::to_owned)
            .collect()
    };
    let with_plugin = |rest: &[&str]| -> Vec<String> {
        let mut lines = vec![
            format!("N={}", rest.len() + 2),
            "<--plugin-dir>".to_owned(),
            format!("<{folder}>"),
        ];
        lines.extend(rest.iter().map(|word| format!("<{word}>")));
        lines.push("CLAUDECODE=unset".to_owned());
        lines
    };
    let hooked_with_plugin = [
        "N=5".to_owned(),
        "<--plugin-dir>".to_owned(),
        format!("<{folder}>"),
        "<--settings>".to_owned(),
        format!("<{hook}>"),
        "<one>".to_owned(),
        "CLAUDECODE=unset".to_owned(),
    ];
    let hooked = [
        "N=3",
        "<--settings>",
        &format!("<{hook}>"),
        "<one>",
        "CLAUDECODE=unset",
    ]
    .map(str::to_owned);
    // The skill is not there: no flag, and `claude --help` is not even asked.
    let asked = machine.home.join("help-asked");
    std::fs::write(&help, HELP_WITH_THE_FLAG).expect("help");
    assert_eq!(run(&["claude", "one"], false), hooked);
    assert!(!asked.exists(), "the help was asked without the skill");
    // The skill is there and this Claude Code lists the flag: both, the plugin first.
    std::fs::create_dir_all(skill.parent().expect("dir")).expect("dir");
    std::fs::write(&skill, "---\nname: bateri\n---\n").expect("skill");
    assert_eq!(run(&["claude", "one"], false), hooked_with_plugin);
    // The user's own `--settings` does not keep the plugin from it.
    let mut theirs = with_plugin(&["--settings", "mine", "one"]);
    theirs[0] = "N=5".to_owned();
    assert_eq!(run(&["claude", "--settings", "mine", "one"], false), theirs);
    // A Claude Code that does not list the flag (or only names it in another option's line) is
    // not given it.
    std::fs::write(&help, HELP_WITHOUT_THE_FLAG).expect("help");
    assert_eq!(run(&["claude", "one"], false), hooked);
    std::fs::write(&help, "").expect("help");
    assert_eq!(run(&["claude", "one"], false), hooked);
    std::fs::remove_file(&help).expect("help");
    assert_eq!(run(&["claude", "one"], false), hooked);
    // Claude Code's own shell: as it was typed, and the help is not asked.
    std::fs::write(&help, HELP_WITH_THE_FLAG).expect("help");
    std::fs::remove_file(&asked).ok();
    assert_eq!(
        run(&["claude", "-p", "hi"], true),
        ["N=2", "<-p>", "<hi>", "CLAUDECODE=1"]
    );
    assert!(!asked.exists(), "the help was asked inside Claude Code");
    // Without the hook's file the skill still goes.
    std::fs::remove_file(machine.shell.join("agents/claude.json")).expect("removed");
    assert_eq!(run(&["claude", "one"], false), with_plugin(&["one"]));
    // The skill taken away: the same call without it.
    std::fs::remove_file(&skill).expect("removed");
    assert_eq!(
        run(&["claude", "one"], false),
        ["N=1", "<one>", "CLAUDECODE=unset"]
    );
}

#[test]
fn the_wrapper_names_the_folder_and_the_file_of_the_skill_the_table_names() {
    for row in crate::program::reply::REPLIERS {
        assert!(
            CLAUDE_WRAPPER.contains(&format!("agents/{}", row.skill_dir)),
            "the wrapper does not name {}",
            row.skill_dir
        );
        assert!(
            CLAUDE_WRAPPER.contains(row.skill_file),
            "the wrapper does not name {}",
            row.skill_file
        );
        assert!(
            row.skill.iter().any(|(path, _)| *path == row.skill_file),
            "{} is not a file of the skill",
            row.skill_file
        );
        assert!(
            CLAUDE_WRAPPER.contains(&format!("agents/{}", row.file)),
            "the wrapper does not name {}",
            row.file
        );
    }
}
