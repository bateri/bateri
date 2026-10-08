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
//! job's: the interpreter's own `--version`, a virtual environment's
//! `pyvenv.cfg` and a kubeconfig's `current-context:` line. The bar is first
//! written with what the table knows and completed when the job returns.
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
//!
//! **Containers and clusters** (`docker`/`podman` `run`/`exec`, compose's
//! `exec`/`run`; `kubectl exec`/`run`/`debug`): the bar names where the
//! session runs — `container redis:alpine`, `k8s prod-eu · payments
//! pod/api-7f9c` ([`container`]). kubectl's context is the marks' name,
//! **whole** (`kubernetes-admin@kubernetes` is one name, its `@` no user's),
//! read from `--context` or, in the background job, from the kubeconfig's
//! `current-context:` line.
//!
//! **Shells** ([`shell`]): a shell running as root — `root  exit to
//! leave`, in the theme's `error` — and an interactive shell started from
//! the prompt or by a tool that opens one — `nested bash  exit to return`.
//! The order is the bar's: a root shell first, then what runs in the group
//! (an interpreter, a client, a container session — a shell that started
//! python leaves the bar to python), a nested shell last. While sudo holds
//! the terminal its own raw modes say nothing of the program
//! ([`Found::Waiting`], [`Found::Elevated`]).

mod container;
mod database;
mod options;
mod shell;

pub use container::Kube;
pub use database::{Client, Target};

use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use bt_core::{MarkSubject, ProgramBar, ProgramTone};

use crate::jobs::{self, ProcArgs, ProcessTable, ShellParent};

/// The environment variables a candidate's record carries — **the whole
/// list**; no other variable of any process is read. Each has a single
/// use: the framework Python's launcher path, the managers' roots, and the
/// database clients' server, address, port, user, database and libpq
/// service, and kubectl's kubeconfig files and home. **No
/// password variable** (`PGPASSWORD`, `MYSQL_PWD`, `REDISCLI_AUTH`) is
/// here, so none is ever read out of a process.
pub const ENV_KEYS: [&str; 17] = [
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
    "KUBECONFIG",
    "HOME",
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
/// of [`INTERPRETERS`]), a database client, a container session or a
/// Kubernetes one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    Python,
    Node,
    Bun,
    Deno,
    Ruby,
    Database(Client),
    /// `docker`, `podman` or compose in a container.
    Container,
    /// `kubectl` in a pod or on a node.
    Kubernetes,
    /// A shell running as root (`sudo -i`, `su`).
    Root,
    /// An interactive shell started from the prompt, or the tool that
    /// opened one: its name or the tool's title (`bash`, `poetry shell`).
    Nested(&'static str),
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
        match name {
            "docker" | "podman" | "docker-compose" => return Some(Self::Container),
            "kubectl" => return Some(Self::Kubernetes),
            _ => {}
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

    /// The bar's title, before a version (a nested shell's name follows
    /// it).
    fn label(self) -> &'static str {
        match self {
            Self::Database(client) => client.label(),
            Self::Container => "container",
            Self::Kubernetes => "k8s",
            Self::Root => "root",
            Self::Nested(_) => shell::NESTED,
            interpreter => interpreter.row().map_or("", |row| row.label),
        }
    }

    /// How to leave.
    fn hint(self) -> &'static str {
        match self {
            Self::Database(client) => client.hint(),
            // The shell inside: its own `exit`.
            Self::Container | Self::Kubernetes | Self::Root => "exit to leave",
            // Back to the terminal's own shell.
            Self::Nested(_) => "exit to return",
            interpreter => interpreter.row().map_or("", |row| row.hint),
        }
    }

    /// An interpreter's row; `None` for the other families (their own
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
    /// SQLite's file); for a container its image, container or service, for
    /// a Kubernetes session its resource (`pod/api-7f9c`). Empty when
    /// unknown.
    pub path: String,
    /// The detail after the title: what manages an interpreter, from the
    /// environment and the path (`venv`, `nvm`, `conda base`), a database
    /// client's libpq service (`service prod`) or a Kubernetes session's
    /// namespace; `None` when nothing says.
    pub detail: Option<String>,
    /// The server's host the `[remote] hosts` marks are resolved against
    /// (a database client's); `None` for an interpreter, a socket or a
    /// file.
    pub host: Option<String>,
    /// A virtual environment's configuration to look for in the background:
    /// a Python run from `{dir}/bin/python` that nothing else names is a
    /// venv's when `{dir}/pyvenv.cfg` is one (an unactivated venv).
    pub venv_config: Option<PathBuf>,
    /// A Kubernetes session's context and namespace, and the kubeconfig
    /// files the background job reads its context from when its command
    /// line names none.
    pub kube: Option<Kube>,
}

