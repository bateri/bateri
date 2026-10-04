//! Which program reads the keyboard itself, and what its guide bar says.
//!
//! While a command runs with the PTY raw (`bt_core::Session::note_raw`) the
//! dock steps aside; a program recognized here keeps the band's context row
//! for a one-line guide (`bt_core::ProgramBar`) — which interpreter, what
//! manages it, where it runs from and how to leave:
//! `Python 3.14.5 · venv  ~/proj/.venv/bin/python3   ⌃D exit`. Anything
//! else — an agent, a TUI, a script — gets no bar and no band.
//!
//! **Recognized by its command line, not its name.** An interpreter is a
//! REPL only when it runs no code it was given: `node` alone is one, `node
//! codex.js` is not — the coding agents shipped as node or python scripts
//! (codex's npm wrapper, Claude Code's npm build, gemini, aider) run under
//! the same names, and a "Node" bar over an agent would be silently wrong.
//! Front-ends (`ipython`, `irb`, `bun repl`) are named by their script or
//! subcommand.
//!
//! **Three halves.** The decision is pure ([`classify`], whose input is the
//! foreground group's records), [`find`] walks the process table for it on
//! the main thread — system calls only — and [`details`] is the background
//! job's: the interpreter's own `--version` and a virtual environment's
//! `pyvenv.cfg`. The bar is first written with what the table knows and
//! completed when the job returns.
//!
//! **The environment is read through a fixed list** ([`ENV_KEYS`],
//! `jobs::ProcArgs`): a program that reads the keyboard often carries API
//! keys in its environment, and nothing but the listed variables is ever
//! copied out of a process's record.
//!
//! **Database clients** (`psql`, `mysql`, `mariadb`, `sqlite3`,
//! `redis-cli`, `mongosh`) are a family too: their bar names what the
//! client is connected to — `postgres  app@db.prod:5432/main` — from its
//! command line ([`database`]), never its password, and its server's host is
//! what the `[remote] hosts` marks color the bar by.

mod database;

pub use database::{Client, Target};

use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use bt_core::{ProgramBar, ProgramTone};

use crate::jobs::{self, ProcArgs, ProcessTable, ShellParent};

/// The environment variables a candidate's record carries — **the whole
/// list**; no other variable of any process is read. Each has a single
/// use: the framework Python's launcher path, the managers' roots, and the
/// database clients' server, address, port, user, database and libpq
/// service. **No
/// password variable** (`PGPASSWORD`, `MYSQL_PWD`, `REDISCLI_AUTH`) is
/// here, so none is ever read out of a process.
pub const ENV_KEYS: [&str; 15] = [
    "__PYVENV_LAUNCHER__",
    "VIRTUAL_ENV",
    "CONDA_PREFIX",
    "CONDA_DEFAULT_ENV",
    "NVM_DIR",
    "VOLTA_HOME",
    "PYENV_ROOT",
    "PGHOST",
    "PGHOSTADDR",
    "PGPORT",
    "PGUSER",
    "PGDATABASE",
    "PGSERVICE",
    "MYSQL_HOST",
    "MYSQL_TCP_PORT",
];

/// How long the interpreter's `--version` may take before it is killed — a
/// **design constant**, not a measurement: well above an interpreter's start
/// (it loads no module and runs no user code), well below a wait anyone
/// would notice on a bar that is already up without its version.
pub const VERSION_TIMEOUT: Duration = Duration::from_secs(2);

/// The longest `--version` output that is read; past it the answer is not
/// a version line.
const VERSION_OUTPUT_MAX: u64 = 4096;

/// The longest `pyvenv.cfg` that is read — the file is a few lines.
const VENV_CONFIG_MAX: u64 = 64 * 1024;

/// A program the guide bar knows: an interpreter family with a REPL (a row
/// of [`INTERPRETERS`]) or a database client.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    Python,
    Node,
    Bun,
    Deno,
    Ruby,
    Database(Client),
}

/// One interpreter: the family, the bar's label and its exit hint. A new
/// language is a row here and an arm in [`is_repl`].
struct Interpreter {
    family: Family,
    /// The title's name, before the version — a UI string.
    label: &'static str,
    /// How to leave its REPL — a UI string; its non-ASCII characters are in
    /// `bt_core::PROGRAM_GLYPHS`.
    hint: &'static str,
}

const INTERPRETERS: [Interpreter; 5] = [
    Interpreter {
        family: Family::Python,
        label: "Python",
        hint: "⌃D exit",
    },
    Interpreter {
        family: Family::Node,
        label: "Node",
        hint: "⌃D exit",
    },
    Interpreter {
        family: Family::Bun,
        label: "Bun",
        hint: "⌃D exit",
    },
    Interpreter {
        family: Family::Deno,
        label: "Deno",
        hint: "⌃D exit",
    },
    Interpreter {
        family: Family::Ruby,
        label: "Ruby",
        hint: "⌃D exit",
    },
];

/// Python front-ends: run as a script by the interpreter
/// (`python /usr/local/bin/ipython`), a REPL all the same.
const PYTHON_FRONTENDS: [&str; 5] = ["ipython", "ipython3", "bpython", "ptpython", "ptipython"];

/// The modules `python -m` runs as a REPL.
const PYTHON_REPL_MODULES: [&str; 3] = ["IPython", "bpython", "ptpython"];

/// Ruby's REPLs: a script run by `ruby`, which rewrites its own argv to the
/// script's name (measured: `irb` shows as `['irb', '']`).
const RUBY_REPLS: [&str; 2] = ["irb", "pry"];

/// Home-relative roots of the version managers' installs: an interpreter
/// under one is that manager's. A row per layout; fnm has three.
const HOME_MANAGERS: [(&str, &str); 10] = [
    (".pyenv/versions", "pyenv"),
    (".local/share/uv/python", "uv"),
    (".nvm/versions", "nvm"),
    (".volta", "volta"),
    (".local/share/fnm", "fnm"),
    (".local/state/fnm_multishells", "fnm"),
    ("Library/Application Support/fnm", "fnm"),
    (".asdf/installs", "asdf"),
    (".rbenv/versions", "rbenv"),
    (".local/share/mise/installs", "mise"),
];

/// Managers whose root the user can move with a variable: an interpreter
/// under the variable's directory is that manager's.
const ENV_MANAGERS: [(&str, &str); 3] = [
    ("NVM_DIR", "nvm"),
    ("VOLTA_HOME", "volta"),
    ("PYENV_ROOT", "pyenv"),
];

impl Family {
    /// The family a process name can be — the candidate filter before a
    /// record is read (`ipython` and `irb` are names on Linux, where a
    /// script's process is named after the script). Python's names are
    /// `python` and its versioned forms (`python3`, `python3.14`) only: a
    /// tool that merely starts with the word (`python-lsp-server`) is not an
    /// interpreter, and its executable must not be run for a version.
    fn of_name(name: &str) -> Option<Self> {
        if let Some(client) = Client::of_name(name) {
            return Some(Self::Database(client));
        }
        let versioned = name
            .strip_prefix("python")
            .is_some_and(|rest| rest.chars().all(|ch| ch.is_ascii_digit() || ch == '.'));
        if name == "Python" || versioned || PYTHON_FRONTENDS.contains(&name) {
            return Some(Self::Python);
        }
        match name {
            "node" => Some(Self::Node),
            "bun" => Some(Self::Bun),
            "deno" => Some(Self::Deno),
            "ruby" | "irb" | "pry" => Some(Self::Ruby),
            _ => None,
        }
    }

    /// The bar's title, before a version.
    fn label(self) -> &'static str {
        match self {
            Self::Database(client) => client.label(),
            interpreter => interpreter.row().map_or("", |row| row.label),
        }
    }

    /// How to leave.
    fn hint(self) -> &'static str {
        match self {
            Self::Database(client) => client.hint(),
            interpreter => interpreter.row().map_or("", |row| row.hint),
        }
    }

    /// An interpreter's row; `None` for a database client (its own
    /// `label`/`hint`). Every interpreter has one —
    /// `the_bars_strings_are_the_ones_the_atlas_checks` walks them.
    fn row(self) -> Option<&'static Interpreter> {
        INTERPRETERS.iter().find(|row| row.family == self)
    }
}

/// A recognized program — what the process table says, on the main thread.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Program {
    /// The recognized process: its exit ends the bar ([`wait_for_exit`]),
    /// even while the command goes on (`python3; make`).
    pub pid: u32,
    pub family: Family,
    /// The interpreter asked for its version: the process's own executable,
    /// not a front-end script (`ipython --version` would run `site`).
    /// `None` when the executable's path is unknown.
    pub exec: Option<PathBuf>,
    /// Where it runs from, as shown: the path the user ran (the framework
    /// Python's launcher, a venv's `bin/python`), `~`-shortened; for a
    /// database client what it is connected to (`app@db.prod:5432/main`,
    /// SQLite's file). Empty when unknown.
    pub path: String,
    /// The detail after the title: what manages an interpreter, from the
    /// environment and the path (`venv`, `nvm`, `conda base`), or a database
    /// client's libpq service (`service prod`); `None` when nothing says.
    pub detail: Option<String>,
    /// The server's host the `[remote] hosts` marks are resolved against
    /// (a database client's); `None` for an interpreter, a socket or a
    /// file.
    pub host: Option<String>,
    /// A virtual environment's configuration to look for in the background:
    /// a Python run from `{dir}/bin/python` that nothing else names is a
    /// venv's when `{dir}/pyvenv.cfg` is one (an unactivated venv).
    pub venv_config: Option<PathBuf>,
}

/// What the background job adds ([`details`]).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Details {
    /// The interpreter's version as its `--version` says it (`3.14.5`,
    /// `v22.13.0`).
    pub version: Option<String>,
    /// [`Program::venv_config`] is a virtual environment's.
    pub venv: bool,
}

impl Program {
    /// The guide bar: what the table knew, completed by `details` when the
    /// background job has returned.
    pub fn bar(&self, details: Option<&Details>) -> ProgramBar {
        let label = self.family.label();
        let title = match details.and_then(|details| details.version.as_deref()) {
            Some(version) => format!("{label} {version}"),
            None => label.to_owned(),
        };
        let detail = self
            .detail
            .clone()
            .or_else(|| {
                details
                    .filter(|details| details.venv)
                    .map(|_| "venv".to_owned())
            })
            .unwrap_or_default();
        ProgramBar {
            title,
            detail,
            path: self.path.clone(),
            hint: self.family.hint().to_owned(),
            host: self.host.clone().unwrap_or_default(),
            tone: ProgramTone::Info,
        }
    }
}

/// A member of the foreground group: its pid, its parent and — for a
/// candidate name only — its name and record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Member {
    pub pid: u32,
    pub parent: Option<u32>,
    /// `None` for a name that is no interpreter's, or an unreadable record.
    pub candidate: Option<(String, ProcArgs)>,
}

/// The program in the terminal's foreground group, if one is recognized:
/// every member with its parent, records read only for candidate names
/// ([`ENV_KEYS`] only). `None` when the shell's own group holds the terminal
/// or the table is unreadable. Main thread: system calls only.
pub fn find(
    parent: ShellParent,
    child: u32,
    table: &impl ProcessTable,
    home: Option<&Path>,
) -> Option<Program> {
    let shell = jobs::shell_pid(parent, child, table)?;
    let groups = table
        .groups(shell)
        .filter(|groups| groups.foreground != 0 && groups.foreground != groups.own)?;
    let mut pids = table.members(groups.foreground);
    pids.sort_unstable();
    let members: Vec<Member> = pids
        .into_iter()
        .map(|pid| Member {
            pid,
            parent: table.parent(pid),
            candidate: table
                .name(pid)
                .filter(|name| Family::of_name(name).is_some())
                .and_then(|name| Some((name, table.procargs(pid, &ENV_KEYS)?))),
        })
        .collect();
    classify(&members, home)
}

/// The group's REPL, when the whole group is its line: the **topmost**
/// recognized member (one with no recognized ancestor in the group — pids
/// wrap, so the lower pid is not the parent), and every other member must
/// be its ancestor (a wrapper: `uv run python`) or its descendant (`bun
/// repl`'s script, a REPL's own children). A member on another branch is a
/// pipeline's other side — in `fzf | python3` fzf turned the terminal raw
/// and python3 reads a pipe — and the bar would name the wrong program.
pub fn classify(members: &[Member], home: Option<&Path>) -> Option<Program> {
    let recognized: Vec<Program> = members
        .iter()
        .filter_map(|member| {
            let (name, record) = member.candidate.as_ref()?;
            recognize(member.pid, name, record, home)
        })
        .collect();
    let parent_of = |pid: u32| {
        members
            .iter()
            .find(|member| member.pid == pid)
            .and_then(|member| member.parent)
    };
    // `pid`'s ancestors inside the group, nearest first; the walk is bounded
    // by the group's size and stops at a cycle — a corrupt table (a process
    // its own parent) must neither loop nor make a process its own ancestor.
    let ancestors = |pid: u32| {
        let mut up = Vec::new();
        let mut at = pid;
        for _ in 0..members.len() {
            match parent_of(at) {
                Some(next)
                    if next != pid
                        && !up.contains(&next)
                        && members.iter().any(|member| member.pid == next) =>
                {
                    up.push(next);
                    at = next;
                }
                _ => break,
            }
        }
        up
    };
    let pids: Vec<u32> = recognized.iter().map(|program| program.pid).collect();
    let program = recognized
        .into_iter()
        .find(|program| !ancestors(program.pid).iter().any(|up| pids.contains(up)))?;
    let up = ancestors(program.pid);
    let in_line = members.iter().all(|member| {
        member.pid == program.pid
            || up.contains(&member.pid)
            || ancestors(member.pid).contains(&program.pid)
    });
    in_line.then_some(program)
}

/// One member: its family, whether it is a REPL, then the bar's parts.
fn recognize(pid: u32, name: &str, record: &ProcArgs, home: Option<&Path>) -> Option<Program> {
    let family = Family::of_name(name)?;
    let args = record.args.get(1..).unwrap_or_default();
    if let Family::Database(client) = family {
        // On Linux a script run by its own shebang (`#!/usr/bin/node`) is
        // named after the script while its argv is the interpreter's —
        // `/usr/bin/node /usr/bin/mongosh …`: the client's arguments come
        // after the script.
        let interpreted = record
            .args
            .first()
            .is_some_and(|zero| matches!(base(zero), "node" | "nodejs"));
        let args = if interpreted {
            node_script(args).map_or(args, |(_, rest)| rest)
        } else {
            args
        };
        return client_program(pid, client, args, record, home);
    }
    // A client shipped as a node script (Homebrew's `mongosh`).
    if family == Family::Node
        && let Some((script, rest)) = node_script(args)
        && let Some(client) = Client::of_script(base(script))
    {
        return client_program(pid, client, rest, record, home);
    }
    if !is_repl(family, &record.args) {
        return None;
    }
    let exec = Some(PathBuf::from(&record.exec)).filter(|exec| exec.is_absolute());
    // The path the user ran: the framework Python re-executes itself from
    // inside its bundle and keeps the launcher (`/opt/homebrew/bin/python3`,
    // a venv's `bin/python`) in this variable — for Python only, since the
    // variable is inherited by whatever Python starts.
    let launcher = match family {
        Family::Python => record
            .var("__PYVENV_LAUNCHER__")
            .map(PathBuf::from)
            .filter(|launcher| launcher.is_absolute()),
        _ => None,
    };
    let shown = launcher.or_else(|| exec.clone());
    let manager = manager(family, record, shown.as_deref(), exec.as_deref(), home);
    let venv_config = match (family, &manager, &shown) {
        (Family::Python, None, Some(shown)) => venv_config(shown),
        _ => None,
    };
    Some(Program {
        pid,
        family,
        exec,
        path: shown
            .as_deref()
            .map(|path| tilde(path, home))
            .unwrap_or_default(),
        detail: manager,
        host: None,
        venv_config,
    })
}