/// What the background job adds ([`details`]).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Details {
    /// The interpreter's version as its `--version` says it (`3.14.5`,
    /// `v22.13.0`).
    pub version: Option<String>,
    /// [`Program::venv_config`] is a virtual environment's.
    pub venv: bool,
    /// The context the kubeconfig files of [`Kube::files`] name.
    pub context: Option<String>,
}

impl Program {
    /// The guide bar: what the table knew, completed by `details` when the
    /// background job has returned.
    ///
    /// A Kubernetes session's context is in the **title** — `k8s prod-eu` —
    /// since the title is never shortened (a cut context reads as another
    /// one) and carries the mark's color; it is the marks' name, whole. The
    /// namespace is the detail (`· payments`): it can drop on a narrow row,
    /// and it shows while the context is not known.
    pub fn bar(&self, details: Option<&Details>) -> ProgramBar {
        let mut title = self.family.label().to_owned();
        if let Family::Nested(name) = self.family {
            title.push(' ');
            title.push_str(name);
        }
        if let Some(version) = details.and_then(|details| details.version.as_deref()) {
            title.push(' ');
            title.push_str(version);
        }
        let context = self.kube.as_ref().and_then(|kube| {
            kube.context
                .clone()
                .or_else(|| details.and_then(|details| details.context.clone()))
        });
        if let Some(context) = &context {
            title.push(' ');
            title.push_str(context);
        }
        let (host, subject) = match context {
            Some(context) => (context, MarkSubject::Whole),
            None => (self.host.clone().unwrap_or_default(), MarkSubject::Host),
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
            host,
            subject,
            // Every key typed there runs as root: the bar is red whatever
            // a mark says.
            tone: if self.family == Family::Root {
                ProgramTone::Error
            } else {
                ProgramTone::Info
            },
        }
    }

    /// A shell's program: nothing to ask in the background, no path — the
    /// bar is its title and how to leave.
    fn shell(pid: u32, family: Family) -> Self {
        Self {
            pid,
            family,
            exec: None,
            path: String::new(),
            detail: None,
            host: None,
            venv_config: None,
            kube: None,
        }
    }
}

/// A member of the foreground group: its pid, its parent and — for a
/// candidate name only — what the bars ask of it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Member {
    pub pid: u32,
    pub parent: Option<u32>,
    /// `None` for a name no bar knows.
    pub candidate: Option<Candidate>,
}

/// A member whose name a bar knows: an interpreter's, a client's, a tool's,
/// a shell's or an elevator's.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub name: String,
    /// The effective user ([`ProcessTable::uid`]); `None` if unreadable.
    pub uid: Option<u32>,
    /// The exec record ([`ENV_KEYS`] only); `None` when unreadable —
    /// another user's process, a root shell's.
    pub record: Option<ProcArgs>,
}

/// What the foreground group says once the terminal went raw ([`find`]).
///
/// **While sudo (or `su`) holds the terminal the raw modes are its own**,
/// not the program's — its password prompt with `pwfeedback`, the relay of
/// the terminal it opened for the command with `use_pty` (sudo's default) —
/// so the answer is the command it runs, looked at by itself: nothing yet
/// is [`Self::Waiting`], a command no bar knows is [`Self::Elevated`].
/// Marking on sudo's modes would keep the band down for a whole `sudo make
/// install`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Found {
    /// No answer yet — asked again on the next output: the terminal's own
    /// shell holds it, the group cannot be read, or sudo runs nothing yet
    /// (its password prompt; `sudo -i`'s root shell comes after it).
    Waiting,
    /// sudo runs a command no bar knows (`sudo make install`): not marked,
    /// and not asked again for this command — the dock stays.
    Elevated,
    /// A program no bar knows — an agent, a TUI: marked, the band goes and
    /// nothing takes its place.
    Unknown,
    /// A program a guide bar names (boxed: the other answers carry
    /// nothing).
    Program(Box<Program>),
}

#[cfg(test)]
impl Found {
    /// The program, when one is named.
    pub fn program(self) -> Option<Program> {
        match self {
            Self::Program(program) => Some(*program),
            Self::Waiting | Self::Elevated | Self::Unknown => None,
        }
    }
}

/// How deep the command sudo runs is looked for, through the processes of
/// [`shell::ELEVATORS`] — a **design constant**: the measured chain is sudo,
/// the monitor it forks for `use_pty`, then the command (two levels);
/// `sudo su -` adds su and the shell it forks.
const ELEVATED_DEPTH: usize = 4;

/// How far above a nested shell the tool that opened it is looked for, up
/// to the terminal's own shell — a **design constant**: a forking tool is
/// the shell's parent, one wrapper between them is allowed.
const TOOL_DEPTH: usize = 2;

/// The program in the terminal's foreground group: every member with its
/// parent, candidates with their user and record ([`ENV_KEYS`] only). A
/// group sudo holds is answered by the command it runs ([`Found`]); a
/// nested shell a forking tool opened takes the tool's title.
/// [`Found::Waiting`] when the shell's own group holds the terminal or the
/// table is unreadable. Main thread: system calls only.
pub fn find(
    parent: ShellParent,
    child: u32,
    table: &impl ProcessTable,
    home: Option<&Path>,
) -> Found {
    let Some(shell) = jobs::shell_pid(parent, child, table) else {
        return Found::Waiting;
    };
    let Some(groups) = table
        .groups(shell)
        .filter(|groups| groups.foreground != 0 && groups.foreground != groups.own)
    else {
        return Found::Waiting;
    };
    let mut pids = table.members(groups.foreground);
    pids.sort_unstable();
    let members: Vec<Member> = pids
        .into_iter()
        .map(|pid| Member {
            pid,
            parent: table.parent(pid),
            candidate: candidate(pid, table),
        })
        .collect();
    let elevators: Vec<u32> = members
        .iter()
        .filter(|member| {
            member.candidate.as_ref().is_some_and(|candidate| {
                shell::ELEVATORS.contains(&candidate.name.as_str()) && candidate.uid == Some(0)
            })
        })
        .map(|member| member.pid)
        .collect();
    if !elevators.is_empty() {
        let Some(command) = command_below(&elevators, table) else {
            return Found::Waiting;
        };
        let member = Member {
            pid: command,
            parent: None,
            candidate: candidate(command, table),
        };
        return classify(&[member], home)
            .map_or(Found::Elevated, |program| Found::Program(Box::new(program)));
    }
    match classify(&members, home) {
        Some(program) => Found::Program(Box::new(opened_by(program, shell, table))),
        None => Found::Unknown,
    }
}

/// A member's candidate: its name when a bar knows it, its user and its
/// record — [`ENV_KEYS`] when readable, else the argv alone (another
/// user's process on Linux, whose `cmdline` anyone may read; on macOS
/// neither reads).
fn candidate(pid: u32, table: &impl ProcessTable) -> Option<Candidate> {
    let name = table
        .name(pid)
        .filter(|name| Family::of_name(name).is_some() || shell::is_candidate(name))?;
    Some(Candidate {
        uid: table.uid(pid),
        record: table
            .procargs(pid, &ENV_KEYS)
            .or_else(|| table.procargs(pid, &[])),
        name,
    })
}

/// The command `elevators` run: the first process under them that is not
/// itself one, walked through sudo's monitor and `su` ([`ELEVATED_DEPTH`]);
/// `None` before it starts (sudo at its prompt). Only the command —
/// `sudo make`'s recipe shells are make's, not a session.
fn command_below(elevators: &[u32], table: &impl ProcessTable) -> Option<u32> {
    let mut level = elevators.to_vec();
    for _ in 0..ELEVATED_DEPTH {
        let mut next = Vec::new();
        for child in level.iter().flat_map(|&pid| table.children(pid)) {
            match table.name(child) {
                Some(name) if shell::ELEVATORS.contains(&name.as_str()) => next.push(child),
                _ => return Some(child),
            }
        }
        if next.is_empty() {
            break;
        }
        level = next;
    }
    None
}

/// A nested shell a forking tool opened (`devbox shell`), named after the
/// tool: an interactive shell takes a group of its own, so the tool is
/// above it, outside the group — looked for up to [`TOOL_DEPTH`] parents,
/// short of the terminal's `shell`. Any other program as it is.
fn opened_by(program: Program, shell: u32, table: &impl ProcessTable) -> Program {
    if !matches!(program.family, Family::Nested(_)) {
        return program;
    }
    let mut at = table.parent(program.pid);
    for _ in 0..TOOL_DEPTH {
        let Some(pid) = at.filter(|&pid| pid != shell && pid > 1) else {
            break;
        };
        let title = table
            .name(pid)
            .filter(|name| Family::of_name(name).is_some() || shell::is_candidate(name))
            .and_then(|name| shell::tool(&name, &table.procargs(pid, &[])?));
        if let Some(title) = title {
            return Program {
                family: Family::Nested(title),
                ..program
            };
        }
        at = table.parent(pid);
    }
    program
}

/// The group's program, in the bar's order: a **root shell**
/// ([`shell::is_root_shell`]); else the group's REPL, client or container
/// session ([`recognize`]); else a **nested shell** or the tool that opened
/// one ([`shell::nested`]) — a shell that started python leaves the bar to
/// python. Each kind must be the group's line ([`in_line`]): a root shell
/// on a pipeline's other side does not name what reads the keyboard.
pub fn classify(members: &[Member], home: Option<&Path>) -> Option<Program> {
    let roots: Vec<Program> = members
        .iter()
        .filter(|member| {
            member.candidate.as_ref().is_some_and(|candidate| {
                shell::is_root_shell(&candidate.name, candidate.uid, candidate.record.as_ref())
            })
        })
        .map(|member| Program::shell(member.pid, Family::Root))
        .collect();
    if let Some(root) = in_line(members, roots) {
        return Some(root);
    }
    let recognized: Vec<Program> = members
        .iter()
        .filter_map(|member| {
            let candidate = member.candidate.as_ref()?;
            recognize(
                member.pid,
                &candidate.name,
                candidate.record.as_ref()?,
                home,
            )
        })
        .collect();
    if let Some(program) = in_line(members, recognized) {
        return Some(program);
    }
    let nested: Vec<Program> = members
        .iter()
        .filter_map(|member| {
            let candidate = member.candidate.as_ref()?;
            let name = shell::nested(&candidate.name, candidate.record.as_ref()?)?;
            Some(Program::shell(member.pid, Family::Nested(name)))
        })
        .collect();
    in_line(members, nested)
}