/// A database client's program: what it is connected to ([`Target`]), its
/// libpq service as the detail and its server's host for the marks. No
/// executable to ask for a version — the client's name is the title. `None`
/// when the command line is not the client's prompt.
fn client_program(
    pid: u32,
    client: Client,
    args: &[String],
    record: &ProcArgs,
    home: Option<&Path>,
) -> Option<Program> {
    let target = client.target(args, record)?;
    Some(Program {
        pid,
        family: Family::Database(client),
        exec: None,
        path: target.shown(home),
        detail: target
            .service
            .as_ref()
            .map(|service| format!("service {service}")),
        host: target.mark_host(),
        venv_config: None,
    })
}

/// The script node runs and the arguments after it, skipping node's own
/// options ([`NODE_VALUED`] take a value); `None` when node runs no script
/// — none given, or code given inline (`-e`, `-p`), after which the next
/// argument is that code's, not a script.
fn node_script(args: &[String]) -> Option<(&str, &[String])> {
    let mut index = 0;
    while let Some(arg) = args.get(index) {
        if arg == "--" {
            let script = args.get(index + 1)?;
            return Some((script, args.get(index + 2..).unwrap_or_default()));
        }
        if !arg.starts_with('-') {
            return Some((arg, args.get(index + 1..).unwrap_or_default()));
        }
        let name = arg.split_once('=').map_or(arg.as_str(), |(name, _)| name);
        if NODE_CODE.contains(&name) {
            return None;
        }
        index += 1 + usize::from(NODE_VALUED.contains(&name) && !arg.contains('='));
    }
    None
}

/// Whether the command line starts the interpreter's REPL rather than code
/// it was given (a script, `-c`, `-e`, a subcommand).
fn is_repl(family: Family, argv: &[String]) -> bool {
    let args = argv.get(1..).unwrap_or_default();
    match family {
        Family::Python => python_is_repl(args),
        Family::Node => node_is_repl(args),
        Family::Bun => first_positional(args, &[]) == Some("repl"),
        Family::Deno => matches!(first_positional(args, &[]), None | Some("repl")),
        Family::Ruby => {
            let named = |arg: &str| RUBY_REPLS.contains(&base(arg));
            argv.first().is_some_and(|zero| named(zero))
                || first_positional(args, &["-I", "-r", "-C", "-E"]).is_some_and(named)
        }
        // A client's prompt is decided with its target (`client_program`).
        Family::Database(_) => false,
    }
}

/// Python's options: `-c` and `-m` end them (the rest is the code's
/// argv), `-W`/`-X` and `--check-hash-based-pycs` take a value, the others
/// are flags that may be clustered (`-iu`). A REPL when nothing runs, when
/// `-i` asks for one after the code, or when the script or module is a
/// front-end that is given nothing to run itself ([`runs_nothing`]).
fn python_is_repl(args: &[String]) -> bool {
    let mut interactive = false;
    let mut index = 0;
    while let Some(arg) = args.get(index) {
        if arg == "--" {
            let rest = args.get(index + 2..).unwrap_or_default();
            return interactive
                || args
                    .get(index + 1)
                    .is_none_or(|script| frontend(script, rest));
        }
        if arg == "-" || !arg.starts_with('-') {
            return interactive || frontend(arg, args.get(index + 1..).unwrap_or_default());
        }
        if let Some(long) = arg.strip_prefix("--") {
            index += 1 + usize::from(long == "check-hash-based-pycs");
            continue;
        }
        let flags = &arg[1..];
        for (at, flag) in flags.char_indices() {
            let rest = &flags[at + flag.len_utf8()..];
            match flag {
                'i' => interactive = true,
                'c' => return interactive,
                'm' => {
                    let (module, after) = if rest.is_empty() {
                        (args.get(index + 1).map(String::as_str), index + 2)
                    } else {
                        (Some(rest), index + 1)
                    };
                    return interactive
                        || (module.is_some_and(|module| PYTHON_REPL_MODULES.contains(&module))
                            && runs_nothing(args.get(after..).unwrap_or_default()));
                }
                'W' | 'X' => {
                    index += usize::from(rest.is_empty());
                    break;
                }
                _ => {}
            }
        }
        index += 1;
    }
    true
}

/// A Python script that is a REPL front-end, given `rest` as its arguments.
fn frontend(script: &str, rest: &[String]) -> bool {
    PYTHON_FRONTENDS.contains(&base(script)) && runs_nothing(rest)
}

/// A front-end's arguments leave it a REPL: options only, or `-i` (the
/// REPL after a script). `ipython train.py` runs a script — a positional
/// argument, even an option's value this does not know, means no bar.
fn runs_nothing(rest: &[String]) -> bool {
    rest.iter().all(|arg| arg.starts_with('-')) || rest.iter().any(|arg| arg == "-i")
}

/// Node's options: `-e`/`--eval`/`-p`/`--print` run code, a positional is a
/// script, `-` reads one from stdin; the listed options take a value. A
/// REPL when nothing runs or `-i` asks for one over code given inline. An
/// option this list does not know that takes a separate value makes its
/// value look like a script — no bar, the safe direction.
fn node_is_repl(args: &[String]) -> bool {
    let mut interactive = false;
    let mut code = false;
    let mut index = 0;
    while let Some(arg) = args.get(index) {
        let name = arg.split_once('=').map_or(arg.as_str(), |(name, _)| name);
        match name {
            "-i" | "--interactive" => interactive = true,
            _ if NODE_CODE.contains(&name) => {
                code = true;
                index += usize::from(!arg.contains('='));
            }
            "--" | "-" => return false,
            _ if NODE_VALUED.contains(&name) => index += usize::from(!arg.contains('=')),
            _ if !arg.starts_with('-') => return false,
            _ => {}
        }
        index += 1;
    }
    !code || interactive
}

/// Node's options that take a value (besides the code's).
const NODE_VALUED: [&str; 7] = [
    "-r",
    "--require",
    "--import",
    "--loader",
    "--experimental-loader",
    "-C",
    "--conditions",
];

/// Node's options that run the code given as their value.
const NODE_CODE: [&str; 4] = ["-e", "--eval", "-p", "--print"];

/// The first argument that is not an option, skipping the value of each
/// option in `valued`; `None` when there is none.
fn first_positional<'a>(args: &'a [String], valued: &[&str]) -> Option<&'a str> {
    let mut index = 0;
    while let Some(arg) = args.get(index) {
        if !arg.starts_with('-') {
            return Some(arg);
        }
        index += 1 + usize::from(valued.contains(&arg.as_str()));
    }
    None
}