/// The program of `recognized` the whole group is the line of: the
/// **topmost** (one with no recognized ancestor in the group — pids wrap,
/// so the lower pid is not the parent), and every other member must be its
/// ancestor (a wrapper: `uv run python`) or its descendant (`bun repl`'s
/// script, a REPL's own children). A member on another branch is a
/// pipeline's other side — in `fzf | python3` fzf turned the terminal raw
/// and python3 reads a pipe — and the bar would name the wrong program.
fn in_line(members: &[Member], recognized: Vec<Program>) -> Option<Program> {
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
    match family {
        Family::Container => {
            let target = container::container(name, args)?;
            return Some(tool(pid, family, target.unwrap_or_default(), None));
        }
        Family::Kubernetes => {
            let kube = container::kubernetes(args, record)?;
            let resource = kube.resource.clone().unwrap_or_default();
            return Some(tool(pid, family, resource, Some(kube)));
        }
        _ => {}
    }
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
        kube: None,
    })
}

/// A container's or a cluster's program: no executable asked for a
/// version, `path` where the session runs, a Kubernetes namespace the
/// detail.
fn tool(pid: u32, family: Family, path: String, kube: Option<Kube>) -> Program {
    Program {
        pid,
        family,
        exec: None,
        path,
        detail: kube.as_ref().and_then(|kube| kube.namespace.clone()),
        host: None,
        venv_config: None,
        kube,
    }
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
        kube: None,
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
        // A client's or a tool's session is decided with its target
        // (`client_program`, `container`), a shell's by `shell`.
        Family::Database(_)
        | Family::Container
        | Family::Kubernetes
        | Family::Root
        | Family::Nested(_) => false,
    }
}

/// What Python runs after its own options ([`python_line`]).
enum PythonRuns<'a> {
    /// Nothing: the REPL.
    Nothing,
    /// Code given inline (`-c`) or read from stdin (`-`).
    Code,
    /// A module (`-m`) and the arguments after it.
    Module(&'a str, &'a [String]),
    /// A script and the arguments after it.
    Script(&'a str, &'a [String]),
}

/// Python's command line (argv without argv[0]): whether `-i` came before
/// what runs, and what runs. `-c` and `-m` end the options (the rest is the
/// code's argv), `-W`/`-X` and `--check-hash-based-pycs` take a value, the
/// others are flags that may be clustered (`-iu`); `--` ends them and a lone
/// `-` reads the code from stdin. The one walk over CPython's option
/// grammar: the REPL check and the scripts of tools and shells read it.
fn python_line(args: &[String]) -> (bool, PythonRuns<'_>) {
    let mut interactive = false;
    let mut index = 0;
    while let Some(arg) = args.get(index) {
        if arg == "--" {
            let runs = args.get(index + 1).map_or(PythonRuns::Nothing, |script| {
                PythonRuns::Script(script, args.get(index + 2..).unwrap_or_default())
            });
            return (interactive, runs);
        }
        if arg == "-" {
            return (interactive, PythonRuns::Code);
        }
        if !arg.starts_with('-') {
            let rest = args.get(index + 1..).unwrap_or_default();
            return (interactive, PythonRuns::Script(arg, rest));
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
                'c' => return (interactive, PythonRuns::Code),
                'm' => {
                    let (module, after) = if rest.is_empty() {
                        (args.get(index + 1).map(String::as_str), index + 2)
                    } else {
                        (Some(rest), index + 1)
                    };
                    let runs = module.map_or(PythonRuns::Code, |module| {
                        PythonRuns::Module(module, args.get(after..).unwrap_or_default())
                    });
                    return (interactive, runs);
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
    (interactive, PythonRuns::Nothing)
}

/// A REPL when nothing runs, when `-i` asks for one after the code, or when
/// the script or module is a front-end that is given nothing to run itself
/// ([`runs_nothing`]).
fn python_is_repl(args: &[String]) -> bool {
    let (interactive, runs) = python_line(args);
    interactive
        || match runs {
            PythonRuns::Nothing => true,
            PythonRuns::Code => false,
            PythonRuns::Module(module, rest) => {
                PYTHON_REPL_MODULES.contains(&module) && runs_nothing(rest)
            }
            PythonRuns::Script(script, rest) => frontend(script, rest),
        }
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

/// The script Python runs and the arguments after it ([`python_line`]);
/// `None` when it runs no script — code given with `-c` or on stdin, a
/// module, nothing.
fn python_script(args: &[String]) -> Option<(&str, &[String])> {
    match python_line(args).1 {
        PythonRuns::Script(script, rest) => Some((script, rest)),
        PythonRuns::Nothing | PythonRuns::Code | PythonRuns::Module(..) => None,
    }
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
/// that is not absolute) shortens nothing. Also the tab bar's summary card's
/// directory (`tabs::card_lines`).
pub(crate) fn tilde(path: &Path, home: Option<&Path>) -> String {
    let rest = home
        .filter(|home| home.is_absolute() && home.components().count() > 1)
        .and_then(|home| path.strip_prefix(home).ok());
    match rest {
        Some(rest) if rest.as_os_str().is_empty() => "~".to_owned(),
        Some(rest) => format!("~/{}", rest.to_string_lossy()),
        None => path.to_string_lossy().into_owned(),
    }
}

/// The background job: the interpreter's `--version` ([`VERSION_TIMEOUT`]),
/// the venv's configuration and a Kubernetes session's context from its
/// kubeconfig files (their `current-context:` line alone,
/// `container::read_context`). Blocks for at most the timeout plus a few file reads
/// — never on the main thread.
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
        context: program
            .kube
            .as_ref()
            .filter(|kube| kube.context.is_none())
            .and_then(|kube| container::read_context(&kube.files)),
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
    use crate::jobs::tests::{USER, login_shell};

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
            candidate: Some(Candidate {
                name: name.to_owned(),
                uid: Some(USER),
                record: Some(ProcArgs {
                    exec: exec.to_owned(),
                    args: argv.iter().map(|&arg| arg.to_owned()).collect(),
                    env: env
                        .iter()
                        .filter(|(key, _)| ENV_KEYS.contains(key))
                        .map(|&(key, value)| (key.to_owned(), value.to_owned()))
                        .collect(),
                }),
            }),
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
            context: None,
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
            context: None,
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
            context: None,
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
        let program = find(ShellParent::Login, 100, &table, home())
            .program()
            .expect("node");
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
        let program = find(ShellParent::Login, 100, &table, home())
            .program()
            .expect("psql");
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
        let program = find(ShellParent::Login, 100, &table, home())
            .program()
            .expect("python");
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
        assert_eq!(find(ShellParent::Login, 100, &idle, home()), Found::Waiting);
        // An unreadable record (another user's process) is skipped.
        let root = login_shell(200).with(200, 101, 200, "python3");
        assert_eq!(find(ShellParent::Login, 100, &root, home()), Found::Unknown);
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
        assert_eq!(
            find(ShellParent::Login, 100, &codex, home()),
            Found::Unknown
        );
    }

    /// A shell member at `pid` under `parent` running as `uid`; another
    /// user's record is unreadable, as root's is.
    fn shell_member(name: &str, argv: &[&str], (pid, parent): (u32, u32), uid: u32) -> Member {
        let mut shell = at(member(name, argv[0], argv, &[]), pid, parent);
        if let Some(candidate) = shell.candidate.as_mut() {
            candidate.uid = Some(uid);
            if uid != USER {
                candidate.record = None;
            }
        }
        shell
    }

    #[test]
    fn a_root_shell_comes_first_and_is_red() {
        // `su -`, or `sudo -i` without `use_pty`: the root shell holds the
        // terminal alone, its argv unreadable — its name and user say it.
        let root =
            classify(&[shell_member("sh", &["-sh"], (300, 299), 0)], home()).expect("a root shell");
        assert_eq!((root.family, root.pid), (Family::Root, 300));
        assert_eq!(root.exec, None, "no version asked");
        let bar = root.bar(None);
        assert_eq!(bar.title, "root");
        assert_eq!(bar.hint, "exit to leave");
        assert_eq!((bar.detail.as_str(), bar.path.as_str()), ("", ""));
        assert_eq!(bar.host, "", "nothing to mark");
        assert_eq!(bar.tone, ProgramTone::Error);
        assert_eq!(root.bar(Some(&Details::default())), bar);
        // Before what runs under it (a root `sh` that started python).
        let python = || {
            at(
                member("python3", "/usr/bin/python3", &["python3"], &[]),
                301,
                300,
            )
        };
        let first = classify(
            &[shell_member("sh", &["sh"], (300, 299), 0), python()],
            home(),
        );
        assert_eq!(first.map(|program| program.family), Some(Family::Root));
        // Root is a shell's: a root python with a readable record is the
        // interpreter.
        let mut root_python = python();
        if let Some(candidate) = root_python.candidate.as_mut() {
            candidate.uid = Some(0);
        }
        assert_eq!(
            classify(&[root_python], home()).map(|program| program.family),
            Some(Family::Python)
        );
        // The user's own shell is no root's.
        assert_ne!(
            classify(&[shell_member("sh", &["sh"], (300, 299), USER)], home())
                .map(|program| program.family),
            Some(Family::Root)
        );
    }

    #[test]
    fn a_nested_shell_comes_after_what_runs_in_its_group() {
        let bash = classify(&[shell_member("bash", &["bash"], (300, 101), USER)], home())
            .expect("a nested bash");
        assert_eq!((bash.family, bash.pid), (Family::Nested("bash"), 300));
        let bar = bash.bar(None);
        assert_eq!(bar.title, "nested bash");
        assert_eq!(bar.hint, "exit to return");
        assert_eq!(bar.tone, ProgramTone::Info);
        assert_eq!((bar.detail.as_str(), bar.path.as_str()), ("", ""));
        // A shell that started python leaves the bar to python: an
        // interactive `sh` without job control, python in its group.
        let python = at(
            member("python3", "/usr/bin/python3", &["python3"], &[]),
            301,
            300,
        );
        let program = classify(
            &[shell_member("sh", &["sh"], (300, 101), USER), python],
            home(),
        );
        assert_eq!(program.map(|program| program.family), Some(Family::Python));
        // A script's shell is no session — `bash build.sh` asking for a
        // key, a wrapper `sh -c` around an agent.
        for argv in [&["bash", "build.sh"][..], &["sh", "-c", "node codex.js"]] {
            let shell = shell_member(argv[0], argv, (300, 101), USER);
            assert_eq!(classify(&[shell], home()), None, "{argv:?}");
        }
        // `poetry shell` holds the terminal itself, the shell in a terminal
        // of its own: the tool is the bar.
        let poetry = at(
            member(
                "Python",
                FRAMEWORK,
                &[FRAMEWORK, "/Users/me/.local/bin/poetry", "shell"],
                &[],
            ),
            300,
            101,
        );
        let program = classify(&[poetry], home()).expect("poetry shell");
        assert_eq!(program.bar(None).title, "nested poetry shell");
        // A sibling is a pipeline's other side: no bar.
        let shells = [
            shell_member("bash", &["bash"], (300, 101), USER),
            other(301, 101),
        ];
        assert_eq!(classify(&shells, home()), None);
    }

    /// `login` 100 → `zsh` 101; the foreground group is `group`; `sudo`
    /// (pid 200, its own group 200) runs as root.
    fn sudo_table(group: u32) -> crate::jobs::tests::Table {
        login_shell(group)
            .run(200, 101, 200, &["sudo", "-i"])
            .owned_by(200, 0)
    }

    #[test]
    fn sudo_is_looked_through_to_the_command_it_runs() {
        let found = |table| find(ShellParent::Login, 100, &table, home());
        // At its prompt (raw with `pwfeedback`): nothing under it yet — no
        // answer, asked again.
        assert_eq!(found(sudo_table(200)), Found::Waiting);
        // `use_pty` (measured): sudo → its monitor → the root shell, in a
        // session of their own.
        let pty = || {
            sudo_table(200)
                .run(201, 200, 201, &["sudo", "-i"])
                .owned_by(201, 0)
        };
        let monitor_alone = pty();
        assert_eq!(
            found(monitor_alone),
            Found::Waiting,
            "the command not forked yet"
        );
        let root = pty()
            .exec(202, 201, 202, ("bash", "/bin/bash"), &["-sh"], &[])
            .owned_by(202, 0);
        let root = found(root).program().expect("the root shell");
        assert_eq!((root.family, root.pid), (Family::Root, 202));
        // `sudo make` under `use_pty` relays a raw terminal for the whole
        // build: make is the command, its recipes' root `sh -c` are make's —
        // settled unmarked.
        let make = pty()
            .run(202, 201, 202, &["make", "install"])
            .owned_by(202, 0)
            .run(203, 202, 202, &["sh", "-c", "cp a b"])
            .owned_by(203, 0);
        assert_eq!(found(make), Found::Elevated);
        // `sudo python3`: root's record does not read on macOS — unmarked.
        let python = || pty().run(202, 201, 202, &["python3"]).owned_by(202, 0);
        assert_eq!(found(python()), Found::Elevated);
        // `sudo su -`: through su too.
        let su = pty()
            .run(202, 201, 202, &["su", "-"])
            .owned_by(202, 0)
            .exec(203, 202, 203, ("zsh", "/bin/zsh"), &["-zsh"], &[])
            .owned_by(203, 0);
        let root = found(su).program().expect("su's root shell");
        assert_eq!((root.family, root.pid), (Family::Root, 203));
        // Without `use_pty` the root shell takes the terminal in a group
        // of its own (measured), sudo outside it.
        let direct = sudo_table(201)
            .exec(201, 200, 201, ("bash", "/bin/bash"), &["-bash"], &[])
            .owned_by(201, 0);
        let root = found(direct).program().expect("the root shell");
        assert_eq!((root.family, root.pid), (Family::Root, 201));
        // A user's own program named sudo elevates nothing.
        let fake = login_shell(200)
            .run(200, 101, 200, &["sudo"])
            .run(201, 200, 200, &["bash"]);
        let program = found(fake).program().expect("a nested bash");
        assert_eq!(program.family, Family::Nested("bash"));
    }

    #[test]
    fn where_root_argv_reads_a_script_is_no_root_shell() {
        // Linux: another user's `cmdline` reads. A root script, or `sh -c`,
        // that sudo keeps raw is no session; a root REPL is its bar.
        let found = |table: crate::jobs::tests::Table| {
            find(
                ShellParent::Login,
                100,
                &table.reading_others_argv(),
                home(),
            )
        };
        let under = |argv: &[&'static str]| {
            sudo_table(200)
                .run(201, 200, 201, &["sudo", "x"])
                .owned_by(201, 0)
                .exec(202, 201, 202, ("bash", "/bin/bash"), argv, &[])
                .owned_by(202, 0)
        };
        let root = found(under(&["-bash"])).program().expect("sudo -i");
        assert_eq!(root.family, Family::Root);
        assert_eq!(
            found(under(&["/bin/bash", "./install.sh"])),
            Found::Elevated
        );
        assert_eq!(
            found(under(&["bash", "-c", "make install"])),
            Found::Elevated
        );
        let python = sudo_table(200)
            .run(201, 200, 201, &["sudo", "python3"])
            .owned_by(201, 0)
            .run(202, 201, 202, &["python3"])
            .owned_by(202, 0);
        let program = found(python).program().expect("a root REPL");
        assert_eq!(program.family, Family::Python);
        assert_eq!(program.exec, None, "its executable is not read");
        // `su -c` straight in the group, too.
        let su = login_shell(201)
            .run(200, 101, 200, &["su", "-c", "x"])
            .owned_by(200, 0)
            .exec(201, 200, 201, ("sh", "/bin/sh"), &["sh", "-c", "x"], &[])
            .owned_by(201, 0);
        assert_eq!(found(su), Found::Unknown);
    }

    #[test]
    fn a_forking_tool_names_the_nested_shell_it_opened() {
        // `bash` from the prompt: its parent is the terminal's shell.
        let bash = login_shell(200).run(200, 101, 200, &["bash"]);
        let program = find(ShellParent::Login, 100, &bash, home()).program();
        assert_eq!(
            program.map(|program| program.family),
            Some(Family::Nested("bash"))
        );
        // `devbox shell` forks the shell, which takes a group of its own:
        // the tool is its parent, outside the group.
        let devbox = login_shell(201)
            .run(200, 101, 200, &["devbox", "shell"])
            .run(201, 200, 201, &["/bin/zsh", "-l"]);
        let program = find(ShellParent::Login, 100, &devbox, home()).program();
        assert_eq!(
            program.map(|program| program.bar(None).title),
            Some("nested devbox shell".to_owned())
        );
        // Another program above the shell leaves its name.
        let other = login_shell(201)
            .run(200, 101, 200, &["devbox", "run", "x"])
            .run(201, 200, 201, &["/bin/zsh", "-l"]);
        let program = find(ShellParent::Login, 100, &other, home()).program();
        assert_eq!(
            program.map(|program| program.family),
            Some(Family::Nested("zsh"))
        );
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
        // macOS looks a file over the first time it runs, before it starts —
        // measured at up to 1.7 s on a loaded machine, against the job's 2 s.
        // One run with room first, so the job's own is the second.
        let _ = run_version(&exec, Duration::from_secs(20));
        let program = Program {
            pid: 1,
            family: Family::Python,
            exec: Some(exec.clone()),
            path: exec.to_string_lossy().into_owned(),
            detail: None,
            host: None,
            venv_config: venv_config(&exec),
            kube: None,
        };
        assert_eq!(
            details(&program),
            Details {
                version: Some("3.99.1".into()),
                venv: true,
                context: None,
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
    fn a_container_session_names_where_it_runs() {
        // Measured: `docker run -it --rm redis:alpine sh` is one `docker`
        // in its own group, argv as typed.
        let table = login_shell(200).exec(
            200,
            101,
            200,
            (
                "docker",
                "/Applications/OrbStack.app/Contents/MacOS/xbin/docker",
            ),
            &["docker", "run", "-it", "--rm", "redis:alpine", "sh"],
            &[],
        );
        let program = find(ShellParent::Login, 100, &table, home())
            .program()
            .expect("docker run");
        assert_eq!(program.family, Family::Container);
        assert_eq!(program.exec, None, "no version is asked of a tool");
        let bar = program.bar(None);
        assert_eq!(bar.title, "container");
        assert_eq!(bar.path, "redis:alpine");
        assert_eq!(bar.hint, "exit to leave");
        assert_eq!(bar.host, "", "a container is no host to mark");
        assert_eq!(program.bar(Some(&details(&program))), bar);
        // Measured: `docker compose exec` is docker running its plugin; the
        // topmost one names it and the plugin is its descendant.
        let table = login_shell(200)
            .exec(
                200,
                101,
                200,
                ("docker", "/usr/local/bin/docker"),
                &["docker", "compose", "-p", "proj", "exec", "cache", "sh"],
                &[],
            )
            .exec(
                201,
                200,
                200,
                (
                    "docker-compose",
                    "/Users/me/.docker/cli-plugins/docker-compose",
                ),
                &[
                    "/Users/me/.docker/cli-plugins/docker-compose",
                    "compose",
                    "-p",
                    "proj",
                    "exec",
                    "cache",
                    "sh",
                ],
                &[],
            );
        let program = find(ShellParent::Login, 100, &table, home())
            .program()
            .expect("compose exec");
        assert_eq!(program.pid, 200);
        assert_eq!(program.bar(None).path, "cache");
        // Another command of docker's is no session: no bar.
        assert_eq!(
            one(member(
                "docker",
                "/usr/local/bin/docker",
                &["docker", "login"],
                &[]
            )),
            None
        );
    }

    #[test]
    fn a_kubernetes_session_names_its_context_whole() {
        // `--context` names it at once; the namespace follows it.
        let program = one(member(
            "kubectl",
            "/usr/local/bin/kubectl",
            &[
                "kubectl",
                "--context",
                "kubernetes-admin@kubernetes",
                "-n",
                "payments",
                "exec",
                "-it",
                "api-7f9c",
                "--",
                "sh",
            ],
            &[],
        ))
        .expect("kubectl exec");
        assert_eq!(program.family, Family::Kubernetes);
        let bar = program.bar(None);
        assert_eq!(bar.title, "k8s kubernetes-admin@kubernetes");
        assert_eq!(bar.detail, "payments");
        assert_eq!(bar.path, "pod/api-7f9c");
        assert_eq!(bar.hint, "exit to leave");
        assert_eq!(bar.host, "kubernetes-admin@kubernetes");
        assert_eq!(bar.subject, MarkSubject::Whole);
        // Without one, the title waits for the kubeconfig's line.
        let program = one(member(
            "kubectl",
            "/usr/local/bin/kubectl",
            &[
                "kubectl", "exec", "-it", "-n", "payments", "api-7f9c", "--", "sh",
            ],
            &[("KUBECONFIG", "/Users/me/.kube/prod:/Users/me/.kube/config")],
        ))
        .expect("kubectl exec");
        assert_eq!(
            program.kube.as_ref().map(|kube| kube.files.clone()),
            Some(vec![
                PathBuf::from("/Users/me/.kube/prod"),
                PathBuf::from("/Users/me/.kube/config")
            ])
        );
        let bar = program.bar(None);
        assert_eq!(bar.title, "k8s");
        assert_eq!(bar.detail, "payments", "the namespace is known already");
        assert_eq!(bar.host, "", "no context, no mark");
        assert_eq!(bar.subject, MarkSubject::Host);
        let details = Details {
            context: Some("prod-eu".into()),
            ..Details::default()
        };
        let bar = program.bar(Some(&details));
        assert_eq!(bar.title, "k8s prod-eu");
        assert_eq!(bar.host, "prod-eu");
        assert_eq!(bar.subject, MarkSubject::Whole);
    }

    #[test]
    fn the_details_job_reads_the_kubeconfigs_line_alone() {
        let dir = std::env::temp_dir().join(format!("bt-program-kube-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        let config = dir.join("config");
        std::fs::write(
            &config,
            "apiVersion: v1\nusers:\n- name: admin\n  user:\n    token: hunter2-token\n    \
             client-key-data: S0VZ\ncurrent-context: prod-eu\n",
        )
        .expect("kubeconfig");
        let program = tool(
            1,
            Family::Kubernetes,
            "pod/api".into(),
            Some(Kube {
                files: vec![config],
                ..Kube::default()
            }),
        );
        let details = details(&program);
        assert_eq!(details.context.as_deref(), Some("prod-eu"));
        let printed = format!("{details:?} {:?}", program.bar(Some(&details)));
        assert!(
            !printed.contains("hunter") && !printed.contains("S0VZ"),
            "a secret of the kubeconfig reached the bar"
        );
        // A context the command line names is not read for.
        let named = tool(
            1,
            Family::Kubernetes,
            String::new(),
            Some(Kube {
                context: Some("stage".into()),
                files: vec![dir.join("config")],
                ..Kube::default()
            }),
        );
        assert_eq!(super::details(&named).context, None);
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
            .chain(Client::ALL.map(Family::Database))
            .chain([
                Family::Container,
                Family::Kubernetes,
                Family::Root,
                Family::Nested("bash"),
            ]);
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