/// A path's last component.
fn base(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// What manages the interpreter at `shown` (the path the user ran) or
/// `exec` (the executable): a conda environment or a venv **containing** it
/// (not merely active — an activated venv does not make `/usr/bin/python3`
/// its own), else a version manager's root, from its variable or its
/// default place under `home`.
fn manager(
    family: Family,
    record: &ProcArgs,
    shown: Option<&Path>,
    exec: Option<&Path>,
    home: Option<&Path>,
) -> Option<String> {
    let under = |root: &Path| {
        root.is_absolute()
            && root.components().count() > 1
            && [shown, exec]
                .into_iter()
                .flatten()
                .any(|path| path.starts_with(root))
    };
    if family == Family::Python {
        if let Some(prefix) = record
            .var("CONDA_PREFIX")
            .map(Path::new)
            .filter(|&prefix| under(prefix))
        {
            // The base prefix holds every environment under `envs/`: an
            // environment's interpreter run while `base` is active is that
            // environment's, not `base`'s.
            let named = [shown, exec].into_iter().flatten().find_map(|path| {
                let mut rest = path.strip_prefix(prefix).ok()?.components();
                (rest.next()?.as_os_str() == "envs")
                    .then(|| rest.next())
                    .flatten()
                    .map(|name| name.as_os_str().to_string_lossy().into_owned())
            });
            let name = named.or_else(|| {
                record
                    .var("CONDA_DEFAULT_ENV")
                    .filter(|name| !name.is_empty())
                    .map(str::to_owned)
            });
            return Some(name.map_or_else(|| "conda".to_owned(), |name| format!("conda {name}")));
        }
        if record
            .var("VIRTUAL_ENV")
            .is_some_and(|root| under(Path::new(root)))
        {
            return Some("venv".to_owned());
        }
    }
    let from_env = ENV_MANAGERS.iter().find_map(|&(key, label)| {
        record
            .var(key)
            .is_some_and(|root| under(Path::new(root)))
            .then_some(label)
    });
    let from_home = || {
        let home = home?;
        HOME_MANAGERS
            .iter()
            .find_map(|&(root, label)| under(&home.join(root)).then_some(label))
    };
    from_env.or_else(from_home).map(str::to_owned)
}

/// `{dir}/pyvenv.cfg` for an interpreter at `{dir}/bin/{name}`.
fn venv_config(path: &Path) -> Option<PathBuf> {
    let bin = path.parent()?;
    (bin.file_name()? == "bin").then(|| bin.parent().map(|dir| dir.join("pyvenv.cfg")))?
}

/// `path` with the home directory written as `~`. A home of `/` (or one
/// that is not absolute) shortens nothing.
fn tilde(path: &Path, home: Option<&Path>) -> String {
    let rest = home
        .filter(|home| home.is_absolute() && home.components().count() > 1)
        .and_then(|home| path.strip_prefix(home).ok());
    match rest {
        Some(rest) if rest.as_os_str().is_empty() => "~".to_owned(),
        Some(rest) => format!("~/{}", rest.to_string_lossy()),
        None => path.to_string_lossy().into_owned(),
    }
}

/// The background job: the interpreter's `--version` ([`VERSION_TIMEOUT`])
/// and the venv's configuration. Blocks for at most the timeout plus a
/// file read — never on the main thread.
pub fn details(program: &Program) -> Details {
    Details {
        version: program
            .exec
            .as_deref()
            .and_then(|exec| run_version(exec, VERSION_TIMEOUT))
            .and_then(|output| parse_version(program.family, &output)),
        venv: program
            .venv_config
            .as_deref()
            .and_then(read_venv_config)
            .is_some_and(|text| pyvenv_value(&text, "home").is_some()),
    }
}

/// `{exec} --version`'s standard output, or `None` if it did not come in
/// `timeout`. Stdin is closed and standard error discarded: the answer is
/// one line. Bounded either way: the child leads a process group of its
/// own and the whole group is killed and the child reaped once the output
/// is in or the time is up — a `--version` that lingers after its line, or
/// forks, holds no thread. (A grandchild that left the group with its own
/// `setsid` and kept the pipe would keep the reading thread until it
/// exits.)
pub fn run_version(exec: &Path, timeout: Duration) -> Option<String> {
    use std::os::unix::process::CommandExt as _;
    let mut child = Command::new(exec)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .ok()?;
    let (done, output) = mpsc::channel();
    let reader = child.stdout.take().and_then(|stdout| {
        std::thread::Builder::new()
            .name("program version".into())
            .spawn(move || {
                let mut text = String::new();
                let read = stdout.take(VERSION_OUTPUT_MAX).read_to_string(&mut text);
                let _ = done.send(read.ok().map(|_| text));
            })
            .ok()
    });
    let text = reader.and_then(|_| output.recv_timeout(timeout).ok().flatten());
    end_group(&mut child);
    text
}

/// Kills `child`'s process group and reaps `child`. The child is not
/// reaped before the signal, so its pid — the group's id — cannot have
/// been reused; a group whose members all ended is no error.
fn end_group(child: &mut std::process::Child) {
    if let Ok(group) = libc::pid_t::try_from(child.id()) {
        // SAFETY: a signal to the group our own unreaped child leads.
        unsafe { libc::kill(-group, libc::SIGKILL) };
    }
    let _ = child.wait();
}

/// Blocks until `pid`, the process started at `start`
/// ([`jobs::start_time`]), exits — the recognized program's end, so its bar
/// goes even while the command runs on (`python3; make`). Returns at once
/// if the process is already gone, was replaced by another with the same
/// pid, or cannot be watched: no bar is never wrong. A background thread's
/// call.
pub fn wait_for_exit(pid: u32, start: Option<u64>) {
    use std::os::fd::AsRawFd as _;
    let Some(exit) = start.and_then(|start| jobs::exit_fd(pid, start)) else {
        return;
    };
    let mut poll = libc::pollfd {
        fd: exit.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    loop {
        // SAFETY: one `pollfd` from this frame over a descriptor it owns.
        let ready = unsafe { libc::poll(&raw mut poll, 1, -1) };
        if ready >= 0 || std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted {
            return;
        }
    }
}

/// The version in an interpreter's `--version` output: the first line's
/// first word after the family's own name (`Python 3.14.5`, `v22.13.0`,
/// `1.1.38`, `deno 2.0.0 (stable…)`, `ruby 3.3.0 (…)`). It must start with
/// a digit (or `v` and a digit) and carry no control character — anything
/// else is not a version and the title stays the bare name.
pub fn parse_version(family: Family, output: &str) -> Option<String> {
    let line = output.lines().next()?.trim();
    let mut words = line.split_whitespace();
    let mut word = words.next()?;
    if word.eq_ignore_ascii_case(family.label()) {
        word = words.next()?;
    }
    let digits = word.strip_prefix('v').unwrap_or(word);
    let valid = digits.starts_with(|ch: char| ch.is_ascii_digit())
        && word.len() <= 32
        && !word.chars().any(char::is_control);
    valid.then(|| word.to_owned())
}

/// A `pyvenv.cfg` that is a regular file, at most [`VENV_CONFIG_MAX`]
/// bytes read. The type is checked before opening: a FIFO named
/// `pyvenv.cfg` would block the open.
fn read_venv_config(path: &Path) -> Option<String> {
    if !std::fs::metadata(path).ok()?.is_file() {
        return None;
    }
    let mut text = String::new();
    std::fs::File::open(path)
        .ok()?
        .take(VENV_CONFIG_MAX)
        .read_to_string(&mut text)
        .ok()?;
    Some(text)
}

/// A `key = value` line's value in `pyvenv.cfg`; `home` (the base
/// interpreter's directory) is what every venv has.
pub fn pyvenv_value<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    text.lines().find_map(|line| {
        let (name, value) = line.split_once('=')?;
        (name.trim() == key).then(|| value.trim())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jobs::tests::login_shell;

    const HOME: &str = "/Users/me";

    fn home() -> Option<&'static Path> {
        Some(Path::new(HOME))
    }

    /// A candidate member, pid 200, whose parent (the shell) is outside the
    /// group.
    fn member(name: &str, exec: &str, argv: &[&str], env: &[(&str, &str)]) -> Member {
        Member {
            pid: 200,
            parent: Some(101),
            candidate: Some((
                name.to_owned(),
                ProcArgs {
                    exec: exec.to_owned(),
                    args: argv.iter().map(|&arg| arg.to_owned()).collect(),
                    env: env
                        .iter()
                        .filter(|(key, _)| ENV_KEYS.contains(key))
                        .map(|&(key, value)| (key.to_owned(), value.to_owned()))
                        .collect(),
                },
            )),
        }
    }

    /// `member` at `pid` under `parent`.
    fn at(member: Member, pid: u32, parent: u32) -> Member {
        Member {
            pid,
            parent: Some(parent),
            ..member
        }
    }

    /// A member with no candidate name (`fzf`, `uv`, an agent).
    fn other(pid: u32, parent: u32) -> Member {
        Member {
            pid,
            parent: Some(parent),
            candidate: None,
        }
    }

    fn one(member: Member) -> Option<Program> {
        classify(&[member], home())
    }

    /// The framework Python's bundle executable (measured).
    const FRAMEWORK: &str = "/opt/homebrew/Cellar/python@3.14/3.14.5/Frameworks/Python.framework/\
        Versions/3.14/Resources/Python.app/Contents/MacOS/Python";

    #[test]
    fn the_framework_python_shows_the_launcher_and_asks_the_executable() {
        // Measured: name `Python`, exec and argv[0] the bundle's, the
        // launcher in `__PYVENV_LAUNCHER__`.
        let program = one(member(
            "Python",
            FRAMEWORK,
            &[FRAMEWORK],
            &[("__PYVENV_LAUNCHER__", "/opt/homebrew/bin/python3")],
        ))
        .expect("python3's REPL");
        assert_eq!(program.family, Family::Python);
        assert_eq!(program.path, "/opt/homebrew/bin/python3");
        assert_eq!(program.exec.as_deref(), Some(Path::new(FRAMEWORK)));
        assert_eq!(program.detail, None);
        // Homebrew's `bin` has no `pyvenv.cfg` but it is asked: nothing
        // else names it.
        assert_eq!(
            program.venv_config.as_deref(),
            Some(Path::new("/opt/homebrew/pyvenv.cfg"))
        );
        let bar = program.bar(None);
        assert_eq!(bar.title, "Python");
        assert_eq!(bar.hint, "⌃D exit");
        assert_eq!(bar.tone, ProgramTone::Info);
        let details = Details {
            version: Some("3.14.5".into()),
            venv: false,
        };
        let bar = program.bar(Some(&details));
        assert_eq!(bar.title, "Python 3.14.5");
        assert_eq!(bar.detail, "");
    }

    #[test]
    fn a_venv_is_named_by_its_variable_or_its_configuration() {
        // Activated: `VIRTUAL_ENV` contains the launcher.
        let activated = one(member(
            "Python",
            FRAMEWORK,
            &[FRAMEWORK],
            &[
                ("__PYVENV_LAUNCHER__", "/Users/me/proj/.venv/bin/python3"),
                ("VIRTUAL_ENV", "/Users/me/proj/.venv"),
            ],
        ))
        .expect("venv python");
        assert_eq!(activated.path, "~/proj/.venv/bin/python3");
        assert_eq!(activated.detail.as_deref(), Some("venv"));
        assert_eq!(activated.venv_config, None, "already named");
        // An activated venv does not make another interpreter its own.
        let system = one(member(
            "python3",
            "/usr/bin/python3",
            &["python3"],
            &[("VIRTUAL_ENV", "/Users/me/proj/.venv")],
        ))
        .expect("system python");
        assert_eq!(system.detail, None);
        assert_eq!(system.path, "/usr/bin/python3");
        // Not activated: run by path, the configuration says.
        let unactivated = one(member(
            "Python",
            FRAMEWORK,
            &[FRAMEWORK],
            &[("__PYVENV_LAUNCHER__", "/Users/me/proj/.venv/bin/python")],
        ))
        .expect("unactivated venv");
        assert_eq!(unactivated.detail, None);
        assert_eq!(
            unactivated.venv_config.as_deref(),
            Some(Path::new("/Users/me/proj/.venv/pyvenv.cfg"))
        );
        let details = Details {
            version: Some("3.14.5".into()),
            venv: true,
        };
        let bar = unactivated.bar(Some(&details));
        assert_eq!(bar.title, "Python 3.14.5");
        assert_eq!(bar.detail, "venv");
        assert_eq!(bar.path, "~/proj/.venv/bin/python");
    }

    #[test]
    fn conda_pyenv_and_uv_are_named_by_where_the_interpreter_lives() {
        let conda = one(member(
            "python3.12",
            "/Users/me/miniconda3/envs/ml/bin/python3.12",
            &["python"],
            &[
                ("CONDA_PREFIX", "/Users/me/miniconda3/envs/ml"),
                ("CONDA_DEFAULT_ENV", "ml"),
            ],
        ))
        .expect("conda python");
        assert_eq!(conda.detail.as_deref(), Some("conda ml"));
        assert_eq!(conda.path, "~/miniconda3/envs/ml/bin/python3.12");
        // An active conda environment does not claim `/usr/bin/python3`.
        let outside = one(member(
            "python3",
            "/usr/bin/python3",
            &["python3"],
            &[
                ("CONDA_PREFIX", "/Users/me/miniconda3"),
                ("CONDA_DEFAULT_ENV", "base"),
            ],
        ))
        .expect("system python");
        assert_eq!(outside.detail, None);
        let pyenv = one(member(
            "python3.12",
            "/Users/me/.pyenv/versions/3.12.4/bin/python3.12",
            &["python"],
            &[],
        ))
        .expect("pyenv python");
        assert_eq!(pyenv.detail.as_deref(), Some("pyenv"));
        let moved = one(member(
            "python3.12",
            "/opt/pyenv/versions/3.12.4/bin/python3.12",
            &["python"],
            &[("PYENV_ROOT", "/opt/pyenv")],
        ))
        .expect("pyenv python under its variable");
        assert_eq!(moved.detail.as_deref(), Some("pyenv"));
        let uv = one(member(
            "python3.13",
            "/Users/me/.local/share/uv/python/cpython-3.13.1-macos-aarch64-none/bin/python3.13",
            &["python"],
            &[],
        ))
        .expect("uv python");
        assert_eq!(uv.detail.as_deref(), Some("uv"));
    }

    #[test]
    fn node_managers_are_named_by_the_executables_root() {
        // Measured: nvm's node, `NVM_DIR` set in every process.
        let nvm = one(member(
            "node",
            "/Users/me/.nvm/versions/node/v22.13.0/bin/node",
            &["node"],
            &[("NVM_DIR", "/Users/me/.nvm")],
        ))
        .expect("node REPL");
        assert_eq!(nvm.family, Family::Node);
        assert_eq!(nvm.detail.as_deref(), Some("nvm"));
        assert_eq!(nvm.path, "~/.nvm/versions/node/v22.13.0/bin/node");
        assert_eq!(nvm.venv_config, None, "only Python has a venv");
        let details = Details {
            version: Some("v22.13.0".into()),
            venv: false,
        };
        assert_eq!(nvm.bar(Some(&details)).title, "Node v22.13.0");
        // `NVM_DIR` alone does not claim a Homebrew node.
        let brew = one(member(
            "node",
            "/opt/homebrew/bin/node",
            &["node"],
            &[("NVM_DIR", "/Users/me/.nvm")],
        ))
        .expect("brew node");
        assert_eq!(brew.detail, None);
        let volta = one(member(
            "node",
            "/Users/me/.volta/tools/image/node/22.13.0/bin/node",
            &["node"],
            &[],
        ))
        .expect("volta node");
        assert_eq!(volta.detail.as_deref(), Some("volta"));
        let fnm = one(member(
            "node",
            "/Users/me/.local/state/fnm_multishells/123_456/bin/node",
            &["node"],
            &[],
        ))
        .expect("fnm node");
        assert_eq!(fnm.detail.as_deref(), Some("fnm"));
        let asdf = one(member(
            "node",
            "/Users/me/.asdf/installs/nodejs/22.13.0/bin/node",
            &["node"],
            &[],
        ))
        .expect("asdf node");
        assert_eq!(asdf.detail.as_deref(), Some("asdf"));
    }

    #[test]
    fn bun_deno_and_ruby_repls_are_recognized() {
        // Measured: `bun repl` starts a second `bun` running its REPL
        // script; the parent (lower pid) names it and the script is no REPL.
        let members = [
            member("bun", "/Users/me/.bun/bin/bun", &["bun", "repl"], &[]),
            at(
                member(
                    "bun",
                    "/Users/me/.bun/bin/bun",
                    &[
                        "bun",
                        "//private/tmp/bun-repl@latest--bunx/node_modules/.bin/bun-repl",
                    ],
                    &[],
                ),
                201,
                200,
            ),
        ];
        let bun = classify(&members, home()).expect("bun repl");
        assert_eq!(bun.family, Family::Bun);
        assert_eq!(bun.pid, 200, "the parent, whose exit ends the bar");
        assert_eq!(bun.path, "~/.bun/bin/bun");
        assert_eq!(one(members[1].clone()), None, "a script under bun");
        assert_eq!(
            one(member(
                "bun",
                "/Users/me/.bun/bin/bun",
                &["bun", "run", "dev"],
                &[]
            )),
            None
        );
        // Deno alone, or its subcommand.
        for argv in [&["deno"][..], &["deno", "repl"], &["deno", "-A"]] {
            let deno = one(member("deno", "/opt/homebrew/bin/deno", argv, &[]));
            assert_eq!(deno.map(|deno| deno.family), Some(Family::Deno), "{argv:?}");
        }
        assert_eq!(
            one(member(
                "deno",
                "/opt/homebrew/bin/deno",
                &["deno", "run", "x.ts"],
                &[]
            )),
            None
        );
        // Measured: `irb` is `ruby` with its argv rewritten to `['irb', '']`.
        let ruby = "/System/Library/Frameworks/Ruby.framework/Versions/2.6/usr/bin/ruby";
        let irb = one(member("ruby", ruby, &["irb", ""], &[])).expect("irb");
        assert_eq!(irb.family, Family::Ruby);
        assert_eq!(irb.exec.as_deref(), Some(Path::new(ruby)));
        assert_eq!(irb.path, ruby);
        // As Linux shows a script: the script names it.
        assert!(
            one(member(
                "pry",
                "/usr/bin/ruby3.3",
                &["ruby", "-I", "lib", "/usr/bin/pry"],
                &[]
            ))
            .is_some()
        );
        assert_eq!(
            one(member("ruby", "/usr/bin/ruby", &["ruby", "server.rb"], &[])),
            None
        );
    }

    #[test]
    fn a_script_on_an_interpreter_is_not_a_repl() {
        // The coding agents shipped as scripts: no bar over them.
        let node = "/Users/me/.nvm/versions/node/v22.13.0/bin/node";
        for argv in [
            &["node", "/Users/me/.nvm/versions/node/v22.13.0/bin/codex"][..],
            &[
                "node",
                "--no-warnings",
                "/usr/local/lib/node_modules/@google/gemini-cli/dist/index.js",
            ],
            &["node", "-e", "require('repl')"],
            &["node", "--eval=1"],
            &["node", "-r", "ts-node/register", "app.ts"],
            &["node", "-"],
            &["node", "--", "x.js"],
        ] {
            assert_eq!(one(member("node", node, argv, &[])), None, "{argv:?}");
        }
        for argv in [
            &["node"][..],
            &["node", "--inspect"],
            &["node", "-r", "dotenv/config"],
            &["node", "--require=dotenv/config"],
            &["node", "-i", "-e", "const x = 1"],
        ] {
            assert!(one(member("node", node, argv, &[])).is_some(), "{argv:?}");
        }
        let python = "/usr/bin/python3";
        for argv in [
            &["python3", "/Users/me/.local/bin/aider"][..],
            &["python3", "-c", "import code; code.interact()"],
            &["python3", "-m", "http.server"],
            &["python3", "-mhttp.server"],
            &["python3", "-W", "ignore", "train.py"],
            &["python3", "-"],
        ] {
            assert_eq!(one(member("python3", python, argv, &[])), None, "{argv:?}");
        }
        for argv in [
            &["python3"][..],
            &["python3", "-q"],
            &["python3", "-Wignore"],
            &["python3", "-X", "dev"],
            &["python3", "-i", "train.py"],
            &["python3", "-iu", "train.py"],
            &["python3", "/opt/homebrew/bin/ipython"],
            &["python3", "-m", "IPython"],
            &["python3", "-mbpython"],
            &["python3", "--check-hash-based-pycs", "never"],
        ] {
            assert!(
                one(member("python3", python, argv, &[])).is_some(),
                "{argv:?}"
            );
        }
        // Measured: `ipython` is the framework Python running the script.
        let ipython = one(member(
            "Python",
            FRAMEWORK,
            &[FRAMEWORK, "/opt/homebrew/bin/ipython"],
            &[(
                "__PYVENV_LAUNCHER__",
                "/opt/homebrew/Cellar/python@3.9/3.9.23/bin/python3.9",
            )],
        ))
        .expect("ipython");
        // The interpreter it runs on, asked for its own version.
        assert_eq!(
            ipython.path,
            "/opt/homebrew/Cellar/python@3.9/3.9.23/bin/python3.9"
        );
        assert_eq!(ipython.exec.as_deref(), Some(Path::new(FRAMEWORK)));
    }

    #[test]
    fn the_group_must_be_the_repls_own_line() {
        let python = || member("python3", "/usr/bin/python3", &["python3"], &[]);
        // `fzf | python3`: fzf is a sibling, it turned the terminal raw and
        // python3 reads the pipe — no bar.
        assert_eq!(classify(&[other(199, 101), python()], home()), None);
        // `uv run python`: the wrapper is the REPL's ancestor.
        let wrapped = classify(&[other(199, 101), at(python(), 200, 199)], home());
        assert_eq!(wrapped.map(|program| program.pid), Some(200));
        // A REPL's own child (a `subprocess` it started) is its descendant.
        let parented = classify(&[python(), other(201, 200)], home());
        assert_eq!(parented.map(|program| program.pid), Some(200));
        // Pids wrap: the REPL a REPL started can have the lower pid; the
        // topmost one is the program the user started.
        let wrapped_pids = [
            at(
                member(
                    "python3",
                    "/usr/bin/python3",
                    &["python3", "-i", "a.py"],
                    &[],
                ),
                300,
                101,
            ),
            at(python(), 100, 300),
        ];
        let program = classify(&wrapped_pids, home()).expect("python");
        assert_eq!(program.pid, 300);
        // A corrupt table (a process its own parent) does not loop.
        let looped = [at(python(), 200, 200)];
        assert_eq!(
            classify(&looped, home()).map(|program| program.pid),
            Some(200)
        );
    }

    #[test]
    fn a_front_end_given_a_script_is_not_a_repl() {
        let python = |argv: &[&str]| one(member("python3", "/usr/bin/python3", argv, &[]));
        for argv in [
            &["python3", "/usr/local/bin/ipython", "train.py"][..],
            &["python3", "-m", "IPython", "train.py"],
            &["python3", "-mIPython", "train.py"],
            &["python3", "/usr/local/bin/bpython", "file.py"],
            &["python3", "--", "/usr/local/bin/ipython", "train.py"],
        ] {
            assert_eq!(python(argv), None, "{argv:?}");
        }
        for argv in [
            &["python3", "/usr/local/bin/ipython", "--profile=dev"][..],
            &["python3", "/usr/local/bin/ipython", "-i", "train.py"],
            &["python3", "-m", "IPython", "--no-banner"],
            &["python3", "--", "/usr/local/bin/ipython"],
        ] {
            assert!(python(argv).is_some(), "{argv:?}");
        }
    }

    #[test]
    fn only_pythons_own_names_are_candidates() {
        for name in [
            "python",
            "python3",
            "python3.14",
            "python2.7",
            "Python",
            "ipython",
        ] {
            assert_eq!(Family::of_name(name), Some(Family::Python), "{name}");
        }
        for name in [
            "pythonista",
            "python-lsp-server",
            "python3-config",
            "pythonw",
        ] {
            assert_eq!(Family::of_name(name), None, "{name}");
            // Not even read: a tool named so is never run for a version.
            let path = format!("/usr/local/bin/{name}");
            assert_eq!(one(member(name, &path, &[name], &[])), None, "{name}");
        }
    }

    #[test]
    fn a_conda_environment_is_named_by_its_directory_under_base() {
        // `base` active, an environment's interpreter run by path.
        let program = one(member(
            "python3.12",
            "/Users/me/miniconda3/envs/ml/bin/python3.12",
            &["/Users/me/miniconda3/envs/ml/bin/python"],
            &[
                ("CONDA_PREFIX", "/Users/me/miniconda3"),
                ("CONDA_DEFAULT_ENV", "base"),
            ],
        ))
        .expect("conda python");
        assert_eq!(program.detail.as_deref(), Some("conda ml"));
        // Base's own interpreter is base's.
        let base = one(member(
            "python3.12",
            "/Users/me/miniconda3/bin/python3.12",
            &["python"],
            &[
                ("CONDA_PREFIX", "/Users/me/miniconda3"),
                ("CONDA_DEFAULT_ENV", "base"),
            ],
        ))
        .expect("conda python");
        assert_eq!(base.detail.as_deref(), Some("conda base"));
    }

    #[test]
    fn the_wait_ends_when_the_program_exits() {
        let mut child = std::process::Command::new("/bin/sleep")
            .arg("30")
            .spawn()
            .expect("sleep did not spawn");
        let pid = child.id();
        let start = jobs::start_time(pid);
        assert!(start.is_some());
        let (done, ended) = mpsc::channel();
        std::thread::spawn(move || {
            wait_for_exit(pid, start);
            let _ = done.send(());
        });
        assert!(
            ended.recv_timeout(Duration::from_millis(300)).is_err(),
            "the program still runs"
        );
        let _ = child.kill();
        let _ = child.wait();
        assert!(
            ended.recv_timeout(Duration::from_secs(10)).is_ok(),
            "its exit ends the wait"
        );
        // Gone already, or never known: no wait at all.
        wait_for_exit(pid, start);
        wait_for_exit(pid, None);
    }

    #[test]
    fn a_version_that_lingers_after_its_line_is_not_waited_for() {
        let dir = std::env::temp_dir().join(format!("bt-program-linger-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        let exec = dir.join("python3");
        std::fs::write(
            &exec,
            "#!/bin/sh\necho 'Python 3.99.2'\nexec >&-\nexec sleep 30\n",
        )
        .expect("script");
        std::fs::set_permissions(&exec, std::os::unix::fs::PermissionsExt::from_mode(0o755))
            .expect("chmod");
        let started = std::time::Instant::now();
        assert_eq!(
            run_version(&exec, Duration::from_secs(20)).as_deref(),
            Some("Python 3.99.2\n")
        );
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "the closed output is the answer; the rest is killed"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unknown_names_and_relative_paths() {
        assert_eq!(
            one(member("codex", "/usr/local/bin/codex", &["codex"], &[])),
            None
        );
        assert_eq!(
            one(member(
                "claude",
                "/Users/me/.local/bin/claude",
                &["claude"],
                &[]
            )),
            None
        );
        assert_eq!(
            one(member("pythonista", "/x/pythonista", &["pythonista"], &[])),
            None
        );
        // An executable path that is not absolute: no path, no version asked.
        let relative = one(member("node", "./node", &["./node"], &[])).expect("node");
        assert_eq!(relative.exec, None);
        assert_eq!(relative.path, "");
        assert_eq!(relative.bar(None).path, "");
        // A launcher that is not absolute falls back to the executable.
        let python = one(member(
            "python3",
            "/usr/bin/python3",
            &["python3"],
            &[("__PYVENV_LAUNCHER__", "python3")],
        ))
        .expect("python");
        assert_eq!(python.path, "/usr/bin/python3");
        // The launcher is Python's: inherited by a node, it is not read.
        let node = one(member(
            "node",
            "/opt/homebrew/bin/node",
            &["node"],
            &[("__PYVENV_LAUNCHER__", "/opt/homebrew/bin/python3")],
        ))
        .expect("node");
        assert_eq!(node.path, "/opt/homebrew/bin/node");
    }

    #[test]
    fn home_is_shortened_by_whole_components() {
        assert_eq!(tilde(Path::new("/Users/me/x"), home()), "~/x");
        assert_eq!(tilde(Path::new("/Users/me"), home()), "~");
        assert_eq!(
            tilde(Path::new("/Users/meadow/x"), home()),
            "/Users/meadow/x"
        );
        assert_eq!(tilde(Path::new("/usr/bin/x"), home()), "/usr/bin/x");
        assert_eq!(
            tilde(Path::new("/usr/bin/x"), Some(Path::new("/"))),
            "/usr/bin/x"
        );
        assert_eq!(tilde(Path::new("/usr/bin/x"), None), "/usr/bin/x");
    }

    #[test]
    fn the_environment_beyond_the_list_never_reaches_a_program() {
        // `find` asks the table for `ENV_KEYS` only; a secret in the
        // process's environment is in no record, no program and no bar.
        let table = login_shell(200).exec(
            200,
            101,
            200,
            ("node", "/Users/me/.nvm/versions/node/v22.13.0/bin/node"),
            &["node"],
            &[
                ("OPENAI_API_KEY", "sk-secret"),
                ("NVM_DIR", "/Users/me/.nvm"),
            ],
        );
        let program = find(ShellParent::Login, 100, &table, home()).expect("node");
        assert_eq!(program.detail.as_deref(), Some("nvm"));
        let printed = format!("{program:?} {:?}", program.bar(None));
        assert!(
            !printed.contains("sk-secret") && !printed.contains("OPENAI"),
            "{printed}"
        );
        for key in ENV_KEYS {
            for word in ["KEY", "TOKEN", "PASS", "PWD", "AUTH", "SECRET"] {
                assert!(!key.contains(word), "{key}");
            }
        }
    }

    #[test]
    fn a_database_client_names_its_server_and_its_mark_host() {
        // `psql` in its own group under the shell; its password variable is
        // not asked for, its server's are.
        let table = login_shell(200).exec(
            200,
            101,
            200,
            ("psql", "/opt/homebrew/opt/libpq/bin/psql"),
            &[
                "psql",
                "-U",
                "app",
                "postgresql://app:hunter2@db.prod:5432/main",
            ],
            &[("PGPASSWORD", "hunter2"), ("PGHOST", "db.stage")],
        );
        let program = find(ShellParent::Login, 100, &table, home()).expect("psql");
        assert_eq!(program.family, Family::Database(Client::Postgres));
        assert_eq!(program.exec, None, "no version is asked of a client");
        assert_eq!(program.host.as_deref(), Some("db.prod"));
        let bar = program.bar(None);
        assert_eq!(bar.title, "postgres");
        assert_eq!(bar.path, "app@db.prod:5432/main");
        assert_eq!(bar.detail, "");
        assert_eq!(bar.hint, "\\q to leave");
        assert_eq!(bar.host, "db.prod");
        assert_eq!(bar.tone, ProgramTone::Info);
        // The background job adds nothing: the bar is whole at once.
        assert_eq!(program.bar(Some(&details(&program))), bar);
        let printed = format!("{program:?} {bar:?}");
        assert!(
            !printed.contains("hunter"),
            "the password reached the program or its bar"
        );
        // A service names the server from its file: the detail says it.
        let service = one(member(
            "psql",
            "/usr/bin/psql",
            &["psql", "service=prod"],
            &[],
        ))
        .expect("psql");
        assert_eq!(service.bar(None).detail, "service prod");
        assert_eq!(service.host, None);
        // SQLite's file, `~`-shortened; no host to mark.
        let sqlite = one(member(
            "sqlite3",
            "/usr/bin/sqlite3",
            &["sqlite3", "/Users/me/x.db"],
            &[],
        ))
        .expect("sqlite3");
        assert_eq!(sqlite.bar(None).path, "~/x.db");
        assert_eq!(sqlite.bar(None).title, "sqlite");
        assert_eq!(sqlite.host, None);
        // A client running a command is no prompt: no bar.
        assert_eq!(
            one(member(
                "redis-cli",
                "/opt/homebrew/bin/redis-cli",
                &["redis-cli", "get", "k"],
                &[],
            )),
            None
        );
    }

    #[test]
    fn mongosh_as_a_node_script_is_the_client() {
        // Homebrew's `mongosh`: node running the script by its path.
        let node = "/opt/homebrew/opt/node/bin/node";
        let program = one(member(
            "node",
            node,
            &[
                "node",
                "/opt/homebrew/bin/mongosh",
                "mongodb://app:hunter2@db.prod/shop",
            ],
            &[],
        ))
        .expect("mongosh");
        assert_eq!(program.family, Family::Database(Client::Mongo));
        assert_eq!(program.bar(None).path, "app@db.prod/shop");
        assert_eq!(program.host.as_deref(), Some("db.prod"));
        // As Linux shows a script run by its own shebang: named after the
        // script, the argv the interpreter's.
        let shebang = one(member(
            "mongosh",
            "/usr/bin/node",
            &[
                "/usr/bin/node",
                "/usr/bin/mongosh",
                "mongodb://db.prod/shop",
            ],
            &[],
        ))
        .expect("mongosh");
        assert_eq!(shebang.bar(None).path, "db.prod/shop");
        // With node's own options before it.
        let optioned = one(member(
            "node",
            node,
            &["node", "--no-warnings", "-r", "x", "/x/mongosh.js", "shop"],
            &[],
        ))
        .expect("mongosh");
        assert_eq!(optioned.bar(None).path, "/shop");
        // Another script is no client — and no REPL either; nor is inline
        // code whose argument happens to be named so.
        for argv in [
            &["node", "/x/server.js"][..],
            &["node", "-e", "run()", "mongosh"],
        ] {
            assert_eq!(one(member("node", node, argv, &[])), None, "{argv:?}");
        }
    }

    #[test]
    fn find_walks_the_foreground_group_in_pid_order() {
        // `uv run python`: uv leads the group, python is its child.
        let table = login_shell(200)
            .run(200, 101, 200, &["/Users/me/.local/bin/uv", "run", "python"])
            .exec(
                201,
                200,
                200,
                ("python3.13", "/Users/me/proj/.venv/bin/python3.13"),
                &["/Users/me/proj/.venv/bin/python3"],
                &[("VIRTUAL_ENV", "/Users/me/proj/.venv")],
            );
        let program = find(ShellParent::Login, 100, &table, home()).expect("python");
        assert_eq!(program.detail.as_deref(), Some("venv"));
        // The shell's own group (`exec`'d, or between commands): no program.
        let idle = login_shell(101).exec(
            300,
            101,
            300,
            ("node", "/opt/homebrew/bin/node"),
            &["node"],
            &[],
        );
        assert_eq!(find(ShellParent::Login, 100, &idle, home()), None);
        // An unreadable record (another user's process) is skipped.
        let root = login_shell(200).with(200, 101, 200, "python3");
        assert_eq!(find(ShellParent::Login, 100, &root, home()), None);
        // An agent: nothing recognized.
        let codex = login_shell(200)
            .run(
                200,
                101,
                200,
                &["/opt/homebrew/bin/node", "/opt/homebrew/bin/codex"],
            )
            .run(
                201,
                200,
                200,
                &["/opt/homebrew/lib/node_modules/@openai/codex/bin/codex"],
            );
        assert_eq!(find(ShellParent::Login, 100, &codex, home()), None);
    }

    #[test]
    fn version_lines_are_read_per_family() {
        let cases = [
            (Family::Python, "Python 3.14.5\n", Some("3.14.5")),
            (Family::Python, "Python 3.13.0rc2+\n", Some("3.13.0rc2+")),
            (Family::Node, "v22.13.0\n", Some("v22.13.0")),
            (Family::Bun, "1.1.38\n", Some("1.1.38")),
            (
                Family::Deno,
                "deno 2.0.0 (stable, release, aarch64-apple-darwin)\nv8 12.9\ntypescript 5.6\n",
                Some("2.0.0"),
            ),
            (
                Family::Ruby,
                "ruby 3.3.0 (2023-12-25 revision 5124f9ac75) [arm64-darwin23]\n",
                Some("3.3.0"),
            ),
            (Family::Python, "", None),
            (Family::Python, "Python\n", None),
            (Family::Node, "Usage: node [options]\n", None),
            (Family::Python, "Python \u{1b}[31m3\n", None),
            (Family::Bun, &format!("{}\n", "9".repeat(40)), None),
        ];
        for (family, output, expected) in cases {
            assert_eq!(
                parse_version(family, output).as_deref(),
                expected,
                "{family:?} {output:?}"
            );
        }
    }

    #[test]
    fn pyvenv_values_are_trimmed_key_lines() {
        let text = "home = /opt/homebrew/opt/python@3.14/bin\n\
                    include-system-site-packages = false\n\
                    version = 3.14.5\n";
        assert_eq!(
            pyvenv_value(text, "home"),
            Some("/opt/homebrew/opt/python@3.14/bin")
        );
        assert_eq!(pyvenv_value(text, "version"), Some("3.14.5"));
        assert_eq!(pyvenv_value(text, "prompt"), None);
        assert_eq!(pyvenv_value("homes = x", "home"), None);
    }

    #[test]
    fn the_details_job_reads_the_version_and_the_venv() {
        let dir = std::env::temp_dir().join(format!("bt-program-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("bin")).expect("dir");
        // A stand-in interpreter: a script that prints a version line.
        let exec = dir.join("bin/python3");
        std::fs::write(&exec, "#!/bin/sh\necho \"Python 3.99.1\"\n").expect("script");
        std::fs::set_permissions(&exec, std::os::unix::fs::PermissionsExt::from_mode(0o755))
            .expect("chmod");
        std::fs::write(dir.join("pyvenv.cfg"), "home = /usr/bin\n").expect("cfg");
        let program = Program {
            pid: 1,
            family: Family::Python,
            exec: Some(exec.clone()),
            path: exec.to_string_lossy().into_owned(),
            detail: None,
            host: None,
            venv_config: venv_config(&exec),
        };
        assert_eq!(
            details(&program),
            Details {
                version: Some("3.99.1".into()),
                venv: true,
            }
        );
        // No `home` line: not a venv's configuration; a FIFO is not read.
        std::fs::write(dir.join("pyvenv.cfg"), "version = 3.99.1\n").expect("cfg");
        assert!(!details(&program).venv);
        std::fs::remove_file(dir.join("pyvenv.cfg")).expect("rm");
        let fifo = std::ffi::CString::new(dir.join("pyvenv.cfg").to_string_lossy().as_bytes())
            .expect("path");
        // SAFETY: a NUL-terminated path from this frame.
        assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
        assert!(!details(&program).venv, "a FIFO is not opened");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_version_that_does_not_come_in_time_is_killed() {
        let dir = std::env::temp_dir().join(format!("bt-program-slow-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        let exec = dir.join("slow");
        std::fs::write(&exec, "#!/bin/sh\nexec sleep 30\n").expect("script");
        std::fs::set_permissions(&exec, std::os::unix::fs::PermissionsExt::from_mode(0o755))
            .expect("chmod");
        let started = std::time::Instant::now();
        assert_eq!(run_version(&exec, Duration::from_millis(200)), None);
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "killed, not waited out"
        );
        assert_eq!(
            run_version(&dir.join("missing"), Duration::from_secs(1)),
            None
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_bars_strings_are_the_ones_the_atlas_checks() {
        // Every non-ASCII character a bar can carry from here is in the list
        // `bt-atlas` checks in Menlo's small class.
        for family in [
            Family::Python,
            Family::Node,
            Family::Bun,
            Family::Deno,
            Family::Ruby,
        ] {
            assert!(family.row().is_some(), "{family:?} has no row");
        }
        let families = INTERPRETERS
            .iter()
            .map(|row| row.family)
            .chain(Client::ALL.map(Family::Database));
        for family in families {
            for ch in family.label().chars().chain(family.hint().chars()) {
                assert!(
                    ch.is_ascii() || bt_core::PROGRAM_GLYPHS.contains(&ch),
                    "'{ch}' in {family:?}"
                );
            }
        }
    }
}
